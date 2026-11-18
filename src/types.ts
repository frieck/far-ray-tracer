/** A time and place that sets the sun's position; see crates/far-core/src/sky.rs. */
export interface SunTime {
  hour: number;
  latitude: number;
  day: number;
}

/** A physical sky and sun computed by the engine, in place of an HDR image. */
export interface SkyConfig {
  elevation: number;
  azimuth: number;
  turbidity: number;
  sun_size: number;
  sun_strength: number;
  time: SunTime | null;
}

export interface EnvironmentConfig {
  /** A panorama (HDR, EXR, PNG or JPEG); empty when `sky` computes the environment. */
  path: string;
  sky?: SkyConfig | null;
  intensity: number;
  rotation: number;
  visible: boolean;
  enabled: boolean;
}
export type MaterialPreset = "metal" | "plastic" | "ceramic" | "glass" | "custom";
export interface SurfaceMaterial {
  preset: MaterialPreset;
  roughness: number;
  metallic: number;
  ior: number;
  color: [number, number, number] | null;
  /** Glass: distance through which light keeps the color; null keeps plain glass's tint. */
  absorption_depth?: number | null;
  /** Glass: Abbe number; lower spreads light into wider rainbows; null disperses nothing. */
  abbe?: number | null;
  clearcoat?: number;
  clearcoat_roughness?: number;
  sheen?: number;
  sheen_color?: [number, number, number];
  /** From -1 to 1: highlights stretched across or along the surface's texture direction. */
  anisotropy?: number;
  /** Degrees. */
  anisotropy_rotation?: number;
  /** Share of diffuse light that travels under the surface. */
  subsurface?: number;
  /** Mean distance light travels under the surface, in scene units. */
  subsurface_radius?: number;
}
export interface SceneMaterial {
  id: number;
  label: string;
  color: [number, number, number];
  ior: number;
  surface: SurfaceMaterial | null;
  bump: BumpInfo | null;
  /** The light it gives off, its brightest channel 1; null when it does not glow. */
  emission: Glow | null;
}
/** A glow as scene files write it: a color from 0 to 1 and a strength, 0 for none. */
export interface Glow { color: [number, number, number]; strength: number }
/** An edit of one material: a new surface, bump strength or glow; null keeps the scene's. */
export interface MaterialOverride {
  id: number;
  surface: SurfaceMaterial | null;
  bump_strength: number | null;
  emission?: Glow | null;
}
export interface BumpInfo {
  /** A height map, whose strength is a depth in scene units, or a tangent-space normal map. */
  kind: "height" | "normal";
  strength: number;
}
export type Mode = "classic" | "fixed" | "enhanced";
export type Comparison = "off" | "classic" | "fixed";
export type Quality = 16 | 64 | 256 | "custom";
export type DofPreset = "subtle" | "portrait" | "strong" | "custom";
export type CausticPreset = "soft" | "balanced" | "sharp" | "custom";
export type Resolution = `${number}x${number}` | "custom";

export interface Pose {
  position: [number, number, number];
  target: [number, number, number];
  zoom: number;
}

export interface SceneInfo {
  environment: EnvironmentConfig | null;
  materials: SceneMaterial[];
  camera: Pose & {
    aperture: number; focus_distance: number; blades: number; blade_rotation: number;
    /** The sensor's width in millimetres, and a scene unit's length in metres. */
    sensor_width: number; scene_unit: number;
  };
  center: [number, number, number] | null;
  name: string;
  path: string;
  primitives: number;
  lights: number;
  /** The file's objects and lights, in its order: what the outliner lists and hides by index. */
  objects: SceneObjectInfo[];
  light_list: SceneLightInfo[];
  /** The file's fog, as it writes it; null without fog. */
  fog: Record<string, unknown> | null;
  warnings: string[];
  /** Automatic indirect light settings for this scene's lights. */
  bounces: Bounces;
  /** Render settings saved with the scene by the app, if any. */
  render: unknown;
}

