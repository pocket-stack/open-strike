// OpenStrike's explicitly reduced Nokia E7 game mode.
//
// This is not a replacement for openstrike-core and does not pretend to be
// the BSP/3D game. The E7 PocketJS host currently has no Pocket3D backend or
// native `strike` surface, so this small deterministic top-down simulation
// preserves the real product loop while that native port is still missing:
// map selection -> rules-driven round phases -> movement/combat -> HUD/events
// -> win/loss -> reset.

import type {
  BotsConfig,
  StrikeEvent,
  StrikeState,
  WeaponConfig,
} from "./sdk.ts";

export const SYMBIAN_COMPAT_MAPS = [
  "de_dust2",
  "de_inferno",
  "de_nuke",
  "cs_assault",
  "de_dust",
  "de_aztec",
  "de_train",
  "cs_office",
] as const;

export interface SymbianCompatInput {
  /** Forward/backward, from -1 through 1. */
  forward: number;
  /** Clockwise/counter-clockwise turn, from -1 through 1. */
  turn: number;
  /** Automatic-fire trigger held this step. */
  fire: boolean;
  /** Rising-edge reload action. */
  reload: boolean;
}

export interface SymbianCompatRect {
  readonly x: number;
  readonly y: number;
  readonly w: number;
  readonly h: number;
}

export interface SymbianCompatActor {
  readonly x: number;
  readonly y: number;
  readonly angle: number;
  readonly alive: boolean;
}

export interface SymbianCompatScene {
  readonly mapIndex: number;
  readonly mapName: string;
  readonly player: SymbianCompatActor;
  readonly bots: readonly SymbianCompatActor[];
  readonly obstacles: readonly SymbianCompatRect[];
}

interface MapLayout {
  readonly player: readonly [number, number, number];
  readonly bots: readonly (readonly [number, number])[];
  readonly obstacles: readonly SymbianCompatRect[];
}

interface MutableBot {
  x: number;
  y: number;
  hp: number;
  attackLeft: number;
  attacks: number;
}

type QueuedCommand =
  | { readonly type: "phase"; readonly phase: StrikeState["phase"] }
  | { readonly type: "reset" }
  | { readonly type: "win" }
  | { readonly type: "loss" }
  | { readonly type: "bot-count"; readonly count: number }
  | { readonly type: "weapon"; readonly config: WeaponConfig }
  | { readonly type: "bots"; readonly config: BotsConfig };

const DEFAULT_WEAPON: WeaponConfig = {
  magSize: 30,
  reserve: 90,
  fireInterval: 0.105,
  reloadTime: 2.4,
  damageBody: 34,
  damageHead: 100,
};

const DEFAULT_BOTS: BotsConfig = {
  count: 3,
  speed: 190,
  attackInterval: 1.4,
  damageMin: 8,
  damageMax: 14,
};

const LAYOUTS: readonly MapLayout[] = [
  {
    player: [0.5, 0.84, -Math.PI / 2],
    // The default three spawns share the center lane: every round is
    // completable without waypoint navigation, which this reduced mode does
    // not claim. Extra modded bots may occupy the outer lanes.
    bots: [[0.5, 0.16], [0.42, 0.24], [0.58, 0.24], [0.2, 0.62], [0.8, 0.62]],
    obstacles: [
      { x: 0.12, y: 0.43, w: 0.27, h: 0.08 },
      { x: 0.61, y: 0.43, w: 0.27, h: 0.08 },
    ],
  },
  {
    player: [0.5, 0.84, -Math.PI / 2],
    bots: [[0.5, 0.16], [0.16, 0.18], [0.84, 0.18], [0.18, 0.7], [0.82, 0.7]],
    obstacles: [
      { x: 0.23, y: 0.31, w: 0.12, h: 0.34 },
      { x: 0.65, y: 0.31, w: 0.12, h: 0.34 },
    ],
  },
  {
    player: [0.5, 0.84, -Math.PI / 2],
    bots: [[0.5, 0.16], [0.2, 0.2], [0.8, 0.2], [0.16, 0.5], [0.84, 0.5]],
    obstacles: [
      { x: 0.18, y: 0.4, w: 0.22, h: 0.1 },
      { x: 0.6, y: 0.4, w: 0.22, h: 0.1 },
      { x: 0.44, y: 0.58, w: 0.12, h: 0.1 },
    ],
  },
  {
    player: [0.5, 0.84, -Math.PI / 2],
    bots: [[0.5, 0.16], [0.13, 0.26], [0.87, 0.26], [0.25, 0.58], [0.75, 0.58]],
    obstacles: [
      { x: 0.16, y: 0.38, w: 0.2, h: 0.08 },
      { x: 0.64, y: 0.38, w: 0.2, h: 0.08 },
      { x: 0.4, y: 0.26, w: 0.2, h: 0.08 },
    ],
  },
];

