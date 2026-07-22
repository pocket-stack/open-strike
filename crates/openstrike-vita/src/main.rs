#[cfg(target_os = "vita")]
mod vita {
    use std::boxed::Box;
    #[cfg(feature = "capture")]
    use std::format;
    use std::string::{String, ToString};
    use std::vec::Vec;

    use openstrike_core::sim::Command;
    use openstrike_core::StrikeSim;
    #[cfg(feature = "capture")]
    use openstrike_vita::capture::CaptureScript;
    use openstrike_vita::input::{PadInput, PadSample};
    use openstrike_vita::map_data::{AlignedMapBuffer, MapCatalogue};
    use openstrike_vita::present_data;
    use openstrike_vita::{sim_boot, strike};
    use pocket3d_bsp::cooked;
    use pocket3d_vita::sky::{self, SkyParams};
    use pocket3d_vita::{Camera3d, FramePool, WorldRenderer};
    use pocketjs_vita::{graphics, input, vita_log, Runtime};

    static APP_JS: &str = include_str!(concat!(env!("OUT_DIR"), "/game.js"));
    static APP_PAK: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/app.pak"));
    static AUTOSTART: &str = env!("OPENSTRIKE_VITA_AUTOSTART");

    #[cfg(feature = "capture")]
    static CAPTURE_INPUT: &str = env!("OPENSTRIKE_VITA_CAPTURE_INPUT");
    #[cfg(feature = "capture")]
    static CAP_START: &str = env!("OPENSTRIKE_VITA_CAP_START");
    #[cfg(feature = "capture")]
    static CAP_N: &str = env!("OPENSTRIKE_VITA_CAP_N");

    const DT: f32 = 1.0 / 60.0;
    const FRAME_POOL_BYTES: u32 = 8 * 1024 * 1024;
    #[cfg(feature = "capture")]
    const CAPTURE_ROOT: &str = "ux0:data/openstrike-vita/cap";

    #[cfg(feature = "bench")]
    struct BenchWindow {
        frames: u64,
        frame_sum_us: u64,
        cpu_sum_us: u64,
        render_sync_sum_us: u64,
        max_frame_us: u64,
        max_cpu_us: u64,
        max_render_sync_us: u64,
    }

    #[cfg(feature = "bench")]
    struct BenchSample {
        frames: u64,
        elapsed_us: u64,
        avg_frame_us: u64,
        max_frame_us: u64,
        avg_cpu_us: u64,
        max_cpu_us: u64,
        avg_render_sync_us: u64,
        max_render_sync_us: u64,
    }

    #[cfg(feature = "bench")]
    impl BenchWindow {
        fn new() -> Self {
            Self {
                frames: 0,
                frame_sum_us: 0,
                cpu_sum_us: 0,
                render_sync_sum_us: 0,
                max_frame_us: 0,
                max_cpu_us: 0,
                max_render_sync_us: 0,
            }
        }

        fn record(
            &mut self,
            frame_started_us: u64,
            render_started_us: u64,
            frame_finished_us: u64,
        ) -> Option<BenchSample> {
            let frame_us = frame_finished_us.saturating_sub(frame_started_us);
            let cpu_us = render_started_us.saturating_sub(frame_started_us);
            // Includes the previous-frame GXM drain in begin_frame and any
            // swap/vblank pacing in present; it is deliberately not labelled
            // as pure CPU render or GPU execution time.
            let render_sync_us = frame_finished_us.saturating_sub(render_started_us);
            self.frame_sum_us += frame_us;
            self.cpu_sum_us += cpu_us;
            self.render_sync_sum_us += render_sync_us;
            self.max_frame_us = self.max_frame_us.max(frame_us);
            self.max_cpu_us = self.max_cpu_us.max(cpu_us);
            self.max_render_sync_us = self.max_render_sync_us.max(render_sync_us);
            self.frames += 1;
            if self.frames < 300 {
                return None;
            }

            let elapsed_us = self.frame_sum_us.max(1);
            let sample = BenchSample {
                frames: self.frames,
                elapsed_us,
                avg_frame_us: elapsed_us / self.frames,
                max_frame_us: self.max_frame_us,
                avg_cpu_us: self.cpu_sum_us / self.frames,
                max_cpu_us: self.max_cpu_us,
                avg_render_sync_us: self.render_sync_sum_us / self.frames,
                max_render_sync_us: self.max_render_sync_us,
            };
            self.frames = 0;
            self.frame_sum_us = 0;
            self.cpu_sum_us = 0;
            self.render_sync_sum_us = 0;
            self.max_frame_us = 0;
            self.max_cpu_us = 0;
            self.max_render_sync_us = 0;
            Some(sample)
        }
    }

