import { afterEach, expect, test } from "bun:test";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, utimesSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { mapCookKey, mapCacheMatches, recordMapCook } from "../scripts/cook-map.ts";

const roots: string[] = [];
afterEach(() => { for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true }); });

test("map cache tracks WAD content, additions, removal, and cooker changes despite preserved timestamps", () => {
  const root = mkdtempSync(join(tmpdir(), "openstrike-map-")); roots.push(root);
  const source = join(root, "maps", "fixture.bsp"), wad = join(root, "support", "base.wad");
  const cooker = join(root, "engine", "pocket3d");
  for (const dir of ["maps", "support", "engine/pocket3d/crates/pocket3d-bsp/src", "engine/pocket3d/crates/pocket3d-cook/src"])
    mkdirSync(join(root, dir), { recursive: true });
  writeFileSync(source, "bsp"); writeFileSync(wad, "before");
  writeFileSync(join(root, "engine/Cargo.lock"), "lock");
  for (const crate of ["pocket3d-bsp", "pocket3d-cook"]) {
    writeFileSync(join(cooker, "crates", crate, "Cargo.toml"), "manifest");
    writeFileSync(join(cooker, "crates", crate, "src/lib.rs"), "source");
  }
  const key = () => mapCookKey(source, [join(root, "support")], cooker);
  const first = key();
  writeFileSync(wad, "after!"); utimesSync(wad, 0, 0);
  expect(key()).not.toBe(first);
  const changed = key();
  const extra = join(root, "support/assault.WAD"); writeFileSync(extra, "new");
  expect(key()).not.toBe(changed);
  rmSync(extra); expect(key()).toBe(changed);
  writeFileSync(join(cooker, "crates/pocket3d-bsp/src/lib.rs"), "fixed cooker");
  expect(key()).not.toBe(changed);
  const cooked = join(root, "fixture.p3d"); writeFileSync(cooked, "cooked map");
  expect(mapCacheMatches(cooked, key())).toBe(false);
  recordMapCook(cooked, key()); expect(mapCacheMatches(cooked, key())).toBe(true);
  writeFileSync(cooked, "stale replacement"); expect(mapCacheMatches(cooked, key())).toBe(false);
});
