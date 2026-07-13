//! Platform-facing pieces of OpenStrike's Vita target that can also be tested
//! on the host. The actual Vita frame loop is kept in `main.rs` so ordinary
//! workspace checks never need VitaSDK.

#![cfg_attr(target_os = "vita", allow(static_mut_refs))]

#[cfg(target_os = "vita")]
extern crate alloc;

// The raw strike surface is intentionally source-shared with PSP: both hosts
// expose the same QuickJS C ABI and button vocabulary. Alias this crate as the
// import name used by the PSP module, with only its two generic FFI helpers.
#[cfg(target_os = "vita")]
extern crate self as pocketjs_psp;

pub mod capture;
pub mod frame_dump;
pub mod input;
pub mod map_data;
pub mod present_data;
pub mod sim_boot;

#[cfg(target_os = "vita")]
pub mod ffi {
    use libquickjs_sys::*;

    /// Read one integer argument from QuickJS, returning zero when omitted.
    ///
    /// # Safety
    ///
    /// `ctx` and `argv` must belong to the active QuickJS context, and `argv`
    /// must expose at least `argc` values for the duration of this call.
    pub unsafe fn arg_i32(ctx: *mut JSContext, argc: i32, argv: *mut JSValue, index: isize) -> i32 {
        if index as i32 >= argc {
            return 0;
        }
        let mut value = 0;
        unsafe { JS_ToInt32(ctx, &mut value, *argv.offset(index)) };
        value
    }

    /// Attach one C callback to a QuickJS object.
    ///
    /// # Safety
    ///
    /// `ctx` and `object` must be live values from the same QuickJS context;
    /// `name` must be NUL-terminated and `function` must obey QuickJS's C
    /// callback ABI for every invocation.
    pub unsafe fn add_fn(
        ctx: *mut JSContext,
        object: JSValue,
        name: &'static [u8],
        function: unsafe extern "C" fn(*mut JSContext, JSValue, i32, *mut JSValue) -> JSValue,
        args: i32,
    ) {
        let value = unsafe {
            JS_NewCFunction2(
                ctx,
                Some(function),
                name.as_ptr().cast(),
                args,
                JS_CFUNC_generic,
                0,
            )
        };
        unsafe { JS_SetPropertyStr(ctx, object, name.as_ptr().cast(), value) };
    }
}

#[cfg(target_os = "vita")]
#[allow(clippy::manual_c_str_literals, clippy::missing_safety_doc)]
#[path = "../../openstrike-psp/src/strike.rs"]
pub mod strike;
