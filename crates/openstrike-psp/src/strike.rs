//! The `strike` surface on PSP: the same hand-written vocabulary the desktop
//! mounts through rquickjs (guest.rs), expressed through the raw QuickJS C
//! API. Ops queue [`Command`]s applied after the guest turn; facts flow the
//! other way through `strike.__dispatch(state, events)` — field for field
//! identical to the desktop build_state/build_event, so game/sdk.ts sees one
//! surface.

use alloc::vec::Vec;

use libquickjs_sys::*;
use openstrike_core::bot::BotConfig;
use openstrike_core::sim::{Command, GameEvent, Phase, StrikeSim};
use openstrike_core::weapon::WeaponConfig;
use pocketjs_psp::ffi::{add_fn, arg_i32};

// Symbols the vendored libquickjs-sys omits (provided by the linked QuickJS
// C library — the established local-extern pattern).
extern "C" {
    fn JS_ParseJSON(
        ctx: *mut JSContext,
        buf: *const core::ffi::c_char,
        len: usize,
        filename: *const core::ffi::c_char,
    ) -> JSValue;
    fn JS_NewAtom(ctx: *mut JSContext, name: *const core::ffi::c_char) -> JSAtom;
    fn JS_NewStringLen(ctx: *mut JSContext, s: *const u8, len: usize) -> JSValue;
    fn JS_NewArray(ctx: *mut JSContext) -> JSValue;
    fn JS_SetPropertyUint32(ctx: *mut JSContext, this_obj: JSValue, idx: u32, val: JSValue) -> i32;
}

/// Commands queued by ops during the guest turn (single-threaded host).
static mut COMMANDS: Vec<Command> = Vec::new();

pub unsafe fn drain(mut apply: impl FnMut(Command)) {
    for cmd in COMMANDS.drain(..) {
        apply(cmd);
    }
}

/// HOST-level intents (world lifecycle, not simulation): queued like
/// Commands, drained by the frame loop after present.
#[derive(Clone, Copy, Debug)]
pub enum HostCmd {
    LoadMap {
        map: usize,
        mod_index: usize,
        crossplay: bool,
    },
    ToMenu,
}

static mut HOST_CMDS: Vec<HostCmd> = Vec::new();

pub unsafe fn drain_host(mut apply: impl FnMut(HostCmd)) {
    for cmd in HOST_CMDS.drain(..) {
        apply(cmd);
    }
}

unsafe extern "C" fn js_load_map(
    ctx: *mut JSContext,
    _this: JSValue,
    argc: i32,
    argv: *mut JSValue,
) -> JSValue {
    let i = arg_i32(ctx, argc, argv, 0);
    let mod_index = if argc > 1 {
        arg_i32(ctx, argc, argv, 1)
    } else {
        openstrike_mods::INITIAL as i32
    };
    let crossplay = argc > 2 && arg_i32(ctx, argc, argv, 2) != 0;
    let mod_index = if crossplay { 0 } else { mod_index };
    if i >= 0 && mod_index >= 0 && openstrike_mods::get(mod_index as usize).is_some() {
        HOST_CMDS.push(HostCmd::LoadMap {
            map: i as usize,
            mod_index: mod_index as usize,
            crossplay,
        });
    }
    JS_UNDEFINED
}

unsafe extern "C" fn js_to_menu(
    _ctx: *mut JSContext,
    _this: JSValue,
    _argc: i32,
    _argv: *mut JSValue,
) -> JSValue {
    HOST_CMDS.push(HostCmd::ToMenu);
    JS_UNDEFINED
}

fn phase_name(phase: Phase) -> &'static str {
    match phase {
        Phase::Starting => "starting",
        Phase::Live => "live",
        Phase::Ended { won: true } => "won",
        Phase::Ended { won: false } => "lost",
    }
}

fn parse_phase(name: &str) -> Option<Phase> {
    Some(match name {
        "starting" => Phase::Starting,
        "live" => Phase::Live,
        "won" => Phase::Ended { won: true },
        "lost" => Phase::Ended { won: false },
        _ => return None,
    })
}

// ---- value helpers ---------------------------------------------------------

unsafe fn set_val(ctx: *mut JSContext, obj: JSValue, key: &'static [u8], val: JSValue) {
    // JS_SetPropertyStr consumes `val`.
    JS_SetPropertyStr(ctx, obj, key.as_ptr() as *const _, val);
}

