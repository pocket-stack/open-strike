// Build the real OpenStrike FPS for Nokia E7:
//   de_dust2 BSP -> Pocket3D .p3d -> canonical PocketJS JS/PAK
//   -> app-specific Rust simulation/GLES2 core -> independently installable SIS.
//
// The pinned vendor is authoritative for native builds. POCKETJS_ROOT/
// --pocketjs-root is a guest-only escape hatch because the native Cargo graph
// also resolves PocketJS from the pinned submodule.

import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  statSync,
} from "node:fs";
import { homedir } from "node:os";
import { delimiter, dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { withArtifactLock } from "../vendor/pocketjs/tools/psp-toolchain.ts";

export const SYMBIAN_TARGET = "symbian-e7-dev";
export const SYMBIAN_MAP_NAME = "de_dust2";
export const SYMBIAN_MAP_KEY = `maps/${SYMBIAN_MAP_NAME}.p3d`;
export const SYMBIAN_APP_OUTPUT = "openstrike";
export const SYMBIAN_GENERATED_ENTRY =
  ".pocket/symbian-e7-dev/app/openstrike.tsx";
const SYMBIAN_GENERATED_MAP_FILE = "../../../dist/maps/de_dust2.p3d";

interface ResolvedPlan {
  readonly app: {
    readonly entry: string;
    readonly output: string;
  };
  readonly target: {
    readonly id: string;
  };
}

interface SymbianProfileModule {
  resolveSymbianE7BuildPlan(input: unknown): ResolvedPlan;
}

interface SymbianToolchainFile {
  readonly runtime?: {
    readonly rustToolchain?: string;
    readonly frameRate?: number;
  };
}

interface PocketManifest {
  readonly version?: unknown;
  readonly engine?: unknown;
  readonly app?: unknown;
}

interface PakManifestEntry {
  readonly key?: unknown;
  readonly file?: unknown;
}

interface PakBlob {
  readonly key: string;
  readonly data: Uint8Array;
}

interface PakCompilerModule {
  unpack(file: Uint8Array): readonly PakBlob[];
}

export interface SymbianCliOptions {
  readonly guestOnly: boolean;
  readonly help: boolean;
  readonly pocketjsRoot?: string;
  readonly sisVersion?: string;
  readonly uid?: string;
}

export interface SymbianBuildPaths {
  readonly repo: string;
  readonly pocketjs: string;
  readonly symbianTool: string;
  readonly symbianProfile: string;
  readonly pocketBuild: string;
  readonly toolchainManifest: string;
  readonly targetSpec: string;
  readonly pocket3dWorkspace: string;
  readonly nativeManifest: string;
  readonly nativeLock: string;
  readonly generatedManifest: string;
  readonly generatedEntry: string;
  readonly generatedPakManifest: string;
  readonly mapSource: string;
  readonly mapSupport: string;
  readonly cookedMap: string;
  readonly plan: string;
  readonly guestOutput: string;
  readonly nativeTarget: string;
  readonly nativeLibrary: string;
  readonly packageOutput: string;
}

const repo = resolve(fileURLToPath(new URL("..", import.meta.url)));

export async function withSymbianBuildLock<T>(
  repository: string,
  operation: () => Promise<T>,
): Promise<T> {
  return await withArtifactLock(
    resolve(repository, ".pocket/openstrike-symbian-build.lock"),
    operation,
    { timeoutMs: 20 * 60_000, staleMs: 30 * 60_000 },
  );
}

function record(value: unknown, label: string): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${label} must be an object`);
  }
  return value as Record<string, unknown>;
}

function optionValue(
  args: readonly string[],
  index: number,
  name: string,
): readonly [string, number] | undefined {
  const argument = args[index]!;
  if (argument.startsWith(`${name}=`)) {
    const value = argument.slice(name.length + 1);
    if (!value) throw new Error(`${name} requires a value`);
    return [value, index];
  }
  if (argument !== name) return undefined;
  const value = args[index + 1];
  if (!value || value.startsWith("-")) {
    throw new Error(`${name} requires a value`);
  }
  return [value, index + 1];
}

export function parseSymbianCli(
  args: readonly string[],
): SymbianCliOptions {
  let guestOnly = false;
  let help = false;
  let pocketjsRoot: string | undefined;
  let sisVersion: string | undefined;
  let uid: string | undefined;
  for (let index = 0; index < args.length; index++) {
    const argument = args[index]!;
    if (argument === "--guest-only") {
      guestOnly = true;
      continue;
    }
    if (argument === "--help" || argument === "-h") {
      help = true;
      continue;
    }
    const pocketjs = optionValue(args, index, "--pocketjs-root");
    if (pocketjs) {
      if (pocketjsRoot !== undefined) {
        throw new Error("--pocketjs-root may only be provided once");
      }
      [pocketjsRoot, index] = pocketjs;
      continue;
    }
    const version = optionValue(args, index, "--sis-version");
    if (version) {
      if (sisVersion !== undefined) {
        throw new Error("--sis-version may only be provided once");
      }
      [sisVersion, index] = version;
      continue;
    }
    const packageUid = optionValue(args, index, "--uid");
    if (packageUid) {
      if (uid !== undefined) throw new Error("--uid may only be provided once");
      [uid, index] = packageUid;
      continue;
    }
    throw new Error(`unknown Symbian build argument: ${argument}`);
  }
  return { guestOnly, help, pocketjsRoot, sisVersion, uid };
}

function expandHome(path: string, home: string): string {
  if (path === "~") return home;
  if (path.startsWith("~/")) return resolve(home, path.slice(2));
  return path;
}

export function resolvePocketJsRoot(
  repository: string,
  options: Pick<SymbianCliOptions, "pocketjsRoot">,
  environment: Readonly<Record<string, string | undefined>> = process.env,
): string {
  const home = environment.HOME || homedir();
  const selected =
    options.pocketjsRoot ||
    environment.POCKETJS_ROOT ||
    resolve(repository, "vendor/pocketjs");
  return resolve(repository, expandHome(selected, home));
}

export function validateNativePocketJsSelection(
  repository: string,
  pocketjs: string,
  guestOnly: boolean,
): void {
  const pinned = resolve(repository, "vendor/pocketjs");
  if (!guestOnly && resolve(pocketjs) !== pinned) {
    throw new Error(
      "full Symbian builds must use pinned vendor/pocketjs so the Cargo core and Qt host cannot drift",
    );
  }
}

export function createSymbianBuildPaths(
  repository: string,
  pocketjs: string,
  mapsRoot: string,
): SymbianBuildPaths {
  const nativeTarget = resolve(repository, "dist/symbian-core");
  return {
    repo: resolve(repository),
    pocketjs: resolve(pocketjs),
    symbianTool: resolve(pocketjs, "tools/symbian.ts"),
    symbianProfile: resolve(pocketjs, "tools/symbian-profile.ts"),
    pocketBuild: resolve(pocketjs, "tools/build.ts"),
    toolchainManifest: resolve(
      pocketjs,
      "tools/cli/symbian-toolchain.json",
    ),
    targetSpec: resolve(
      pocketjs,
      "engine/symbian/targets/armv6-symbian-eabi.json",
    ),
    pocket3dWorkspace: resolve(pocketjs, "engine/pocket3d"),
    nativeManifest: resolve(
      repository,
      "crates/openstrike-symbian/Cargo.toml",
    ),
    nativeLock: resolve(repository, "crates/openstrike-symbian/Cargo.lock"),
    generatedManifest: resolve(
      repository,
      ".pocket/symbian-e7-dev/pocket.json",
    ),
    generatedEntry: resolve(repository, SYMBIAN_GENERATED_ENTRY),
    generatedPakManifest: resolve(
      repository,
      ".pocket/symbian-e7-dev/app/pak.json",
    ),
    mapSource: resolve(mapsRoot, "maps/de_dust2.bsp"),
    mapSupport: resolve(mapsRoot, "support"),
    cookedMap: resolve(repository, "dist/maps/de_dust2.p3d"),
    plan: resolve(repository, ".pocket/symbian-e7-dev/plan.json"),
    guestOutput: resolve(repository, "dist/pocket/symbian-e7-dev"),
    nativeTarget,
    nativeLibrary: resolve(
      nativeTarget,
      "armv6-symbian-eabi/release/libopenstrike_symbian.a",
    ),
    packageOutput: resolve(repository, "dist/symbian"),
  };
}

export function validateSymbianSourceContract(
  manifestInput: unknown,
): { readonly version: string } {
  const manifest = record(manifestInput, "pocket.json");
  const app = record(manifest.app, "pocket.json app");
  const viewport = record(app.viewport, "pocket.json app.viewport");
  const dynamic = record(
    viewport.dynamic,
    "pocket.json app.viewport.dynamic",
  );
  const engine = record(manifest.engine, "pocket.json engine");
  const capabilities = record(
    engine.capabilities,
    "pocket.json engine.capabilities",
  );
  const enhances = capabilities.enhances;
  if (
    app.entry !== "game/openstrike.tsx" ||
    app.output !== SYMBIAN_APP_OUTPUT
  ) {
    throw new Error(
      "Symbian must build the canonical OpenStrike game entry and output",
    );
  }
  if (
    !Array.isArray(enhances) ||
    !enhances.includes("display.viewport.live")
  ) {
    throw new Error("OpenStrike must enhance display.viewport.live");
  }
  for (const name of ["default", "min", "max"] as const) {
    const actual = dynamic[name];
    if (
      !Array.isArray(actual) ||
      actual.length !== 2 ||
      actual.some((extent) =>
        !Number.isInteger(extent) || extent <= 0
      )
    ) {
      throw new Error(
        `OpenStrike dynamic viewport ${name} must be a positive extent`,
      );
    }
  }
  if (
    typeof manifest.version !== "string" ||
    !/^[0-9]+\.[0-9]+\.[0-9]+$/.test(manifest.version)
  ) {
    throw new Error("pocket.json version must be a three-part SIS version");
  }
  return { version: manifest.version };
}

export function createSymbianBuildManifest(
  manifestInput: unknown,
): Record<string, unknown> {
  const manifest = structuredClone(record(manifestInput, "pocket.json"));
  const app = record(manifest.app, "pocket.json app");
  manifest.app = { ...app, entry: SYMBIAN_GENERATED_ENTRY };
  return manifest;
}

export function symbianGeneratedPakManifest(): readonly PakManifestEntry[] {
  return [{
    key: SYMBIAN_MAP_KEY,
    file: SYMBIAN_GENERATED_MAP_FILE,
  }];
}

async function materializeSymbianBuildProject(
  paths: SymbianBuildPaths,
  manifestInput: unknown,
): Promise<Record<string, unknown>> {
  const manifest = createSymbianBuildManifest(manifestInput);
  mkdirSync(dirname(paths.generatedEntry), { recursive: true });
  await Bun.write(
    paths.generatedEntry,
    'import "../../../game/openstrike.tsx";\n',
  );
  await Bun.write(
    paths.generatedPakManifest,
    JSON.stringify(symbianGeneratedPakManifest(), null, 2) + "\n",
  );
  await Bun.write(
    paths.generatedManifest,
    JSON.stringify(manifest, null, 2) + "\n",
  );
  return manifest;
}

function sha256(bytes: Uint8Array): string {
  return createHash("sha256").update(bytes).digest("hex");
}

export function validateGuestArtifacts(
  js: Uint8Array,
  entries: readonly PakBlob[],
  cookedMap: Uint8Array,
): void {
  if (
    js.byteLength < 1024 ||
    !new TextDecoder().decode(js).toLowerCase().includes("strike")
  ) {
    throw new Error("Symbian guest JavaScript is empty or not OpenStrike");
  }
  const mapEntries = entries.filter((entry) =>
    entry.key.startsWith("maps/")
  );
  if (
    mapEntries.length !== 1 ||
    mapEntries[0]!.key !== SYMBIAN_MAP_KEY
  ) {
    throw new Error(
      "Symbian guest PAK must contain only maps/de_dust2.p3d",
    );
  }
  if (
    mapEntries[0]!.data.byteLength !== cookedMap.byteLength ||
    sha256(mapEntries[0]!.data) !== sha256(cookedMap)
  ) {
    throw new Error("Symbian guest PAK map differs from cooked de_dust2");
  }
}

async function unpackGuestPak(
  pocketjsRoot: string,
  pak: Uint8Array,
): Promise<readonly PakBlob[]> {
  const compiler = resolve(
    pocketjsRoot,
    "framework/compiler/pak.ts",
  );
  if (!existsSync(compiler)) {
    throw new Error(`PocketJS PAK authority is missing at ${compiler}`);
  }
  const url = pathToFileURL(compiler);
  url.searchParams.set("mtime", String(statSync(compiler).mtimeMs));
  const module = await import(url.href) as PakCompilerModule;
  return module.unpack(pak);
}

export function symbianCookCommand(
  paths: SymbianBuildPaths,
): readonly string[] {
  return [
    "cargo",
    "run",
    "--release",
    "-q",
    "-p",
    "pocket3d-cook",
    "--",
    paths.mapSource,
    "--wads",
    paths.mapSupport,
    "--subdivide",
    "32",
    "-o",
    paths.cookedMap,
    "--verify",
  ];
}

export function symbianVerifyCookedCommand(
  paths: SymbianBuildPaths,
): readonly string[] {
  return [
    "cargo",
    "run",
    "--release",
    "-q",
    "-p",
    "pocket3d-cook",
    "--",
    "--verify-cooked",
    paths.cookedMap,
  ];
}

export function symbianNativeCommand(
  paths: SymbianBuildPaths,
  rustToolchain: string,
  rustup = "rustup",
): readonly string[] {
  return [
    rustup,
    "run",
    rustToolchain,
    "cargo",
    "build",
    "--manifest-path",
    paths.nativeManifest,
    "--release",
    "--locked",
    "--target",
    paths.targetSpec,
    "-Z",
    "json-target-spec",
    "-Z",
    "build-std=core,alloc,compiler_builtins",
    "-Z",
    "build-std-features=compiler-builtins-mem",
  ];
}

export function symbianPackageCommand(
  paths: SymbianBuildPaths,
  bun: string,
  sisVersion: string,
  uid?: string,
): readonly string[] {
  const command = [
    bun,
    paths.symbianTool,
    "build",
    "app",
    "--manifest",
    paths.generatedManifest,
    "--project-root",
    paths.repo,
    "--outdir",
    paths.packageOutput,
    "--sis-version",
    sisVersion,
    "--core-library",
    paths.nativeLibrary,
  ];
  if (uid) command.push("--uid", uid);
  return command;
}

function validatePocketJsRoot(
  paths: SymbianBuildPaths,
  requireNativePackaging: boolean,
): void {
  const required = [
    paths.symbianTool,
    paths.symbianProfile,
    paths.pocketBuild,
    paths.toolchainManifest,
    paths.targetSpec,
    resolve(paths.pocket3dWorkspace, "../Cargo.toml"),
  ];
  const missing = required.filter((path) => !existsSync(path));
  if (missing.length > 0) {
    throw new Error(
      `PocketJS Symbian toolchain is incomplete at ${paths.pocketjs}: ` +
        missing.map((path) => path.slice(paths.pocketjs.length + 1)).join(", "),
    );
  }
  if (
    requireNativePackaging &&
    !readFileSync(paths.symbianTool, "utf8").includes("coreLibrary:")
  ) {
    throw new Error(
      `PocketJS at ${paths.pocketjs} predates custom Symbian cores; ` +
        "advance the pinned vendor/pocketjs revision",
    );
  }
}

function newestMapInput(paths: SymbianBuildPaths): number {
  if (!existsSync(paths.mapSource)) {
    throw new Error(
      `missing ${paths.mapSource}; set OPENSTRIKE_MAPS to your GoldSrc map data`,
    );
  }
  if (!existsSync(paths.mapSupport) || !statSync(paths.mapSupport).isDirectory()) {
    throw new Error(`missing WAD support directory ${paths.mapSupport}`);
  }
  const wads = readdirSync(paths.mapSupport)
    .filter((name) => name.toLowerCase().endsWith(".wad"))
    .map((name) => resolve(paths.mapSupport, name));
  if (wads.length === 0) {
    throw new Error(`no WAD files found under ${paths.mapSupport}`);
  }
  return Math.max(
    statSync(paths.mapSource).mtimeMs,
    ...wads.map((path) => statSync(path).mtimeMs),
  );
}

function mapNeedsCooking(paths: SymbianBuildPaths): boolean {
  if (!existsSync(paths.cookedMap)) {
    newestMapInput(paths);
    return true;
  }
  // A valid ignored .p3d is already a complete build input. Requiring the
  // copyrighted BSP again would make a copied build tree unusable; when the
  // source is present, however, its mtime and the WAD mtimes still detect a
  // stale cook.
  if (!existsSync(paths.mapSource)) return false;
  const newestInput = newestMapInput(paths);
  return statSync(paths.cookedMap).mtimeMs < newestInput;
}

interface RunOptions {
  readonly cwd: string;
  readonly env?: Record<string, string | undefined>;
}

async function run(
  command: readonly string[],
  options: RunOptions,
): Promise<void> {
  const child = Bun.spawn({
    cmd: [...command],
    cwd: options.cwd,
    env: options.env,
    stdout: "inherit",
    stderr: "inherit",
  });
  const exitCode = await child.exited;
  if (exitCode !== 0) {
    throw new Error(
      `${command.slice(0, 4).join(" ")} failed with exit code ${exitCode}`,
    );
  }
}

async function resolveBuildPlan(
  paths: SymbianBuildPaths,
  manifest: unknown,
): Promise<ResolvedPlan> {
  const profileUrl = pathToFileURL(paths.symbianProfile);
  profileUrl.searchParams.set(
    "mtime",
    String(statSync(paths.symbianProfile).mtimeMs),
  );
  const profile = await import(profileUrl.href) as SymbianProfileModule;
  const plan = profile.resolveSymbianE7BuildPlan(manifest);
  if (
    plan.target.id !== SYMBIAN_TARGET ||
    plan.app.output !== SYMBIAN_APP_OUTPUT ||
    plan.app.entry !== SYMBIAN_GENERATED_ENTRY
  ) {
    throw new Error("PocketJS resolved the wrong Symbian OpenStrike plan");
  }
  mkdirSync(dirname(paths.plan), { recursive: true });
  await Bun.write(paths.plan, JSON.stringify(plan, null, 2) + "\n");
  return plan;
}

async function ensureDust2(paths: SymbianBuildPaths): Promise<void> {
  if (mapNeedsCooking(paths)) {
    mkdirSync(dirname(paths.cookedMap), { recursive: true });
    console.log("openstrike-symbian: cooking de_dust2");
    await run(symbianCookCommand(paths), {
      cwd: paths.pocket3dWorkspace,
      env: process.env,
    });
  }
  // Always ask the canonical Rust reader to validate reused as well as newly
  // cooked artifacts. Header-only checks can accept truncated section tables.
  await run(symbianVerifyCookedCommand(paths), {
    cwd: paths.pocket3dWorkspace,
    env: process.env,
  });
}

async function buildGuest(
  paths: SymbianBuildPaths,
  bun: string,
): Promise<void> {
  mkdirSync(paths.guestOutput, { recursive: true });
  await run([
    bun,
    paths.pocketBuild,
    `--plan=${paths.plan}`,
    `--project-root=${paths.repo}`,
    `--outdir=${paths.guestOutput}`,
  ], {
    cwd: paths.repo,
    env: process.env,
  });
  const js = new Uint8Array(
    readFileSync(resolve(paths.guestOutput, `${SYMBIAN_APP_OUTPUT}.js`)),
  );
  const pak = new Uint8Array(
    readFileSync(resolve(paths.guestOutput, `${SYMBIAN_APP_OUTPUT}.pak`)),
  );
  validateGuestArtifacts(
    js,
    await unpackGuestPak(paths.pocketjs, pak),
    new Uint8Array(readFileSync(paths.cookedMap)),
  );
}

export function validateSymbianRuntimePin(
  input: SymbianToolchainFile,
): string {
  const toolchain = input.runtime?.rustToolchain;
  if (!toolchain || !/^nightly-[0-9]{4}-[0-9]{2}-[0-9]{2}$/.test(toolchain)) {
    throw new Error("PocketJS Symbian Rust nightly pin is missing");
  }
  if (input.runtime?.frameRate !== 30) {
    throw new Error(
      "OpenStrike requires the pinned Symbian host frameRate to remain 30 Hz",
    );
  }
  return toolchain;
}

function readRustToolchain(paths: SymbianBuildPaths): string {
  return validateSymbianRuntimePin(
    JSON.parse(
      readFileSync(paths.toolchainManifest, "utf8"),
    ) as SymbianToolchainFile,
  );
}

const HELP = `OpenStrike Nokia E7 3D build

  bun scripts/symbian.ts --guest-only
      cook only de_dust2, compile the real JS/PAK, and validate the embedded map

  bun scripts/symbian.ts [--sis-version 0.2.0] [--uid 0xE.......]
      additionally build the pinned Rust GLES2 core and package openstrike.sis

  OPENSTRIKE_MAPS defaults to ~/Downloads/cs-maps-20260705-1836.
  POCKETJS_ROOT or --pocketjs-root selects an explicit checkout only with
  --guest-only. Native builds always use pinned vendor/pocketjs.
`;

async function runSymbianMain(
  args: readonly string[] = Bun.argv.slice(2),
): Promise<void> {
  const options = parseSymbianCli(args);
  if (options.help) {
    console.log(HELP);
    return;
  }
  const pocketjs = resolvePocketJsRoot(repo, options);
  validateNativePocketJsSelection(repo, pocketjs, options.guestOnly);
  const mapsRoot = resolve(
    expandHome(
      process.env.OPENSTRIKE_MAPS ||
        resolve(homedir(), "Downloads/cs-maps-20260705-1836"),
      process.env.HOME || homedir(),
    ),
  );
  const paths = createSymbianBuildPaths(repo, pocketjs, mapsRoot);
  validatePocketJsRoot(paths, !options.guestOnly);

  const manifest = JSON.parse(
    readFileSync(resolve(repo, "pocket.json"), "utf8"),
  ) as PocketManifest;
  const contract = validateSymbianSourceContract(manifest);
  await ensureDust2(paths);
  const buildManifest = await materializeSymbianBuildProject(paths, manifest);
  await resolveBuildPlan(paths, buildManifest);

  const bun = Bun.which("bun") || process.execPath;
  if (options.guestOnly) {
    await buildGuest(paths, bun);
    console.log(
      `OpenStrike E7 real guest: ${paths.guestOutput}/${SYMBIAN_APP_OUTPUT}.{js,pak}`,
    );
    return;
  }

  if (!existsSync(paths.nativeManifest) || !existsSync(paths.nativeLock)) {
    throw new Error(
      "crates/openstrike-symbian must contain Cargo.toml and Cargo.lock",
    );
  }
  const rustup = Bun.which("rustup");
  if (!rustup) {
    throw new Error(
      "rustup is required; run the PocketJS Symbian setup before building",
    );
  }
  const rustToolchain = readRustToolchain(paths);
  mkdirSync(paths.nativeTarget, { recursive: true });
  const rustEnvironment = {
    ...process.env,
    CARGO_TARGET_DIR: paths.nativeTarget,
    PATH: [dirname(rustup), process.env.PATH].filter(Boolean).join(delimiter),
  };
  console.log(
    `openstrike-symbian: building native core with ${rustToolchain}`,
  );
  await run(symbianNativeCommand(paths, rustToolchain, rustup), {
    cwd: paths.repo,
    env: rustEnvironment,
  });
  if (
    !existsSync(paths.nativeLibrary) ||
    statSync(paths.nativeLibrary).size === 0
  ) {
    throw new Error(`native build did not produce ${paths.nativeLibrary}`);
  }

  const sisVersion = options.sisVersion || contract.version;
  await run(
    symbianPackageCommand(
      paths,
      bun,
      sisVersion,
      options.uid,
    ),
    { cwd: paths.repo, env: rustEnvironment },
  );

  const payload = resolve(
    paths.packageOutput,
    "build",
    SYMBIAN_APP_OUTPUT,
  );
  const packagedJs = new Uint8Array(
    readFileSync(resolve(payload, "app.js")),
  );
  const packagedPak = new Uint8Array(
    readFileSync(resolve(payload, "app.pak")),
  );
  validateGuestArtifacts(
    packagedJs,
    await unpackGuestPak(paths.pocketjs, packagedPak),
    new Uint8Array(readFileSync(paths.cookedMap)),
  );
  const sis = resolve(paths.packageOutput, `${SYMBIAN_APP_OUTPUT}.sis`);
  if (!existsSync(sis) || statSync(sis).size === 0) {
    throw new Error(`Symbian build did not produce ${sis}`);
  }
  console.log(`OpenStrike E7 full 3D SIS: ${sis}`);
}

export async function symbianMain(
  args: readonly string[] = Bun.argv.slice(2),
): Promise<void> {
  await withSymbianBuildLock(repo, () => runSymbianMain(args));
}

if (import.meta.main) {
  await symbianMain();
}
