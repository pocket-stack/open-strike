// The main menu — same night-ops language as the HUD, driven by the PocketJS
// focus system. On mount it grabs focus (first map lit) and registers a
// two-column grid so the d-pad walks the visible layout; `○` deploys the
// focused map via `strike.loadMap(i)` and the host swaps the world in.

import { createSignal, For, Show, onCleanup, onMount } from "solid-js";
import { Text, View } from "@pocketjs/framework/components";
import { pushFocusGrid, pushFocusScope } from "@pocketjs/framework/input";
import { platform } from "@pocketjs/framework/platform";
import { onButtonPress } from "@pocketjs/framework/lifecycle";
import { strike } from "./sdk.ts";

const INK = "#e8f0f2";
const DIM = "#8fa3ad";
const LIME = "#b8f34a";
const AMBER = "#fbbf24";

const vp = (globalThis as { ui?: { __viewport?: { w: number; h: number } } }).ui
  ?.__viewport ?? { w: 480, h: 272 };
// Scale only when a complete 480x272 reference frame fits. Height-only
// scaling made the E7's 360x640 portrait viewport choose 2x and clip the
// 600px-wide map grid; using both extents keeps every supported E7
// orientation at the compact scale while preserving exact 1x PSP and 2x
// Vita proportions when those logical viewports are published.
const S = Math.max(1, Math.floor(Math.min(vp.w / 480, vp.h / 272)));
const IS_SYMBIAN_E7 = platform.target === "symbian-e7-dev";

// Selected-row look: a lime-tinted fill + a bright lime border + a small
// slide-in. The default keeps a transparent 1px border so the focused border
// doesn't change layout, and transition-all eases the whole thing in. This
// MUST be one string literal — the Tailwind subset bakes whole class strings,
// so a concatenation would leave the runtime string unbaked (no style).
const ROW_BASE =
  "flex-row items-center gap-2 px-2 py-1 rounded-sm border-[#0a121a00] bg-[#0a121a80] transition-all duration-100 focus:bg-[#33470f] focus:border-[#b8f34a] focus:translate-x-1";

/** "de_dust2" -> { tag: "DE", name: "DUST2" } */
const pretty = (raw: string): { tag: string; name: string } => {
  const us = raw.indexOf("_");
  if (us <= 0) return { tag: "", name: raw.toUpperCase() };
  return {
    tag: raw.slice(0, us).toUpperCase(),
    name: raw.slice(us + 1).toUpperCase(),
  };
};

export default function MainMenu() {
  const [choosingMod, setChoosingMod] = createSignal(strike.mods.length > 1 || strike.networkSupported);
  return (
    <Show
      when={!choosingMod()}
      fallback={<ModMenu onSelect={() => setChoosingMod(false)} />}
    >
      <MapMenu onBack={() => setChoosingMod(true)} />
    </Show>
  );
}

function ModMenu(props: { onSelect(): void }) {
  let grid!: Parameters<typeof pushFocusGrid>[0];
  onMount(() => {
    const disposeGrid = pushFocusGrid(grid, { columns: 1, wrap: true });
    const disposeScope = pushFocusScope(grid, { autoFocus: true });
    onCleanup(() => {
      disposeScope();
      disposeGrid();
    });
  });
  return (
    <View
      class="w-full h-full justify-center items-center"
      style={{ bgColor: "#05080cf5" }}
    >
      <View class="flex-col items-center gap-1">
        <Text
          class={
            S >= 2
              ? "text-4xl font-bold tracking-wide"
              : "text-xl font-bold tracking-wide"
          }
          style={{ textColor: INK }}
        >
          OPENSTRIKE
        </Text>
        <Text class="text-xs tracking-wide" style={{ textColor: DIM }}>
          CHOOSE YOUR LOADOUT
        </Text>
        <View
          ref={(el) => (grid = el)}
          class="flex-col gap-1 mt-3"
          style={{ width: 330 * S }}
        >
          <For each={strike.mods}>
            {(mod, index) => (
              <View
                focusable
                class={ROW_BASE}
                onPress={() => {
                  strike.selectNetwork(false);
                  if (strike.selectMod(index())) props.onSelect();
                }}
                style={{ width: 330 * S }}
              >
                <Text
                  class={S >= 2 ? "text-xl font-bold" : "text-sm font-bold"}
                  style={{ textColor: LIME, width: 86 * S }}
                >
                  {mod.title}
                </Text>
                <Text
                  class="text-xs"
                  style={{ textColor: DIM, width: 220 * S, height: 14 * S }}
                >
                  {mod.description}
                </Text>
              </View>
            )}
          </For>
          <Show when={strike.networkSupported}>
            <View focusable class={ROW_BASE} style={{ width: 330 * S }} onPress={() => {
              if (strike.selectMod(0)) { strike.selectNetwork(true); props.onSelect(); }
            }}>
              <Text class="text-sm font-bold" style={{ textColor: LIME, width: 86 * S }}>CROSSPLAY</Text>
              <Text class="text-xs" style={{ textColor: DIM }}>1 VS 1 · MAC + PSP</Text>
            </View>
          </Show>
        </View>
        <Text class="text-xs mt-3 tracking-wide" style={{ textColor: DIM }}>
          ↑↓ SELECT · ○ CONTINUE
        </Text>
      </View>
    </View>
  );
}

