// Execute a compiled symbian-e7-dev guest against a strict in-process model
// of the PocketJS HostOps/frame contract, then drive a real menu selection and
// combat input. This catches startup/module-order failures (especially a
// missing `strike` surface) without claiming to replace hardware acceptance.
//
// Usage:
//   bun scripts/symbian-compat-smoke.ts \
//     --js=/path/to/openstrike.js --pak=/path/to/openstrike.pak

import { resolve } from "node:path";

interface StrikeState {
  phase: "menu" | "starting" | "live" | "won" | "lost";
  hp: number;
  ammo: number;
  aliveBots: number;
  totalBots: number;
  wins: number;
  losses: number;
}

interface StrikeEvent {
  type: string;
}

interface StrikeSurface {
  __dispatch?: (state: StrikeState, events: StrikeEvent[]) => void;
}

interface SmokeGlobals {
  ui?: Record<string, unknown>;
  __pak?: ArrayBuffer;
  frame?: (buttons: number, analog?: number, touches?: readonly number[]) => void;
  strike?: StrikeSurface;
}

const flag = (name: string): string | undefined =>
  process.argv
    .slice(2)
    .find((argument) => argument.startsWith(`${name}=`))
    ?.slice(name.length + 1);

const jsPath = flag("--js");
const pakPath = flag("--pak");
if (!jsPath || !pakPath) {
  throw new Error(
    "usage: bun scripts/symbian-compat-smoke.ts --js=<openstrike.js> --pak=<openstrike.pak>",
  );
}

const absoluteJs = resolve(jsPath);
const absolutePak = resolve(pakPath);
if (!(await Bun.file(absoluteJs).exists()) || !(await Bun.file(absolutePak).exists())) {
  throw new Error(`missing compatibility smoke input: ${absoluteJs} or ${absolutePak}`);
}

let nextNode = 2;
let nextTexture = 1;
let nextAnimation = 1;
let operationCount = 0;
let focusedNode = 0;
const op = () => {
  operationCount++;
};

const globals = globalThis as typeof globalThis & SmokeGlobals;
globals.ui = {
  __host: "symbian-e7-dev",
  __hostAbi: 4,
  __viewport: { w: 640, h: 360 },
  createNode: () => {
    op();
    return nextNode++;
  },
  destroyNode: op,
  insertBefore: op,
  removeChild: op,
  setStyle: op,
  setProp: op,
  setPropBatch: op,
  setText: op,
  replaceText: op,
  uploadTexture: () => {
    op();
    return nextTexture++;
  },
  uploadImgEntry: () => {
    op();
    return nextTexture++;
  },
  setImage: op,
  setSprite: op,
  animate: () => {
    op();
    return nextAnimation++;
  },
  cancelAnim: op,
  setFocus: (node: number) => {
    focusedNode = node;
    op();
  },
  setActive: op,
  loadStyles: op,
  loadFontAtlas: op,
  measureText: (text: string) => text.length * 7,
};
globals.__pak = await Bun.file(absolutePak).arrayBuffer();

// The output is a trusted local PocketJS IIFE generated from this checkout.
const source = await Bun.file(absoluteJs).text();
(0, eval)(source);
// The native host drains pending QuickJS jobs after JS_Eval before starting
// its frame timer. Mirror that boundary for Solid's mount effects.
await Promise.resolve();
await Promise.resolve();

if (typeof globals.frame !== "function") {
  throw new Error("compatibility bundle did not install the PocketJS frame hook");
}
const surface = globals.strike;
if (!surface?.__dispatch) {
  throw new Error("compatibility bundle did not install the strike dispatch surface");
}

let latest: StrikeState | undefined;
const eventTypes: string[] = [];
const dispatch = surface.__dispatch;
surface.__dispatch = (state, events) => {
  latest = { ...state };
  eventTypes.push(...events.map((event) => event.type));
  dispatch(state, events);
};

const BTN_CIRCLE = 0x2000;
const BTN_RTRIGGER = 0x0200;
const BTN_LEFT = 0x0080;
const BTN_RIGHT = 0x0020;
const frame = (buttons = 0) => globals.frame!(buttons, 0x8080, []);
const phase = (): StrikeState["phase"] | undefined => latest?.phase;
const snapshot = (): StrikeState | undefined => (latest ? { ...latest } : undefined);

// Let onMount establish the menu's first focused map, then activate it using
// the exact Circle/Enter edge the E7 native host publishes.
frame();
if (phase() !== "menu") {
  throw new Error(`expected menu boot, received ${phase() ?? "no state"}`);
}
frame(BTN_CIRCLE);
frame();
if (phase() !== "starting") {
  throw new Error(
    `map selection did not enter a round: ${phase() ?? "no state"} (focused node ${focusedNode})`,
  );
}

for (let index = 0; index < 60 && phase() !== "live"; index++) frame();
if (phase() !== "live") {
  throw new Error(`rules did not advance starting -> live: ${phase() ?? "no state"}`);
}
if (!latest) throw new Error("live phase was reached without a state snapshot");

const botsBefore = latest.aliveBots;
frame(BTN_RTRIGGER);
frame();
if (!eventTypes.includes("hit") || !latest || latest.aliveBots >= botsBefore) {
  throw new Error(
    `combat input did not produce a hit (bots ${botsBefore} -> ${latest?.aliveBots ?? "?"})`,
  );
}

const fireBurst = (frames: number) => {
  for (let index = 0; index < frames; index++) frame(BTN_RTRIGGER);
  frame();
};

// Clear the two flanking center-lane bots using only the E7 button protocol.
// Integer frame turns intentionally allow body hits, exercising automatic
// fire rather than relying on the initial exact headshot.
for (let index = 0; index < 2; index++) frame(BTN_LEFT);
fireBurst(12);
const leftLaneState = snapshot();
if (!leftLaneState || leftLaneState.aliveBots !== 1) {
  throw new Error(
    `left-lane engagement did not leave one bot: ${leftLaneState?.aliveBots ?? "?"}`,
  );
}
for (let index = 0; index < 4; index++) frame(BTN_RIGHT);
fireBurst(12);
for (let index = 0; index < 4 && phase() !== "won"; index++) frame();
const wonState = snapshot();
if (!wonState || phase() !== "won" || wonState.aliveBots !== 0 || wonState.wins !== 1) {
  throw new Error(
    `elimination did not reach rules-owned win state: ${phase() ?? "?"}, ` +
      `bots=${wonState?.aliveBots ?? "?"}, wins=${wonState?.wins ?? "?"}`,
  );
}

for (let index = 0; index < 130 && phase() !== "starting"; index++) frame();
const resetState = snapshot();
if (
  !resetState ||
  phase() !== "starting" ||
  resetState.aliveBots !== resetState.totalBots ||
  !eventTypes.includes("roundReset")
) {
  throw new Error(
    `rules-owned next round did not reset: ${phase() ?? "?"}, ` +
      `bots=${resetState?.aliveBots ?? "?"}/${resetState?.totalBots ?? "?"}`,
  );
}
if (operationCount === 0) {
  throw new Error("compatibility renderer emitted no HostOps");
}

console.log(
  JSON.stringify(
    {
      result: "ok",
      mode: "E7 2D compatibility (not the Pocket3D renderer)",
      phase: latest.phase,
      bots: `${latest.aliveBots}/${latest.totalBots}`,
      hp: latest.hp,
      ammo: latest.ammo,
      events: eventTypes,
      hostOperations: operationCount,
    },
    null,
    2,
  ),
);
