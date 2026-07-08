//! openstrike-core: the OpenStrike simulation, shared verbatim between the
//! desktop (wgpu) binary and the PSP EBOOT.
//!
//! The split follows RUNTIMES.md: this crate is the FPS core's *state and
//! time* — everything that must be deterministic and identical across
//! platforms. Presentation (scenes, GE display lists), input devices
//! (keyboard/mouse vs. PSP pad) and guest hosting (rquickjs vs. embedded
//! QuickJS) live in the platform binaries, which drive [`StrikeSim`] through
//! [`SimInput`] and drain its [`GameEvent`]s / apply its [`Command`]s.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod bot;
pub mod sim;
pub mod weapon;

pub use bot::{Bot, BotConfig, BotState};
pub use sim::{Command, GameEvent, Phase, Player, Score, SimInput, StrikeSim};
pub use weapon::{
    EffectKind, Effects, FxBeam, FxSprite, GUN_COLORS, MUZZLE_LOCAL, RANGE, RifleBox, Rng, Weapon,
    WeaponConfig, rifle_boxes,
};

/// Animation playback state, mirrored into the desktop renderer's
/// `AnimState` (kept here so bot logic stays platform-free).
#[derive(Clone, Copy, Debug)]
pub struct AnimPlayback {
    pub clip: usize,
    pub time: f32,
    pub speed: f32,
    pub looping: bool,
}

impl Default for AnimPlayback {
    fn default() -> Self {
        Self {
            clip: 0,
            time: 0.0,
            speed: 1.0,
            looping: true,
        }
    }
}

impl AnimPlayback {
    pub fn advance(&mut self, dt: f32) {
        self.time += dt * self.speed;
    }
}

#[inline]
pub(crate) fn sin_cos(x: f32) -> (f32, f32) {
    libm::sincosf(x)
}

#[inline]
pub(crate) fn sqrtf(x: f32) -> f32 {
    libm::sqrtf(x)
}

#[inline]
pub(crate) fn atan2f(y: f32, x: f32) -> f32 {
    libm::atan2f(y, x)
}

#[inline]
pub(crate) fn sinf(x: f32) -> f32 {
    libm::sinf(x)
}

#[inline]
pub(crate) fn cosf(x: f32) -> f32 {
    libm::cosf(x)
}