    #[no_mangle]
    #[used]
    pub static sceUserMainThreadStackSize: u32 = 2 * 1024 * 1024;

    struct Game {
        sim: StrikeSim,
        world: WorldRenderer<'static>,
    }

    fn fail(message: &str) -> ! {
        vita_log(format_args!("[OpenStrike Vita] {message}"));
        #[cfg(feature = "capture")]
        {
            let _ = std::fs::create_dir_all(CAPTURE_ROOT);
            let _ = std::fs::write(format!("{CAPTURE_ROOT}/error.txt"), message);
        }
        loop {
            std::thread::yield_now();
        }
    }

    /// Reuse one aligned allocation for every map. The caller drops the old
    /// `Game` before invoking this function again, so no `CookedMap` view can
    /// overlap the next write into the buffer.
    unsafe fn load_game(
        catalogue: &MapCatalogue,
        buffer: *mut AlignedMapBuffer,
        name: &str,
        boot_config: &[Command],
    ) -> Result<Game, String> {
        let bytes = unsafe { catalogue.load(name, &mut *buffer) };
        let bytes = match bytes {
            Ok(bytes) => bytes,
            Err(error) => return Err(error.to_string()),
        };
        // SAFETY: `buffer` was leaked for the process lifetime. The lifecycle
        // rule above prevents mutation until `world` is dropped.
        let bytes: &'static [u8] =
            unsafe { std::slice::from_raw_parts(bytes.as_ptr(), bytes.len()) };
        unsafe { pocket3d_vita::writeback(bytes) };
        let map = cooked::read(bytes).map_err(String::from)?;
        let sim = sim_boot::from_map(&map, boot_config).map_err(String::from)?;
        Ok(Game {
            sim,
            world: WorldRenderer::new(map),
        })
    }

    #[cfg(feature = "capture")]
    fn capture_u32(source: &str, fallback: u32) -> u32 {
        source.parse().unwrap_or(fallback)
    }

    #[cfg(feature = "capture")]
    fn dump_capture(
        runtime: &mut Runtime,
        index: u32,
        world_faces: u32,
        world_tris: u32,
        world_direct_draw_calls: u32,
        geometry_resident: bool,
        geometry_error: Option<&str>,
        texture_error: Option<pocket3d_vita::texture::UploadError>,
        frame_pool: &FramePool,
        camera: &Camera3d,
    ) -> Result<(), String> {
        let image = format!("{CAPTURE_ROOT}/f{index:04}.rgba");
        unsafe { runtime.capture_golden(&image) }.map_err(|error| error.to_string())?;
        let gxm_error = unsafe { pocket3d_vita::last_gxm_error() }.unwrap_or("none");
        let geometry_error = geometry_error.unwrap_or("none");
        let texture_error = texture_error
            .map(|error| format!("{error:?}"))
            .unwrap_or_else(|| "none".to_string());
        let scene = format!(
            "renderer=gxm\ngxm_error={gxm_error}\ngeometry_resident={}\ngeometry_error={geometry_error}\ntexture_error={texture_error}\nworld_faces={world_faces}\nworld_tris={world_tris}\nworld_direct_draw_calls={world_direct_draw_calls}\nsubmitted_tris={}\ndraw_calls={}\ndropped_triangles={}\nsubmission_errors={}\neffect_tris={}\ncamera_yaw={}\ncamera_pitch={}\n",
            u8::from(geometry_resident),
            frame_pool.last.triangles,
            frame_pool.last.draw_calls,
            frame_pool.last.dropped_triangles,
            frame_pool.last.submission_errors,
            frame_pool.last.additive_triangles,
            camera.yaw,
            camera.pitch,
        );
        std::fs::write(format!("{CAPTURE_ROOT}/f{index:04}.scene"), scene)
            .map_err(|error| error.to_string())
    }