unsafe fn set_str(ctx: *mut JSContext, obj: JSValue, key: &'static [u8], s: &str) {
    let v = JS_NewStringLen(ctx, s.as_ptr(), s.len());
    set_val(ctx, obj, key, v);
}

unsafe fn get_f32(ctx: *mut JSContext, obj: JSValue, key: &'static [u8], default: f32) -> f32 {
    let v = JS_GetPropertyStr(ctx, obj, key.as_ptr() as *const _);
    if JS_IsUndefined(v) {
        JS_FreeValue(ctx, v);
        return default;
    }
    let mut out = 0f64;
    let bad = JS_ToFloat64(ctx, &mut out, v) != 0;
    JS_FreeValue(ctx, v);
    if bad {
        default
    } else {
        out as f32
    }
}

unsafe fn get_i32(ctx: *mut JSContext, obj: JSValue, key: &'static [u8], default: i32) -> i32 {
    let f = get_f32(ctx, obj, key, default as f32);
    f as i32
}

unsafe fn get_u32(ctx: *mut JSContext, obj: JSValue, key: &'static [u8], default: u32) -> u32 {
    get_i32(ctx, obj, key, default as i32).max(0) as u32
}

unsafe fn arg_str_apply(ctx: *mut JSContext, argc: i32, argv: *mut JSValue, f: impl FnOnce(&str)) {
    if argc < 1 {
        return;
    }
    let mut len: size_t = 0;
    let s = JS_ToCStringLen2(ctx, &mut len, *argv, 0);
    if !s.is_null() {
        if let Ok(text) = core::str::from_utf8(core::slice::from_raw_parts(s as *const u8, len)) {
            f(text);
        }
        JS_FreeCString(ctx, s);
    }
}

unsafe extern "C" fn js_network_reply(
    ctx: *mut JSContext,
    _this: JSValue,
    argc: i32,
    argv: *mut JSValue,
) -> JSValue {
    arg_str_apply(ctx, argc, argv, |raw| {
        if raw.len() <= openstrike_core::net::PAYLOAD_LIMIT {
            COMMANDS.push(Command::NetworkReply(alloc::string::String::from(raw)));
        }
    });
    JS_UNDEFINED
}
// ---- ops --------------------------------------------------------------------

unsafe extern "C" fn js_set_phase(
    ctx: *mut JSContext,
    _this: JSValue,
    argc: i32,
    argv: *mut JSValue,
) -> JSValue {
    arg_str_apply(ctx, argc, argv, |name| {
        if let Some(p) = parse_phase(name) {
            COMMANDS.push(Command::SetPhase(p));
        }
    });
    JS_UNDEFINED
}

unsafe extern "C" fn js_reset_round(
    _ctx: *mut JSContext,
    _this: JSValue,
    _argc: i32,
    _argv: *mut JSValue,
) -> JSValue {
    COMMANDS.push(Command::ResetRound);
    JS_UNDEFINED
}

unsafe extern "C" fn js_add_win(
    _ctx: *mut JSContext,
    _this: JSValue,
    _argc: i32,
    _argv: *mut JSValue,
) -> JSValue {
    COMMANDS.push(Command::AddWin);
    JS_UNDEFINED
}

unsafe extern "C" fn js_add_loss(
    _ctx: *mut JSContext,
    _this: JSValue,
    _argc: i32,
    _argv: *mut JSValue,
) -> JSValue {
    COMMANDS.push(Command::AddLoss);
    JS_UNDEFINED
}

unsafe extern "C" fn js_set_bot_count(
    ctx: *mut JSContext,
    _this: JSValue,
    argc: i32,
    argv: *mut JSValue,
) -> JSValue {
    let n = arg_i32(ctx, argc, argv, 0).max(0) as usize;
    COMMANDS.push(Command::SetBotCount(n));
    JS_UNDEFINED
}

