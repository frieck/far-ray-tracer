# Changelog

What changed in each release of Far Ray Tracer. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/). Each change goes under **Unreleased** as it lands,
and moves under its version when that version is released.

## [Unreleased]

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

[Unreleased]: https://github.com/frieck/far-ray-tracer/compare/v3.0.0-beta.3...HEAD
[3.0.0-beta.3]: https://github.com/frieck/far-ray-tracer/compare/v3.0.0-beta.2...v3.0.0-beta.3
[3.0.0-beta.2]: https://github.com/frieck/far-ray-tracer/compare/v3.0.0-beta.1...v3.0.0-beta.2
[3.0.0-beta.1]: https://github.com/frieck/far-ray-tracer/releases/tag/v3.0.0-beta.1
