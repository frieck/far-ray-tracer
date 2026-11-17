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

export const DOF_PRESETS: Record<Exclude<DofPreset, "custom">, { fNumber: number; passes: number }> = {
  subtle: { fNumber: 5.6, passes: 128 },
  portrait: { fNumber: 2.8, passes: 256 },
  strong: { fNumber: 1.4, passes: 512 },
};

/** The camera as far-core's camera.rs: the image plane 5 units ahead, a 4:3 picture 8 wide. */
const IMAGE_DISTANCE = 5;
const PICTURE_WIDTH = 8;
export const DEFAULT_SENSOR_WIDTH = 36;
export const DEFAULT_SCENE_UNIT = 0.1;
/** Sensor widths in millimetres, from the largest. */
export const SENSORS: { width: number; label: string }[] = [
  { width: 43.8, label: "Medium format" },
  { width: 36, label: "Full frame" },
  { width: 24.9, label: "Super 35" },
  { width: 23.6, label: "APS-C" },
  { width: 17.3, label: "Micro Four Thirds" },
  { width: 13.2, label: "1-inch" },
  { width: 9.8, label: "Phone" },
];

/** The color looks bundled in assets/luts, by the name a render asks for. */
export const LOOKS: { name: string; label: string }[] = [
  { name: "warm-film", label: "Warm film" },
  { name: "teal-orange", label: "Teal and orange" },
  { name: "bleach-bypass", label: "Bleach bypass" },
  { name: "black-and-white", label: "Black and white" },
  { name: "faded", label: "Faded print" },
];

/** A LUT as the panel names it: the bundled look's name, or the file's. */
export function lutLabel(lut: string): string {
  return LOOKS.find((look) => look.name === lut)?.label ?? lut.split(/[\\/]/).pop() ?? lut;
}

/** What a camera preset sets: the sensor, the look of film and lens, the curve and the LUT. */
export interface CameraPreset {
  label: string;
  note: string;
  sensorWidth: number;
  toneMapping: import("./types").ToneMapping;
  lut: string | null;
  lutStrength: number;
  grain: number;
  halation: number;
  vignette: number;
  chromaticAberration: number;
}

/**
 * Cameras as a whole: choosing one keeps the framing and the exposure, so the lens's focal
 * length follows the sensor, and with it how much the f-number blurs.
 */
export const CAMERA_PRESETS: Record<string, CameraPreset> = {
  digital: {
    label: "Digital", note: "A clean full-frame camera: no grain, no look.",
    sensorWidth: 36, toneMapping: "agx", lut: null, lutStrength: 1, grain: 0, halation: 0, vignette: 0, chromaticAberration: 0,
  },
  colorFilm: {
    label: "35 mm color film", note: "Color negative film printed warm, with grain and a little halation.",
    sensorWidth: 36, toneMapping: "agx", lut: "warm-film", lutStrength: 1, grain: 0.3, halation: 0.3, vignette: 0.2,
    chromaticAberration: 0.05,
  },
  blackAndWhite: {
    label: "Black-and-white film", note: "Panchromatic film with strong grain.",
    sensorWidth: 36, toneMapping: "agx", lut: "black-and-white", lutStrength: 1, grain: 0.45, halation: 0, vignette: 0.25,
    chromaticAberration: 0,
  },
  cinema: {
    label: "Cinema camera", note: "A Super 35 sensor, the ACES curve and a teal-and-orange grade.",
    sensorWidth: 24.9, toneMapping: "aces", lut: "teal-orange", lutStrength: 0.8, grain: 0.12, halation: 0.2, vignette: 0.1,
    chromaticAberration: 0.05,
  },
  phone: {
    label: "Phone", note: "A small sensor, so nearly everything is sharp, and punchy colors.",
    sensorWidth: 9.8, toneMapping: "agx_punchy", lut: null, lutStrength: 1, grain: 0, halation: 0, vignette: 0.1,
    chromaticAberration: 0.15,
  },
  vintage: {
    label: "Vintage", note: "An old lens on faded film: dark corners, color fringes and grain.",
    sensorWidth: 36, toneMapping: "agx", lut: "faded", lutStrength: 1, grain: 0.4, halation: 0.35, vignette: 0.55,
    chromaticAberration: 0.4,
  },
};