export interface SceneObjectInfo {
  name: string;
  /** sphere, box, cylinder, tube, cone, torus, ground, plane, mesh, model, volume or fog. */
  kind: string;
  /** Its materials' ids, as in `SceneInfo.materials`. */
  materials: number[];
  /** A model's parts, one name for each of `materials`; empty for anything else. */
  parts: string[];
  /** Its entry in the file: what the Object tab edits, and what a duplicate copies. */
  source: Record<string, unknown>;
}

/** The objects added in the app as the scene read with them has them, and their materials. */
export interface AddedObjects {
  objects: SceneObjectInfo[];
  materials: SceneMaterial[];
}

export interface SceneLightInfo {
  name: string;
  /** point, rectangle, sphere, spot or ies. */
  kind: string;
  /** Its entry in the file. */
  source: Record<string, unknown>;
}

/** Fields that replace those of the file's objects and lights, by index, as the engine reads them. */
export interface EntryEdits {
  objects: [number, Record<string, unknown>][];
  lights: [number, Record<string, unknown>][];
  /** Whole entries added after the file's, and entries deleted, by index. */
  added_objects: Record<string, unknown>[];
  added_lights: Record<string, unknown>[];
  removed_objects: number[];
  removed_lights: number[];
  /** A fog in place of the file's, or none at all. */
  fog?: Record<string, unknown>;
  no_fog?: boolean;
  /** The camera's scene unit in place of the file's, for lamps given in watts. */
  scene_unit?: number;
}

/** What the interface changes in the scene itself, for the commands that look at it. */
export interface SceneViewArgs {
  hidden_objects: number[];
  hidden_lights: number[];
  entries: EntryEdits;
}

/** What the outliner has selected: an object or a light, by its index. */
export type Selection = { kind: "object" | "light"; id: number } | null;

export interface Bounces {
  strength: number;
  max: number;
  /** Most light one rebound may bring back; 0 means no limit. */
  clamp: number;
}

export interface Startup {
  cores: number;
  initial_scene: string | null;
}

export interface MenuAction {
  action: "open" | "save" | "save_scene" | "save_scene_as" | "render" | "example" | "undo" | "redo";
  path: string | null;
}

/** A part of the image, as fractions of its width and height. */
export interface Area { x0: number; y0: number; x1: number; y1: number }

/** How light becomes screen colors; see crates/far-core/src/tonemap.rs. */
export type ExposureMode = "simple" | "physical";

export type ToneMapping = "standard" | "agx" | "agx_punchy" | "aces";

/** Where Enhanced renders run: Auto takes the graphics card for scenes it renders as the CPU does. */
export type Device = "auto" | "gpu" | "cpu";

export interface RenderRequest {
  device: Device;
  /** Renders only this rectangle, in pixels: [x0, y0, x1, y1], the end excluded. */
  region?: [number, number, number, number] | null;
  environment: EnvironmentConfig | null;
  material_overrides: MaterialOverride[];
  pose: Pose;
  aperture: number;
  focus_distance: number;
  /** Diaphragm blades, 0 for a round opening, and their rotation in degrees. */
  blades: number;
  blade_rotation: number;
  mode: Mode;
  width: number;
  height: number;
  depth: number;
  /** Most mirror and glossy reflections, and most glass crossings, in a path; null is Ray depth. */
  glossy_depth?: number | null;
  transmission_depth?: number | null;
  threads: number;
  use_kd: boolean;
  samples: number;
  indirect: boolean;
  indirect_strength?: number;
  max_bounces?: number;
  indirect_clamp?: number;
  denoise: boolean;
  bloom: boolean;
  exposure: number;
  tone_mapping: ToneMapping;
  /** The camera's white balance in kelvin (6500 changes nothing), vignetting and chromatic
   * aberration from 0 to 1: shown again without rendering, as exposure is. */
  white_balance: number;
  tint: number;
  vignette: number;
  chromatic_aberration: number;
  /** Film grain from 0 to 1 at `iso`, which makes it stronger above 100. */
  grain: number;
  iso: number;
  halation: number;
  /** A color look graded over the finished picture: a bundled look's name or a .cube file's
   * path, and how strongly, from 0 to 1. */
  lut: string | null;
  lut_strength: number;
  /** What the outliner hides, by index in the scene's objects and lights. */
  hidden_objects: number[];
  hidden_lights: number[];
  entries: EntryEdits;
  caustics: boolean;
  photons: number;
  caustic_radius: number;
  caustic_strength: number;
  caustics_only: boolean;
  keep: boolean;
}

