# Changelog

What changed in each release of Far Ray Tracer. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/). Each change goes under **Unreleased** as it lands,
and moves under its version when that version is released.

## [Unreleased]

### Added

- Caustics from HDRI maps and the physical sun and sky in Enhanced rendering, through
  both glass and reflective objects. Includes an Environment caustics example. Scenes with
  infinite reflective grounds still use path tracing for the environment.

- Rendering on the graphics card, in the app (**System > Device**: Auto, GPU or CPU) and on the
  command line (`far render … --gpu`). Auto takes the card for scenes it renders as the CPU
  does. The image builds up live and is denoised and bloomed by the CPU's filters. One compute
  shader runs on Metal, DirectX 12 and Vulkan, 15 to 35 times faster than 16 CPU cores, and
  gives the same image as the CPU's Enhanced mode within noise. It covers every shape, the 2008
  material, GGX, glass with absorption, color textures, every kind of light, HDR environments
  and the sun and sky, fog and smoke with light shafts, bump and normal maps, roughness,
  metallic, emission and alpha maps, clear coat, sheen and anisotropy, spotlights and IES
  profiles, dispersion in glass, displacement of objects and of the ground, subsurface
  scattering, depth of field, and caustics, gathering on the card the photons the CPU traces:
  every example scene renders on the GPU. More than eight volumes stay on the CPU, and Auto
  renders such scenes there. Samples come from scrambled Sobol points, as on the CPU,
for as little noise at few samples. Render regions and raising the passes work as on the CPU: a region
renders only its pixels, and a longer render continues from the last one. Software renderers
are not used. The shader is compiled for what
  each scene uses, up to a third faster on simple scenes. The material ball renders on the
  card too, 4 to 11 times faster.

- Environments read OpenEXR panoramas, as HDR ones, and PNG or JPEG panoramas as sRGB turned
  into linear light, in the app (**Light > Environment > Image**) and in scene files. The
  HDRI studio example comes in EXR, PNG and JPEG versions too.

- Mesh lights: in Enhanced rendering, on the CPU and the GPU, triangles and spheres that glow
  are sampled as lights (spheres within the cone they fill), with multiple importance sampling against GGX reflections, so small bright ones,
  such as the Still life's candle flame, light the scene with clean shadows instead of
  scattered dots. About a tenth of the error at the same samples on a floor lit by a small
  glowing panel. New example: Techniques > Glowing surfaces.

- The material editor sets a material's glow: **Materials > Emission** turns it on or off and
  sets its color and strength, shown on the material ball, saved with the scene, and glowing
  triangles light the scene as mesh lights.

- A camera much like V-Ray's physical camera, in the **Camera** tab and on the command line,
  for Enhanced rendering: exposure by ISO and shutter speed (`--iso`, `--shutter`), or one
  Brightness in stops that says which ISO and shutter a camera would pick; white balance and
  tint (`--white-balance`, `--tint`); vignetting and chromatic aberration (`--vignette`,
  `--chromatic-aberration`); and film grain, stronger at high ISO, and halation (`--grain`,
  `--halation`). Each changes after a render without rendering again, on the CPU and the GPU.

- **Light > Environment** shows the environment as a panorama, tone-mapped as renders show
  it, with a ring where the camera looks; dragging it sideways turns the environment.

- Images save as OpenEXR, in the app (**File > Export Image**) and on the command line
  (`-o image.exr`): the linear light of an Enhanced render before tone mapping and, when it
  denoised, the denoised light, albedo, normal and depth passes.

- Color looks from LUTs: **Camera > Film > LUT** grades the finished picture with a `.cube` file, as
  DaVinci Resolve and V-Ray's frame buffer do, with a strength. Five looks come with the
  program (Warm film, Teal and orange, Bleach bypass, Black and white, Faded print), made by
  `tools/generate_luts.py`, and any 1D or 3D `.cube` file loads. It changes without rendering
  again and is saved with the scene; on the command line, `--lut FILE` and `--lut-strength N`.

- An outliner, always in view above the settings as in Blender, with a divider to share the
  height: the scene's camera, environment, lights and objects, by the `name` the file gives
  or one made from their kind or file, with an eye to hide each one and a search by name.
  Hidden objects and lights render as if they were not there, in every stage and on the GPU,
  and are saved with the scene. Clicking an object in the image selects it, and its materials
  open in the Materials tab, which now names materials by their object (`Ball`, `teapot
  part 2`). The selection is outlined on the image, fainter where something hides it, and what
  the pointer is over is outlined in white: a smooth line at the screen's resolution, from an
  ID map the engine traces in milliseconds.

