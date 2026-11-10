# Changelog

What changed in each release of Far Ray Tracer. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/). Each change goes under **Unreleased** as it lands,
and moves under its version when that version is released.

## [Unreleased]

### Added

- **Save Scene** and **Save Scene As…**: write the scene with the app's edits (camera,
  environment, materials and render settings), which reopen as saved. Paths are rewritten when
  it is saved to another folder.
- A gallery of the example scenes in the README, rendered by `tools/screenshots.sh`.
- A warning before unsaved edits are lost, when opening another scene, closing the window or
  quitting: Save, Don't Save or Cancel. The save button shows a dot while there are edits.
- Scene files carry a format version (`"format": 3`); a newer format than the app knows is
  refused with a message to update.

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

[Unreleased]: https://github.com/frieck/far-ray-tracer/compare/v3.0.0-beta.4...HEAD
[3.0.0-beta.4]: https://github.com/frieck/far-ray-tracer/compare/v3.0.0-beta.3...v3.0.0-beta.4
[3.0.0-beta.3]: https://github.com/frieck/far-ray-tracer/compare/v3.0.0-beta.2...v3.0.0-beta.3
[3.0.0-beta.2]: https://github.com/frieck/far-ray-tracer/compare/v3.0.0-beta.1...v3.0.0-beta.2
[3.0.0-beta.1]: https://github.com/frieck/far-ray-tracer/releases/tag/v3.0.0-beta.1
