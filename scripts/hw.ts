// Run OpenStrike on a REAL PSP over USB (PSPLINK + usbhostfs_pc).
//
//   bun scripts/hw.ts                 # build + run de_dust2
//   bun scripts/hw.ts -r              # release profile
//   bun scripts/hw.ts --bench         # bake the frame-time probe; numbers
//                                     # stream into this terminal
//   bun scripts/hw.ts --no-build      # just (re)load what's built
//
// Serves the EBOOT dir as host0:, then `reset` + `ldstart` the PRX through
// pspsh. Enter = rebuild + reload, q = quit. With --bench, the PSP appends
// rolling 300-frame windows to host0:/OpenStrike-bench.jsonl, tailed here.

import { $ } from "bun";
import { existsSync, readFileSync, unlinkSync } from "node:fs";
import { createServer } from "node:net";
import { createInterface } from "node:readline";

const repo = new URL("..", import.meta.url).pathname;
const argv = Bun.argv.slice(2);
const flags = new Set(argv.filter((a) => a.startsWith("-")));
const release = flags.has("-r") || flags.has("--release");
const bench = flags.has("--bench");
const noBuild = flags.has("--no-build");
const profile = release ? "release" : "debug";

const usbhostfs = Bun.which("usbhostfs_pc");
const pspsh = Bun.which("pspsh");
if (!usbhostfs || !pspsh) {
  console.error("PSPLINK host tools not found on PATH (need usbhostfs_pc and pspsh).");
  process.exit(1);
}

const targetDir = `${repo}crates/openstrike-psp/target/mipsel-sony-psp/${profile}`;
const prx = "host0:/openstrike-psp.prx";
const benchPath = `${targetDir}/OpenStrike-bench.jsonl`;

function portFree(port: number): Promise<boolean> {
  return new Promise((resolve) => {
    const srv = createServer();
    srv.once("error", () => resolve(false));
    srv.once("listening", () => srv.close(() => resolve(true)));
    srv.listen(port, "127.0.0.1");
  });
}

async function findBasePort(start: number): Promise<number> {
  for (let base = start; base <= start + 3000; base += 100) {
    let ok = true;
    for (let i = 0; i <= 8; i++) {
      if (!(await portFree(base + i))) {
        ok = false;
        break;
      }
    }
    if (ok) return base;
  }
  throw new Error("no free TCP port block found for the PSPLINK link");
}

async function build(): Promise<boolean> {
  if (noBuild) return existsSync(`${targetDir}/openstrike-psp.prx`);
  const args = [...(release ? ["-r"] : []), ...(bench ? ["--bench"] : [])];
  const extra = argv.filter((a) => !a.startsWith("-"));
  const res = await $`bun ${repo}scripts/psp.ts ${args} ${extra}`.cwd(repo).nothrow();
  return res.exitCode === 0;
}

let connectCount = 0;
async function pump(stream: ReadableStream<Uint8Array>): Promise<void> {
  const dec = new TextDecoder();
  for await (const chunk of stream) {
    for (const _ of dec.decode(chunk).matchAll(/Connected to device/g)) connectCount++;
  }
}

async function waitForConnect(prev: number, timeoutMs = 20000): Promise<boolean> {
  const t0 = Date.now();
  while (connectCount <= prev) {
    if (Date.now() - t0 > timeoutMs) return false;
    await Bun.sleep(200);
  }
  return true;
}

let basePort = 0;
async function runPspsh(command: string, timeoutMs = 8000): Promise<string> {
  const child = Bun.spawn([pspsh!, "-p", String(basePort), "-e", command], {
    stdout: "pipe",
    stderr: "pipe",
  });
  const timer = setTimeout(() => child.kill(), timeoutMs);
  const [stdout, stderr] = await Promise.all([
    new Response(child.stdout).text(),
    new Response(child.stderr).text(),
    child.exited,
  ]);
  clearTimeout(timer);
  return (stdout + stderr).trim();
}

async function load(): Promise<void> {
  if (existsSync(benchPath)) unlinkSync(benchPath);
  const prev = connectCount;
  process.stdout.write("resetting PSPLINK... ");
  await runPspsh("reset");
  console.log((await waitForConnect(prev)) ? "connected." : "timeout; using current session.");
  const out = await runPspsh(`ldstart ${prx}`);
  console.log("  " + (out || "(no output)"));
}

if (!(await build())) process.exit(1);
basePort = await findBasePort(Number(process.env.PSP_HW_PORT ?? 10000));

console.log(`serving ${targetDir.replace(repo, "")} as host0: on port ${basePort}`);
const child = Bun.spawn([usbhostfs, "-b", String(basePort), targetDir], {
  stdout: "pipe",
  stderr: "pipe",
});
void pump(child.stdout);
void pump(child.stderr);
process.on("SIGINT", () => {
  child.kill();
  process.exit(0);
});

console.log("waiting for the PSP... launch PSPLINK on it (XMB -> Game -> PSPLINK).");
if (!(await waitForConnect(0, 120000))) {
  console.error("PSP never connected. Check the USB cable and that PSPLINK is running.");
  child.kill();
  process.exit(1);
}
console.log("PSP connected.");
await load();

// Tail bench windows as the device writes them over usbhostfs.
if (bench) {
  let seen = 0;
  setInterval(() => {
    if (!existsSync(benchPath)) return;
    const lines = readFileSync(benchPath, "utf8").trimEnd().split("\n").filter(Boolean);
    for (; seen < lines.length; seen++) {
      try {
        const w = JSON.parse(lines[seen]);
        const fps = Math.min(60, Math.round(1e6 / Math.max(w.avg_work_us, w.avg_gpu_us, 16667)));
        const seg =
          w.avg_sim_us !== undefined
            ? `  [sim ${w.avg_sim_us} dispatch ${w.avg_dispatch_us} js ${w.avg_js_us} ui ${w.avg_ui_us}]`
            : "";
        console.log(
          `[bench] work ${w.avg_work_us}us (max ${w.max_work_us})  gpu ${w.avg_gpu_us}us (max ${w.max_gpu_us})  ` +
            `faces ${w.avg_faces}  tris ${w.avg_tris}${seg}  → ~${fps} fps`,
        );
      } catch {
        // partial line; retry next poll
        break;
      }
    }
  }, 1000);
}

if (flags.has("--daemon")) {
  // Agent/headless sessions: keep the usbhostfs link + bench tail alive;
  // reload from another terminal via `pspsh -p <port> -e "ldstart ..."`.
  console.log(`[OpenStrike:hw] daemon mode; pspsh port ${basePort}`);
  await new Promise(() => {});
}

console.log("\n[OpenStrike:hw] Enter = rebuild + reload  |  q + Enter = quit\n");
const rl = createInterface({ input: process.stdin });
for await (const line of rl) {
  const cmd = line.trim().toLowerCase();
  if (cmd === "q" || cmd === "quit") break;
  if (await build()) await load();
  console.log("\n[OpenStrike:hw] Enter = rebuild + reload  |  q + Enter = quit\n");
}
rl.close();
child.kill();
process.exit(0);
