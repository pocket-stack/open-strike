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
#[cfg(all(feature = "combat-bench", feature = "character-bench"))]
compile_error!("combat-bench requires the real simulation; use one probe at a time");
mod input;
mod maps;
mod present;
mod strike;

use core::ffi::c_void;

use libquickjs_sys::*;
use pocket3d_gu::{Camera3d, FramePool, WorldRenderer, sky};
use pocketjs_psp::{dbg, ffi, ge, host, pak};
#[cfg(feature = "capture")]
use psp::sys::CtrlButtons;
#[cfg(feature = "capture")]
use psp::sys::DisplayPixelFormat;
#[cfg(feature = "capture")]
use psp::sys::DisplaySetBufSync;
#[cfg(any(feature = "capture", feature = "bench"))]
use psp::sys::IoOpenFlags;
use psp::sys::{self, CtrlMode, GuContextType, GuSyncBehavior, GuSyncMode, SceCtrlData};

use input::PadInput;
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

const DT: f32 = 1.0 / 60.0;

use alloc::vec::Vec;
use openstrike_core::sim::Command;

/// A loaded world: the renderer borrows the (never-freed) map buffer, so the
/// borrow is 'static; the struct must be dropped before the buffer is
/// overwritten by the next load (see the frame loop's host_cmd handling).
struct Game {
    sim: StrikeSim,
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
    let words = (max_map_bytes as usize + 15) / 16 + 1;
    let map_buf_ptr = alloc::boxed::Box::leak(alloc::vec![0u128; words].into_boxed_slice())
        .as_mut_ptr() as *mut u8;
    let map_buf_cap = words * 16;

    let mut pool = FramePool::new();
    let sky_params = sky::SkyParams::default();
    let rifle = present::build_rifle();
    let mut officers = present::OfficerRenderer::new();

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
    let mut game: Option<Game> = None;
    let mut menu_time: f64 = 0.0;
    if !AUTOSTART.is_empty() {
        let idx = map_names.iter().position(|n| n == AUTOSTART);
        let Some(idx) = idx else {
            host::halt("autostart map not in the catalogue");
        };
        match maps::load(&map_names[idx], map_buf_ptr, map_buf_cap, &boot_cfg) {
            Ok(g) => game = Some(g),
            Err(e) => host::halt(e),
        }
    }

