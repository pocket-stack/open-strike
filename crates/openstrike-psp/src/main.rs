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

#[cfg(feature = "character-bench")]
mod character_probe;
#[cfg(feature = "combat-bench")]
mod combat_probe;
#[cfg(any(
    all(feature = "combat-bench", feature = "character-bench"),
    all(feature = "motion-bench", feature = "combat-bench"),
    all(feature = "motion-bench", feature = "character-bench"),
    all(feature = "motion-bench", feature = "capture"),
    all(feature = "idle-bench", feature = "motion-bench"),
    all(feature = "idle-bench", feature = "combat-bench"),
    all(feature = "idle-bench", feature = "character-bench"),
    all(feature = "idle-bench", feature = "capture"),
))]
compile_error!("use one scripted benchmark/capture mode at a time");
mod input;
mod maps;
mod present;
mod strike;

use core::ffi::c_void;

use libquickjs_sys::*;
use pocket3d_gu::{sky, Camera3d, FramePool, WorldRenderer};
use pocketjs_psp::{dbg, ffi, ge, host, pak};
#[cfg(any(feature = "capture", feature = "motion-bench", feature = "idle-bench"))]
use psp::sys::CtrlButtons;
#[cfg(feature = "capture")]
use psp::sys::DisplayPixelFormat;
#[cfg(feature = "capture")]
use psp::sys::DisplaySetBufSync;
#[cfg(any(feature = "capture", feature = "bench"))]
use psp::sys::IoOpenFlags;
use psp::sys::{self, CtrlMode, GuContextType, GuSyncBehavior, GuSyncMode, SceCtrlData};

use input::PadInput;
use openstrike_core::clock::{FixedClock, TICK_SECONDS};
use openstrike_core::StrikeSim;

psp::module!("openstrike", 1, 1);

static APP_JS: &str = include_str!(concat!(env!("OUT_DIR"), "/game.js"));
static APP_PAK: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/app.pak"));
// Deterministic runs (e2e, bench) skip the menu and boot straight into this
// map; empty = boot into the menu (the shipped behavior).
static AUTOSTART: &str = env!("OPENSTRIKE_PSP_AUTOSTART");

#[cfg(feature = "capture")]
static CAPTURE_INPUT: &str = env!("OPENSTRIKE_PSP_CAPTURE_INPUT");
#[cfg(feature = "capture")]
static CAP_START: &str = env!("OPENSTRIKE_PSP_CAP_START");
#[cfg(feature = "capture")]
static CAP_N: &str = env!("OPENSTRIKE_PSP_CAP_N");

const DT: f32 = TICK_SECONDS;

use alloc::vec::Vec;
use openstrike_core::sim::Command;

