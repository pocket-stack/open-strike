import { describe, expect, test } from "bun:test";
import {
  SYMBIAN_COMPAT_MAPS,
  SymbianCompatSimulation,
  type SymbianCompatInput,
} from "../game/symbian-compat-sim.ts";
import {
  SYMBIAN_COMPAT_DYNAMIC_VIEWPORT,
  deriveSymbianCompatManifest,
  SYMBIAN_COMPAT_ENTRY,
  SYMBIAN_COMPAT_LIVE_VIEWPORT,
  SYMBIAN_COMPAT_OUTPUT,
} from "../scripts/symbian-manifest.ts";

const IDLE: SymbianCompatInput = {
  forward: 0,
  turn: 0,
  fire: false,
  reload: false,
};

const LIVE = { ...IDLE, forward: 1 };
const FIRE = { ...IDLE, fire: true };

function enterLive(simulation: SymbianCompatSimulation, bots = 3): void {
  simulation.queueConfigureBots({
    count: bots,
    speed: 190,
    attackInterval: 1.4,
    damageMin: 8,
    damageMax: 14,
  });
  simulation.flushCommands();
  simulation.loadMap(0);
  simulation.queueSetPhase("live");
  simulation.flushCommands();
}

describe("OpenStrike Nokia E7 2D compatibility simulation", () => {
  test("derives the compatibility package from canonical pocket.json", async () => {
    const canonical = await Bun.file(
      new URL("../pocket.json", import.meta.url),
    ).json() as Record<string, any>;
    const derived = deriveSymbianCompatManifest(canonical) as Record<string, any>;

    expect(derived).not.toBe(canonical);
    expect(derived.version).toBe(canonical.version);
    expect(canonical.app.viewport).not.toHaveProperty("dynamic");
    expect(derived.app.viewport.fixed).toEqual(canonical.app.viewport.fixed);
    expect(derived.app.viewport.dynamic).toEqual(SYMBIAN_COMPAT_DYNAMIC_VIEWPORT);
    expect(canonical.engine.capabilities.enhances).not.toContain(
      SYMBIAN_COMPAT_LIVE_VIEWPORT,
    );
    expect(derived.engine.capabilities.enhances).toEqual([
      ...canonical.engine.capabilities.enhances,
      SYMBIAN_COMPAT_LIVE_VIEWPORT,
    ]);
    expect(derived.engine.capabilities.enhances).not.toContain("input.touch");
    expect(derived).toMatchObject({
      id: `${canonical.id}.e7-compat`,
      name: "open-strike-e7-compat",
      title: "OpenStrike E7 2D Compat",
      app: {
        entry: SYMBIAN_COMPAT_ENTRY,
        output: SYMBIAN_COMPAT_OUTPUT,
        framework: "solid",
      },
    });
    expect(canonical.app.entry).toBe("game/openstrike.tsx");
  });

  test("keeps the native strike command ordering and completes a combat round", () => {
    const simulation = new SymbianCompatSimulation();
    expect(simulation.snapshot().phase).toBe("menu");
    expect(SYMBIAN_COMPAT_MAPS).toHaveLength(8);

    enterLive(simulation, 1);
    const before = simulation.scene().player;
    simulation.tick(LIVE, 0.1);
    expect(simulation.scene().player.y).toBeLessThan(before.y);

    // The first spawn is centered on the player's initial sightline, so this
    // is a deterministic precision hit rather than a random test fixture.
    simulation.tick(FIRE, 0.1);
    expect(simulation.drainEvents()).toEqual([
      {
        type: "hit",
        bot: 0,
        headshot: true,
        damage: 100,
        fatal: true,
      },
    ]);
    expect(simulation.snapshot().aliveBots).toBe(0);

    simulation.queueAddWin();
    simulation.queueSetPhase("won");
    // Guest intents do not mutate facts until the host-side flush.
    expect(simulation.snapshot().phase).toBe("live");
    simulation.flushCommands();
    expect(simulation.snapshot()).toMatchObject({ phase: "won", wins: 1 });

    simulation.queueResetRound();
    simulation.flushCommands();
    expect(simulation.snapshot()).toMatchObject({
      phase: "starting",
      hp: 100,
      aliveBots: 1,
      wins: 1,
    });
    expect(simulation.drainEvents()).toEqual([{ type: "roundReset" }]);
  });

  test("supports magazine depletion and an edge-triggered reload", () => {
    const simulation = new SymbianCompatSimulation();
    simulation.queueConfigureWeapon({
      magSize: 2,
      reserve: 3,
      fireInterval: 0.03,
      reloadTime: 0.2,
      damageBody: 1,
      damageHead: 1,
    });
    simulation.flushCommands();
    enterLive(simulation, 0);

    simulation.tick(FIRE, 0.1);
    simulation.tick(FIRE, 0.1);
    expect(simulation.snapshot()).toMatchObject({ ammo: 0, reserve: 3 });

    simulation.tick({ ...IDLE, reload: true }, 0.05);
    expect(simulation.snapshot().reloading).toBe(true);
    simulation.tick(IDLE, 0.1);
    simulation.tick(IDLE, 0.1);
    expect(simulation.snapshot()).toMatchObject({
      ammo: 2,
      reserve: 1,
      reloading: false,
    });
  });

  test("bots pursue, attack, and emit the same facts consumed by rules and HUD", () => {
    const simulation = new SymbianCompatSimulation();
    enterLive(simulation, 1);

    const events = [];
    for (let step = 0; step < 120 && simulation.snapshot().hp === 100; step++) {
      simulation.tick(IDLE, 0.1);
      events.push(...simulation.drainEvents());
    }
    expect(simulation.snapshot().hp).toBeLessThan(100);
    expect(events.some((event) => event.type === "playerDamaged")).toBe(true);
  });

  test("replays an input sequence deterministically", () => {
    const left = new SymbianCompatSimulation();
    const right = new SymbianCompatSimulation();
    enterLive(left, 3);
    enterLive(right, 3);

    const sequence: SymbianCompatInput[] = [
      LIVE,
      LIVE,
      { ...IDLE, turn: -1 },
      { ...IDLE, turn: -1, fire: true },
      IDLE,
      { ...IDLE, reload: true },
    ];
    for (let cycle = 0; cycle < 20; cycle++) {
      const input = sequence[cycle % sequence.length];
      left.tick(input, 1 / 30);
      right.tick(input, 1 / 30);
    }

    expect(right.snapshot()).toEqual(left.snapshot());
    expect(right.scene()).toEqual(left.scene());
    expect(right.drainEvents()).toEqual(left.drainEvents());
  });
});
