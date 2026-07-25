// A source checkout intentionally omits PocketJS's ignored generated style
// mirror. TypeScript follows the Solid runtime's relative import before any
// app build has regenerated it, so seed the smallest valid table for
// setup/typecheck. Every real PocketJS compile overwrites this file with the
// app-specific table embedded in the pak.

import { existsSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(new URL("..", import.meta.url).pathname);
const framework = resolve(root, "vendor/pocketjs/framework/src/index.ts");
const generated = resolve(
  root,
  "vendor/pocketjs/framework/src/styles.generated.ts",
);

if (!existsSync(framework)) {
  throw new Error(
    "PocketJS framework sources are missing; initialize vendor/pocketjs first",
  );
}
if (!existsSync(generated)) {
  await Bun.write(
    generated,
    [
      "// Generated placeholder; PocketJS tools/build.ts replaces this file.",
      "export const STYLE_IDS: Readonly<Record<string, number>> = {};",
      "",
    ].join("\n"),
  );
}
