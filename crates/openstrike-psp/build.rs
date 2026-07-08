//! Embeds the product bundle (dist/openstrike.{js,pak}), the cooked map
//! (OPENSTRIKE_PSP_MAP, a .p3d path), and the capture window/script envs.
//! Empty fallbacks keep bare builds green.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let dist = manifest.join("../../dist");

    // game.js gets a trailing NUL for JS_Eval (input[len] == '\0').
    let js_src = dist.join("openstrike.js");
    println!("cargo:rerun-if-changed={}", js_src.display());
    let mut js = fs::read(&js_src).unwrap_or_default();
    js.push(0);
    fs::write(out.join("game.js"), js).unwrap();

    let pak_src = dist.join("openstrike.pak");
    println!("cargo:rerun-if-changed={}", pak_src.display());
    let pak = fs::read(&pak_src).unwrap_or_default();
    fs::write(out.join("app.pak"), pak).unwrap();

    let map = env::var("OPENSTRIKE_PSP_MAP").unwrap_or_default();
    println!("cargo:rerun-if-env-changed=OPENSTRIKE_PSP_MAP");
    let dst = out.join("map.p3d");
    if !map.is_empty() && Path::new(&map).exists() {
        println!("cargo:rerun-if-changed={map}");
        fs::copy(&map, &dst).expect("copying OPENSTRIKE_PSP_MAP");
    } else {
        fs::write(&dst, []).expect("writing empty map.p3d");
    }

    for var in [
        "OPENSTRIKE_PSP_CAPTURE_INPUT",
        "OPENSTRIKE_PSP_CAP_START",
        "OPENSTRIKE_PSP_CAP_N",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
        let v = env::var(var).unwrap_or_default();
        println!("cargo:rustc-env={var}={v}");
    }
}