unsafe extern "C" fn js_configure_weapon(
    ctx: *mut JSContext,
    _this: JSValue,
    argc: i32,
    argv: *mut JSValue,
) -> JSValue {
    if argc >= 1 {
        let o = *argv;
        let d = WeaponConfig::default();
        COMMANDS.push(Command::ConfigureWeapon(WeaponConfig {
            mag_size: get_u32(ctx, o, b"magSize\0", d.mag_size),
            reserve: get_u32(ctx, o, b"reserve\0", d.reserve),
            fire_interval: get_f32(ctx, o, b"fireInterval\0", d.fire_interval),
            reload_time: get_f32(ctx, o, b"reloadTime\0", d.reload_time),
            damage_body: get_i32(ctx, o, b"damageBody\0", d.damage_body),
            damage_head: get_i32(ctx, o, b"damageHead\0", d.damage_head),
        }));
    }
    JS_UNDEFINED
}

unsafe extern "C" fn js_configure_bots(
    ctx: *mut JSContext,
    _this: JSValue,
    argc: i32,
    argv: *mut JSValue,
) -> JSValue {
    if argc >= 1 {
        let o = *argv;
        let d = BotConfig::default();
        COMMANDS.push(Command::ConfigureBots(BotConfig {
            count: get_u32(ctx, o, b"count\0", d.count as u32) as usize,
            speed: get_f32(ctx, o, b"speed\0", d.speed),
            attack_interval: get_f32(ctx, o, b"attackInterval\0", d.attack_interval),
            damage_min: get_i32(ctx, o, b"damageMin\0", d.damage_min),
            damage_max: get_i32(ctx, o, b"damageMax\0", d.damage_max),
        }));
    }
    JS_UNDEFINED
}

/// Install `globalThis.strike` (intent ops; the SDK adds `__dispatch`).
pub unsafe fn register(ctx: *mut JSContext, global: JSValue, maps: &[alloc::string::String]) {
    let obj = JS_NewObject(ctx);
    let metadata = openstrike_mods::METADATA;
    let mut source = metadata.as_bytes().to_vec();
    source.push(0);
    let mods = JS_ParseJSON(
        ctx,
        source.as_ptr() as *const _,
        metadata.len(),
        b"mods.json\0".as_ptr() as *const _,
    );
    if JS_ValueGetTag(mods) == JS_TAG_EXCEPTION {
        pocketjs_psp::host::halt("cannot initialize mod catalogue");
    }
    set_val(ctx, obj, b"mods\0", mods);
    set_val(
        ctx,
        obj,
        b"initialMod\0",
        JS_NewInt32(ctx, openstrike_mods::INITIAL as i32),
    );
    for (i, name) in STATE_NAMES.iter().enumerate() {
        STATE_ATOMS[i] = JS_NewAtom(ctx, name.as_ptr() as *const _);
    }
    #[cfg(feature = "bench")]
    add_fn(ctx, obj, b"__hudMark\0", js_hud_mark, 1);
    add_fn(ctx, obj, b"setPhase\0", js_set_phase, 1);
    add_fn(ctx, obj, b"resetRound\0", js_reset_round, 0);
    add_fn(ctx, obj, b"addWin\0", js_add_win, 0);
    add_fn(ctx, obj, b"addLoss\0", js_add_loss, 0);
    add_fn(ctx, obj, b"setBotCount\0", js_set_bot_count, 1);
    add_fn(ctx, obj, b"configureWeapon\0", js_configure_weapon, 1);
    add_fn(ctx, obj, b"configureBots\0", js_configure_bots, 1);
    add_fn(ctx, obj, b"loadMap\0", js_load_map, 3);
    add_fn(ctx, obj, b"networkReply\0", js_network_reply, 1);
    set_val(
        ctx,
        obj,
        b"networkSupported\0",
        JS_NewBool(ctx, pocketjs_psp::offload::enabled()),
    );
    add_fn(ctx, obj, b"toMenu\0", js_to_menu, 0);
    // The cooked-map catalogue (menu hosts): strike.maps = ["de_dust2", …].
    let arr = JS_NewArray(ctx);
    for (i, name) in maps.iter().enumerate() {
        let v = JS_NewStringLen(ctx, name.as_ptr(), name.len());
        JS_SetPropertyUint32(ctx, arr, i as u32, v);
    }
    set_val(ctx, obj, b"maps\0", arr);
    JS_SetPropertyStr(ctx, global, b"strike\0".as_ptr() as *const _, obj);
}

