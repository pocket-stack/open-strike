// @title OpenStrike — E7 2D Compatibility
//
// Symbian-specific product entry. It installs the OpenStrike-owned playable
// 2D fallback before rules/sdk evaluate, then layers the ordinary menu/HUD on
// top. Native PSP/Vita/desktop entries never import this module or its sim.

import { SymbianCompatibilityLayer } from "./symbian-compat.tsx";
import "./rules.ts";
import { mount } from "@pocketjs/framework/solid";
import { View } from "@pocketjs/framework/components";
import OpenStrikeScreen from "./screen.tsx";

mount(() => (
  <View class="w-full h-full">
    <SymbianCompatibilityLayer />
    <OpenStrikeScreen />
  </View>
));
