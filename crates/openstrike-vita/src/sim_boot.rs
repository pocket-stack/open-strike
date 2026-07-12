//! Construct the shared simulation from a parsed cooked map and replay the
//! product configuration that the JS guest emitted before a world existed.

use openstrike_core::sim::Command;
use openstrike_core::StrikeSim;
use pocket3d_bsp::cooked::CookedMap;

pub fn from_map(map: &CookedMap<'_>, boot_config: &[Command]) -> Result<StrikeSim, &'static str> {
    let spawn = map.ct_spawns.first().ok_or("map has no CT spawns")?;
    let bot_spawns = if map.t_spawns.is_empty() {
        map.ct_spawns.clone()
    } else {
        map.t_spawns.clone()
    };
    let mut sim = StrikeSim::new(spawn.pos, spawn.yaw, bot_spawns, 3);
    for command in boot_config {
        sim.apply(command.clone(), 0);
    }
    Ok(sim)
}