const PRESET_KEYS = ["sensorWidth", "toneMapping", "lut", "lutStrength", "grain", "halation", "vignette", "chromaticAberration"] as const;

/** The camera preset the settings match, or null when any of its values was changed. */
export function cameraPresetOf(settings: Settings): string | null {
  return Object.keys(CAMERA_PRESETS).find((name) =>
    PRESET_KEYS.every((key) => CAMERA_PRESETS[name][key] === settings[key])) ?? null;
}

/** What a camera preset changes; the zoom stays, so the framing does and the focal length follows the sensor. */
export function withCameraPreset(name: string): Partial<Settings> {
  const preset = CAMERA_PRESETS[name];
  if (!preset) return {};
  return Object.fromEntries(PRESET_KEYS.map((key) => [key, preset[key]])) as Partial<Settings>;
}

/** The focal length, in millimetres, of a zoom on a sensor `sensorWidth` millimetres wide. */
export function focalLength(zoom: number, sensorWidth: number): number {
  return (IMAGE_DISTANCE * sensorWidth * zoom) / PICTURE_WIDTH;
}

/** The zoom of a focal length on a sensor. */
export function zoomFor(focal: number, sensorWidth: number): number {
  return (PICTURE_WIDTH * focal) / (IMAGE_DISTANCE * sensorWidth);
}

/** The lens opening's diameter in scene units, for a focal length and an f-number. */
export function apertureFor(focal: number, fNumber: number, sceneUnit: number): number {
  return focal / fNumber / (1000 * sceneUnit);
}

/** The f-number of an opening `aperture` scene units across. */
export function fNumberFor(focal: number, aperture: number, sceneUnit: number): number {
  return focal / (aperture * 1000 * sceneUnit);
}

