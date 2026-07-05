// @title OpenStrike
// The product bundle: gameplay rules (the base game as a mod) + the JSX HUD.
// One QuickJS guest runs both — apps, games and mods are the same kind of
// artifact here.

import "./rules.ts";
import { mount } from "@pocketjs/framework";
import Hud from "./hud.tsx";

mount(() => <Hud />);
