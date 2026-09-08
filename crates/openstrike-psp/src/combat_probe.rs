//! Hardware combat regression: real collision, bot AI, hits, damage, reloads,
//! deaths and guest-owned round transitions. Only spawn setup and player
//! input are scripted. Alternate winning and losing rounds; no fake events.
use glam::Vec3;
use openstrike_core::sim::{GameEvent, Phase, SimInput};
use openstrike_core::{Bot, StrikeSim};

pub struct CombatProbe {
    initialized: bool,
    round: u32,
    started: f32,
    reloaded: bool,
}

impl CombatProbe {
    pub fn new() -> Self {
        Self {
            initialized: false,
            round: 0,
            started: 0.0,
            reloaded: false,
        }
    }

    pub fn input(&mut self, sim: &mut StrikeSim) -> SimInput {
        if !self.initialized
            || sim
                .events
                .iter()
                .any(|e| matches!(e, GameEvent::RoundReset))
        {
            if self.initialized {
                self.round += 1;
            }
            self.initialized = true;
            self.started = sim.time;
            self.reloaded = false;
            sim.player.pitch = 0.0;
            let forward = sim.player.forward_flat();
            let right = sim.player.right();
            // Two rounds at ordinary range, then two at close range with a
            // wider camera turn between targets. Both alternate win/loss.
            let distance = if self.round % 4 < 2 { 185.0 } else { 80.0 };
            for (i, bot) in sim.bots.iter_mut().enumerate() {
                *bot = Bot::spawn(
                    sim.player.state.pos + forward * distance + right * (i as f32 - 1.0) * 58.0,
                    sim.player.yaw + core::f32::consts::PI,
                );
            }
        }
        let mut input = SimInput::default();
        if sim.phase != Phase::Live {
            return input;
        }
        if let Some(bot) = sim.bots.iter().find(|b| b.alive()) {
            // Aim at the chest, allowing several genuine hits and the Hit
            // clip before a fatal shot. AI and physics continue normally.
            let d = bot.state.pos + Vec3::Y * 5.0 - sim.player.eye();
            sim.player.yaw = libm::atan2f(-d.x, -d.z);
            sim.player.pitch = libm::atan2f(d.y, libm::sqrtf(d.x * d.x + d.z * d.z));
        }
        if self.round % 2 == 0 && sim.time - self.started > 3.5 {
            input.fire = true;
            if !self.reloaded && sim.alive_bots() < sim.bots.len() {
                input.reload = true;
                input.fire = false;
                self.reloaded = true;
            }
        }
        input
    }
}
