// Exercise the real Mac guest/worker and the PSP USB record protocol against
// one authority. Requires matching locally supplied BSP/P3D, never game data in Git.
import { mkdirSync, readFileSync, writeFileSync, renameSync } from "node:fs";
import { resolve } from "node:path";
import { randomBytes } from "node:crypto";
import { connectOffloadUsbProvider, usbPacket, usbHash } from "../vendor/pocketjs/tools/offload-usb-provider.ts";
const [bspPath, cookedPath, mode] = Bun.argv.slice(2);
if (!bspPath || !cookedPath) throw Error("Usage: bun scripts/check-crossplay.ts map.bsp map.p3d");
const root = resolve(import.meta.dir, ".."), out = resolve(root, "out/validation/crossplay-" + Date.now());
mkdirSync(out, { recursive: true });
const bytes = readFileSync(cookedPath);
let hash = 0xcbf29ce484222325n;
for (const tag of ["WCLP", "WVIS", "WENT"]) {
  let part: Buffer | undefined;
  for (let i = 0; i < bytes.readUInt32LE(8); i++) {
    const at = 16 + i * 16;
    if (bytes.toString("ascii", at, at + 4) === tag) part = bytes.subarray(bytes.readUInt32LE(at + 4), bytes.readUInt32LE(at + 4) + bytes.readUInt32LE(at + 8));
  }
  if (!part) throw Error("Missing map section");
  for (const b of Buffer.concat([Buffer.from(tag), part])) hash = BigInt.asUintN(64, (hash ^ BigInt(b)) * 0x100000001b3n);
}
const map = hash.toString(16).padStart(16, "0"), key = randomBytes(32).toString("hex");
const server = Bun.spawn([resolve(root, "target/debug/openstrike-companion"), resolve(cookedPath), "0"], { env: { ...process.env, OPENSTRIKE_COMPANION_KEY: key }, stdout: "pipe", stderr: "pipe" });
const reader = server.stdout.getReader(), first = await reader.read(); reader.releaseLock();
const ready = new TextDecoder().decode(first.value), address = ready.match(/Companion ready: (127\.0\.0\.1:\d+)/)?.[1];
if (!address) { server.kill(); throw Error("Server did not start: " + ready); }
const config = resolve(out, "desktop.json");writeFileSync(config, JSON.stringify({ address, key, slot: 0 }), { mode: 0o600 });
const provider = connectOffloadUsbProvider({ directory: out, app: "dev.pocket-stack.openstrike", worker: new URL("./crossplay-worker.ts", import.meta.url), data: { address, key, slot: 1 } });
const desktop = Bun.spawn([resolve(root, "target/debug/openstrike"), "--map", resolve(bspPath), "--script", mode === "--combat" ? "crossplay-combat" : "crossplay", "--screenshot", resolve(out,"mac-crossplay.png"), "--size", "960x544"], {
  cwd: root, env: { ...process.env, OPENSTRIKE_COMPANION_CONFIG: config }, stdout: "pipe", stderr: "pipe",
});
let epoch = 0, generation = 0, serial = 0, boot = 914, next = 1, round = 0, inFlight = false, requests = 0, live = 0, maxPayload = 0;
let history: number[][] = [], lastTick = 0, lastResponse = Date.now(), firstPosition: number[] | undefined, moved = false;
const start = Date.now();
try {
  while (Date.now() - start < 14000 && desktop.exitCode === null) {
    const tick = Math.floor((Date.now()-start) * 64 / 1000);
    while (lastTick < tick) {
      lastTick++;
      if (epoch) history.push([next++, lastTick < 160 ? 0.35 : 0, 0, Math.PI, 0, 0]);
      if (history.length > 128) throw Error("Input history overflow");
    }
    let readyBytes: Buffer | undefined;
    try { readyBytes = readFileSync(resolve(provider.root, "ready")); } catch {}
    if (readyBytes && readyBytes.length === 64) generation = readyBytes.readUInt32LE(4);
    if (generation && !inFlight) {
      const payload = JSON.stringify({ v: 1, map, epoch, inputs: epoch ? history.slice(0,12) : [] });
      maxPayload = Math.max(maxPayload, payload.length);
      const id = ++serial, packet = usbPacket(generation, boot, serial, id, Buffer.from(JSON.stringify({ v: 1, id, method:"strike.exchange", payload })));
      const path = resolve(provider.root, "req0"); writeFileSync(path+".tmp",packet);renameSync(path+".tmp",path);inFlight=true;requests++;
    }
    if (inFlight) {
      try {
        const packet = readFileSync(resolve(provider.root, "res0"));
        if (packet.readUInt32LE(4)===generation && packet.readUInt32LE(8)===boot && packet.readUInt32LE(12)===serial) {
          const payload = packet.subarray(64);
          if (payload.length!==packet.readUInt32LE(32) || usbHash(payload)!==packet.readUInt32LE(36)) throw Error("USB checksum mismatch");
          const envelope = JSON.parse(payload.toString());
          if (envelope.error) throw Error(envelope.error);
          const reply = JSON.parse(envelope.payload);epoch=reply.epoch;history=history.filter(i=>i[0]>reply.ack);
          if (reply.round!==round) {round=reply.round;history=[];next=reply.ack+1;}
          if (reply.playing) live++;
          const pos=reply.players[1].pos as number[];
          if (!firstPosition) firstPosition=pos;
          if (Math.hypot(...pos.map((n,i)=>n-firstPosition![i]))>4) moved=true;
          lastResponse=Date.now();inFlight=false;
        }
      } catch (error) { if ((error as NodeJS.ErrnoException).code!=="ENOENT") throw error; }
    }
    if (Date.now()-lastResponse>2000) throw Error("Companion reply timeout");
    await Bun.sleep(5);
  }
  if (desktop.exitCode===null) {desktop.kill();throw Error("Mac guest timed out");}
  const stdout=await new Response(desktop.stdout).text(),stderr=await new Response(desktop.stderr).text();
  writeFileSync(resolve(out,"mac.log"),stdout+stderr);
  if (await desktop.exited || !stdout.includes("CROSSPLAY_GUEST_OK") || live<30 || !moved) throw Error("Crossplay acceptance failed: "+stdout+stderr);
  const result={requests,liveReplies:live,remoteMoved:moved,maxInputPayload:maxPayload,mac:stdout.trim(),device:"USB protocol peer, not physical PSP"};
  writeFileSync(resolve(out,"result.json"),JSON.stringify(result,null,2));console.log(JSON.stringify({out,...result}));
} finally { provider.close();desktop.kill();server.kill(); }
