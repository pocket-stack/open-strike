import { describe, expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  SYMBIAN_GENERATED_ENTRY,
  SYMBIAN_MAP_KEY,
  createSymbianBuildManifest,
  createSymbianBuildPaths,
  parseSymbianCli,
  resolvePocketJsRoot,
  symbianCookCommand,
  symbianGeneratedPakManifest,
  symbianNativeCommand,
  symbianPackageCommand,
  symbianVerifyCookedCommand,
  validateGuestArtifacts,
  validateNativePocketJsSelection,
  validateSymbianRuntimePin,
  validateSymbianSourceContract,
  withSymbianBuildLock,
} from "../scripts/symbian.ts";
import { PAK_DTYPE } from "../vendor/pocketjs/contracts/spec/spec.ts";
import {
  pack,
  unpack,
} from "../vendor/pocketjs/framework/compiler/pak.ts";

const manifest = await Bun.file(new URL("../pocket.json", import.meta.url)).json();
const packageJson = await Bun.file(new URL("../package.json", import.meta.url)).json();
const script = await Bun.file(new URL("../scripts/symbian.ts", import.meta.url)).text();
const nativeCore = await Bun.file(
  new URL("../crates/openstrike-symbian/src/lib.rs", import.meta.url),
).text();

function cookedMap(): Uint8Array {
  const bytes = new Uint8Array(32);
  bytes.set(new TextEncoder().encode("P3D1"), 0);
  const view = new DataView(bytes.buffer);
  view.setUint32(4, 1, true);
  view.setUint32(8, 1, true);
  return bytes;
}

describe("OpenStrike Symbian source contract", () => {
  test("uses the canonical full game with one embedded Dust2 map", () => {
    expect(validateSymbianSourceContract(manifest)).toEqual({
      version: "0.2.0",
    });
    expect(manifest.app.entry).toBe("game/openstrike.tsx");
    expect(manifest.app.viewport.dynamic).toEqual({
      default: [640, 360],
      min: [360, 360],
      max: [640, 640],
    });
    const generated = createSymbianBuildManifest(manifest);
    expect((generated.app as { entry: string }).entry).toBe(
      SYMBIAN_GENERATED_ENTRY,
    );
    expect(symbianGeneratedPakManifest()).toEqual([
      {
        key: SYMBIAN_MAP_KEY,
        file: "../../../dist/maps/de_dust2.p3d",
      },
    ]);
    expect(nativeCore).toContain(
      `const MAP_KEY: &str = "${SYMBIAN_MAP_KEY}";`,
    );
    expect(script).not.toContain('resolve(repo, "game/pak.json")');
  });

  test("has no compatibility-game build path", () => {
    expect(packageJson.scripts["test:symbian"]).toBe(
      "bun test test/symbian-tooling.test.ts",
    );
    expect(packageJson.scripts["test:symbian-compat"]).toBeUndefined();
    expect(script.toLowerCase()).not.toContain("compat");
  });
});

