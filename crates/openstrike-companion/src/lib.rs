//! Blocking networking adapter, owned exclusively by the PocketJS offload worker.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::Path,
    time::Duration,
};
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BridgeConfig {
    pub address: SocketAddr,
    pub key: String,
    pub slot: usize,
}
impl BridgeConfig {
    pub fn read(path: &Path) -> Result<Self> {
        let c: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        ensure!(
            c.address.ip().is_loopback()
                && c.key.len() == 64
                && c.key.bytes().all(|b| b.is_ascii_hexdigit())
                && c.slot < 2,
            "invalid local companion configuration"
        );
        Ok(c)
    }
}
pub struct Bridge {
    config: BridgeConfig,
    stream: Option<TcpStream>,
}
impl Bridge {
    pub fn new(config: BridgeConfig) -> Self {
        Self {
            config,
            stream: None,
        }
    }
    fn connect(&mut self) -> Result<()> {
        let mut stream =
            TcpStream::connect_timeout(&self.config.address, Duration::from_millis(250))?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(Duration::from_millis(500)))?;
        stream.set_write_timeout(Some(Duration::from_millis(500)))?;
        writeln!(
            stream,
            "{}",
            serde_json::json!({"key":self.config.key,"slot":self.config.slot})
        )?;
        self.stream = Some(stream);
        Ok(())
    }
    fn exchange(&mut self, raw: &str) -> Result<String> {
        ensure!(
            raw.len() <= 4096 && !raw.contains('\n'),
            "record budget exceeded"
        );
        if self.stream.is_none() {
            self.connect()?;
        }
        let stream = self.stream.as_mut().unwrap();
        stream.write_all(raw.as_bytes())?;
        stream.write_all(b"\n")?;
        let mut reply = Vec::with_capacity(2048);
        let mut byte = [0u8; 1];
        loop {
            stream.read_exact(&mut byte)?;
            if byte[0] == b'\n' {
                break;
            }
            ensure!(reply.len() < 4096, "reply budget exceeded");
            reply.push(byte[0]);
        }
        Ok(String::from_utf8(reply)?)
    }
    pub fn handle(&mut self, raw: &str) -> String {
        match self.exchange(raw) {
            Ok(reply) => reply,
            Err(_) => {
                self.stream = None;
                error_reply(raw, "Companion disconnected")
            }
        }
    }
}

/// Preserve request identity even when worker initialization or transport fails.
pub fn error_reply(raw: &str, error: &str) -> String {
    let id = serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|v| v["id"].as_u64())
        .unwrap_or(0);
    serde_json::json!({"id":id,"error":error}).to_string()
}
