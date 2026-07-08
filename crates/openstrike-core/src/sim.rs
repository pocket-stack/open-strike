//! The round simulation: player, bots, combat, phase gate, and the strike
//! surface's command/event/state types.

use alloc::vec::Vec;

use glam::{Mat4, Vec3};
use pocket3d_bsp::collide::{CharacterState, HullKind, MoveInput, MoveParams, step_character};
use pocket3d_bsp::trace::{Hull, MapCollision};
use pocket3d_bsp::types::SpawnPoint;

use crate::bot::{Bot, BotConfig};
use crate::weapon::{EffectKind, Effects, MUZZLE_LOCAL, RANGE, Rng, Weapon, WeaponConfig};
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
    Ended { won: bool },
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
    SetPhase(Phase),
    ResetRound,
    AddWin,
    AddLoss,
    SetBotCount(usize),
    ConfigureWeapon(WeaponConfig),
    ConfigureBots(BotConfig),
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
    /// Reload pressed this tick.
    pub reload: bool,
}

/// The platform-free simulation. Platforms own presentation and input; this
/// owns state and time.
pub struct StrikeSim {
    pub player: Player,
    pub bots: Vec<Bot>,
    pub bot_count: usize,
    pub weapon: Weapon,
    pub effects: Effects,
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
            player: Player::spawn(spawn_pos, spawn_yaw),
            bots: Vec::new(),
            bot_count,
            weapon: Weapon::default(),
            effects: Effects::default(),
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
        self.spawn_bots(walk_clip);
        self.phase = Phase::Starting;
        self.events.push(GameEvent::RoundReset);
    }

    /// Apply one guest command (drained after each guest turn).
    pub fn apply(&mut self, cmd: Command, walk_clip: usize) {
        match cmd {
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
        self.time += dt;
        self.effects.tick(dt);
        self.weapon.tick(dt);

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
            if input.fire && self.weapon.fire() {
                self.fire_shot(col);
            }
        }

        // Bots.
        let player_eye = self.player.eye();
        let player_alive = self.player.alive;
        let mut incoming = 0i32;
        let bot_cfg = self.bot_cfg.clone();
        for bot in &mut self.bots {
            let shot = bot.tick(
                col,
                player_eye,
                player_alive && live,
                dt,
                &bot_cfg,
                &mut self.rng,
                &mut self.effects,
            );
            if live {
                if let Some(s) = shot {
                    incoming += s.damage;
                }
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

    fn tick_player_movement(
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

    fn fire_shot(&mut self, col: &MapCollision) {
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
            .viewmodel_transform_at(1.0)
            .transform_point3(MUZZLE_LOCAL);
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

    /// Viewmodel placement: camera-anchored with bob, recoil, and reload dip.
    ///
    /// `alpha` is the render interpolation factor. The gun MUST ride the same
    /// interpolated eye as the camera — anchoring it to the raw tick position
    /// makes it jitter against the world at the tick rate whenever the player
    /// moves. Bob and recoil interpolate for the same reason.
    pub fn viewmodel_transform_at(&self, alpha: f32) -> Mat4 {
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
