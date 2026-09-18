use anyhow::{Context, Result, ensure};
use openstrike_core::net::{Duel, map_key};
use serde::Deserialize;
use std::{
    io::{ErrorKind, Read, Write},
    net::{TcpListener, TcpStream},
    time::{Duration, Instant},
};
#[derive(Deserialize)]
struct Hello {
    key: String,
    slot: usize,
}
#[derive(Deserialize)]
struct Envelope {
    v: u8,
    id: u32,
    method: String,
    payload: String,
}
struct Connection {
    stream: TcpStream,
    slot: Option<usize>,
    input: Vec<u8>,
    output: Vec<u8>,
    written: usize,
    last: Instant,
}
impl Connection {
    fn poll(&mut self, room: &mut Duel, key: &str, occupied: [bool; 2]) -> Result<()> {
        ensure!(self.last.elapsed() < Duration::from_secs(3), "idle timeout");
        if self.written < self.output.len() {
            match self.stream.write(&self.output[self.written..]) {
                Ok(0) => anyhow::bail!("closed"),
                Ok(n) => self.written += n,
                Err(e) if e.kind() == ErrorKind::WouldBlock => return Ok(()),
                Err(e) => return Err(e.into()),
            }
            if self.written < self.output.len() {
                return Ok(());
            }
        }
        self.output.clear();
        self.written = 0;
        let mut bytes = [0u8; 4096];
        match self.stream.read(&mut bytes) {
            Ok(0) => anyhow::bail!("closed"),
            Ok(n) => {
                self.last = Instant::now();
                for &byte in &bytes[..n] {
                    if byte != b'\n' {
                        ensure!(self.input.len() < 4096, "oversized record");
                        self.input.push(byte);
                        continue;
                    }
                    if let Some(slot) = self.slot {
                        let request: Envelope = serde_json::from_slice(&self.input)?;
                        ensure!(
                            request.v == 1 && request.id > 0 && request.method == "strike.exchange",
                            "invalid capability"
                        );
                        let reply = match room.exchange(slot, &request.payload) {
                            Ok(payload) => serde_json::json!({"id":request.id,"payload":payload}),
                            Err(error) => serde_json::json!({"id":request.id,"error":error}),
                        };
                        let raw = reply.to_string();
                        ensure!(
                            self.output.len() + raw.len() + 1 <= 4096,
                            "reply queue full"
                        );
                        self.output.extend_from_slice(raw.as_bytes());
                        self.output.push(b'\n');
                    } else {
                        let hello: Hello = serde_json::from_slice(&self.input)?;
                        ensure!(
                            hello.key == key && hello.slot < 2 && !occupied[hello.slot],
                            "pairing rejected"
                        );
                        self.slot = Some(hello.slot);
                    }
                    self.input.clear();
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.into()),
        }
        ensure!(self.last.elapsed() < Duration::from_secs(3), "idle timeout");
        Ok(())
    }
}
fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let map_path = args
        .get(1)
        .context("usage: openstrike-companion map.p3d [port]")?;
    let port: u16 = args.get(2).map(String::as_str).unwrap_or("8748").parse()?;
    let key = std::env::var("OPENSTRIKE_COMPANION_KEY").context("pairing key is required")?;
    ensure!(
        key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid pairing key"
    );
    let bytes = std::fs::read(map_path)?;
    let map = pocket3d_bsp::cooked::read(&bytes).map_err(anyhow::Error::msg)?;
    let ct = *map.ct_spawns.first().context("map needs CT spawn")?;
    let t = *map.t_spawns.first().context("map needs T spawn")?;
    let map_id = map_key(&bytes).map_err(anyhow::Error::msg)?;
    let mut room = Duel::new(map_id.clone(), [ct, t]);
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    listener.set_nonblocking(true)?;
    println!(
        "Companion ready: {} map={} identity={}",
        listener.local_addr()?,
        map.name,
        map_id
    );
    let mut connections: Vec<Connection> = Vec::with_capacity(4);
    let step = Duration::from_nanos(1_000_000_000 / 64);
    let mut next = Instant::now() + step;
    loop {
        if connections.len() < 4 {
            match listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(true)?;
                    stream.set_nodelay(true)?;
                    connections.push(Connection {
                        stream,
                        slot: None,
                        input: Vec::with_capacity(4096),
                        output: Vec::with_capacity(4096),
                        written: 0,
                        last: Instant::now(),
                    });
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                Err(e) => return Err(e.into()),
            }
        }
        let mut i = 0;
        while i < connections.len() {
            let mut occupied = [false; 2];
            for (j, c) in connections.iter().enumerate() {
                if j != i {
                    if let Some(slot) = c.slot {
                        occupied[slot] = true;
                    }
                }
            }
            if connections[i].poll(&mut room, &key, occupied).is_err() {
                if let Some(slot) = connections[i].slot {
                    room.disconnect(slot);
                }
                connections.swap_remove(i);
            } else {
                i += 1;
            }
        }
        let now = Instant::now();
        let mut ticks = 0;
        while now >= next && ticks < 4 {
            room.step(&map.collision);
            next += step;
            ticks += 1;
        }
        if now >= next {
            next = now + step;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pocket3d_bsp::types::SpawnPoint;
    fn pair() -> (Connection, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let (stream, _) = listener.accept().unwrap();
        stream.set_nonblocking(true).unwrap();
        (
            Connection {
                stream,
                slot: None,
                input: Vec::new(),
                output: Vec::new(),
                written: 0,
                last: Instant::now(),
            },
            client,
        )
    }
    fn room() -> Duel {
        let spawn = SpawnPoint {
            pos: Default::default(),
            yaw: 0.0,
        };
        Duel::new("fixture".into(), [spawn; 2])
    }
    #[test]
    fn paired_socket_preserves_offload_ids_and_rejects_incompatible_maps() {
        let (mut connection, mut client) = pair();
        let mut room = room();
        let key = "7".repeat(64);
        writeln!(client, "{}", serde_json::json!({"key":key,"slot":0})).unwrap();
        let request = serde_json::json!({"v":1,"id":7,"method":"strike.exchange","payload":r#"{"v":1,"map":"wrong","epoch":0,"inputs":[]}"#});
        writeln!(client, "{request}").unwrap();
        for _ in 0..20 {
            connection.poll(&mut room, &key, [false; 2]).unwrap();
            std::thread::sleep(Duration::from_millis(1));
        }
        let mut response = [0; 4096];
        let n = client.read(&mut response).unwrap();
        let reply: serde_json::Value = serde_json::from_slice(&response[..n]).unwrap();
        assert_eq!(reply["id"], 7);
        assert_eq!(reply["error"], "map or protocol mismatch");
    }
    #[test]
    fn wrong_key_duplicate_slot_and_oversized_record_close_connection() {
        fn rejected(connection: &mut Connection, room: &mut Duel, occupied: bool) -> bool {
            for _ in 0..100 {
                if connection.poll(room, "key", [occupied, false]).is_err() {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            false
        }
        for (supplied, occupied) in [("wrong", false), ("key", true)] {
            let (mut connection, mut client) = pair();
            let mut room = room();
            writeln!(client, "{}", serde_json::json!({"key":supplied,"slot":0})).unwrap();
            assert!(rejected(&mut connection, &mut room, occupied));
        }
        let (mut connection, mut client) = pair();
        let mut room = room();
        client.write_all(&vec![b'x'; 4097]).unwrap();
        assert!(rejected(&mut connection, &mut room, false));
    }
}
