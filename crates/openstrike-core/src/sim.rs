//! The round simulation: player, bots, combat, phase gate, and the strike
//! surface's command/event/state types.

use alloc::vec::Vec;

use glam::{Mat4, Vec3};
use pocket3d_bsp::collide::{CharacterState, HullKind, MoveInput, MoveParams, step_character};
use pocket3d_bsp::trace::{Hull, MapCollision};
use pocket3d_bsp::types::SpawnPoint;

use crate::bot::{Bot, BotConfig};
use crate::weapon::{EffectKind, Effects, RANGE, Rng, Weapon, WeaponConfig};
use crate::{sin_cos, sinf, sqrtf};

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
        let (sy, cy) = sin_cos(self.yaw);
        Vec3::new(-sy, 0.0, -cy)
    }

    pub fn right(&self) -> Vec3 {
        let (sy, cy) = sin_cos(self.yaw);
        Vec3::new(cy, 0.0, -sy)
    }

    pub fn view_dir(&self) -> Vec3 {
        let (sy, cy) = sin_cos(self.yaw);
        let (sp, cp) = sin_cos(self.pitch);
        Vec3::new(-sy * cp, sp, -cy * cp)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    /// Round countdown; movement frozen. The countdown itself lives in the
    /// gameplay mod (JS) — Rust only knows the gate is closed.
    Starting,
    Live,
    Ended {
        won: bool,
    },
}

/// Facts the core reports to the guest, batched per tick (RUNTIMES.md Law 2:
/// facts cross as events).
#[derive(Clone, Debug)]
pub enum GameEvent {
    /// The player's shot connected.
    Hit {
        bot: usize,
        headshot: bool,
        damage: i32,
        fatal: bool,
    },
    PlayerDamaged {
        amount: i32,
        hp: i32,
    },
    PlayerDied,
    RoundReset,
}

/// Intent the guest sends back through the `strike` surface ops. Queued
/// during the guest turn, applied by the host afterwards.
#[derive(Clone, Debug)]
pub enum Command {
    NetworkReply(alloc::string::String),
    SetPhase(Phase),
    ResetRound,
    AddWin,
    AddLoss,
    SetBotCount(usize),
    ConfigureWeapon(WeaponConfig),
    ConfigureBots(BotConfig),
}

/// Retain only the latest initialization settings, in command order. Menu
/// visits must not grow a replay log or retain score/round commands.
pub fn retain_configuration(pending: &mut Vec<Command>, command: Command) {
    if !matches!(
        command,
        Command::ConfigureWeapon(_) | Command::ConfigureBots(_) | Command::SetBotCount(_)
    ) {
        return;
    }
    pending.retain(|old| core::mem::discriminant(old) != core::mem::discriminant(&command));
    pending.push(command);
}

#[derive(Default, Clone, Copy)]
pub struct Score {
    pub wins: u32,
    pub losses: u32,
}

/// One tick of player intent, already mapped from whatever the platform's
/// input device is (keyboard+mouse or PSP pad).
#[derive(Clone, Copy, Debug, Default)]
pub struct SimInput {
    /// Strafe axis, -1..1 (right positive).
    pub move_x: f32,
    /// Forward axis, -1..1 (forward positive).
    pub move_y: f32,
    pub walk: bool,
    pub jump: bool,
    /// Trigger held.
    pub fire: bool,
    /// Reload requested this tick; platforms may map either an edge or a held
    /// level because the weapon gate makes repeated requests idempotent.
    pub reload: bool,
}

/// The platform-free simulation. Platforms own presentation and input; this
/// owns state and time.
pub struct StrikeSim {
    pub network: Option<crate::net::Client>,
    pub player: Player,
    pub bots: Vec<Bot>,
    pub bot_count: usize,
    pub weapon: Weapon,
    pub effects: Effects,
    pub presentation: crate::presentation::Presentation,
    pub projectile_config: Option<crate::projectile::Config>,
    pub projectiles: crate::projectile::Projectiles,
    pub rng: Rng,
    pub phase: Phase,
    pub score: Score,
    pub bot_cfg: BotConfig,
    /// Per-tick event batch drained by the guest turn.
    pub events: Vec<GameEvent>,
    pub time: f32,
    pub fly_mode: bool,
    pub fired_this_tick: bool,