- Objects and lights can be edited: the **Object** tab shows the selection's position,
  rotation and sizes, and a light's color, intensity, size, direction and cone, as fields of
  its entry in the scene file. The engine reads the scene again with them, in some 70 ms for
  the Still life, so renders, the 3D view and the outline follow; **Save Scene** writes them
  into the file.

- A gizmo on the image for the selected object or light, in three modes as in Blender: move
  (arrows and a ring across the view), rotate (a ring per axis and one about the view) and
  scale (handles per axis and a ring for the whole), with G, R and S from the keyboard and
  Shift for steps of a tenth, or of 5 degrees. Turning aims spotlights and turns area lights. X, Y or Z locks a move to
  that axis, and with nothing moving starts one along it, and G a free one, as Blender's
  grab, placed with a click or Enter and called off with Esc. The quick 3D view follows the
  move at once, and placing it writes the edit.

- Undo and redo (⌘Z, ⇧⌘Z) of edits to objects, lights, materials, the environment and the
  settings: a drag or slider move is one step, orbiting the camera is not undone, and a text
  field still undoes its typing.

- A Lights panel at the top of the Light tab, as V-Ray's Light Lister: every light with an
  on/off box, Solo, its color and its intensity.

- Lights can take their color from a temperature in kelvin (`"temperature": 2700`), the color
  of a black body of the same brightness as white, from 1000 to 40000 K. The Object tab's
  **Color by** switches between a color and a temperature. An edit can now remove a field.

- Objects and lights can be added from the outliner's **+** (primitives, area, sphere and spot
  lights, where the camera looks), duplicated (⇧D moves the copy with the mouse) and deleted;
  Save Scene writes them into the file and opens it again.

- A plane shows its rotation, width and depth in the Object tab, from its corners, and 3DS
  meshes take a `rotation`, so both turn with the gizmo and from their fields.

- Light portals, as in V-Ray and Cycles: a `portal` in a window draws the environment's light
  through it, on the CPU and the GPU, with about 2.6 times less noise indoors (Techniques >
  Window room, a room lit by the sky through one window). The app adds, moves, turns and sizes
  them as area lights.

- A damaged glTF file whose embedded image lacks its type is reported as damaged instead of
  crashing the app: the glTF library's panic while reading it becomes the file's error.

- A Fog clouds example (Techniques): morning mist that billows in puffs among tree trunks, its
  fog's clouds on; `tools/atmosphere.py` builds it. Volume objects show the fog's settings,
  clouds and thinning with height included, in the Object tab.

- **Light > Atmosphere** edits the scene's fog: on or off, density, color, forward scatter,
  thinning with height and clouds; Save Scene writes it into the file. The outliner's Fog row
  opens it.

- A more compact panel: Render, Render after each change, Open, Save and Export share one
  row, and the tab's title, which the rail already shows, no longer takes one.

- Camera presets in **Camera > Camera**: Digital, 35 mm color film, Black-and-white film,
  Cinema camera, Phone and Vintage set the sensor, grain, halation, vignetting, chromatic
  aberration, tone mapping and look together, keeping the framing and the exposure. The
  Sensor list adds Super 35 and a phone's.

### Changed

- The camera has a focal length, a sensor and an f-number, as in V-Ray and Blender: **Camera >
  Lens** shows the zoom as a focal length in millimetres and the aperture as an f-number, which
  a scene's unit (10 cm by default) and the sensor (full frame) turn into the opening. Depth of
  field presets are f/5.6, f/2.8 and f/1.4, and in Physical exposure the f-number sets the
  light too. On the command line: `--focal-length`, `--sensor-width`, `--scene-unit` and
  `--f-number`. Scenes keep their aperture, so they look the same; the Bokeh and Depth of
  field examples use a 1 cm unit.
- Exposure moved from **Image** to **Camera > Exposure**. Scenes saved with the earlier
  Exposure multiplier open with the same light, as a Brightness.