export interface CausticReport {
  emitted: number;
  stored: number;
  trace_ms: number;
  memory_bytes: number;
  /** Counted on the CPU only: null after a GPU render. */
  lookups: number | null;
  photons_per_lookup: number | null;
}

export interface RenderResult {
  kd_ms: number | null;
  render_ms: number;
  rays: number;
  cancelled: boolean;
  caustics: CausticReport | null;
  /** The denoiser the render used, if any. */
  denoiser: "oidn" | "builtin" | null;
  /** The last render shown again with new exposure, tone mapping, denoise or bloom. */
  redisplayed?: boolean;
  /** The render continued from this many passes of the last one. */
  continued_from?: number | null;
  /** The graphics card that rendered, when it was not the CPU. */
  gpu?: string | null;
  /** What the scene uses that the graphics card left out. */
  gpu_notes?: string[];
  /** Why the CPU rendered although the graphics card was asked for. */
  cpu_reason?: string | null;
}

export interface Settings {
  environment: EnvironmentConfig | null;
  materialOverrides: MaterialOverride[];
  pose: Pose;
  depthOfField: boolean;
  dofPreset: DofPreset;
  dofPasses: number;
  /** The lens opening as an f-number: the depth of field's, and in Physical exposure the light's. */
  fNumber: number;
  /** The sensor's width in millimetres, which turns the zoom into a focal length. */
  sensorWidth: number;
  /** How long a scene unit is, in metres, which turns the f-number into an opening and lamps'
   * watts into light. */
  sceneUnit: number;
  /** The scene file's own unit: when `sceneUnit` differs, the scene is read again with it. */
  fileSceneUnit: number;
  focusDistance: number;
  blades: number;
  bladeRotation: number;
  quality: Quality;
  customPasses: number;
  /** Simple sets the light with one Brightness, in stops; Physical with the ISO and shutter. */
  exposureMode: ExposureMode;
  brightness: number;
  iso: number;
  /** The shutter time in seconds. */
  shutter: number;
  toneMapping: ToneMapping;
  whiteBalance: number;
  tint: number;
  vignette: number;
  chromaticAberration: number;
  grain: number;
  halation: number;
  /** A bundled look's name, such as "warm-film", or the path of a .cube file; null for none. */
  lut: string | null;
  lutStrength: number;
  /** Objects and lights the outliner hides, by index; saved with the scene. */
  hiddenObjects: number[];
  hiddenLights: number[];
  /** Edited fields of the file's objects and lights, by index; Save Scene writes them in. */
  objectEdits: Record<number, Record<string, unknown>>;
  lightEdits: Record<number, Record<string, unknown>>;
  /**
   * Objects and lights added after the file's, so the first added object's index is the
   * file's count of objects; and those deleted, by index, hidden until Save Scene drops them.
   */
  addedObjects: Record<string, unknown>[];
  addedLights: Record<string, unknown>[];
  removedObjects: number[];
  removedLights: number[];
  /** The fog as edited: its fields, false for none, or null for the file's. */
  fogEdit: Record<string, unknown> | false | null;
  denoise: boolean;
  bloom: boolean;
  caustics: boolean;
  causticPreset: CausticPreset;
  photonMillions: number;
  causticRadius: number;
  causticStrength: number;
  resolution: Resolution;
  customWidth: number;
  customHeight: number;
  depth: number;
  /** Reflection and transmission limits within `depth`; null follows it. */
  glossyDepth: number | null;
  transmissionDepth: number | null;
  threads: number;
  indirect: boolean;
  /** Indirect light settings chosen by hand; null follows the scene's lights. */
  bounces: Bounces | null;
  useKd: boolean;
  device: Device;
  evolution: boolean;
  mode: Mode;
  comparison: Comparison;
}
