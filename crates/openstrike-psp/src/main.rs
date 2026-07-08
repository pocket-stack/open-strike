#![no_std]
#![no_main]
#![allow(static_mut_refs)]

//! OpenStrike on the PSP: the full composition.
//!
//! Per frame (one guest turn per tick, RUNTIMES.md Law 3):
//!   pad → SimInput → sim.tick → strike.__dispatch(state, events) →
//!   frame(buttons) → microtasks → drain strike commands → ui.tick →
//!   [pipelined present] → 3D pass (sky, world, bots, effects, viewmodel)
//!   → 2D HUD DrawList pass → kick.
//!
//! Boot skeleton follows the pocketjs-psp bin (2 MB VFPU worker thread,
//! arena allocator installed by linking the host library, pak fed to the
//! core before JS). The cooked map renders in place from `.rodata`.

extern crate alloc;

mod input;
mod present;
mod strike;

use core::ffi::c_void;

use libquickjs_sys::*;
use pocket3d_bsp::cooked;
use pocket3d_gu::{Camera3d, FramePool, WorldRenderer, sky};
use pocketjs_psp::{dbg, ffi, ge, host, pak};
#[cfg(feature = "capture")]
use psp::sys::DisplaySetBufSync;
#[cfg(feature = "capture")]
use psp::sys::DisplayPixelFormat;
#[cfg(feature = "capture")]
use psp::sys::{CtrlButtons, IoOpenFlags};
use psp::sys::{self, CtrlMode, GuContextType, GuSyncBehavior, GuSyncMode, SceCtrlData};
use psp::Align16;

use input::PadInput;
use openstrike_core::StrikeSim;

psp::module!("openstrike", 1, 1);

static APP_JS: &str = include_str!(concat!(env!("OUT_DIR"), "/game.js"));
static APP_PAK: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/app.pak"));
// 16-byte aligned: the GE reads vertices/indices/CLUTs/texels in place.
static MAP_P3D: Align16<[u8; include_bytes!(concat!(env!("OUT_DIR"), "/map.p3d")).len()]> =
    Align16(*include_bytes!(concat!(env!("OUT_DIR"), "/map.p3d")));

#[cfg(feature = "capture")]
static CAPTURE_INPUT: &str = env!("OPENSTRIKE_PSP_CAPTURE_INPUT");
#[cfg(feature = "capture")]
static CAP_START: &str = env!("OPENSTRIKE_PSP_CAP_START");
#[cfg(feature = "capture")]
static CAP_N: &str = env!("OPENSTRIKE_PSP_CAP_N");

const DT: f32 = 1.0 / 60.0;

// libquickjs-sys omits JS_NewArrayBuffer (local-extern pattern).
extern "C" {
    fn JS_NewArrayBuffer(
        ctx: *mut JSContext,
        buf: *mut u8,
        len: usize,
        free_func: Option<unsafe extern "C" fn(*mut JSRuntime, *mut c_void, *mut c_void)>,
        opaque: *mut c_void,
        is_shared: i32,
    ) -> JSValue;
}

fn psp_main() {
    unsafe {
        host::reset_fpu_status();
        host::run_on_worker(worker_main, run);
    }
}

unsafe extern "C" fn worker_main(_argc: usize, _argv: *mut c_void) -> i32 {
    host::reset_fpu_status();
    run();
    0
}

unsafe fn log_exception(ctx: *mut JSContext) {
    host::log_exception_with(ctx, |_| {});
}

