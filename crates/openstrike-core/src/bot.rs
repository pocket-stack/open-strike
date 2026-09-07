//! Bots: T-side dummies with a small patrol/chase/attack brain.

use core::f32::consts::{FRAC_PI_2, PI, TAU};

use glam::{Mat4, Vec3};
use pocket3d_bsp::collide::{CharacterState, HullKind, MoveInput, MoveParams, step_character};
use pocket3d_bsp::trace::{Hull, MapCollision};

use crate::weapon::{EffectKind, Effects, Rng};
use crate::{AnimPlayback, atan2f, sin_cos, sqrtf};

pub const BOT_HEALTH: i32 = 100;
pub const BOT_EYE: f32 = 20.0;
const SIGHT_RANGE: f32 = 2600.0;
const ATTACK_RANGE: f32 = 420.0;
const LOSE_SIGHT_AFTER: f32 = 1.6;

/// Semantic animation names, independent of a renderer's clip ordering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum ActorClip {
    Idle,
    Walk,
    Run,
    Fire,
    Reload,
    Hit,
    Death,
}
impl ActorClip {
    pub const ALL: [Self; 7] = [
        Self::Idle,
        Self::Walk,
        Self::Run,
        Self::Fire,
        Self::Reload,
        Self::Hit,
        Self::Death,
    ];
    pub fn name(self) -> &'static str {
        ["Idle", "Walk", "Run", "Fire", "Reload", "Hit", "Death"][self as usize]
    }
}

/// Bot tuning — owned by the `strike` surface (mods set it through
/// `strike.configureBots`). Defaults are the base game's difficulty.
#[derive(Clone, Debug)]
pub struct BotConfig {
    /// Enemy count per round.
    pub count: usize,
    pub speed: f32,
    /// Seconds between attack volleys (scaled ±25% per shot).
    pub attack_interval: f32,
    pub damage_min: i32,
    pub damage_max: i32,
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            count: 3,
            speed: 190.0,
            attack_interval: 1.4,
            damage_min: 8,
            damage_max: 14,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BotState {
    Patrol,
    Chase,
    Attack,
    Dead,
}

pub struct Bot {
    pub state: CharacterState,
    pub prev_pos: Vec3,
    pub yaw: f32,
    pub health: i32,
    pub brain: BotState,
    pub anim: AnimPlayback,
    wander_yaw: f32,
    think_timer: f32,
    attack_timer: f32,
    lost_timer: f32,
    pub death_time: f32,
    pub shot_age: f32,
    pub hurt_age: f32,
    pub reload_remaining: f32,
    pub magazine: u8,
    pub idle_time: f32,
}

pub struct BotShot {
    pub damage: i32,
}

impl Bot {
    pub fn spawn(pos: Vec3, yaw: f32) -> Self {
        Self {
            state: CharacterState::new(pos),
            prev_pos: pos,
            yaw,
            health: BOT_HEALTH,
            brain: BotState::Patrol,
            anim: AnimPlayback::default(),
            wander_yaw: yaw,
            think_timer: 0.0,
            attack_timer: 1.0,
            lost_timer: 0.0,
            death_time: 0.0,
            shot_age: 100.0,
            hurt_age: 100.0,
            reload_remaining: 0.0,
            magazine: 12,
            idle_time: 0.0,
        }
    }

    pub fn alive(&self) -> bool {
        self.brain != BotState::Dead
    }

    pub fn eye(&self) -> Vec3 {
        self.state.pos + Vec3::Y * BOT_EYE
    }

    pub fn hurt(&mut self, dmg: i32) -> bool {
        if !self.alive() {
            return false;
        }
        self.health -= dmg;
        self.hurt_age = 0.0;
        if self.health <= 0 {
            self.brain = BotState::Dead;
            self.death_time = 0.0;
            return true;
        }
        false
    }

    fn yaw_towards(&mut self, target: Vec3, dt: f32, rate: f32) {
        let d = target - self.state.pos;
        let want = atan2f(-d.x, -d.z);
        let mut diff = want - self.yaw;
        while diff > PI {
            diff -= TAU;
        }
        while diff < -PI {
            diff += TAU;
        }
        let max = rate * dt;
        self.yaw += diff.clamp(-max, max);
    }

