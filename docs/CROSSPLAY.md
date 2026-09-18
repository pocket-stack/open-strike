# Mac and PSP crossplay

**Classic 1 vs 1 runs through a Companion on the Mac.** The PSP uses PocketJS's
USB offload worker through PSPLINK. The Mac game uses PocketJS's native offload
worker. The guest forwards bounded input batches and snapshots; socket and
file operations run outside the game thread.

This transport requires a PSP tethered through PSPLINK. It does not enable
PSP Wi-Fi, ad-hoc play, public matchmaking or multiplayer character mods.
Frieren and Pikachu remain available in offline play.

## Run

Build the desktop UI for its own host contract and the PSP package:

```sh
bun run setup
bun scripts/build-ui.ts --target macos-app
bun scripts/psp.ts --release --package --cooked-maps dist/maps
cargo build --locked -p openstrike -p openstrike-companion
```

Provide the same map revision on both sides: a BSP for the desktop renderer,
and its cooked P3D for PSP and the Companion. The game compares collision,
hull topology, visibility and spawn sections, so texture resolution and
rendering tessellation can differ. A mismatch stops joining and appears on
the HUD; return to the menu and select the matching map.

Start `usbhostfs_pc` with the PSP release directory as its host0 root, then:

```sh
bun scripts/crossplay.ts \
  --map dist/maps/de_dust2.p3d \
  --usb-root crates/openstrike-psp/target/mipsel-sony-psp/release \
  --desktop /path/to/de_dust2.bsp
```

Launch `openstrike-psp.prx` through PSPLINK. Choose **CROSSPLAY**, then the same
map. The round begins when both players connect. A kill updates both scores;
the room respawns both players after three seconds. PSP controls match local
play. On Mac, use WASD, mouse aim/fire, Space and R.

Without `--desktop`, the launcher prints the command for starting the Mac
game. It writes a local pairing configuration under ignored `out/crossplay/`.
The TCP listener binds to loopback, authenticates a generated pairing key and
admits one connection per player slot. Restart the Mac game when restarting
the launcher, because the launcher creates a new key. Stop the launcher with
Ctrl-C. An ordinary Memory Stick launch without PSPLINK has no USB transport.

## Simulation and recovery

**The Companion advances the shared Rust simulation at 64 Hz.** Input cannot
choose elapsed time, position, health, ammunition or damage. The server uses
the same BSP hulls, movement law, spread, reload gate and hitscan traces as
local play. It resolves simultaneous shots before applying either death.
Client prediction replays unacknowledged movement after an authoritative
snapshot; remote actors interpolate toward received positions. There is no
server rewind for lag compensation.

Each peer keeps at most **128 input ticks**, sends at most **12 per exchange**,
and has one guest request in flight. Payloads stay within PocketJS's
**2,500-character / 4,096-byte** record budgets. The framework mailbox has
8 fixed slots. Snapshots report both received and executed input sequences.
Clients keep unexecuted input for prediction but send only input the server
has not received, so USB round trips do not fill each batch with duplicates.
Duplicate inputs are ignored; gaps and stale epochs are
rejected. Two seconds without a valid snapshot freeze local gameplay and
start a fresh join. The authority expires inactive peers after two seconds
and stops the round. A map mismatch requires a map change instead of retrying.

## Validation

```sh
cargo test --locked -p openstrike-core -p openstrike-companion
cargo check --locked -p openstrike-core --no-default-features --features libm
bun run typecheck

# Builds must precede this test; supply matching local map files.
bun scripts/check-crossplay.ts map.bsp map.p3d
# On a test map with opposing visible spawns, also require a kill and respawn:
bun scripts/check-crossplay.ts map.bsp map.p3d --combat
```

The last command runs the real Mac QuickJS bundle and native worker against a
second peer using the PSP USB mailbox protocol. It records replies, movement,
HUD capture and a JSON result under ignored `out/validation/`. It does not
simulate USB bus timing or validate a physical PSP. Hardware acceptance must
check input latency, frame pacing, mutual damage, respawn and unplug/reconnect.