/// A loaded world: the renderer borrows the (never-freed) map buffer, so the
/// borrow is 'static; the struct must be dropped before the buffer is
/// overwritten by the next load (see the frame loop's host_cmd handling).
struct Game {
    sim: StrikeSim,
    map_key: alloc::string::String,
    world: WorldRenderer<'static>,
}

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
    // Full clocks (PSPLINK sessions inherit 222 MHz; retail 3D games run 333).
    sys::scePowerSetClockFrequency(333, 333, 166);
    host::init_graphics(host::GfxConfig { depth: true });

    sys::sceCtrlSetSamplingCycle(0);
    sys::sceCtrlSetSamplingMode(CtrlMode::Analog);
    let mut pad_data = SceCtrlData::default();
    let mut pad = PadInput::new();

    // ---- UI core + assets (before any JS) ----
    let ui = ffi::init_ui();
    let (textures, sprites) = pak::feed(ui, APP_PAK);

    // ---- Map catalogue + reusable map buffer ----
    let (map_names, max_map_bytes) = maps::scan();
    if map_names.is_empty() {
        host::halt("no cooked maps found (maps/*.p3d next to the EBOOT)");
    }
    // One 16-aligned buffer sized for the largest map, reused across loads.
    // The backing memory is never freed (arena world), so treating it as
    // 'static is honest; soundness rule: the current Game (which borrows
    // it through CookedMap) is dropped before any reload overwrites it.
    // Recyclable Box/Vec allocations round up to a power-of-two size class:
    // an 18.2 MB map would reserve 32 MiB on a PSP-1000. The permanent arena
    // API consumes the requested bytes plus pointer-alignment padding only.
    let map_buf_cap = max_map_bytes as usize;
    let map_buf_ptr = pocketjs_psp::arena::alloc_permanent(map_buf_cap, 16);
    if map_buf_ptr.is_null() {
        host::halt("not enough memory for the largest cooked map");
    }

    let mut pool = FramePool::new();
    let sky_params = sky::SkyParams::default();
    let mut active_mod = openstrike_mods::INITIAL;
    let mut rifle = present::build_viewmodel(openstrike_mods::get(active_mod).unwrap());
    let mut effects = present::EffectRenderer::new();
    let mut projectile_mesh = present::build_projectile(openstrike_mods::get(active_mod).unwrap());
    let mut officers = Some(present::CharacterRenderer::new(
        openstrike_mods::get(active_mod).unwrap().character(),
    ));

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
    strike::register(ctx, global, &map_names);
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

    // Commands issued while no world exists (rules.ts configures the weapon
    // and bots at eval time) are game CONFIGURATION: keep them and replay
    // into every freshly loaded simulation.
    let mut boot_cfg: Vec<Command> = Vec::new();
    // Configuration issued during guest startup also applies to autostart's
    // first round, before the first simulation tick or rendered frame.
    strike::drain(|cmd| openstrike_core::sim::retain_configuration(&mut boot_cfg, cmd));
    let mut game: Option<Game> = None;
    let mut menu_time: f64 = 0.0;
    if !AUTOSTART.is_empty() {
        let idx = map_names.iter().position(|n| n == AUTOSTART);
        let Some(idx) = idx else {
            host::halt("autostart map not in the catalogue");
        };
        match maps::load(&map_names[idx], map_buf_ptr, map_buf_cap, &boot_cfg) {
            Ok(mut g) => {
                openstrike_mods::get(active_mod)
                    .unwrap()
                    .configure(&mut g.sim);
                game = Some(g);
            }
            Err(e) => host::halt(e),
        }
    }

    // ---- Fixed simulation clock; presentation may skip display refreshes. ----
    let mut last_gc_bump = 0usize;
    let mut clock = FixedClock::new(sys::sceKernelGetSystemTimeWide() as u64);
    #[cfg(feature = "bench")]
    let mut tick_count = 0u64;
    let mut frame_count: u32 = 0;
    let mut last_present_vcount = sys::sceDisplayGetVcount();
    #[cfg(feature = "bench")]
    let mut bench = Bench::new();
    #[cfg(feature = "combat-bench")]
    let mut combat = combat_probe::CombatProbe::new();
    loop {
        #[cfg(feature = "bench")]
        let bench_t0 = bench_now();
        // Peek avoids waiting for another controller sampling interrupt.
        sys::sceCtrlPeekBufferPositive(&mut pad_data, 1);
        #[cfg(not(feature = "idle-bench"))]
        #[cfg_attr(not(feature = "capture"), allow(unused_mut))]
        let mut sample = (pad_data.buttons, pad_data.lx, pad_data.ly);
        #[cfg(feature = "capture")]
        {
            sample = capture_sample(frame_count, sample);
        }
        #[cfg(feature = "idle-bench")]
        let sample = (CtrlButtons::empty(), 128, 128);
        #[cfg(feature = "bench")]
        let mut observed_buttons = sample.0.bits();
        #[cfg(not(feature = "capture"))]
        let ticks = clock.advance(sys::sceKernelGetSystemTimeWide() as u64);
        // Capture scripts remain deterministic: one fixed tick per captured frame.
        #[cfg(feature = "capture")]
        let ticks = 1;
        #[cfg(feature = "bench")]
        let mut segments = [0u64; 4];
        let mut _tick = input::TickInput::default();
        let mut host_cmd: Option<strike::HostCmd> = None;
        #[cfg(feature = "bench")]
        let ticks_before = tick_count;
        for _ in 0..ticks {
            #[cfg(feature = "bench")]
            let sim_start = bench_now();
            #[cfg(feature = "motion-bench")]
            let sample = motion_sample(tick_count);
            #[cfg(feature = "bench")]
            {
                observed_buttons = sample.0.bits();
            }
            _tick = pad.map(sample.0, sample.1, sample.2, DT);
            if let Some(g) = &mut game {
                #[cfg(feature = "combat-bench")]
                {
                    _tick.sim = combat.input(&mut g.sim);
                    _tick.look_dx = 0.0;
                    _tick.look_dy = 0.0;
                }
                #[cfg(not(feature = "character-bench"))]
                {
                    for bot in &mut g.sim.bots {
                        bot.muzzle_local = officers.as_ref().unwrap().asset.attack_origin();
                    }
                    g.sim.apply_look(_tick.look_dx, _tick.look_dy);
                    g.sim.tick(&g.world.map().collision, DT, &_tick.sim);
                }
                #[cfg(feature = "character-bench")]
                {
                    g.sim.time += DT;
                }
            } else {
                menu_time += DT as f64;
            }
            #[cfg(feature = "bench")]
            let after_sim = bench_now();
            #[cfg(feature = "bench")]
            if let Some(g) = &game {
                bench.observe_events(&g.sim);
            }

            // Preserve one complete guest turn and UI tick per simulation step.
            let ok = match &mut game {
                Some(g) => strike::dispatch(ctx, global, &mut g.sim),
                None => strike::dispatch_menu(ctx, global, menu_time),
            };
            if !ok {
                log_exception(ctx);
            }
            #[cfg(feature = "bench")]
            let after_dispatch = bench_now();
            pocketjs_psp::offload::frame(sample.0.bits(), (sample.1 as u32) << 8 | sample.2 as u32);
            let mut args = [JS_NewInt32(ctx, sample.0.bits() as i32)];
            let r = JS_Call(ctx, frame_fn, global, 1, args.as_mut_ptr());
            if JS_ValueGetTag(r) == JS_TAG_EXCEPTION {
                log_exception(ctx);
            }
            JS_FreeValue(ctx, r);
            #[cfg(feature = "bench")]
            let after_js = bench_now();
            host::drain_jobs(rt);
            strike::drain(|cmd| match &mut game {
                Some(g) => g.sim.apply(cmd, 0),
                None => openstrike_core::sim::retain_configuration(&mut boot_cfg, cmd),
            });
            #[cfg(feature = "character-bench")]
            if let Some(g) = &mut game {
                character_probe::stage(&mut g.sim, tick_count as u32);
            }
            strike::drain_host(|c| host_cmd = Some(c));
            ffi::ui().tick();
            #[cfg(feature = "bench")]
            {
                tick_count += 1;
                segments[0] += after_sim - sim_start;
                segments[1] += after_dispatch - after_sim;
                segments[2] += after_js - after_dispatch;
                segments[3] += bench_now() - after_js;
            }
            // A transition applies after this frame; do not tick the old world again.
            if host_cmd.is_some() {
                break;
            }
        }
        // Match the PocketJS PSP host's arena-pressure cycle collection.
        // Component unmounts release owners but can leave JS reference cycles.
        // Collect outside guest turns only after fresh arena growth; ordinary
        // frames reuse free-list blocks and do not run a collector.
        let bump = pocketjs_psp::arena::stats().bump_bytes;
        if bump > last_gc_bump.saturating_add(256 * 1024) {
            JS_RunGC(rt);
            last_gc_bump = pocketjs_psp::arena::stats().bump_bytes;
        }
        #[cfg(feature = "bench")]
        let ui_draw_start = bench_now();
        let ui = ffi::ui();
        let (words_ptr, words_len) = {
            let dl = ui.draw();
            (dl.words.as_ptr(), dl.words.len())
        };

        #[cfg(feature = "bench")]
        {
            bench.ui_draw_sum += bench_now() - ui_draw_start;
        }

        // Pipelined present: wait out frame N-1, show it, then record N.
        #[cfg(feature = "bench")]
        let bench_before_sync = bench_now();
        sys::sceGuSync(GuSyncMode::Finish, GuSyncBehavior::Wait);
        #[cfg(feature = "bench")]
        let bench_after_sync = bench_now();
        // A completed frame may already be inside a NEW blanking interval.
        // Waiting for another start would discard that refresh opportunity.
        // The vcount guard still permits at most one present per
        // refresh when a very cheap frame finishes within the same blank.
        let current_vcount = sys::sceDisplayGetVcount();
        if current_vcount == last_present_vcount {
            sys::sceDisplayWaitVblankStart();
        } else {
            sys::sceDisplayWaitVblank();
        }
        let presented_vcount = sys::sceDisplayGetVcount();
        #[cfg(feature = "bench")]
        {
            bench.reused_vblanks += (presented_vcount == current_vcount) as u32;
            bench.missed_vblanks += presented_vcount
                .wrapping_sub(last_present_vcount)
                .saturating_sub(1);
        }
        last_present_vcount = presented_vcount;
        sys::sceGuSwapBuffers();
        #[cfg(feature = "bench")]
        let bench_after_present = bench_now();
        #[cfg(feature = "capture")]
        if frame_count > 0 {
            cap_dump_frame(frame_count.wrapping_sub(1));
        }
        ge::reset_pool();
        pool.reset();

        sys::sceGuStart(GuContextType::Direct, host::list_ptr());
        let cam = match &game {
            Some(g) => Camera3d {
                pos: g.sim.player.eye_interpolated(1.0),
                yaw: g.sim.player.yaw,
                pitch: g.sim.player.pitch,
                fov_y: 74f32.to_radians(),
                ..Camera3d::default()
            },
            None => Camera3d {
                fov_y: 74f32.to_radians(),
                ..Camera3d::default()
            },
        };
        pocket3d_gu::begin_3d(&cam);
        sky::draw(&mut pool, &cam, &sky_params);
        #[cfg(feature = "bench")]
        let mut actor_us = 0;
        if let Some(g) = &mut game {
            g.world.draw(&mut pool, &cam);
            #[cfg(feature = "bench")]
            let actor_start = bench_now();
            officers
                .as_mut()
                .unwrap()
                .draw(&mut pool, &g.sim.bots, &cam);
            #[cfg(feature = "bench")]
            {
                actor_us = bench_now() - actor_start;
            }
            effects.prepare(&g.sim, &cam);
            present::draw_projectiles(&mut pool, &projectile_mesh, &g.sim);
            effects.draw_world(&mut pool);
            present::draw_viewmodel(&mut pool, &rifle, &g.sim, &effects);
        }
        pocket3d_gu::end_3d();
        // The JSX HUD, unchanged from every other PocketJS host.
        #[cfg(feature = "bench")]
        let ui_ge_start = bench_now();
        ge::render_over(ffi::ui(), core::slice::from_raw_parts(words_ptr, words_len));
        #[cfg(feature = "bench")]
        {
            bench.ui_ge_sum += bench_now() - ui_ge_start;
            bench.hud_script_sum += strike::take_hud_time();
        }
        sys::sceGuFinish();

        #[cfg(feature = "bench")]
        {
            let (faces, tris, indices) = match &game {
                Some(g) => (g.world.last_faces, g.world.last_tris, g.world.last_indices),
                None => (0, 0, 0),
            };
            bench.observe(observed_buttons, &_tick, game.as_ref().map(|g| &g.sim));
            bench.sim_ticks += (tick_count - ticks_before) as u32;
            bench.record(
                frame_count,
                bench_t0,
                &segments,
                bench_before_sync,
                bench_after_sync,
                bench_after_present,
                faces,
                tris,
                indices,
                actor_us,
                if game.is_some() {
                    officers.as_ref().unwrap().visible
                } else {
                    0
                },
                officers.as_ref().unwrap().asset.triangle_count(),
            );
        }

        let transition = host_cmd.is_some();
        if transition {
            // The GE still borrows this frame's world textures/vertices. Finish
            // before freeing a world or overwriting the shared map buffer.
            sys::sceGuSync(GuSyncMode::Finish, GuSyncBehavior::Wait);
        }
        // World lifecycle intents, applied OUTSIDE all world borrows (the
        // presented frame already showed the menu's LOADING state).
        match host_cmd {
            Some(strike::HostCmd::LoadMap {
                map: i,
                mod_index,
                crossplay,
            }) if game.is_none() => {
                if let Some(name) = map_names.get(i) {
                    match maps::load(name, map_buf_ptr, map_buf_cap, &boot_cfg) {
                        Ok(mut g) => {
                            let pack = openstrike_mods::get(mod_index).unwrap();
                            if active_mod != mod_index {
                                // The GE has finished. Release the old cache before
                                // allocating another full-detail character cache.
                                drop(officers.take());
                                officers = Some(present::CharacterRenderer::new(pack.character()));
                                rifle = present::build_viewmodel(pack);
                                projectile_mesh = present::build_projectile(pack);
                                active_mod = mod_index;
                            }
                            pack.configure(&mut g.sim);
                            if crossplay {
                                g.sim.bots.clear();
                                g.sim.network =
                                    Some(openstrike_core::net::Client::new(g.map_key.clone()));
                            }
                            game = Some(g);
                            #[cfg(feature = "bench")]
                            {
                                bench.map_loads += 1;
                            }
                        }
                        Err(e) => psp::dprintln!("[openstrike] loadMap failed: {}", e),
                    }
                }
            }
            Some(strike::HostCmd::ToMenu) => {
                #[cfg(feature = "bench")]
                {
                    bench.menu_returns += 1;
                }
                game = None; // drops every borrow of the map buffer
                menu_time = 0.0;
            }
            _ => {}
        }

        if transition {
            clock.reset(sys::sceKernelGetSystemTimeWide() as u64);
        }
        frame_count = frame_count.wrapping_add(1);
    }
}

