import type { DofPreset, CausticPreset, Comparison, Mode, Quality, RenderRequest, Resolution, Settings } from "./types";

export const MODE_NAMES: Record<Mode, string> = {
  classic: "2008",
  fixed: "Fixed",
  enhanced: "Enhanced",
};

export const MODE_NOTES: Record<Mode, string> = {
  classic: "The original program, pixel by pixel, bugs included.",
  fixed: "The same algorithm with its bugs removed: one ray per pixel and hard shadows.",
  enhanced: "Adds antialiasing, soft shadows, realistic glass and indirect light.",
};

const DEFAULT_DEPTH: Record<Mode, number> = { classic: 3, fixed: 3, enhanced: 5 };

export const RESOLUTION_GROUPS: { label: string; options: { value: Resolution; label: string }[] }[] = [
  {
    label: "4:3",
    options: [
      { value: "400x300", label: "400 × 300" },
      { value: "800x600", label: "800 × 600" },
      { value: "1024x768", label: "1024 × 768" },
      { value: "1600x1200", label: "1600 × 1200" },
    ],
  },
  {
    label: "16:9",
    options: [
      { value: "1280x720", label: "1280 × 720 (HD)" },
      { value: "1920x1080", label: "1920 × 1080 (Full HD)" },
      { value: "2560x1440", label: "2560 × 1440 (QHD)" },
      { value: "3840x2160", label: "3840 × 2160 (4K)" },
      { value: "5120x2880", label: "5120 × 2880 (5K)" },
      { value: "7680x4320", label: "7680 × 4320 (8K)" },
    ],
  },
  {
    label: "16:10",
    options: [
      { value: "1440x900", label: "1440 × 900" },
      { value: "1920x1200", label: "1920 × 1200" },
      { value: "2560x1600", label: "2560 × 1600" },
      { value: "3024x1964", label: "3024 × 1964 (MacBook Pro 14)" },
      { value: "3456x2234", label: "3456 × 2234 (MacBook Pro 16)" },
    ],
  },
  {
    label: "Ultrawide",
    options: [
      { value: "2560x1080", label: "2560 × 1080" },
      { value: "3440x1440", label: "3440 × 1440" },
      { value: "5120x2160", label: "5120 × 2160" },
    ],
  },
  {
    label: "Square and vertical",
    options: [
      { value: "1080x1080", label: "1080 × 1080" },
      { value: "1080x1350", label: "1080 × 1350" },
      { value: "1080x1920", label: "1080 × 1920" },
    ],
  },
];

/** Each preset keeps about the same number of photons per lookup, so all stay clean. */
export const CAUSTIC_PRESETS: Record<Exclude<CausticPreset, "custom">, { photons: number; radius: number }> = {
  soft: { photons: 2_000_000, radius: 0.2 },
  balanced: { photons: 6_000_000, radius: 0.12 },
  sharp: { photons: 16_000_000, radius: 0.07 },
};

export const DOF_PRESETS: Record<Exclude<DofPreset, "custom">, { aperture: number; passes: number }> = {
  subtle: { aperture: 0.25, passes: 128 },
  portrait: { aperture: 0.6, passes: 256 },
  strong: { aperture: 1.2, passes: 512 },
};

export function dofValues(settings: Settings) {
  return settings.dofPreset === "custom"
    ? { aperture: settings.aperture, passes: settings.dofPasses }
    : DOF_PRESETS[settings.dofPreset];
}

/** Passes of the quality preset, or the custom count. */
export function qualityPasses(settings: Settings): number {
  return settings.quality === "custom" ? settings.customPasses : settings.quality;
}

export const QUALITY_PRESETS: { value: Exclude<Quality, "custom">; label: string }[] = [
  { value: 16, label: "Draft" },
  { value: 64, label: "Good" },
  { value: 256, label: "High" },
];

/** A quality that shows these passes: the preset with that count, or Custom. */
export function withPasses(passes: number): Partial<Settings> {
  const preset = QUALITY_PRESETS.find((p) => p.value === passes);
  return { quality: preset ? preset.value : "custom", customPasses: passes };
}

/** Depth of field values after editing one of them: the preset they match, or Custom. */
export function withDof(settings: Settings, change: Partial<{ aperture: number; passes: number }>): Partial<Settings> {
  const values = { ...dofValues(settings), ...change };
  const preset = (Object.keys(DOF_PRESETS) as Exclude<DofPreset, "custom">[])
    .find((name) => DOF_PRESETS[name].aperture === values.aperture && DOF_PRESETS[name].passes === values.passes);
  return { dofPreset: preset ?? "custom", aperture: values.aperture, dofPasses: values.passes };
}

/** Caustic values as the panel shows them, from the preset or the custom fields. */
export function causticSettings(settings: Settings) {
  if (settings.causticPreset === "custom") {
    return { photonMillions: settings.photonMillions, radius: settings.causticRadius, strength: settings.causticStrength };
  }
  const { photons, radius } = CAUSTIC_PRESETS[settings.causticPreset];
  return { photonMillions: photons / 1_000_000, radius, strength: 1 };
}

