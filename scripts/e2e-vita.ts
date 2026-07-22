// Native PS Vita E2E: build a capture VPK, stage it into an isolated VitaFS,
// run the real QuickJS/input/simulation/render loop in Vita3K, then compare
// deterministic 960x544 HUD captures and assert live Pocket3D scene stats.
//
//   bun scripts/e2e-vita.ts            # compare checked-in goldens
//   UPDATE=1 bun scripts/e2e-vita.ts   # intentionally re-baseline
//
// The current macOS Vita3K Vulkan backend does not expose a coherent guest
// color buffer after presentation. PocketJS capture builds therefore raster
// the same DrawList deterministically at Vita's native density, while `.scene`
// sidecars prove that the native Pocket3D pass submitted visible world data.

import { $ } from "bun";
import {
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
} from "node:fs";
import {
  DEFAULT_VITA_PACKAGE_ASSETS,
  VITA_REQUIRED_SYSTEM_ASSETS,
  VITA_SYSTEM_ASSET_PATHS,
} from "../vendor/pocketjs/scripts/vita-package.ts";

const repo = new URL("..", import.meta.url).pathname;
const home = process.env.HOME ?? "";
const update = process.env.UPDATE === "1";
const goldens = `${repo}test/goldens-vita`;
const outDir = `${repo}out/e2e-vita`;
const profile = `${outDir}/vita3k`;
const vitaFs = `${profile}/vitafs`;
const configDir = `${profile}/config`;
const configFile = `${configDir}/config.yml`;
const titleId = "OPSK00001";
const appDir = `${vitaFs}/ux0/app/${titleId}`;
const capDir = `${vitaFs}/ux0/data/openstrike-vita/cap`;
const packageIcon = `${repo}crates/openstrike-vita/static/sce_sys/icon0.png`;
const baseConfigCandidates = [
  process.env.VITA3K_CONFIG,
  `${home}/Library/Application Support/Vita3K/Vita3K/config.yml`,
  `${home}/Library/Application Support/Vita3K/config.yml`,
].filter((path): path is string => Boolean(path));
const baseConfig = baseConfigCandidates.find(existsSync);
let activeValidatorCleanup: (() => void) | undefined;
let activeEmulator: { kill(signal?: number | NodeJS.Signals): void } | undefined;

for (const [signal, code] of [
  ["SIGINT", 130],
  ["SIGTERM", 143],
] as const) {
  process.on(signal, () => {
    activeEmulator?.kill("SIGTERM");
    activeValidatorCleanup?.();
    process.exit(code);
  });
}

interface Spec {
  name: string;
  /** frame:mask:lx:ly:rx:ry; the latest entry at/before a frame wins. */
  input: string;
  capStart: number;
  capN: number;
  shots: number[];
  lookProbe?: {
    from: number;
    to: number;
    yaw: "decrease" | "increase";
    minYawDelta: number;
    maxPitchDrift: number;
  };
}

const R = 0x0200;
const SPECS: Spec[] = [
  { name: "spawn", input: "0:0", capStart: 96, capN: 4, shots: [0] },
  {
    name: "walk",
    input: "0:0,80:0:128:20,150:0:128:20:220:128,190:0:128:96",
    capStart: 150,
    capN: 44,
    shots: [0, 40],
    // The right stick is held right from global frame 150 through 189. Prove
    // that the native pad path reaches the simulation camera, independently
    // of the image golden (which could otherwise change for another reason).
    lookProbe: {
      from: 0,
      to: 40,
      yaw: "decrease",
      minYawDelta: 0.75,
      maxPitchDrift: 0.001,
    },
  },
  { name: "fire", input: `0:0,90:${R}`, capStart: 92, capN: 12, shots: [1, 8] },
];
const selectedSpecs = process.env.VITA_E2E_SPEC
  ? SPECS.filter((spec) => spec.name === process.env.VITA_E2E_SPEC)
  : SPECS;
if (selectedSpecs.length === 0) {
  console.error(`unknown VITA_E2E_SPEC=${process.env.VITA_E2E_SPEC}`);
  process.exit(1);
}