    // ---- Frame loop (pipelined present, one tick per vblank) ----
    let mut frame_count: u32 = 0;
    let mut last_present_vcount = sys::sceDisplayGetVcount();
    #[cfg(feature = "bench")]
    let mut bench = Bench::new();
    #[cfg(feature = "combat-bench")]
    let mut combat = combat_probe::CombatProbe::new();
    loop {
        #[cfg(feature = "bench")]
        let bench_t0 = bench_now();
        sys::sceCtrlReadBufferPositive(&mut pad_data, 1);
        #[cfg_attr(not(feature = "capture"), allow(unused_mut))]
        let mut sample = (pad_data.buttons, pad_data.lx, pad_data.ly);
        #[cfg(feature = "capture")]
        {
            sample = capture_sample(frame_count, sample);
        }
        #[cfg_attr(not(feature = "combat-bench"), allow(unused_mut))]
        let mut _tick = pad.map(sample.0, sample.1, sample.2, DT);
        let mask = sample.0.bits() as i32;

        // Simulation (game stage only; the menu has no world).
        if let Some(g) = &mut game {
            #[cfg(feature = "combat-bench")]
            {
                _tick.sim = combat.input(&mut g.sim);
                _tick.look_dx = 0.0;
                _tick.look_dy = 0.0;
            }
            #[cfg(not(feature = "character-bench"))]
            {
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
        let bench_after_sim = bench_now();
        #[cfg(feature = "bench")]
        if let Some(g) = &game {
            bench.observe_events(&g.sim);
        }

        // Guest turn: facts out, HUD frame, microtasks, intent in.
        let ok = match &mut game {
            Some(g) => strike::dispatch(ctx, global, &mut g.sim),
            None => strike::dispatch_menu(ctx, global, menu_time),
        };
        if !ok {
            log_exception(ctx);
        }
        #[cfg(feature = "bench")]
        let bench_after_dispatch = bench_now();
        let mut args = [JS_NewInt32(ctx, mask)];
        let r = JS_Call(ctx, frame_fn, global, 1, args.as_mut_ptr());
        if JS_ValueGetTag(r) == JS_TAG_EXCEPTION {
            log_exception(ctx);
        }
        JS_FreeValue(ctx, r);
        #[cfg(feature = "bench")]
        let bench_after_js = bench_now();
        host::drain_jobs(rt);
        strike::drain(|cmd| match &mut game {
            Some(g) => g.sim.apply(cmd, 0),
            None => boot_cfg.push(cmd),
        });
        #[cfg(feature = "character-bench")]
        if let Some(g) = &mut game {
            character_probe::stage(&mut g.sim, frame_count);
        }
        let mut host_cmd: Option<strike::HostCmd> = None;
        strike::drain_host(|c| host_cmd = Some(c));

        // UI core frame.
        let ui = ffi::ui();
        ui.tick();
        let (words_ptr, words_len) = {
            let dl = ui.draw();
            (dl.words.as_ptr(), dl.words.len())
        };

        // Pipelined present: wait out frame N-1, show it, then record N.
        #[cfg(feature = "bench")]
        let bench_after_ui = bench_now();
        #[cfg(feature = "bench")]
        let bench_before_sync = bench_after_ui;
        sys::sceGuSync(GuSyncMode::Finish, GuSyncBehavior::Wait);
        #[cfg(feature = "bench")]
        let bench_after_sync = bench_now();
        // A completed frame may already be inside a NEW blanking interval.
        // Waiting for another start would discard that refresh opportunity.
        // The vcount guard still permits at most one simulation/present per
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
            officers.draw(&mut pool, &g.sim.bots, &cam);
            #[cfg(feature = "bench")]
            {
                actor_us = bench_now() - actor_start;
            }
            present::draw_effects(&mut pool, &g.sim, &cam);
            present::draw_viewmodel(&mut pool, &rifle, &g.sim);
        }
        pocket3d_gu::end_3d();
        // The JSX HUD, unchanged from every other PocketJS host.
        ge::render_over(ffi::ui(), core::slice::from_raw_parts(words_ptr, words_len));
        sys::sceGuFinish();

        #[cfg(feature = "bench")]
        {
            let (faces, tris) = match &game {
                Some(g) => (g.world.last_faces, g.world.last_tris),
                None => (0, 0),
            };
            bench.observe(mask as u32, &_tick, game.as_ref().map(|g| &g.sim));
            bench.record(
                frame_count,
                bench_t0,
                &[
                    ("sim", bench_after_sim),
                    ("dispatch", bench_after_dispatch),
                    ("js", bench_after_js),
                    ("ui", bench_after_ui),
                ],
                bench_before_sync,
                bench_after_sync,
                bench_after_present,
                faces,
                tris,
                actor_us,
                if game.is_some() { officers.visible } else { 0 },
            );
        }

        // World lifecycle intents, applied OUTSIDE all world borrows (the
        // presented frame already showed the menu's LOADING state).
        match host_cmd {
            Some(strike::HostCmd::LoadMap(i)) if game.is_none() => {
                if let Some(name) = map_names.get(i) {
                    match maps::load(name, map_buf_ptr, map_buf_cap, &boot_cfg) {
                        Ok(g) => game = Some(g),
                        Err(e) => psp::dprintln!("[openstrike] loadMap failed: {}", e),
                    }
                }
            }
            Some(strike::HostCmd::ToMenu) => {
                game = None; // drops every borrow of the map buffer
                menu_time = 0.0;
            }
            _ => {}
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
    window: u32,
    work_sum: u64,
    max_work: u64,
    gpu_sum: u64,
    max_gpu: u64,
    faces_sum: u64,
    tris_sum: u64,
    seg_sums: [u64; 4],
    /// Breakdown of the frame that set max_work: 4 segments + the
    /// uninstrumented rest (draw recording, input, GE list build).
    max_segs: [u64; 5],
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
            window: 0,
            work_sum: 0,
            max_work: 0,
            gpu_sum: 0,
            max_gpu: 0,
            faces_sum: 0,
            tris_sum: 0,
            seg_sums: [0; 4],
            max_segs: [0; 5],
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
        segments: &[(&str, u64); 4],
        before_sync: u64,
        after_sync: u64,
        after_present: u64,
        faces: u32,
        tris: u32,
        actor_us: u64,
        actors: u32,
    ) {
        let now = bench_now();
        let mut prev = t0;
        let mut segs = [0u64; 5];
        for (i, (_, at)) in segments.iter().enumerate() {
            segs[i] = at.saturating_sub(prev);
            self.seg_sums[i] += segs[i];
            prev = *at;
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
        // Spike forensics: any frame past ~1.5x budget logs itself with its
        // absolute frame index so it can be correlated with the input script.
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
        if self.frames < 300 {
            return;
        }
        let n = self.frames as u64;
        self.window += 1;
        let arena = unsafe { pocketjs_psp::arena::stats() };
        self.work_times.sort_unstable();
        self.frame_times.sort_unstable();
        let line = alloc::format!(
            "{{\"window\":{},\"frames\":{},\"avg_work_us\":{},\"max_work_us\":{},\"avg_gpu_us\":{},\"max_gpu_us\":{},\"avg_faces\":{},\"avg_tris\":{},\"avg_sim_us\":{},\"avg_dispatch_us\":{},\"avg_js_us\":{},\"avg_ui_us\":{},\"arena_capacity_bytes\":{},\"arena_bump_bytes\":{},\"arena_tail_free_bytes\":{},\"max_segs_us\":[{},{},{},{},{}],\"actor_probe\":{},\"avg_actor_us\":{},\"max_actor_us\":{},\"avg_actors\":{},\"max_actors\":{},\"actor_triangles_each\":{},\"observed_fps_milli\":{},\"p95_frame_us\":{},\"p99_frame_us\":{},\"p95_work_us\":{},\"late_frames\":{},\"input\":{{\"buttons_or\":{},\"analog_frames\":{},\"movement_frames\":{},\"look_frames\":{},\"ammo_min\":{},\"ammo_max\":{},\"reloading_frames\":{}}},\"actor_clip_frames\":[{},{},{},{},{},{},{}],\"combat_probe\":{},\"events\":{{\"hits\":{},\"kills\":{},\"damage\":{},\"deaths\":{},\"resets\":{},\"shots\":{}}},\"reused_vblanks\":{},\"missed_vblanks\":{},\"max_work_frame\":{}}}\n",
            self.window,
            n,
            self.work_sum / n,
            self.max_work,
            self.gpu_sum / n,
            self.max_gpu,
            self.faces_sum / n,
            self.tris_sum / n,
            self.seg_sums[0] / n,
            self.seg_sums[1] / n,
            self.seg_sums[2] / n,
            self.seg_sums[3] / n,
            arena.capacity_bytes,
            arena.bump_bytes,
            arena.tail_free_bytes,
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
            openstrike_character::triangle_count(),
            self.frame_samples as u64 * 1_000_000_000 / self.frame_sum.max(1),
            self.frame_times[284],
            self.frame_times[296],
            self.work_times[284],
            self.late_frames,
            self.buttons_or,
            self.analog_frames,
            self.movement_frames,
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
        self.work_sum = 0;
        self.max_work = 0;
        self.gpu_sum = 0;
        self.max_gpu = 0;
        self.faces_sum = 0;
        self.tris_sum = 0;
        self.seg_sums = [0; 4];
        self.max_segs = [0; 5];
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
