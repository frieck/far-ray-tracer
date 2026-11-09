# TODO

Rendering techniques still to add. Effort is a rough guess: **low** is a day or less,
**medium** a few days, **high** a week or more.

## Suggested order

Visual gain for the effort:

1. Rough and colored glass, then spectral dispersion: they complete glass, the heart of the
   showcase and of the app icon.
2. ACES/AgX tone mapping and shaped bokeh: cheap, and every image gains.
3. Volumes with light shafts: the largest change in atmosphere.
4. glTF and OBJ import: opens the app to real scenes.

## Materials

- [ ] **Rough glass**: GGX transmission, for frosted glass. Removes a limit the README
  states. Medium.
- [ ] **Colored glass**: absorption that grows with thickness (Beer's law), like a green
  bottle. Low.
- [ ] **Spectral dispersion**: glass splits light into a rainbow, as in the app icon.
  Needs rendering by wavelength. Medium.
- [ ] **Subsurface scattering**: light that enters and spreads inside skin, wax, marble or
  jade. High.
- [ ] **Clearcoat**: a varnish layer over the base, for car paint or varnished wood.
  Low to medium.
- [ ] **Anisotropic GGX**: brushed metal. Medium.
- [ ] **Sheen**: velvet and cloth. Medium.
- [ ] **Full PBR maps**: roughness, metallic and emission from textures, and alpha cutouts
  for leaves and grilles. Low to medium.

## Light and atmosphere

- [ ] **Volumes**: fog, smoke and light shafts through the air. Medium to high.
- [ ] **Physical sun and sky**: a sky computed from the time of day, without an HDR image.
  Medium.
- [ ] **Spotlights and IES profiles**: a flashlight beam or a real fixture's light. Low.

## Camera and image

- [ ] **Shaped bokeh**: hexagonal or other aperture shapes. Low.
- [ ] **Vignetting and chromatic aberration**. Low.
- [ ] **Film tone mapping (ACES or AgX) and white balance**: natural colors in bright light.
  Low.
- [ ] **Motion blur**: needs moving objects in the scene format. Medium.

## Quality and speed

- [ ] **Adaptive sampling**: stop sampling pixels that have converged; same quality,
  faster. Medium.
- [ ] **AI denoiser (Intel Open Image Denoise)**: the one in Blender and V-Ray; a large jump
  in quality at few samples. Low to medium, but needs a native library.
- [ ] **Bidirectional path tracing or progressive photon mapping**: hard caustics, such as
  light seen through glass. High.
- [ ] **GPU rendering (wgpu)**: much faster. Very high.

## Geometry and scenes

- [ ] **OBJ and glTF import**: models from any 3D program; today only `.3ds` loads.
  Medium.
- [ ] **Instancing**: many copies of an object for almost no memory. Medium.
- [ ] **Subdivision surfaces**: smooth meshes from coarse ones. High.
- [ ] **CSG (boolean operations)**: would carve the material ball's medallion into the
  sphere, for one. High.
- [ ] **Displacement height in the material editor**: needs the mesh and the KD-Tree
  rebuilt on each change. Medium.
