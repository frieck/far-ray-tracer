# Far Ray Tracer

A ray tracer that renders `.frt` scenes and shows the image as it builds up.
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
cargo run --release -p far-cli -- render assets/scenes/cube.frt -o cube.png --enhanced --samples 64 --depth 5
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
| `tools/frt_gen.py` | Creates `.frt` scenes from Python |

In `far-core`, `engine.rs` holds the 2008 stage. `enhanced.rs` holds the corrected tracer,
which runs both Fixed and Enhanced.

## Validation

The 2008 stage is checked against the original engine, built from `reference/cpp` with
only the fixes it needs to run on 64-bit systems:

```bash
npm run validate
```

The script renders each scene with both engines and compares every pixel. On Apple
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

Enhanced then adds:

- Antialiasing with a tent filter
- Soft shadows from area lights
- Light passing through glass, and Fresnel reflection
- Indirect light
- Optional caustics: light focused by glass and mirrors, from a photon map
- Material weights that no longer return more light than they receive
- A soft shoulder instead of a hard cut at white

Enhanced builds the image in passes, one sample per pixel each. Pixel position, the point
on the light and the reflection direction all vary per sample, so a single mechanism gives
antialiasing, soft shadows and glossy reflection.

Caustics are off by default. They work with glass and mirror objects, except that a light
inside an object's bounding sphere, such as one above a large mirror floor, sends no photons
at it. `assets/scenes/caustics.frt` shows the effect.

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
per lookup on `primitives.far`, and 43 was visibly blotchy. **Caustics only** shows the
photon map's light alone.

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

The KD-Tree build always runs in parallel and produces the same tree as the original.

Fixed runs at about the same speed as the 2008 stage. Enhanced at 64 samples, depth 5, takes
about 3 to 4 seconds on `cube`, `rhino` and `scene-b`.

## Scenes

The app opens two formats. `.frt` is the binary format of the 2008 program, and
`tools/frt_gen.py` can create those files. `.far` is a JSON format that holds every
primitive. `assets/scenes/primitives.far` is a complete example:

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

Rules for all scenes:

- The camera is fixed at (0, 0, -5), looking toward +z.
- Only spheres can be lights.
- The order of a plane's points defines its lit side.
- Textures and meshes are given by file name and are searched in the nearest `scene-data`
  folder, walking up from the scene file. The folder layout of the original 2008
  distribution is found too, so an untouched copy of it still opens.
- In `.frt` scenes with the KD-Tree on, geometry must fit in the original fixed bounds:
  x -14 to 14, y -6 to 8 and z -6 to 30. `.far` scenes have no such limit.

## Known limits

- Only macOS on Apple Silicon has been tested. Windows and Linux are unverified, including
  the translucent window.
- The C++ reference in `reference/cpp` keeps its original Portuguese comments and file
  names, since it is the historical baseline.
- The C++ reference needs `sips`, so `npm run validate` runs on macOS only.
- Only the last object of a `.3ds` file is loaded, as in the original.

## License

Copyright (c) 2026 [FARSystems](https://farsystems.com.br). Released under the MIT
license: see [LICENSE](LICENSE). Parts of the project come from third parties and are
listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