const clamp = (value: number, min: number, max: number): number =>
  Math.max(min, Math.min(max, value));

const copyWeapon = (config: WeaponConfig): WeaponConfig => ({
  magSize: clamp(Math.floor(config.magSize), 1, 999),
  reserve: clamp(Math.floor(config.reserve), 0, 9999),
  fireInterval: clamp(config.fireInterval, 0.03, 10),
  reloadTime: clamp(config.reloadTime, 0.1, 30),
  damageBody: clamp(Math.floor(config.damageBody), 1, 9999),
  damageHead: clamp(Math.floor(config.damageHead), 1, 9999),
});

const copyBots = (config: BotsConfig): BotsConfig => ({
  count: clamp(Math.floor(config.count), 0, 8),
  speed: clamp(config.speed, 1, 1000),
  attackInterval: clamp(config.attackInterval, 0.1, 30),
  damageMin: clamp(Math.floor(config.damageMin), 0, 999),
  damageMax: clamp(Math.floor(config.damageMax), 0, 999),
});

const pointBlocked = (
  x: number,
  y: number,
  radius: number,
  layout: MapLayout,
): boolean =>
  layout.obstacles.some(
    (rect) =>
      x + radius > rect.x &&
      x - radius < rect.x + rect.w &&
      y + radius > rect.y &&
      y - radius < rect.y + rect.h,
  );

/** Liang-Barsky segment/rectangle test, expanded by the actor radius. */
const segmentBlocked = (
  x0: number,
  y0: number,
  x1: number,
  y1: number,
  layout: MapLayout,
  radius = 0,
): boolean => {
  const dx = x1 - x0;
  const dy = y1 - y0;
  for (const rect of layout.obstacles) {
    const minX = rect.x - radius;
    const maxX = rect.x + rect.w + radius;
    const minY = rect.y - radius;
    const maxY = rect.y + rect.h + radius;
    let near = 0;
    let far = 1;
    const clips: readonly (readonly [number, number])[] = [
      [-dx, x0 - minX],
      [dx, maxX - x0],
      [-dy, y0 - minY],
      [dy, maxY - y0],
    ];
    let hit = true;
    for (const [p, q] of clips) {
      if (Math.abs(p) < 1e-9) {
        if (q < 0) hit = false;
        continue;
      }
      const t = q / p;
      if (p < 0) near = Math.max(near, t);
      else far = Math.min(far, t);
      if (near > far) hit = false;
    }
    if (hit) return true;
  }
  return false;
};

export class SymbianCompatSimulation {
  private phase: StrikeState["phase"] = "menu";
  private time = 0;
  private hp = 100;
  private alive = true;
  private ammo = DEFAULT_WEAPON.magSize;
  private reserve = DEFAULT_WEAPON.reserve;
  private reloadLeft = 0;
  private fireLeft = 0;
  private wins = 0;
  private losses = 0;
  private speed = 0;
  private mapIndex = 0;
  private playerX = 0.5;
  private playerY = 0.84;
  private playerAngle = -Math.PI / 2;
  private bots: MutableBot[] = [];
  private weapon = copyWeapon(DEFAULT_WEAPON);
  private botConfig = copyBots(DEFAULT_BOTS);
  private readonly events: StrikeEvent[] = [];
  private readonly commands: QueuedCommand[] = [];

  initialState(): StrikeState {
    return this.snapshot();
  }

  snapshot(): StrikeState {
    const reloadFrac =
      this.reloadLeft > 0
        ? 1 - clamp(this.reloadLeft / this.weapon.reloadTime, 0, 1)
        : 0;
    return {
      time: this.time,
      phase: this.phase,
      hp: this.hp,
      alive: this.alive,
      ammo: this.ammo,
      reserve: this.reserve,
      reloading: this.reloadLeft > 0,
      reloadFrac,
      aliveBots: this.bots.filter((bot) => bot.hp > 0).length,
      totalBots: this.bots.length,
      wins: this.wins,
      losses: this.losses,
      speed: this.speed,
    };
  }

