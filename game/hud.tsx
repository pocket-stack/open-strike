// The OpenStrike HUD: Solid owns the fixed tree and quit dialog; combat
// values use fixed text cells and a precompiled paint batch. Scores and
// round transitions must follow the same path as ammo/health: they can
// coincide with hits, effects and character animation on the PSP.
// Native tweens advance damage/feed fades without per-frame JS calls.
// Class strings remain full literals for the Tailwind subset compiler.

import { createSignal, onCleanup, onMount, Show } from "solid-js";
import { Text, View } from "@pocketjs/framework/components";
import * as hot from "@pocketjs/framework/hot";
import { animate, jump, createJumpBatch, type JumpBatch } from "@pocketjs/framework/animation";
import { pushFocusScope } from "@pocketjs/framework/input";
import { onButtonPress, onFrame } from "@pocketjs/framework/lifecycle";
import { platform } from "@pocketjs/framework/platform";
import { strike } from "./sdk.ts";
import { ROUND_FREEZE, ROUND_END_PAUSE, phaseAge } from "./rules.ts";

// Palette (military night-ops): lime reticle, amber warnings, blood red.
const INK = "#e8f0f2";
const DIM = "#8fa3ad";
const LIME = "#b8f34a";
const AMBER = "#fbbf24";
const RED = "#f43f3f";
const STRIP = "#04070a90";

// Packed ABGR for hot color swaps (hot.prop is numeric-only).
const abgr = (hex: string): number => {
  const n = parseInt(hex.slice(1), 16);
  return (((0xff << 24) | ((n & 0xff) << 16) | (n & 0xff00) | ((n >> 16) & 0xff)) >>> 0);
};
const INK_N = abgr(INK);
const AMBER_N = abgr(AMBER);
const RED_N = abgr(RED);
const LIME_N = abgr(LIME);

// The same bundle renders at 480x272 on the PSP and at the window size on
// desktop; scale the chrome so it occupies the same screen fraction.
const vp = (globalThis as { ui?: { __viewport?: { w: number; h: number } } }).ui
  ?.__viewport ?? { w: 480, h: 272 };
const S = Math.max(1, Math.floor(Math.min(vp.w / 480, vp.h / 272)));
const IS_SYMBIAN_E7 = platform.target === "symbian-e7-dev";

const FEED_ROWS = 3;
const BAR_W = 90;
const AMMO_BAR_W = 64;

type Ref = NonNullable<Parameters<typeof hot.text>[0]>;