    pub unsafe fn run() {
        graphics::init_with_pool(FRAME_POOL_BYTES).unwrap_or_else(|error| fail(error));

        let catalogue = MapCatalogue::vita().unwrap_or_else(|error| fail(&error.to_string()));
        let map_names = catalogue.names().to_vec();
        if map_names.is_empty() {
            fail("no cooked maps found under app0:/maps");
        }
        let map_buffer = Box::into_raw(Box::new(AlignedMapBuffer::with_capacity(
            catalogue.largest_bytes(),
        )));

        let mut runtime = Runtime::new(APP_PAK).unwrap_or_else(|error| fail(&error));
        strike::register(runtime.context(), runtime.global(), &map_names);
        runtime.eval(APP_JS).unwrap_or_else(|error| fail(&error));

        // Configuration commands run synchronously during bundle evaluation;
        // retain them as the template for every subsequently loaded map.
        let mut boot_config: Vec<Command> = Vec::new();
        strike::drain(|command| boot_config.push(command));

        let mut game = if AUTOSTART.is_empty() {
            None
        } else if map_names.iter().any(|name| name == AUTOSTART) {
            Some(
                load_game(&catalogue, map_buffer, AUTOSTART, &boot_config)
                    .unwrap_or_else(|error| fail(&error)),
            )
        } else {
            fail("autostart map is not in the packaged catalogue")
        };

        let mut pad_mapper = PadInput::new();
        let mut frame_pool = FramePool::new();
        let sky = SkyParams::default();
        let rifle = present_data::build_rifle();
        let bot_body = present_data::build_bot_body();
        let mut effect_geometry = present_data::EffectGeometry::default();
        let mut menu_time = 0.0f64;
        let mut frame = 0u32;
        #[cfg(feature = "bench")]
        let mut bench_window = BenchWindow::new();

        #[cfg(feature = "capture")]
        let capture_script = CaptureScript::parse(CAPTURE_INPUT);
        #[cfg(feature = "capture")]
        let cap_start = capture_u32(CAP_START, 96);
        #[cfg(feature = "capture")]
        let cap_n = capture_u32(CAP_N, 1).max(1);
        #[cfg(feature = "capture")]
        let mut capture_complete = false;

        loop {
            #[cfg(feature = "bench")]
            let bench_frame_started = vitasdk_sys::sceKernelGetProcessTimeWide();
            let physical = input::read();
            let physical = PadSample {
                buttons: physical.buttons,
                lx: physical.lx,
                ly: physical.ly,
                rx: physical.rx,
                ry: physical.ry,
            };
            #[cfg(feature = "capture")]
            let sample = capture_script.sample(frame, physical);
            #[cfg(not(feature = "capture"))]
            let sample = physical;
            let tick = pad_mapper.map(sample, DT);

            if let Some(current) = &mut game {
                current.sim.apply_look(tick.look_dx, tick.look_dy);
                current
                    .sim
                    .tick(&current.world.map().collision, DT, &tick.sim);
            } else {
                menu_time += DT as f64;
            }

            let dispatched = match &mut game {
                Some(current) => {
                    strike::dispatch(runtime.context(), runtime.global(), &mut current.sim)
                }
                None => strike::dispatch_menu(runtime.context(), runtime.global(), menu_time),
            };
            if !dispatched {
                fail("strike.__dispatch threw");
            }
            runtime
                .frame(tick.ui_buttons as i32)
                .unwrap_or_else(|error| fail(&error));
            strike::drain(|command| match &mut game {
                Some(current) => current.sim.apply(command, 0),
                None => boot_config.push(command),
            });
            let mut host_command = None;
            strike::drain_host(|command| host_command = Some(command));
            runtime.tick();

            #[cfg(feature = "bench")]
            let bench_render_started = vitasdk_sys::sceKernelGetProcessTimeWide();
            graphics::begin_frame(0xff00_0000);
            frame_pool.reset();
            let camera = match &game {
                Some(current) => Camera3d {
                    pos: current.sim.player.eye_interpolated(1.0),
                    yaw: current.sim.player.yaw,
                    pitch: current.sim.player.pitch,
                    fov_y: 74f32.to_radians(),
                    ..Camera3d::default()
                },
                None => Camera3d {
                    fov_y: 74f32.to_radians(),
                    ..Camera3d::default()
                },
            };
            pocket3d_vita::begin_3d(&camera);
            sky::draw(&mut frame_pool, &camera, &sky);
            if let Some(current) = &mut game {
                current.world.draw(&mut frame_pool, &camera);
                present_data::draw_bots(&mut frame_pool, &bot_body, &current.sim.bots);
                present_data::draw_effects(
                    &mut frame_pool,
                    &mut effect_geometry,
                    &current.sim,
                    camera.forward(),
                );
                present_data::draw_viewmodel(&mut frame_pool, &rifle, &current.sim);
            }
            pocket3d_vita::end_3d();
            runtime.render_over();
            graphics::present();

            #[cfg(feature = "bench")]
            if let Some(sample) = bench_window.record(
                bench_frame_started,
                bench_render_started,
                vitasdk_sys::sceKernelGetProcessTimeWide(),
            ) {
                let (faces, triangles, world_draws) = game
                    .as_ref()
                    .map(|current| {
                        (
                            current.world.last_faces,
                            current.world.last_tris,
                            current.world.last_direct_draw_calls,
                        )
                    })
                    .unwrap_or((0, 0, 0));
                let fps_x100 = sample.frames.saturating_mul(100_000_000) / sample.elapsed_us;
                vita_log(format_args!(
                    "[OpenStrike Vita] bench frames={} fps={}.{:02} avg_frame_us={} max_frame_us={} avg_cpu_us={} max_cpu_us={} avg_render_sync_us={} max_render_sync_us={} faces={} world_tris={} world_direct_draw_calls={} submitted_tris={} draw_calls={} dropped_tris={} submission_errors={}",
                    sample.frames,
                    fps_x100 / 100,
                    fps_x100 % 100,
                    sample.avg_frame_us,
                    sample.max_frame_us,
                    sample.avg_cpu_us,
                    sample.max_cpu_us,
                    sample.avg_render_sync_us,
                    sample.max_render_sync_us,
                    faces,
                    triangles,
                    world_draws,
                    frame_pool.last.triangles,
                    frame_pool.last.draw_calls,
                    frame_pool.last.dropped_triangles,
                    frame_pool.last.submission_errors,
                ));
            }

            #[cfg(feature = "capture")]
            if !capture_complete && frame >= cap_start && frame < cap_start.saturating_add(cap_n) {
                let index = frame - cap_start;
                let (
                    faces,
                    triangles,
                    gpu_draw_calls,
                    geometry_resident,
                    geometry_error,
                    texture_error,
                ) = game
                    .as_ref()
                    .map(|current| {
                        (
                            current.world.last_faces,
                            current.world.last_tris,
                            current.world.last_direct_draw_calls,
                            current.world.gpu_geometry_resident(),
                            current.world.geometry_error(),
                            current.world.last_texture_error,
                        )
                    })
                    .unwrap_or((0, 0, 0, false, None, None));
                dump_capture(
                    &mut runtime,
                    index,
                    faces,
                    triangles,
                    gpu_draw_calls,
                    geometry_resident,
                    geometry_error,
                    texture_error,
                    &frame_pool,
                    &camera,
                )
                .unwrap_or_else(|error| fail(&error));
                if index + 1 == cap_n {
                    let _ = std::fs::write(format!("{CAPTURE_ROOT}/done"), b"ok\n");
                    capture_complete = true;
                }
            }

            // Host intents are applied outside every renderer/map borrow. A
            // map reload can therefore safely reuse the single aligned arena.
            match host_command {
                Some(strike::HostCmd::LoadMap(index)) if game.is_none() => {
                    if let Some(name) = map_names.get(index) {
                        match load_game(&catalogue, map_buffer, name, &boot_config) {
                            Ok(loaded) => game = Some(loaded),
                            Err(error) => {
                                vita_log(format_args!("[OpenStrike Vita] loadMap failed: {error}"))
                            }
                        }
                    }
                }
                Some(strike::HostCmd::ToMenu) => {
                    game = None;
                    menu_time = 0.0;
                }
                _ => {}
            }

            frame = frame.wrapping_add(1);
        }
    }
}

#[cfg(target_os = "vita")]
fn main() {
    unsafe { vita::run() }
}

#[cfg(not(target_os = "vita"))]
fn main() {
    eprintln!("openstrike-vita is a PS Vita target; use `bun scripts/vita.ts`");
}
