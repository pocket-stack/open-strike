// Build OpenStrike's independently installable Nokia E7 compatibility SIS
// through the canonical PocketJS Symbian toolchain.
//
// The pinned vendor is authoritative. POCKETJS_ROOT/--pocketjs-root remains
// available only for deliberately testing a newer PocketJS checkout.
//
// `--guest-only` stops before Rust/qmake/SIS packaging and runs the HostOps
// gameplay smoke against dist/pocket/symbian-e7-dev.

import { existsSync, mkdirSync } from "node:fs";
import { delimiter, dirname, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import {
  deriveSymbianCompatManifest,
  SYMBIAN_COMPAT_OUTPUT,
} from "./symbian-manifest.ts";

interface ResolvedPlan {
  readonly app: { readonly output: string };
}

interface SymbianProfileModule {
  resolveSymbianE7BuildPlan(input: unknown): ResolvedPlan;
}

const repo = resolve(new URL("..", import.meta.url).pathname);
const args = process.argv.slice(2);

function option(name: string): string | undefined {
  const equals = args.find((argument) => argument.startsWith(`${name}=`));
  if (equals) return equals.slice(name.length + 1);
  const index = args.indexOf(name);
  if (index >= 0) return args[index + 1];
  return undefined;
}

const vendorRoot = resolve(repo, "vendor/pocketjs");
const pocketjsRoot = resolve(
  option("--pocketjs-root") ??
    process.env.POCKETJS_ROOT ??
    vendorRoot,
);
const symbianTool = resolve(pocketjsRoot, "tools/symbian.ts");
const symbianProfile = resolve(pocketjsRoot, "tools/symbian-profile.ts");
const pocketBuild = resolve(pocketjsRoot, "tools/build.ts");
if (
  !existsSync(symbianTool) ||
  !existsSync(symbianProfile) ||
  !existsSync(pocketBuild)
) {
  throw new Error(
    `PocketJS Symbian toolchain is unavailable at ${pocketjsRoot}; ` +
      "set POCKETJS_ROOT or --pocketjs-root to a current PocketJS checkout",
  );
}

const canonicalPath = resolve(repo, "pocket.json");
const canonical = await Bun.file(canonicalPath).json();
const manifest = deriveSymbianCompatManifest(canonical);
const generatedRoot = resolve(repo, ".pocket/symbian-e7-dev");
const manifestPath = resolve(generatedRoot, "pocket.generated.json");
mkdirSync(dirname(manifestPath), { recursive: true });
await Bun.write(manifestPath, JSON.stringify(manifest, null, 2) + "\n");

const profileUrl = pathToFileURL(symbianProfile);
profileUrl.searchParams.set("mtime", String(Date.now()));
const profile = await import(profileUrl.href) as SymbianProfileModule;
const plan = profile.resolveSymbianE7BuildPlan(manifest);
if (plan.app.output !== SYMBIAN_COMPAT_OUTPUT) {
  throw new Error(
    `derived Symbian output mismatch: ${plan.app.output}`,
  );
}

const bun = Bun.which("bun");
if (!bun) throw new Error("Bun is required to build OpenStrike for Symbian");
const rustup = Bun.which("rustup");
const subprocessEnv = rustup
  ? {
      ...process.env,
      // PocketJS invokes its pinned toolchain through rustup, but Cargo then
      // resolves `rustc` through PATH. Keep the rustup proxy ahead of a
      // Homebrew stable compiler so the upstream nightly selection survives.
      PATH: [dirname(rustup), process.env.PATH].filter(Boolean).join(delimiter),
    }
  : process.env;

async function run(command: readonly string[]): Promise<void> {
  const child = Bun.spawn({
    cmd: [...command],
    cwd: repo,
    env: subprocessEnv,
    stdout: "inherit",
    stderr: "inherit",
  });
  const exitCode = await child.exited;
  if (exitCode !== 0) {
    throw new Error(`${command.slice(0, 3).join(" ")} failed with exit code ${exitCode}`);
  }
}

async function smoke(js: string, pak: string): Promise<void> {
  await run([
    bun!,
    resolve(repo, "scripts/symbian-compat-smoke.ts"),
    `--js=${js}`,
    `--pak=${pak}`,
  ]);
}

if (args.includes("--guest-only")) {
  const planPath = resolve(generatedRoot, "plan.json");
  const outdir = resolve(repo, "dist/pocket/symbian-e7-dev");
  await Bun.write(planPath, JSON.stringify(plan, null, 2) + "\n");
  await run([
    bun,
    pocketBuild,
    `--plan=${planPath}`,
    `--project-root=${repo}`,
    `--outdir=${outdir}`,
  ]);
  await smoke(
    resolve(outdir, `${SYMBIAN_COMPAT_OUTPUT}.js`),
    resolve(outdir, `${SYMBIAN_COMPAT_OUTPUT}.pak`),
  );
  console.log(`OpenStrike E7 compatibility guest: ${outdir}`);
} else {
  const outputRoot = resolve(repo, "dist/symbian");
  const command = [
    bun,
    symbianTool,
    "build",
    "app",
    "--manifest",
    manifestPath,
    "--project-root",
    repo,
    "--outdir",
    outputRoot,
    "--sis-version",
    option("--sis-version") ?? String(manifest.version),
  ];
  const uid = option("--uid");
  if (uid) command.push("--uid", uid);
  await run(command);

  const payload = resolve(outputRoot, "build", SYMBIAN_COMPAT_OUTPUT);
  await smoke(resolve(payload, "app.js"), resolve(payload, "app.pak"));
  const sis = resolve(outputRoot, `${SYMBIAN_COMPAT_OUTPUT}.sis`);
  if (!existsSync(sis)) {
    throw new Error(`Symbian build did not produce ${sis}`);
  }
  console.log(`OpenStrike E7 compatibility SIS: ${sis}`);
}
