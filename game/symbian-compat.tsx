// Nokia E7 compatibility runtime + renderer.
//
// The generic PocketJS Symbian host owns a 2D UI surface, not OpenStrike's
// native `strike` vocabulary or Pocket3D world renderer. On that one host,
// and only while no future native `strike` surface exists, install the
// deterministic top-down compatibility simulation. The on-screen label is
// intentionally explicit: this is a playable 2D mode, not the BSP/3D port.

import { createSignal, For, Show } from "solid-js";
import { Text, View } from "@pocketjs/framework/components";
import * as hot from "@pocketjs/framework/hot";
import { BTN } from "@pocketjs/framework/input";
import { onFrame } from "@pocketjs/framework/lifecycle";
import type { NativeStrike, StrikeState } from "./sdk.ts";
import {
  SYMBIAN_COMPAT_MAPS,
  SymbianCompatSimulation,
  type SymbianCompatInput,
} from "./symbian-compat-sim.ts";

const ARENA_W = 320;
const ARENA_H = 200;
const ACTOR_SIZE = 10;
const FRAME_DT = 1 / 30;

type Ref = NonNullable<Parameters<typeof hot.prop>[0]>;
type HostGlobal = typeof globalThis & {
  ui?: { __host?: string };
  strike?: NativeStrike;
};

const hostGlobal = globalThis as HostGlobal;
const nativeStrikeExists = hostGlobal.strike !== undefined;

/** True only for the reduced OpenStrike-owned mode, never a native game host. */
export const symbianCompatibilityActive =
  hostGlobal.ui?.__host === "symbian-e7-dev" && !nativeStrikeExists;

// Native PSP/Vita/desktop bundles parse the shared guest but allocate none of
// the compatibility game state.
const simulation = symbianCompatibilityActive
  ? new SymbianCompatSimulation()
  : undefined;
const compatibilitySurface: NativeStrike | undefined = simulation
  ? {
      maps: [...SYMBIAN_COMPAT_MAPS],
      __initialState: simulation.initialState(),
      loadMap: (index) => simulation.loadMap(index),
      toMenu: () => simulation.toMenu(),
      setPhase: (phase) => {
        if (
          phase === "menu" ||
          phase === "starting" ||
          phase === "live" ||
          phase === "won" ||
          phase === "lost"
        ) {
          simulation.queueSetPhase(phase);
        }
      },
      resetRound: () => simulation.queueResetRound(),
      addWin: () => simulation.queueAddWin(),
      addLoss: () => simulation.queueAddLoss(),
      setBotCount: (count) => simulation.queueSetBotCount(count),
      configureWeapon: (config) => simulation.queueConfigureWeapon(config),
      configureBots: (config) => simulation.queueConfigureBots(config),
    }
  : undefined;

if (compatibilitySurface) hostGlobal.strike = compatibilitySurface;

export function symbianCompatInput(
  buttons: number,
  previousButtons: number,
): SymbianCompatInput {
  return {
    forward:
      (buttons & BTN.UP ? 1 : 0) -
      (buttons & BTN.DOWN ? 1 : 0),
    turn:
      (buttons & BTN.RIGHT ? 1 : 0) -
      (buttons & BTN.LEFT ? 1 : 0),
    // E on the E7 keyboard is the Symbian host's right-trigger mapping.
    fire: (buttons & BTN.RTRIGGER) !== 0,
    // S is Square. Reload is an edge, like the native hosts.
    reload:
      (buttons & BTN.SQUARE) !== 0 &&
      (previousButtons & BTN.SQUARE) === 0,
  };
}

/**
 * Always-mounted driver plus the reduced world view. Keeping the driver
 * alive while the menu is visible lets `loadMap()` transition into gameplay
 * on the next host frame without a native lifecycle channel.
 */
