# OpenStrike patrol officer

Original low-poly US patrol-police character, authored in Blender for this
repository under its MIT license. Navy uniform, peaked cap, shield badges,
body camera, duty belt, radio, holster and two-handed service carbine. No
third-party character, rig, texture or animation is used.

![Blender officer preview](preview.png)

`officer.blend` is the editable source with weighted armature and seven named
actions. `build.py` reproduces it, its packed-atlas `officer.glb`, the handheld
`officer.opch`, the studio `preview.png` and the measured `receipt.json`:

```sh
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup \
  --python-exit-code 1 --python assets/characters/police/build.py
cargo run --locked -p openstrike -- --script character --size 480x272 \
  --screenshot out/character
bun scripts/character-psp.ts
```

The desktop preview needs no BSP/WAD assets. PSP capture uses the existing
local `dist/maps` cooked maps, or `OPENSTRIKE_COOKED_MAPS`. Those maps and
map-bearing EBOOT packages remain local/ignored.

## Runtime contract and budget

- 1,390 triangles, 847 unique handheld vertices, 17 bones, one opaque 64×64
  palette material. Desktop glTF splits normals/UVs into 2,634 vertices.
- Actions: Idle, Walk, Run, Fire, Reload, Hit, Death. Hosts resolve glTF names;
  simulation never depends on the exporter’s alphabetical clip indices.
- Handhelds sample 16-bit quantized positions and interpolate at presentation
  rate. Idle is baked at 6 Hz, Run/Fire at 24 Hz, other clips at 12 Hz. There
  is no on-device armature solver, glTF parser, texture allocation or per-frame
  character heap allocation. The full binary stays below 512 KiB.
- PSP uses u16 indexed triangles, one draw per visible actor and camera-frustum
  culling. Only 847 vertices are sampled per actor. The shared index upload is
  8,340 bytes; each visible actor uploads 13,552 bytes into the retained frame
  pool. Both fit its 64 KiB allocation limit. GPU reads finish before reuse.
- Vita/Symbian reuse the same sampled poses and retain their existing triangle
  submission APIs; their reusable conversion buffers expand the index stream.
- The 60 Hz Blender comparison measures each clip, including between bake
  keys, and rejects position error above 2 game units. The actor is 70 units
  tall. The receipt includes support-sole height for locomotion.

Bot shots, including misses, start Fire and originate from the carried
carbine muzzle. Each bot reloads its 12-shot magazine for two seconds and
cannot shoot during that interval. Hit and Death interrupt presentation;
death retains the simulation's existing world-space fall. The first-person
rifle and player input contract are unchanged.

## PSP acceptance

```sh
# Ordinary playable build, menu and local eight-map package.
bun scripts/psp.ts -r --cooked-maps dist/maps --package

# Real gameplay timing, including simulation (when the PSP is available).
OPENSTRIKE_COOKED_MAPS=dist/maps bun scripts/hw.ts -r --bench --daemon

# Render stress: one, three, then six actors; 30 seconds per count.
OPENSTRIKE_COOKED_MAPS=dist/maps bun scripts/hw.ts -r --character-bench --daemon
```

The stress mode is a separate feature that stages poses in the BSP scene and
omits gameplay simulation. Its records carry `actor_probe: true`; it must be
paired with ordinary gameplay measurements. The bench now records actor
count/cost, observed frame cadence, p95/p99 frame interval, late frames and
arena high-water. `avg_tris` is world geometry; actor triangles are reported
separately. `gpu_us` remains the wait at synchronization, not total GPU time.

The target is a 16.67 ms frame budget at 333 MHz. Physical PSP acceptance
requires sustained gameplay plus the 1/3/6-actor sweep, stable arena use,
visual inspection and fresh movement/fire input. PPSSPP captures prove
rendering and liveness; emulator timings cannot establish hardware smoothness.
Capture builds exit after their requested frames: rebuild the normal or bench
binary before loading on hardware.

Current evidence is in `out/character` and `out/character-psp`; physical PSP
performance is pending because the connected device is in use by pocket-map.
This character work stays separate from upstream-update PR #16.
