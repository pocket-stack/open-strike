// Build the OpenStrike PSP EBOOT: JS bundle → cooked map → cargo psp.
//
//   bun scripts/psp.ts                     # de_dust2, debug profile
//   bun scripts/psp.ts -r                  # release
//   bun scripts/psp.ts --map de_inferno --bots 4
//   OPENSTRIKE_MAPS=~/cs bun scripts/psp.ts
//
// Maps root (maps/*.bsp + support/*.wad) comes from OPENSTRIKE_MAPS or the
// local default. The PSP SDK is discovered like vendor/pocketjs/scripts/
// psp.ts (PSP_SDK env or the dreamcart checkout); PSPDEV is exported for
// libquickjs-sys's own include resolution.

import { $ } from "bun";
import { cpSync, existsSync, mkdirSync, readdirSync, rmSync, statSync } from "node:fs";
import { compilePocketTarget, nativePocketContract } from "./pocket-contract.ts";

const repo = new URL("..", import.meta.url).pathname;
const home = process.env.HOME ?? "";
const pspDir = `${repo}crates/openstrike-psp/`;

const argv = Bun.argv.slice(2);
function flag(name: string, def: string): string {
  const i = argv.indexOf(`--${name}`);
  return i !== -1 && argv[i + 1] ? argv[i + 1] : def;
}
const mapName = flag("map", "de_dust2");
const release = argv.includes("-r") || argv.includes("--release");
const features: string[] = [];
if (argv.includes("--capture")) features.push("capture");
if (argv.includes("--bench")) features.push("bench");

const mapsRoot = process.env.OPENSTRIKE_MAPS ?? `${home}/Downloads/cs-maps-20260705-1836`;
if (!existsSync(`${mapsRoot}/maps`)) {
  console.error(`no maps dir at ${mapsRoot}/maps (set OPENSTRIKE_MAPS)`);
  process.exit(1);
}

// ---- 1. product bundle (rules + JSX HUD) --------------------------------
console.log("openstrike-psp: resolving and building the Pocket app contract");
const pocketPlan = await compilePocketTarget("psp");

// ---- 2. cook EVERY map (the menu lists them all) -------------------------
// 32-unit light grid: samples every other GoldSrc luxel — crisp baked
// shadows for ~0.9 MB more map (GE headroom is huge, this is cheap).
mkdirSync(`${repo}dist/maps`, { recursive: true });
const bsps = readdirSync(`${mapsRoot}/maps`)
  .filter((f) => f.endsWith(".bsp"))
  .sort();
for (const f of bsps) {
  const stem = f.slice(0, -4);
  const src = `${mapsRoot}/maps/${f}`;
  const p3d = `${repo}dist/maps/${stem}.p3d`;
  if (existsSync(p3d) && statSync(p3d).mtimeMs > statSync(src).mtimeMs) continue;
  console.log(`openstrike-psp: cooking ${stem}`);
  await $`cargo run --release -q -p pocket3d-cook -- ${src} --wads ${mapsRoot}/support --subdivide 32 -o ${p3d} --verify`.cwd(
    `${repo}vendor/pocketjs/pocket3d`,
  );
}

// ---- 3. cargo psp ---------------------------------------------------------
const sdkCandidates = [
  process.env.PSP_SDK,
  `${home}/code/dreamcart/mipsel-sony-psp`,
].filter((p): p is string => !!p);
const sdk = sdkCandidates.find((p) => existsSync(`${p}/psp/lib/libc.a`));
if (!sdk) {
  console.error("PSP SDK not found (set PSP_SDK)");
  process.exit(1);
}
const llvm = existsSync("/opt/homebrew/opt/llvm/bin")
  ? "/opt/homebrew/opt/llvm/bin"
  : "/usr/local/opt/llvm/bin";
const TOOLCHAIN = "nightly-2026-05-28";
const rustup = Bun.which("rustup") ?? `${home}/.cargo/bin/rustup`;

