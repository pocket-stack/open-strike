// Shared product screen. Every native 3D host mounts this same menu/HUD over
// its target-specific Pocket3D renderer.

import { createSignal, Show } from "solid-js";
import Hud from "./hud.tsx";
import MainMenu from "./menu.tsx";
import { strike } from "./sdk.ts";

export default function OpenStrikeScreen() {
  const [inMenu, setInMenu] = createSignal(strike.state().phase === "menu");
  strike.onTick((state) => setInMenu(state.phase === "menu"));
  return (
    <Show when={!inMenu()} fallback={<MainMenu />}>
      <Hud />
    </Show>
  );
}
