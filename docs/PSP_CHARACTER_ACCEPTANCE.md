# PSP officer hardware acceptance — 2026-09-07

**Follow-up:** manual combat on this revision exposed 41–49 fps windows and
JS/HUD transition stalls. The animation-only sweep below remains valid but
does not constitute full combat acceptance. See the subsequent
[combat investigation and fixes](PSP_COMBAT_PERFORMANCE.md).

The original CPU-baked officer path dropped to about 30 fps with six actors.
The PSP now uses its GE vertex morphing hardware; actor CPU cost fell from
about 8.24 ms to 0.41 ms for the same 1,390 triangles and 847 unique vertices
per actor. The Blender Walk/Run actions now plant and roll the feet using
offline IK. Death is a complete 1.25-second authored recoil, collapse, fall
and settle, with relaxed arms and carbine. The host no longer rotates the
whole actor to fake a fall. Continuous quaternion keys remove the wrist flip
found during intermediate-pose validation.

## Hardware and scope

- Physical PSP, PSPLINK over USB, firmware 6.61; measured CPU/bus 333/166 MHz.
- Release build, `de_dust2`, normal PocketJS rules/HUD, 480×272 output.
- Heap deliberately capped to 18 MiB (`POCKETJS_ARENA_BYTES=18874368`).
- Final pose asset SHA-256: `05fbcfc3af8d1d23996a10c4036df82e94710c4b80033f9360e07f28fdb8b8e1`.
- Binary is 504,818 bytes; the immutable shared GE pose cache is 1,971,816
  bytes plus 8,340 bytes of indices. Per-frame actor vertices are not uploaded.
- This is a heap-cap test on the connected device, not a separate PSP-1000
  result. Other maps have not received the same sustained hardware sweep.

## Final actor sweep

Two complete 1/3/6-actor cycles, 36 windows of 300 frames: **10,800 frames**.
The camera/world configuration stays fixed; all seven actions are exercised.
This feature stages animation and omits gameplay simulation. Full simulation
is measured separately below. No screenshot command ran inside this final
measurement interval.

| Visible actors | Frames | Observed fps | Actor CPU, mean | Worst window p95 | Worst window p99 |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 3,600 | 59.22 | 0.167 ms | 16.695 ms | 33.373 ms |
| 3 | 3,600 | 59.02 | 0.262 ms | 16.698 ms | 33.409 ms |
| 6 | 3,600 | 58.91 | 0.412 ms | 16.709 ms | 53.309 ms |

Arena high-water stayed at **15,166,368 bytes (14.46 MiB)** after warmup,
leaving **3,708,000 bytes (3.54 MiB)** of unused tail. The second complete
cycle did not increase it. These are allocator high-water/tail measurements,
not total process memory or an exhaustive minimum-heap search.

There are still occasional long frames during JS/HUD transitions. The worst
window p99 above must not be described as the pooled p99 of the whole run.
The average is computed from measured frame cadence, including stalls;
`avg_work_us` excludes presentation waits. `gpu_us` measures the wait at
`sceGuSync`, not total GPU execution time. The synchronous benchmark logger
also contributes to cadence; this is not an uninstrumented retail trace.

## Gameplay and interaction

Ordinary full simulation ran for **11,700 frames** at **59.70 fps** while
stationary, with three officers and the same 18 MiB heap limit. High-water
was 15,166,272 bytes. This interval received no physical pad input.

A separate software-injected input route ran on the physical PSP with full
simulation: walking out of spawn, turning, shooting and reloading. Its first
**900 frames** averaged **59.03 fps**, with worst window p95 **16.824 ms** and
p99 **33.373 ms**. Native telemetry recorded 433 moving frames, 105 look
frames, 145 reloading frames and ammo changing between 30 and 12; arena
high-water was 15,234,304 bytes. The screenshot was taken after that timing
interval. The script remains in `scripted-gameplay-input.json`.

The user subsequently played this optimized build and confirmed significant
combat stalls. The software route above fired without actually killing bots;
it missed the full hit/death/round-transition path. That coverage gap is now
addressed by a dedicated full-simulation combat probe and separate report.
Automated input proves the physical-device runtime path, not acceptance of
the physical controls or subjective feel.
Character PR #17 remains separate from merged upstream PR #16 and stays
Draft until that review is complete.

## Rendering and regression evidence

- Blender 5.1.2 regenerated the editable `.blend`, GLB and quantized asset.
  Every clip was compared at 60 Hz; maximum position error is below two game
  units. The final corpse is grounded and below 30 units high; gait endpoints
  close within 0.5 units. CPU sampling and reference GE blend arithmetic
  agree within 0.001 units in tests.
- 8 Rust behavior/asset tests; 12 Vita host tests; 16 app/tooling tests;
  PSP/Vita TypeScript and capability contracts; Symbian ARMv6 release build.
- All 7 GLB actions rendered in Pocket3D. Walk/Run/Death have full 24 Hz
  preview sequences, including the final held death pose.
- 9 PPSSPP correctness scenarios passed (all actions, three and six actors).
  Spawn/walk/fire regression produced 60/60 frames and five byte-exact goldens.
  Emulator timings are not part of the hardware fps results.
- 22 physical PSP framebuffer captures show gait phases, carbine poses and
  corpses after the timing sweep. These captures do not measure frame rate.

## Reproduction and receipts

Use `scripts/hw.ts` to own one PSPLINK host session. It serves the release
output directory as `host0:` and prints its actual pspsh port. Start each
mode separately and archive `OpenStrike-bench.jsonl` before resetting the
application, because it truncates the log on boot.

```sh
POCKETJS_ARENA_BYTES=18874368 OPENSTRIKE_COOKED_MAPS=dist/maps \
  bun scripts/hw.ts -r --character-bench --daemon

POCKETJS_ARENA_BYTES=18874368 OPENSTRIKE_COOKED_MAPS=dist/maps \
  bun scripts/hw.ts -r --bench --daemon

bun scripts/psp.ts -r --cooked-maps dist/maps --package
```

Local authoritative evidence is under `out/hardware-20260907/`:

- `receipt.json`, `stress-final.jsonl` and `stress-final-prx.sha256`;
- `stress-initial.jsonl` and `stress-batch.jsonl` preserve the earlier results;
- `gameplay-final-idle.jsonl`, `gameplay-final-prx.sha256`;
- `scripted-gameplay-final.jsonl`, `scripted-gameplay-input.json` and PRX hash;
- `physical-motion/receipt.json`, framebuffer PNGs and contact sheet;
- `motion-final/`, `walk-final.gif`, `death-final.gif` and build/test logs.

`out/character-psp/receipt.json` holds the independent emulator capture
receipt. Map data, framebuffer evidence containing those maps and map-bearing
packages remain local/ignored.