export default function Hud() {
  const s0 = strike.state();
  // SELECT opens/closes the quit dialog (BTN.SELECT = 0x0001). The mount is
  // structural but user-initiated — never on the combat hot path.
  const [dialog, setDialog] = createSignal(false);
  onButtonPress(0x0001, () => setDialog((d) => !d));

  // One owner per value: static JSX initial values, then the hot path.
  let banner: Ref;
  let bannerTitle: Ref;
  let bannerSub: Ref;
  let bannerTop: Ref;
  let bannerBottom: Ref;
  let winsText: Ref;
  let lossesText: Ref;
  let botsText: Ref;
  let reserveText: Ref;
  let hpText: Ref;
  let hpFill: Ref;
  let ammoText: Ref;
  let ammoFill: Ref;
  let reloadGroup: Ref;
  let reloadFill: Ref;
  let flashOverlay: Ref;
  let vignette: Ref;
  let crosshair: Ref;
  let hitmarker: Ref;
  const feedRows: Ref[] = [];
  const feedTexts: Ref[] = [];

  let feedDirty = false;
  let flashDirty = false;
  let resetEffects = false;
  let hitDuration = 0;
  const feedUntil: number[] = Array.from({ length: FEED_ROWS }, () => 0);
  const feedStr: string[] = Array.from({ length: FEED_ROWS }, () => " ");
  const pushFeed = (text: string) => {
    feedDirty = true;
    for (let i = 0; i < FEED_ROWS - 1; i++) {
      feedStr[i] = feedStr[i + 1];
      feedUntil[i] = feedUntil[i + 1];
    }
    feedStr[FEED_ROWS - 1] = text;
    feedUntil[FEED_ROWS - 1] = strike.state().time + 2.4;
  };

  onCleanup(strike.on("playerDamaged", () => (flashDirty = true)));
  onCleanup(strike.on("hit", (e) => {
    if (e.type !== "hit") return;
    hitDuration = e.headshot ? 240 : 160;
    if (e.fatal) pushFeed(e.headshot ? "HEADSHOT × HOSTILE DOWN" : "HOSTILE DOWN");
  }));
  onCleanup(strike.on("roundReset", () => {
    for (let i = 0; i < FEED_ROWS; i++) feedUntil[i] = 0;
    feedDirty = true;
    resetEffects = true;
  }));

  // Compile this fixed HUD's paint properties once. A kill/reset can change
  // many at once; publish them with one native call, preserving the existing
  // framework fallback on hosts without setPropBatch. Animated effect props
  // have their own jump/animate owner and are deliberately outside this batch.
  const P = {
    banner: 0, title: 1, top: 2, bottom: 3,
    hpText: 4, hpFill: 5, hpScale: 6, hpOffset: 7,
    ammoText: 8, ammoFill: 9, ammoScale: 10, ammoOffset: 11,
    reload: 12, reloadScale: 13, reloadOffset: 14, vignette: 15, crosshair: 16,
  };
  let paintBatch: JumpBatch;
  let paintDirty = false;
  const paint = (index: number, value: number) => {
    paintBatch.set(index, value);
    paintDirty = true;
  };
  onMount(() => {
    paintBatch = createJumpBatch([
      [banner, "opacity"], [bannerTitle, "textColor"],
      [bannerTop, "bgColor"], [bannerBottom, "bgColor"],
      [hpText, "textColor"], [hpFill, "bgColor"], [hpFill, "scaleX"], [hpFill, "translateX"],
      [ammoText, "textColor"], [ammoFill, "bgColor"], [ammoFill, "scaleX"], [ammoFill, "translateX"],
      [reloadGroup, "opacity"], [reloadFill, "scaleX"], [reloadFill, "translateX"],
      [vignette, "opacity"], [crosshair, "opacity"],
    ]);
    const initial = [0, INK_N, INK_N, INK_N, INK_N, INK_N, 1, 0, INK_N, LIME_N, 1, 0, 0, 1, 0, 0, 1];
    for (let i = 0; i < initial.length; i++) paint(i, initial[i]);
  });

  /** Left-anchored fill: scaleX shrinks about the center, so pull the bar
   *  left by half the lost width. Paint-only — never touches layout. */
  const fill = (scale: number, frac: number, w: number) => {
    const f = Math.max(0, Math.min(1, frac));
    paint(scale, f);
    paint(scale + 1, (-(1 - f) * w * S) / 2);
  };

  // Interpreter discipline: on a 333 MHz QuickJS even a GATED call costs
  // real time, so the quiet-frame path below is straight-line number
  // compares on closure locals — zero function calls when nothing changed.
  let lHp = -1;
  let lAmmo = -1;
  let lPhase = "";
  let lBots = -1;
  let lTotal = -1;
  let lWins = -1;
  let lLosses = -1;
  let lReserve = -1;
  let lCount = -1;
  let lReloading = false;
  let lAlive = true;
  let lHpColor = 0;
  let lAmmoColor = 0;
  let lAmmoBarColor = 0;
  onFrame(() => {
    const s = strike.state();

    if (s.phase !== lPhase) {
      lPhase = s.phase;
      lCount = -1;
      paint(P.banner, s.phase === "live" ? 0 : 1);
      if (s.phase !== "live") {
        const color = s.phase === "won" ? LIME_N : s.phase === "lost" ? RED_N : INK_N;
        hot.text(bannerTitle, s.phase === "won" ? "HOSTILES ELIMINATED" : s.phase === "lost" ? "YOU DIED" : "ROUND START");
        paint(P.title, color);
        paint(P.top, color);
        paint(P.bottom, color);
      }
    }
    if (s.phase !== "live") {
      const left = (s.phase === "starting" ? ROUND_FREEZE : ROUND_END_PAUSE) - phaseAge();
      const c = Math.max(0, Math.ceil(left));
      if (c !== lCount) {
        lCount = c;
        hot.text(bannerSub, (s.phase === "starting" ? "GO IN " : "NEXT ROUND IN ") + c);
      }
    }
    if (s.aliveBots !== lBots || s.totalBots !== lTotal) {
      lBots = s.aliveBots;
      lTotal = s.totalBots;
      hot.text(botsText, s.aliveBots + "/" + s.totalBots);
    }
    if (s.wins !== lWins) {
      lWins = s.wins;
      hot.text(winsText, s.wins);
    }
    if (s.losses !== lLosses) {
      lLosses = s.losses;
      hot.text(lossesText, s.losses);
    }
    if (s.reserve !== lReserve) {
      lReserve = s.reserve;
      hot.text(reserveText, "/ " + s.reserve);
    }

    // Hot values: imperative, change-guarded, zero-layout.
    const hp = s.hp > 0 ? s.hp : 0;
    if (hp !== lHp) {
      lHp = hp;
      hot.text(hpText, hp);
      const hpColor = hp > 60 ? INK_N : hp > 25 ? AMBER_N : RED_N;
      if (hpColor !== lHpColor) {
        lHpColor = hpColor;
        paint(P.hpText, hpColor);
        paint(P.hpFill, hpColor);
      }
      fill(P.hpScale, hp / 100, BAR_W);
    }
    if (s.ammo !== lAmmo) {
      lAmmo = s.ammo;
      hot.text(ammoText, s.ammo);
      const color = s.ammo === 0 ? RED_N : INK_N;
      const barColor = s.ammo <= 5 ? RED_N : LIME_N;
      if (color !== lAmmoColor) {
        lAmmoColor = color;
        paint(P.ammoText, color);
      }
      if (barColor !== lAmmoBarColor) {
        lAmmoBarColor = barColor;
        paint(P.ammoFill, barColor);
      }
      fill(P.ammoScale, s.ammo / 30, AMMO_BAR_W);
    }
    if (s.reloading !== lReloading) {
      lReloading = s.reloading;
      paint(P.reload, s.reloading ? 1 : 0);
    }
    if (s.reloading) {
      const frac = Math.max(0, Math.min(1, s.reloadFrac));
      paint(P.reloadScale, frac);
      paint(P.reloadOffset, (-(1 - frac) * AMMO_BAR_W * S) / 2);
    }
    if (s.alive !== lAlive) {
      lAlive = s.alive;
      paint(P.vignette, s.alive ? 0 : 0.4);
      paint(P.crosshair, s.alive ? 1 : 0);
    }

    // The core advances these tweens at the same fixed tick as the game.
    // jump/animate exclusively own these props: no hot-value cache can hide
    // a repeated hit's restart or the cancellation on a round reset.
    if (resetEffects) {
      jump(flashOverlay, "opacity", 0);
      jump(hitmarker, "opacity", 0);
      resetEffects = false;
      flashDirty = false;
      hitDuration = 0;
    }
    if (flashDirty) {
      jump(flashOverlay, "opacity", 0.275);
      animate(flashOverlay, "opacity", 0, { dur: 423, easing: "linear" });
      flashDirty = false;
    }
    if (hitDuration > 0) {
      jump(hitmarker, "opacity", 1);
      animate(hitmarker, "opacity", 0, { dur: 0, delay: hitDuration, easing: "linear" });
      hitDuration = 0;
    }
    if (feedDirty) {
      for (let i = 0; i < FEED_ROWS; i++) {
        const left = Math.max(0, feedUntil[i] - s.time);
        hot.text(feedTexts[i], feedStr[i]);
        jump(feedRows[i], "opacity", Math.min(1, left / 0.4));
        if (left > 0) {
          animate(feedRows[i], "opacity", 0, {
            dur: Math.min(left, 0.4) * 1000,
            delay: Math.max(0, left - 0.4) * 1000,
            easing: "linear",
          });
        }
      }
      feedDirty = false;
    }
    if (paintDirty) {
      paintBatch.commit();
      paintDirty = false;
    }
  });

  return (
    <View class="w-full h-full">
      {/* Damage flash + death vignette (pre-mounted, hot opacity) */}
      <View
        ref={(el) => (flashOverlay = el)}
        class="absolute inset-0"
        style={{ bgColor: "#c40e0e", opacity: 0, zIndex: 5 }}
      />
      <View
        ref={(el) => (vignette = el)}
        class="absolute inset-0"
        style={{ bgColor: "#2a0404", opacity: 0, zIndex: 5 }}
      />

      {/* Crosshair + hitmarker */}
      <View
        ref={(el) => (crosshair = el)}
        class="absolute inset-0 justify-center items-center"
        style={{ zIndex: 10 }}
      >
        <View style={{ width: 36 * S, height: 36 * S }}>
          <Cross color={LIME} gap={11 * S} len={8 * S} thick={2 * S} />
          <View
            ref={(el) => (hitmarker = el)}
            class="absolute inset-0"
            style={{ opacity: 0 }}
          >
            <Cross color={RED} gap={6 * S} len={6 * S} thick={2 * S} rotated />
          </View>
        </View>
      </View>

      {/* Frame chrome: one full-screen column, everything anchored off it */}
      <View
        class={
          S >= 2
            ? "absolute inset-0 flex-col justify-between p-6"
            : "absolute inset-0 flex-col justify-between p-3"
        }
        style={{ zIndex: 20 }}
      >
        {/* ---- top row ---- */}
        <View class="flex-row justify-between items-start">
          <View style={{ width: 90 * S }} />
          {/* Phase banner: one instance, text/color swapped in place */}
          <View ref={(el) => (banner = el)} class="flex-col items-center" style={{ opacity: 0 }}>
            <View
              class="flex-col items-center gap-1 px-4 py-1 rounded-sm"
              style={{ bgColor: STRIP }}
            >
              <View ref={(el) => (bannerTop = el)} style={{ width: 120 * S, height: 1 * S, bgColor: INK }} />
              <Text
                ref={(el) => (bannerTitle = el)}
                class={S >= 2 ? "text-xl font-bold text-center" : "text-sm font-bold text-center"}
                style={{ textColor: INK, width: S >= 2 ? 280 : 174, height: S >= 2 ? 28 : 18 }}
              >
                ROUND START
              </Text>
              <Text
                class={
                  S >= 2
                    ? "text-sm font-bold tracking-wide text-center"
                    : "text-xs font-bold tracking-wide text-center"
                }
                ref={(el) => (bannerSub = el)}
                style={{ textColor: AMBER, width: S >= 2 ? 280 : 174, height: S >= 2 ? 20 : 14 }}
              >
                GO IN 2
              </Text>
              <View ref={(el) => (bannerBottom = el)} style={{ width: 120 * S, height: 1 * S, bgColor: INK }} />
            </View>
          </View>
          {/* Score strip + pooled kill feed */}
          <View class="flex-col items-end gap-1" style={{ width: 90 * S }}>
            <View
              class={
                S >= 2
                  ? "flex-row items-center gap-3 px-3 py-2 rounded-md"
                  : "flex-row items-center gap-2 px-2 py-1 rounded-sm"
              }
              style={{ bgColor: STRIP }}
            >
              <Text
                class={
                  S >= 2
                    ? "text-sm font-bold tracking-wide"
                    : "text-xs font-bold tracking-wide"
                }
                ref={(el) => (winsText = el)}
                style={{ textColor: LIME, width: S >= 2 ? 50 : 25, height: S >= 2 ? 20 : 14 }}
              >
                {s0.wins}
              </Text>
              <Text
                class={
                  S >= 2
                    ? "text-sm font-bold tracking-wide"
                    : "text-xs font-bold tracking-wide"
                }
                ref={(el) => (lossesText = el)}
                style={{ textColor: RED, width: S >= 2 ? 50 : 25, height: S >= 2 ? 20 : 14 }}
              >
                {s0.losses}
              </Text>
              <View style={{ width: 1, height: 8 * S, bgColor: "#e8f0f240" }} />
              <Text
                class={
                  S >= 2
                    ? "text-sm font-bold tracking-wide"
                    : "text-xs font-bold tracking-wide"
                }
                ref={(el) => (botsText = el)}
                style={{ textColor: AMBER, width: S >= 2 ? 68 : 34, height: S >= 2 ? 20 : 14 }}
              >
                {s0.aliveBots + "/" + s0.totalBots}
              </Text>
            </View>
            {Array.from({ length: FEED_ROWS }, (_, i) => (
              <View
                ref={(el) => (feedRows[i] = el)}
                class="px-2 py-1 rounded-sm"
                style={{ bgColor: STRIP, opacity: 0 }}
              >
                <Text
                  ref={(el) => (feedTexts[i] = el)}
                  class={
                    S >= 2
                      ? "text-sm font-bold tracking-wide"
                      : "text-xs font-bold tracking-wide"
                  }
                  style={{ textColor: INK, width: S >= 2 ? 260 : 170, height: S >= 2 ? 20 : 14 }}
                >
                  {" "}
                </Text>
              </View>
            ))}
          </View>
        </View>

        {/* ---- bottom row ---- */}
        <View class="flex-row justify-between items-end">
          {/* Health: fixed number cell + paint-only bar fill */}
          <View class="flex-col gap-1">
            <View
              class={
                S >= 2
                  ? "flex-row items-end gap-2 px-3 py-2 rounded-md"
                  : "flex-row items-end gap-1 px-2 py-1 rounded-sm"
              }
              style={{ bgColor: STRIP }}
            >
              <Text
                class={
                  S >= 2
                    ? "text-sm font-bold tracking-wide"
                    : "text-xs font-bold tracking-wide"
                }
                style={{ textColor: DIM }}
              >
                HP
              </Text>
              <Text
                ref={(el) => (hpText = el)}
                class={S >= 2 ? "text-4xl font-bold" : "text-lg font-bold"}
                style={{
                  textColor: INK,
                  width: S >= 2 ? 66 : 30,
                  height: S >= 2 ? 40 : 20,
                }}
              >
                100
              </Text>
            </View>
            <View style={{ width: BAR_W * S, height: 2 * S, bgColor: "#e8f0f21c" }}>
              <View
                ref={(el) => (hpFill = el)}
                style={{ width: BAR_W * S, height: 2 * S, bgColor: INK }}
              />
            </View>
          </View>
          {/* Ammo: fixed mag cell / reserve + reload bar (all pre-mounted) */}
          <View class="flex-col items-end gap-1">
            <View
              ref={(el) => (reloadGroup = el)}
              class="flex-col items-end gap-1"
              style={{ opacity: 0 }}
            >
              <Text
                class={
                  S >= 2
                    ? "text-sm font-bold tracking-wide"
                    : "text-xs font-bold tracking-wide"
                }
                style={{ textColor: AMBER }}
              >
                RELOADING
              </Text>
              <View style={{ width: AMMO_BAR_W * S, height: 2 * S, bgColor: "#e8f0f21c" }}>
                <View
                  ref={(el) => (reloadFill = el)}
                  style={{ width: AMMO_BAR_W * S, height: 2 * S, bgColor: AMBER }}
                />
              </View>
            </View>
            <View
              class={
                S >= 2
                  ? "flex-row items-end gap-2 px-3 py-2 rounded-md"
                  : "flex-row items-end gap-1 px-2 py-1 rounded-sm"
              }
              style={{ bgColor: STRIP }}
            >
              <Text
                ref={(el) => (ammoText = el)}
                class={S >= 2 ? "text-4xl font-bold" : "text-lg font-bold"}
                style={{
                  textColor: INK,
                  width: S >= 2 ? 44 : 20,
                  height: S >= 2 ? 40 : 20,
                }}
              >
                30
              </Text>
              <Text
                class={
                  S >= 2
                    ? "text-sm font-bold tracking-wide"
                    : "text-xs font-bold tracking-wide"
                }
                ref={(el) => (reserveText = el)}
                style={{ textColor: DIM, width: S >= 2 ? 70 : 35, height: S >= 2 ? 20 : 14 }}
              >
                {"/ " + s0.reserve}
              </Text>
            </View>
            <View style={{ width: AMMO_BAR_W * S, height: 2 * S, bgColor: "#e8f0f21c" }}>
              <View
                ref={(el) => (ammoFill = el)}
                style={{ width: AMMO_BAR_W * S, height: 2 * S, bgColor: LIME }}
              />
            </View>
          </View>
        </View>
      </View>

      {/* Static E7 keyboard legend: mounted once, never touched by the hot path. */}
      <Show when={IS_SYMBIAN_E7}>
        <View
          class="absolute items-center"
          style={{ insetL: 0, insetR: 0, insetB: 12 * S, zIndex: 21 }}
        >
          <View
            class="flex-col items-center gap-1 px-2 py-1 rounded-sm"
            style={{ bgColor: STRIP }}
          >
            <Text class="text-xs tracking-wide" style={{ textColor: DIM }}>
              WASD MOVE · ARROWS LOOK
            </Text>
            <Text class="text-xs tracking-wide" style={{ textColor: DIM }}>
              ENTER/E FIRE · R RELOAD
            </Text>
            <Text class="text-xs tracking-wide" style={{ textColor: DIM }}>
              SPACE JUMP · SHIFT WALK
            </Text>
            <Text class="text-xs tracking-wide" style={{ textColor: DIM }}>
              BACKSPACE/HOME MENU
            </Text>
          </View>
        </View>
      </Show>

      {/* Quit dialog (SELECT). CS-style: the world keeps running behind it. */}
      <Show when={dialog()}>
        <QuitDialog
          onStay={() => setDialog(false)}
          onQuit={() => {
            setDialog(false);
            strike.toMenu();
          }}
        />
      </Show>
    </View>
  );
}