  scene(): SymbianCompatScene {
    const layout = this.layout();
    return {
      mapIndex: this.mapIndex,
      mapName: SYMBIAN_COMPAT_MAPS[this.mapIndex],
      player: {
        x: this.playerX,
        y: this.playerY,
        angle: this.playerAngle,
        alive: this.alive,
      },
      bots: this.bots.map((bot) => ({
        x: bot.x,
        y: bot.y,
        angle: Math.atan2(this.playerY - bot.y, this.playerX - bot.x),
        alive: bot.hp > 0,
      })),
      obstacles: layout.obstacles,
    };
  }

  drainEvents(): StrikeEvent[] {
    return this.events.splice(0);
  }

  loadMap(index: number): void {
    if (!Number.isInteger(index) || index < 0 || index >= SYMBIAN_COMPAT_MAPS.length) return;
    this.mapIndex = index;
    this.time = 0;
    this.resetRound(false);
  }

  toMenu(): void {
    this.phase = "menu";
    this.speed = 0;
    this.reloadLeft = 0;
    this.fireLeft = 0;
  }

  queueSetPhase(phase: StrikeState["phase"]): void {
    if (phase !== "menu" && phase !== "starting" && phase !== "live" && phase !== "won" && phase !== "lost") {
      return;
    }
    this.commands.push({ type: "phase", phase });
  }

  queueResetRound(): void {
    this.commands.push({ type: "reset" });
  }

  queueAddWin(): void {
    this.commands.push({ type: "win" });
  }

  queueAddLoss(): void {
    this.commands.push({ type: "loss" });
  }

  queueSetBotCount(count: number): void {
    this.commands.push({ type: "bot-count", count });
  }

  queueConfigureWeapon(config: WeaponConfig): void {
    this.commands.push({ type: "weapon", config: copyWeapon(config) });
  }

  queueConfigureBots(config: BotsConfig): void {
    this.commands.push({ type: "bots", config: copyBots(config) });
  }

  /** Apply guest intents after dispatch, matching the native host ordering. */
  flushCommands(): void {
    for (const command of this.commands.splice(0)) {
      switch (command.type) {
        case "phase":
          this.phase = command.phase;
          break;
        case "reset":
          this.resetRound(true);
          break;
        case "win":
          this.wins++;
          break;
        case "loss":
          this.losses++;
          break;
        case "bot-count":
          this.botConfig = {
            ...this.botConfig,
            count: clamp(Math.floor(command.count), 0, 8),
          };
          break;
        case "weapon":
          this.weapon = command.config;
          if (this.phase === "menu") {
            this.ammo = this.weapon.magSize;
            this.reserve = this.weapon.reserve;
          } else {
            this.ammo = Math.min(this.ammo, this.weapon.magSize);
          }
          break;
        case "bots":
          this.botConfig = command.config;
          break;
      }
    }
  }

  tick(input: SymbianCompatInput, elapsed: number): void {
    const dt = clamp(Number.isFinite(elapsed) ? elapsed : 0, 0, 0.1);
    this.time += dt;
    this.fireLeft = Math.max(0, this.fireLeft - dt);

    if (this.reloadLeft > 0) {
      this.reloadLeft = Math.max(0, this.reloadLeft - dt);
      if (this.reloadLeft === 0) this.finishReload();
    }

    if (this.phase !== "live" || !this.alive) {
      this.speed = 0;
      return;
    }

    const turn = clamp(input.turn, -1, 1);
    const forward = clamp(input.forward, -1, 1);
    this.playerAngle += turn * 2.45 * dt;
    const movement = forward * 0.28 * dt;
    this.movePlayer(Math.cos(this.playerAngle) * movement, Math.sin(this.playerAngle) * movement);
    this.speed = Math.abs(forward) * 250;

    if (input.reload) this.startReload();
    if (input.fire) this.fire();

    this.tickBots(dt);
  }

  private layout(): MapLayout {
    return LAYOUTS[this.mapIndex % LAYOUTS.length];
  }

  private resetRound(emitEvent: boolean): void {
    const layout = this.layout();
    this.phase = "starting";
    this.hp = 100;
    this.alive = true;
    this.ammo = this.weapon.magSize;
    this.reserve = this.weapon.reserve;
    this.reloadLeft = 0;
    this.fireLeft = 0;
    this.speed = 0;
    [this.playerX, this.playerY, this.playerAngle] = layout.player;
    this.bots = Array.from({ length: this.botConfig.count }, (_, index) => {
      const spawn = layout.bots[index % layout.bots.length];
      return {
        x: spawn[0],
        y: spawn[1],
        hp: Math.max(this.weapon.damageBody * 2, 50),
        attackLeft: this.botConfig.attackInterval * (0.45 + index * 0.12),
        attacks: 0,
      };
    });
    if (emitEvent) this.events.push({ type: "roundReset" });
  }