unsafe fn run() {
    psp::enable_home_button();
    host::init_graphics(host::GfxConfig { depth: true });

    sys::sceCtrlSetSamplingCycle(0);
    sys::sceCtrlSetSamplingMode(CtrlMode::Analog);
    let mut pad_data = SceCtrlData::default();
    let mut pad = PadInput::new();

    // ---- UI core + assets (before any JS) ----
    let ui = ffi::init_ui();
    let (textures, sprites) = pak::feed(ui, APP_PAK);

    // ---- Map + game ----
    let map = match cooked::read(&MAP_P3D.0) {
        Ok(m) => m,
        Err(e) => host::halt(e),
    };
    pocket3d_gu::writeback(&MAP_P3D.0);
    if map.ct_spawns.is_empty() {
        host::halt("map has no CT spawns");
    }
    let spawn = map.ct_spawns[0];
    let bot_spawns = if map.t_spawns.is_empty() {
        map.ct_spawns.clone()
    } else {
        map.t_spawns.clone()
    };
    let mut sim = StrikeSim::new(spawn.pos, spawn.yaw, bot_spawns, 3);
    let mut world = WorldRenderer::new(map);
    let mut pool = FramePool::new();
    let sky_params = sky::SkyParams::default();
    let rifle = present::build_rifle();
    let bot_body = present::build_bot_body();

    // ---- QuickJS ----
    let rt = pocketjs_psp::qjs_alloc::new_runtime();
    if rt.is_null() {
        host::halt("JS_NewRuntime returned null");
    }
    let ctx = JS_NewContext(rt);
    if ctx.is_null() {
        host::halt("JS_NewContext returned null");
    }
    let global = JS_GetGlobalObject(ctx);
    dbg::init();
    ffi::register(ctx, global, &textures, &sprites);
    strike::register(ctx, global);
    if !APP_PAK.is_empty() {
        let ab = JS_NewArrayBuffer(
            ctx,
            APP_PAK.as_ptr() as *mut u8,
            APP_PAK.len(),
            None,
            core::ptr::null_mut(),
            0,
        );
        JS_SetPropertyStr(ctx, global, b"__pak\0".as_ptr() as *const _, ab);
    }

    let res = JS_Eval(
        ctx,
        APP_JS.as_ptr() as *const _,
        APP_JS.len() - 1, // exclude the trailing NUL
        b"openstrike.js\0".as_ptr() as *const _,
        JS_EVAL_TYPE_GLOBAL as i32,
    );
    if JS_ValueGetTag(res) == JS_TAG_EXCEPTION {
        log_exception(ctx);
        host::halt("JS_Eval threw");
    }
    JS_FreeValue(ctx, res);

    let frame_fn = JS_GetPropertyStr(ctx, global, b"frame\0".as_ptr() as *const _);
    if JS_IsUndefined(frame_fn) {
        host::halt("globalThis.frame is undefined");
    }

    // ---- Frame loop (pipelined present, one tick per vblank) ----
    let mut frame_count: u32 = 0;
    loop {
        sys::sceCtrlReadBufferPositive(&mut pad_data, 1);
        #[cfg_attr(not(feature = "capture"), allow(unused_mut))]
        let mut sample = (pad_data.buttons, pad_data.lx, pad_data.ly);
        #[cfg(feature = "capture")]
        {
            sample = capture_sample(frame_count, sample);
        }
        let tick = pad.map(sample.0, sample.1, sample.2, DT);
        let mask = sample.0.bits() as i32;

        // Simulation.
        sim.apply_look(tick.look_dx, tick.look_dy);
        sim.tick(&world.map().collision, DT, &tick.sim);

        // Guest turn: facts out, HUD frame, microtasks, intent in.
        if !strike::dispatch(ctx, global, &mut sim) {
            log_exception(ctx);
        }
        let mut args = [JS_NewInt32(ctx, mask)];
        let r = JS_Call(ctx, frame_fn, global, 1, args.as_mut_ptr());
        if JS_ValueGetTag(r) == JS_TAG_EXCEPTION {
            log_exception(ctx);
        }
        JS_FreeValue(ctx, r);
        host::drain_jobs(rt);
        strike::drain(|cmd| sim.apply(cmd, 0));

        // UI core frame.
        let ui = ffi::ui();
        ui.tick();
        let (words_ptr, words_len) = {
            let dl = ui.draw();
            (dl.words.as_ptr(), dl.words.len())
        };

        // Pipelined present: wait out frame N-1, show it, then record N.
        sys::sceGuSync(GuSyncMode::Finish, GuSyncBehavior::Wait);
        sys::sceDisplayWaitVblankStart();
        sys::sceGuSwapBuffers();
        #[cfg(feature = "capture")]
        if frame_count > 0 {
            cap_dump_frame(frame_count.wrapping_sub(1));
        }
        ge::reset_pool();
        pool.reset();

        sys::sceGuStart(GuContextType::Direct, host::list_ptr());
        let cam = Camera3d {
            pos: sim.player.eye_interpolated(1.0),
            yaw: sim.player.yaw,
            pitch: sim.player.pitch,
            fov_y: 74f32.to_radians(),
            ..Camera3d::default()
        };
        pocket3d_gu::begin_3d(&cam);
        sky::draw(&mut pool, &cam, &sky_params);
        world.draw(&mut pool, &cam);
        present::draw_bots(&mut pool, &bot_body, &sim.bots);
        present::draw_effects(&mut pool, &sim, &cam);
        present::draw_viewmodel(&mut pool, &rifle, &sim);
        pocket3d_gu::end_3d();
        // The JSX HUD, unchanged from every other PocketJS host.
        ge::render_over(ffi::ui(), core::slice::from_raw_parts(words_ptr, words_len));
        sys::sceGuFinish();

        frame_count = frame_count.wrapping_add(1);
    }
}

