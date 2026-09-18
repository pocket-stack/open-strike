//! Bounded two-player Companion protocol and client prediction. The room runs
//! the same movement and rifle simulation as the local game at 64 Hz.
use crate::{
    clock::TICK_SECONDS, Bot, BotState, GameEvent, Phase, Player, SimInput, StrikeSim, Weapon,
};
use alloc::{collections::VecDeque, format, string::String, vec, vec::Vec};
use glam::Vec3;
use pocket3d_bsp::{collide::CharacterState, trace::MapCollision, types::SpawnPoint};
use serde::{Deserialize, Serialize};

pub const HISTORY: usize = 128;
pub const BATCH: usize = 12;
pub const PAYLOAD_LIMIT: usize = 2500;
const STALE_TICKS: u32 = 128;

/// Fingerprint the collision, hull-0 topology and spawn sections. Rendering
/// tessellation and texture resolution may differ between the two hosts.
pub fn map_key(bytes: &[u8]) -> Result<String, &'static str> {
    let read = |i: usize| -> Result<u32, &'static str> {
        Ok(u32::from_le_bytes(
            bytes
                .get(i..i + 4)
                .ok_or("truncated map")?
                .try_into()
                .unwrap(),
        ))
    };
    if bytes.get(..4) != Some(b"P3D1") || read(4)? != 1 {
        return Err("unsupported map");
    }
    let count = read(8)? as usize;
    if count > 32 {
        return Err("invalid section count");
    }
    let mut hash = 0xcbf29ce484222325u64;
    for tag in [b"WCLP", b"WVIS", b"WENT"] {
        let mut found = None;
        for i in 0..count {
            let entry = 16 + i * 16;
            if bytes.get(entry..entry + 4) == Some(tag) {
                if found.is_some() {
                    return Err("duplicate section");
                }
                let start = read(entry + 4)? as usize;
                let length = read(entry + 8)? as usize;
                found = Some(
                    bytes
                        .get(start..start.checked_add(length).ok_or("invalid section")?)
                        .ok_or("invalid section")?,
                );
            }
        }
        for byte in tag
            .iter()
            .chain(found.ok_or("missing collision section")?.iter())
        {
            hash = (hash ^ *byte as u64).wrapping_mul(0x100000001b3);
        }
    }
    Ok(format!("{hash:016x}"))
}