    spawn_point: (Vec3, f32),
    bot_spawns: Vec<SpawnPoint>,
    bob_time: f32,
    prev_bob_time: f32,
}

impl StrikeSim {
    /// `bot_spawns` is the enemy spawn list (T side, falling back to CT on
    /// maps without one — the caller picks).
    pub fn new(
        spawn_pos: Vec3,
        spawn_yaw: f32,
        bot_spawns: Vec<SpawnPoint>,
        bot_count: usize,
    ) -> Self {
        let mut sim = Self {
            network: None,
            player: Player::spawn(spawn_pos, spawn_yaw),
            bots: Vec::new(),
            bot_count,
            weapon: Weapon::default(),
            effects: Effects::default(),
            presentation: crate::presentation::Presentation::default(),
            projectile_config: None,
            projectiles: crate::projectile::Projectiles::default(),
            rng: Rng(0x0DDB1A5E5BAD5EED),
            phase: Phase::Starting,
            score: Score::default(),
            bot_cfg: BotConfig::default(),
            events: Vec::new(),
            time: 0.0,
            fly_mode: false,
            fired_this_tick: false,
            spawn_point: (spawn_pos, spawn_yaw),
            bot_spawns,
            bob_time: 0.0,
            prev_bob_time: 0.0,
        };
        sim.spawn_bots(0);
        sim
    }

    /// (Re)spawn bots; `walk_clip` is the platform's walk-cycle clip index.
    pub fn spawn_bots(&mut self, walk_clip: usize) {
        self.bots.clear();
        if self.bot_spawns.is_empty() {
            return;
        }
        for i in 0..self.bot_count {
            // Spread bots over the spawn list.
            let sp = self.bot_spawns[(i * 3 + 1) % self.bot_spawns.len()];
            let mut bot = Bot::spawn(sp.pos, sp.yaw);
            bot.anim.clip = walk_clip;
            self.bots.push(bot);
        }
    }

    pub fn reset_round(&mut self, walk_clip: usize) {
        let (pos, yaw) = self.spawn_point;
        let pitch = self.player.pitch;
        self.player = Player::spawn(pos, yaw);
        self.player.pitch = pitch * 0.25;
        self.weapon.reset();
        self.effects.clear();
        self.projectiles.list.clear();
        self.spawn_bots(walk_clip);
        self.phase = Phase::Starting;
        self.events.push(GameEvent::RoundReset);
    }