function MapMenu(props: { onBack(): void }) {
  const [loading, setLoading] = createSignal(-1);
  onButtonPress(0x0001, () => {
    if (loading() < 0 && (strike.mods.length > 1 || strike.networkSupported)) props.onBack();
  });
  const deploy = (i: number) => {
    if (loading() >= 0) return;
    setLoading(i);
    strike.loadMap(i);
  };

  // Focus: light the first map on mount and give the grid real 2-column
  // d-pad semantics (↕ moves a whole row, ↔ moves within it).
  let grid!: Parameters<typeof pushFocusGrid>[0];
  onMount(() => {
    const disposeGrid = pushFocusGrid(grid, { columns: 2, wrap: true });
    const disposeScope = pushFocusScope(grid, { autoFocus: true });
    onCleanup(() => {
      disposeScope();
      disposeGrid();
    });
  });

  return (
    <View
      class="w-full h-full justify-center items-center"
      style={{ bgColor: "#05080cE8" }}
    >
      <View class="flex-col items-center gap-1">
        {/* Masthead */}
        <Text
          class={
            S >= 2
              ? "text-5xl font-bold tracking-wide"
              : "text-2xl font-bold tracking-wide"
          }
          style={{ textColor: INK }}
        >
          OPENSTRIKE
        </Text>
        <View class="flex-row items-center gap-2">
          <View style={{ width: 28 * S, height: 1, bgColor: LIME }} />
          <Text
            class={S >= 2 ? "text-sm tracking-wide" : "text-xs tracking-wide"}
            style={{ textColor: DIM }}
          >
            {strike.networkSelected() ? "CROSSPLAY · CHOOSE COMPANION MAP" : strike.mods.length > 1
              ? strike.mod().title.toUpperCase()
              : "TACTICAL OPERATIONS"}
          </Text>
          <View style={{ width: 28 * S, height: 1, bgColor: LIME }} />
        </View>

        {/* Map grid: two columns so eight maps + masthead fit 272 px */}
        <View
          ref={(el) => (grid = el)}
          class="flex-row flex-wrap gap-1 mt-3 justify-center"
          style={{ width: 300 * S }}
        >
          <For each={strike.maps as string[]}>
            {(raw, i) => (
              <View
                focusable
                onPress={() => deploy(i())}
                class={ROW_BASE}
                style={{ width: 145 * S }}
              >
                <Text
                  class={S >= 2 ? "text-sm font-bold" : "text-xs font-bold"}
                  style={{ textColor: LIME, width: 18 * S }}
                >
                  {pretty(raw).tag}
                </Text>
                <Text
                  class={
                    S >= 2
                      ? "text-xl font-bold tracking-wide"
                      : "text-sm font-bold tracking-wide"
                  }
                  style={{ textColor: INK }}
                >
                  {pretty(raw).name}
                </Text>
                <View class="flex-1" />
                <Show when={loading() === i()}>
                  <Text
                    class={S >= 2 ? "text-sm font-bold" : "text-xs font-bold"}
                    style={{ textColor: AMBER }}
                  >
                    …
                  </Text>
                </Show>
              </View>
            )}
          </For>
        </View>

        {/* Footer hints: retain console glyphs verbatim; E7 names its real keys. */}
        <Show
          when={IS_SYMBIAN_E7}
          fallback={
            <View class="flex-row gap-3 mt-3">
              <Text class="text-xs tracking-wide" style={{ textColor: DIM }}>
                ↕↔ SELECT
              </Text>
              <Text class="text-xs tracking-wide" style={{ textColor: DIM }}>
                ○ DEPLOY
              </Text>
              <Show when={strike.mods.length > 1}>
                <Text class="text-xs tracking-wide" style={{ textColor: DIM }}>
                  SELECT MOD
                </Text>
              </Show>
            </View>
          }
        >
          <View
            class="flex-col items-center gap-1 mt-3 px-2 py-1 rounded-sm"
            style={{ bgColor: "#0a121a80" }}
          >
            <Text class="text-xs tracking-wide" style={{ textColor: LIME }}>
              ARROWS SELECT · ENTER DEPLOY
            </Text>
            <Text class="text-xs tracking-wide" style={{ textColor: DIM }}>
              WASD MOVE · ARROWS LOOK
            </Text>
            <Text class="text-xs tracking-wide" style={{ textColor: DIM }}>
              ENTER/E FIRE · R RELOAD · SPACE JUMP
            </Text>
            <Text class="text-xs tracking-wide" style={{ textColor: DIM }}>
              SHIFT WALK · BACKSPACE/HOME MENU
            </Text>
          </View>
        </Show>
      </View>
    </View>
  );
}
