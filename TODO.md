# TODO

What is left to do, by area. Effort is a rough guess: **low** is a day or less, **medium** a
few days, **high** a week or more. What is done is in the CHANGELOG; a short record by area is at
the end.

## Suggested order

1. **More for lights** (Scene editing): finishes editing lights in the app. Medium.
2. **Outliner, next** (Scene editing): a model's parts, renaming, and editing an added
   object's material. Medium.
3. **Light groups** (Image): change each light's strength and color after rendering. High.
4. Then measure before choosing between **path guiding** and a **light cache** (Quality).

## Light and atmosphere

- [ ] **Photons from mesh lights**: caustics take no light from glowing triangles, spheres and
  solids. Low to medium.
- [ ] **Glowing solids drawn by the part that faces the point**: a solid is drawn over all its
  surface, so about half its samples land on the far side its near side hides; drawing by the
  solid angle each piece fills would help a torus most. Medium.
- [ ] **A light tree**: mesh lights are drawn by their power alone, wherever the point is; in a
  scene with many glowing surfaces far apart, drawing by distance too would cut noise. Medium.
- [ ] **Portals chosen by how big they look**: each portal now gets the same share of samples;
  choosing by the solid angle a portal fills from the shaded point, as V-Ray does, would help
  rooms with windows of different sizes. Low to medium.
- [ ] **Caustics on an infinite mirror ground**: photons are not aimed at it, and scenes with
  one skip environment photons and keep path-traced environment light. Medium.

## Scene editing

- [ ] **More for lights**: aiming a light at the selected object, changing a light's kind
  (sphere, spot, area), loading an IES file, and lights invisible to the camera, as in V-Ray.
  Medium.
- [ ] **Outliner, next**: a model's parts as children; renaming; editing an added object's
  material before the scene is saved and opened again. Medium.
- [ ] **Displacement height in the material editor**: edits now read the scene again, so the
  rebuilt mesh and tree come with them; the editor needs the fields and a preview that waits
  for the rebuild. Low to medium.
- [ ] **Scaling along an object's own axis in the 3D view**: while a scale handle is dragged,
  the quick view stretches along the world's axis; a turned box looks wrong until it is let
  go. Low.

## Camera and image

- [ ] **Light groups and direct and indirect passes**: the light of each lamp (or group made in
  the outliner), of the environment and of glowing materials kept apart, so the app can change
  their strength and color after rendering, as Blender's light groups and V-Ray's LightMix,
  and save them to the EXR. Every place the CPU and GPU integrators add light would carry a
  color per group, and memory grows by one image of light per group. Changing a material's
  color still renders again. High.

## Animation

- [ ] **Animation**: keyframes for the camera, objects and lights, and rendering a sequence of
  frames as PNG or EXR. High.
- [ ] **Motion blur**: once things move, the shutter time blurs them. Medium, after animation.

## Quality and speed

- [ ] **Path guiding**: learn where light comes from and send bounces there, as Cycles does
  with Open PGL. Measure first in Techniques > Window room whether portals leave enough noise
  to need it; it would help the CPU only, as in Cycles. High.
- [ ] **Light cache for indirect light**: V-Ray's default; indirect light read from a
  precomputed map instead of long paths, removing most of its noise. Slightly approximate.
  High.
- [ ] **Bidirectional path tracing or progressive photon mapping**: hard caustics, such as
  light seen through glass. High.
- [ ] **A better error estimate for adaptive sampling**: it is in the command line only
  (`--noise-threshold`) and saves little in these scenes; two half buffers, as in Cycles,
  could make it pay. Medium.

## GPU

The CPU engine stays the reference and the fallback; the GPU renders every example within
noise of it.

- [ ] **Adaptive sampling on the GPU**. Medium.
- [ ] **What is left on the CPU**: more than eight volumes, and tracing caustic photons, quick
  there; after a GPU render the Photon map panel does not report lookups, which the card does
  not count. Medium.
- [ ] **Split `trace.wgsl`** (about 2,400 lines, one kernel) into files by subject: sampling,
  BVH and shapes, materials, lights, volumes, caustics, joined before compiling, since WGSL has
  no includes. The compiled shader stays the same. Low.
- [ ] **A wide BVH on the GPU**: finding hits takes most of the card's time (see below), and
  in a SIMD group of 32 only a quarter to a half of the lanes do useful work while it walks
  the tree. A tree of 4 or 8 children with their boxes in the parent, compressed as in
  Ylitalo et al.'s CWBVH, reads fewer nodes and runs fewer steps per ray. Medium to high.
- [ ] **Shadow rays**: each hit casts one toward every light (up to four), one toward the
  environment and one toward glowing surfaces; in the measures below they cost as much as
  the path's own rays or more. Measure which of them pay for themselves. Low to measure.
