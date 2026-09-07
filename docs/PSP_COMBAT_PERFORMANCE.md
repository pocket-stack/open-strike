# PSP combat stalls — 2026-09-07

Manual play of `9af4cee` exposed combat windows at 40.87–48.87 fps despite
about 0.3 ms of actor CPU work. The prior animation sweep and movement/fire
route did not establish full combat acceptance: the former skipped the
simulation and the latter never killed an opponent.

## Causes and changes

- Hit/kill/round transitions updated Solid signals and variable-size HUD
  text. A repeatable real-combat run produced 35 CPU frames above 25 ms
  after startup, with a worst warm frame of 41.406 ms. Scores, reserve,
  phase titles and countdowns now use fixed text cells. Paint updates are
  precompiled into the framework's property batch; damage/feed fades use
  native tweens. HUD event subscriptions are removed when the HUD unmounts.
- After GPU synchronization, an unconditional `sceDisplayWaitVblankStart`
  discarded an available refresh when execution had already entered a new
  blanking interval. The host now compares display vcounts and uses
  `sceDisplayWaitVblank` for an interval it has not presented in. The same
  vcount always waits for a new start, retaining at most one game tick and
  buffer swap per refresh. GPU synchronization still precedes the swap.
- Terminal/exact animation poses no longer blend two identical GE inputs.
  A second 8,340-byte index table selects the first vertex of each cached
  pair. Geometry, colors and the authored animation bake are unchanged;
  the shared pose cache remains 1,971,816 bytes.

The manual session also rendered busier views (up to about 8,971 average
world triangles in one window) than the controlled routes. `gpu_us` is the
wait at `sceGuSync`, not a measurement of total GPU execution time. These
results do not establish a locked 60 fps at every camera position or map.

## Physical A/B measurements

Connected PSP, firmware 6.61, CPU/bus 333/166 MHz, release `de_dust2`, full
simulation and normal JS rules, 480×272, 18 MiB heap cap. Each row contains
24 windows of 300 frames, with no framebuffer captures during measurement.
Input/aim and initial opponent positions are scripted; collision, traces,
AI, damage, deaths, reloading and round transitions execute normally.

| Ordinary-range combat, 7,200 frames each | Observed fps | Intervals >20 ms | Worst warm CPU frame |
| --- | ---: | ---: | ---: |
| Original character build + combat probe | 57.26 | 279 | 41.406 ms |
| Fixed HUD cells | 58.63 | 157 | 20.160 ms |
| Plus held-pose GE path | 58.75 | 143 | 19.670 ms |
| Plus native effects and batched paint | 58.83 | 132 | 20.688 ms |
| Plus display-vcount guard | 59.41 | 62 | 20.697 ms |

Every row produced the same 52 real hits, 18 kills, 77 damage events,
6 player deaths, 11 round resets and 870 reloading frames. The final row
used 74 already-open new blanking intervals and had no CPU work spikes
above 25 ms after startup.

A second A/B alternated opponents at 185 and 80 world units, with winning
and losing rounds at both distances. Only the wait policy differed between
these builds; both included the final HUD and held-pose changes.

| Mixed-distance combat, 7,200 frames each | Observed fps | Intervals >20 ms | Worst warm CPU frame |
| --- | ---: | ---: | ---: |
| Unconditional wait for a new start | 58.97 | 116 | 21.805 ms |
| Display-vcount guard | 59.43 | 58 | 21.624 ms |

Both produced 54 hits, 18 kills, 77 damage events, 6 player deaths,
11 resets and 870 reload frames. The corrected build reused 57 new blanking
intervals. Its heap high-water was 15,196,176 bytes, leaving 3,678,192 bytes
of untouched tail. Worst warm-window p95/p99 intervals were 16.729/33.441 ms.
Those are worst per-window percentiles, not pooled percentiles.

Occasional missed refreshes remain. The benchmark synchronously writes
logs every 300 frames, so these cadence results include measurement I/O;
the restored manual build has neither benchmark logging nor scripted input.
Cold startup is excluded only from the warm CPU maxima/spike count, not
from the overall fps or long-interval totals.

## Verification and remaining acceptance

- Eight core/character Rust tests passed.
- PSP/Vita TypeScript and capability checks and the Symbian guest build passed.
- Ten PPSSPP character scenarios passed, including the held final Death pose.
- Spawn/walk/fire regression captured 60/60 frames and matched five updated
  goldens. The image changes are confined to fixed HUD number cells; the
  central gameplay image remained pixel-identical. Emulator timing is not
  used for performance claims.
- A release build without instrumentation or injected input was restored on
  the physical PSP; the normal menu-based Memory Stick package was rebuilt.

The exact busy viewpoint from manual play still needs a fresh human check.
Character PR #17 remains Draft and separate from merged upstream PR #16.
The animation-only report remains available in
[PSP_CHARACTER_ACCEPTANCE.md](PSP_CHARACTER_ACCEPTANCE.md).

## Reproduce

```sh
POCKETJS_ARENA_BYTES=18874368 OPENSTRIKE_COOKED_MAPS=dist/maps \
  bun scripts/hw.ts -r --combat-bench --daemon

# Ordinary manual build: no synchronous benchmark I/O or scripted control.
OPENSTRIKE_PSP_AUTOSTART=de_dust2 POCKETJS_ARENA_BYTES=18874368 \
  bun scripts/hw.ts -r --cooked-maps dist/maps --daemon

bun scripts/psp.ts -r --cooked-maps dist/maps --package
```

Use only one USB host session. Archive its JSONL before resetting, because
boot truncates it. `--combat-bench` now alternates ordinary/close range;
`--character-bench` is a separate animation-only probe and cannot be combined.

Local receipts are in `out/combat-20260907/`: `user-session.jsonl`, staged
`combat-*.jsonl` and PRX hashes, `mixed-waitstart.jsonl`, `mixed-final.jsonl`,
`mixed-final-full.jsonl`, `comparison.json`, `receipt.json`, and build/test
logs. Cooked maps, device captures, binaries and packages remain local.