// ---------------------------------------------------------------------------
// Bench (hardware smoothness evidence): rolling 300-frame windows appended
// as JSONL to host0:/OpenStrike-bench.jsonl (PSPLINK) and ms0:. work_us is
// CPU per frame (present wait excluded); gpu_us is the sceGuSync wait —
// budget 16667 us each at 60 Hz.
// ---------------------------------------------------------------------------

#[cfg(feature = "bench")]
fn bench_now() -> u64 {
    unsafe { sys::sceKernelGetSystemTimeWide() as u64 }
}

#[cfg(feature = "bench")]
struct Bench {
    frames: u32,
    sim_ticks: u32,
    map_loads: u32,
    menu_returns: u32,
    window: u32,
    work_sum: u64,
    max_work: u64,
    gpu_sum: u64,
    max_gpu: u64,
    faces_sum: u64,
    tris_sum: u64,
    indices_sum: u64,
    seg_sums: [u64; 4],
    /// Breakdown of the frame that set max_work: 4 segments + the
    /// uninstrumented rest (draw recording, input, GE list build).
    max_segs: [u64; 5],
    hud_script_sum: u64,
    ui_draw_sum: u64,
    ui_ge_sum: u64,
    actor_sum: u64,
    actor_max: u64,
    actor_count_sum: u64,
    actor_count_max: u32,
    last_start: u64,
    frame_sum: u64,
    frame_samples: u32,
    frame_times: [u32; 300],
    work_times: [u32; 300],
    late_frames: u32,
    buttons_or: u32,
    analog_frames: u32,
    movement_frames: u32,
    airborne_frames: u32,
    look_frames: u32,
    reload_frames: u32,
    ammo_min: u32,
    ammo_max: u32,
    previous_pos: Option<glam::Vec3>,
    clip_frames: [u32; 7],
    events: [u32; 6],
    reused_vblanks: u32,
    missed_vblanks: u32,
    max_work_frame: u32,
}

