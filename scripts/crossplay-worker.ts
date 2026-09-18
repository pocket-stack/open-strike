import { connect, type Socket } from "node:net";
import type { OffloadRequest } from "../vendor/pocketjs/contracts/spec/offload.ts";
interface Config { address: string; key: string; slot: number }
let config: Config, socket: Socket | undefined, buffer = "", busy = false;
let pending: { id: number; resolve(reply: unknown): void; reject(error: Error): void } | undefined;
const stop = () => { pending = undefined; socket?.destroy(); socket = undefined; buffer = ""; };
function attach(): Promise<void> {
  if (socket && !socket.destroyed) return Promise.resolve();
  return new Promise((resolve, reject) => {
    const [host, port] = config.address.split(":");
    if (host !== "127.0.0.1" || !/^\d+$/.test(port) || !/^[a-f0-9]{64}$/.test(config.key) || config.slot !== 1) return reject(Error("Invalid local pairing"));
    const connection = connect({ host, port: Number(port) }); socket = connection;
    connection.setNoDelay(true);
    connection.setTimeout(1000, () => connection.destroy(Error("Companion timeout")));
    connection.on("connect", () => { connection.write(JSON.stringify({ key: config.key, slot: config.slot }) + "\n"); resolve(); });
    connection.on("error", error => { reject(error); if (socket === connection) { pending?.reject(error); pending = undefined; } });
    connection.on("close", () => { if (socket === connection) { socket = undefined; buffer = ""; pending?.reject(Error("Companion closed")); pending = undefined; } });
    connection.on("data", bytes => {
      buffer += bytes.toString("utf8");
      if (Buffer.byteLength(buffer) > 4097) { connection.destroy(Error("Reply budget exceeded")); return; }
      const at = buffer.indexOf("\n");
      if (at < 0) return;
      try {
        const reply = JSON.parse(buffer.slice(0, at)); buffer = buffer.slice(at + 1);
        if (!pending || reply.id !== pending.id || buffer.length) throw Error("Unexpected reply");
        pending.resolve(reply); pending = undefined;
      } catch { connection.destroy(Error("Invalid reply")); }
    });
  });
}
self.onmessage = async ({ data }: MessageEvent<OffloadRequest | { init: Config }>) => {
  if ("init" in data) { config = data.init; self.postMessage({ ready: true }); return; }
  const request = data as OffloadRequest;
  if (busy || request.method !== "strike.exchange" || request.payload.length > 2500) {
    self.postMessage({ id: request.id, error: "Crossplay busy or invalid request" }); return;
  }
  busy = true;
  try {
    await attach();
    const reply = await new Promise((resolve, reject) => {
      pending = { id: request.id, resolve, reject };
      socket!.write(JSON.stringify(request) + "\n");
    });
    self.postMessage(reply);
  } catch { stop(); self.postMessage({ id: request.id, error: "Companion disconnected" }); }
  finally { busy = false; }
};
