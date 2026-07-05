//! Game state: player, bots, weapon, round loop, HUD.

use std::sync::Arc;

use pocket3d::bsp::MapData;
use pocket3d::input::Input;
use pocket3d::prelude::*;
use pocket3d::winit::event::MouseButton;
use pocket3d::winit::keyboard::KeyCode;

use crate::bot::Bot;
use crate::weapon::{EffectKind, Effects, MUZZLE_LOCAL, RANGE, Rng, Weapon, build_rifle};

pub const MOUSE_SENS: f32 = 0.002;
pub const WALK_SPEED_SCALE: f32 = 0.52;
const BOT_HALF: Vec3 = Vec3::new(16.0, 36.0, 16.0);

pub struct Player {
    pub state: CharacterState,
    pub prev_pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub params: MoveParams,
    pub health: i32,
    pub alive: bool,
}

impl Player {
    pub fn spawn(pos: Vec3, yaw: f32) -> Self {
        Self {
            state: CharacterState::new(pos),
            prev_pos: pos,
            yaw,
            pitch: 0.0,
            params: MoveParams::default(),
            health: 100,
            alive: true,
        }
    }

    pub fn eye_interpolated(&self, alpha: f32) -> Vec3 {
        self.prev_pos.lerp(self.state.pos, alpha) + Vec3::Y * self.params.eye_height
    }

    pub fn eye(&self) -> Vec3 {
        self.state.pos + Vec3::Y * self.params.eye_height
    }

    pub fn forward_flat(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        Vec3::new(-sy, 0.0, -cy)
    }

    pub fn right(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        Vec3::new(cy, 0.0, -sy)
    }

    pub fn view_dir(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        Vec3::new(-sy * cp, sp, -cy * cp)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    /// Round countdown; movement frozen. The countdown itself lives in the
    /// gameplay mod (JS) — Rust only knows the gate is closed.
    Starting,
    Live,
    Ended { won: bool },
}

/// Facts the core reports to the guest, batched per tick (RUNTIMES.md Law 2:
/// facts cross as events).
#[derive(Clone, Debug)]
pub enum GameEvent {
    /// The player's shot connected.
    Hit { bot: usize, headshot: bool, damage: i32, fatal: bool },
    PlayerDamaged { amount: i32, hp: i32 },
    PlayerDied,
    RoundReset,
}

/// Intent the guest sends back through the `strike` surface ops. Queued
/// during the guest turn, applied by the host afterwards.
#[derive(Clone, Debug)]
pub enum Command {
    SetPhase(Phase),
    ResetRound,
    AddWin,
    AddLoss,
    SetBotCount(usize),
    ConfigureWeapon(crate::weapon::WeaponConfig),
    ConfigureBots(crate::bot::BotConfig),
}

#[derive(Default, Clone, Copy)]
pub struct Score {
    pub wins: u32,
    pub losses: u32,
}

pub struct OpenStrike {
    pub map: MapData,
    pub player: Player,
    pub scene: Scene,
    pub camera: Camera,
    pub hud: Hud,
    pub fly_mode: bool,
    pub time: f32,
    pub debug_overlay: bool,

    pub bot_asset: Option<Arc<ModelAsset>>,
    /// Clip index of the bot model's walk cycle (see load in `upload`).
    pub(crate) bot_walk_clip: usize,
    pub rifle_asset: Option<Arc<ModelAsset>>,
    pub bots: Vec<Bot>,
    pub bot_count: usize,
    pub weapon: Weapon,
    pub effects: Effects,
    pub rng: Rng,
    pub phase: Phase,
    pub score: Score,
    pub bot_cfg: crate::bot::BotConfig,
    /// Per-tick event batch drained by the guest turn.
    pub events: Vec<GameEvent>,
    /// Exit the app after this many seconds (smoke tests).
    pub auto_quit: Option<f32>,

