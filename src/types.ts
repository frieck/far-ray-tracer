export type Mode = "classic" | "fixed" | "enhanced";
export type Comparison = "off" | "classic" | "fixed";
export type Quality = 16 | 64 | 256 | "custom";
export type CausticPreset = "soft" | "balanced" | "sharp" | "custom";
export type Resolution = `${number}x${number}` | "custom";

export interface SceneInfo {
  name: string;
  path: string;
  primitives: number;
  lights: number;
  warnings: string[];
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
  mode: Mode;
  width: number;
  height: number;
  depth: number;
  threads: number;
  use_kd: boolean;
  samples: number;
  indirect: boolean;
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
  quality: Quality;
  customPasses: number;
  exposure: number;
  caustics: boolean;
  causticPreset: CausticPreset;
  causticsOnly: boolean;
  photonMillions: number;
  causticRadius: number;
  causticStrength: number;
  resolution: Resolution;
  customWidth: number;
  customHeight: number;
  depth: number;
  threads: number;
  indirect: boolean;
  useKd: boolean;
  evolution: boolean;
  mode: Mode;
  comparison: Comparison;
}