    /// Advance one tick. Returns a shot descriptor when the bot lands a hit
    /// on the player this tick.
    #[allow(clippy::too_many_arguments)]
    pub fn tick(
        &mut self,
        col: &MapCollision,
        player_eye: Vec3,
        player_alive: bool,
        dt: f32,
        cfg: &BotConfig,
        rng: &mut Rng,
        effects: &mut Effects,
    ) -> Option<BotShot> {
        self.prev_pos = self.state.pos;
        self.shot_age += dt;
        self.hurt_age += dt;
        self.idle_time += dt;
        if self.brain == BotState::Dead {
            self.death_time += dt;
            self.anim.speed = 0.0;
            return None;
        }
        if self.reload_remaining > 0.0 {
            self.reload_remaining = (self.reload_remaining - dt).max(0.0);
            if self.reload_remaining == 0.0 {
                self.magazine = 12;
            }
        }

        // Perception: distance + line of sight to the player's eye.
        let to_player = player_eye - self.eye();
        let dist = to_player.length();
        let visible = player_alive && dist < SIGHT_RANGE && {
            let tr = col.trace(Hull::Point, self.eye(), player_eye);
            tr.fraction >= 1.0
        };

        if visible {
            self.lost_timer = 0.0;
        } else {
            self.lost_timer += dt;
        }

        // Brain transitions.
        self.brain = match self.brain {
            BotState::Patrol if visible => BotState::Chase,
            BotState::Chase if visible && dist < ATTACK_RANGE => BotState::Attack,
            BotState::Chase if self.lost_timer > LOSE_SIGHT_AFTER => BotState::Patrol,
            BotState::Attack if !visible || dist > ATTACK_RANGE * 1.25 => {
                if self.lost_timer > LOSE_SIGHT_AFTER {
                    BotState::Patrol
                } else {
                    BotState::Chase
                }
            }
            s => s,
        };

        let mut shot = None;
        let mut wish = Vec3::ZERO;
        let mut speed = 0.0;

        match self.brain {
            BotState::Patrol => {
                self.think_timer -= dt;
                let (sy, cy) = sin_cos(self.wander_yaw);
                let fwd = Vec3::new(-sy, 0.0, -cy);
                // Re-pick direction when the timer expires or a wall is close.
                let probe = col.trace(Hull::Stand, self.state.pos, self.state.pos + fwd * 56.0);
                if self.think_timer <= 0.0 || probe.fraction < 1.0 {
                    self.wander_yaw = rng.range(0.0, TAU);
                    self.think_timer = rng.range(1.5, 4.0);
                }
                let (sy, cy) = sin_cos(self.wander_yaw);
                let fwd = Vec3::new(-sy, 0.0, -cy);
                wish = fwd;
                speed = 0.55;
                self.yaw_towards(self.state.pos + fwd * 100.0, dt, 4.0);
            }
            BotState::Chase => {
                let dir = Vec3::new(to_player.x, 0.0, to_player.z).normalize_or_zero();
                wish = dir;
                speed = 1.0;
                self.yaw_towards(player_eye, dt, 7.0);
            }
            BotState::Attack => {
                self.yaw_towards(player_eye, dt, 9.0);
                self.attack_timer -= dt;
                if self.magazine == 0 && self.reload_remaining == 0.0 && self.shot_age >= 0.333 {
                    self.reload_remaining = 2.0;
                }
                if self.attack_timer <= 0.0
                    && visible
                    && self.magazine > 0
                    && self.reload_remaining == 0.0
                {
                    self.attack_timer = cfg.attack_interval * rng.range(0.85, 1.25);
                    self.magazine -= 1;
                    self.shot_age = 0.0;
                    // Muzzle flash + tracer from the bot towards the player.
                    let from = self.muzzle_position();
                    let miss = rng.f32() > (1.25 - dist / 900.0).clamp(0.25, 0.85);
                    let aim = if miss {
                        player_eye
                            + Vec3::new(rng.signed(), rng.signed() * 0.4, rng.signed()) * 45.0
                    } else {
                        player_eye
                    };
                    effects.spawn(EffectKind::MuzzleFlash { pos: from }, 0.08);
                    effects.spawn(EffectKind::Tracer { a: from, b: aim }, 0.09);
                    if !miss {
                        let span = (cfg.damage_max - cfg.damage_min).max(0) as f32;
                        shot = Some(BotShot {
                            damage: cfg.damage_min + (rng.f32() * (span + 1.0)) as i32,
                        });
                    }
                }
            }
            BotState::Dead => {}
        }

        let input = MoveInput {
            wish_dir: wish,
            speed,
            jump: false,
        };
        step_character(
            col,
            HullKind::Stand,
            &mut self.state,
            &MoveParams {
                max_speed: cfg.speed,
                ..Default::default()
            },
            &input,
            dt,
        );

        // Animation: walk speed scales the clip; idle freezes it.
        let ground_speed =
            sqrtf(self.state.vel.x * self.state.vel.x + self.state.vel.z * self.state.vel.z);
        if ground_speed > 12.0 {
            self.anim.speed = (ground_speed / 90.0).clamp(0.6, 2.2);
        } else {
            self.anim.speed = 0.0;
        }
        self.anim.advance(dt);
        shot
    }