    spawn_point: (Vec3, f32),
    bob_time: f32,
    prev_bob_time: f32,
    pub fired_this_tick: bool,
}

impl OpenStrike {
    pub fn new(map: MapData, spawn_pos: Vec3, spawn_yaw: f32, bot_count: usize) -> Self {
        let mut scene = Scene::default();
        if let Some(sun) = map.sun {
            scene.sky.sun_dir = sun.dir;
            scene.lighting.sun_dir = sun.dir;
            scene.lighting.sun_color = sun.color * 0.9;
        }
        let camera = Camera {
            fov_y: 74f32.to_radians(),
            ..Default::default()
        };
        let mut game = Self {
            map,
            player: Player::spawn(spawn_pos, spawn_yaw),
            scene,
            camera,
            hud: Hud::default(),
            fly_mode: false,
            time: 0.0,
            debug_overlay: false,
            bot_asset: None,
            bot_walk_clip: 0,
            rifle_asset: None,
            bots: Vec::new(),
            bot_count,
            weapon: Weapon::default(),
            effects: Effects::default(),
            rng: Rng(0x0DDB1A5E5BAD5EED),
            phase: Phase::Starting,
            score: Score::default(),
            bot_cfg: crate::bot::BotConfig::default(),
            events: Vec::new(),
            auto_quit: None,
            spawn_point: (spawn_pos, spawn_yaw),
            bob_time: 0.0,
            prev_bob_time: 0.0,
            fired_this_tick: false,
        };
        game.spawn_bots();
        game
    }

    /// Upload GPU resources (called from `Game::init` or headless setup).
    pub fn upload_world(&mut self, gpu: &Gpu, renderer: &Renderer) {
        let world = Arc::new(WorldModel::from_bsp(
            gpu,
            &renderer.world_material_layout,
            &renderer.samplers,
            &self.map,
        ));
        self.scene.world = Some(world);
        self.rifle_asset = Some(build_rifle(gpu, renderer));

        match crate::args::find_asset("models/Soldier.glb") {
            Some(path) => {
                match ModelAsset::load_glb(
                    gpu,
                    &renderer.model_material_layout,
                    &renderer.samplers,
                    &path,
                ) {
                    Ok(asset) => {
                        // Bots move on their Walk cycle; fall back to clip 0
                        // for single-clip models.
                        self.bot_walk_clip = asset
                            .clips
                            .iter()
                            .position(|c| c.name.eq_ignore_ascii_case("walk"))
                            .unwrap_or(0);
                        self.bot_asset = Some(asset);
                        for bot in &mut self.bots {
                            bot.anim.clip = self.bot_walk_clip;
                        }
                    }
                    Err(e) => log::warn!("bot model failed to load: {e:#}"),
                }
            }
            None => {
                log::warn!("bot model not found (models/Soldier.glb); bots render as nothing")
            }
        }
    }

    fn spawn_bots(&mut self) {
        self.bots.clear();
        let spawns = if self.map.t_spawns.is_empty() {
            &self.map.ct_spawns
        } else {
            &self.map.t_spawns
        };
        if spawns.is_empty() {
            return;
        }
        for i in 0..self.bot_count {
            // Spread bots over the spawn list.
            let sp = spawns[(i * 3 + 1) % spawns.len()];
            let mut bot = Bot::spawn(sp.pos, sp.yaw);
            bot.anim.clip = self.bot_walk_clip;
            self.bots.push(bot);
        }
    }

    pub fn reset_round(&mut self) {
        let (pos, yaw) = self.spawn_point;
        let pitch = self.player.pitch;
        self.player = Player::spawn(pos, yaw);
        self.player.pitch = pitch * 0.25;
        self.weapon.reset();
        self.effects.clear();
        self.spawn_bots();
        self.phase = Phase::Starting;
        self.events.push(GameEvent::RoundReset);
    }

    /// Apply one guest command (drained after each guest turn).
    pub fn apply(&mut self, cmd: Command) {
        match cmd {
            Command::SetPhase(p) => self.phase = p,
            Command::ResetRound => self.reset_round(),
            Command::AddWin => self.score.wins += 1,
            Command::AddLoss => self.score.losses += 1,
            Command::SetBotCount(n) => self.bot_count = n.min(16),
            Command::ConfigureWeapon(cfg) => {
                self.weapon.cfg = cfg;
                let mag = self.weapon.cfg.mag_size;
                self.weapon.ammo = self.weapon.ammo.min(mag);
            }
            Command::ConfigureBots(cfg) => {
                self.bot_count = cfg.count.min(16);
                self.bot_cfg = cfg;
            }
        }
    }

