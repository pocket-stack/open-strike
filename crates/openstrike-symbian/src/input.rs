//! E7 hardware keyboard to deterministic OpenStrike input.

use openstrike_core::sim::{SimInput, MOUSE_SENS};
use pocketjs_core::spec::btn;
use pocketjs_symbian_core::extension::{
    KEY_FIRE, KEY_JUMP, KEY_LOOK_DOWN, KEY_LOOK_LEFT, KEY_LOOK_RIGHT, KEY_LOOK_UP, KEY_MOVE_BACK,
    KEY_MOVE_FORWARD, KEY_MOVE_LEFT, KEY_MOVE_RIGHT, KEY_RELOAD, KEY_WALK,
};

const LOOK_YAW_RATE: f32 = 2.6;
const LOOK_PITCH_RATE: f32 = 1.7;

pub struct KeyboardInput {
    look_hold: f32,
}

pub struct TickInput {
    pub sim: SimInput,
    pub look_dx: f32,
    pub look_dy: f32,
}

impl KeyboardInput {
    pub const fn new() -> Self {
        Self {
            look_hold: 0.0,
        }
    }

    pub fn map(&mut self, keys: u32, buttons: u32, dt: f32) -> TickInput {
        let move_x = f32::from((keys & KEY_MOVE_RIGHT != 0) as u8)
            - f32::from((keys & KEY_MOVE_LEFT != 0) as u8);
        let move_y = f32::from((keys & KEY_MOVE_FORWARD != 0) as u8)
            - f32::from((keys & KEY_MOVE_BACK != 0) as u8);
        let yaw = f32::from((keys & KEY_LOOK_RIGHT != 0) as u8)
            - f32::from((keys & KEY_LOOK_LEFT != 0) as u8);
        let pitch = f32::from((keys & KEY_LOOK_DOWN != 0) as u8)
            - f32::from((keys & KEY_LOOK_UP != 0) as u8);

        if yaw != 0.0 || pitch != 0.0 {
            self.look_hold = (self.look_hold + dt).min(0.3);
        } else {
            self.look_hold = 0.0;
        }
        let acceleration = 0.55 + 0.45 * (self.look_hold / 0.3);

        TickInput {
            sim: SimInput {
                move_x,
                move_y,
                walk: keys & KEY_WALK != 0,
                jump: keys & KEY_JUMP != 0,
                fire: keys & KEY_FIRE != 0 || buttons & (btn::CIRCLE | btn::RTRIGGER) != 0,
                // Keep the request alive across both 60 Hz ticks in a 30 Hz
                // host frame. Weapon::trigger_reload is idempotent while a
                // reload is active and rejects full/empty-reserve magazines.
                reload: keys & KEY_RELOAD != 0,
            },
            look_dx: yaw * LOOK_YAW_RATE * acceleration * dt / MOUSE_SENS,
            look_dy: pitch * LOOK_PITCH_RATE * acceleration * dt / MOUSE_SENS,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_movement_fire_and_reload_levels() {
        let mut input = KeyboardInput::new();
        let keys = KEY_MOVE_FORWARD | KEY_MOVE_LEFT | KEY_FIRE | KEY_RELOAD;
        let first = input.map(keys, 0, 1.0 / 60.0);
        let held = input.map(keys, 0, 1.0 / 60.0);
        assert_eq!(first.sim.move_x, -1.0);
        assert_eq!(first.sim.move_y, 1.0);
        assert!(first.sim.fire && held.sim.fire);
        assert!(first.sim.reload && held.sim.reload);
    }

    #[test]
    fn look_signs_match_apply_look() {
        let mut input = KeyboardInput::new();
        let right_down = input.map(KEY_LOOK_RIGHT | KEY_LOOK_DOWN, 0, 1.0 / 60.0);
        assert!(right_down.look_dx > 0.0);
        assert!(right_down.look_dy > 0.0);
        let left_up = input.map(KEY_LOOK_LEFT | KEY_LOOK_UP, 0, 1.0 / 60.0);
        assert!(left_up.look_dx < 0.0);
        assert!(left_up.look_dy < 0.0);
    }

    #[test]
    fn circle_and_right_trigger_are_sustained_fire_inputs() {
        let mut input = KeyboardInput::new();
        for button in [btn::CIRCLE, btn::RTRIGGER, btn::CIRCLE | btn::RTRIGGER] {
            assert!(input.map(0, button, 1.0 / 60.0).sim.fire);
            assert!(input.map(0, button, 1.0 / 60.0).sim.fire);
            assert!(!input.map(0, 0, 1.0 / 60.0).sim.fire);
        }
    }
}