    /// Geometry-independent action state consumed by every presentation host.
    pub fn animation_sample(&self) -> (ActorClip, f32) {
        if !self.alive() {
            return (ActorClip::Death, self.death_time);
        }
        if self.reload_remaining > 0.0 {
            return (ActorClip::Reload, 2.0 - self.reload_remaining);
        }
        if self.hurt_age < 0.333 {
            return (ActorClip::Hit, self.hurt_age);
        }
        if self.shot_age < 0.333 {
            return (ActorClip::Fire, self.shot_age);
        }
        if self.anim.speed > 1.5 {
            (ActorClip::Run, self.anim.time * (0.667 / 1.0))
        } else if self.anim.speed > 0.0 {
            (ActorClip::Walk, self.anim.time)
        } else {
            (ActorClip::Idle, self.idle_time)
        }
    }

    /// The officer's carbine muzzle in the same feet space as its asset.
    pub fn muzzle_position(&self) -> Vec3 {
        self.transform_scaled(1.0)
            .transform_point3(Vec3::new(3.60, 50.97, -36.52))
    }

    /// World transform, including the death fall. `scale` maps the model's
    /// native height to the 70-unit game height (desktop passes
    /// `70.0 / asset.height()`).
    pub fn transform_scaled(&self, scale: f32) -> Mat4 {
        let feet = self.state.pos - Vec3::Y * 36.0;
        let fall = (self.death_time * 3.0).min(1.0);
        // Ease-out fall backwards, slight sink so the corpse hugs the ground.
        let ease = 1.0 - (1.0 - fall) * (1.0 - fall);
        Mat4::from_translation(feet + Vec3::Y * (2.0 - 2.0 * ease))
            * Mat4::from_rotation_y(self.yaw)
            * Mat4::from_rotation_x(-ease * FRAC_PI_2 * 0.94)
            * Mat4::from_scale(Vec3::splat(scale))
    }

    pub fn tint(&self) -> [f32; 4] {
        // The soldier ships real materials — no warm tint on the living.
        if self.alive() {
            [1.0, 1.0, 1.0, 1.0]
        } else {
            [0.5, 0.42, 0.4, 1.0]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use pocket3d_bsp::trace::ModelHulls;
    use pocket3d_bsp::types::CONTENTS_EMPTY;

    fn open_space() -> MapCollision {
        MapCollision::from_parts(
            vec![],
            vec![],
            vec![],
            vec![ModelHulls {
                headnodes: [CONTENTS_EMPTY; 4],
                origin: Vec3::ZERO,
            }],
            vec![],
        )
    }
    #[test]
    fn firing_misses_still_animate_and_empty_magazine_reloads_before_next_shot() {
        let col = open_space();
        let mut bot = Bot::spawn(Vec3::ZERO, 0.0);
        bot.brain = BotState::Attack;
        bot.magazine = 1;
        bot.attack_timer = 0.0;
        let mut effects = Effects::default();
        let mut rng = Rng(7);
        let config = BotConfig::default();
        bot.tick(
            &col,
            Vec3::new(0.0, 20.0, -100.0),
            true,
            1.0 / 60.0,
            &config,
            &mut rng,
            &mut effects,
        );
        assert_eq!(bot.magazine, 0);
        assert_eq!(bot.animation_sample().0, ActorClip::Fire);
        for _ in 0..22 {
            bot.state.pos = Vec3::ZERO;
            bot.tick(
                &col,
                Vec3::new(0.0, 20.0, -100.0),
                true,
                1.0 / 60.0,
                &config,
                &mut rng,
                &mut effects,
            );
        }
        assert_eq!(bot.animation_sample().0, ActorClip::Reload);
        for _ in 0..100 {
            bot.state.pos = Vec3::ZERO;
            bot.tick(
                &col,
                Vec3::new(0.0, 20.0, -100.0),
                true,
                1.0 / 60.0,
                &config,
                &mut rng,
                &mut effects,
            );
            assert_eq!(bot.magazine, 0, "fired/refilled before reload completed");
        }
        for _ in 0..25 {
            bot.state.pos = Vec3::ZERO;
            bot.tick(
                &col,
                Vec3::new(0.0, 20.0, -100.0),
                true,
                1.0 / 60.0,
                &config,
                &mut rng,
                &mut effects,
            );
        }
        assert!(bot.magazine > 0);
        assert_eq!(bot.reload_remaining, 0.0);
    }
    #[test]
    fn death_overrides_actions_and_muzzle_tracks_actor_facing() {
        let mut bot = Bot::spawn(Vec3::new(10.0, 36.0, 20.0), 0.0);
        let forward = bot.muzzle_position() - bot.state.pos;
        bot.yaw = PI;
        let reverse = bot.muzzle_position() - bot.state.pos;
        assert!((forward.x + reverse.x).abs() < 0.01);
        assert!((forward.z + reverse.z).abs() < 0.01);
        assert!((forward.y - reverse.y).abs() < 0.01);
        bot.reload_remaining = 1.0;
        bot.shot_age = 0.0;
        assert!(bot.hurt(100));
        assert_eq!(bot.animation_sample(), (ActorClip::Death, 0.0));
    }
}