    pub fn apply_look(&mut self, dx: f32, dy: f32) {
        self.player.yaw -= dx * MOUSE_SENS;
        self.player.pitch =
            (self.player.pitch - dy * MOUSE_SENS).clamp(-89f32.to_radians(), 89f32.to_radians());
    }

    pub fn alive_bots(&self) -> usize {
        self.bots.iter().filter(|b| b.alive()).count()
    }

    /// Full fixed-step game tick.
    pub fn tick(&mut self, dt: f32, input: &Input) {
        self.time += dt;
        self.effects.tick(dt);
        self.weapon.tick(dt);
        if input.key_pressed(KeyCode::KeyV) {
            self.fly_mode = !self.fly_mode;
        }

        // Round phase is a gate here; countdowns and transitions live in the
        // gameplay mod (JS), which drives them through `strike` commands.
        let movement_frozen = self.phase == Phase::Starting;

        self.tick_player_movement(dt, input, movement_frozen);

        // Combat.
        self.fired_this_tick = false;
        let live = self.phase == Phase::Live;
        if live && self.player.alive && !movement_frozen {
            if input.key_pressed(KeyCode::KeyR) {
                self.weapon.trigger_reload();
            }
            if input.mouse_button_down(MouseButton::Left) && self.weapon.fire() {
                self.fire_shot();
            }
        }

        // Bots.
        let player_eye = self.player.eye();
        let player_alive = self.player.alive;
        let mut incoming = 0i32;
        let bot_cfg = self.bot_cfg.clone();
        for bot in &mut self.bots {
            let shot = bot.tick(
                &self.map.collision,
                player_eye,
                player_alive && live,
                dt,
                &bot_cfg,
                &mut self.rng,
                &mut self.effects,
            );
            if live && let Some(s) = shot {
                incoming += s.damage;
            }
        }
        if incoming > 0 && self.player.alive {
            self.player.health -= incoming;
            if self.player.health <= 0 {
                self.player.health = 0;
                self.player.alive = false;
            }
            self.events.push(GameEvent::PlayerDamaged {
                amount: incoming,
                hp: self.player.health,
            });
            if !self.player.alive {
                self.events.push(GameEvent::PlayerDied);
            }
        }

        // Soft push-out so bots don't share space with the player.
        if self.player.alive {
            for bot in self.bots.iter().filter(|b| b.alive()) {
                let d = self.player.state.pos - bot.state.pos;
                let horiz = Vec3::new(d.x, 0.0, d.z);
                let dist = horiz.length();
                if dist < 34.0 && d.y.abs() < 72.0 && dist > 0.001 {
                    self.player.state.pos += horiz / dist * (34.0 - dist) * 0.35;
                }
            }
        }

    }

    fn tick_player_movement(&mut self, dt: f32, input: &Input, frozen: bool) {
        let p = &mut self.player;
        p.prev_pos = p.state.pos;
        if !p.alive {
            return;
        }

        let mut wish = Vec3::ZERO;
        if !frozen {
            if input.key_down(KeyCode::KeyW) {
                wish += p.forward_flat();
            }
            if input.key_down(KeyCode::KeyS) {
                wish -= p.forward_flat();
            }
            if input.key_down(KeyCode::KeyD) {
                wish += p.right();
            }
            if input.key_down(KeyCode::KeyA) {
                wish -= p.right();
            }
        }

        if self.fly_mode {
            let mut v = Vec3::ZERO;
            if input.key_down(KeyCode::KeyW) {
                v += p.view_dir() * 600.0;
            }
            if input.key_down(KeyCode::KeyS) {
                v -= p.view_dir() * 600.0;
            }
            if input.key_down(KeyCode::KeyD) {
                v += p.right() * 600.0;
            }
            if input.key_down(KeyCode::KeyA) {
                v -= p.right() * 600.0;
            }
            if input.key_down(KeyCode::Space) {
                v += Vec3::Y * 400.0;
            }
            p.state.pos += v * dt;
            p.state.vel = Vec3::ZERO;
            return;
        }

        let minput = MoveInput {
            wish_dir: wish,
            speed: if input.key_down(KeyCode::ShiftLeft) {
                WALK_SPEED_SCALE
            } else {
                1.0
            },
            jump: !frozen && input.key_down(KeyCode::Space),
        };
        step_character(
            &self.map.collision,
            HullKind::Stand,
            &mut p.state,
            &p.params,
            &minput,
            dt,
        );

        // Weapon bob clock follows ground speed. Keep the previous value so
        // the viewmodel can interpolate between ticks like the camera does.
        self.prev_bob_time = self.bob_time;
        let speed = (p.state.vel.x * p.state.vel.x + p.state.vel.z * p.state.vel.z).sqrt();
        if p.state.on_ground {
            self.bob_time += dt * (speed / 250.0) * 11.0;
        }
    }

