//! Deterministic actor inspection/stress scene. Compiled only with
//! --character-bench; regular gameplay keeps its real simulation and input.
use glam::Vec3;
use openstrike_core::bot::ActorClip;
use openstrike_core::{Bot, BotState, StrikeSim};

pub fn stage(sim: &mut StrikeSim, frame: u32) {
    // Fixed camera from the loaded map's CT spawn, looking into the room.
    // One / three / six visible actors exercise separate load configurations.
    let count = match (frame / 1800) % 3 {
        0 => 1,
        1 => 3,
        _ => 6,
    };
    while sim.bots.len() < count {
        sim.bots.push(Bot::spawn(sim.player.state.pos, 0.0));
    }
    sim.bots.truncate(count);
    sim.player.health = 100;
    sim.player.alive = true;
    let forward = sim.player.forward_flat();
    let right = Vec3::new(-forward.z, 0.0, forward.x);
    let clip = ActorClip::ALL[((frame / 180) % 7) as usize];
    let time = (frame % 180) as f32 / 60.0;
    for (i, bot) in sim.bots.iter_mut().enumerate() {
        let col = i % 3;
        let row = i / 3;
        let shift = if count == 1 {
            0.0
        } else {
            col as f32 - 1.0
                + if count == 6 {
                    if row == 0 { -0.35 } else { 0.35 }
                } else {
                    0.0
                }
        };
        bot.state.pos = sim.player.state.pos
            + forward
                * (if clip == ActorClip::Death {
                    185.0
                } else {
                    110.0
                } + row as f32 * 70.0)
            + right * shift * 45.0;
        bot.prev_pos = bot.state.pos;
        bot.yaw = sim.player.yaw + core::f32::consts::PI;
        bot.health = 100;
        bot.brain = BotState::Patrol;
        bot.shot_age = 100.0;
        bot.hurt_age = 100.0;
        bot.reload_remaining = 0.0;
        bot.anim.speed = 0.0;
        bot.anim.time = time;
        bot.idle_time = time;
        match clip {
            ActorClip::Idle => {}
            ActorClip::Walk => bot.anim.speed = 1.0,
            ActorClip::Run => bot.anim.speed = 2.0,
            ActorClip::Fire => bot.shot_age = time % 0.333,
            ActorClip::Reload => bot.reload_remaining = 2.0 - time % 2.0,
            ActorClip::Hit => bot.hurt_age = time % 0.333,
            ActorClip::Death => {
                bot.brain = BotState::Dead;
                bot.death_time = time;
            }
        }
    }
}
