# Blender maps

**A closed convex mesh becomes one BSP brush.** The exporter applies object
transforms and evaluated modifiers, checks manifold edges and face planes,
then writes a Valve 220 `.map`. SDHLT produces a GoldSrc BSP v30 with embedded
textures, collision hulls, visibility and baked lighting. Pocket3D cooks that
BSP into the `.p3d` consumed by PSP. The desktop build can open the BSP.

```sh
# Requires Blender, Bun, CMake, a C++17 compiler and the normal Rust toolchain.
bun scripts/blender-map.ts --demo --stage

# Export an edited or new scene.
bun scripts/blender-map.ts --blend scene.blend --out out/my-map --stage

# Compile with an existing SDHLT installation.
bun scripts/blender-map.ts --blend scene.blend --sdhlt-dir /path/to/tools
```

The bootstrap pins `seedee/SDHLT` to the revision in `scripts/blender-map.ts`.
`scripts/sdhlt-portable.patch` adapts its C library declarations and string
logging for Clang, enables POSIX extent calculation on macOS, and bounds
32-bit compression shifts. The original extent and compression self-tests
remain enabled. C++17 and disabled strict aliasing preserve its transfer-data
representation. Source, binaries and build logs live in
ignored `.pocket/toolchains/`. `BLENDER` or `--blender` selects Blender.

Object custom properties:

| Property | Value | Result |
| --- | --- | --- |
| `bsp_role` | `world` | Structural solid; default for meshes |
| `bsp_role` | `detail` | Solid detail brush |
| `bsp_role` | `entity` | Point entity at the object's world position |
| `bsp_role` | `ignore` | Preview object; omitted from the map |
| `bsp_fit` | `true` | Fit the material image once across each brush face |
| `bsp_classname` | `info_player_start`, `light`, etc. | Point entity class |
| `bsp_angle`, `bsp__light`, etc. | String or number | Entity key without the `bsp_` prefix |

**One Blender metre equals 32 map units.** BSP coordinates keep Blender's Z-up
axes; Pocket3D converts them to Y-up on load. Spawn markers identify standing
hull centres, 36 units above the floor. Meshes must be closed and convex;
split concave rooms into floor, wall and ceiling brushes. Unsupported meshes
fail with the object's name. Lights and cameras without entity properties
are preview objects.

Materials use a base color or one image texture. Bake other shader graphs to
images before export. The exporter writes 64×64 WAD textures with palettes
and four mip levels. Texture names need 1–15 ASCII characters; `bsp_texture`
can override the Blender material name. `bsp_scale` controls world-aligned
mapping (default `0.5`, a one-metre tile). Arbitrary Blender UV mapping is not
exported. Names beginning with `{` use palette-index transparency. Sparse
glass uses a cutout pattern because the runtime has no translucent BSP pass.

The tool writes `.blend`, `.map`, `.wad`, `.bsp`, `.p3d`, logs and hash receipts
under ignored output. **Generated binaries and source video stay outside Git.**
`--stage` copies the cooked map into `dist/maps`; package that directory with
`bun scripts/psp.ts --release --package --cooked-maps dist/maps`.

## WWDC24 route

The reference is Apple's [WWDC24 keynote](https://developer.apple.com/videos/play/wwdc2024/101/),
the iPadOS-to-macOS transition around **50:40–50:57**. The authored scene joins
the upper atrium bridge, opposing stair flights and ground-floor presentation
hall shown in that sequence. Dimensions and connections between camera cuts
are inferred. Gardens, rear galleries, cover and spawn positions are authored
for play; this is not an architectural survey of the entire Apple Park.

`scripts/build-parkour.py` creates the editable scene, packed material images
and a route through the stairs. The route validator uses the game's standing
hull, gravity, friction and step law in both directions:

```sh
cargo run -p openstrike-core --example verify_map_route -- \
  out/parkour/wwdc24-parkour.p3d out/parkour/route.txt

"$BLENDER" --background --factory-startup --python-exit-code 1 \
  --python test/blender-map-fixture.py -- out/blender-fixture
```

The generated route passes in both directions with four valid spawns. The
current cooked map is about 1.3 MB. Frame rate on PSP requires device validation;
a desktop screenshot and collision traversal do not establish that result.