const vita3kCandidates = [
  process.env.VITA3K,
  `${home}/Applications/Vita3K.app/Contents/MacOS/Vita3K`,
  "/Applications/Vita3K.app/Contents/MacOS/Vita3K",
].filter((path): path is string => Boolean(path));
const vita3k = vita3kCandidates.find(existsSync);
if (!vita3k) {
  console.error(`Vita3K not found (set VITA3K; checked ${vita3kCandidates.join(", ")})`);
  process.exit(1);
}
if (!baseConfig) {
  console.error(
    `Vita3K config not found (set VITA3K_CONFIG; checked ${baseConfigCandidates.join(", ")})`,
  );
  process.exit(1);
}
if (!Bun.which("magick")) {
  console.error("ImageMagick `magick` is required for Vita golden conversion");
  process.exit(1);
}
if (!Bun.which("unzip")) {
  console.error("`unzip` is required to stage the VPK into the isolated VitaFS");
  process.exit(1);
}
mkdirSync(goldens, { recursive: true });
mkdirSync(outDir, { recursive: true });

async function prepareProfile(vpk: string): Promise<void> {
  rmSync(appDir, { recursive: true, force: true });
  mkdirSync(appDir, { recursive: true });
  await $`unzip -oq ${vpk} -d ${appDir}`.quiet();
  for (const path of VITA_REQUIRED_SYSTEM_ASSETS) {
    const installed = `${appDir}/${path}`;
    const expected = path === VITA_SYSTEM_ASSET_PATHS.icon
      ? packageIcon
      : `${DEFAULT_VITA_PACKAGE_ASSETS}/${path}`;
    if (!existsSync(installed) || !readFileSync(installed).equals(readFileSync(expected))) {
      throw new Error(`OpenStrike VPK is missing its byte-exact ${path}`);
    }
  }
  mkdirSync(`${vitaFs}/ux0/data`, { recursive: true });
  mkdirSync(`${vitaFs}/ux0/user/00`, { recursive: true });
  await Bun.write(
    `${vitaFs}/ux0/user/time.xml`,
    '<?xml version="1.0" encoding="utf-8"?>\n<time><user id="00" /></time>\n',
  );
  await Bun.write(
    `${vitaFs}/ux0/user/00/user.xml`,
    '<?xml version="1.0" encoding="utf-8"?>\n<user id="00" name="Vita3K"><theme use-background="true"><content-id>default</content-id></theme><start-screen type="default"><path></path></start-screen><backgrounds /></user>\n',
  );

  mkdirSync(configDir, { recursive: true });
  let config = readFileSync(baseConfig!, "utf8");
  const set = (key: string, value: string): void => {
    const line = new RegExp(`^${key}:.*$`, "m");
    if (!line.test(config)) throw new Error(`Vita3K base config is missing ${key}`);
    config = config.replace(line, `${key}: ${value}`);
  };
  set("initial-setup", "false");
  set("backend-renderer", "Vulkan");
  set("resolution-multiplier", "1");
  set("screen-filter", "Nearest");
  set("v-sync", "false");
  set("memory-mapping", "double-buffer");
  set("disable-surface-sync", "false");
  set("modules-mode", "2");
  set("pref-path", JSON.stringify(vitaFs));
  set("show-live-area-screen", "false");
  set("boot-apps-full-screen", "false");
  set("show-welcome", "false");
  set("warn-missing-firmware", "false");
  set("check-for-updates-mode", "0");
  set("discord-rich-presence", "false");
  set("log-level", "0");
  config = config.replace(/^lle-modules:\n(?:[ \t].*\n)*/m, "lle-modules: []\n");
  await Bun.write(configFile, config);
}

function globalVitaFs(): string {
  const config = readFileSync(baseConfig!, "utf8");
  const encoded = config.match(/^pref-path:\s*(.+?)\s*$/m)?.[1];
  if (!encoded) throw new Error("Vita3K base config is missing pref-path");
  if (encoded.startsWith('"') && encoded.endsWith('"')) return JSON.parse(encoded);
  if (encoded.startsWith("'") && encoded.endsWith("'")) return encoded.slice(1, -1);
  return encoded;
}