- [ ] **The displaced ground's march**: most of a showcase render on the GPU (5 s against
  1.2 s without it); a coarser first pass over a max-height mip chain would skip most steps.
  Medium.
- [ ] **Gathering caustics where glass focuses light**: thousands of photons in one cell keep
  a group of pixels waiting, so the GPU gains 7 to 9 times there against 15 to 35 without
  caustics; a coarser map for dense cells, or photons sorted by cell, could help. Medium.

The GPU now ends paths by Russian roulette as PBRT does, by what a path carries after the
roulette's corrections; the CPU weighs by the last surface alone. Both are exact on average; in
the Window room the GPU keeps some 15% more noise than the CPU at the same passes, and the CPU's
choice there would cost 40% more time for it.

Divergence, measured on an M4 Max with the shader counting each pixel's segments and BVH
steps, and with every ray traced twice to time the search for hits. In SIMD groups of 32
(8 by 4 pixels), 40 to 75% of lanes are busy over a path's segments and 22 to 46% over its
tree walk. Finding hits takes 66 to 100% of the time in most examples (40% in Christmas,
15% in Fog clouds, where volumes take the rest), shadow rays alone 25 to 86%. Packing the
rays still going into full groups, as a wavefront would, leaves tree-walk use where it was
(28% against 28% in Christmas, 70% against 68% in the Window room): neighbors walk the tree
differently, not just for different lengths. A wall of columns whose materials alternate
every 4 pixels, against the same columns grouped by material: plastic and subsurface 20%
slower with the same work, all four materials 30% slower with 13% more work; in the examples
shading is 10 to 35% of the time. So a wavefront with paths sorted by material, high effort,
would win little here; the tree walk is where the time is.

Tried and measured without gain on an M4 Max: triangles split into what finding a hit reads
and what shading reads (48 bytes against 96 per triangle), and their normals worked out only
for the triangle a ray stops at. Render times stayed within noise in six scenes; the card's
caches seem to hold the 96-byte records well enough.

Risks: two renderers to keep equal; no recursion in shaders; Windows stops a GPU command
after 2 seconds, so work goes in small slices; CI has no GPU, only a software renderer; GPU and
CPU agree within noise, not bit for bit. No hardware ray tracing: its support in wgpu is
experimental.

## Geometry and models

- [ ] **Instancing**: many copies of an object for almost no memory. Medium.
- [ ] **Subdivision surfaces**: smooth meshes from coarse ones. High.
- [ ] **CSG (boolean operations)**: would carve the material ball's medallion into the
  sphere, for one. High.
- [ ] **More of glTF**: texture coordinate sets after the first, its lights and cameras, and
  16-bit or HDR textures; lines, points, animation and skinning are ignored now. Low to
  medium.
- [ ] **Displaced objects past 4 million triangles**: detail is lowered evenly now, with a
  warning; adaptive tessellation by distance to the camera would keep it where it shows.
  Medium.

## Code

- [ ] **The 2008 stage on the BVH**: it still walks its own KD-Tree. Since it no longer has to
  match the original pixel for pixel (see AGENTS.md), it can move to the BVH, and the KD-Tree
  and what is kept only for that match (`--exact`, the ordered random numbers, `npm run
  validate`) can go; Classic keeps working in the Evolution group. Medium.

## Done

- **Materials**: rough and colored glass, spectral dispersion, subsurface scattering, clear
  coat, anisotropic GGX, sheen, full PBR maps with alpha cutouts.
- **Light and atmosphere**: fog, smoke and light shafts, uneven volumes with clouds and height
  falloff, the physical sun and sky, spotlights and IES profiles, EXR and LDR environments,
  mesh lights with MIS (triangles, spheres and solids), environment caustics, light portals.
- **Camera and image**: shaped bokeh, vignetting and chromatic aberration, ACES and AgX, white
  balance, physical exposure, grain and halation, focal length, sensor and f-number, LUT looks
  and camera presets, EXR passes.
- **Scene editing**: the outliner with selection, outline and hiding; editing objects and
  lights; move, rotate and scale gizmos with G, R and S; undo and redo; the Lights panel with
  color temperature; adding, duplicating and deleting; fog in Light > Atmosphere.
- **Quality and speed**: Sobol sampling, adaptive sampling (command line), Open Image Denoise.
- **GPU**: a WGSL path tracer on Metal, DirectX 12 and Vulkan, the scene in flat buffers with
  a BVH, parity with the CPU for every example, previews, the material ball and final renders
  as a Device setting, regions and continued renders, a shader specialized per scene, and the
  CPU's Sobol points.
- **Geometry**: OBJ and glTF import.
