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

- [x] **Volumes**: fog, smoke and light shafts through the air, in boxes and spheres of even
  density. Medium to high.
- [ ] **Uneven volumes**: smoke and clouds from a density grid or noise, and fog that thins with
  height. Medium.
- [ ] **Physical sun and sky**: a sky computed from the time of day, without an HDR image.
  Medium.
- [ ] **Spotlights and IES profiles**: a flashlight beam or a real fixture's light. Low.

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
- [ ] **GPU rendering (wgpu)**: much faster. Very high.

## Geometry and scenes

- [x] **OBJ and glTF import**: models from any 3D program, with their materials. Medium.
- [ ] **Instancing**: many copies of an object for almost no memory. Medium.
- [ ] **Subdivision surfaces**: smooth meshes from coarse ones. High.
- [ ] **CSG (boolean operations)**: would carve the material ball's medallion into the
  sphere, for one. High.
- [ ] **Displacement height in the material editor**: needs the mesh and the KD-Tree
  rebuilt on each change. Medium.