// State snapshots keep their public shape and allocation semantics. Intern
// their fixed property names once per host context instead of per tick.
const STATE_NAMES: [&[u8]; 13] = [
    b"time\0",
    b"phase\0",
    b"hp\0",
    b"alive\0",
    b"ammo\0",
    b"reserve\0",
    b"reloading\0",
    b"reloadFrac\0",
    b"aliveBots\0",
    b"totalBots\0",
    b"wins\0",
    b"losses\0",
    b"speed\0",
];
static mut STATE_ATOMS: [JSAtom; 13] = [0; 13];

unsafe fn set_state(ctx: *mut JSContext, obj: JSValue, field: usize, value: JSValue) {
    JS_SetProperty(ctx, obj, STATE_ATOMS[field], value);
}

#[cfg(feature = "bench")]
static mut HUD_START: u64 = 0;
#[cfg(feature = "bench")]
static mut HUD_TIME: u64 = 0;
#[cfg(feature = "bench")]
unsafe extern "C" fn js_hud_mark(
    ctx: *mut JSContext,
    _this: JSValue,
    argc: i32,
    argv: *mut JSValue,
) -> JSValue {
    let now = psp::sys::sceKernelGetSystemTimeWide() as u64;
    if arg_i32(ctx, argc, argv, 0) == 0 {
        HUD_START = now;
    } else {
        HUD_TIME += now.saturating_sub(HUD_START);
    }
    JS_UNDEFINED
}
#[cfg(feature = "bench")]
pub unsafe fn take_hud_time() -> u64 {
    let elapsed = HUD_TIME;
    HUD_TIME = 0;
    elapsed
}

// ---- state/events → guest ---------------------------------------------------

unsafe fn build_state(ctx: *mut JSContext, sim: &StrikeSim) -> JSValue {
    let o = JS_NewObject(ctx);
    set_state(ctx, o, 0, JS_NewFloat64(ctx, sim.time as f64));
    if let Some(network) = &sim.network {
        set_str(ctx, o, b"network\0", network.status);
        if (sim.time / openstrike_core::clock::TICK_SECONDS) as u32 % 4 == 0 {
            set_str(ctx, o, b"networkRequest\0", &network.request());
        }
    }
    set_state(
        ctx,
        o,
        1,
        JS_NewStringLen(
            ctx,
            phase_name(sim.phase).as_ptr(),
            phase_name(sim.phase).len(),
        ),
    );
    set_state(ctx, o, 2, JS_NewInt32(ctx, sim.player.health));
    set_state(ctx, o, 3, JS_NewBool(ctx, sim.player.alive));
    set_state(ctx, o, 4, JS_NewInt32(ctx, sim.weapon.ammo as i32));
    set_state(ctx, o, 5, JS_NewInt32(ctx, sim.weapon.reserve as i32));
    set_state(ctx, o, 6, JS_NewBool(ctx, sim.weapon.reloading()));
    set_state(ctx, o, 7, JS_NewFloat64(ctx, sim.reload_frac() as f64));
    set_state(ctx, o, 8, JS_NewInt32(ctx, sim.alive_bots() as i32));
    set_state(ctx, o, 9, JS_NewInt32(ctx, sim.bots.len() as i32));
    set_state(
        ctx,
        o,
        10,
        JS_NewInt32(
            ctx,
            sim.network.as_ref().map_or(sim.score.wins, |n| n.kills) as i32,
        ),
    );
    set_state(
        ctx,
        o,
        11,
        JS_NewInt32(
            ctx,
            sim.network.as_ref().map_or(sim.score.losses, |n| n.deaths) as i32,
        ),
    );
    set_state(ctx, o, 12, JS_NewFloat64(ctx, sim.ground_speed() as f64));
    o
}

unsafe fn build_event(ctx: *mut JSContext, e: &GameEvent) -> JSValue {
    let o = JS_NewObject(ctx);
    match e {
        GameEvent::Hit {
            bot,
            headshot,
            damage,
            fatal,
        } => {
            set_str(ctx, o, b"type\0", "hit");
            set_val(ctx, o, b"bot\0", JS_NewInt32(ctx, *bot as i32));
            set_val(ctx, o, b"headshot\0", JS_NewBool(ctx, *headshot));
            set_val(ctx, o, b"damage\0", JS_NewInt32(ctx, *damage));
            set_val(ctx, o, b"fatal\0", JS_NewBool(ctx, *fatal));
        }
        GameEvent::PlayerDamaged { amount, hp } => {
            set_str(ctx, o, b"type\0", "playerDamaged");
            set_val(ctx, o, b"amount\0", JS_NewInt32(ctx, *amount));
            set_state(ctx, o, 2, JS_NewInt32(ctx, *hp));
        }
        GameEvent::PlayerDied => set_str(ctx, o, b"type\0", "playerDied"),
        GameEvent::RoundReset => set_str(ctx, o, b"type\0", "roundReset"),
    }
    o
}