/// seq, strafe, forward, yaw, pitch, button bits. Fixed tick duration; clients
/// cannot choose elapsed time, speed, health, weapon damage or ammunition.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Input(pub u32, pub f32, pub f32, pub f32, pub f32, pub u8);
impl Input {
    fn capture(seq: u32, sim: &StrikeSim, input: &SimInput) -> Self {
        Self(
            seq,
            input.move_x,
            input.move_y,
            sim.player.yaw,
            sim.player.pitch,
            input.walk as u8
                | (input.jump as u8) << 1
                | (input.fire as u8) << 2
                | (input.reload as u8) << 3,
        )
    }
    fn valid(&self) -> bool {
        self.0 > 0
            && [self.1, self.2, self.3, self.4]
                .iter()
                .all(|n| n.is_finite())
            && self.1.abs() <= 1.0
            && self.2.abs() <= 1.0
            && self.3.abs() < 1e6
            && self.4.abs() <= core::f32::consts::FRAC_PI_2
            && self.5 < 16
    }
    fn sim(self) -> SimInput {
        SimInput {
            move_x: self.1,
            move_y: self.2,
            walk: self.5 & 1 != 0,
            jump: self.5 & 2 != 0,
            fire: self.5 & 4 != 0,
            reload: self.5 & 8 != 0,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub v: u8,
    pub map: String,
    pub epoch: u32,
    pub inputs: Vec<Input>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct State {
    pub pos: [f32; 3],
    pub vel: [f32; 3],
    pub angles: [f32; 2],
    pub ground: bool,
    pub hp: i32,
    pub ammo: u32,
    pub reserve: u32,
    pub reload: f32,
    pub cooldown: f32,
    pub recoil: f32,
    pub shot: f32,
    pub shots: u32,
    pub endpoint: [f32; 3],
    pub hits: u32,
    pub damage: i32,
    pub headshot: bool,
}
impl State {
    fn capture(
        sim: &StrikeSim,
        shots: u32,
        endpoint: Vec3,
        hits: u32,
        damage: i32,
        headshot: bool,
    ) -> Self {
        Self {
            pos: sim.player.state.pos.to_array(),
            vel: sim.player.state.vel.to_array(),
            angles: [sim.player.yaw, sim.player.pitch],
            ground: sim.player.state.on_ground,
            shots,
            endpoint: endpoint.to_array(),
            hits,
            damage,
            headshot,
            hp: sim.player.health,
            ammo: sim.weapon.ammo,
            reserve: sim.weapon.reserve,
            reload: sim.weapon.reload_left,
            cooldown: sim.weapon.cooldown,
            recoil: sim.weapon.recoil,
            shot: sim.weapon.shot_age,
        }
    }
    fn valid(&self) -> bool {
        self.pos
            .iter()
            .chain(&self.endpoint)
            .chain(&self.vel)
            .chain(&self.angles)
            .chain([&self.reload, &self.cooldown, &self.recoil, &self.shot])
            .all(|v| v.is_finite())
            && (0..=100).contains(&self.hp)
            && self.ammo <= 30
            && self.reserve <= 90
    }
    fn character(&self) -> CharacterState {
        CharacterState {
            pos: Vec3::from_array(self.pos),
            vel: Vec3::from_array(self.vel),
            on_ground: self.ground,
        }
    }
}
#[derive(Serialize, Deserialize)]
pub struct Reply {
    pub v: u8,
    pub epoch: u32,
    pub tick: u32,
    pub ack: u32,
    /// Inputs held by the authority, including those awaiting a fixed tick.
    pub received: u32,
    pub you: usize,
    pub round: u32,
    pub playing: bool,
    pub connected: [bool; 2],
    pub score: [u32; 2],
    pub players: [State; 2],
}

pub struct Client {
    map: String,
    epoch: u32,
    next: u32,
    received: u32,
    tick: u32,
    round: u32,
    age: u32,
    hits: u32,
    blocked: bool,
    history: VecDeque<Input>,
    pending: Option<Reply>,
    remote: Option<State>,
    pub status: &'static str,
    pub kills: u32,
    pub deaths: u32,
}
impl Client {
    pub fn new(map: String) -> Self {
        Self {
            map,
            epoch: 0,
            next: 1,
            received: 0,
            tick: 0,
            round: 0,
            age: 0,
            hits: 0,
            blocked: false,
            history: VecDeque::with_capacity(HISTORY),
            pending: None,
            remote: None,
            status: "CONNECTING",
            kills: 0,
            deaths: 0,
        }
    }
    pub fn request(&self) -> String {
        if self.blocked {
            return String::new();
        }
        let request = Request {
            v: 1,
            map: self.map.clone(),
            epoch: self.epoch,
            inputs: if self.epoch == 0 {
                Vec::new()
            } else {
                self.history
                    .iter()
                    .filter(|input| input.0 > self.received)
                    .take(BATCH)
                    .copied()
                    .collect()
            },
        };
        serde_json::to_string(&request).unwrap()
    }
    pub fn receive(&mut self, raw: &str) {
        if raw.len() > PAYLOAD_LIMIT {
            return;
        }
        if raw.starts_with('!') {
            if raw.contains("map or protocol mismatch") {
                self.blocked = true;
                self.status = "MAP MISMATCH";
                self.remote = None;
                self.history.clear();
            } else {
                self.fail();
            }
            return;
        }
        let Ok(reply) = serde_json::from_str::<Reply>(raw) else {
            self.status = "CONNECTION ERROR";
            return;
        };
        if reply.v != 1
            || reply.you > 1
            || reply.epoch == 0
            || reply.ack >= self.next
            || reply.received < reply.ack
            || reply.received >= self.next
            || !reply.players.iter().all(State::valid)
        {
            return;
        }
        if self.epoch != 0 && reply.epoch != self.epoch {
            return;
        }
        if self.epoch != 0 && reply.tick < self.tick {
            return;
        }
        self.pending = Some(reply);
    }
    pub fn fail(&mut self) {
        self.age = STALE_TICKS;
    }
    pub fn tick(&mut self, sim: &mut StrikeSim, col: &MapCollision, input: &SimInput) {
        self.age = self.age.saturating_add(1);
        if let Some(reply) = self.pending.take() {
            let joining = self.epoch == 0;
            self.age = 0;
            self.epoch = reply.epoch;
            self.tick = reply.tick;
            self.received = reply.received;
            let me = &reply.players[reply.you];
            let round_changed = self.round != reply.round;
            self.round = reply.round;
            if me.hits > self.hits && !joining && !round_changed {
                sim.events.push(GameEvent::Hit {
                    bot: 0,
                    headshot: me.headshot,
                    damage: me.damage,
                    fatal: reply.score[reply.you] > self.kills,
                });
            }
            self.hits = me.hits;
            self.kills = reply.score[reply.you];
            self.deaths = reply.score[1 - reply.you];
            self.status = if reply.playing {
                "LIVE"
            } else if reply.connected.iter().all(|v| *v) {
                "NEXT ROUND"
            } else {
                "WAITING FOR PLAYER"
            };
            self.history.retain(|entry| entry.0 > reply.ack);
            let yaw = sim.player.yaw;
            let pitch = sim.player.pitch;
            let previous_hp = sim.player.health;
            sim.player.state = me.character();
            sim.player.health = me.hp;
            sim.player.alive = me.hp > 0;
            sim.weapon.ammo = me.ammo;
            sim.weapon.reserve = me.reserve;
            sim.weapon.reload_left = me.reload;
            sim.weapon.cooldown = me.cooldown;
            sim.weapon.recoil = me.recoil;
            sim.weapon.shot_age = me.shot;
            if round_changed {
                self.history.clear();
                self.next = reply.ack + 1;
                sim.effects.clear();
                sim.player.yaw = me.angles[0];
                sim.player.pitch = me.angles[1];
            } else {
                for command in &self.history {
                    sim.player.yaw = command.3;
                    sim.player.pitch = command.4;
                    sim.tick_player_movement(
                        col,
                        TICK_SECONDS,
                        &command.sim(),
                        self.status != "LIVE",
                    );
                    sim.weapon.tick(TICK_SECONDS);
                    if command.5 & 8 != 0 {
                        sim.weapon.trigger_reload();
                    }
                    if command.5 & 4 != 0 && self.status == "LIVE" {
                        sim.weapon.fire();
                    }
                }
                sim.player.yaw = yaw;
                sim.player.pitch = pitch;
            }
            sim.player.prev_pos = sim.player.state.pos;
            if me.hp < previous_hp {
                sim.events.push(GameEvent::PlayerDamaged {
                    amount: previous_hp - me.hp,
                    hp: me.hp,
                });
            }
            let other = &reply.players[1 - reply.you];
            if self
                .remote
                .as_ref()
                .is_some_and(|old| other.shots > old.shots)
                && !round_changed
            {
                let from = Vec3::from_array(other.pos) + Vec3::Y * 20.0;
                sim.effects
                    .spawn(crate::EffectKind::MuzzleFlash { pos: from }, 0.08);
                sim.effects.spawn(
                    crate::EffectKind::Tracer {
                        a: from,
                        b: Vec3::from_array(other.endpoint),
                    },
                    0.09,
                );
            }
            self.remote =
                reply.connected[1 - reply.you].then(|| reply.players[1 - reply.you].clone());
        }
        if !self.blocked && (self.age >= STALE_TICKS || self.history.len() == HISTORY) {
            self.epoch = 0;
            self.next = 1;
            self.received = 0;
            self.tick = 0;
            self.round = 0;
            self.history.clear();
            self.remote = None;
            self.status = "RECONNECTING";
        }
        sim.bots.truncate(if self.remote.is_some() { 1 } else { 0 });
        if let Some(remote) = &self.remote {
            if sim.bots.is_empty() {
                sim.bots
                    .push(Bot::spawn(Vec3::from_array(remote.pos), remote.angles[0]));
            }
            let bot = &mut sim.bots[0];
            let target = Vec3::from_array(remote.pos);
            bot.prev_pos = bot.state.pos;
            bot.state.pos = if bot.state.pos.distance(target) > 256.0 {
                target
            } else {
                bot.state.pos.lerp(target, 0.25)
            };
            bot.state.vel = Vec3::from_array(remote.vel);
            bot.yaw = remote.angles[0];
            bot.health = remote.hp;
            bot.brain = if remote.hp > 0 {
                BotState::Patrol
            } else {
                BotState::Dead
            };
            bot.shot_age = remote.shot + self.age as f32 * TICK_SECONDS;
            bot.reload_remaining = (remote.reload - self.age as f32 * TICK_SECONDS).max(0.0);
            bot.anim.speed = bot.state.vel.length() / 90.0;
            bot.anim.time += TICK_SECONDS * bot.anim.speed;
            bot.idle_time += TICK_SECONDS;
            bot.death_time += TICK_SECONDS;
        }
        sim.phase = if self.status == "LIVE" {
            Phase::Live
        } else {
            Phase::Starting
        };
        if !self.blocked && self.epoch != 0 {
            self.history
                .push_back(Input::capture(self.next, sim, input));
            self.next = self.next.checked_add(1).unwrap_or(1);
        }
        let controls = if self.status == "LIVE" {
            *input
        } else {
            SimInput::default()
        };
        sim.tick_world(col, TICK_SECONDS, &controls, false, false);
    }
}

struct Peer {
    epoch: u32,
    last: u32,
    received: u32,
    ack: u32,
    input: VecDeque<Input>,
    connected: bool,
}
impl Peer {
    fn new() -> Self {
        Self {
            epoch: 0,
            last: 0,
            received: 0,
            ack: 0,
            input: VecDeque::with_capacity(HISTORY),
            connected: false,
        }
    }
}
/// Companion authority. Network callbacks enqueue bounded intent; `tick` is
/// driven by the server clock, never by request frequency.
pub struct Duel {
    map: String,
    pub players: [StrikeSim; 2],
    peers: [Peer; 2],
    spawns: [SpawnPoint; 2],
    shots: [u32; 2],
    endpoints: [Vec3; 2],
    hits: [u32; 2],
    damage: [i32; 2],
    headshot: [bool; 2],
    pub tick: u32,
    pub round: u32,
    pub score: [u32; 2],
    generation: u32,
    restart: u32,
    live: bool,
}
impl Duel {
    pub fn new(map: String, spawns: [SpawnPoint; 2]) -> Self {
        Self {
            map,
            players: core::array::from_fn(|i| {
                StrikeSim::new(spawns[i].pos, spawns[i].yaw, vec![spawns[1 - i]], 0)
            }),
            peers: core::array::from_fn(|_| Peer::new()),
            spawns,
            shots: [0; 2],
            endpoints: [Vec3::ZERO; 2],
            hits: [0; 2],
            damage: [0; 2],
            headshot: [false; 2],
            tick: 0,
            round: 0,
            score: [0; 2],
            generation: 0,
            restart: 0,
            live: false,
        }
    }
    pub fn exchange(&mut self, slot: usize, raw: &str) -> Result<String, &'static str> {
        if slot > 1 || raw.len() > PAYLOAD_LIMIT {
            return Err("request exceeds budget");
        }
        let req: Request = serde_json::from_str(raw).map_err(|_| "invalid request")?;
        if req.v != 1 || req.map != self.map {
            return Err("map or protocol mismatch");
        }
        if req.inputs.len() > BATCH || req.inputs.iter().any(|i| !i.valid()) {
            return Err("invalid input batch");
        }
        let peer = &mut self.peers[slot];
        if req.epoch == 0 {
            if !req.inputs.is_empty() {
                return Err("join cannot contain input");
            }
            {
                self.live = false;
                self.generation = self.generation.checked_add(1).ok_or("restart companion")?;
                *peer = Peer::new();
                peer.epoch = self.generation;
                peer.connected = true;
            }
        } else if !peer.connected || req.epoch != peer.epoch {
            return Err("session expired");
        }
        let mut next = peer.received;
        for command in &req.inputs {
            if command.0 > next {
                if command.0 != next + 1 {
                    return Err("input gap");
                }
                next = command.0;
            }
        }
        let fresh = req.inputs.iter().filter(|i| i.0 > peer.received).count();
        if peer.input.len() + fresh > HISTORY {
            return Err("input queue full");
        }
        for command in req.inputs {
            if command.0 > peer.received {
                peer.received = command.0;
                peer.input.push_back(command);
            }
        }
        peer.last = self.tick;
        let reply = Reply {
            v: 1,
            epoch: peer.epoch,
            tick: self.tick,
            ack: peer.ack,
            received: peer.received,
            you: slot,
            round: self.round,
            playing: self.live && self.restart == 0,
            connected: [self.peers[0].connected, self.peers[1].connected],
            score: self.score,
            players: core::array::from_fn(|i| {
                State::capture(
                    &self.players[i],
                    self.shots[i],
                    self.endpoints[i],
                    self.hits[i],
                    self.damage[i],
                    self.headshot[i],
                )
            }),
        };
        let raw = serde_json::to_string(&reply).map_err(|_| "invalid state")?;
        if raw.len() > PAYLOAD_LIMIT {
            return Err("snapshot exceeds budget");
        }
        Ok(raw)
    }
    pub fn disconnect(&mut self, slot: usize) {
        if let Some(peer) = self.peers.get_mut(slot) {
            peer.connected = false;
            peer.input.clear();
        }
    }
    fn reset(&mut self) {
        self.round += 1;
        self.restart = 0;
        for i in 0..2 {
            self.players[i].player = Player::spawn(self.spawns[i].pos, self.spawns[i].yaw);
            self.players[i].weapon = Weapon::default();
            self.players[i].effects.clear();
            self.players[i].events.clear();
            self.peers[i].input.clear();
            self.peers[i].ack = self.peers[i].received;
        }
    }
    pub fn step(&mut self, col: &MapCollision) {
        self.tick += 1;
        for peer in &mut self.peers {
            if self.tick.saturating_sub(peer.last) >= STALE_TICKS {
                peer.connected = false;
                peer.input.clear();
            }
        }
        let ready = self.peers.iter().all(|p| p.connected);
        if ready && !self.live {
            self.reset();
        }
        self.live = ready;
        if self.restart > 0 {
            self.restart -= 1;
            if self.restart == 0 {
                self.reset();
            }
        }
        let live = ready && self.restart == 0;
        // Capture both actors before either shot; simultaneous lethal shots
        // resolve in the same tick without giving slot zero an ordering edge.
        for i in 0..2 {
            let other = &self.players[1 - i].player;
            let mut bot = Bot::spawn(other.state.pos, other.yaw);
            bot.state = other.state;
            bot.health = other.health;
            bot.brain = if other.alive {
                BotState::Patrol
            } else {
                BotState::Dead
            };
            self.players[i].bots.clear();
            self.players[i].bots.push(bot);
        }
        let mut damage = [0; 2];
        for i in 0..2 {
            let input = self.peers[i].input.pop_front();
            let sim = &mut self.players[i];
            let controls = if let Some(input) = input {
                self.peers[i].ack = input.0;
                sim.player.yaw = input.3;
                sim.player.pitch = input.4;
                input.sim()
            } else {
                SimInput::default()
            };
            sim.phase = if live { Phase::Live } else { Phase::Starting };
            sim.events.clear();
            sim.tick_world(col, TICK_SECONDS, &controls, false, true);
            if sim.fired_this_tick {
                self.shots[i] += 1;
                if let Some(endpoint) = sim.effects.list.iter().rev().find_map(|e| match e.kind {
                    crate::EffectKind::Tracer { b, .. } => Some(b),
                    _ => None,
                }) {
                    self.endpoints[i] = endpoint;
                }
            }
            for event in &sim.events {
                if let GameEvent::Hit {
                    damage: amount,
                    headshot,
                    ..
                } = event
                {
                    damage[1 - i] += amount;
                    self.hits[i] += 1;
                    self.damage[i] = *amount;
                    self.headshot[i] = *headshot;
                }
            }
        }
        if live {
            for i in 0..2 {
                let player = &mut self.players[i].player;
                if player.alive && damage[i] > 0 {
                    player.health = (player.health - damage[i]).max(0);
                    player.alive = player.health > 0;
                    if !player.alive {
                        self.score[1 - i] += 1;
                        self.restart = 192;
                    }
                }
            }
        }
    }
}
