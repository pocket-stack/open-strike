import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

function hashFile(path: string): string {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

/** Includes WAD additions/removals/content and cooker code, even when a copy
 * preserves old mtimes. The search directories match pocket3d-bsp::cook_map. */
export function mapCookKey(source: string, wadDirs: readonly string[], cooker: string): string {
  const parent = dirname(resolve(source));
  const dirs = [...wadDirs.map((p) => resolve(p)), parent,
    join(parent, "..", "support"), join(parent, "..", "wads"), dirname(parent)];
  const files = new Set([resolve(source)]);
  for (const dir of dirs) {
    if (!existsSync(dir)) continue;
    for (const name of readdirSync(dir).sort()) {
      if (name.toLowerCase().endsWith(".wad")) files.add(resolve(dir, name));
    }
  }
  for (const crate of ["pocket3d-bsp", "pocket3d-cook"]) {
    const root = join(cooker, "crates", crate);
    files.add(join(root, "Cargo.toml"));
    const walk = (dir: string) => {
      for (const entry of readdirSync(dir, { withFileTypes: true })) {
        if (entry.isDirectory()) walk(join(dir, entry.name));
        else if (entry.name.endsWith(".rs")) files.add(join(dir, entry.name));
      }
    };
    walk(join(root, "src"));
  }
  // Dependency resolution can change cooking without changing the source.
  files.add(resolve(cooker, "../Cargo.lock"));
  const inputs = [...files].sort().map((path) => [path, hashFile(path)]);
  return createHash("sha256").update(JSON.stringify({ version: 1, subdivide: 32, inputs })).digest("hex");
}

export function mapCacheMatches(cooked: string, key: string): boolean {
  try {
    const receipt = JSON.parse(readFileSync(cooked + ".cook.json", "utf8"));
    return receipt.key === key && receipt.output === hashFile(cooked);
  } catch { return false; }
}

export function recordMapCook(cooked: string, key: string): void {
  const temporary = cooked + `.cook.json.${process.pid}.tmp`;
  writeFileSync(temporary, JSON.stringify({ key, output: hashFile(cooked) }) + "\n");
  renameSync(temporary, cooked + ".cook.json");
}

export async function cookMap(source: string, cooked: string, wadDirs: readonly string[], cooker: string): Promise<void> {
  const key = mapCookKey(source, wadDirs, cooker);
  if (mapCacheMatches(cooked, key)) return;
  mkdirSync(dirname(cooked), { recursive: true });
  const temporary = cooked + `.${process.pid}.tmp`;
  try {
    const child = Bun.spawn(["cargo", "run", "--release", "--locked", "-q", "-p", "pocket3d-cook", "--",
      resolve(source), ...wadDirs.flatMap((dir) => ["--wads", resolve(dir)]),
      "--subdivide", "32", "--verify", "-o", resolve(temporary)],
      { cwd: cooker, stdout: "inherit", stderr: "inherit" });
    if (await child.exited !== 0) throw new Error(`Map cooking failed: ${source}`);
    if (mapCookKey(source, wadDirs, cooker) !== key) throw new Error(`Map inputs changed while cooking: ${source}`);
    renameSync(temporary, cooked);
    recordMapCook(cooked, key);
  } finally { rmSync(temporary, { force: true }); }
}
