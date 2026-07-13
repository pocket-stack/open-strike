// @title OpenStrike
// The product bundle: gameplay rules (the base game as a mod) + the JSX HUD.
// One QuickJS guest runs both — apps, games and mods are the same kind of
// artifact here.

import "./rules.ts";
import { createSignal, Show } from "solid-js";
import { mount } from "@pocketjs/framework";
import Hud from "./hud.tsx";
import MainMenu from "./menu.tsx";
import { strike } from "./sdk.ts";

// Phase-switched root: handheld menu hosts (PSP EBOOT / Vita VPK) boot into
// phase "menu";
// hosts that pre-load a map (desktop --map) never publish it and go
// straight to the HUD. The menu<->game swap is structural but rare.
function App() {
  const [inMenu, setInMenu] = createSignal(strike.state().phase === "menu");
  strike.onTick((s) => setInMenu(s.phase === "menu"));
  return (
    <Show when={!inMenu()} fallback={<MainMenu />}>
      <Hud />
    </Show>
  );
}

mount(() => <App />);
