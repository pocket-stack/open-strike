// Render every shipped officer action and 1/3/6-actor loads through the PSP
// binary. These are PPSSPP correctness captures, not hardware performance proof.
import { $ } from "bun";
import { existsSync, mkdirSync, readdirSync, rmSync, copyFileSync } from "node:fs";
import { resolve } from "node:path";

const repo = resolve(import.meta.dir, "..");
const emulator = process.env.PPSSPP_HEADLESS ?? `${process.env.HOME}/ppsspp-src/build/PPSSPPHeadless`;
if (!existsSync(emulator)) throw new Error(`PPSSPPHeadless missing: ${emulator}`);
const out = resolve(repo, "out/character-psp");
const cap = `${process.env.HOME}/.ppsspp/dc_cap`;
const target = resolve(repo, "crates/openstrike-psp/target/mipsel-sony-psp/debug");
mkdirSync(out, { recursive: true });
const shots = [
  ["Idle", 30], ["Walk", 210], ["Run", 390], ["Fire", 550],
  ["Reload", 775], ["Hit", 910], ["Death", 1140], ["Death-held", 1170],
  ["three", 2730], ["six", 3990],
] as const;
const receipt: unknown[] = [];
const assetPath = resolve(repo, "assets/characters/police/receipt.json");
const asset = await Bun.file(assetPath).json();
async function assertAssetUnchanged() {
  const bytes = await Bun.file(resolve(repo, "assets/characters/police/officer.opch")).arrayBuffer();
  const hash = new Bun.CryptoHasher("sha256").update(bytes).digest("hex");
  if (hash !== asset.opch_sha256) throw new Error("officer asset changed during capture; regenerate and rerun");
}
await assertAssetUnchanged();
for (const [name, start] of shots) {
  await assertAssetUnchanged();
  console.log(`officer PSP: ${name}`);
  await $`bun scripts/psp.ts --capture --character-bench`.cwd(repo).env({
    ...process.env, OPENSTRIKE_COOKED_MAPS: process.env.OPENSTRIKE_COOKED_MAPS ?? resolve(repo, "dist/maps"),
    OPENSTRIKE_PSP_CAP_START: String(start), OPENSTRIKE_PSP_CAP_N: "2",
  }).quiet();
  mkdirSync(cap, { recursive: true });
  for (const file of readdirSync(cap).filter(f => /^f\d+\.raw$/.test(f))) rmSync(`${cap}/${file}`);
  // An existing mailbox would change frame costs and can pause capture.
  rmSync(`${target}/pocketjs-dbg`, { recursive: true, force: true });
  await $`${emulator} --graphics=software --timeout=180 ${target}/EBOOT.PBP`.quiet();
  const raws = readdirSync(cap).filter(f => /^f\d+\.raw$/.test(f));
  if (raws.length !== 2) throw new Error(`${name}: expected two fresh frames, got ${raws.length}`);
  await $`magick -size 512x272 -depth 8 RGBA:${cap}/f0000.raw -alpha off -crop 480x272+0+0 +repage ${out}/${name}.png`.quiet();
  const colors = Number((await $`magick ${out}/${name}.png -format %k info:`.text()).trim());
  if (colors < 64) throw new Error(`${name}: degenerate image (${colors} colors)`);
  if (existsSync(`${target}/OpenStrike-bench.jsonl`)) copyFileSync(`${target}/OpenStrike-bench.jsonl`, `${out}/${name}.emulator.jsonl`);
  receipt.push({ name, frame: start, captured: raws.length, colors });
}
await assertAssetUnchanged();
await Bun.write(`${out}/receipt.json`, JSON.stringify({
  kind: "PPSSPP software rendering; not hardware timing", revision: (await $`git rev-parse HEAD`.text()).trim(),
  asset, shots: receipt,
}, null, 2) + "\n");
console.log(`officer PSP: ten capture scenarios passed; ${out}/receipt.json`);
