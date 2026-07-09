//! Embeds the product bundle (dist/openstrike.{js,pak}) and the capture /
//! autostart envs. Maps are NOT embedded: the EBOOT loads cooked .p3d files
//! from maps/ next to itself (see src/maps.rs).

use std::env;
use std::fs;
use std::path::PathBuf;

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

    for var in [
        "OPENSTRIKE_PSP_CAPTURE_INPUT",
        "OPENSTRIKE_PSP_CAP_START",
        "OPENSTRIKE_PSP_CAP_N",
        "OPENSTRIKE_PSP_AUTOSTART",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
        let v = env::var(var).unwrap_or_default();
        println!("cargo:rustc-env={var}={v}");
    }
}
