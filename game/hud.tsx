// The OpenStrike HUD — a PocketJS app (Solid + Tailwind subset) composited
// over the 3D frame by pocket-ui-wgpu. Everything here is ordinary PocketJS
// UI code: the same framework that drives PSP hardware renders this at the
// game's resolution.

import { For, Show, createSignal } from "solid-js";
import { Text, View } from "@pocketjs/framework/components";
import { onFrame } from "@pocketjs/framework/lifecycle";
import { strike, type StrikeState } from "./sdk.ts";
import { ROUND_FREEZE, ROUND_END_PAUSE, phaseAge } from "./rules.ts";

const TICK = 1 / 64;

// Palette (military night-ops): lime reticle, amber warnings, blood red.
const INK = "#e8f0f2";
const LIME = "#b8f34aee";
const AMBER = "#fbbf24";
const RED = "#f43f3f";
const PANEL = "#080d12aa";

export default function Hud() {
  // One signal per state field, written every frame: Solid's equality gate
  // means an unchanged field costs one comparison and wakes nothing. A
  // single whole-state signal here re-ran every binding every frame — on
  // the PSP that was ~20 ms of QuickJS per frame; fine-grained it is ~1 ms.
  const s0 = strike.state();
  const [phase, setPhase] = createSignal(s0.phase);
  const [hp, setHp] = createSignal(s0.hp);
  const [alive, setAlive] = createSignal(s0.alive);
  const [ammo, setAmmo] = createSignal(s0.ammo);
  const [reserve, setReserve] = createSignal(s0.reserve);
  const [reloading, setReloading] = createSignal(s0.reloading);
  const [reloadFrac, setReloadFrac] = createSignal(s0.reloadFrac);
  const [aliveBots, setAliveBots] = createSignal(s0.aliveBots);
  const [totalBots, setTotalBots] = createSignal(s0.totalBots);
  const [wins, setWins] = createSignal(s0.wins);
  const [losses, setLosses] = createSignal(s0.losses);
  const readState = (s: StrikeState) => {
    setPhase(s.phase);
    setHp(s.hp);
    setAlive(s.alive);
    setAmmo(s.ammo);
    setReserve(s.reserve);
    setReloading(s.reloading);
    setReloadFrac(s.reloadFrac);
    setAliveBots(s.aliveBots);
    setTotalBots(s.totalBots);
    setWins(s.wins);
    setLosses(s.losses);
  };
  const [flash, setFlash] = createSignal(0);
  const [hitmark, setHitmark] = createSignal(0);
  const [feed, setFeed] = createSignal<{ id: number; text: string; ttl: number }[]>([]);
  let feedId = 0;

  strike.on("playerDamaged", () => setFlash(0.55));
  strike.on("hit", (e) => {
    if (e.type !== "hit") return;
    setHitmark(e.headshot ? 0.24 : 0.16);
    if (e.fatal) {
      const text = e.headshot ? "HEADSHOT  ×  HOSTILE DOWN" : "HOSTILE DOWN";
      setFeed((f) => [...f.slice(-3), { id: feedId++, text, ttl: 2.4 }]);
    }
  });
  strike.on("roundReset", () => {
    setFeed([]);
    setFlash(0);
  });

  onFrame(() => {
    readState(strike.state());
    setFlash((f) => Math.max(0, f - TICK * 1.3));
    setHitmark((h) => Math.max(0, h - TICK));
    setFeed((f) => {
      let dirty = false;
      const next = f
        .map((e) => ({ ...e, ttl: e.ttl - TICK }))
        .filter((e) => (e.ttl > 0 ? true : ((dirty = true), false)));
      return dirty || f.length > 0 ? next : f;
    });
  });

  const hpColor = () => (hp() > 60 ? INK : hp() > 25 ? AMBER : RED);

  return (
    <View class="w-full h-full">
      {/* Damage flash + death vignette */}
      <Show when={flash() > 0}>
        <View
          class="absolute inset-0"
          style={{ bgColor: "#c40e0e", opacity: flash() * 0.5, zIndex: 5 }}
        />
      </Show>
      <Show when={!alive()}>
        <View class="absolute inset-0" style={{ bgColor: "#2a040466", zIndex: 5 }} />
      </Show>

      {/* Crosshair + hitmarker */}
      <Show when={alive()}>
        <View class="absolute inset-0 justify-center items-center" style={{ zIndex: 10 }}>
          <View style={{ width: 44, height: 44 }}>
            <Cross color={LIME} gap={16} len={12} thick={2} />
            <Show when={hitmark() > 0}>
              <Cross color={RED} gap={7} len={7} thick={3} rotated />
            </Show>
          </View>
        </View>
      </Show>

      {/* Frame chrome: everything anchors off one full-screen column */}
      <View class="absolute inset-0 flex-col justify-between p-6" style={{ zIndex: 20 }}>
        {/* ---- top row ---- */}
        <View class="flex-row justify-between items-start">
          <View style={{ width: 240 }} />
          {/* Banner */}
          <View class="flex-col items-center gap-2">
            <Show when={phase() === "starting"}>
              <Banner color={INK} title="ROUND START">
                <Text class="text-sm tracking-wide" style={{ textColor: AMBER }}>
                  {"GO IN " + Math.max(0, Math.ceil(ROUND_FREEZE - phaseAge())) + " "}
                </Text>
              </Banner>
            </Show>
            <Show when={phase() === "won"}>
              <Banner color={LIME} title="HOSTILES ELIMINATED">
                <Text class="text-sm tracking-wide" style={{ textColor: INK }}>
                  {"NEXT ROUND IN " + Math.max(0, Math.ceil(ROUND_END_PAUSE - phaseAge())) + " "}
                </Text>
              </Banner>
            </Show>
            <Show when={phase() === "lost"}>
              <Banner color={RED} title="YOU DIED">
                <Text class="text-sm tracking-wide" style={{ textColor: AMBER }}>
                  {"NEXT ROUND IN " + Math.max(0, Math.ceil(ROUND_END_PAUSE - phaseAge())) + " "}
                </Text>
              </Banner>
            </Show>
          </View>
          {/* Score + hostiles + kill feed */}
          <View class="flex-col items-end gap-2" style={{ width: 240 }}>
            <View class="flex-row items-center gap-3 px-3 py-2" style={{ bgColor: PANEL }}>
              <Text class="text-sm font-bold tracking-wide" style={{ textColor: LIME }}>
                {"W " + wins()}
              </Text>
              <Text class="text-sm font-bold tracking-wide" style={{ textColor: RED }}>
                {"L " + losses()}
              </Text>
              <View style={{ width: 2, height: 14, bgColor: "#e8f0f240" }} />
              <Text class="text-sm font-bold tracking-wide" style={{ textColor: AMBER }}>
                {"HOSTILES " + aliveBots() + "/" + totalBots()}
              </Text>
            </View>
            <For each={feed()}>
              {(e) => (
                <View class="px-3 py-1" style={{ bgColor: PANEL, opacity: Math.min(1, e.ttl / 0.4) }}>
                  <Text class="text-sm font-bold tracking-wide" style={{ textColor: INK }}>
                    {e.text}
                  </Text>
                </View>
              )}
            </For>
          </View>
        </View>

        {/* ---- bottom row ---- */}
        <View class="flex-row justify-between items-end">
          {/* Health */}
          <View class="flex-col gap-1">
            <View class="flex-row items-end gap-3 px-4 py-2" style={{ bgColor: PANEL }}>
              <Text class="text-sm font-bold tracking-wide" style={{ textColor: "#8fa3ad" }}>
                HP
              </Text>
              <Text class="text-4xl font-bold" style={{ textColor: hpColor() }}>
                {Math.max(0, hp())}
              </Text>
            </View>
            <View style={{ width: 220, height: 5, bgColor: "#e8f0f21c" }}>
              <View
                style={{
                  width: Math.max(0, (hp() / 100) * 220),
                  height: 5,
                  bgColor: hpColor(),
                }}
              />
            </View>
          </View>
          {/* Ammo */}
          <View class="flex-col items-end gap-1">
            <Show when={reloading()}>
              <View class="flex-col items-end gap-1">
                <Text class="text-sm font-bold tracking-wide" style={{ textColor: AMBER }}>
                  RELOADING
                </Text>
                <View style={{ width: 160, height: 4, bgColor: "#e8f0f21c" }}>
                  <View
                    style={{ width: reloadFrac() * 160, height: 4, bgColor: AMBER }}
                  />
                </View>
              </View>
            </Show>
            <View class="flex-row items-end gap-2 px-4 py-2" style={{ bgColor: PANEL }}>
              <Text
                class="text-4xl font-bold"
                style={{ textColor: ammo() === 0 ? RED : INK }}
              >
                {ammo()}
              </Text>
              <Text class="text-xl font-bold" style={{ textColor: "#8fa3ad" }}>
                {"/ " + reserve()}
              </Text>
            </View>
          </View>
        </View>
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
  const c = 22; // center of the 44x44 reticle box
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

/** Angular top banner: rule / TITLE / rule. */
function Banner(props: { color: string; title: string; children?: unknown }) {
  return (
    <View class="flex-col items-center gap-2 px-6 py-3" style={{ bgColor: PANEL }}>
      <View style={{ width: 260, height: 2, bgColor: props.color }} />
      <Text class="text-2xl font-bold tracking-wide" style={{ textColor: props.color }}>
        {props.title}
      </Text>
      <View style={{ width: 260, height: 2, bgColor: props.color }} />
      {props.children as never}
    </View>
  );
}