describe("OpenStrike Symbian tooling", () => {
  const paths = createSymbianBuildPaths(
    "/repo",
    "/pocketjs",
    "/maps",
  );

  test("prefers explicit PocketJS, then POCKETJS_ROOT, then the pin", () => {
    expect(
      resolvePocketJsRoot(
        "/repo",
        { pocketjsRoot: "/explicit" },
        { HOME: "/home", POCKETJS_ROOT: "/environment" },
      ),
    ).toBe("/explicit");
    expect(
      resolvePocketJsRoot(
        "/repo",
        {},
        { HOME: "/home", POCKETJS_ROOT: "/environment" },
      ),
    ).toBe("/environment");
    expect(
      resolvePocketJsRoot("/repo", {}, { HOME: "/home" }),
    ).toBe("/repo/vendor/pocketjs");
    expect(() =>
      validateNativePocketJsSelection("/repo", "/newer-pocketjs", false)
    ).toThrow("full Symbian builds must use pinned vendor/pocketjs");
    expect(() =>
      validateNativePocketJsSelection("/repo", "/newer-pocketjs", true)
    ).not.toThrow();
    expect(() =>
      validateNativePocketJsSelection(
        "/repo",
        "/repo/vendor/pocketjs",
        false,
      )
    ).not.toThrow();
  });

  test("parses only the documented build switches", () => {
    expect(
      parseSymbianCli([
        "--guest-only",
        "--pocketjs-root=/pocketjs",
        "--sis-version",
        "0.2.1",
        "--uid=0xE0000001",
      ]),
    ).toEqual({
      guestOnly: true,
      help: false,
      pocketjsRoot: "/pocketjs",
      sisVersion: "0.2.1",
      uid: "0xE0000001",
    });
    expect(() => parseSymbianCli(["--compat"])).toThrow(
      "unknown Symbian build argument",
    );
  });

  test("serializes generated plans, guests, native cores, and packaging", async () => {
    const root = mkdtempSync(join(tmpdir(), "openstrike-symbian-lock-"));
    try {
      let active = 0;
      let maximum = 0;
      const build = () => withSymbianBuildLock(root, async () => {
        active += 1;
        maximum = Math.max(maximum, active);
        await Bun.sleep(20);
        active -= 1;
      });
      await Promise.all([build(), build()]);
      expect(maximum).toBe(1);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  test("cooks only de_dust2 with the verified 32-unit recipe", () => {
    expect(symbianCookCommand(paths)).toEqual([
      "cargo",
      "run",
      "--release",
      "-q",
      "-p",
      "pocket3d-cook",
      "--",
      "/maps/maps/de_dust2.bsp",
      "--wads",
      "/maps/support",
      "--subdivide",
      "32",
      "-o",
      "/repo/dist/maps/de_dust2.p3d",
      "--verify",
    ]);
    expect(symbianVerifyCookedCommand(paths)).toEqual([
      "cargo",
      "run",
      "--release",
      "-q",
      "-p",
      "pocket3d-cook",
      "--",
      "--verify-cooked",
      "/repo/dist/maps/de_dust2.p3d",
    ]);
  });

  test("pins nightly build-std and passes the exact custom core to packaging", () => {
    expect(validateSymbianRuntimePin({
      runtime: {
        rustToolchain: "nightly-2026-07-02",
        frameRate: 30,
      },
    })).toBe("nightly-2026-07-02");
    expect(() => validateSymbianRuntimePin({
      runtime: {
        rustToolchain: "nightly-2026-07-02",
        frameRate: 20,
      },
    })).toThrow("frameRate to remain 30 Hz");
    const native = symbianNativeCommand(
      paths,
      "nightly-2026-07-02",
      "/rustup",
    );
    expect(native.slice(0, 4)).toEqual([
      "/rustup",
      "run",
      "nightly-2026-07-02",
      "cargo",
    ]);
    expect(native).toContain("--locked");
    expect(native).toContain(
      "build-std=core,alloc,compiler_builtins",
    );
    expect(native).toContain(
      "/pocketjs/engine/symbian/targets/armv6-symbian-eabi.json",
    );

    const packaged = symbianPackageCommand(
      paths,
      "/bun",
      "0.2.0",
      "0xE0000001",
    );
    expect(packaged.slice(0, 4)).toEqual([
      "/bun",
      "/pocketjs/tools/symbian.ts",
      "build",
      "app",
    ]);
    expect(packaged[packaged.indexOf("--manifest") + 1]).toBe(
      "/repo/.pocket/symbian-e7-dev/pocket.json",
    );
    expect(packaged[packaged.indexOf("--core-library") + 1]).toBe(
      paths.nativeLibrary,
    );
    expect(packaged.at(-1)).toBe("0xE0000001");
  });
});

describe("OpenStrike Symbian guest validation", () => {
  test("accepts the real bundle only when its sole map is byte-exact Dust2", () => {
    const map = cookedMap();
    const pak = pack([
      { key: "ui:styles", dtype: PAK_DTYPE.u8, data: new Uint8Array([1]) },
      { key: SYMBIAN_MAP_KEY, dtype: PAK_DTYPE.u8, data: map },
    ]);
    const js = new TextEncoder().encode(
      `const strike = "openstrike";${" ".repeat(1100)}`,
    );
    expect(() => validateGuestArtifacts(js, unpack(pak), map)).not.toThrow();
    expect(unpack(pak).map((entry) => entry.key)).toEqual([
      SYMBIAN_MAP_KEY,
      "ui:styles",
    ]);
  });

  test("rejects a renamed or substituted map", () => {
    const map = cookedMap();
    const pak = pack([
      {
        key: "maps/de_inferno.p3d",
        dtype: PAK_DTYPE.u8,
        data: map,
      },
    ]);
    const js = new TextEncoder().encode(
      `const strike = "openstrike";${" ".repeat(1100)}`,
    );
    expect(() => validateGuestArtifacts(js, unpack(pak), map)).toThrow(
      "must contain only maps/de_dust2.p3d",
    );
  });
});