    fn fire_shot(&mut self) {
        self.fired_this_tick = true;
        let p = &self.player;
        let eye = p.eye();
        let dir = p.view_dir();
        let right = p.right();
        let up = right.cross(dir).normalize_or_zero();

        // Spread: base + recoil + movement penalty.
        let speed = (p.state.vel.x * p.state.vel.x + p.state.vel.z * p.state.vel.z).sqrt();
        let spread = 0.006
            + self.weapon.recoil * 0.014
            + (speed / 250.0) * 0.02
            + if p.state.on_ground { 0.0 } else { 0.03 };
        let dir = (dir + right * self.rng.signed() * spread + up * self.rng.signed() * spread)
            .normalize();

        // World hit.
        let wt = self
            .map
            .collision
            .trace(pocket3d::bsp::Hull::Point, eye, eye + dir * RANGE);
        let mut best_t = wt.fraction * RANGE;
        let mut hit_bot: Option<usize> = None;
        for (i, bot) in self.bots.iter().enumerate() {
            if !bot.alive() {
                continue;
            }
            let c = bot.state.pos;
            if let Some(t) = ray_aabb(eye, dir, c - BOT_HALF, c + BOT_HALF)
                && t < best_t
            {
                best_t = t;
                hit_bot = Some(i);
            }
        }
        let hit_point = eye + dir * best_t;

        // Effects: muzzle flash + tracer + impact.
        let muzzle = self.viewmodel_transform().transform_point3(MUZZLE_LOCAL);
        self.effects
            .spawn(EffectKind::MuzzleFlash { pos: muzzle }, 0.06);
        self.effects.spawn(
            EffectKind::Tracer {
                a: muzzle,
                b: hit_point,
            },
            0.07,
        );

        if let Some(i) = hit_bot {
            let bot = &mut self.bots[i];
            let headshot = hit_point.y > bot.state.pos.y + 22.0;
            let dmg = if headshot {
                self.weapon.cfg.damage_head
            } else {
                self.weapon.cfg.damage_body
            };
            let died = bot.hurt(dmg);
            self.effects
                .spawn(EffectKind::BloodPuff { pos: hit_point }, 0.22);
            self.events.push(GameEvent::Hit { bot: i, headshot, damage: dmg, fatal: died });
        } else if wt.fraction < 1.0 {
            self.effects
                .spawn(EffectKind::Impact { pos: hit_point }, 0.16);
        }

        // Camera kick.
        self.player.pitch = (self.player.pitch + 0.0045).min(89f32.to_radians());
    }

    /// Viewmodel placement for gameplay (muzzle position at the tick state).
    fn viewmodel_transform(&self) -> Mat4 {
        self.viewmodel_transform_at(1.0)
    }

