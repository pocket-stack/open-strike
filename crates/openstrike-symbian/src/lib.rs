#![no_std]
#![allow(static_mut_refs)]

extern crate alloc;
extern crate self as libquickjs_sys;
extern crate self as pocketjs_psp;

mod input;
#[cfg(feature = "embedded-map-catalog")]
mod maps;
mod quickjs;

pub use quickjs::*;

#[allow(dead_code)]
#[path = "../../openstrike-vita/src/present_data.rs"]
mod present_data;
#[path = "../../openstrike-vita/src/sim_boot.rs"]
mod sim_boot;
#[allow(clippy::manual_c_str_literals, clippy::missing_safety_doc)]
#[path = "../../openstrike-psp/src/strike.rs"]
mod strike;

use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_void;

use glam::{Mat4, Vec3};
use input::KeyboardInput;
#[cfg(feature = "embedded-map-catalog")]
use maps::{AlignedMapBuffer, MAP_CATALOG};
use openstrike_core::sim::Command;
use openstrike_core::StrikeSim;
use pocket3d_bsp::cooked::{self, CookedMap};
use pocket3d_gles2::{
    BlendMode, Camera3d, DynamicRenderer, FrameOptions, TextureUpload, Viewport, WorldRenderer,
};
use pocketjs_symbian_core::extension::{self, ExtensionV1};

pub mod ffi {
    use super::*;

    pub unsafe fn arg_i32(
        context: *mut JSContext,
        argc: i32,
        argv: *mut JSValue,
        index: isize,
    ) -> i32 {
        if index as i32 >= argc {
            return 0;
        }
        let mut value = 0;
        JS_ToInt32(context, &mut value, *argv.offset(index));
        value
    }

    pub unsafe fn add_fn(
        context: *mut JSContext,
        object: JSValue,
        name: &'static [u8],
        function: unsafe extern "C" fn(*mut JSContext, JSValue, i32, *mut JSValue) -> JSValue,
        args: i32,
    ) {
        let value = JS_NewCFunction2(
            context,
            Some(function),
            name.as_ptr().cast(),
            args,
            JS_CFUNC_generic,
            0,
        );
        JS_SetPropertyStr(context, object, name.as_ptr().cast(), value);
    }
}

#[cfg(not(feature = "embedded-map-catalog"))]
const MAP_KEY: &str = "maps/de_dust2.p3d";
#[cfg(not(feature = "embedded-map-catalog"))]
const MAP_NAME: &str = "de_dust2";
const FIXED_DT: f32 = 1.0 / 60.0;
const TICKS_PER_HOST_FRAME: usize = 2;

#[used]
static POCKETJS_CORE_LINK_ROOT: extern "C" fn(u32) = pocketjs_symbian_core::ui_init;

struct Game {
    sim: StrikeSim,
    world: WorldRenderer<'static>,
}

struct State {
    context: *mut JSContext,
    global: JSValue,
    #[cfg(not(feature = "embedded-map-catalog"))]
    map_bytes: &'static [u8],
    // Keep Game before map_buffer: Rust drops struct fields in declaration
    // order, so even an ordinary State drop releases every borrowed map view
    // before freeing the backing allocation.
    game: Option<Game>,
    #[cfg(feature = "embedded-map-catalog")]
    map_buffer: AlignedMapBuffer,
    boot_config: Vec<Command>,
    pending_host: Option<strike::HostCmd>,
    input: KeyboardInput,
    menu_time: f64,
    viewport_width: u32,
    viewport_height: u32,
    dynamic: DynamicRenderer,
    rifle: Vec<present_data::ColorVertex>,
    bot_body: Vec<present_data::ColorVertex>,
    effects: present_data::EffectGeometry,
}

static mut STATE: Option<State> = None;

const _: () = assert!(
    core::mem::size_of::<present_data::ColorVertex>()
        == core::mem::size_of::<pocket3d_gles2::ColorVertex>()
);
const _: () = assert!(
    core::mem::align_of::<present_data::ColorVertex>()
        == core::mem::align_of::<pocket3d_gles2::ColorVertex>()
);

