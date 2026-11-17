# TODO

Rendering techniques still to add. Effort is a rough guess: **low** is a day or less,
**medium** a few days, **high** a week or more.

## Suggested order

Visual gain for the effort:

1. ~~Rough and colored glass, then spectral dispersion~~ (done, with every material below).
2. ~~ACES/AgX tone mapping and shaped bokeh~~ (done).
3. ~~Volumes with light shafts~~ (done).
4. ~~glTF and OBJ import~~ (done).

## Materials

- [x] **Rough glass**: GGX transmission, for frosted glass. Removes a limit the README
  states. Medium.
- [x] **Colored glass**: absorption that grows with thickness (Beer's law), like a green
  bottle. Low.
- [x] **Spectral dispersion**: glass splits light into a rainbow, as in the app icon.
  Needs rendering by wavelength. Medium.
- [x] **Subsurface scattering**: light that enters and spreads inside skin, wax, marble or
  jade. High.
- [x] **Clearcoat**: a varnish layer over the base, for car paint or varnished wood.
  Low to medium.
- [x] **Anisotropic GGX**: brushed metal. Medium.
- [x] **Sheen**: velvet and cloth. Medium.
- [x] **Full PBR maps**: roughness, metallic and emission from textures, and alpha cutouts
  for leaves and grilles. Low to medium.

## Light and atmosphere

- [x] **Environment caustics on the CPU**: emit photons from HDRI maps and the physical sky,
  through bounded reflective and refractive objects onto diffuse surfaces.

- [x] **Volumes**: fog, smoke and light shafts through the air, in boxes and spheres of even
  density. Medium to high.
- [x] **Uneven volumes**: smoke and clouds from a density grid or noise, and fog that thins with
  height. Medium.
- [x] **Physical sun and sky**: a sky computed from the time of day, without an HDR image.
  Medium.
- [x] **Spotlights and IES profiles**: a flashlight beam or a real fixture's light. Low.
- [x] **EXR and LDR environments**: OpenEXR panoramas, and PNG or JPEG ones as sRGB.
- [x] **Mesh lights**: glowing triangles and spheres sampled as lights, with MIS, on the CPU
  and the GPU.
- [ ] **Glowing solids as lights, and photons from mesh lights**: boxes, cylinders, tubes, cones
  and tori are not sampled yet, and caustics take no light from glowing surfaces. Low to medium.
- [ ] **A light tree**: mesh lights are drawn by their power alone, wherever the point is; in a
  scene with many glowing surfaces far apart, drawing by distance too would cut noise. Medium.
- [ ] **Caustics on an infinite mirror ground**: photons are not aimed at it, and scenes with
  one skip environment photons and keep path-traced environment light. Medium.

## Camera and image

- [x] **Shaped bokeh**: hexagonal or other aperture shapes. Low.
- [x] **Vignetting and chromatic aberration**: natural vignetting and lateral aberration.
- [x] **Film tone mapping (ACES or AgX)**: natural colors in bright light. Low.
- [x] **White balance**: a color temperature for the camera, with tint.
- [x] **Physical exposure and film**: ISO and shutter, or one Brightness in stops; grain and
  halation.
- [x] **Focal length, sensor size and f-number**: as in V-Ray and Blender, with a scene unit.
- [x] **Film looks and camera presets**: `.cube` LUTs, five bundled looks, and camera presets
  that set the sensor, film, lens flaws, curve and look together.
- [ ] **Motion blur**: needs moving objects in the scene format. Medium.
- [x] **Raw light and render passes**: the linear light as EXR, with the denoised light,
  albedo, normal and depth, from the app and the command line.
- [ ] **Light groups and direct and indirect passes** (after the outliner, where groups are
  made): the light of each lamp (or group), of
  the environment and of glowing materials kept apart, so the app can change their strength
  and color after rendering, as Blender's light groups, and save them to the EXR. Every place
  the CPU and GPU integrators add light would carry a color per group, and memory grows by
  one image of light per group. Changing a material's color still renders again. High.

## Quality and speed

- [x] **Low-discrepancy sampling**: Owen-scrambled Sobol points for each decision of a
  camera sample. Low to medium.
- [x] **Adaptive sampling**: stop sampling pixels that have converged. In the command line
  only (`--noise-threshold`): it saves little in these scenes, see the README. A better
  error estimate (two half buffers, as in Cycles) could make it pay. Medium.
- [ ] **Light cache for indirect light**: V-Ray's default; indirect light read from a
  precomputed map instead of long paths, removing most of its noise. Slightly approximate.
  High.
- [x] **Light portals**: windows that guide the environment's light indoors, CPU and GPU.
- [ ] **Path guiding**: learn where light comes from and send bounces there, as Cycles does.
  Measure first with Techniques > Window room whether portals leave enough noise to need it;
  it would help the CPU only, as in Cycles. High.
- [ ] **GPU noise in the Window room**: the GPU is noisier than the CPU there and some 2%
  darker, with or without a portal; find which sampling differs. Medium.
- [x] **AI denoiser (Intel Open Image Denoise)**: the one in Blender and V-Ray; a large jump
  in quality at few samples. Low to medium, but needs a native library.
- [ ] **Bidirectional path tracing or progressive photon mapping**: hard caustics, such as
  light seen through glass. High.
- [ ] **GPU rendering (wgpu)**: path tracing in compute shaders (WGSL), one code for Metal,
  DirectX 12 and Vulkan, with the CPU engine as the reference and the fallback. Very high.
  After 3.0, in this order:
  1. ~~A flattened BVH on the CPU~~ (done for Fixed and Enhanced, in `bvh.rs`). Left: the
     2008 stage still walks its own KD-Tree; since it no longer has to match the original, it
     can move to the BVH, and the KD-Tree and the code kept only for that match (`--exact`,
     the ordered random numbers, `npm run validate`) can go.
  2. ~~The scene in flat buffers the GPU reads~~ (done in `crates/far-gpu`: the BVH, every
     shape, grounds, materials, color textures, lights and the environment's sampling tables).
  3. The GPU for previews first, camera movement and the material ball. Done so far: a compute
     shader path tracer behind `far render --gpu` that follows the Enhanced integrator (every
     shape, 2008, GGX with MIS, glass with absorption, textures, light picking, environment
     sampling, depth of field), used by the app through a Device setting, with live frames,
     denoise and bloom, and the material ball (done).
  4. ~~Parity, feature by feature, each compared with the CPU~~ (done: every example renders
     on the GPU and matches the CPU within noise. Fog, smoke, bump and normal maps,
     roughness, metallic, emission and alpha maps, clear coat, sheen, anisotropy, spotlight
     cones, IES profiles, dispersion, displacement, subsurface and caustics, whose photons the
     CPU still traces.) Left on the CPU: more than eight volumes, and the tracing of caustic
     photons, which is quick there; after a GPU render the Photon map panel does not report
     lookups, which the card does not count.
  5. ~~Final renders on the GPU, as a Device setting~~ (done: Auto falls back to the CPU for
     scenes using what the GPU lacks, and says so; regions, and longer renders continued from
     the last one). Left: adaptive sampling on the GPU.
  6. Speed: the shader is specialized per scene (done, up to a third faster on simple scenes).
     Next, measure how much divergence costs in mixed scenes (glass, metal, volumes and
     subsurface side by side, against the same scene with one material), and only if it costs
     most: a wavefront path tracer, with kernels per stage (intersect, shade, shadow, volume)
     and paths sorted by material, as Cycles does; it needs more than wgpu's 8 storage buffers
     per stage, or buffers merged. Low for the measurement, high for the wavefront.
     And split `trace.wgsl` (over 2,200 lines, one kernel) into files by subject: sampling
     and Sobol, BVH and shapes, materials, lights, volumes, caustics, joined before compiling,
     since WGSL has no includes. The compiled shader stays the same. Low.
     ~~Low-discrepancy samples on the GPU~~ (done: the CPU's scrambled Sobol points).
     And the displaced ground's march, most of a showcase render on the GPU (5 s against 1.2 s
     without it): a coarser first pass over a max-height mip chain would skip most steps.
     And gathering caustics where glass focuses light: thousands of photons in one cell keep a
     group of pixels waiting, so the GPU gains 7 to 9 times there against 15 to 35 without
     caustics; a coarser map for dense cells, or photons sorted by cell, could help.

  Risks: two renderers to keep equal; no recursion in shaders; Windows stops a GPU command
  after 2 seconds, so work goes in small slices; CI has no GPU, only a software renderer;
  GPU and CPU agree within noise, not bit for bit. No hardware ray tracing at first: its
  support in wgpu is experimental.

## Geometry and scenes

- [x] **OBJ and glTF import**: models from any 3D program, with their materials. Medium.
- [x] **Outliner, first part**: the scene's camera, environment, lights and objects in a list,
  with names, selection from the list or the image, its outline on the image, and hiding.
- [x] **Editing objects and lights**: place, rotation and sizes, and a light's color,
  intensity, size and cone, in the Object tab, saved into the scene file.
- [x] **Move gizmo**: arrows along X, Y and Z and a ring across the view, on the image.
- [x] **Undo and redo** of scene and settings edits.
- [x] **Lights panel and color temperature**: every light with on/off, solo, color or kelvin,
  and intensity.
- [ ] **More for lights**: aiming a light at the selected object, changing a light's kind,
  loading an IES file, lights invisible to the camera. Medium.
- [x] **Rotate and scale gizmos**, with R and S, which also aim spotlights and turn area lights.
- [x] **Adding, duplicating and deleting** objects and lights.
- [ ] **Outliner, next**: a model's parts as children; renaming; editing an added object's
  material before saving. Medium.
- [ ] **Instancing**: many copies of an object for almost no memory. Medium.
- [ ] **Subdivision surfaces**: smooth meshes from coarse ones. High.
- [ ] **CSG (boolean operations)**: would carve the material ball's medallion into the
  sphere, for one. High.
- [ ] **Displacement height in the material editor**: needs the mesh and the KD-Tree
  rebuilt on each change. Medium.
- [ ] **More of glTF**: texture coordinate sets after the first, its lights and cameras, and
  16-bit or HDR textures; lines, points, animation and skinning are ignored now. Low to
  medium.
- [ ] **Displaced objects past 4 million triangles**: detail is lowered evenly now, with a
  warning; adaptive tessellation by distance to the camera would keep it where it shows.
  Medium.
