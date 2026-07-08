//! PSP pad → sim input. Analog stick moves, face buttons look (Coded
//! Arms-style), R fires, L jumps, d-pad down reloads, d-pad up walks.

use openstrike_core::sim::{MOUSE_SENS, SimInput};
use psp::sys::CtrlButtons;

/// Look rates in radians/second (converted to the sim's mouse-delta units).
const LOOK_YAW_RATE: f32 = 2.6;
const LOOK_PITCH_RATE: f32 = 1.7;
const DEADZONE: f32 = 0.19;

pub struct PadInput {
    prev: CtrlButtons,
    /// Look acceleration: ramps 0.55x -> 1x over the first held ~0.3 s.
    look_hold: f32,
}

pub struct TickInput {
    pub sim: SimInput,
    /// Deltas for `StrikeSim::apply_look` (mouse-unit compatible).
    pub look_dx: f32,
    pub look_dy: f32,
}

impl PadInput {
    pub const fn new() -> Self {
        Self {
            prev: CtrlButtons::empty(),
            look_hold: 0.0,
        }
    }

    /// Map one pad sample (buttons + analog) for one fixed step.
    pub fn map(&mut self, buttons: CtrlButtons, lx: u8, ly: u8, dt: f32) -> TickInput {
        let pressed = buttons & !self.prev;
        self.prev = buttons;

        let axis = |raw: u8| {
            let v = (raw as f32 - 128.0) / 128.0;
            if v.abs() < DEADZONE {
                0.0
            } else {
                // Rescale so movement starts at 0 just past the deadzone.
                (v - DEADZONE * v.signum()) / (1.0 - DEADZONE)
            }
        };
        let sim = SimInput {
            move_x: axis(lx),
            move_y: -axis(ly),
            walk: buttons.contains(CtrlButtons::UP),
            jump: buttons.contains(CtrlButtons::LTRIGGER),
            fire: buttons.contains(CtrlButtons::RTRIGGER),
            reload: pressed.contains(CtrlButtons::DOWN),
        };

        let mut yaw = 0.0f32;
        let mut pitch = 0.0f32;
        if buttons.contains(CtrlButtons::SQUARE) {
            yaw += 1.0;
        }
        if buttons.contains(CtrlButtons::CIRCLE) {
            yaw -= 1.0;
        }
        if buttons.contains(CtrlButtons::TRIANGLE) {
            pitch += 1.0;
        }
        if buttons.contains(CtrlButtons::CROSS) {
            pitch -= 1.0;
        }
        if yaw != 0.0 || pitch != 0.0 {
            self.look_hold = (self.look_hold + dt).min(0.3);
        } else {
            self.look_hold = 0.0;
        }
        let accel = 0.55 + 0.45 * (self.look_hold / 0.3);

        // apply_look treats deltas as mouse counts (yaw -= dx * MOUSE_SENS,
        // pitch -= dy * MOUSE_SENS); convert rad/s rates into those units.
        let look_dx = -(yaw * LOOK_YAW_RATE * accel * dt) / MOUSE_SENS;
        let look_dy = -(pitch * LOOK_PITCH_RATE * accel * dt) / MOUSE_SENS;
        TickInput { sim, look_dx, look_dy }
    }
}
