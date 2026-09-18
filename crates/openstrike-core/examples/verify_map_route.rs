//! Exercise a cooked map with the same standing hull and movement law as PSP.
//! cargo run -p openstrike-core --example verify_map_route -- map.p3d route.txt
//! route.txt: one hull-centre x y z per line, Pocket3D units (+Y up).
use glam::Vec3;
use pocket3d_bsp::collide::{CharacterState, HullKind, MoveInput, MoveParams, step_character};
use pocket3d_bsp::{cooked, trace::Hull};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let bytes = std::fs::read(args.next().ok_or("map.p3d required")?)?;
    let map = cooked::read(&bytes)?;
    let route = std::fs::read_to_string(args.next().ok_or("route.txt required")?)?;
    let points: Vec<Vec3> = route
        .lines()
        .filter(|s| !s.trim().is_empty())
        .map(|line| {
            let coordinates: Result<Vec<f32>, _> =
                line.split_whitespace().map(str::parse).collect();
            let coordinates = coordinates?;
            if coordinates.len() != 3 || coordinates.iter().any(|n| !n.is_finite()) {
                return Err("route points must contain three finite coordinates".into());
            }
            Ok(Vec3::new(coordinates[0], coordinates[1], coordinates[2]))
        })
        .collect::<Result<_, Box<dyn std::error::Error>>>()?;
    if points.len() < 2 {
        return Err("route needs at least two points".into());
    }
    let params = MoveParams::default();
    let dt = openstrike_core::clock::TICK_SECONDS;
    for spawn in map.ct_spawns.iter().chain(&map.t_spawns) {
        if map.collision.hull_contents(Hull::Stand, spawn.pos) == -2 {
            return Err(format!("spawn inside solid: {:?}", spawn.pos).into());
        }
        let ground = map
            .collision
            .trace(Hull::Stand, spawn.pos, spawn.pos - Vec3::Y * 128.0);
        if ground.fraction == 1.0 || ground.start_solid || ground.normal.y < 0.7 {
            return Err(format!("spawn has no walkable ground: {:?}", spawn.pos).into());
        }
    }
    for reverse in [false, true] {
        let path: Vec<_> = if reverse {
            points.iter().copied().rev().collect()
        } else {
            points.clone()
        };
        let mut state = CharacterState::new(path[0]);
        for _ in 0..64 {
            step_character(
                &map.collision,
                HullKind::Stand,
                &mut state,
                &params,
                &MoveInput::default(),
                dt,
            );
        }
        let mut total = 0;
        for (index, target) in path.iter().enumerate().skip(1) {
            let mut arrived = false;
            for _ in 0..64 * 15 {
                let delta = *target - state.pos;
                let horizontal = Vec3::new(delta.x, 0.0, delta.z);
                if horizontal.length() < 6.0 && delta.y.abs() < 10.0 {
                    arrived = true;
                    break;
                }
                let input = MoveInput {
                    wish_dir: horizontal.normalize_or_zero(),
                    speed: (horizontal.length() / 32.0).min(1.0),
                    jump: false,
                };
                step_character(
                    &map.collision,
                    HullKind::Stand,
                    &mut state,
                    &params,
                    &input,
                    dt,
                );
                total += 1;
                if !state.pos.is_finite()
                    || map.collision.hull_contents(Hull::Stand, state.pos) == -2
                {
                    return Err(format!("route entered solid at {:?}", state.pos).into());
                }
            }
            if !arrived {
                return Err(format!(
                    "route {} blocked at waypoint {index}: {:?} -> {target:?}",
                    if reverse { "reverse" } else { "forward" },
                    state.pos
                )
                .into());
            }
        }
        println!(
            "{}: {} route passed, {} waypoints, {} simulation ticks",
            map.name,
            if reverse { "reverse" } else { "forward" },
            path.len(),
            total
        );
    }
    Ok(())
}
