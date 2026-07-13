// Build OpenStrike for PS Vita: product bundle -> cooked maps -> cargo-vita
// VPK. Toolchain installation is deliberately outside this script; it checks
// the stable VitaSDK/cargo-vita contract and gives an actionable error.
//
//   bun scripts/vita.ts                         # debug VPK, menu boot
//   bun scripts/vita.ts --release               # optimized VPK
//   bun scripts/vita.ts --map de_inferno --bench
//   OPENSTRIKE_MAPS=~/cs bun scripts/vita.ts

import { $ } from "bun";
import {
  cpSync,
  existsSync,
  mkdirSync,
  readdirSync,
  rmSync,
  statSync,
} from "node:fs";
import { compilePocketTarget, nativePocketContract } from "./pocket-contract.ts";

const repo = new URL("..", import.meta.url).pathname;
const home = process.env.HOME ?? "";
const vitaDir = `${repo}crates/openstrike-vita/`;
const argv = Bun.argv.slice(2);

function value(name: string, fallback: string): string {
  const index = argv.indexOf(`--${name}`);
  return index >= 0 && argv[index + 1] ? argv[index + 1]! : fallback;
}

const mapName = value("map", "de_dust2");
const release = argv.includes("-r") || argv.includes("--release");
const features: string[] = [];
if (argv.includes("--capture")) features.push("capture");
if (argv.includes("--bench")) features.push("bench");

const mapsRoot = process.env.OPENSTRIKE_MAPS ?? `${home}/Downloads/cs-maps-20260705-1836`;
if (!existsSync(`${mapsRoot}/maps`)) {
  console.error(`no maps dir at ${mapsRoot}/maps (set OPENSTRIKE_MAPS)`);
  process.exit(1);
}

const vitaSdk = process.env.VITASDK ?? (existsSync(`${home}/vitasdk`) ? `${home}/vitasdk` : undefined);
if (!vitaSdk || !existsSync(`${vitaSdk}/bin/vita-pack-vpk`)) {
  console.error("VitaSDK not found (set VITASDK to a complete vitasdk install)");
  process.exit(1);
}
if (!Bun.which("cargo-vita")) {
  console.error("cargo-vita not found (install with `cargo +nightly install cargo-vita`)");
  process.exit(1);
}

// 1. Validate the app contract for Vita, then compile the JS/pak from the
// immutable resolved plan. The same source manifest is also valid on PSP.
console.log("openstrike-vita: resolving and building the Pocket app contract");
const pocketPlan = await compilePocketTarget("vita");

// 2. Cook every map so the on-device menu owns the complete catalogue.
mkdirSync(`${repo}dist/maps`, { recursive: true });
const bsps = readdirSync(`${mapsRoot}/maps`)
  .filter((file) => file.endsWith(".bsp"))
  .sort();
if (bsps.length === 0) {
  console.error(`no BSP maps found under ${mapsRoot}/maps`);
  process.exit(1);
}
for (const file of bsps) {
  const stem = file.slice(0, -4);
  const source = `${mapsRoot}/maps/${file}`;
  const cooked = `${repo}dist/maps/${stem}.p3d`;
  if (existsSync(cooked) && statSync(cooked).mtimeMs > statSync(source).mtimeMs) continue;
  console.log(`openstrike-vita: cooking ${stem}`);
  await $`cargo run --release -q -p pocket3d-cook -- ${source} --wads ${mapsRoot}/support --subdivide 32 -o ${cooked} --verify`.cwd(
    `${repo}vendor/pocketjs/pocket3d`,
  );
}

// cargo-vita recursively adds `package.metadata.vita.assets`. Recreate the map
// subtree so a removed source map cannot survive in a later VPK.
const stagedMaps = `${vitaDir}static/maps`;
rmSync(stagedMaps, { recursive: true, force: true });
mkdirSync(stagedMaps, { recursive: true });
for (const file of readdirSync(`${repo}dist/maps`).filter((name) => name.endsWith(".p3d"))) {
  cpSync(`${repo}dist/maps/${file}`, `${stagedMaps}/${file}`);
}

// 3. Rust tier-3 target -> SELF -> VPK. Capture and bench builds autostart a
// map so automation never depends on menu input; retail builds show the menu.
const profile = release ? "release" : "debug";
const rustup =
  process.env.OPENSTRIKE_VITA_RUSTUP ?? Bun.which("rustup") ?? `${home}/.cargo/bin/rustup`;
if (!existsSync(rustup)) {
  console.error("rustup not found");
  process.exit(1);
}
const toolchain = process.env.OPENSTRIKE_VITA_RUST_TOOLCHAIN ?? "nightly-2026-05-28";
const cargoArgs: string[] = [];
if (release) cargoArgs.push("--release");
if (features.length) cargoArgs.push(`--features=${features.join(",")}`);
const env = {
  ...process.env,
  ...nativePocketContract(pocketPlan),
  VITASDK: vitaSdk,
  // Homebrew's cargo/rustc may precede rustup on macOS. cargo-vita needs the
  // nightly rustup proxy for every recursive cargo/rustc invocation.
  PATH: `${home}/.cargo/bin:${vitaSdk}/bin:${process.env.PATH ?? ""}`,
  TARGET_AR: "arm-vita-eabi-ar",
  AR_armv7_sony_vita_newlibeabihf: "arm-vita-eabi-ar",
  TARGET_CC: "arm-vita-eabi-gcc",
  CC_armv7_sony_vita_newlibeabihf: "arm-vita-eabi-gcc",
  TARGET_CXX: "arm-vita-eabi-g++",
  CXX_armv7_sony_vita_newlibeabihf: "arm-vita-eabi-g++",
  OPENSTRIKE_VITA_AUTOSTART:
    process.env.OPENSTRIKE_VITA_AUTOSTART ?? (features.length > 0 ? mapName : ""),
  OPENSTRIKE_VITA_CAPTURE_INPUT: process.env.OPENSTRIKE_VITA_CAPTURE_INPUT ?? "",
  OPENSTRIKE_VITA_CAP_START: process.env.OPENSTRIKE_VITA_CAP_START ?? "",
  OPENSTRIKE_VITA_CAP_N: process.env.OPENSTRIKE_VITA_CAP_N ?? "",
};

console.log(`openstrike-vita: cargo vita (map=${mapName}, profile=${profile})`);
await $`${rustup} run ${toolchain} cargo vita build vpk -- ${cargoArgs}`.cwd(vitaDir).env(env);

const artifact = `${vitaDir}target/armv7-sony-vita-newlibeabihf/${profile}/openstrike-vita.vpk`;
if (!existsSync(artifact)) {
  console.error(`cargo-vita completed but no VPK was found at ${artifact}`);
  process.exit(1);
}

const packaged = `${repo}dist/vita/OpenStrike.vpk`;
mkdirSync(`${repo}dist/vita`, { recursive: true });
cpSync(artifact, packaged);
console.log(`output: ${packaged}`);
