import { describe, expect, test } from "bun:test";
import { PSP_TOOLCHAIN } from "../vendor/pocketjs/scripts/psp-toolchain.ts";

const script = await Bun.file(new URL("../scripts/psp.ts", import.meta.url)).text();
const packageJson = await Bun.file(new URL("../package.json", import.meta.url)).json();
const dependencyContract = (
  await Promise.all(
    ["../.gitmodules", "../crates/openstrike-psp/Cargo.toml", "../crates/openstrike-vita/Cargo.toml"]
      .map((path) => Bun.file(new URL(path, import.meta.url)).text()),
  )
).join("\n");

describe("PSP toolchain contract", () => {
  test("delegates bootstrap and builds to PocketJS's pinned authority", () => {
    expect(packageJson.scripts.bootstrap).toBe("bun vendor/pocketjs/scripts/bootstrap.ts");
    expect(script).toContain('from "../vendor/pocketjs/scripts/psp-toolchain.ts"');
    expect(script).toContain("resolvePspBuildToolchain()");
    expect(script).toContain("...toolchain.environment");
    expect(script).toContain("toolchain.manifest.rust.toolchain");
  });

  test("has no personal fork or sibling-checkout toolchain fallback", () => {
    expect(`${script}\n${dependencyContract}`).not.toMatch(
      /code\/dreamcart|github\.com\/doodlewind\/(?:quickjs-rs|rust-psp|pspdev)/i,
    );
  });

  test("uses the organization revisions from PocketJS's manifest", () => {
    expect(dependencyContract).toContain(PSP_TOOLCHAIN.rustPsp.repository);
    expect(dependencyContract).toContain(PSP_TOOLCHAIN.rustPsp.rev);
    expect(dependencyContract).toContain(PSP_TOOLCHAIN.quickJsRs.repository);
    expect(dependencyContract).toContain(PSP_TOOLCHAIN.quickJsRs.rev);
  });
});