function exposeTitleToCliValidator(): () => void {
  // Vita3K snapshots the global VitaFS app list while defining CLI options,
  // before it parses --config-location. Expose only an empty title directory
  // there so `-r` validates; all actual content stays in the isolated VitaFS.
  const placeholder = `${globalVitaFs()}/ux0/app/${titleId}`;
  const created = !existsSync(placeholder);
  if (created) mkdirSync(placeholder, { recursive: true });
  let cleaned = false;
  const cleanup = () => {
    if (cleaned) return;
    cleaned = true;
    if (created) rmSync(placeholder, { recursive: true, force: true });
    if (activeValidatorCleanup === cleanup) activeValidatorCleanup = undefined;
  };
  activeValidatorCleanup = cleanup;
  return cleanup;
}

function capturedFrames(): string[] {
  if (!existsSync(capDir)) return [];
  return readdirSync(capDir)
    .filter((name) => /^f\d{4}\.rgba$/.test(name))
    .sort();
}

async function launchAndWait(expectedFrames: number): Promise<string[]> {
  const hideTitle = exposeTitleToCliValidator();
  try {
    const proc = Bun.spawn(
      [
        vita3k!,
        "--keep-config",
        "--load-config",
        "--config-location",
        configFile,
        "-r",
        titleId,
      ],
      {
        cwd: repo,
        stdout: "pipe",
        stderr: "pipe",
        env: process.env,
      },
    );
    activeEmulator = proc;
    const stdoutPromise = new Response(proc.stdout).text();
    const stderrPromise = new Response(proc.stderr).text();
    const deadline = Date.now() + 180_000;
    const done = `${capDir}/done`;
    const errorFile = `${capDir}/error.txt`;

    while (Date.now() < deadline && !existsSync(done) && !existsSync(errorFile)) {
      if (await Promise.race([proc.exited.then(() => true), Bun.sleep(100).then(() => false)])) {
        break;
      }
    }

    const frames = capturedFrames();
    const appError = existsSync(errorFile) ? readFileSync(errorFile, "utf8") : "";
    // Capture apps deliberately remain alive after writing `done`: current
    // Vita3K builds can fault while emulating sceKernelExitProcess cleanup.
    // The marker is the authoritative terminal event; terminate the emulator.
    if (proc.exitCode === null) proc.kill("SIGTERM");
    const stopped = await Promise.race([
      proc.exited.then(() => true),
      Bun.sleep(5_000).then(() => false),
    ]);
    if (!stopped && proc.exitCode === null) {
      proc.kill("SIGKILL");
      await proc.exited;
    }
    const [stdout, stderr] = await Promise.all([stdoutPromise, stderrPromise]);

    if (!existsSync(done) || frames.length !== expectedFrames || appError) {
      const log = [stdout, stderr].filter(Boolean).join("\n").slice(-16_000);
      throw new Error(
        `${appError ? `app error: ${appError}\n` : ""}` +
          `Vita3K produced ${frames.length}/${expectedFrames} frames (done=${existsSync(done)})\n${log}`,
      );
    }
    return frames;
  } finally {
    activeEmulator = undefined;
    hideTitle();
  }
}

/** Prove the HUD did not regress to rendering 480x272 and duplicating each pixel. */
function assertNativeDetail(raw: Buffer, label: string): void {
  const width = 960;
  const height = 544;
  const expected = width * height * 4;
  if (raw.length !== expected) {
    throw new Error(`${label}: expected ${expected} bytes (960x544 RGBA), got ${raw.length}`);
  }
  for (let y = 0; y < height; y += 2) {
    for (let x = 0; x < width; x += 2) {
      const a = (y * width + x) * 4;
      const b = a + 4;
      const c = a + width * 4;
      const d = c + 4;
      for (let channel = 0; channel < 4; channel++) {
        const value = raw[a + channel];
        if (raw[b + channel] !== value || raw[c + channel] !== value || raw[d + channel] !== value) {
          return;
        }
      }
    }
  }
  throw new Error(`${label}: framebuffer contains only duplicated 2x2 logical pixels`);
}