- Fixed and Enhanced modes find what a ray hits with a bounding volume hierarchy (BVH) instead
  of the KD-Tree: scenes are ready 20 to 90 times sooner, and meshes render 10% to 35% faster.
  It is also the first step toward rendering on the GPU. The 2008 stage keeps its KD-Tree.

### Fixed

- The material ball read a material's roughness, metallic, emission and alpha maps from the
  wrong images.
- The Photon map panel now reports photons actually traced instead of the requested count,
  and explains empty maps without recommending more photons. Empty maps no longer cause
  unnecessary lookups or suppress lighting. This affected the beta releases and 3.0.0-rc.1.
- Rays no longer pass through a displaced ground where its map is black: the relief never goes
  below the base plane, so a ray that reaches it meets the ground there. Rounding let some
  through, which showed the sky below the horizon in dark patches.

## [3.0.0-rc.1] - 2026-11-09

### Added

- **Save Scene** and **Save Scene As…**: write the scene with the app's edits (camera,
  environment, materials and render settings), which reopen as saved. Paths are rewritten when
  it is saved to another folder.
- A gallery of the example scenes in the README, rendered by `tools/screenshots.sh`.
- A warning before unsaved edits are lost, when opening another scene, closing the window or
  quitting: Save, Don't Save or Cancel. The save button shows a dot while there are edits.
- Scene files carry a format version (`"format": 3`); a newer format than the app knows is
  refused with a message to update.
- A **Still life** example: a wine glass, a decanter, grapes in a glazed bowl, a pear, a copper
  cup and a candle on an oak table by a window at sunset. `tools/still_life.py` generates its
  meshes and textures.
- Separate depth limits for mirror-like reflections and for glass crossings in Enhanced mode,
  within Ray depth, so glassware can go deep while reflections between metals stop early:
  **Reflection depth** and **Glass depth** under Render > Sampling, saved with the scene, and
  `--glossy-depth N` and `--transmission-depth N` on the command line. The still life opens with
  depth 12, reflections 3 and glass 10.

### Changed

- **Rendering again reuses the last render** in the app:
  - changing exposure, tone mapping, or turning denoise or bloom off shows the image again at
    once, without rendering, even with **Render after each change** off; turning denoise or
    bloom on after a render without them, or changing exposure with bloom on, still renders;
  - raising the passes with nothing else changed continues from the last render, so 64 to 256
    passes costs 192, with the same image as a full 256-pass render;
  - a render stopped halfway by such a change, such as dragging the exposure while it runs,
    continues from where it stopped instead of starting over.
- The Standard tone mapping averages linear light before its curve in Enhanced mode, as the
  film curves and HDR scenes already did: scenes without an HDR environment keep the energy of
  bright paths and come out somewhat brighter where light bounces strongly, as in the Cornell
  box. The 2008 and Fixed stages are unchanged.
- **Faster rendering in Enhanced mode:**
  - scenes with many lights sample one per point, picked by the light it brings: the Christmas
    scene renders about 4.5 times faster, with less noise in the same time;
  - the KD-Tree grows deeper with the scene and splits across any side, so meshes render about
    a third faster;
  - a displaced ground renders about 40% faster.
- Caustics take at most 50 million photons (about 2 GB), and a model at most 20 million
  triangles; larger values give a clear error instead of running out of memory.
- Damaged scenes, models, IES files, `.3ds` meshes and HDR images are tested: each fails with an
  error, never a crash.

### Fixed

- Enhanced renders that slowed down sharply with ray depth. Every bounce traced every kind of
  light leaving the surface (bounced light, subsurface, reflection, refraction), so rays per
  pixel grew about threefold per level. What the camera sees, directly or through glass and
  mirrors, still traces them all; after the first diffuse bounce one is drawn at random,
  weighted by the light it carries. The image converges to the same result, and scenes with
  subsurface, coatings or fog render 2.5 to 4 times faster, with less noise in the same time.
  MIS between light sampling and reflection rays accounts for how often a reflection is drawn.
- Models of about 10 000 to 50 000 triangles vanished in Fixed and Enhanced renders, with
  everything else in the scene: the KD-Tree build split them into two empty boxes. Such
  scenes now also build in a fraction of a second instead of several.
- `Cmd/Ctrl+S` and the Save Scene menu items did nothing in 3.0.0-beta.3 and beta.4; they now
  save the scene. Exporting the image as PNG is `Cmd/Ctrl+E`, as in those versions.
