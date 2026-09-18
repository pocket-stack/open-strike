// Run the authority and the PocketJS PSP provider on the computer serving host0.
import { randomBytes } from "node:crypto";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { connectOffloadUsbProvider } from "../vendor/pocketjs/tools/offload-usb-provider.ts";
const args = Bun.argv.slice(2);
const option = (name: string) => { const i = args.indexOf(name); return i < 0 ? undefined : args[i + 1]; };
const map = option("--map"), usb = option("--usb-root"), port = Number(option("--port") ?? "8748");
if (!map || !usb || !Number.isInteger(port) || port < 1024 || port > 65535) throw Error("Usage: bun scripts/crossplay.ts --map map.p3d --usb-root HOST0_DIR [--port 8748] [--desktop map.bsp]");
const root = resolve(import.meta.dir, "..");
const build = Bun.spawn(["cargo", "build", "--locked", "-p", "openstrike-companion"], { cwd: root, stdout: "inherit", stderr: "inherit" });
if (await build.exited) throw Error("Companion build failed");
const key = randomBytes(32).toString("hex"), address = `127.0.0.1:${port}`;
const config = resolve(root, "out/crossplay/desktop.json");
mkdirSync(dirname(config), { recursive: true });
writeFileSync(config, JSON.stringify({ address, key, slot: 0 }), { mode: 0o600 });
const server = Bun.spawn([resolve(root, "target/debug/openstrike-companion"), resolve(map), String(port)], {
  cwd: root, env: { ...process.env, OPENSTRIKE_COMPANION_KEY: key }, stdout: "pipe", stderr: "inherit",
});
// Wait for the server's bind/map-validation receipt before advertising USB.
const reader = server.stdout.getReader();
const first = await reader.read();
if (first.done || !new TextDecoder().decode(first.value).includes("Companion ready:")) { server.kill(); throw Error("Companion failed to start"); }
console.log(new TextDecoder().decode(first.value).trim()); reader.releaseLock();
const provider = connectOffloadUsbProvider({ directory: resolve(usb), app: "dev.pocket-stack.openstrike",
  worker: new URL("./crossplay-worker.ts", import.meta.url), data: { address, key, slot: 1 }, log: console.log });
console.log(`PSP: choose CROSSPLAY and the same map.\nMac: OPENSTRIKE_COMPANION_CONFIG=${config} cargo run --locked -p openstrike -- --map MAP.bsp`);
const desktop = option("--desktop");
const game = desktop ? Bun.spawn(["cargo", "run", "--locked", "-p", "openstrike", "--", "--map", resolve(desktop)], {
  cwd: root, env: { ...process.env, OPENSTRIKE_COMPANION_CONFIG: config }, stdout: "inherit", stderr: "inherit",
}) : undefined;
const close = () => { provider.close(); game?.kill(); server.kill(); };
process.on("SIGINT", close); process.on("SIGTERM", close);
await server.exited; provider.close(); game?.kill();