#[cfg(feature = "bench")]
impl Bench {
    fn new() -> Self {
        // Truncate old logs once at boot.
        for path in [
            b"host0:/OpenStrike-bench.jsonl\0".as_ptr(),
            b"ms0:/OpenStrike-bench.jsonl\0".as_ptr(),
        ] {
            unsafe {
                let fd = sys::sceIoOpen(
                    path,
                    IoOpenFlags::WR_ONLY | IoOpenFlags::CREAT | IoOpenFlags::TRUNC,
                    0o777,
                );
                if fd.0 >= 0 {
                    sys::sceIoClose(fd);
                }
            }
        }
        Self {
            frames: 0,
            sim_ticks: 0,
            map_loads: 0,
            menu_returns: 0,
            window: 0,
            work_sum: 0,
            max_work: 0,
            gpu_sum: 0,
            max_gpu: 0,
            faces_sum: 0,
            tris_sum: 0,
            indices_sum: 0,
            seg_sums: [0; 4],
            max_segs: [0; 5],
            hud_script_sum: 0,
            ui_draw_sum: 0,
            ui_ge_sum: 0,
            actor_sum: 0,
            actor_max: 0,
            actor_count_sum: 0,
            actor_count_max: 0,
            last_start: 0,
            frame_sum: 0,
            frame_samples: 0,
            frame_times: [0; 300],
            work_times: [0; 300],
            late_frames: 0,
            buttons_or: 0,
            analog_frames: 0,
            movement_frames: 0,
            airborne_frames: 0,
            look_frames: 0,
            reload_frames: 0,
            ammo_min: u32::MAX,
            ammo_max: 0,
            previous_pos: None,
            clip_frames: [0; 7],
            events: [0; 6],
            reused_vblanks: 0,
            missed_vblanks: 0,
            max_work_frame: 0,
        }
    }

