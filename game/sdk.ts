// The `strike` surface SDK — the JS half of OpenStrike's vocabulary.
// Host side: crates/openstrike/src/guest.rs.
//
// Per tick the host calls `strike.__dispatch(state, events)` (facts), then
// the PocketJS frame turn runs (HUD). Commands issued here queue on the host
// and apply after the guest turn — state read through this module is always
// the host's last-published snapshot, never a guess.

export interface StrikeState {
  time: number;
  phase: "menu" | "starting" | "live" | "won" | "lost";
  hp: number;
  alive: boolean;
  ammo: number;
  reserve: number;
  reloading: boolean;
  reloadFrac: number;
  aliveBots: number;
  totalBots: number;
  wins: number;
  losses: number;
  speed: number;
}

export type StrikeEvent =
  | { type: "hit"; bot: number; headshot: boolean; damage: number; fatal: boolean }
  | { type: "playerDamaged"; amount: number; hp: number }
  | { type: "playerDied" }
  | { type: "roundReset" };

export interface WeaponConfig {
  magSize: number;
  reserve: number;
  fireInterval: number;
  reloadTime: number;
  damageBody: number;
  damageHead: number;
}

export interface BotsConfig {
  count: number;
  speed: number;
  attackInterval: number;
  damageMin: number;
  damageMax: number;
}

interface NativeStrike {
  /** Cooked maps available to loadMap (index-aligned), host-injected. */
  maps?: string[];
  loadMap?(index: number): void;
  toMenu?(): void;
  setPhase(phase: string): void;
  resetRound(): void;
  addWin(): void;
  addLoss(): void;
  setBotCount(n: number): void;
  configureWeapon(cfg: WeaponConfig): void;
  configureBots(cfg: BotsConfig): void;
  __dispatch?: (state: StrikeState, events: StrikeEvent[]) => void;
}

const native = (globalThis as { strike?: NativeStrike }).strike;
if (!native) {
  throw new Error("openstrike: no `strike` surface — is this running under the game host?");
}

let current: StrikeState = {
  time: 0,
  phase: "starting",
  hp: 100,
  alive: true,
  ammo: 30,
  reserve: 90,
  reloading: false,
  reloadFrac: 0,
  aliveBots: 0,
  totalBots: 0,
  wins: 0,
  losses: 0,
  speed: 0,
};

type Handler = (e: StrikeEvent) => void;
type TickHandler = (s: StrikeState) => void;
const handlers = new Map<string, Set<Handler>>();
const tickHandlers = new Set<TickHandler>();

native.__dispatch = (state, events) => {
  current = state;
  for (const e of events) {
    const set = handlers.get(e.type);
    if (set) for (const h of [...set]) h(e);
  }
  for (const h of [...tickHandlers]) h(state);
};

export const strike = {
  /** The last state snapshot the host published (this tick). */
  state: (): StrikeState => current,

  /** Subscribe to a game event; returns an unsubscribe. */
  on(type: StrikeEvent["type"], fn: Handler): () => void {
    let set = handlers.get(type);
    if (!set) handlers.set(type, (set = new Set()));
    set.add(fn);
    return () => set.delete(fn);
  },

  /** Runs once per tick, after events, with the fresh state. */
  onTick(fn: TickHandler): () => void {
    tickHandlers.add(fn);
    return () => tickHandlers.delete(fn);
  },

  // ---- intent (queued host-side, applied after this guest turn) ----------
  /** Map names the host can load (empty on hosts that boot pre-loaded). */
  maps: (native.maps ?? []) as readonly string[],
  /** Ask the host to load a cooked map and start a round (menu hosts). */
  loadMap: (index: number) => native.loadMap?.(index),
  /** Leave the round and return to the main menu (menu hosts). */
  toMenu: () => native.toMenu?.(),
  setPhase: (phase: StrikeState["phase"]) => native.setPhase(phase),
  resetRound: () => native.resetRound(),
  addWin: () => native.addWin(),
  addLoss: () => native.addLoss(),
  setBotCount: (n: number) => native.setBotCount(n),
  configureWeapon: (cfg: WeaponConfig) => native.configureWeapon(cfg),
  configureBots: (cfg: BotsConfig) => native.configureBots(cfg),
};
