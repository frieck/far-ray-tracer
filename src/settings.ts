import type { CausticPreset, Comparison, Mode, RenderRequest, Resolution, Settings } from "./types";

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
const CAUSTIC_PRESETS: Record<Exclude<CausticPreset, "custom">, { photons: number; radius: number }> = {
  soft: { photons: 2_000_000, radius: 0.2 },
  balanced: { photons: 6_000_000, radius: 0.12 },
  sharp: { photons: 16_000_000, radius: 0.07 },
};

export const MAX_PASSES = 4096;
export const MIN_IMAGE_SIDE = 16;
export const MAX_IMAGE_SIDE = 8192;

export function defaultSettings(cores: number): Settings {
  return {
    quality: 64,
    customPasses: 128,
    exposure: 1,
    caustics: false,
    causticPreset: "balanced",
    causticsOnly: false,
    photonMillions: 6,
    causticRadius: 0.12,
    causticStrength: 1,
    resolution: "800x600",
    customWidth: 1920,
    customHeight: 1080,
    depth: DEFAULT_DEPTH.enhanced,
    threads: cores,
    indirect: true,
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

export function buildRequest(settings: Settings, mode: Mode, keep: boolean): RenderRequest {
  const [width, height] = imageSize(settings);
  return {
    mode,
    width,
    height,
    depth: settings.depth,
    threads: settings.threads,
    use_kd: settings.useKd,
    samples: settings.quality === "custom" ? settings.customPasses : settings.quality,
    indirect: settings.indirect,
    exposure: settings.exposure,
    caustics: settings.caustics,
    ...causticValues(settings),
    caustics_only: settings.causticsOnly,
    keep,
  };
}