- A crash while opening a scene or rendering a material preview shows an error instead of
  closing the app.
- Saved paths are right where folders are reached through a link (such as `/var` on macOS).
- The update dialog shows only the new and current versions, with Later and Install.

## [3.0.0-beta.4] - 2026-11-09

### Changed

- Beta-channel update test from 3.0.0-beta.3. Version bump only; no functional changes.

## [3.0.0-beta.3] - 2026-11-09

### Added

- **Light and atmosphere.**
  - Spotlights, with a cone and a soft edge, and IES profiles that give a lamp the measured
    light of a real fixture.
  - A physical sun and sky, from angles or from the time of day, place and date, in place of an
    HDR image.
  - Volumes that thin with height, such as ground fog, or billow from noise, such as smoke and
    clouds.
  - Examples: Spotlights, Sun and sky, Smoke.
- **Models and volumes.**
  - OBJ (with MTL materials) and glTF/GLB models, placed in scenes or opened directly.
  - Fog, smoke and light shafts in boxes and spheres.
  - Examples: Models, Light shafts.
- **Image and camera.**
  - Film tone mapping: AgX, AgX Punchy and ACES besides the original Standard curve.
  - Shaped bokeh from a diaphragm of 3 to 16 blades.
  - Example: Bokeh.
- **App.**
  - Updates itself from GitHub releases: it checks at startup and from **Check for Updates…**,
    and asks before installing.
  - A fuller About window, with the techniques, author, license and credits.

### Changed

- The settings panel is a render-app property editor:
  - an icon rail picks the group, and each group has sections that fold;
  - check boxes in section headers turn features on and off, and presets sit in the headers;
  - number fields are dragged or typed into;
  - an Advanced area in each section holds the rarer settings;
  - a search finds any setting.
- Typed values may go past a field's usual range, up to the setting's real limits.
- The render view has square corners.
- Release notes come from this changelog, and a release tag needs its section here.

## [3.0.0-beta.2] - 2026-11-09

### Added

- Intel Open Image Denoise, the denoiser of Blender and V-Ray, bundled with the app. Without
  it, the built-in filter is used.
- Render region: draw a rectangle on the image to render only that part.
- Owen-scrambled Sobol sampling, for less noise at the same number of samples.
- Adaptive sampling in the command line (`--noise-threshold`).

### Changed

- Denoise is on by default.

## [3.0.0-beta.1] - 2026-11-08

The first release of the Rust port, a desktop app for macOS (Apple Silicon and Intel) and
Windows.

### Added

- **The 2008 renderer**, ported from C++ and checked pixel by pixel against the original.
  The Fixed stage removes its bugs; the Enhanced stage adds:
  - antialiasing, soft shadows, glossy reflections and realistic glass;
  - indirect light (path tracing) and photon-mapped caustics;
  - depth of field.
- **The `.far` scene format**, with every primitive:
  - spheres, boxes, cylinders, tubes, cones and tori;
  - an infinite ground;
  - rectangular and spherical lights;
  - HDR environments.
- **Materials:**
  - GGX metals, plastics and ceramics;
  - rough, tinted and dispersive glass;
  - clear coat, sheen, anisotropy and subsurface scattering;
  - PBR maps (roughness, metallic, emission and alpha cutouts);
  - bump and normal maps, and displacement that moves the surface.
- **A material editor** with a shader-ball preview.
- **Evolution:** compares the 2008, Fixed and Enhanced stages side by side.
- **Example scenes:** a showcase, materials and techniques.

[Unreleased]: https://github.com/frieck/far-ray-tracer/compare/v3.0.0-rc.1...HEAD
[3.0.0-rc.1]: https://github.com/frieck/far-ray-tracer/compare/v3.0.0-beta.4...v3.0.0-rc.1
[3.0.0-beta.4]: https://github.com/frieck/far-ray-tracer/compare/v3.0.0-beta.3...v3.0.0-beta.4
[3.0.0-beta.3]: https://github.com/frieck/far-ray-tracer/compare/v3.0.0-beta.2...v3.0.0-beta.3
[3.0.0-beta.2]: https://github.com/frieck/far-ray-tracer/compare/v3.0.0-beta.1...v3.0.0-beta.2
[3.0.0-beta.1]: https://github.com/frieck/far-ray-tracer/releases/tag/v3.0.0-beta.1