/** The depth of field's f-number and passes, and the opening they make, in scene units. */
export function dofValues(settings: Settings) {
  const { fNumber, passes } = settings.dofPreset === "custom"
    ? { fNumber: settings.fNumber, passes: settings.dofPasses }
    : DOF_PRESETS[settings.dofPreset];
  const aperture = apertureFor(focalLength(settings.pose.zoom, settings.sensorWidth), fNumber, settings.sceneUnit);
  return { fNumber, passes, aperture };
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
export function withDof(settings: Settings, change: Partial<{ fNumber: number; passes: number }>): Partial<Settings> {
  const values = { ...dofValues(settings), ...change };
  const preset = (Object.keys(DOF_PRESETS) as Exclude<DofPreset, "custom">[])
    .find((name) => DOF_PRESETS[name].fNumber === values.fNumber && DOF_PRESETS[name].passes === values.passes);
  return { dofPreset: preset ?? "custom", fNumber: values.fNumber, dofPasses: values.passes };
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

/** The engine's default sky: mid-afternoon sun on a clear day. */
export const DEFAULT_SKY: import("./types").SkyConfig = {
  elevation: 35, azimuth: 140, turbidity: 3, sun_size: 0.53, sun_strength: 1, time: null,
};
export const DEFAULT_SUN_TIME: import("./types").SunTime = { hour: 15, latitude: -27, day: 80 };

/** ISO 100 at 1/125 s leaves the light as rendered, as in the engine's `lens::exposure`. */
export const REFERENCE_ISO = 100;
export const REFERENCE_SHUTTER = 1 / 125;
export const REFERENCE_F_NUMBER = 8;
/** The usual ranges of the camera's ISO and shutter time. */
export const ISO_RANGE: [number, number] = [25, 6400];
export const SHUTTER_RANGE: [number, number] = [1 / 4000, 1];

/** How much more light than at f/8 the lens lets in: twice per stop wider. */
function apertureGain(settings: Settings): number {
  return (REFERENCE_F_NUMBER / dofValues(settings).fNumber) ** 2;
}

/**
 * The light's multiplier: 2 to the brightness, or what the ISO, shutter and f-number make of
 * it, as in the engine's `lens::exposure`.
 */
export function exposureOf(settings: Settings): number {
  if (settings.exposureMode === "simple") return 2 ** settings.brightness;
  return (settings.iso / REFERENCE_ISO) * (settings.shutter / REFERENCE_SHUTTER) * apertureGain(settings);
}

/**
 * The ISO and shutter time of the picture: as set, or in Simple what a camera in program mode
 * would pick for that light at the lens's f-number. From ISO 100 at 1/125 s, more light takes a
 * longer shutter down to 1/30 s, held in the hand, then a higher ISO up to 6400, then longer
 * still, on a tripod; less light takes a shorter shutter up to 1/4000 s, then ISO down to 50.
 */
export function cameraOf(settings: Settings): { iso: number; shutter: number } {
  if (settings.exposureMode === "physical") return { iso: settings.iso, shutter: settings.shutter };
  // What the ISO and shutter must give, the opening's share taken out.
  const stops = settings.brightness - Math.log2(apertureGain(settings));
  const handheld = Math.log2((1 / 30) / REFERENCE_SHUTTER);
  const fastest = Math.log2(SHUTTER_RANGE[0] / REFERENCE_SHUTTER);
  let shutterStops: number;
  let isoStops: number;
  if (stops >= 0) {
    shutterStops = Math.min(stops, handheld);
    isoStops = Math.min(stops - shutterStops, 6);
    shutterStops += stops - shutterStops - isoStops;
  } else {
    shutterStops = Math.max(stops, fastest);
    isoStops = Math.max(stops - shutterStops, -1);
    shutterStops += stops - shutterStops - isoStops;
  }
  return { iso: REFERENCE_ISO * 2 ** isoStops, shutter: REFERENCE_SHUTTER * 2 ** shutterStops };
}

/** The settings in the other exposure mode, with the same light. */
export function withExposureMode(settings: Settings, mode: Settings["exposureMode"]): Partial<Settings> {
  if (mode === settings.exposureMode) return {};
  if (mode === "physical") {
    const { iso, shutter } = cameraOf(settings);
    return { exposureMode: mode, iso: Math.round(iso), shutter };
  }
  return { exposureMode: mode, brightness: Math.round(Math.log2(exposureOf(settings)) * 100) / 100 };
}

/** The values cameras mark, in thirds of a stop: of shutter speed (the N of 1/N s) and of ISO. */
const MARKED_SPEEDS = [8000, 6400, 5000, 4000, 3200, 2500, 2000, 1600, 1250, 1000, 800, 640, 500, 400, 320, 250, 200, 160,
  125, 100, 80, 60, 50, 40, 30, 25, 20, 15, 13, 10, 8, 6, 5, 4, 3, 2.5, 2, 1.6, 1.3, 1, 0.8, 0.6, 0.5, 0.4, 0.3];
const MARKED_ISOS = [25, 32, 40, 50, 64, 80, 100, 125, 160, 200, 250, 320, 400, 500, 640, 800, 1000, 1250, 1600, 2000, 2500,
  3200, 4000, 5000, 6400, 8000, 10000, 12800, 16000, 20000, 25600];

/** The marked value nearest `value` in stops, or `value` itself beyond the marks. */
function nearestMark(marks: number[], value: number): number {
  const best = marks.reduce((a, b) => (Math.abs(Math.log2(b / value)) < Math.abs(Math.log2(a / value)) ? b : a));
  return Math.abs(Math.log2(best / value)) <= 1 / 6 ? best : value;
}

/** A shutter time as a camera would mark it, for a time a program picked. */
export function markedShutter(seconds: number): string {
  return shutterLabel(1 / nearestMark(MARKED_SPEEDS, 1 / seconds));
}

/** An ISO as a camera would mark it, for one a program picked. */
export function markedIso(iso: number): number {
  return Math.round(nearestMark(MARKED_ISOS, iso));
}

/** A shutter time as cameras show it: 1/125 s, or 2 s. */
export function shutterLabel(seconds: number): string {
  if (seconds >= 0.4) return `${Number(seconds.toFixed(seconds >= 10 ? 0 : 1))} s`;
  return `1/${Math.round(1 / seconds)} s`;
}

export function defaultSettings(cores: number): Settings {
  return {
    environment: null,
    materialOverrides: [],
    pose: { position: [0, 0, -5], target: [0, 0, 0], zoom: 1 },
    depthOfField: false,
    dofPreset: "portrait",
    fNumber: DOF_PRESETS.portrait.fNumber,
    sensorWidth: DEFAULT_SENSOR_WIDTH,
    sceneUnit: DEFAULT_SCENE_UNIT,
    dofPasses: 256,
    focusDistance: 10,
    blades: 0,
    bladeRotation: 0,
    quality: 64,
    customPasses: 128,
    exposureMode: "simple",
    brightness: 0,
    iso: REFERENCE_ISO,
    shutter: REFERENCE_SHUTTER,
    toneMapping: "standard",
    whiteBalance: 6500,
    tint: 0,
    vignette: 0,
    chromaticAberration: 0,
    grain: 0,
    halation: 0,
    lut: null,
    lutStrength: 1,
    hiddenObjects: [],
    hiddenLights: [],
    objectEdits: {},
    lightEdits: {},
    addedObjects: [],
    addedLights: [],
    removedObjects: [],
    removedLights: [],
    fogEdit: null,
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
    glossyDepth: null,
    transmissionDepth: null,
    threads: cores,
    indirect: true,
    bounces: null,
    useKd: true,
    device: "auto",
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

/** Render settings saved with a scene, so it opens looking as it was saved. */
const SAVED_SETTINGS = [
  "quality", "customPasses", "resolution", "customWidth", "customHeight", "depth", "glossyDepth", "transmissionDepth",
  "exposureMode", "brightness", "iso", "shutter", "toneMapping", "whiteBalance", "tint", "vignette",
  "chromaticAberration", "grain", "halation", "lut", "lutStrength", "hiddenObjects", "hiddenLights",
  "denoise", "bloom", "indirect", "bounces", "caustics", "causticPreset", "photonMillions", "causticRadius",
  "causticStrength", "dofPreset", "dofPasses",
] as const satisfies readonly (keyof Settings)[];

export function savedRenderSettings(settings: Settings): Record<string, unknown> {
  return { app: 1, ...Object.fromEntries(SAVED_SETTINGS.map((key) => [key, settings[key]])) };
}

/** The settings with what a scene saved, keeping values of the wrong kind out. */
export function withSavedRenderSettings(settings: Settings, saved: unknown): Settings {
  if (!saved || typeof saved !== "object") return settings;
  const values = saved as Record<string, unknown>;
  const next: Record<string, unknown> = { ...settings };
  for (const key of SAVED_SETTINGS) {
    const value = values[key];
    const current = settings[key];
    const sameKind = key === "bounces"
      ? value === null || (typeof value === "object" && value !== null)
      : key === "glossyDepth" || key === "transmissionDepth"
      ? value === null || (typeof value === "number" && value >= 1)
      : key === "lut"
      ? value === null || (typeof value === "string" && value !== "")
      : key === "hiddenObjects" || key === "hiddenLights"
      ? Array.isArray(value) && value.every((v) => Number.isInteger(v) && v >= 0)
      : key === "exposureMode"
      ? value === "simple" || value === "physical"
      : typeof value === typeof current || (key === "quality" && (typeof value === "number" || value === "custom"));
    if (value !== undefined && sameKind) next[key] = value;
  }
  // Scenes saved before the camera had ISO and shutter kept an exposure multiplier: the same
  // light as a brightness in stops.
  const exposure = values.exposure;
  if (values.brightness === undefined && typeof exposure === "number" && exposure > 0 && Number.isFinite(exposure)) {
    next.exposureMode = "simple";
    next.brightness = Math.log2(exposure);
  }
  return next as unknown as Settings;
}

/** What Save Scene writes: the scene's edits made in the app. */
export function sceneSaveRequest(settings: Settings) {
  return {
    camera: {
      ...settings.pose,
      aperture: settings.depthOfField ? dofValues(settings).aperture : 0,
      focus_distance: settings.focusDistance,
      sensor_width: settings.sensorWidth,
      scene_unit: settings.sceneUnit,
      blades: settings.blades,
      blade_rotation: settings.bladeRotation,
    },
    environment: settings.environment,
    material_overrides: settings.materialOverrides,
    render: savedRenderSettings(settings),
    entries: entryEdits(settings),
  };
}

/** The settings with fields of object or light `id` edited; null takes a field away. */
export function withEntryEdit(settings: Settings, light: boolean, id: number, patch: Record<string, unknown>): Partial<Settings> {
  const key = light ? "lightEdits" : "objectEdits";
  const all = settings[key];
  return { [key]: { ...all, [id]: { ...all[id], ...patch } } } as Partial<Settings>;
}

/** The edits to the file's objects and lights, as the engine takes them. */
export function entryEdits(settings: Settings): import("./types").EntryEdits {
  const list = (edits: Record<number, Record<string, unknown>>) => Object.entries(edits)
    .filter(([, fields]) => Object.keys(fields).length > 0)
    .map(([index, fields]) => [Number(index), fields] as [number, Record<string, unknown>])
    .sort((a, b) => a[0] - b[0]);
  return {
    objects: list(settings.objectEdits),
    lights: list(settings.lightEdits),
    added_objects: settings.addedObjects,
    added_lights: settings.addedLights,
    removed_objects: settings.removedObjects,
    removed_lights: settings.removedLights,
    ...(settings.fogEdit === false ? { no_fog: true } : settings.fogEdit ? { fog: settings.fogEdit } : {}),
  };
}

/** The scene's fog as edited, or null when it has none. */
export function fogOf(scene: import("./types").SceneInfo, settings: Settings): Record<string, unknown> | null {
  if (settings.fogEdit === false) return null;
  return settings.fogEdit ?? scene.fog;
}

/** What the interface changes in the scene, for picking, outlines and the 3D view. */
export function sceneView(settings: Settings): import("./types").SceneViewArgs {
  return { hidden_objects: settings.hiddenObjects, hidden_lights: settings.hiddenLights, entries: entryEdits(settings) };
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
    glossy_depth: mode === "enhanced" ? settings.glossyDepth : null,
    transmission_depth: mode === "enhanced" ? settings.transmissionDepth : null,
    threads: settings.threads,
    use_kd: settings.useKd,
    device: settings.device,
    samples: effectivePasses(settings),
    indirect: settings.indirect,
    ...(settings.bounces && {
      indirect_strength: settings.bounces.strength,
      max_bounces: settings.bounces.max,
      indirect_clamp: settings.bounces.clamp,
    }),
    denoise: mode === "enhanced" && settings.denoise,
    bloom: mode === "enhanced" && settings.bloom,
    exposure: exposureOf(settings),
    tone_mapping: settings.toneMapping,
    white_balance: settings.whiteBalance,
    tint: settings.tint,
    vignette: settings.vignette,
    chromatic_aberration: settings.chromaticAberration,
    grain: settings.grain,
    iso: cameraOf(settings).iso,
    halation: settings.halation,
    lut: mode === "enhanced" ? settings.lut : null,
    lut_strength: settings.lutStrength,
    hidden_objects: settings.hiddenObjects,
    hidden_lights: settings.hiddenLights,
    entries: entryEdits(settings),
    caustics: settings.caustics,
    ...causticValues(settings),
    caustics_only: false,
    keep,
  };
}
