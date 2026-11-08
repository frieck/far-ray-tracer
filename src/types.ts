export interface EnvironmentConfig {
  path: string;
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
}
export interface SceneMaterial {
  id: number;
  label: string;
  color: [number, number, number];
  ior: number;
  surface: SurfaceMaterial | null;
}
export interface MaterialOverride { id: number; surface: SurfaceMaterial }
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
  camera: Pose & { aperture: number; focus_distance: number };
  center: [number, number, number] | null;
  name: string;
  path: string;
  primitives: number;
  lights: number;
  warnings: string[];
  /** Automatic indirect light settings for this scene's lights. */
  bounces: Bounces;
}

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
  action: "open" | "save" | "render" | "example";
  path: string | null;
}

export interface RenderRequest {
  environment: EnvironmentConfig | null;
  material_overrides: MaterialOverride[];
  pose: Pose;
  aperture: number;
  focus_distance: number;
  mode: Mode;
  width: number;
  height: number;
  depth: number;
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
  lookups: number;
  photons_per_lookup: number;
}

export interface RenderResult {
  kd_ms: number | null;
  render_ms: number;
  rays: number;
  cancelled: boolean;
  caustics: CausticReport | null;
}

export interface Settings {
  environment: EnvironmentConfig | null;
  materialOverrides: MaterialOverride[];
  pose: Pose;
  depthOfField: boolean;
  dofPreset: DofPreset;
  dofPasses: number;
  aperture: number;
  focusDistance: number;
  quality: Quality;
  customPasses: number;
  exposure: number;
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
  threads: number;
  indirect: boolean;
  /** Indirect light settings chosen by hand; null follows the scene's lights. */
  bounces: Bounces | null;
  useKd: boolean;
  evolution: boolean;
  mode: Mode;
  comparison: Comparison;
}
