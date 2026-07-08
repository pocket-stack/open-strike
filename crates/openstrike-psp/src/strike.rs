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
    fn JS_NewStringLen(ctx: *mut JSContext, s: *const u8, len: usize) -> JSValue;
    fn JS_NewArray(ctx: *mut JSContext) -> JSValue;
    fn JS_SetPropertyUint32(ctx: *mut JSContext, this_obj: JSValue, idx: u32, val: JSValue)
    -> i32;
}

/// Commands queued by ops during the guest turn (single-threaded host).
static mut COMMANDS: Vec<Command> = Vec::new();

pub unsafe fn drain(mut apply: impl FnMut(Command)) {
    for cmd in COMMANDS.drain(..) {
        apply(cmd);
    }
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
    if bad { default } else { out as f32 }
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
pub unsafe fn register(ctx: *mut JSContext, global: JSValue) {
    let obj = JS_NewObject(ctx);
    add_fn(ctx, obj, b"setPhase\0", js_set_phase, 1);
    add_fn(ctx, obj, b"resetRound\0", js_reset_round, 0);
    add_fn(ctx, obj, b"addWin\0", js_add_win, 0);
    add_fn(ctx, obj, b"addLoss\0", js_add_loss, 0);
    add_fn(ctx, obj, b"setBotCount\0", js_set_bot_count, 1);
    add_fn(ctx, obj, b"configureWeapon\0", js_configure_weapon, 1);
    add_fn(ctx, obj, b"configureBots\0", js_configure_bots, 1);
    JS_SetPropertyStr(ctx, global, b"strike\0".as_ptr() as *const _, obj);
}

// ---- state/events → guest ---------------------------------------------------

unsafe fn build_state(ctx: *mut JSContext, sim: &StrikeSim) -> JSValue {
    let o = JS_NewObject(ctx);
    set_val(ctx, o, b"time\0", JS_NewFloat64(ctx, sim.time as f64));
    set_str(ctx, o, b"phase\0", phase_name(sim.phase));
    set_val(ctx, o, b"hp\0", JS_NewInt32(ctx, sim.player.health));
    set_val(ctx, o, b"alive\0", JS_NewBool(ctx, sim.player.alive));
    set_val(ctx, o, b"ammo\0", JS_NewInt32(ctx, sim.weapon.ammo as i32));
    set_val(ctx, o, b"reserve\0", JS_NewInt32(ctx, sim.weapon.reserve as i32));
    set_val(ctx, o, b"reloading\0", JS_NewBool(ctx, sim.weapon.reloading()));
    set_val(ctx, o, b"reloadFrac\0", JS_NewFloat64(ctx, sim.reload_frac() as f64));
    set_val(ctx, o, b"aliveBots\0", JS_NewInt32(ctx, sim.alive_bots() as i32));
    set_val(ctx, o, b"totalBots\0", JS_NewInt32(ctx, sim.bots.len() as i32));
    set_val(ctx, o, b"wins\0", JS_NewInt32(ctx, sim.score.wins as i32));
    set_val(ctx, o, b"losses\0", JS_NewInt32(ctx, sim.score.losses as i32));
    set_val(ctx, o, b"speed\0", JS_NewFloat64(ctx, sim.ground_speed() as f64));
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
            set_val(ctx, o, b"hp\0", JS_NewInt32(ctx, *hp));
        }
        GameEvent::PlayerDied => set_str(ctx, o, b"type\0", "playerDied"),
        GameEvent::RoundReset => set_str(ctx, o, b"type\0", "roundReset"),
    }
    o
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