    /// Apply one guest command (drained after each guest turn).
    pub fn apply(&mut self, cmd: Command, walk_clip: usize) {
        if let Command::NetworkReply(raw) = &cmd {
            if let Some(network) = &mut self.network {
                if raw.is_empty() {
                    network.fail();
                } else {
                    network.receive(raw);
                }
            }
            return;
        }
        if self.network.is_some() {
            return;
        }
        match cmd {
            Command::NetworkReply(_) => {}
            Command::SetPhase(p) => self.phase = p,
            Command::ResetRound => self.reset_round(walk_clip),
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

    pub fn toggle_fly(&mut self) {
        self.fly_mode = !self.fly_mode;
    }

    pub fn alive_bots(&self) -> usize {
        self.bots.iter().filter(|b| b.alive()).count()
    }

    pub fn reload_frac(&self) -> f32 {
        if self.weapon.reloading() {
            1.0 - (self.weapon.reload_left / self.weapon.cfg.reload_time).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    pub fn ground_speed(&self) -> f32 {
        let v = self.player.state.vel;
        sqrtf(v.x * v.x + v.z * v.z)
    }

    /// Full fixed-step game tick.
    pub fn tick(&mut self, col: &MapCollision, dt: f32, input: &SimInput) {
        if let Some(mut network) = self.network.take() {
            network.tick(self, col, input);
            self.network = Some(network);
            return;
        }
        self.tick_world(col, dt, input, true, true);
    }

    pub(crate) fn tick_world(
        &mut self,
        col: &MapCollision,
        dt: f32,
        input: &SimInput,
        ai: bool,
        damage: bool,
    ) {
        self.time += dt;
        self.effects.tick(dt);
        let was_reloading = self.weapon.reloading();
        self.weapon.tick(dt);
        if was_reloading
            && !self.weapon.reloading()
            && self.presentation.motion == crate::presentation::ViewMotion::Staff
        {
            self.effects.spawn_viewmodel_muzzle(
                self.viewmodel_transform_at(1.0)
                    .transform_point3(self.presentation.muzzle),
                0.24,
            );
        }

        // Round phase is a gate here; countdowns and transitions live in the
        // gameplay mod (JS), which drives them through `strike` commands.
        let movement_frozen = self.phase == Phase::Starting;

        self.tick_player_movement(col, dt, input, movement_frozen);

        // Combat.
        self.fired_this_tick = false;
        let live = self.phase == Phase::Live;
        if live && self.player.alive && !movement_frozen {
            if input.reload {
                self.weapon.trigger_reload();
            }
            if input.fire
                && (self.projectile_config.is_none() || self.projectiles.available())
                && self.weapon.fire()
            {
                self.fire_shot(col, damage);
            }
        }

        // Bots.
        let player_eye = self.player.eye();
        let player_alive = self.player.alive;
        let mut incoming = 0i32;
        let bot_cfg = self.bot_cfg.clone();
        for bot in self.bots.iter_mut().filter(|_| ai) {
            let shot = bot.tick(
                col,
                player_eye,
                player_alive && live,
                dt,
                &bot_cfg,
                &mut self.rng,
            );
            if live {
                if let Some(s) = shot {
                    if let Some(config) = self.projectile_config {
                        self.projectiles.launch(
                            s.from,
                            s.target,
                            crate::projectile::Owner::Bot,
                            config,
                            s.damage,
                            s.damage,
                        );
                    } else {
                        self.effects
                            .spawn(EffectKind::MuzzleFlash { pos: s.from }, 0.08);
                        self.effects.spawn(
                            EffectKind::Tracer {
                                a: s.from,
                                b: s.target,
                            },
                            0.09,
                        );
                        if s.hitscan_hit {
                            incoming += s.damage;
                        }
                    }
                }
            }
        }
        if live {
            incoming += self.tick_projectiles(col, dt);
        } else {
            self.projectiles.list.clear();
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
        if ai && self.player.alive {
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

    pub(crate) fn tick_player_movement(
        &mut self,
        col: &MapCollision,
        dt: f32,
        input: &SimInput,
        frozen: bool,
    ) {
        let p = &mut self.player;
        p.prev_pos = p.state.pos;
        if !p.alive {
            return;
        }

        let mut wish = Vec3::ZERO;
        if !frozen {
            wish += p.forward_flat() * input.move_y;
            wish += p.right() * input.move_x;
        }

        if self.fly_mode {
            let mut v = Vec3::ZERO;
            v += p.view_dir() * 600.0 * input.move_y;
            v += p.right() * 600.0 * input.move_x;
            if input.jump {
                v += Vec3::Y * 400.0;
            }
            p.state.pos += v * dt;
            p.state.vel = Vec3::ZERO;
            return;
        }

        let minput = MoveInput {
            wish_dir: wish,
            speed: if input.walk { WALK_SPEED_SCALE } else { 1.0 },
            jump: !frozen && input.jump,
        };
        step_character(col, HullKind::Stand, &mut p.state, &p.params, &minput, dt);

        // Weapon bob clock follows ground speed. Keep the previous value so
        // the viewmodel can interpolate between ticks like the camera does.
        self.prev_bob_time = self.bob_time;
        let speed = sqrtf(p.state.vel.x * p.state.vel.x + p.state.vel.z * p.state.vel.z);
        if p.state.on_ground {
            self.bob_time += dt * (speed / 250.0) * 11.0;
        }
    }

    fn fire_shot(&mut self, col: &MapCollision, damage: bool) {
        self.fired_this_tick = true;
        let p = &self.player;
        let eye = p.eye();
        let dir = p.view_dir();
        let right = p.right();
        let up = right.cross(dir).normalize_or_zero();

        // Spread: base + recoil + movement penalty.
        let speed = sqrtf(p.state.vel.x * p.state.vel.x + p.state.vel.z * p.state.vel.z);
        let spread = 0.006
            + self.weapon.recoil * 0.014
            + (speed / 250.0) * 0.02
            + if p.state.on_ground { 0.0 } else { 0.03 };
        let dir = (dir + right * self.rng.signed() * spread + up * self.rng.signed() * spread)
            .normalize();

        // World hit.
        let wt = col.trace(Hull::Point, eye, eye + dir * RANGE);
        let mut best_t = wt.fraction * RANGE;
        let mut hit_bot: Option<usize> = None;
        for (i, bot) in self.bots.iter().enumerate() {
            if !bot.alive() {
                continue;
            }
            let c = bot.state.pos;
            if let Some(t) = ray_aabb(eye, dir, c - BOT_HALF, c + BOT_HALF) {
                if t < best_t {
                    best_t = t;
                    hit_bot = Some(i);
                }
            }
        }
        let hit_point = eye + dir * best_t;

        // Effects: muzzle flash + tracer + impact.
        let muzzle = self
            .viewmodel_pose(1.0, false)
            .transform_point3(self.presentation.muzzle);
        if let Some(config) = self.projectile_config {
            // Keep the release point on this side of cover even when the
            // first-person mesh itself uses the independent viewmodel depth.
            let release = col.trace(Hull::Point, eye, muzzle);
            if release.start_solid || release.fraction < 1.0 {
                self.effects
                    .spawn(EffectKind::Impact { pos: release.end }, 0.22);
                return;
            }
            self.projectiles.launch(
                release.end,
                hit_point,
                crate::projectile::Owner::Player,
                config,
                self.weapon.cfg.damage_body,
                self.weapon.cfg.damage_head,
            );
            return;
        }
        let beam = self.presentation.shot == crate::presentation::ShotStyle::Beam;
        self.effects
            .spawn_viewmodel_muzzle(muzzle, if beam { 0.18 } else { 0.06 });
        self.effects.spawn(
            EffectKind::Tracer {
                a: muzzle,
                b: hit_point,
            },
            if beam { 0.18 } else { 0.07 },
        );
        self.effects.list.last_mut().unwrap().viewmodel = true;

        if let Some(i) = hit_bot.filter(|_| damage) {
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
            self.events.push(GameEvent::Hit {
                bot: i,
                headshot,
                damage: dmg,
                fatal: died,
            });
        } else if wt.fraction < 1.0 {
            self.effects
                .spawn(EffectKind::Impact { pos: hit_point }, 0.16);
        }

        // Camera kick.
        self.player.pitch = (self.player.pitch + 0.0045).min(89f32.to_radians());
    }

    /// Sweep each flight segment against the map and living targets. World
    /// cover wins equal-distance hits; a shot is removed after one contact.
    fn tick_projectiles(&mut self, col: &MapCollision, dt: f32) -> i32 {
        use crate::projectile::{Owner, segment_box};
        let mut incoming = 0;
        let mut i = 0;
        while i < self.projectiles.list.len() {
            let p = &mut self.projectiles.list[i];
            p.advance(dt.min((p.config.lifetime - p.age).max(0.0)));
            let world = col.trace(Hull::Point, p.previous, p.position);
            let mut fraction = if world.start_solid {
                0.0
            } else {
                world.fraction
            };
            let mut target = None;
            let half = BOT_HALF + Vec3::splat(p.config.radius);
            if p.owner == Owner::Player {
                for (index, bot) in self.bots.iter().enumerate().filter(|(_, b)| b.alive()) {
                    if let Some(t) = segment_box(
                        p.previous,
                        p.position,
                        bot.state.pos - half,
                        bot.state.pos + half,
                    ) {
                        if t < fraction {
                            fraction = t;
                            target = Some(index);
                        }
                    }
                }
            } else if self.player.alive {
                if let Some(t) = segment_box(
                    p.previous,
                    p.position,
                    self.player.state.pos - half,
                    self.player.state.pos + half,
                ) {
                    if t < fraction {
                        fraction = t;
                        target = Some(usize::MAX);
                    }
                }
            }
            if fraction < 1.0 || world.start_solid {
                let point = p.previous.lerp(p.position, fraction);
                self.effects.spawn(EffectKind::Impact { pos: point }, 0.22);
                if let Some(index) = target {
                    if index == usize::MAX {
                        incoming += p.damage_body;
                    } else {
                        let bot = &mut self.bots[index];
                        let headshot = point.y > bot.state.pos.y + 22.0;
                        let damage = if headshot {
                            p.damage_head
                        } else {
                            p.damage_body
                        };
                        let fatal = bot.hurt(damage);
                        self.events.push(GameEvent::Hit {
                            bot: index,
                            headshot,
                            damage,
                            fatal,
                        });
                    }
                }
                self.projectiles.list.swap_remove(i);
            } else if p.age >= p.config.lifetime {
                self.projectiles.list.swap_remove(i);
            } else {
                i += 1;
            }
        }
        incoming
    }

    /// Viewmodel placement: camera-anchored with bob, recoil, and reload dip.
    ///
    /// `alpha` is the render interpolation factor. The gun MUST ride the same
    /// interpolated eye as the camera — anchoring it to the raw tick position
    /// makes it jitter against the world at the tick rate whenever the player
    /// moves. Bob and recoil interpolate for the same reason.
    pub fn viewmodel_transform_at(&self, alpha: f32) -> Mat4 {
        self.viewmodel_pose(alpha, true)
    }
    fn viewmodel_pose(&self, alpha: f32, animate_throw: bool) -> Mat4 {
        let p = &self.player;
        let eye = p.eye_interpolated(alpha);
        let speed = sqrtf(p.state.vel.x * p.state.vel.x + p.state.vel.z * p.state.vel.z);
        let bob_amp = (speed / 250.0).min(1.0) * if p.state.on_ground { 1.0 } else { 0.2 };
        let bob_t = self.prev_bob_time + (self.bob_time - self.prev_bob_time) * alpha;
        let bob_y = sinf(bob_t * 2.0) * 0.55 * bob_amp;
        let bob_x = crate::cosf(bob_t) * 0.4 * bob_amp;

        let recoil =
            self.weapon.prev_recoil + (self.weapon.recoil - self.weapon.prev_recoil) * alpha;
        let reload = if self.weapon.reloading() {
            let f = 1.0 - (self.weapon.reload_left / self.weapon.cfg.reload_time).clamp(0.0, 1.0);
            sinf(f * core::f32::consts::PI)
        } else {
            0.0
        };

        let camera = Mat4::from_translation(eye)
            * Mat4::from_rotation_y(p.yaw)
            * Mat4::from_rotation_x(p.pitch);
        match self.presentation.motion {
            crate::presentation::ViewMotion::Staff => {
                let f = self.reload_frac();
                let channel = if self.weapon.reloading() {
                    sinf(f * core::f32::consts::PI)
                } else {
                    0.0
                };
                return camera
                    * Mat4::from_translation(Vec3::new(
                        7.2 + bob_x - 3.8 * channel,
                        -7.0 + bob_y + 3.8 * channel,
                        -8.5 + recoil * 1.5,
                    ))
                    * Mat4::from_rotation_z(-0.24 * channel)
                    * Mat4::from_rotation_x(recoil * 0.06 + 0.10 * channel)
                    * Mat4::from_rotation_y(-0.03 + 0.15 * channel);
            }
            crate::presentation::ViewMotion::Throw => {
                let age = if animate_throw {
                    self.weapon.shot_age
                } else {
                    100.0
                };
                let recover = if age < 0.12 {
                    1.0
                } else {
                    (1.0 - (age - 0.12) / 0.34).clamp(0.0, 1.0)
                };
                return camera
                    * Mat4::from_translation(Vec3::new(
                        8.0 + bob_x + 5.0 * recover,
                        -7.0 + bob_y - 20.0 * recover - 10.0 * reload,
                        -8.5 - 4.0 * recover,
                    ))
                    * Mat4::from_rotation_x(-0.4 * reload - 0.25 * recover);
            }
            crate::presentation::ViewMotion::Rifle => {}
        }

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
}

/// Slab-method ray/AABB intersection; returns distance along `dir`.
pub fn ray_aabb(origin: Vec3, dir: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
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

#[cfg(test)]
mod mod_tests {
    use super::*;
    use crate::presentation::{Presentation, ShotStyle};
    use crate::projectile::{Config, MAX_PROJECTILES, Owner};

    fn flight_config(speed: f32) -> Config {
        Config {
            speed,
            gravity: 0.0,
            lift: 0.0,
            radius: 3.0,
            lifetime: 2.0,
        }
    }
    fn flight_map(wall: bool) -> MapCollision {
        use pocket3d_bsp::{
            trace::ModelHulls,
            types::{CONTENTS_EMPTY, CONTENTS_SOLID, ClipNode, Plane},
        };
        MapCollision::from_parts(
            alloc::vec![Plane {
                normal: Vec3::Z,
                dist: -50.0
            }],
            alloc::vec![ClipNode {
                plane: 0,
                children: [CONTENTS_EMPTY, CONTENTS_SOLID]
            }],
            alloc::vec![ClipNode {
                plane: 0,
                children: [CONTENTS_EMPTY, CONTENTS_SOLID]
            }],
            alloc::vec![ModelHulls {
                headnodes: [if wall { 0 } else { CONTENTS_EMPTY }; 4],
                origin: Vec3::ZERO
            }],
            alloc::vec![],
        )
    }
    fn flight_sim() -> StrikeSim {
        let mut sim = StrikeSim::new(Vec3::ZERO, 0.0, Vec::new(), 0);
        sim.bots.push(Bot::spawn(Vec3::new(0.0, 0.0, -100.0), 0.0));
        sim.phase = Phase::Live;
        sim
    }
    #[test]
    fn thrown_shots_hit_on_arrival_and_targets_can_dodge() {
        let map = flight_map(false);
        for dodge in [false, true] {
            let mut sim = flight_sim();
            sim.projectiles.launch(
                Vec3::ZERO,
                sim.bots[0].state.pos,
                Owner::Player,
                flight_config(120.0),
                40,
                100,
            );
            for _ in 0..10 {
                sim.tick_projectiles(&map, 1.0 / 60.0);
            }
            assert_eq!(sim.bots[0].health, 100);
            assert!(sim.events.is_empty());
            if dodge {
                sim.bots[0].state.pos.x = 100.0;
            }
            for _ in 0..70 {
                sim.tick_projectiles(&map, 1.0 / 60.0);
            }
            assert_eq!(sim.bots[0].health, if dodge { 100 } else { 60 });
            assert_eq!(sim.events.len(), usize::from(!dodge));
        }
    }
    #[test]
    fn swept_projectiles_respect_cover_and_do_not_tunnel() {
        for wall in [false, true] {
            let mut sim = flight_sim();
            sim.projectiles.launch(
                Vec3::ZERO,
                sim.bots[0].state.pos,
                Owner::Player,
                flight_config(12000.0),
                40,
                100,
            );
            sim.tick_projectiles(&flight_map(wall), 1.0 / 60.0);
            assert_eq!(sim.bots[0].health, if wall { 100 } else { 60 });
            assert!(sim.projectiles.list.is_empty());
        }
    }
    #[test]
    fn bot_projectiles_obey_the_same_cover_and_contact_law() {
        for wall in [false, true] {
            let mut sim = flight_sim();
            sim.player.state.pos = Vec3::new(0.0, 0.0, -100.0);
            sim.projectiles.launch(
                Vec3::ZERO,
                sim.player.state.pos,
                Owner::Bot,
                flight_config(12000.0),
                9,
                9,
            );
            assert_eq!(
                sim.tick_projectiles(&flight_map(wall), 1.0 / 60.0),
                if wall { 0 } else { 9 }
            );
            assert_eq!(sim.tick_projectiles(&flight_map(wall), 1.0 / 60.0), 0);
        }
    }
    #[test]
    fn flight_count_lifetime_round_reset_and_gravity_are_bounded() {
        let mut sim = flight_sim();
        let mut cfg = flight_config(200.0);
        cfg.gravity = 320.0;
        for _ in 0..MAX_PROJECTILES {
            assert!(
                sim.projectiles
                    .launch(Vec3::ZERO, Vec3::X, Owner::Player, cfg, 10, 10)
            );
        }
        let capacity = sim.projectiles.list.capacity();
        assert!(
            !sim.projectiles
                .launch(Vec3::ZERO, Vec3::X, Owner::Player, cfg, 10, 10)
        );
        sim.tick_projectiles(&flight_map(false), 0.5);
        let p = &sim.projectiles.list[0];
        assert!((p.position.x - 100.0).abs() < 0.001);
        assert!((p.position.y + 40.0).abs() < 0.001);
        for _ in 0..4 {
            sim.tick_projectiles(&flight_map(false), 0.5);
        }
        assert!(sim.projectiles.list.is_empty());
        assert_eq!(sim.projectiles.list.capacity(), capacity);
        sim.projectiles
            .launch(Vec3::ZERO, Vec3::X, Owner::Player, cfg, 10, 10);
        sim.reset_round(0);
        assert!(sim.projectiles.list.is_empty());
    }
    #[test]
    fn staff_recharge_stays_visible_and_returns_to_the_hold_pose() {
        let mut sim = flight_sim();
        sim.presentation.motion = crate::presentation::ViewMotion::Staff;
        let held = sim.viewmodel_transform_at(1.0);
        sim.weapon.ammo = 1;
        sim.weapon.trigger_reload();
        sim.weapon.reload_left = sim.weapon.cfg.reload_time / 2.0;
        let raised = sim.viewmodel_transform_at(1.0);
        assert!(raised.w_axis.y > held.w_axis.y + 3.0);
        sim.weapon.reload_left = 0.0;
        assert_eq!(sim.viewmodel_transform_at(1.0), held);
        sim.presentation.motion = crate::presentation::ViewMotion::Throw;
        let origin = sim
            .viewmodel_pose(1.0, false)
            .transform_point3(Vec3::new(0.0, 2.0, -13.0));
        sim.weapon.shot_age = 0.0;
        assert_eq!(
            sim.viewmodel_pose(1.0, false)
                .transform_point3(Vec3::new(0.0, 2.0, -13.0)),
            origin
        );
    }
    #[test]
    fn menu_configuration_is_bounded_and_last_command_wins() {
        let mut pending = Vec::new();
        for i in 0..100 {
            retain_configuration(&mut pending, Command::SetBotCount(8));
            retain_configuration(&mut pending, Command::AddWin);
            retain_configuration(&mut pending, Command::ResetRound);
            retain_configuration(
                &mut pending,
                Command::ConfigureBots(BotConfig {
                    count: i % 3 + 1,
                    ..Default::default()
                }),
            );
            retain_configuration(
                &mut pending,
                Command::ConfigureWeapon(WeaponConfig {
                    mag_size: 12,
                    reserve: 120,
                    ..Default::default()
                }),
            );
            assert_eq!(pending.len(), 3);
        }
        let mut sim = StrikeSim::new(Vec3::ZERO, 0., Vec::new(), 3);
        sim.presentation = Presentation {
            muzzle: Vec3::new(0., 12., -29.3),
            shot: ShotStyle::Beam,
            ..Default::default()
        };
        for command in pending {
            sim.apply(command, 0);
        }
        sim.reset_round(0);
        assert_eq!(sim.bot_count, 1);
        assert_eq!(sim.weapon.ammo, 12);
        assert_eq!(sim.weapon.reserve, 120);
        assert_eq!(sim.score.wins, 0);
        assert_eq!(sim.presentation.shot, ShotStyle::Beam);
        assert_eq!(sim.presentation.muzzle, Vec3::new(0., 12., -29.3));
    }
    #[test]
    fn presentation_does_not_change_ballistics_or_rng() {
        use pocket3d_bsp::{trace::ModelHulls, types::CONTENTS_EMPTY};
        let col = MapCollision::from_parts(
            alloc::vec![],
            alloc::vec![],
            alloc::vec![],
            alloc::vec![ModelHulls {
                headnodes: [CONTENTS_EMPTY; 4],
                origin: Vec3::ZERO
            }],
            alloc::vec![],
        );
        let mut gun = StrikeSim::new(Vec3::ZERO, 0., Vec::new(), 0);
        let mut magic = StrikeSim::new(Vec3::ZERO, 0., Vec::new(), 0);
        magic.presentation = Presentation {
            muzzle: Vec3::new(0., 12., -29.3),
            shot: ShotStyle::Beam,
            ..Default::default()
        };
        gun.fire_shot(&col, true);
        magic.fire_shot(&col, true);
        let end = |sim: &StrikeSim| {
            sim.effects
                .list
                .iter()
                .find_map(|e| match e.kind {
                    EffectKind::Tracer { b, .. } => Some(b),
                    _ => None,
                })
                .unwrap()
        };
        assert_eq!(end(&gun), end(&magic));
        assert_eq!(gun.rng.0, magic.rng.0);
        assert_eq!(gun.player.pitch, magic.player.pitch);
    }
}