fn gles_vertices(vertices: &[present_data::ColorVertex]) -> &[pocket3d_gles2::ColorVertex] {
    // Both shared presentation vertices and the GLES backend pin the same
    // repr(C) { ABGR u32, x/y/z f32 } layout above.
    unsafe { core::slice::from_raw_parts(vertices.as_ptr().cast(), vertices.len()) }
}

fn valid_extent(value: i32) -> Option<u32> {
    (value > 0 && value <= 640).then_some(value as u32)
}

fn game_from_map(bytes: &'static [u8], boot_config: &[Command]) -> Result<Game, &'static str> {
    let map: CookedMap<'static> = cooked::read(bytes).map_err(|_| "invalid cooked map")?;
    let sim = sim_boot::from_map(&map, boot_config)?;
    Ok(Game {
        sim,
        world: WorldRenderer::new(map),
    })
}

unsafe fn drain_commands(state: &mut State) {
    strike::drain(|command| match &mut state.game {
        Some(game) => game.sim.apply(command, 0),
        None => state.boot_config.push(command),
    });
    strike::drain_host(|command| state.pending_host = Some(command));
}

unsafe fn dispatch_tick(state: &mut State, native_keys: u32, buttons: u32) -> bool {
    let tick = state.input.map(native_keys, buttons, FIXED_DT);
    let dispatched = match &mut state.game {
        Some(game) => {
            game.sim.apply_look(tick.look_dx, tick.look_dy);
            game.sim
                .tick(&game.world.map().collision, FIXED_DT, &tick.sim);
            strike::dispatch(state.context, state.global, &mut game.sim)
        }
        None => {
            state.menu_time += FIXED_DT as f64;
            strike::dispatch_menu(state.context, state.global, state.menu_time)
        }
    };
    if dispatched {
        drain_commands(state);
    }
    dispatched
}

unsafe fn apply_pending_host(state: &mut State) -> Result<(), ()> {
    match state.pending_host.take() {
        Some(strike::HostCmd::LoadMap(index)) if valid_map_index(index) => {
            shutdown_game(state)?;
            let mut game = load_game(state, index)?;
            game.world.initialize_gpu().map_err(|_| ())?;
            state.game = Some(game);
        }
        Some(strike::HostCmd::ToMenu) => {
            shutdown_game(state)?;
            state.menu_time = 0.0;
        }
        _ => {}
    }
    Ok(())
}

unsafe fn shutdown_game(state: &mut State) -> Result<(), ()> {
    if let Some(mut game) = state.game.take() {
        let shutdown = game.world.shutdown_gpu().map_err(|_| ());
        // Drop every CookedMap/renderer view before load_game is allowed to
        // resize or overwrite the shared backing allocation.
        drop(game);
        shutdown?;
    }
    Ok(())
}

#[cfg(feature = "embedded-map-catalog")]
fn valid_map_index(index: usize) -> bool {
    index < MAP_CATALOG.len()
}

#[cfg(not(feature = "embedded-map-catalog"))]
fn valid_map_index(index: usize) -> bool {
    index == 0
}

#[cfg(feature = "embedded-map-catalog")]
unsafe fn load_game(state: &mut State, index: usize) -> Result<Game, ()> {
    let entry = MAP_CATALOG.get(index).ok_or(())?;
    let bytes = state.map_buffer.load(entry).map_err(|_| ())?;
    // SAFETY: map_buffer belongs to STATE and is released only after Game.
    // apply_pending_host always drops Game before the next buffer mutation.
    let bytes: &'static [u8] = core::slice::from_raw_parts(bytes.as_ptr(), bytes.len());
    game_from_map(bytes, &state.boot_config).map_err(|_| ())
}

#[cfg(not(feature = "embedded-map-catalog"))]
unsafe fn load_game(state: &mut State, index: usize) -> Result<Game, ()> {
    if index != 0 {
        return Err(());
    }
    game_from_map(state.map_bytes, &state.boot_config).map_err(|_| ())
}

fn camera_for(state: &State) -> Camera3d {
    let mut camera = match &state.game {
        Some(game) => Camera3d {
            pos: game.sim.player.eye_interpolated(1.0),
            yaw: game.sim.player.yaw,
            pitch: game.sim.player.pitch,
            fov_y: 74f32.to_radians(),
            ..Camera3d::default()
        },
        None => Camera3d {
            fov_y: 74f32.to_radians(),
            ..Camera3d::default()
        },
    };
    camera.set_viewport(state.viewport_width, state.viewport_height);
    camera
}

fn sky_color(camera: &Camera3d) -> [f32; 4] {
    let zenith = Vec3::new(0.34, 0.48, 0.66);
    let horizon = Vec3::new(0.93, 0.79, 0.62);
    let amount = libm::powf(
        libm::sinf(camera.pitch + camera.fov_y * 0.2).clamp(0.0, 1.0),
        0.65,
    );
    let color = horizon + (zenith - horizon) * amount;
    [color.x, color.y, color.z, 1.0]
}

unsafe extern "C" fn boot(
    context: *mut c_void,
    pak: *const u8,
    _pak_len: usize,
    viewport_width: i32,
    viewport_height: i32,
) -> i32 {
    if STATE.is_some() || context.is_null() || pak.is_null() {
        return 0;
    }
    let (Some(width), Some(height)) = (valid_extent(viewport_width), valid_extent(viewport_height))
    else {
        return 0;
    };
    #[cfg(not(feature = "embedded-map-catalog"))]
    let map = {
        let pak = core::slice::from_raw_parts(pak, _pak_len);
        let Some(map) = pocketjs_core::pak::find(pak, MAP_KEY) else {
            return 0;
        };
        // The extension ABI guarantees that the host-owned PAK outlives boot
        // and remains immutable until shutdown.
        core::slice::from_raw_parts(map.as_ptr(), map.len())
    };
    let context = context.cast::<JSContext>();
    let global = JS_GetGlobalObject(context);

    // A cold app boot must never inherit intent queued by an earlier guest.
    strike::drain(drop);
    strike::drain_host(drop);
    #[cfg(feature = "embedded-map-catalog")]
    let map_names: Vec<String> = MAP_CATALOG
        .iter()
        .map(|entry| String::from(entry.name))
        .collect();
    #[cfg(not(feature = "embedded-map-catalog"))]
    let map_names = [String::from(MAP_NAME)];
    strike::register(context, global, &map_names);

    STATE = Some(State {
        context,
        global,
        #[cfg(not(feature = "embedded-map-catalog"))]
        map_bytes: map,
        game: None,
        #[cfg(feature = "embedded-map-catalog")]
        map_buffer: AlignedMapBuffer::default(),
        boot_config: Vec::new(),
        pending_host: None,
        input: KeyboardInput::new(),
        menu_time: 0.0,
        viewport_width: width,
        viewport_height: height,
        dynamic: DynamicRenderer::new(),
        rifle: present_data::build_rifle(),
        bot_body: present_data::build_bot_body(),
        effects: present_data::EffectGeometry::default(),
    });
    1
}

unsafe extern "C" fn shutdown(gl_context_current: i32) {
    let Some(mut state) = STATE.take() else {
        return;
    };
    if let Some(mut game) = state.game.take() {
        if gl_context_current != 0 {
            let _ = game.world.shutdown_gpu();
        } else {
            game.world.abandon_lost_context();
        }
    }
    if gl_context_current != 0 {
        let _ = state.dynamic.shutdown_gpu();
    } else {
        state.dynamic.abandon_lost_context();
    }
    strike::drain(drop);
    strike::drain_host(drop);
    JS_FreeValue(state.context, state.global);
}

unsafe extern "C" fn before_guest(
    context: *mut c_void,
    buttons: u32,
    _analog: u32,
    native_keys: u32,
) -> i32 {
    let Some(state) = STATE.as_mut() else {
        return 0;
    };
    if context.cast::<JSContext>() != state.context {
        return 0;
    }
    for _ in 0..TICKS_PER_HOST_FRAME {
        if !dispatch_tick(state, native_keys, buttons) {
            return 0;
        }
    }
    1
}

unsafe extern "C" fn after_guest(context: *mut c_void) -> i32 {
    let Some(state) = STATE.as_mut() else {
        return 0;
    };
    if context.cast::<JSContext>() != state.context {
        return 0;
    }
    drain_commands(state);
    1
}

unsafe extern "C" fn resize(viewport_width: i32, viewport_height: i32) {
    let (Some(width), Some(height)) = (valid_extent(viewport_width), valid_extent(viewport_height))
    else {
        return;
    };
    if let Some(state) = STATE.as_mut() {
        state.viewport_width = width;
        state.viewport_height = height;
    }
}

unsafe extern "C" fn render(
    target_x: i32,
    target_y: i32,
    target_width: i32,
    target_height: i32,
    _window_width: i32,
    window_height: i32,
) -> i32 {
    let Some(state) = STATE.as_mut() else {
        return 0;
    };
    let (Some(width), Some(height)) = (valid_extent(target_width), valid_extent(target_height))
    else {
        return 0;
    };
    let gl_y = window_height - target_y - target_height;
    if target_x < 0 || gl_y < 0 {
        return 0;
    }
    let viewport = Viewport::new(target_x, gl_y, width, height);
    state.viewport_width = width;
    state.viewport_height = height;
    state.dynamic.reset_counters();

    if apply_pending_host(state).is_err() {
        return 0;
    }
    if !state.dynamic.gpu_ready() && state.dynamic.initialize_gpu().is_err() {
        return 0;
    }

    let camera = camera_for(state);
    let view_projection = camera.view_proj();
    let Some(game) = state.game.as_mut() else {
        return state
            .dynamic
            .clear_frame(viewport, [0.02, 0.035, 0.055, 1.0], true)
            .is_ok() as i32;
    };

    match game.world.upload_next_texture() {
        Ok(TextureUpload::Uploaded { .. }) | Ok(TextureUpload::Complete { .. }) => {}
        Err(_) => return 0,
    }
    let options = FrameOptions {
        viewport,
        clear_color: Some(sky_color(&camera)),
        clear_depth: true,
    };
    if game.world.render(&camera, options).is_err() {
        return 0;
    }

    for bot in &game.sim.bots {
        if state
            .dynamic
            .draw_color_tris(
                gles_vertices(&state.bot_body),
                bot.transform_scaled(1.0),
                view_projection,
                BlendMode::Opaque,
            )
            .is_err()
        {
            return 0;
        }
    }
    present_data::build_effects_into(&mut state.effects, &game.sim, camera.forward());
    if state
        .dynamic
        .draw_color_tris(
            gles_vertices(&state.effects.vertices),
            Mat4::IDENTITY,
            view_projection,
            BlendMode::Additive,
        )
        .is_err()
    {
        return 0;
    }
    if game.sim.player.alive {
        if state.dynamic.clear_depth_for_viewmodel().is_err()
            || state
                .dynamic
                .draw_color_tris(
                    gles_vertices(&state.rifle),
                    game.sim.viewmodel_transform_at(1.0),
                    view_projection,
                    BlendMode::Opaque,
                )
                .is_err()
        {
            return 0;
        }
    }
    1
}

static EXTENSION: ExtensionV1 = ExtensionV1 {
    abi_version: extension::ABI_V1,
    struct_size: ExtensionV1::struct_size(),
    flags: extension::FLAG_DEPTH_BUFFER,
    boot: Some(boot),
    shutdown: Some(shutdown),
    before_guest: Some(before_guest),
    after_guest: Some(after_guest),
    resize: Some(resize),
    render: Some(render),
};

#[no_mangle]
pub extern "C" fn pocketjs_symbian_extension_v1() -> *const ExtensionV1 {
    &EXTENSION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_requests_depth_and_keeps_the_fixed_step_ratio() {
        assert_eq!(EXTENSION.abi_version, extension::ABI_V1);
        assert_eq!(EXTENSION.flags, extension::FLAG_DEPTH_BUFFER);
        assert_eq!(TICKS_PER_HOST_FRAME, 2);
        assert!((FIXED_DT * TICKS_PER_HOST_FRAME as f32 - 1.0 / 30.0).abs() < f32::EPSILON);
    }

    #[test]
    fn rejects_transient_or_oversized_viewports() {
        assert_eq!(valid_extent(0), None);
        assert_eq!(valid_extent(641), None);
        assert_eq!(valid_extent(640), Some(640));
    }

    #[test]
    fn quickjs_value_matches_the_symbian_host_abi() {
        assert_eq!(core::mem::size_of::<JSValue>(), 8);
        assert!(core::mem::align_of::<JSValue>() <= 8);
    }
}