    fn observe_events(&mut self, sim: &StrikeSim) {
        use openstrike_core::sim::GameEvent;
        self.events[5] += sim.fired_this_tick as u32;
        for event in &sim.events {
            match event {
                GameEvent::Hit { fatal, .. } => {
                    self.events[0] += 1;
                    self.events[1] += *fatal as u32;
                }
                GameEvent::PlayerDamaged { .. } => self.events[2] += 1,
                GameEvent::PlayerDied => self.events[3] += 1,
                GameEvent::RoundReset => self.events[4] += 1,
            }
        }
    }

    fn observe(
        &mut self,
        buttons: u32,
        input: &input::TickInput,
        sim: Option<&openstrike_core::StrikeSim>,
    ) {
        self.buttons_or |= buttons;
        let moving = input.sim.move_x != 0.0 || input.sim.move_y != 0.0;
        self.analog_frames += moving as u32;
        self.look_frames += (input.look_dx != 0.0 || input.look_dy != 0.0) as u32;
        if let Some(sim) = sim {
            self.airborne_frames += (!sim.player.state.on_ground) as u32;
            let pos = sim.player.state.pos;
            if let Some(previous) = self.previous_pos {
                let delta = pos.distance_squared(previous);
                // Exclude round teleports; retain physical analog movement.
                self.movement_frames += (moving && delta > 0.01 && delta < 100.0) as u32;
            }
            self.previous_pos = Some(pos);
            self.reload_frames += sim.weapon.reloading() as u32;
            self.ammo_min = self.ammo_min.min(sim.weapon.ammo);
            self.ammo_max = self.ammo_max.max(sim.weapon.ammo);
            for bot in &sim.bots {
                self.clip_frames[bot.animation_sample().0 as usize] += 1;
            }
        } else {
            self.previous_pos = None;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn record(
        &mut self,
        abs_frame: u32,
        t0: u64,
        segments: &[u64; 4],
        before_sync: u64,
        after_sync: u64,
        after_present: u64,
        faces: u32,
        tris: u32,
        indices: u32,
        actor_us: u64,
        actors: u32,
        actor_triangles: usize,
    ) {
        let now = bench_now();
        let mut segs = [0u64; 5];
        for (i, &duration) in segments.iter().enumerate() {
            segs[i] = duration;
            self.seg_sums[i] += duration;
        }
        let present = after_present.saturating_sub(before_sync);
        let work = now.saturating_sub(t0).saturating_sub(present);
        let gpu = after_sync.saturating_sub(before_sync);
        segs[4] = work.saturating_sub(segs[0] + segs[1] + segs[2] + segs[3]);
        let interval = if self.last_start == 0 {
            0
        } else {
            t0.saturating_sub(self.last_start)
        };
        self.last_start = t0;
        if interval > 0 {
            self.frame_sum += interval;
            self.frame_samples += 1;
        }
        self.frame_times[self.frames as usize] = interval.min(u32::MAX as u64) as u32;
        self.work_times[self.frames as usize] = work.min(u32::MAX as u64) as u32;
        self.late_frames += (interval > 20_000) as u32;
        self.actor_sum += actor_us;
        self.actor_max = self.actor_max.max(actor_us);
        self.actor_count_sum += actors as u64;
        self.actor_count_max = self.actor_count_max.max(actors);
        self.frames += 1;
        self.work_sum += work;
        if work > self.max_work {
            self.max_work = work;
            self.max_segs = segs;
            self.max_work_frame = abs_frame;
        }
        // Explicit forensics only: writing both USB and Memory Stick for
        // every slow frame adds stalls and simulation catch-up to the next
        // frame. Normal benchmarks retain the 300-frame summary and maxima.
        #[cfg(feature = "bench-spikes")]
        if work > 25_000 {
            let line = alloc::format!(
                "{{\"spike_frame\":{},\"work_us\":{},\"segs_us\":[{},{},{},{},{}]}}\n",
                abs_frame,
                work,
                segs[0],
                segs[1],
                segs[2],
                segs[3],
                segs[4],
            );
            for path in [
                b"host0:/OpenStrike-bench.jsonl\0".as_ptr(),
                b"ms0:/OpenStrike-bench.jsonl\0".as_ptr(),
            ] {
                unsafe {
                    let fd = sys::sceIoOpen(
                        path,
                        IoOpenFlags::WR_ONLY | IoOpenFlags::CREAT | IoOpenFlags::APPEND,
                        0o777,
                    );
                    if fd.0 >= 0 {
                        sys::sceIoWrite(fd, line.as_ptr() as *const c_void, line.len());
                        sys::sceIoClose(fd);
                    }
                }
            }
        }
        self.gpu_sum += gpu;
        self.max_gpu = self.max_gpu.max(gpu);
        self.faces_sum += faces as u64;
        self.tris_sum += tris as u64;
        self.indices_sum += indices as u64;
        if self.frames < 300 {
            return;
        }
        let n = self.frames as u64;
        self.window += 1;
        let arena = unsafe { pocketjs_psp::arena::stats() };
        let (arena_free, _) = unsafe { pocketjs_psp::arena::debug_free() };
        let js_live = unsafe { pocketjs_psp::qjs_alloc::stats().live_requested };
        self.work_times.sort_unstable();
        self.frame_times.sort_unstable();
        let line = alloc::format!(
            "{{\"window\":{},\"frames\":{},\"sim_ticks\":{},\"map_loads\":{},\"menu_returns\":{},\"avg_work_us\":{},\"max_work_us\":{},\"avg_gpu_us\":{},\"max_gpu_us\":{},\"avg_faces\":{},\"avg_tris\":{},\"avg_indices\":{},\"avg_sim_us\":{},\"avg_dispatch_us\":{},\"avg_js_us\":{},\"avg_ui_us\":{},\"arena_capacity_bytes\":{},\"arena_bump_bytes\":{},\"arena_tail_free_bytes\":{},\"arena_total_free_bytes\":{},\"js_live_requested_bytes\":{},\"max_segs_us\":[{},{},{},{},{}],\"actor_probe\":{},\"avg_actor_us\":{},\"max_actor_us\":{},\"avg_actors\":{},\"max_actors\":{},\"actor_triangles_each\":{},\"observed_fps_milli\":{},\"p95_frame_us\":{},\"p99_frame_us\":{},\"p95_work_us\":{},\"late_frames\":{},\"input\":{{\"buttons_or\":{},\"analog_frames\":{},\"movement_frames\":{},\"airborne_frames\":{},\"look_frames\":{},\"ammo_min\":{},\"ammo_max\":{},\"reloading_frames\":{}}},\"actor_clip_frames\":[{},{},{},{},{},{},{}],\"combat_probe\":{},\"events\":{{\"hits\":{},\"kills\":{},\"damage\":{},\"deaths\":{},\"resets\":{},\"shots\":{}}},\"reused_vblanks\":{},\"missed_vblanks\":{},\"avg_hud_script_us\":{},\"avg_ui_draw_us\":{},\"avg_ui_ge_us\":{},\"max_work_frame\":{}}}\n",
            self.window,
            n,
            self.sim_ticks,
            self.map_loads,
            self.menu_returns,
            self.work_sum / n,
            self.max_work,
            self.gpu_sum / n,
            self.max_gpu,
            self.faces_sum / n,
            self.tris_sum / n,
            self.indices_sum / n,
            self.seg_sums[0] / n,
            self.seg_sums[1] / n,
            self.seg_sums[2] / n,
            self.seg_sums[3] / n,
            arena.capacity_bytes,
            arena.bump_bytes,
            arena.tail_free_bytes,
            arena_free,
            js_live,
            self.max_segs[0],
            self.max_segs[1],
            self.max_segs[2],
            self.max_segs[3],
            self.max_segs[4],
            cfg!(feature = "character-bench"),
            self.actor_sum / n,
            self.actor_max,
            self.actor_count_sum / n,
            self.actor_count_max,
            actor_triangles,
            self.frame_samples as u64 * 1_000_000_000 / self.frame_sum.max(1),
            self.frame_times[284],
            self.frame_times[296],
            self.work_times[284],
            self.late_frames,
            self.buttons_or,
            self.analog_frames,
            self.movement_frames,
            self.airborne_frames,
            self.look_frames,
            if self.ammo_min == u32::MAX {
                0
            } else {
                self.ammo_min
            },
            self.ammo_max,
            self.reload_frames,
            self.clip_frames[0],
            self.clip_frames[1],
            self.clip_frames[2],
            self.clip_frames[3],
            self.clip_frames[4],
            self.clip_frames[5],
            self.clip_frames[6],
            cfg!(feature = "combat-bench"),
            self.events[0],
            self.events[1],
            self.events[2],
            self.events[3],
            self.events[4],
            self.events[5],
            self.reused_vblanks,
            self.missed_vblanks,
            self.hud_script_sum / n,
            self.ui_draw_sum / n,
            self.ui_ge_sum / n,
            self.max_work_frame,
        );
        for path in [
            b"host0:/OpenStrike-bench.jsonl\0".as_ptr(),
            b"ms0:/OpenStrike-bench.jsonl\0".as_ptr(),
        ] {
            unsafe {
                let fd = sys::sceIoOpen(
                    path,
                    IoOpenFlags::WR_ONLY | IoOpenFlags::CREAT | IoOpenFlags::APPEND,
                    0o777,
                );
                if fd.0 >= 0 {
                    sys::sceIoWrite(fd, line.as_ptr() as *const c_void, line.len());
                    sys::sceIoClose(fd);
                }
            }
        }
        self.frames = 0;
        self.sim_ticks = 0;
        self.map_loads = 0;
        self.menu_returns = 0;
        self.work_sum = 0;
        self.max_work = 0;
        self.gpu_sum = 0;
        self.max_gpu = 0;
        self.faces_sum = 0;
        self.tris_sum = 0;
        self.indices_sum = 0;
        self.seg_sums = [0; 4];
        self.max_segs = [0; 5];
        self.hud_script_sum = 0;
        self.ui_draw_sum = 0;
        self.ui_ge_sum = 0;
        self.actor_sum = 0;
        self.actor_max = 0;
        self.actor_count_sum = 0;
        self.actor_count_max = 0;
        self.frame_sum = 0;
        self.frame_samples = 0;
        self.late_frames = 0;
        self.buttons_or = 0;
        self.analog_frames = 0;
        self.movement_frames = 0;
        self.airborne_frames = 0;
        self.look_frames = 0;
        self.reload_frames = 0;
        self.ammo_min = u32::MAX;
        self.ammo_max = 0;
        self.clip_frames = [0; 7];
        self.events = [0; 6];
        self.reused_vblanks = 0;
        self.missed_vblanks = 0;
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
fn capture_sample(frame: u32, fallback: (CtrlButtons, u8, u8)) -> (CtrlButtons, u8, u8) {
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

/// Exercise the production pad mapper by simulation tick, not render count.
/// Two ten-second movement cycles, then quit through the HUD and load a
/// map through the menu. Repeat after thirty seconds of simulation time.
#[cfg(feature = "motion-bench")]
fn motion_sample(tick: u64) -> (CtrlButtons, u8, u8) {
    let cycle = tick % 1800;
    if cycle >= 1200 {
        let button = match cycle {
            1200 => CtrlButtons::SELECT,
            1230 => CtrlButtons::RIGHT,
            t if (1290..1318).contains(&t)
                && (t - 1290) % 4 == 0
                && (t - 1290) / 4 < (tick / 1800) % openstrike_mods::PACKS.len() as u64 =>
            {
                CtrlButtons::DOWN
            }
            1260 | 1320 => CtrlButtons::CIRCLE,
            1350 if openstrike_mods::PACKS.len() > 1 => CtrlButtons::CIRCLE,
            _ => CtrlButtons::empty(),
        };
        return (button, 128, 128);
    }
    let t = cycle % 600;
    let mut buttons = CtrlButtons::empty();
    if (120..240).contains(&t) {
        buttons |= CtrlButtons::CIRCLE;
    }
    if (250..254).contains(&t) {
        buttons |= CtrlButtons::LTRIGGER;
    }
    if (330..400).contains(&t) {
        buttons |= CtrlButtons::RTRIGGER;
    }
    if t == 420 {
        buttons |= CtrlButtons::DOWN;
    }
    let ly = if t < 120 || (240..330).contains(&t) {
        0
    } else {
        128
    };
    (buttons, 128, ly)
}
