# OpenStrike for PS Vita

`openstrike-vita` packages the complete OpenStrike product as a native VPK:
the shared Rust simulation, cooked Pocket3D worlds, the normal PocketJS
QuickJS guest, and the unchanged Solid JSX rules/HUD bundle.

The Vita host renders Pocket3D first and the PocketJS HUD over the same
vita2d scene. PocketJS keeps its 480x272 logical viewport and expands every
coordinate exactly 2x, filling Vita's native 960x544 framebuffer without
letterboxing or cropping. Touch is intentionally not implemented yet.

## Toolchain and build

The pinned development setup is VitaSDK, `cargo-vita` 0.2.2 and Rust nightly
`2026-05-28` with `rust-src`. Vita3K is used for emulator E2E.

```sh
export VITASDK="$HOME/vitasdk"
export PATH="$VITASDK/bin:$HOME/.cargo/bin:$PATH"

OPENSTRIKE_MAPS=~/path/to/cs-maps bun scripts/vita.ts --release
# dist/vita/OpenStrike.vpk
```

`scripts/vita.ts` validates `pocket.json` against the Vita capability profile,
compiles the product JS/pak from its resolved plan, verifies the plan checksum,
and projects stable target, host ABI and viewport inputs for the Pocket host.
It then cooks every supplied BSP, stages the `.p3d` catalogue into the VPK,
and invokes the pinned Rust toolchain. Map and WAD data is not committed or
redistributed.

## Controls

| Input | Action |
| --- | --- |
| Left stick | Move |
| Right stick | Look |
| R | Fire |
| L | Jump |
| D-pad down | Reload |
| D-pad up | Walk |
| SELECT | Open or close the return-to-menu dialog |
| D-pad + Circle | Navigate and confirm menus |

No gameplay or menu flow depends on the touchscreen.

## Vita3K golden E2E

```sh
bun run test:e2e:vita
VITA_E2E_SPEC=spawn bun run test:e2e:vita
```

The driver builds capture VPKs, installs each into an isolated VitaFS, boots
the real QuickJS/input/simulation/render loop, waits for its `done` marker,
and terminates only the spawned emulator. Every selected capture must be a
960x544 RGBA frame whose pixels form exact 2x2 logical blocks and must match
`test/goldens-vita` byte-for-byte. A scene sidecar additionally requires
positive visible-face, world-triangle, submitted-triangle, and draw-call
counts from the native `pocket3d-vita` pass.

Current Vita3K macOS Vulkan builds do not expose a coherent presented color
buffer back to guest memory. The pixel oracle therefore uses PocketJS's
deterministic DrawList rasterizer for the HUD, while the native scene counters
guard the 3D pass. Capture builds park after `done`; this avoids a Vita3K GXM
teardown crash in `sceKernelExitProcess`. Production VPKs are unaffected.