const env = {
  ...process.env,
  ...nativePocketContract(pocketPlan),
  PATH: `${llvm}:${home}/.cargo/bin:${process.env.PATH}`,
  // newlib (QuickJS needs -lc) and rust-psp both define memcpy/_exit/truncf
  // with identical semantics; whichever the linker sees first wins.
  RUSTFLAGS:
    "-A linker-messages -A unexpected-cfgs -A unstable-name-collisions -C link-arg=--allow-multiple-definition",
  CRATE_CC_NO_DEFAULTS: "1",
  TARGET_CC: "clang",
  TARGET_AR: `${llvm}/llvm-ar`,
  TARGET_CFLAGS:
    `-target mipsel-sony-psp -mcpu=mips2 -msingle-float -mlittle-endian -mno-abicalls -fno-pic -G0 -mno-check-zero-division ` +
    `-fno-stack-protector -I${sdk}/psp/include -I${sdk}/psp/sdk/include`,
  AR_mipsel_sony_psp: `${llvm}/llvm-ar`,
  RANLIB_mipsel_sony_psp: `${llvm}/llvm-ranlib`,
  // libquickjs-sys resolves the SDK through PSPDEV (…/psp appended).
  PSPDEV: sdk,
  RUST_PSP_TARGET: `${repo}vendor/pocketjs/native/targets/mipsel-sony-psp.json`,
  RUST_PSP_ABORT_ONLY: "1",
  CARGO_PROFILE_DEV_OPT_LEVEL: process.env.CARGO_PROFILE_DEV_OPT_LEVEL ?? "3",
  // e2e/bench builds skip the menu and boot straight into --map; interactive
  // builds boot into the menu.
  OPENSTRIKE_PSP_AUTOSTART:
    process.env.OPENSTRIKE_PSP_AUTOSTART ??
    (features.length > 0 ? mapName : ""),
  // pocketjs-psp's build.rs runs as a dependency; nativePocketContract keeps
  // the JS bundle and custom host on the same resolved target contract.
  POCKETJS_CAPTURE_INPUT: "",
  POCKETJS_TRACE: process.env.POCKETJS_TRACE ?? "",
  POCKETJS_CAP_START: "",
  POCKETJS_CAP_N: "",
  POCKETJS_ARENA_BYTES: process.env.POCKETJS_ARENA_BYTES ?? "",
  POCKETJS_BENCH_DUMP_FRAMES: "",
  OPENSTRIKE_PSP_CAPTURE_INPUT: process.env.OPENSTRIKE_PSP_CAPTURE_INPUT ?? "",
  OPENSTRIKE_PSP_CAP_START: process.env.OPENSTRIKE_PSP_CAP_START ?? "",
  OPENSTRIKE_PSP_CAP_N: process.env.OPENSTRIKE_PSP_CAP_N ?? "",
};

const cargoArgs: string[] = [];
if (release) cargoArgs.push("--release");
if (features.length) cargoArgs.push(`--features=${features.join(",")}`);
console.log(`openstrike-psp: cargo psp (map=${mapName})`);
await $`${rustup} run ${TOOLCHAIN} cargo psp ${cargoArgs}`.cwd(pspDir).env(env);

const profile = release ? "release" : "debug";
const ebootDir = `${pspDir}target/mipsel-sony-psp/${profile}`;
mkdirSync(`${ebootDir}/maps`, { recursive: true });
for (const f of readdirSync(`${repo}dist/maps`).filter((f) => f.endsWith(".p3d"))) {
  cpSync(`${repo}dist/maps/${f}`, `${ebootDir}/maps/${f}`);
}
const named = `${ebootDir}/openstrike-psp.EBOOT.PBP`;
if (existsSync(named)) {
  await Bun.write(`${ebootDir}/EBOOT.PBP`, await Bun.file(named).arrayBuffer());
}
if (!existsSync(`${ebootDir}/EBOOT.PBP`)) {
  console.error(`no EBOOT.PBP under ${ebootDir}`);
  process.exit(1);
}
console.log(`output: ${ebootDir}/EBOOT.PBP`);

// ---- 4. Memory-Stick package (--package) --------------------------------
// Assemble the drop-on-a-PSP layout: PSP/GAME/OpenStrike/{EBOOT.PBP, maps/}.
// The EBOOT already carries ICON0/PIC1 (Psp.toml), so a homebrew-enabled XMB
// shows the branded entry; maps/ sits beside it for the runtime loader.
if (argv.includes("--package")) {
  const pkg = `${repo}dist/PSP/GAME/OpenStrike`;
  rmSync(`${repo}dist/PSP`, { recursive: true, force: true });
  mkdirSync(`${pkg}/maps`, { recursive: true });
  cpSync(`${ebootDir}/EBOOT.PBP`, `${pkg}/EBOOT.PBP`);
  for (const f of readdirSync(`${repo}dist/maps`).filter((f) => f.endsWith(".p3d"))) {
    cpSync(`${repo}dist/maps/${f}`, `${pkg}/maps/${f}`);
  }
  console.log(`packaged: ${pkg}/  (copy dist/PSP/ to a Memory Stick root)`);
}
