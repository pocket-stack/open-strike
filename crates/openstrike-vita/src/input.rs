//! Vita pad to deterministic simulation input.
//!
//! Left stick moves and right stick looks. No touch input is required: R fires,
//! L jumps, d-pad down reloads, d-pad up walks, and SELECT is forwarded in the
//! raw button mask so the shared PocketJS HUD can open its return-to-menu
//! dialog. The Vita button bits intentionally match PocketJS' BTN contract.

use openstrike_core::sim::{SimInput, MOUSE_SENS};

pub const BTN_SELECT: u32 = 0x0001;
pub const BTN_UP: u32 = 0x0010;
pub const BTN_DOWN: u32 = 0x0040;
pub const BTN_L: u32 = 0x0100;
pub const BTN_R: u32 = 0x0200;

const MOVE_DEADZONE: f32 = 0.19;
const LOOK_DEADZONE: f32 = 0.14;
const LOOK_YAW_RATE: f32 = 2.9;
const LOOK_PITCH_RATE: f32 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PadSample {
    pub buttons: u32,
    pub lx: u8,
    pub ly: u8,
    pub rx: u8,
    pub ry: u8,
}

impl Default for PadSample {
    fn default() -> Self {
        Self {
            buttons: 0,
            lx: 128,
            ly: 128,
            rx: 128,
            ry: 128,
        }
    }
}

pub struct TickInput {
    pub sim: SimInput,
    /// Deltas accepted by `StrikeSim::apply_look` (mouse-count units).
    pub look_dx: f32,
    pub look_dy: f32,
    /// Unmodified mask passed to PocketJS `frame(mask)` for focus/button edges.
    pub ui_buttons: u32,
}

#[derive(Default)]
pub struct PadInput {
    previous_buttons: u32,
}

impl PadInput {
    pub const fn new() -> Self {
        Self {
            previous_buttons: 0,
        }
    }

    pub fn map(&mut self, sample: PadSample, dt: f32) -> TickInput {
        let pressed = sample.buttons & !self.previous_buttons;
        self.previous_buttons = sample.buttons;

        let move_x = axis(sample.lx, MOVE_DEADZONE);
        let move_y = -axis(sample.ly, MOVE_DEADZONE);
        let look_x = axis(sample.rx, LOOK_DEADZONE);
        let look_y = axis(sample.ry, LOOK_DEADZONE);

        TickInput {
            sim: SimInput {
                move_x,
                move_y,
                walk: sample.buttons & BTN_UP != 0,
                jump: sample.buttons & BTN_L != 0,
                fire: sample.buttons & BTN_R != 0,
                reload: pressed & BTN_DOWN != 0,
            },
            // Positive right/down stick movement maps to the same sign as the
            // desktop host's mouse delta contract.
            look_dx: look_x * LOOK_YAW_RATE * dt / MOUSE_SENS,
            look_dy: look_y * LOOK_PITCH_RATE * dt / MOUSE_SENS,
            ui_buttons: sample.buttons,
        }
    }
}

fn axis(raw: u8, deadzone: f32) -> f32 {
    let value = (raw as f32 - 128.0) / 128.0;
    if value.abs() < deadzone {
        0.0
    } else {
        (value - deadzone * value.signum()) / (1.0 - deadzone)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centered_sticks_are_idle() {
        let tick = PadInput::new().map(PadSample::default(), 1.0 / 60.0);
        assert_eq!(tick.sim.move_x, 0.0);
        assert_eq!(tick.sim.move_y, 0.0);
        assert_eq!(tick.look_dx, 0.0);
        assert_eq!(tick.look_dy, 0.0);
    }

    #[test]
    fn both_sticks_cover_their_full_ranges() {
        let tick = PadInput::new().map(
            PadSample {
                lx: 255,
                ly: 0,
                rx: 255,
                ry: 0,
                ..PadSample::default()
            },
            1.0 / 60.0,
        );
        assert!(tick.sim.move_x > 0.98);
        assert!(tick.sim.move_y > 0.98);
        assert!(tick.look_dx > 0.0);
        assert!(tick.look_dy < 0.0);
    }

    #[test]
    fn reload_is_an_edge_while_fire_and_jump_are_levels() {
        let mut pad = PadInput::new();
        let sample = PadSample {
            buttons: BTN_DOWN | BTN_L | BTN_R,
            ..PadSample::default()
        };
        let first = pad.map(sample, 1.0 / 60.0);
        let held = pad.map(sample, 1.0 / 60.0);
        assert!(first.sim.reload);
        assert!(!held.sim.reload);
        assert!(first.sim.fire && held.sim.fire);
        assert!(first.sim.jump && held.sim.jump);
    }
}