// ---------------------------------------------------------------------------
// Capture support (PPSSPPHeadless, scripts/e2e-psp.ts). The input script
// extends the PocketJS format with analog: `frame:mask:lx:ly` entries
// (lx/ly default to 128 = centered when omitted).
// ---------------------------------------------------------------------------

#[cfg(feature = "capture")]
fn cap_env_u32(s: &str, default: u32) -> u32 {
    s.parse::<u32>().unwrap_or(default)
}

#[cfg(feature = "capture")]
fn parse_num(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else {
        s.parse().ok()
    }
}

#[cfg(feature = "capture")]
fn capture_sample(
    frame: u32,
    fallback: (CtrlButtons, u8, u8),
) -> (CtrlButtons, u8, u8) {
    if CAPTURE_INPUT.is_empty() {
        return fallback;
    }
    let mut best_frame: Option<u32> = None;
    let mut best = fallback;
    for entry in CAPTURE_INPUT.split([',', ';']) {
        let mut it = entry.split(':');
        let (Some(f), Some(m)) = (it.next().and_then(parse_num), it.next().and_then(parse_num))
        else {
            continue;
        };
        let lx = it.next().and_then(parse_num).unwrap_or(128) as u8;
        let ly = it.next().and_then(parse_num).unwrap_or(128) as u8;
        if f <= frame && best_frame.is_none_or(|b| f >= b) {
            best_frame = Some(f);
            best = (CtrlButtons::from_bits_truncate(m), lx, ly);
        }
    }
    best
}

/// Dump the just-presented framebuffer (identical contract to the PocketJS
/// capture path: ms0:/dc_cap/fNNNN.raw, 512-stride RGBA, exit after window).
#[cfg(feature = "capture")]
unsafe fn cap_dump_frame(frame_count: u32) {
    let cap_start = cap_env_u32(CAP_START, 16);
    let cap_n = cap_env_u32(CAP_N, 32);
    if frame_count < cap_start || frame_count >= cap_start + cap_n {
        return;
    }
    let idx = frame_count - cap_start;
    if idx == 0 {
        sys::sceIoMkdir(b"ms0:/dc_cap\0".as_ptr(), 0o777);
    }
    let mut name: [u8; 22] = *b"ms0:/dc_cap/f0000.raw\0";
    let mut v = idx;
    let mut i = 16usize;
    loop {
        name[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if i == 13 {
            break;
        }
        i -= 1;
    }
    let mut top: *mut c_void = core::ptr::null_mut();
    let mut bw: usize = 0;
    let mut fmt = DisplayPixelFormat::Psm8888;
    sys::sceDisplayGetFrameBuf(&mut top, &mut bw, &mut fmt, DisplaySetBufSync::Immediate);
    let mut addr = top as u32;
    if addr < 0x0400_0000 {
        addr += 0x0400_0000;
    }
    addr |= 0x4000_0000;
    let fd = sys::sceIoOpen(
        name.as_ptr(),
        IoOpenFlags::CREAT | IoOpenFlags::WR_ONLY | IoOpenFlags::TRUNC,
        0o777,
    );
    if fd.0 >= 0 {
        sys::sceIoWrite(fd, addr as *const c_void, 512 * 272 * 4);
        sys::sceIoClose(fd);
    }
    if idx + 1 == cap_n {
        sys::sceKernelExitGame();
    }
}
