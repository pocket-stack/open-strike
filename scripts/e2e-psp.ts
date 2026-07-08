// PSP e2e: deterministic PPSSPPHeadless runs of openstrike-psp against
// byte-exact goldens (the vendor/pocketjs test/e2e-ppsspp.ts recipe, adapted
// for the 3D runtime and its extended frame:mask:lx:ly input scripts).
//
//   bun scripts/e2e-psp.ts            # compare against test/goldens-psp
//   UPDATE=1 bun scripts/e2e-psp.ts   # re-baseline
//
// Requires PPSSPPHeadless (PPSSPP_HEADLESS env or ~/ppsspp-src/build) and
// the CS maps (OPENSTRIKE_MAPS). Software renderer only — it is the only
// deterministic backend; goldens are only promised for the PPSSPP commit in
// test/goldens-psp/PPSSPP-COMMIT.txt.

import { $ } from "bun";
import { existsSync, mkdirSync, readdirSync, rmSync } from "node:fs";

const repo = new URL("..", import.meta.url).pathname;
const home = process.env.HOME ?? "";
const goldens = `${repo}test/goldens-psp`;
const update = process.env.UPDATE === "1";

// PSP button bits.
const R = 0x200; // fire
const SQUARE = 0x8000; // look left
const CIRCLE = 0x2000; // look right

interface Spec {
  name: string;
  // frame:mask:lx:ly entries (lx/ly optional, default 128).
  input: string;
  capStart: number;
  capN: number;
  // Frame indices (relative to capStart) to keep as goldens.
  shots: number[];
}

// Round flow: rules.ts freeze 1.2s (72 ticks) after eval, then live.
const SPECS: Spec[] = [
  {
    // Idle at CT spawn: world + viewmodel + HUD, nothing moving.
    name: "spawn",
    input: "0:0",
    capStart: 96,
    capN: 4,
    shots: [0],
  },
  {
    // Walk forward out of spawn, then sweep the view right.
    name: "walk",
    input: "0:0,80:0:128:20,150:0x2000:128:20,190:0:128:96",
    capStart: 150,
    capN: 44,
    shots: [0, 40],
  },
  {
    // Hold fire: muzzle flash + tracer + ammo drain on the HUD.
    name: "fire",
    input: `0:0,90:${R}`,
    capStart: 92,
    capN: 12,
    shots: [1, 8],
  },
];

const ppsspp = process.env.PPSSPP_HEADLESS ?? `${home}/ppsspp-src/build/PPSSPPHeadless`;
if (!existsSync(ppsspp)) {
  console.error(`PPSSPPHeadless not found at ${ppsspp}`);
  process.exit(1);
}

const capDir = `${home}/.ppsspp/dc_cap`;
const outDir = `${repo}out/e2e-psp`;
mkdirSync(outDir, { recursive: true });
mkdirSync(goldens, { recursive: true });

let failures = 0;
for (const spec of SPECS) {
  console.log(`\n## ${spec.name} (input: ${spec.input})`);
  console.log("# build capture EBOOT ...");
  await $`bun scripts/psp.ts --capture`
    .cwd(repo)
    .env({
      ...process.env,
      OPENSTRIKE_PSP_CAPTURE_INPUT: spec.input,
      OPENSTRIKE_PSP_CAP_START: String(spec.capStart),
      OPENSTRIKE_PSP_CAP_N: String(spec.capN),
    })
    .quiet();

  rmSync(capDir, { recursive: true, force: true });
  const eboot = `${repo}crates/openstrike-psp/target/mipsel-sony-psp/debug/EBOOT.PBP`;
  rmSync(`${repo}crates/openstrike-psp/target/mipsel-sony-psp/debug/pocketjs-dbg`, {
    recursive: true,
    force: true,
  });
  console.log("# PPSSPPHeadless (software renderer) ...");
  await $`${ppsspp} --graphics=software --timeout=180 ${eboot}`.nothrow().quiet();

  const raws = existsSync(capDir)
    ? readdirSync(capDir).filter((f) => f.endsWith(".raw")).sort()
    : [];
  if (raws.length !== spec.capN) {
    console.error(`FAIL ${spec.name}: ${raws.length}/${spec.capN} frames dumped`);
    failures++;
    continue;
  }
  console.log(`liveness: ${raws.length}/${spec.capN} frames dumped`);

  for (const shot of spec.shots) {
    const raw = `${capDir}/f${String(shot).padStart(4, "0")}.raw`;
    const png = `${outDir}/${spec.name}.f${shot}.png`;
    await $`magick -size 512x272 -depth 8 RGBA:${raw} -alpha off -crop 480x272+0+0 +repage -define png:exclude-chunks=date,time PNG24:${png}`.quiet();

    // Degenerate-frame guard: a real frame has plenty of distinct colors.
    const ident = await $`magick ${png} -format %k info:`.text();
    if (parseInt(ident.trim(), 10) < 16) {
      console.error(`FAIL ${spec.name}.f${shot}: degenerate frame (${ident.trim()} colors)`);
      failures++;
      continue;
    }

    const golden = `${goldens}/${spec.name}.f${shot}.png`;
    if (update || !existsSync(golden)) {
      await Bun.write(golden, Bun.file(png));
      console.log(`baseline ${spec.name}.f${shot} written`);
    } else {
      const a = Buffer.from(await Bun.file(png).arrayBuffer());
      const b = Buffer.from(await Bun.file(golden).arrayBuffer());
      if (a.equals(b)) {
        console.log(`ok ${spec.name}.f${shot} (byte-exact)`);
      } else {
        console.error(`FAIL ${spec.name}.f${shot}: differs from golden (see ${png})`);
        failures++;
      }
    }
  }
}

if (update) {
  const commit = await $`git -C ${home}/ppsspp-src rev-parse HEAD`.nothrow().text();
  if (commit.trim()) {
    await Bun.write(`${goldens}/PPSSPP-COMMIT.txt`, commit);
  }
}

if (failures > 0) {
  console.error(`\nE2E FAILED (${failures})`);
  process.exit(1);
}
console.log("\nE2E OK");