interface SceneTelemetry {
  cameraYaw: number;
  cameraPitch: number;
  effectTriangles: number;
}

function assertLiveScene(
  scenePath: string,
  label: string,
  expectEffects: boolean,
): SceneTelemetry {
  if (!existsSync(scenePath)) throw new Error(`${label}: scene sidecar missing`);
  const values = Object.fromEntries(
    readFileSync(scenePath, "utf8")
      .trim()
      .split("\n")
      .map((line) => line.split("=", 2)),
  );
  if (values.renderer !== "gxm") {
    throw new Error(`${label}: expected renderer=gxm, got ${values.renderer ?? "missing"}`);
  }
  for (const field of ["gxm_error", "geometry_error", "texture_error"]) {
    if (values[field] !== "none") {
      throw new Error(`${label}: ${field}=${values[field] ?? "missing"}`);
    }
  }
  for (const field of [
    "geometry_resident",
    "world_faces",
    "world_tris",
    "world_direct_draw_calls",
    "submitted_tris",
    "draw_calls",
  ]) {
    const value = Number(values[field]);
    if (!Number.isFinite(value) || value <= 0) {
      throw new Error(`${label}: ${field} must be positive, got ${values[field] ?? "missing"}`);
    }
  }
  if (Number(values.geometry_resident) !== 1) {
    throw new Error(`${label}: geometry_resident must be 1`);
  }
  for (const field of ["dropped_triangles", "submission_errors"]) {
    if (Number(values[field]) !== 0) {
      throw new Error(`${label}: ${field} must be zero, got ${values[field] ?? "missing"}`);
    }
  }
  const worldTriangles = Number(values.world_tris);
  const submittedTriangles = Number(values.submitted_tris);
  const drawCalls = Number(values.draw_calls);
  if (submittedTriangles < worldTriangles) {
    throw new Error(
      `${label}: submitted_tris ${submittedTriangles} is below world_tris ${worldTriangles}`,
    );
  }
  // The old CPU-projected backend emitted almost one draw per triangle. The
  // indexed GXM backend must retain a wide batching margin on this fixed map.
  if (drawCalls * 8 >= submittedTriangles) {
    throw new Error(
      `${label}: ${drawCalls} draws for ${submittedTriangles} triangles misses the GXM batching budget`,
    );
  }
  const effectTriangles = Number(values.effect_tris);
  if (!Number.isFinite(effectTriangles) || effectTriangles < 0) {
    throw new Error(`${label}: invalid effect_tris=${values.effect_tris ?? "missing"}`);
  }
  if (expectEffects && effectTriangles <= 0) {
    throw new Error(`${label}: scripted fire frame submitted no additive effect triangles`);
  }
  const cameraYaw = Number(values.camera_yaw);
  const cameraPitch = Number(values.camera_pitch);
  if (!Number.isFinite(cameraYaw) || !Number.isFinite(cameraPitch)) {
    throw new Error(`${label}: finite camera_yaw/camera_pitch telemetry is required`);
  }
  return { cameraYaw, cameraPitch, effectTriangles };
}

function assertLookProbe(
  spec: Spec,
  telemetry: ReadonlyMap<number, SceneTelemetry>,
): void {
  const probe = spec.lookProbe;
  if (!probe) return;
  const from = telemetry.get(probe.from);
  const to = telemetry.get(probe.to);
  if (!from || !to) {
    throw new Error(`${spec.name}: missing look telemetry at f${probe.from}/f${probe.to}`);
  }
  const yawDelta = to.cameraYaw - from.cameraYaw;
  const yawMoved = probe.yaw === "decrease" ? -yawDelta : yawDelta;
  if (yawMoved < probe.minYawDelta) {
    throw new Error(
      `${spec.name}: right-stick yaw moved ${yawDelta.toFixed(4)} rad; expected ${probe.yaw} by at least ${probe.minYawDelta}`,
    );
  }
  const pitchDrift = Math.abs(to.cameraPitch - from.cameraPitch);
  if (pitchDrift > probe.maxPitchDrift) {
    throw new Error(
      `${spec.name}: centered right-stick Y drifted pitch by ${pitchDrift.toFixed(6)} rad`,
    );
  }
  console.log(
    `look: yaw ${yawDelta.toFixed(4)} rad, pitch drift ${pitchDrift.toFixed(6)} rad (native right stick)`,
  );
}