    /// Viewmodel placement: camera-anchored with bob, recoil, and reload dip.
    ///
    /// `alpha` is the render interpolation factor. The gun MUST ride the same
    /// interpolated eye as the camera — anchoring it to the raw tick position
    /// makes it jitter against the world at the tick rate whenever the player
    /// moves. Bob and recoil interpolate for the same reason.
    pub(crate) fn viewmodel_transform_at(&self, alpha: f32) -> Mat4 {
        let p = &self.player;
        let eye = p.eye_interpolated(alpha);
        let speed = (p.state.vel.x * p.state.vel.x + p.state.vel.z * p.state.vel.z).sqrt();
        let bob_amp = (speed / 250.0).min(1.0) * if p.state.on_ground { 1.0 } else { 0.2 };
        let bob_t = self.prev_bob_time + (self.bob_time - self.prev_bob_time) * alpha;
        let bob_y = (bob_t * 2.0).sin() * 0.55 * bob_amp;
        let bob_x = bob_t.cos() * 0.4 * bob_amp;

        let recoil =
            self.weapon.prev_recoil + (self.weapon.recoil - self.weapon.prev_recoil) * alpha;
        let reload = if self.weapon.reloading() {
            let f = 1.0
                - (self.weapon.reload_left / self.weapon.cfg.reload_time).clamp(0.0, 1.0);
            (f * std::f32::consts::PI).sin()
        } else {
            0.0
        };

        Mat4::from_translation(eye)
            * Mat4::from_rotation_y(p.yaw)
            * Mat4::from_rotation_x(p.pitch)
            * Mat4::from_translation(Vec3::new(
                7.2 + bob_x,
                -7.0 + bob_y - reload * 4.5,
                -8.5 + recoil * 2.8,
            ))
            * Mat4::from_rotation_x(recoil * 0.10 - reload * 0.55)
            * Mat4::from_rotation_y(-0.03)
    }

    /// Build camera, scene models/effects, and HUD for the current frame.
    pub fn compose_base(&mut self, alpha: f32, time: f32, screen: (f32, f32)) {
        self.scene.time = time;
        self.camera.pos = self.player.eye_interpolated(alpha);
        self.camera.yaw = self.player.yaw;
        self.camera.pitch = self.player.pitch;

        // Bots.
        self.scene.models.clear();
        if let Some(asset) = &self.bot_asset {
            for bot in &self.bots {
                let mut inst = ModelInstance::new(asset.clone());
                inst.transform = bot.transform(asset);
                inst.anim = bot.anim;
                inst.tint = bot.tint();
                self.scene.models.push(inst);
            }
        }

        // Viewmodel (interpolated with the camera — see viewmodel_transform_at).
        self.scene.viewmodel = match (&self.rifle_asset, self.player.alive) {
            (Some(rifle), true) => {
                let mut vm = ModelInstance::new(rifle.clone());
                vm.transform = self.viewmodel_transform_at(alpha);
                vm.lit = 1.0;
                Some(vm)
            }
            _ => None,
        };

        // Effects.
        self.scene.sprites.clear();
        self.scene.beams.clear();
        self.effects
            .emit(&mut self.scene.sprites, &mut self.scene.beams);

        self.compose_hud(screen);
    }

    fn compose_hud(&mut self, screen: (f32, f32)) {
        let _ = screen;
        let hud = &mut self.hud;
        hud.clear();
        // The player-facing HUD is a PocketJS app composited by the host
        // (game/hud.tsx via pocket-ui-wgpu); the bitmap HUD only carries the
        // F3 debug overlay now.
        if self.debug_overlay {
            let p = &self.player.state;
            let phase = format!("{:?}", self.phase);
            hud.text(
                8.0,
                8.0,
                2.0,
                [1.0, 1.0, 1.0, 0.8],
                &format!(
                    "POS {:6.0} {:6.0} {:6.0}  VEL {:5.0}  {}  {}",
                    p.pos.x,
                    p.pos.y,
                    p.pos.z,
                    (p.vel.x * p.vel.x + p.vel.z * p.vel.z).sqrt(),
                    if p.on_ground { "GND" } else { "AIR" },
                    phase,
                ),
            );
        }
    }

}

/// Slab-method ray/AABB intersection; returns distance along `dir`.
fn ray_aabb(origin: Vec3, dir: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let inv = dir.recip();
    let t0 = (min - origin) * inv;
    let t1 = (max - origin) * inv;
    let tmin = t0.min(t1);
    let tmax = t0.max(t1);
    let enter = tmin.x.max(tmin.y).max(tmin.z);
    let exit = tmax.x.min(tmax.y).min(tmax.z);
    if enter <= exit && exit >= 0.0 {
        Some(enter.max(0.0))
    } else {
        None
    }
}
