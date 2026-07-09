//! The cooked-map catalogue: `maps/*.p3d` next to the EBOOT. Under PSPLINK
//! that is `host0:/maps/` (PPSSPP maps the EBOOT's own directory as host0:
//! too); from a Memory Stick it is `ms0:/PSP/GAME/OpenStrike/maps/`. Scanned
//! once at boot; files load into one reusable 16-aligned arena buffer.

use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_void;

use openstrike_core::StrikeSim;
use openstrike_core::sim::Command;
use pocket3d_bsp::cooked;
use pocket3d_gu::WorldRenderer;
use psp::sys::{self, IoOpenFlags};

use crate::Game;

const ROOTS: [&str; 2] = ["host0:/maps", "ms0:/PSP/GAME/OpenStrike/maps"];

static mut ACTIVE_ROOT: usize = 0;

fn zpath(dir: &str, name: &str, ext: &str) -> Vec<u8> {
    let mut p = Vec::with_capacity(dir.len() + name.len() + ext.len() + 2);
    p.extend_from_slice(dir.as_bytes());
    p.push(b'/');
    p.extend_from_slice(name.as_bytes());
    p.extend_from_slice(ext.as_bytes());
    p.push(0);
    p
}

/// Enumerate `<root>/maps/*.p3d` (first root that exists wins). Returns the
/// sorted map names (without extension) and the largest file size seen.
pub unsafe fn scan() -> (Vec<String>, u32) {
    for (ri, root) in ROOTS.iter().enumerate() {
        let mut zdir = Vec::from(root.as_bytes());
        zdir.push(0);
        let fd = sys::sceIoDopen(zdir.as_ptr());
        if fd.0 < 0 {
            continue;
        }
        let mut names: Vec<String> = Vec::new();
        let mut max_size: u32 = 0;
        loop {
            let mut ent: sys::SceIoDirent = core::mem::zeroed();
            if sys::sceIoDread(fd, &mut ent) <= 0 {
                break;
            }
            let raw = &ent.d_name;
            let len = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
            let name = core::str::from_utf8(
                core::slice::from_raw_parts(raw.as_ptr() as *const u8, len),
            )
            .unwrap_or("");
            // FAT-backed roots report 8.3-fitting names UPPERCASE
            // (DE_DUST2.P3D); every target filesystem here is
            // case-insensitive, so normalize to lowercase throughout.
            let lower = name.to_lowercase();
            if let Some(stem) = lower.strip_suffix(".p3d") {
                names.push(String::from(stem));
                let size = ent.d_stat.st_size as u32;
                if size > max_size {
                    max_size = size;
                }
            }
        }
        sys::sceIoDclose(fd);
        if !names.is_empty() {
            names.sort();
            ACTIVE_ROOT = ri;
            return (names, max_size);
        }
    }
    (Vec::new(), 0)
}

/// Load `<root>/maps/<name>.p3d` into the shared buffer and build the world:
/// cooked view, renderer, and a fresh simulation with the boot configuration
/// replayed. The caller guarantees no previous Game still borrows the buffer.
pub unsafe fn load(
    name: &str,
    buf_ptr: *mut u8,
    buf_cap: usize,
    boot_cfg: &[Command],
) -> Result<Game, &'static str> {
    let path = zpath(ROOTS[ACTIVE_ROOT], name, ".p3d");
    let fd = sys::sceIoOpen(path.as_ptr(), IoOpenFlags::RD_ONLY, 0o777);
    if fd.0 < 0 {
        return Err("map file missing");
    }
    let mut off = 0usize;
    loop {
        if off >= buf_cap {
            sys::sceIoClose(fd);
            return Err("map larger than the map buffer");
        }
        let n = sys::sceIoRead(
            fd,
            buf_ptr.add(off) as *mut c_void,
            (buf_cap - off) as u32,
        );
        if n < 0 {
            sys::sceIoClose(fd);
            return Err("map read failed");
        }
        if n == 0 {
            break;
        }
        off += n as usize;
    }
    sys::sceIoClose(fd);

    let data: &'static [u8] = core::slice::from_raw_parts(buf_ptr, off);
    pocket3d_gu::writeback(data);
    let map = cooked::read(data)?;
    if map.ct_spawns.is_empty() {
        return Err("map has no CT spawns");
    }
    let spawn = map.ct_spawns[0];
    let bot_spawns = if map.t_spawns.is_empty() {
        map.ct_spawns.clone()
    } else {
        map.t_spawns.clone()
    };
    let mut sim = StrikeSim::new(spawn.pos, spawn.yaw, bot_spawns, 3);
    for cmd in boot_cfg {
        sim.apply(cmd.clone(), 0);
    }
    let world = WorldRenderer::new(map);
    Ok(Game { sim, world })
}