let failures = 0;
for (const spec of selectedSpecs) {
  console.log(`\n## ${spec.name} (input: ${spec.input})`);
  await $`bun scripts/vita.ts --capture --release --map de_dust2`
    .cwd(repo)
    .env({
      ...process.env,
      OPENSTRIKE_VITA_CAPTURE_INPUT: spec.input,
      OPENSTRIKE_VITA_CAP_START: String(spec.capStart),
      OPENSTRIKE_VITA_CAP_N: String(spec.capN),
    });

  const vpk = `${repo}dist/vita/OpenStrike.vpk`;
  rmSync(capDir, { recursive: true, force: true });
  await prepareProfile(vpk);
  console.log("# Vita3K ...");

  let raws: string[];
  try {
    raws = await launchAndWait(spec.capN);
    console.log(`liveness: ${raws.length}/${spec.capN} frames + done marker`);
  } catch (error) {
    console.error(`FAIL ${spec.name}: ${error}`);
    failures++;
    continue;
  }

  const telemetry = new Map<number, SceneTelemetry>();
  for (const shot of spec.shots) {
    const stem = `f${String(shot).padStart(4, "0")}`;
    const rawPath = `${capDir}/${stem}.rgba`;
    const label = `${spec.name}.${stem}`;
    try {
      const raw = readFileSync(rawPath);
      assertNativeDetail(raw, label);
      telemetry.set(
        shot,
        assertLiveScene(`${capDir}/${stem}.scene`, label, spec.name === "fire"),
      );

      const png = `${outDir}/${label}.png`;
      await $`magick -size 960x544 -depth 8 RGBA:${rawPath} -alpha off -define png:exclude-chunks=date,time PNG24:${png}`.quiet();
      const dimensions = (await $`magick identify -format %wx%h ${png}`.text()).trim();
      if (dimensions !== "960x544") throw new Error(`PNG is ${dimensions}, expected 960x544`);
      const colors = Number((await $`magick ${png} -format %k info:`.text()).trim());
      if (!Number.isFinite(colors) || colors < 16) {
        throw new Error(`degenerate UI capture (${colors} colors)`);
      }

      const golden = `${goldens}/${label}.png`;
      if (update) {
        cpSync(png, golden);
        console.log(`baseline ${label} written`);
      } else if (!existsSync(golden)) {
        throw new Error(`golden missing (${golden}); run UPDATE=1 intentionally`);
      } else if (readFileSync(png).equals(readFileSync(golden))) {
        console.log(`ok ${label} (byte-exact, 960x544 native density)`);
      } else {
        throw new Error(`differs from golden (actual: ${png})`);
      }
    } catch (error) {
      console.error(`FAIL ${label}: ${error}`);
      failures++;
    }
  }
  try {
    assertLookProbe(spec, telemetry);
  } catch (error) {
    console.error(`FAIL ${spec.name} look probe: ${error}`);
    failures++;
  }
}

if (update && failures === 0) {
  const result = Bun.spawnSync([vita3k, "--version"], { stdout: "pipe", stderr: "pipe" });
  const version = `${result.stdout.toString()}${result.stderr.toString()}`.trim();
  if (version) await Bun.write(`${goldens}/VITA3K-VERSION.txt`, `${version}\n`);
}

if (failures > 0) {
  console.error(`\nVITA E2E FAILED (${failures})`);
  process.exit(1);
}
console.log("\nVITA E2E OK");
