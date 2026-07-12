//! Embed the product bundle and freeze deterministic-run parameters into the
//! Vita binary. Cooked maps are VPK assets under `app0:/maps`, not `.rodata`.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let dist = env::var_os("POCKETJS_OUTPUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest.join("../../dist/pocket/vita"));
    let is_vita = env::var("TARGET").is_ok_and(|target| target.contains("vita"));
    let app = env::var("POCKETJS_APP_OUTPUT").unwrap_or_else(|_| "openstrike".into());
    println!("cargo:rerun-if-env-changed=POCKETJS_OUTPUT_DIR");
    println!("cargo:rerun-if-env-changed=POCKETJS_APP_OUTPUT");

    let js_src = dist.join(format!("{app}.js"));
    let pak_src = dist.join(format!("{app}.pak"));
    println!("cargo:rerun-if-changed={}", js_src.display());
    println!("cargo:rerun-if-changed={}", pak_src.display());

    let mut js = fs::read(&js_src).unwrap_or_else(|error| {
        if !is_vita {
            return Vec::new();
        }
        panic!(
            "missing {} ({error}); run `bun run build:ui` or `bun scripts/vita.ts`",
            js_src.display()
        )
    });
    // QuickJS' JS_Eval contract permits length-delimited source, but keeping a
    // sentinel also makes the raw-C host path identical to PSP.
    js.push(0);
    fs::write(out.join("game.js"), js).expect("write embedded game.js");

    let pak = fs::read(&pak_src).unwrap_or_else(|error| {
        if !is_vita {
            return Vec::new();
        }
        panic!(
            "missing {} ({error}); run `bun run build:ui` or `bun scripts/vita.ts`",
            pak_src.display()
        )
    });
    fs::write(out.join("app.pak"), pak).expect("write embedded app.pak");

    for var in [
        "OPENSTRIKE_VITA_CAPTURE_INPUT",
        "OPENSTRIKE_VITA_CAP_START",
        "OPENSTRIKE_VITA_CAP_N",
        "OPENSTRIKE_VITA_AUTOSTART",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
        println!(
            "cargo:rustc-env={var}={}",
            env::var(var).unwrap_or_default()
        );
    }
}