export function SymbianCompatibilityLayer() {
  if (!simulation || !compatibilitySurface) return null;

  const [phase, setPhase] = createSignal<StrikeState["phase"]>("menu");
  const [mapIndex, setMapIndex] = createSignal(0);
  let previousButtons = 0;
  let playerRef: Ref | undefined;
  let aimRef: Ref | undefined;
  const botRefs: Array<Ref | undefined> = Array.from({ length: 8 });

  const syncScene = () => {
    const scene = simulation.scene();
    const playerX = scene.player.x * ARENA_W - ACTOR_SIZE / 2;
    const playerY = scene.player.y * ARENA_H - ACTOR_SIZE / 2;
    const angle = (scene.player.angle * 180) / Math.PI;
    hot.prop(playerRef, "translateX", playerX);
    hot.prop(playerRef, "translateY", playerY);
    hot.prop(playerRef, "rotate", angle);
    hot.prop(playerRef, "opacity", scene.player.alive ? 1 : 0.35);
    hot.prop(aimRef, "translateX", playerX + ACTOR_SIZE / 2);
    hot.prop(aimRef, "translateY", playerY + ACTOR_SIZE / 2 - 1);
    hot.prop(aimRef, "rotate", angle);

    for (let index = 0; index < botRefs.length; index++) {
      const bot = scene.bots[index];
      const ref = botRefs[index];
      hot.prop(ref, "opacity", bot?.alive ? 1 : bot ? 0.18 : 0);
      if (!bot) continue;
      hot.prop(ref, "translateX", bot.x * ARENA_W - ACTOR_SIZE / 2);
      hot.prop(ref, "translateY", bot.y * ARENA_H - ACTOR_SIZE / 2);
      hot.prop(ref, "rotate", (bot.angle * 180) / Math.PI);
    }
  };

  onFrame((buttons) => {
    simulation.tick(symbianCompatInput(buttons, previousButtons), FRAME_DT);
    previousButtons = buttons;

    const state = simulation.snapshot();
    const events = simulation.drainEvents();
    compatibilitySurface.__dispatch?.(state, events);
    // Gameplay rules queue intents from __dispatch callbacks. Apply them only
    // afterwards, matching the native PSP/Vita strike surface.
    simulation.flushCommands();

    const next = simulation.snapshot();
    setPhase(next.phase);
    setMapIndex(simulation.scene().mapIndex);
    if (next.phase !== "menu") syncScene();
  });

  const obstacles = () => {
    mapIndex();
    return simulation.scene().obstacles;
  };

  return (
    <Show when={phase() !== "menu"}>
      <View
        class="absolute inset-0 justify-center items-center"
        style={{ bgColor: "#071019", zIndex: 0 }}
      >
        <View
          class="relative overflow-hidden border-[#324454]"
          style={{
            width: ARENA_W,
            height: ARENA_H,
            bgColor: "#0b1720",
          }}
        >
          {/* Simple tactical floor grid. */}
          {Array.from({ length: 7 }, (_, index) => (
            <View
              class="absolute"
              style={{
                insetL: 0,
                insetT: 25 * (index + 1),
                width: ARENA_W,
                height: 1,
                bgColor: "#172936",
              }}
            />
          ))}
          {Array.from({ length: 11 }, (_, index) => (
            <View
              class="absolute"
              style={{
                insetL: 28 * (index + 1),
                insetT: 0,
                width: 1,
                height: ARENA_H,
                bgColor: "#172936",
              }}
            />
          ))}

          <For each={obstacles()}>
            {(obstacle) => (
              <View
                class="absolute border-[#60717d]"
                style={{
                  insetL: obstacle.x * ARENA_W,
                  insetT: obstacle.y * ARENA_H,
                  width: obstacle.w * ARENA_W,
                  height: obstacle.h * ARENA_H,
                  bgColor: "#263743",
                }}
              />
            )}
          </For>

          <View
            ref={(element) => (aimRef = element)}
            class="absolute"
            style={{
              insetL: 0,
              insetT: 0,
              width: 30,
              height: 2,
              bgColor: "#b8f34a80",
              opacity: 1,
            }}
          />
          <View
            ref={(element) => (playerRef = element)}
            class="absolute border-[#e8f0f2]"
            style={{
              insetL: 0,
              insetT: 0,
              width: ACTOR_SIZE,
              height: ACTOR_SIZE,
              bgColor: "#b8f34a",
            }}
          >
            <View
              class="absolute"
              style={{
                insetL: ACTOR_SIZE - 2,
                insetT: 3,
                width: 5,
                height: 3,
                bgColor: "#e8f0f2",
              }}
            />
          </View>

          {Array.from({ length: 8 }, (_, index) => (
            <View
              ref={(element) => (botRefs[index] = element)}
              class="absolute border-[#fecaca]"
              style={{
                insetL: 0,
                insetT: 0,
                width: ACTOR_SIZE,
                height: ACTOR_SIZE,
                bgColor: "#dc3038",
                opacity: 0,
              }}
            >
              <View
                class="absolute"
                style={{
                  insetL: ACTOR_SIZE - 2,
                  insetT: 3,
                  width: 4,
                  height: 3,
                  bgColor: "#fecaca",
                }}
              />
            </View>
          ))}

          <View
            class="absolute flex-row items-center gap-2 px-2 py-1"
            style={{ insetL: 4, insetT: 4, bgColor: "#02070acc" }}
          >
            <Text class="text-xs font-bold tracking-wide" style={{ textColor: "#b8f34a" }}>
              E7 2D COMPAT
            </Text>
            <Text class="text-xs tracking-wide" style={{ textColor: "#fbbf24" }}>
              NO 3D
            </Text>
          </View>
          <Text
            class="absolute text-xs tracking-wide"
            style={{
              insetL: 6,
              insetB: 5,
              textColor: "#8fa3ad",
            }}
          >
            {SYMBIAN_COMPAT_MAPS[mapIndex()].toUpperCase()}
          </Text>
        </View>
        <Text
          class="text-xs tracking-wide mt-1"
          style={{ textColor: "#8fa3ad" }}
        >
          ARROWS MOVE / TURN · E FIRE · S RELOAD
        </Text>
      </View>
    </Show>
  );
}