/** Caustic values after editing one of them: the preset they match, or Custom. */
export function withCaustics(settings: Settings, change: Partial<ReturnType<typeof causticSettings>>): Partial<Settings> {
  const values = { ...causticSettings(settings), ...change };
  const preset = (Object.keys(CAUSTIC_PRESETS) as Exclude<CausticPreset, "custom">[]).find((name) =>
    CAUSTIC_PRESETS[name].photons === Math.round(values.photonMillions * 1_000_000)
      && CAUSTIC_PRESETS[name].radius === values.radius && values.strength === 1);
  return {
    causticPreset: preset ?? "custom",
    photonMillions: values.photonMillions,
    causticRadius: values.radius,
    causticStrength: values.strength,
  };
}

export function effectivePasses(settings: Settings): number {
  const quality = qualityPasses(settings);
  return settings.depthOfField ? Math.max(quality, dofValues(settings).passes) : quality;
}

export const MAX_PASSES = 4096;
export const MIN_IMAGE_SIDE = 16;
export const MAX_IMAGE_SIDE = 8192;
const PREVIEW_SIDE = 480;

export function defaultSettings(cores: number): Settings {
  return {
    environment: null,
    materialOverrides: [],
    pose: { position: [0, 0, -5], target: [0, 0, 0], zoom: 1 },
    depthOfField: false,
    dofPreset: "portrait",
    dofPasses: 256,
    aperture: 0.5,
    focusDistance: 10,
    blades: 0,
    bladeRotation: 0,
    quality: 64,
    customPasses: 128,
    exposure: 1,
    toneMapping: "standard",
    denoise: true,
    bloom: false,
    caustics: false,
    causticPreset: "balanced",
    photonMillions: 6,
    causticRadius: 0.12,
    causticStrength: 1,
    resolution: "800x600",
    customWidth: 1920,
    customHeight: 1080,
    depth: DEFAULT_DEPTH.enhanced,
    threads: cores,
    indirect: true,
    bounces: null,
    useKd: true,
    evolution: false,
    mode: "enhanced",
    comparison: "off",
  };
}

/** The stage that actually renders: outside evolution it is always Enhanced. */
export function activeMode(settings: Settings): Mode {
  return settings.evolution ? settings.mode : "enhanced";
}

/** The stage shown on the left of the divider, if any. */
export function activeComparison(settings: Settings): Mode | null {
  if (!settings.evolution || settings.comparison === "off") return null;
  return settings.comparison;
}

/** Changes the stage, keeping the comparison valid: only earlier stages can be compared. */
export function withMode(settings: Settings, mode: Mode): Settings {
  let comparison: Comparison = settings.comparison;
  if (mode === "classic") comparison = "off";
  if (mode === "fixed" && comparison === "fixed") comparison = "classic";
  return { ...settings, mode, comparison, depth: DEFAULT_DEPTH[mode] };
}

/** Turning evolution on starts a comparison right away; off returns to the normal app. */
export function withEvolution(settings: Settings, evolution: boolean): Settings {
  return {
    ...settings,
    evolution,
    mode: "enhanced",
    comparison: evolution ? "classic" : "off",
    depth: DEFAULT_DEPTH.enhanced,
  };
}

export function imageSize(settings: Settings): [number, number] {
  if (settings.resolution === "custom") return [settings.customWidth, settings.customHeight];
  const [width, height] = settings.resolution.split("x").map(Number);
  return [width, height];
}

function causticValues(settings: Settings) {
  if (settings.causticPreset === "custom") {
    return {
      photons: Math.round(settings.photonMillions * 1_000_000),
      caustic_radius: settings.causticRadius,
      caustic_strength: settings.causticStrength,
    };
  }
  const { photons, radius } = CAUSTIC_PRESETS[settings.causticPreset];
  return { photons, caustic_radius: radius, caustic_strength: 1 };
}

/** A small, quick image to follow the camera while it moves. */
export function buildPreview(settings: Settings): RenderRequest {
  const [width, height] = imageSize(settings);
  const scale = Math.min(1, PREVIEW_SIDE / Math.max(width, height));
  return {
    ...buildRequest(settings, "enhanced", false),
    width: Math.max(MIN_IMAGE_SIDE, Math.round(width * scale)),
    height: Math.max(MIN_IMAGE_SIDE, Math.round(height * scale)),
    samples: 2,
    depth: Math.min(settings.depth, 3),
    aperture: 0,
    indirect: false,
    denoise: false,
    bloom: false,
    caustics: false,
  };
}

export function buildRequest(settings: Settings, mode: Mode, keep: boolean): RenderRequest {
  const [width, height] = imageSize(settings);
  return {
    mode,
    environment: mode === "enhanced" ? settings.environment : null,
    material_overrides: mode === "enhanced" ? settings.materialOverrides : [],
    pose: settings.pose,
    aperture: mode === "enhanced" && settings.depthOfField ? dofValues(settings).aperture : 0,
    focus_distance: settings.focusDistance,
    blades: settings.blades,
    blade_rotation: settings.bladeRotation,
    width,
    height,
    depth: settings.depth,
    threads: settings.threads,
    use_kd: settings.useKd,
    samples: effectivePasses(settings),
    indirect: settings.indirect,
    ...(settings.bounces && {
      indirect_strength: settings.bounces.strength,
      max_bounces: settings.bounces.max,
      indirect_clamp: settings.bounces.clamp,
    }),
    denoise: mode === "enhanced" && settings.denoise,
    bloom: mode === "enhanced" && settings.bloom,
    exposure: settings.exposure,
    tone_mapping: settings.toneMapping,
    caustics: settings.caustics,
    ...causticValues(settings),
    caustics_only: false,
    keep,
  };
}
