// The base game, written as the first mod (RUNTIMES.md discipline #5: if the
// built-in behavior can't be expressed through the surface, the surface is
// too weak). Round flow, scoring and difficulty all live HERE — the Rust
// core simulates; it never decides.

import { strike } from "./sdk.ts";

/** Freeze time before a round goes live (seconds). */
export const ROUND_FREEZE = 1.2;
/** Pause on the end screen before the next round (seconds). */
export const ROUND_END_PAUSE = 3.5;

// Mod data owns initialization settings; the same round rules run for every
// pack. Native resources switch at map load, after the prior frame finishes.
strike.configureWeapon(strike.mod().weapon);
strike.configureBots(strike.mod().bots);

/** Seconds since the current phase began (HUD reads this for countdowns). */
let age = 0;
export const phaseAge = () => age;

let lastPhase = "";
let phaseStart = 0;

strike.onTick((s) => {
  if (s.phase !== lastPhase) {
    lastPhase = s.phase;
    phaseStart = s.time;
  }
  age = s.time - phaseStart;
  if (s.network) return;

  if (s.phase === "starting" && s.time - phaseStart >= ROUND_FREEZE) {
    strike.setPhase("live");
  }
  if (s.phase === "live" && s.totalBots > 0 && s.aliveBots === 0) {
    strike.addWin();
    strike.setPhase("won");
  }
  if ((s.phase === "won" || s.phase === "lost") && s.time - phaseStart >= ROUND_END_PAUSE) {
    strike.resetRound();
  }
});

strike.on("playerDied", () => {
  if (strike.state().network) return;
  strike.addLoss();
  strike.setPhase("lost");
});
