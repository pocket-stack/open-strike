// Shared product screen. Native 3D hosts mount this directly; the explicitly
// reduced Symbian entry places its compatibility world underneath it.

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
