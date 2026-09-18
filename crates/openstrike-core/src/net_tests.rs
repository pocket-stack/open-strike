use super::net::*;
use crate::*;
use glam::Vec3;
use pocket3d_bsp::{
    trace::{MapCollision, ModelHulls},
    types::{ClipNode, Plane, SpawnPoint},
};
fn floor(wall: bool) -> MapCollision {
    let mut planes = vec![
        Plane {
            normal: Vec3::Y,
            dist: 0.0,
        },
        Plane {
            normal: Vec3::Y,
            dist: 36.0,
        },
    ];
    let mut point = vec![ClipNode {
        plane: 0,
        children: [-1, -2],
    }];
    let mut stand = vec![ClipNode {
        plane: 1,
        children: [-1, -2],
    }];
    let head = if wall {
        planes.push(Plane {
            normal: Vec3::Z,
            dist: 0.0,
        });
        point.push(ClipNode {
            plane: 2,
            children: [0, -2],
        });
        stand.push(ClipNode {
            plane: 2,
            children: [0, -2],
        });
        1
    } else {
        0
    };
    MapCollision::from_parts(
        planes,
        point,
        stand,
        vec![ModelHulls {
            headnodes: [head; 4],
            origin: Vec3::ZERO,
        }],
        vec![],
    )
}
fn duel() -> Duel {
    Duel::new(
        "fixture".into(),
        [
            SpawnPoint {
                pos: Vec3::new(0.0, 36.1, 150.0),
                yaw: 0.0,
            },
            SpawnPoint {
                pos: Vec3::new(0.0, 36.1, -150.0),
                yaw: core::f32::consts::PI,
            },
        ],
    )
}
fn exchange(room: &mut Duel, slot: usize, epoch: u32, inputs: Vec<Input>) -> Reply {
    let raw = serde_json::to_string(&Request {
        v: 1,
        map: "fixture".into(),
        epoch,
        inputs,
    })
    .unwrap();
    serde_json::from_str(&room.exchange(slot, &raw).unwrap()).unwrap()
}
#[test]
fn authority_damage_respawn_and_replayed_inputs() {
    let col = floor(false);
    let mut room = duel();
    let a = exchange(&mut room, 0, 0, vec![]);
    let b = exchange(&mut room, 1, 0, vec![]);
    room.step(&col);
    for n in 1..=80 {
        let cmd = Input(n, 0.0, 0.0, 0.0, 0.0, 4);
        exchange(&mut room, 0, a.epoch, vec![cmd]);
        exchange(&mut room, 0, a.epoch, vec![cmd]); // duplicate cannot fire twice
        exchange(
            &mut room,
            1,
            b.epoch,
            vec![Input(n, 0.0, 0.0, core::f32::consts::PI, 0.0, 0)],
        );
        room.step(&col);
    }
    assert_eq!(room.score, [1, 0]);
    assert_eq!(room.players[1].player.health, 0);
    assert!(room.players[0].weapon.ammo >= 27);
    for _ in 0..200 {
        exchange(&mut room, 0, a.epoch, vec![]);
        exchange(&mut room, 1, b.epoch, vec![]);
        room.step(&col);
    }
    assert_eq!(room.players[1].player.health, 100);
    assert_eq!(room.players[0].weapon.ammo, 30);
}
#[test]
fn wall_blocks_hits_and_clock_prevents_request_speedup() {
    let mut room = duel();
    let col = floor(true);
    let a = exchange(&mut room, 0, 0, vec![]);
    exchange(&mut room, 1, 0, vec![]);
    room.step(&col);
    for n in 1..=12 {
        exchange(&mut room, 0, a.epoch, vec![Input(n, 0.0, 0.0, 0.0, 0.0, 4)]);
    }
    assert_eq!(room.tick, 1);
    assert_eq!(room.players[0].weapon.ammo, 30);
    for _ in 0..12 {
        room.step(&col);
    }
    assert_eq!(room.players[1].player.health, 100);
    assert!(room.players[0].weapon.ammo < 30);
}
#[test]
fn malformed_map_gap_epoch_and_stale_peer_are_rejected() {
    let mut room = duel();
    let col = floor(false);
    assert!(room
        .exchange(0, r#"{"v":1,"map":"wrong","epoch":0,"inputs":[]}"#)
        .is_err());
    let a = exchange(&mut room, 0, 0, vec![]);
    exchange(&mut room, 1, 0, vec![]);
    let raw = serde_json::to_string(&Request {
        v: 1,
        map: "fixture".into(),
        epoch: a.epoch,
        inputs: vec![Input(2, 0.0, 1.0, 0.0, 0.0, 0)],
    })
    .unwrap();
    assert_eq!(room.exchange(0, &raw).unwrap_err(), "input gap");
    for _ in 0..130 {
        room.step(&col);
    }
    assert!(room.exchange(0, &raw).is_err());
    let fresh = exchange(&mut room, 0, 0, vec![]);
    assert_ne!(fresh.epoch, a.epoch);
    assert!(!fresh.playing);
    assert!(room.exchange(0, &raw).is_err());
    let oversized = "x".repeat(PAYLOAD_LIMIT + 1);
    assert!(room.exchange(0, &oversized).is_err());
}
#[test]
fn two_clients_predict_and_reconcile_with_bounded_payloads() {
    let col = floor(false);
    let mut room = duel();
    let mut sims = [
        StrikeSim::new(room.players[0].player.state.pos, 0.0, vec![], 0),
        StrikeSim::new(
            room.players[1].player.state.pos,
            core::f32::consts::PI,
            vec![],
            0,
        ),
    ];
    for sim in &mut sims {
        sim.network = Some(Client::new("fixture".into()));
    }
    for tick in 0..600 {
        for sim in &mut sims {
            sim.tick(
                &col,
                clock::TICK_SECONDS,
                &SimInput {
                    move_x: if tick < 100 { 1.0 } else { 0.0 },
                    ..Default::default()
                },
            );
        }
        if tick % 4 == 0 {
            for (i, sim) in sims.iter_mut().enumerate() {
                let client = sim.network.as_mut().unwrap();
                let request = client.request();
                assert!(request.len() < PAYLOAD_LIMIT);
                let reply = room.exchange(i, &request).unwrap();
                assert!(reply.len() < PAYLOAD_LIMIT);
                client.receive(&reply);
            }
        }
        room.step(&col);
    }
    for (i, sim) in sims.iter().enumerate() {
        assert_eq!(sim.network.as_ref().unwrap().status, "LIVE");
        assert!(
            sim.player
                .state
                .pos
                .distance(room.players[i].player.state.pos)
                < 1.0
        );
        assert_eq!(sim.bots.len(), 1);
    }
}
#[test]
fn usb_round_trip_does_not_accumulate_input_or_rejoin() {
    let col = floor(false);
    let mut room = duel();
    let mut sim = StrikeSim::new(room.players[0].player.state.pos, 0.0, vec![], 0);
    sim.network = Some(Client::new("fixture".into()));
    let other = exchange(&mut room, 1, 0, vec![]);
    let mut to_server: Option<(u32, String)> = None;
    let mut to_client: Option<(u32, String)> = None;
    let mut epoch = None;
    // One request in flight, 156 ms RTT, and authoritative execution after
    // receipt. An immediate-response fixture hides duplicate batch starvation.
    for tick in 0..64 * 30 {
        exchange(&mut room, 1, other.epoch, vec![]);
        if to_client.as_ref().is_some_and(|(due, _)| *due == tick) {
            let (_, raw) = to_client.take().unwrap();
            sim.network.as_mut().unwrap().receive(&raw);
        }
        sim.tick(
            &col,
            clock::TICK_SECONDS,
            &SimInput {
                move_x: if tick < 64 * 20 { 0.3 } else { 0.0 },
                ..Default::default()
            },
        );
        if to_server.as_ref().is_some_and(|(due, _)| *due == tick) {
            let (_, raw) = to_server.take().unwrap();
            let reply = room.exchange(0, &raw).unwrap();
            let state: Reply = serde_json::from_str(&reply).unwrap();
            assert_eq!(
                *epoch.get_or_insert(state.epoch),
                state.epoch,
                "unexpected rejoin"
            );
            to_client = Some((tick + 5, reply));
        }
        if to_server.is_none() && to_client.is_none() {
            let raw = sim.network.as_ref().unwrap().request();
            assert!(raw.len() <= PAYLOAD_LIMIT);
            to_server = Some((tick + 5, raw));
        }
        room.step(&col);
    }
    assert_eq!(room.round, 1);
    assert_eq!(sim.network.as_ref().unwrap().status, "LIVE");
    assert!(
        sim.player
            .state
            .pos
            .distance(room.players[0].player.state.pos)
            < 1.0
    );
}
#[test]
fn map_identity_covers_collision_and_topology_not_texture_tessellation() {
    use pocket3d_bsp::cooked::{tag, P3dWriter};
    let bytes = |collision: u8, texture: u8| {
        let mut w = P3dWriter::new();
        for (name, data) in [
            (b"WVTX", vec![texture]),
            (b"WCLP", vec![collision]),
            (b"WVIS", vec![2]),
            (b"WENT", vec![3]),
        ] {
            w.section(tag(name), data);
        }
        w.finish()
    };
    assert_eq!(map_key(&bytes(1, 0)), map_key(&bytes(1, 42)));
    assert_ne!(map_key(&bytes(1, 0)), map_key(&bytes(2, 0)));
    let map = bytes(1, 0);
    assert!(map_key(&map[..map.len() - 16]).is_err());
}
#[test]
fn client_handles_jitter_dropped_replies_disconnect_and_map_error() {
    let col = floor(false);
    let mut room = duel();
    let mut a = StrikeSim::new(room.players[0].player.state.pos, 0.0, vec![], 0);
    a.network = Some(Client::new("fixture".into()));
    let b = exchange(&mut room, 1, 0, vec![]);
    for tick in 0..800 {
        a.tick(
            &col,
            clock::TICK_SECONDS,
            &SimInput {
                move_x: if tick < 200 { 0.3 } else { 0.0 },
                ..Default::default()
            },
        );
        exchange(&mut room, 1, b.epoch, vec![]);
        if tick % 7 == 0 {
            let client = a.network.as_mut().unwrap();
            let reply = room.exchange(0, &client.request()).unwrap();
            if tick % 21 != 0 {
                client.receive(&reply);
            }
        }
        room.step(&col);
    }
    assert!(
        a.player
            .state
            .pos
            .distance(room.players[0].player.state.pos)
            < 1.0
    );
    for _ in 0..140 {
        a.tick(
            &col,
            clock::TICK_SECONDS,
            &SimInput {
                fire: true,
                move_y: 1.0,
                ..Default::default()
            },
        );
        room.step(&col);
    }
    assert_eq!(a.network.as_ref().unwrap().status, "RECONNECTING");
    let old_ammo = a.weapon.ammo;
    for _ in 0..20 {
        a.tick(
            &col,
            clock::TICK_SECONDS,
            &SimInput {
                fire: true,
                ..Default::default()
            },
        );
    }
    assert_eq!(a.weapon.ammo, old_ammo);
    let client = a.network.as_mut().unwrap();
    let reply = room.exchange(0, &client.request()).unwrap();
    client.receive(&reply);
    a.tick(&col, clock::TICK_SECONDS, &SimInput::default());
    a.network
        .as_mut()
        .unwrap()
        .receive("!map or protocol mismatch");
    a.tick(&col, clock::TICK_SECONDS, &SimInput::default());
    assert_eq!(a.network.as_ref().unwrap().status, "MAP MISMATCH");
    assert!(a.network.as_ref().unwrap().request().is_empty());
}