  private movePlayer(dx: number, dy: number): void {
    const layout = this.layout();
    const nextX = clamp(this.playerX + dx, 0.035, 0.965);
    if (!pointBlocked(nextX, this.playerY, 0.025, layout)) this.playerX = nextX;
    const nextY = clamp(this.playerY + dy, 0.055, 0.945);
    if (!pointBlocked(this.playerX, nextY, 0.025, layout)) this.playerY = nextY;
  }

  private moveBot(bot: MutableBot, dx: number, dy: number): void {
    const layout = this.layout();
    const nextX = clamp(bot.x + dx, 0.035, 0.965);
    if (!pointBlocked(nextX, bot.y, 0.022, layout)) bot.x = nextX;
    const nextY = clamp(bot.y + dy, 0.055, 0.945);
    if (!pointBlocked(bot.x, nextY, 0.022, layout)) bot.y = nextY;
  }

  private startReload(): void {
    if (
      this.reloadLeft === 0 &&
      this.ammo < this.weapon.magSize &&
      this.reserve > 0
    ) {
      this.reloadLeft = this.weapon.reloadTime;
    }
  }

  private finishReload(): void {
    const loaded = Math.min(this.weapon.magSize - this.ammo, this.reserve);
    this.ammo += loaded;
    this.reserve -= loaded;
  }

  private fire(): void {
    if (this.reloadLeft > 0 || this.fireLeft > 0) return;
    if (this.ammo <= 0) {
      this.startReload();
      return;
    }
    this.ammo--;
    this.fireLeft = this.weapon.fireInterval;

    const dx = Math.cos(this.playerAngle);
    const dy = Math.sin(this.playerAngle);
    let bestIndex = -1;
    let bestProjection = Number.POSITIVE_INFINITY;
    let bestMiss = Number.POSITIVE_INFINITY;
    const layout = this.layout();
    for (let index = 0; index < this.bots.length; index++) {
      const bot = this.bots[index];
      if (bot.hp <= 0) continue;
      const bx = bot.x - this.playerX;
      const by = bot.y - this.playerY;
      const projection = bx * dx + by * dy;
      if (projection <= 0 || projection >= bestProjection) continue;
      const miss = Math.abs(bx * dy - by * dx);
      if (miss > 0.05) continue;
      if (segmentBlocked(this.playerX, this.playerY, bot.x, bot.y, layout, 0.005)) continue;
      bestIndex = index;
      bestProjection = projection;
      bestMiss = miss;
    }
    if (bestIndex < 0) return;

    const bot = this.bots[bestIndex];
    const headshot = bestMiss < 0.012;
    const damage = headshot ? this.weapon.damageHead : this.weapon.damageBody;
    bot.hp = Math.max(0, bot.hp - damage);
    this.events.push({
      type: "hit",
      bot: bestIndex,
      headshot,
      damage,
      fatal: bot.hp === 0,
    });
  }

  private tickBots(dt: number): void {
    const layout = this.layout();
    for (const bot of this.bots) {
      if (bot.hp <= 0 || !this.alive) continue;
      const dx = this.playerX - bot.x;
      const dy = this.playerY - bot.y;
      const distance = Math.sqrt(dx * dx + dy * dy);
      const visible = !segmentBlocked(bot.x, bot.y, this.playerX, this.playerY, layout, 0.004);
      if (distance > 0.28 || !visible) {
        const step = (this.botConfig.speed / 190) * 0.075 * dt;
        const inverse = distance > 1e-6 ? 1 / distance : 0;
        this.moveBot(bot, dx * inverse * step, dy * inverse * step);
      }

      bot.attackLeft -= dt;
      if (visible && distance <= 0.38 && bot.attackLeft <= 0) {
        const span = Math.max(0, this.botConfig.damageMax - this.botConfig.damageMin);
        const damage =
          this.botConfig.damageMin +
          (span > 0 ? (bot.attacks * 3 + this.mapIndex + 1) % (span + 1) : 0);
        bot.attacks++;
        bot.attackLeft += this.botConfig.attackInterval;
        this.hp = Math.max(0, this.hp - damage);
        this.events.push({ type: "playerDamaged", amount: damage, hp: this.hp });
        if (this.hp === 0) {
          this.alive = false;
          this.events.push({ type: "playerDied" });
        }
      }
    }
  }
}
