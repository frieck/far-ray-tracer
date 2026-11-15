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

## Camera and image

- [x] **Shaped bokeh**: hexagonal or other aperture shapes. Low.
- [ ] **Vignetting and chromatic aberration**. Low.
- [x] **Film tone mapping (ACES or AgX)**: natural colors in bright light. Low.
- [ ] **White balance**: a color temperature for the camera. Low.
- [ ] **Motion blur**: needs moving objects in the scene format. Medium.

## Quality and speed

- [x] **Low-discrepancy sampling**: Owen-scrambled Sobol points for each decision of a
  camera sample. Low to medium.
- [x] **Adaptive sampling**: stop sampling pixels that have converged. In the command line
  only (`--noise-threshold`): it saves little in these scenes, see the README. A better
  error estimate (two half buffers, as in Cycles) could make it pay. Medium.
- [ ] **Light cache for indirect light**: V-Ray's default; indirect light read from a
  precomputed map instead of long paths, removing most of its noise. Slightly approximate.
  High.
- [ ] **Path guiding**: learn where light comes from and send bounces there, as Cycles does.
  High.
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
     denoise and bloom. Left: the material ball.
  4. Parity, feature by feature, each compared with the CPU against a reference. Left: the
     displacement of the ground, subsurface. (Fog, smoke, bump and normal maps, roughness,
     metallic, emission and alpha maps, clear coat, sheen, anisotropy, spotlight cones, IES
     profiles and dispersion are done.)
  5. ~~Final renders on the GPU, as a Device setting~~ (done: Auto falls back to the CPU for
     scenes using what the GPU lacks, and says so). Left: continuing a longer render from the
     last one, regions and adaptive sampling on the GPU.
  6. Speed: the shader is specialized per scene (done, up to a third faster on simple scenes).
     Next, if measurements show divergence costs most in mixed scenes: a wavefront path
     tracer, with kernels per stage (intersect, shade, shadow, volume) and paths sorted by
     material, as Cycles does; it needs more than wgpu's 8 storage buffers per stage.
     Also: stratified or low-discrepancy samples on the GPU, which is grainier than the CPU at
     few samples because it draws from a hash.

  Risks: two renderers to keep equal; no recursion in shaders; Windows stops a GPU command
  after 2 seconds, so work goes in small slices; CI has no GPU, only a software renderer;
  GPU and CPU agree within noise, not bit for bit. No hardware ray tracing at first: its
  support in wgpu is experimental.

## Geometry and scenes

- [x] **OBJ and glTF import**: models from any 3D program, with their materials. Medium.
- [ ] **Instancing**: many copies of an object for almost no memory. Medium.
- [ ] **Subdivision surfaces**: smooth meshes from coarse ones. High.
- [ ] **CSG (boolean operations)**: would carve the material ball's medallion into the
  sphere, for one. High.
- [ ] **Displacement height in the material editor**: needs the mesh and the KD-Tree
  rebuilt on each change. Medium.