/// Menu-mode dispatch: no simulation exists, publish a minimal snapshot
/// with phase "menu" (and an empty event batch).
pub unsafe fn dispatch_menu(ctx: *mut JSContext, global: JSValue, time: f64) -> bool {
    let strike = JS_GetPropertyStr(ctx, global, b"strike\0".as_ptr() as *const _);
    if JS_IsUndefined(strike) {
        JS_FreeValue(ctx, strike);
        return true;
    }
    let dispatch = JS_GetPropertyStr(ctx, strike, b"__dispatch\0".as_ptr() as *const _);
    let mut ok = true;
    if !JS_IsUndefined(dispatch) {
        let o = JS_NewObject(ctx);
        set_state(ctx, o, 0, JS_NewFloat64(ctx, time));
        set_state(ctx, o, 1, JS_NewStringLen(ctx, b"menu".as_ptr(), 4));
        set_state(ctx, o, 2, JS_NewInt32(ctx, 100));
        set_state(ctx, o, 3, JS_NewBool(ctx, true));
        set_state(ctx, o, 4, JS_NewInt32(ctx, 0));
        set_state(ctx, o, 5, JS_NewInt32(ctx, 0));
        set_state(ctx, o, 6, JS_NewBool(ctx, false));
        set_state(ctx, o, 7, JS_NewFloat64(ctx, 0.0));
        set_state(ctx, o, 8, JS_NewInt32(ctx, 0));
        set_state(ctx, o, 9, JS_NewInt32(ctx, 0));
        set_state(ctx, o, 10, JS_NewInt32(ctx, 0));
        set_state(ctx, o, 11, JS_NewInt32(ctx, 0));
        set_state(ctx, o, 12, JS_NewFloat64(ctx, 0.0));
        let batch = JS_NewArray(ctx);
        let mut args = [o, batch];
        let r = JS_Call(ctx, dispatch, strike, 2, args.as_mut_ptr());
        if JS_ValueGetTag(r) == JS_TAG_EXCEPTION {
            ok = false;
        }
        JS_FreeValue(ctx, r);
        JS_FreeValue(ctx, o);
        JS_FreeValue(ctx, batch);
    }
    JS_FreeValue(ctx, dispatch);
    JS_FreeValue(ctx, strike);
    ok
}

/// One guest-ward dispatch: drain the sim's event batch and call
/// `strike.__dispatch(state, events)` if the SDK installed it.
pub unsafe fn dispatch(ctx: *mut JSContext, global: JSValue, sim: &mut StrikeSim) -> bool {
    let events = core::mem::take(&mut sim.events);
    let strike = JS_GetPropertyStr(ctx, global, b"strike\0".as_ptr() as *const _);
    if JS_IsUndefined(strike) {
        JS_FreeValue(ctx, strike);
        return true;
    }
    let dispatch = JS_GetPropertyStr(ctx, strike, b"__dispatch\0".as_ptr() as *const _);
    let mut ok = true;
    if !JS_IsUndefined(dispatch) {
        let state = build_state(ctx, sim);
        let batch = JS_NewArray(ctx);
        for (i, e) in events.iter().enumerate() {
            // JS_SetPropertyUint32 consumes the value.
            JS_SetPropertyUint32(ctx, batch, i as u32, build_event(ctx, e));
        }
        let mut args = [state, batch];
        let r = JS_Call(ctx, dispatch, strike, 2, args.as_mut_ptr());
        if JS_ValueGetTag(r) == JS_TAG_EXCEPTION {
            ok = false;
        }
        JS_FreeValue(ctx, r);
        JS_FreeValue(ctx, state);
        JS_FreeValue(ctx, batch);
    }
    JS_FreeValue(ctx, dispatch);
    JS_FreeValue(ctx, strike);
    ok
}
