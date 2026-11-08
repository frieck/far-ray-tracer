<p align="center">
  <img src="src-tauri/icons/128x128@2x.png" width="128" height="128" alt="Far Ray Tracer icon: a glass sphere splitting a beam of light into a spectrum">
</p>

<h1 align="center">Far Ray Tracer</h1>

A ray tracer that renders `.far` scenes and shows the image as it builds up.
It began as a C++ program written in 2008 for a computer science final project on
KD-Tree acceleration. This version is a Rust port with a better renderer and a new
desktop app.

The app opens a scene and renders it at the best quality. A **Show evolution** option,
off by default, shows how the program got there, in three stages:

| Stage | What it is |
| --- | --- |
| **2008** | The original program, pixel by pixel, bugs included |
| **Fixed** | The same algorithm with its bugs removed: one ray per pixel and hard shadows |
| **Enhanced** | Adds antialiasing, soft shadows, realistic glass and indirect light |

A draggable divider compares any stage with an earlier one, so the gain from fixing
bugs and the gain from new techniques can be seen separately.

## Requirements

- [Rust](https://rustup.rs), tested with 1.92
- [Node.js](https://nodejs.org), tested with 24
- The [Tauri system dependencies](https://tauri.app/start/prerequisites/) on Linux
- A C++ compiler, only to run the validation against the original engine

## Running the app

Install the dependencies once:

```bash
npm install
```

Start the app with live reload:

```bash
npm run tauri dev
```

Build the app bundle:

```bash
npm run tauri build
```

On macOS the result is `target/release/bundle/macos/Far Ray Tracer.app`. The same scripts
work with Yarn.

Quality sets the passes, which are samples per pixel: Draft is 16, Good is 64, High is 256
and Custom takes any number up to 4096. Resolution offers common display sizes up to 8K,
and Custom takes any size up to 8192 pixels per side. The original image plane is 4:3, so
other proportions show more of the scene along one axis instead of stretching it.

Example scenes are in **File > Open Example**. Shortcuts: `Cmd/Ctrl+O` opens a scene,
`Cmd/Ctrl+R` renders, `Cmd/Ctrl+S` saves a PNG and `Esc` cancels.

## Command line

```bash
cargo run --release -p far-cli -- render assets/scenes/cube.far -o cube.png --enhanced --samples 64 --depth 5
```

Without `--fixed` or `--enhanced` it renders the 2008 stage. Run `far` with no arguments to
see all options. `far compare <a> <b>` reports how two images differ.

## Project layout

This is a standard Tauri project, with the engine in its own Rust crates.

| Path | Contents |
| --- | --- |
| `src` | Interface (React and TypeScript, built with Vite) |
| `src-tauri` | Desktop app (Tauri 2) |
| `crates/far-core` | Engine: scene loading, KD-Tree and the renderers |
| `crates/far-cli` | Command line |
| `assets/scenes` | Scenes, plus images rendered in 2008 |
| `assets/scene-data` | Textures and meshes used by the scenes |
| `reference/cpp` | Original C++ engine, used as the validation baseline |
| `tools/far_gen.py` | Creates `.far` scenes from Python |

In `far-core`, `engine.rs` holds the 2008 stage. `enhanced.rs` holds the corrected tracer,
which runs both Fixed and Enhanced.

## Validation

The 2008 stage is checked against the original engine, built from `reference/cpp` with
the fixes it needs to run on 64-bit systems and a native JSON scene loader:

```bash
npm run validate
```

The script renders the historical corpus in `reference/scenes.txt` with both engines and compares every pixel. On Apple
Silicon all 18 scenes match exactly, with the same ray counts. It skips
`assets/scenes/debug` and ignores the first row and column, which the original program
never writes.

The other checks run with:

```bash
npm test
```

```bash
npm run typecheck
```

The tests also pin down what Fixed is. On scenes that no bug touches, Fixed must match
the 2008 stage to within one color level. Enhanced has no reference image: its results
are covered by those tests and judged by eye, not compared with an external renderer.

## What each stage changes

Fixed removes these bugs:

| Defect in 2008 | Cause | Fix |
| --- | --- | --- |
| Black patches on meshes | Smoothing counted a face only for its first vertex, leaving others with NaN normals | Per-corner normals with welded vertices and a 60 degree crease angle |
| Wrong reflections on meshes | Barycentric coordinates were overwritten by later rays | Coordinates are kept at the hit point |
| Glass does not bend light on exit | The same index was used in both directions | Correct index per direction, with total internal reflection |
| Glass darkens with lighter colors | Absorption used the color itself, on every crossing | Absorption from the inverse color, only inside the material |
| Light leaks into closed spheres | Rays inside a sphere heading away from its center missed it | The test now only runs where it was meant to |
| Black dots on textured spheres | `acos` outside [-1, 1] gave NaN | The value is clamped |
| Stretched non-square textures | The width was used for both axes | Each axis uses its own size |
| Distorted glossy reflection | The sampling basis was not orthogonal, and deeper levels were too bright | Orthonormal basis and a true average |
| The KD-Tree changes the image | The two paths shaded differently | One shading path; the KD-Tree only changes the speed |
| Rays pass through some walls | A triangle lying exactly on a split plane rounded to just outside both children | The triangle-box test allows a tiny margin |

Enhanced then adds:

- Antialiasing with a tent filter
- Soft shadows from area lights
- Light passing through glass, and Fresnel reflection
- Indirect light (global illumination). Scenes lit only by rectangles, sphere lights and
  the environment get full path tracing: light bounces at full strength until Russian
  roulette ends the path, so a white room is as bright as it should be. A rebound is
  capped at 10 to keep rare paths from leaving white dots. A 2008 light does not fall
  off with distance, so a scene that has one keeps two bounces at 45%, capped at 1.5,
  to keep from blowing out.
  All three can be set by hand: Advanced > Custom bounces in the app, or
  `--indirect-strength`, `--bounces` and `--indirect-clamp` (0 for no limit) on the
  command line.
- Optional caustics: light focused by glass and mirrors, from a photon map
- Material weights that no longer return more light than they receive
- A soft shoulder instead of a hard cut at white

Enhanced builds the image in passes, one sample per pixel each. Pixel position, the point
on the light and the reflection direction all vary per sample, so a single mechanism gives
antialiasing, soft shadows and glossy reflection.

Caustics are off by default. They work with glass and mirror objects, except that a light
inside an object's bounding sphere, such as one above a large mirror floor, sends no photons
at it. `assets/scenes/caustics.far` shows the effect.

| Preset | Photons | Radius | Look |
| --- | ---: | ---: | --- |
| Soft | 2 million | 0.20 | Smooth, with blurred edges |
| Balanced | 6 million | 0.12 | The default |
| Sharp | 16 million | 0.07 | Crisp edges, more memory |

Custom exposes the photons, the lookup radius and the strength. Noise depends on how many
photons fall inside the radius, so more photons allow a smaller radius at about the same
render time. Strength 1 is the physically correct amount; higher values also amplify noise.

After a render, the app reports what the photon pass did: photons emitted and stored,
tracing time, memory, lookups and photons per lookup. The presets give about 240 photons
per lookup on `primitives.far`, and 43 was visibly blotchy.

When the photon pass stores photons, a **Photon map** icon appears at the bottom right
of the image, while the render continues. It shows the raw photon deposits projected
through the camera, including hidden surfaces, with brightness normalized for inspection.
This point view is capped at 1024 pixels on its longest side. Switching back to the full
render is immediate and does not restart either pass. Saving still exports the rendered image.

Fixed leaves material weights alone on purpose, since that changes the look of a scene
instead of removing an error. As a result `rhino` and `cube` are overexposed in Fixed:
without the old absorption, their `diffuse=1, refraction=1` materials return twice the light.

All stages keep one choice from the original: only one side of a plane receives diffuse
light. Scene B relies on this for its black mirror floor.

## Performance

Measured on an Apple M4 Max with 16 cores at 800 x 600, depth 3, in the 2008 stage:

| Scene | C++ `-O2`, 1 thread | Rust, 1 thread | Rust, 16 threads |
| --- | ---: | ---: | ---: |
| `rhino` render | 152 ms | 161 ms | 13 ms |
| `rhino` KD-Tree build | 2782 ms | 347 ms | 324 ms |
| `scene-b` render | 425 ms | 404 ms | 52 ms |
| `showcase-1` render | 4912 ms | 4401 ms | 401 ms |

The KD-Tree build always runs in parallel. Classic mode uses the same tree as the original;
Fixed and Enhanced use one with the split-plane fix above, so each is built on first use.

Fixed runs at about the same speed as the 2008 stage. Enhanced at 64 samples, depth 5, takes
about 3 to 4 seconds on `cube`, `rhino` and `scene-b`.

## Scenes

The app and command line use one format: `.far`, readable JSON.
`tools/far_gen.py` can create scenes from Python. The format holds every primitive. `assets/scenes/primitives.far` is a complete example:

```json
{
  "lights": [{ "position": [-5, 11, 3], "radius": 0.6, "color": [1, 1, 1] }],
  "objects": [
    { "type": "sphere", "center": [0, 0, 5], "radius": 1 },
    { "type": "box", "center": [-4, 0, 5], "size": [2, 2, 2], "rotation": [0, 30, 0] },
    { "type": "cylinder", "center": [4, 0, 5], "radius": 1, "height": 3 },
    { "type": "tube", "center": [0, 3, 8], "radius": 1.5, "inner_radius": 1, "height": 2 },
    { "type": "cone", "center": [-4, 3, 8], "radius": 1, "height": 2 },
    {
      "type": "torus",
      "center": [4, 3, 8],
      "major_radius": 1.5,
      "minor_radius": 0.5,
      "material": { "color": [0.9, 0.7, 0.2], "reflection": 0.2 }
    },
    { "type": "plane", "points": [[-16, -2, -6], [16, -2, -6], [16, -2, 26], [-16, -2, 26]] },
    { "type": "mesh", "file": "knot1.3ds", "position": [0, 0, 5], "size": 3 }
  ]
}
```

- Solids are centered on `center` with Y as their axis. `rotation` is in degrees around X,
  then Y, then Z.
- `material` is optional and accepts `color`, `diffuse`, `specular`, `reflection`,
  `refraction`, `ior`, `glossy`, `texture`, `scale_u` and `scale_v`.
- Box, cylinder, tube, cone and torus are intersected exactly, so curves stay smooth at any
  resolution.

A `plane` has four corners and ends at them. For a floor that reaches the horizon, use
an infinite `ground` instead:

```json
{ "type": "ground", "position": [0, -2, 0], "normal": [0, 1, 0], "material": { "color": [0.5, 0.5, 0.5] } }
```

- The plane passes through `position` and faces `normal`. Both default to a floor at
  height 0 facing up.
- It is solid from both sides. A texture repeats every 18 scene units, times `scale_u`
  and `scale_v`.
- It has no bounds, so it stays out of the KD-Tree and every ray tests it separately,
  which costs one dot product and one division. The 3D view draws it as a large square.
- Photons for caustics are not aimed at it, so a mirror ground does not focus light.
- Open File > Open Example > Infinite ground shows one under an HDR sky.

Open File > Open Example > Cornell box is the classic test of indirect light, built by
`tools/cornell_box.py`. The ceiling and the faces of the blocks that look away from the
panel receive only bounced light, and the blocks take a red or green tint from the walls
beside them. Turn indirect light off to see them go dark.

Rectangular lights use two perpendicular vectors for their full width and height:

```json
{
  "type": "rectangle",
  "position": [-3, 5, 6],
  "edge_u": [5, 0, 0],
  "edge_v": [0, 0, 3],
  "color": [1, 0.9, 0.78],
  "intensity": 9
}
```

`position` is the center. The emitting side faces `edge_u × edge_v`; this example
faces downward. Reverse one edge to flip the panel. `color` defaults to white and
`intensity` to 1. Intensity scales emitted radiance, so making a panel larger at the
same intensity also increases its total power.

In Enhanced mode, panels are sampled across their area for soft shadows, with
emission-angle and inverse-square distance weighting. They are visible directly,
in reflections and through glass, and emit photons when caustics are enabled.
The back side is opaque and dark. The existing material model is still used.
Fixed mode samples the center for hard shadows; Classic approximates each panel
with a legacy point light. Existing spherical lights keep their historical behavior.

Spherical lights are physical bulbs, unlike the legacy lights above, which light every
point equally at any distance:

```json
{ "type": "sphere", "position": [0, 3, 6], "radius": 0.05, "color": [1, 0.72, 0.4], "intensity": 22 }
```

In Enhanced mode they are visible to the camera, in reflections and through glass, light
the scene with inverse-square falloff and soft shadows, and use multiple importance
sampling with GGX reflections, like panels. Fixed lights from the center for hard shadows;
Classic treats each one as a legacy point light. They do not emit caustic photons yet.
Open `assets/scenes/christmas.far` for a scene lit by strings of them.

Open **File > Open Example > Rectangular lights** for a two-panel studio scene with
mirror and glass spheres. Enable Caustics to inspect the resulting photon map.

Depth of field is available in Enhanced mode under **Camera > Depth of field**.
**Pick focus in image** turns the cursor into a crosshair. Click a surface to enable
depth of field and set its focus distance; Esc cancels picking. Clicking the background
keeps the picker active. Glass and mirrors focus on their first surface.
A larger aperture increases defocus; focus distance is measured along the viewing
axis from the camera, in scene units. The aperture value is the lens diameter in
scene units, not an f-number. Opening a scene loads its camera settings:

```json
"camera": { "aperture": 1.2, "focus_distance": 11.7 }
```

The app offers depth-of-field presets. They change aperture and minimum render passes,
while keeping the selected focus distance:

| Preset | Aperture diameter | Minimum passes |
| --- | ---: | ---: |
| Subtle | 0.25 | 128 |
| Portrait | 0.60 | 256 |
| Strong | 1.20 | 512 |
| Custom | Adjustable | 1–4096 |

The actual pass count is the higher of the Quality setting and the depth-of-field
minimum; the panel shows that total. Custom starts from the current preset's values.
Opening a scene with a nonzero aperture selects Custom to preserve its authored camera.
Lens samples use a progressive distribution with a separate rotation per pixel to
spread samples across the aperture. Larger apertures generally need more passes.

This optional top-level field defaults to aperture 0 (pinhole) and focus distance 10.
With the default camera at `(0, 0, -5)`, distance 11.7 puts the focus plane at world
`z = 6.7`. Classic and Fixed always use the original pinhole camera. The photon map
preview also remains a sharp point projection for inspection.

### Moving the camera (Enhanced)

Drag the image to orbit around the objects, Shift-drag or right-drag to pan, and scroll to
move closer. While the camera moves, a real-time WebGL view shows the scene's shapes and
base colors, without textures, shadows or reflections, through the same lens as the render.
A scene opens in this 3D view and is ray traced only when you press **Render**; turn on
**Auto-render** to render again after every change, as soon as you let go of the camera.
If WebGL is unavailable, Auto-render stays on and a small ray traced image follows the
camera instead. **Camera > Zoom** changes the lens and **Reset camera** returns to the scene's view.
Camera moves last for the session; to keep one, add it to the scene:

```json
"camera": { "position": [6, 3, -4], "target": [0, 0, 6], "zoom": 1.2 }
```

`zoom` 1 is the original field of view. The defaults are the 2008 camera, position
`(0, 0, -5)` and target `(0, 0, 0)`. Classic and Fixed always keep that camera, so that
they stay comparable with the original. CLI overrides are `--camera X,Y,Z`,
`--target X,Y,Z` and `--zoom N`.

**File > Open Example > Depth of field** demonstrates objects at three distances.
Use High quality for smoother blur. CLI overrides are `--aperture` and
`--focus-distance`, for example:

```bash
cargo run --release -p far-cli -- render assets/scenes/depth-of-field.far -o dof.png --enhanced --samples 256 --aperture 1.2 --focus-distance 11.7
```

Rules for all scenes:

- The camera is fixed at (0, 0, -5), looking toward +z.
- Lights may be spheres or rectangular panels.
- The order of a plane's points defines its lit side.
- Textures and meshes are given by file name and are searched in the nearest `scene-data`
  folder, walking up from the scene file. The legacy `DadosCena` asset folder layout
  is also recognized. All scene files themselves must use JSON.
- Bounds normally grow to fit the scene. An optional `bounds: [[min_x, min_y, min_z],
  [max_x, max_y, max_z]]` preserves the original KD-Tree volume for historical scenes.

## Known limits

- Only macOS on Apple Silicon has been tested. Windows and Linux are unverified, including
  the translucent window.
- The C++ reference reads `.far` scenes containing spheres, planes and meshes; it reports
  unsupported modern primitives and lights explicitly. Its JSON loader uses vendored
  nlohmann/json 3.12.0. It keeps its original Portuguese comments and file
  names, since it is the historical baseline.
- The C++ reference needs `sips`, so `npm run validate` runs on macOS only.
- Only the last object of a `.3ds` file is loaded, as in the original.

## License

Copyright (c) 2026 [FARSystems](https://farsystems.com.br). Released under the MIT
license: see [LICENSE](LICENSE). Parts of the project come from third parties and are
listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

### GGX materials (Enhanced)

The **Materials** panel selects an object's material and applies Metal, Plastic,
Ceramic, Glass or Custom presets. Roughness spreads reflections; metallic blends
dielectric and metal response. Color controls the base color; IOR controls dielectric
Fresnel reflectance. Edits apply to this session's Enhanced renders. **Original scene
material** removes the override. Reopening a scene resets edits.

Author persistent materials in a `.far` object's `material`:
```json
{
  "color": [0.85, 0.52, 0.22],
  "surface": { "preset": "metal", "roughness": 0.35 }
}
```
The surface also accepts `metallic` (0–1), `ior` (1–3), and optional `color`
(three values in 0–1). Presets supply defaults; explicit values override them.
Open `assets/scenes/ggx-materials.far` to compare roughness 0.12, 0.35 and 0.7.

Opaque surfaces use isotropic, single-scattering GGX reflection, Smith masking,
Schlick Fresnel and visible-normal importance sampling, with multiple importance sampling for
rectangular emitters. This extends the existing integrator; it is not a replacement
with a fully physically based path tracer. Glass retains clear legacy transmission;
frosted glass and anisotropic brushed metal are not implemented. Higher roughness
can require more samples. Legacy materials without `surface` keep their behavior.
Classic and Fixed have no GGX, so they approximate these materials with a sharp mirror
reflection of the same head-on strength. The historical C++ reference rejects modern
surface materials explicitly.

### Bump and normal maps (Enhanced)

A material's `bump` adds surface detail, such as tile joints or hammered dents, by tilting
the shading normal; the geometry and its silhouette do not change.
```json
{
  "color": [1.0, 0.77, 0.36],
  "surface": { "preset": "metal", "roughness": 0.22 },
  "bump": { "map": "hammered-normal.png", "kind": "normal", "strength": 1 },
  "scale_u": 0.33, "scale_v": 0.5
}
```
- `kind` is `height` (the default) or `normal`.
  - A **height** map is grayscale, white is high, and `strength` is the relief depth in
    scene units.
  - A **normal** map is tangent-space with the OpenGL convention (green points up in the
    image), and `strength` multiplies its tilt; 1 uses it as drawn.
- The map follows the material's texture coordinates and `scale_u` / `scale_v`, so it lines
  up with the color texture, on spheres, solids, triangles and the infinite ground.
- Images live in the scene's `images` folder, like color textures.

The Materials panel shows the strength of a material that has a map; 0 turns it off.
Changing only the strength keeps a 2008 material as it is. Classic and Fixed ignore the
maps. Open `assets/scenes/bump-maps.far` for tiles (height map) and hammered metal (normal
map); `tools/bump_maps.py` builds its images.

### Displacement

A material's `displacement` moves the surface itself by a grayscale height map, so the
outline, the shadows and the contact with other objects show the relief, which a bump map
cannot do.
```json
{
  "color": [0.55, 0.52, 0.48],
  "displacement": { "map": "cobbles-height.png", "height": 0.12, "detail": 128 }
}
```
- `height` is how far white moves the surface out, in scene units; black stays in place.
- `detail` (default 128, 4 to 4096) is the number of divisions across the object. Higher
  shows smaller relief in the outline and costs memory.
- The map follows the material's texture coordinates and scale, like color and bump maps.

How it works:
- **Objects** (spheres, solids and meshes) become dense meshes when the scene loads. Each
  point moves along the surface normal by the map's height; points shared by two faces, at
  seams and box edges, move together so the surface does not tear. The meshes join the
  KD-Tree. Shading does not use the facets: each triangle remembers where it lies on the
  original shape, so color and normal come from the shape and the map at the exact point.
  All displaced objects together are limited to 4 million triangles; past that, detail is
  lowered evenly, with a warning.
- **The infinite ground** cannot become a mesh. Rays crossing the thin slab its relief can
  fill march over the map about half a texel at a time and refine the crossing, so its
  relief is exact to the map's resolution.

Displacement is geometry, so every stage sees it, although Classic and Fixed shade the
displaced meshes by their facets. Scenes over 50 000 primitives build the KD-Tree with a
sweep over primitive extents, and without the original's starting cost of 10 000, which
left them with an empty tree; smaller scenes build exactly as before. The material preview
displaces its ball at a lower detail. Open `assets/scenes/displacement.far`: a cobbled
ground, a displaced sphere and box, and the same map as a bump on a smaller sphere, whose
outline stays smooth. `tools/displacement_maps.py` builds it.

### Denoising (Enhanced)

**Denoise** is off by default, and the app suggests it when HDR lighting is active; the
command line uses `--denoise`. It runs once,
after the last pass, so the image sharpens up at the end of the render. Each pixel records
the variance of its samples and the color, normal and depth of the first surface it hit.
A non-local means filter then averages similar neighbors: a difference counts as detail
only if it exceeds the measured noise, and it never crosses a surface edge. Isolated
fireflies are replaced first. The filter needs at least 4 passes and leaves cancelled
renders untouched. It costs about 1 second at 1920x1080. Classic and Fixed never use it.

On `hdri-studio.far` at 64 passes it cuts isolated bright pixels from 365 to 35. Scenes
that are already clean gain nothing, and fine noise-like detail in reflections can soften.

### Bloom (Enhanced)

**Bloom** is off by default; the command line uses `--bloom`. Like light scattering in a
camera lens, it spreads light brighter than white into a soft halo, so bulbs, panels and
bright highlights glow. It runs after the last pass and after the denoiser, and keeps the
rest of the image unchanged. Each pass records how far the exposed radiance goes above
1.0; three blurs of that excess, a tight core, a soft glow and a faint wide haze, are
screened over the image. Halo sizes follow the image size, so a preview and a full
render look alike. Cancelled renders are left without it.

### HDR environment lighting (Enhanced)

**Environment → Import HDR…** opens a Radiance `.hdr` latitude-longitude panorama.
Intensity changes its brightness; Rotation turns it around the vertical axis.
**Show background** affects camera misses only: turning it off keeps lighting,
reflections and refraction. **Environment lighting** disables the entire environment;
**Remove environment** clears it. UI changes last for the current scene session.

For persistent settings, add this top-level field to a `.far` scene:
```json
"environment": {
  "path": "../environments/studio.hdr",
  "intensity": 1.0,
  "rotation": 0,
  "visible": false,
  "enabled": true
}
```
The panorama's center faces the camera (+Z), and it is not mirrored. Paths are relative
to the scene file (absolute paths also work). The only required
field is `path`; intensity defaults to 1, rotation to 0, and both switches to true.
Intensity accepts 0–100. Maps are limited to 16,777,216 pixels; Radiance HDR is the
supported input format, not EXR or an ordinary LDR panorama.

Open **File → Open Example → Hdri Studio** (or `assets/scenes/hdri-studio.far`) for a scene lit
only by the bundled environment. The synthetic studio map is generated by
`python3 tools/generate_studio_hdr.py` and requires no external downloads.

HDR radiance stays in floating point and is averaged before highlight compression,
so small bright sources retain their energy. Sampling uses separate latitude and
longitude distributions, weighted by luminance and texel solid angle. The weights
cover the bilinear filter footprint to avoid rare bright samples at texel edges,
with a small uniform mixture for full support. GGX uses MIS
between environment and reflection samples. Loading builds the sampling table
once; intensity and rotation edits reuse it. Classic and Fixed ignore environments,
and the historical C++ reference rejects them explicitly.

Environment lighting does not emit caustic photons in this version: photon mapping
still uses local sphere and rectangle lights. Hiding the background produces black,
not a transparent PNG.
