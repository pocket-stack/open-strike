// Editable Blender scene -> Valve 220 brushes/WAD -> GoldSrc BSP -> Pocket3D.
import { createHash } from "node:crypto";
import { cpSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const SDHLT_REVISION = "df45198b3c03a5a09e9d1aead9c9457e51753e39";
const hash = (path: string) => createHash("sha256").update(readFileSync(path)).digest("hex");

async function run(cmd: string[], cwd: string, log?: string) {
  console.log(`map-toolchain: ${basename(cmd[0])} ${cmd.slice(1, 4).join(" ")}`);
  const child = Bun.spawn(cmd, { cwd, stdout: "pipe", stderr: "pipe" });
  const [stdout, stderr, code] = await Promise.all([new Response(child.stdout).text(), new Response(child.stderr).text(), child.exited]);
  if (log) writeFileSync(log, stdout + stderr);
  if (code !== 0) throw new Error(`${cmd[0]} failed (${code}):\n${(stdout + stderr).slice(-5000)}`);
  return stdout;
}

export async function mapCompiler(explicit?: string): Promise<string> {
  const names = ["sdHLCSG", "sdHLBSP", "sdHLVIS", "sdHLRAD"];
  if (explicit) {
    const dir = resolve(explicit);
    if (!names.every((name) => existsSync(join(dir, name)))) throw new Error(`Incomplete SDHLT directory: ${dir}`);
    return dir;
  }
  if (!["darwin", "linux"].includes(process.platform)) throw new Error("The map compiler bootstrap supports macOS and Linux");
  const cache = join(root, ".pocket", "toolchains", "sdhlt", SDHLT_REVISION);
  const source = join(cache, "source"), build = join(cache, "build");
  mkdirSync(cache, { recursive: true });
  if (!existsSync(join(source, ".git"))) {
    await run(["git", "clone", "--filter=blob:none", "--no-checkout", "https://github.com/seedee/SDHLT.git", source], cache);
    await run(["git", "checkout", "--detach", SDHLT_REVISION], source);
  }
  if ((await run(["git", "rev-parse", "HEAD"], source)).trim() !== SDHLT_REVISION) throw new Error("Unexpected SDHLT source revision");
  const patch = join(root, "scripts", "sdhlt-portable.patch");
  const receipt = join(cache, "build.json"), signature = JSON.stringify({ source: SDHLT_REVISION, patch: hash(patch), script: hash(fileURLToPath(import.meta.url)), platform: process.platform, arch: process.arch });
  if (existsSync(receipt) && readFileSync(receipt, "utf8") === signature && names.every((name) => existsSync(join(source, "tools", name)))) return join(source, "tools");
  const check = Bun.spawn(["git", "apply", "--reverse", "--check", patch], { cwd: source, stdout: "ignore", stderr: "ignore" });
  if (await check.exited !== 0) await run(["git", "apply", patch], source);
  const flags = "-DSYSTEM_POSIX -DSTDC_HEADERS -DHAVE_FCNTL_H -DHAVE_PTHREAD_H -DHAVE_STDDEF_H -DHAVE_SYS_RESOURCE_H -DHAVE_SYS_STAT_H -DHAVE_SYS_TIME_H -DHAVE_UNISTD_H -Wno-register -fno-strict-aliasing";
  await run(["cmake", "-S", source, "-B", build, "-DCMAKE_BUILD_TYPE=Release", "-DCMAKE_CXX_STANDARD=17", `-DCMAKE_CXX_FLAGS=${flags}`], cache, join(cache, "configure.log"));
  await run(["cmake", "--build", build, "--parallel", "8"], cache, join(cache, "build.log"));
  writeFileSync(receipt, signature);
  return join(source, "tools");
}

async function main() {
  const args = Bun.argv.slice(2);
  const value = (key: string) => {
    const i = args.indexOf(key);
    if (i < 0) return undefined;
    if (!args[i + 1] || args[i + 1].startsWith("--")) throw new Error(`${key} needs a value`);
    return args[i + 1];
  };
  if (args.includes("--help")) {
    console.log("bun scripts/blender-map.ts (--demo | --blend scene.blend) [--out out/map] [--sdhlt-dir DIR] [--blender PATH] [--stage]");
    return;
  }
  if (args.includes("--demo") === !!value("--blend")) throw new Error("Choose --demo or --blend scene.blend");
  const out = resolve(value("--out") ?? "out/parkour");
  mkdirSync(out, { recursive: true });
  const blender = value("--blender") ?? process.env.BLENDER ?? Bun.which("blender") ?? "/Applications/Blender.app/Contents/MacOS/Blender";
  if (!existsSync(blender)) throw new Error("Blender not found; set BLENDER or pass --blender");
  const scene = args.includes("--demo") ? join(out, "wwdc24-parkour.blend") : resolve(value("--blend")!);
  if (args.includes("--demo")) await run([blender, "--background", "--factory-startup", "--python-exit-code", "1", "--python", join(root, "scripts/build-parkour.py"), "--", "--out", out], root, join(out, "blender.log"));
  const map = join(out, basename(scene, ".blend") + ".map");
  await run([blender, "--background", scene, "--python-exit-code", "1", "--python", join(root, "scripts/blender-to-map.py"), "--", "--out", map], root, join(out, "export.log"));
  const tools = await mapCompiler(value("--sdhlt-dir"));
  for (const [tool, options] of [
    ["sdHLCSG", ["-nowadtextures", "-threads", "4"]],
    ["sdHLBSP", ["-threads", "4"]],
    ["sdHLVIS", ["-threads", "4"]],
    ["sdHLRAD", ["-threads", "4", "-chop", "128", "-texchop", "128", "-bounce", "4", "-scale", "1", "-gamma", "0.8", "-ambient", "0.08", "0.09", "0.11"]],
  ] as const) await run([join(tools, tool), ...options, map], out, join(out, tool + ".log"));
  const bsp = map.replace(/\.map$/, ".bsp"), cooked = map.replace(/\.map$/, ".p3d");
  if (readFileSync(bsp).readInt32LE(0) !== 30) throw new Error("Expected GoldSrc BSP version 30");
  await run(["cargo", "run", "--release", "--locked", "-q", "-p", "pocket3d-cook", "--", bsp, "--subdivide", "64", "--verify", "-o", cooked], join(root, "vendor/pocketjs/engine/pocket3d"), join(out, "cook.log"));
  const files = [scene, map, map.replace(/\.map$/, ".wad"), bsp, cooked];
  writeFileSync(join(out, "build.json"), JSON.stringify({
    compiler: { revision: value("--sdhlt-dir") ? "external; inspect binary hashes" : SDHLT_REVISION, binaries: Object.fromEntries(["sdHLCSG", "sdHLBSP", "sdHLVIS", "sdHLRAD"].map((name) => [name, hash(join(tools, name))])) },
    files: Object.fromEntries(files.map((path) => [basename(path), { sha256: hash(path), bytes: readFileSync(path).length }])),
  }, null, 2));
  if (args.includes("--stage")) {
    mkdirSync(join(root, "dist/maps"), { recursive: true });
    cpSync(cooked, join(root, "dist/maps", basename(cooked)));
  }
  console.log(`Map ready: ${cooked}`);
}

if (import.meta.main) await main();