/** SELECT dialog. On mount it traps focus (STAY lit by default — the safe
 *  choice); ↔ moves between the two buttons, each with a clear selected
 *  state (neutral for STAY, danger-red for QUIT), `○` confirms the lit one. */
function QuitDialog(props: { onStay: () => void; onQuit: () => void }) {
  let panel!: Ref;
  onMount(() => {
    const dispose = pushFocusScope(panel, { autoFocus: true, restoreFocus: true });
    onCleanup(dispose);
  });
  // Whole class strings must be single literals (the subset bakes them as a
  // unit — a `base + variant` concat would never resolve at runtime).
  const label = S >= 2 ? "text-sm font-bold tracking-wide" : "text-xs font-bold tracking-wide";
  return (
    <View
      class="absolute inset-0 justify-center items-center"
      style={{ bgColor: "#02040788", zIndex: 40 }}
    >
      <View class="flex-col items-center gap-2 px-5 py-3 rounded-md" style={{ bgColor: "#0a121aF0" }}>
        <Text
          class={S >= 2 ? "text-xl font-bold tracking-wide" : "text-sm font-bold tracking-wide"}
          style={{ textColor: INK }}
        >
          RETURN TO MAIN MENU?
        </Text>
        <View ref={(el) => (panel = el)} class="flex-row gap-2">
          <View
            focusable
            onPress={props.onStay}
            class="px-4 py-1 rounded-sm border-[#00000000] transition-colors duration-100 bg-[#111a24] focus:bg-slate-600 focus:border-slate-300"
          >
            <Text class={label} style={{ textColor: INK }}>
              STAY
            </Text>
          </View>
          <View
            focusable
            onPress={props.onQuit}
            class="px-4 py-1 rounded-sm border-[#00000000] transition-colors duration-100 bg-[#241014] focus:bg-red-800 focus:border-red-400"
          >
            <Text class={label} style={{ textColor: RED }}>
              QUIT
            </Text>
          </View>
        </View>
        <Show
          when={IS_SYMBIAN_E7}
          fallback={
            <Text class="text-xs tracking-wide" style={{ textColor: DIM }}>
              ↔ SELECT · ○ CONFIRM · SELECT CLOSE
            </Text>
          }
        >
          <View class="flex-col items-center gap-1">
            <Text class="text-xs tracking-wide" style={{ textColor: DIM }}>
              LEFT/RIGHT SELECT · ENTER CONFIRM
            </Text>
            <Text class="text-xs tracking-wide" style={{ textColor: DIM }}>
              BACKSPACE/HOME CLOSE
            </Text>
          </View>
        </Show>
      </View>
    </View>
  );
}

/** Four reticle bars around a center gap (rotated 45° for the hitmarker). */
function Cross(props: {
  color: string;
  gap: number;
  len: number;
  thick: number;
  rotated?: boolean;
}) {
  const c = 18 * S; // center of the reticle box
  const bar = (x: number, y: number, w: number, h: number) => (
    <View
      class="absolute"
      style={{ insetL: x, insetT: y, width: w, height: h, bgColor: props.color }}
    />
  );
  return (
    <View
      class="absolute inset-0"
      style={props.rotated ? { rotate: 45 } : undefined}
    >
      {bar(c - props.thick / 2, c - props.gap / 2 - props.len, props.thick, props.len)}
      {bar(c - props.thick / 2, c + props.gap / 2, props.thick, props.len)}
      {bar(c - props.gap / 2 - props.len, c - props.thick / 2, props.len, props.thick)}
      {bar(c + props.gap / 2, c - props.thick / 2, props.len, props.thick)}
    </View>
  );
}
