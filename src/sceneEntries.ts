/**
 * The scene's objects and lights as the interface edits them: the file's, then those added
 * here, each at a fixed index; deleted ones are marked, hidden until Save Scene drops them.
 * New entries are made here, sized to the view, and duplicates copy an entry as edited.
 */
import type { AddedObjects, Pose, SceneInfo, SceneLightInfo, SceneObjectInfo, Settings } from "./types";

type Vec3 = [number, number, number];
type Entry = Record<string, unknown>;

export interface Entries {
  objects: SceneObjectInfo[];
  lights: SceneLightInfo[];
}

/** What kind of light an entry is, as the engine names it. */
export function lightKind(entry: Entry): string {
  if (entry.type === "rectangle") return "rectangle";
  if (entry.type === "portal") return "portal";
  if (entry.type !== "sphere") return "point";
  if (typeof entry.ies === "string") return "ies";
  return typeof entry.spot_angle === "number" ? "spot" : "sphere";
}

/** An object or light with the name the outliner gave it, if it was renamed. */
function renamed<T extends { name: string }>(entry: T, edits: Record<string, unknown> | undefined): T {
  const name = edits?.name;
  return typeof name === "string" && name !== entry.name ? { ...entry, name } : entry;
}

/**
 * The file's objects and lights, then the added ones; `added` gives the materials and parts
 * of those, as the scene read with them has them, once it is read.
 */
export function sceneEntries(scene: SceneInfo, settings: Settings, added?: AddedObjects): Entries {
  // The fog fills the scene: it is edited in Light > Atmosphere, not as an object, and as the
  // engine puts it after every object, the added ones included, it is left out here.
  const objects: SceneObjectInfo[] = [
    ...scene.objects.filter((object) => object.kind !== "fog"),
    ...settings.addedObjects.map((entry, k) => {
      const read = added?.objects[k];
      return {
        name: String(entry.name ?? "Object"), kind: String(entry.type ?? "object"),
        materials: read?.materials ?? [], parts: read?.parts ?? [], source: entry,
      };
    }),
  ];
  const lights: SceneLightInfo[] = [
    ...scene.light_list,
    ...settings.addedLights.map((entry) => ({ name: String(entry.name ?? "Light"), kind: lightKind(entry), source: entry })),
  ].map((light, id) => {
    // An edit may make a sphere light a spotlight or a panel; see lightKinds.ts.
    const edits = settings.lightEdits[id];
    const switched = edits && ["type", "spot_angle", "ies"].some((key) => key in edits);
    return renamed(switched ? { ...light, kind: lightKind(edited(light.source, edits)) } : light, edits);
  });
  return { objects: objects.map((object, id) => renamed(object, settings.objectEdits[id])), lights };
}

/** A name not yet taken: "Sphere", then "Sphere 2", "Sphere 3" and so on. */
function freeName(base: string, taken: string[]): string {
  if (!taken.includes(base)) return base;
  for (let n = 2; ; n++) if (!taken.includes(`${base} ${n}`)) return `${base} ${n}`;
}

const round = (v: number) => Math.round(v * 1000) / 1000;
const watts = (power: number) => Number(power.toPrecision(3));
/** A bulb's power for an entry sized by `s`: a glow of 120 over a sphere of radius 0.15 s, in metres. */
const bulbPower = (s: number, unit: number) => watts(4 * Math.PI * Math.PI * (0.15 * s * unit) ** 2 * 120);
const at = (p: Vec3, dx = 0, dy = 0, dz = 0): Vec3 => [round(p[0] + dx), round(p[1] + dy), round(p[2] + dz)];

export type NewKind = "sphere" | "box" | "cylinder" | "cone" | "torus" | "plane" | "rectangle" | "bulb" | "spot" | "portal";

export const NEW_OBJECTS: { kind: NewKind; label: string }[] = [
  { kind: "sphere", label: "Sphere" },
  { kind: "box", label: "Box" },
  { kind: "cylinder", label: "Cylinder" },
  { kind: "cone", label: "Cone" },
  { kind: "torus", label: "Torus" },
  { kind: "plane", label: "Plane" },
];
export const NEW_LIGHTS: { kind: NewKind; label: string }[] = [
  { kind: "rectangle", label: "Area light" },
  { kind: "bulb", label: "Sphere light" },
  { kind: "spot", label: "Spotlight" },
  { kind: "portal", label: "Light portal" },
];

/**
 * A new object or light where the camera looks, sized to how far away that is so it shows
 * whatever the scene's scale: an object at the camera's target, a light above it.
 */
export function newEntry(kind: NewKind, pose: Pose, entries: Entries, unit: number): { light: boolean; entry: Entry } {
  const target = pose.target as Vec3;
  const distance = Math.hypot(...pose.target.map((v, k) => v - pose.position[k]));
  const s = round(Math.max(0.01, distance * 0.12));
  const material = { color: [0.8, 0.8, 0.8] };
  const names = [...entries.objects, ...entries.lights].map((e) => e.name);
  const label = [...NEW_OBJECTS, ...NEW_LIGHTS].find((n) => n.kind === kind)!.label;
  const name = freeName(label, names);
  const above = at(target, 0, 3 * s, 0);
  switch (kind) {
    case "sphere": return { light: false, entry: { type: "sphere", name, center: at(target), radius: s, material } };
    case "box": return { light: false, entry: { type: "box", name, center: at(target), size: [2 * s, 2 * s, 2 * s], material } };
    case "cylinder": return { light: false, entry: { type: "cylinder", name, center: at(target), radius: s, height: 2 * s, material } };
    case "cone": return { light: false, entry: { type: "cone", name, center: at(target), radius: s, height: 2 * s, material } };
    case "torus":
      return { light: false, entry: { type: "torus", name, center: at(target), major_radius: round(1.2 * s), minor_radius: round(0.35 * s), material } };
    case "plane": {
      const h = 2 * s;
      return {
        light: false,
        entry: { type: "plane", name, points: [at(target, -h, 0, -h), at(target, h, 0, -h), at(target, h, 0, h), at(target, -h, 0, h)], material },
      };
    }
    // A panel facing down, as edge_u × edge_v gives its front. Lamps give their light in
    // watts over metres, `unit` to a scene unit, as bright as a glow of 5 for the panel and 120
    // for a bulb at this size.
    case "rectangle":
      return {
        light: true,
        entry: { type: "rectangle", name, position: above, edge_u: [2 * s, 0, 0], edge_v: [0, 0, 2 * s], power: watts(Math.PI * 4 * (s * unit) ** 2 * 5) },
      };
    // A window's opening facing the camera's target, its size; the environment's light is
    // drawn through it, so put it in a window.
    case "portal":
      return { light: true, entry: { type: "portal", name, position: at(target, 0, s, 0), edge_u: [2 * s, 0, 0], edge_v: [0, 1.5 * s, 0] } };
    case "bulb": return { light: true, entry: { type: "sphere", name, position: above, radius: round(0.15 * s), power: bulbPower(s, unit) } };
    case "spot":
      return {
        light: true,
        entry: { type: "sphere", name, position: above, radius: round(0.15 * s), power: bulbPower(s, unit), direction: [0, -1, 0], spot_angle: 40 },
      };
  }
}

/**
 * A model from the file at `path`, resting where the camera looks and sized, as new objects
 * are, to how far away that is.
 */
export function newModel(path: string, pose: Pose, entries: Entries): Entry {
  const target = pose.target as Vec3;
  const distance = Math.hypot(...pose.target.map((v, k) => v - pose.position[k]));
  const stem = path.split(/[\\/]/).pop()!.replace(/\.[^.]+$/, "") || "Model";
  const names = [...entries.objects, ...entries.lights].map((e) => e.name);
  return { type: "model", name: freeName(stem, names), file: path, position: at(target), size: round(Math.max(0.01, distance * 0.36)) };
}

/** An entry with its edits written in, a null taking a field away. */
function edited(source: Entry, edits: Entry | undefined): Entry {
  const entry: Entry = { ...source, ...edits };
  for (const [key, value] of Object.entries(entry)) if (value === null) delete entry[key];
  return entry;
}

/** A copy of an object or light as edited, named after it, beside it when `beside`. */
export function duplicate(entries: Entries, settings: Settings, light: boolean, id: number, beside: boolean): Entry | null {
  const info = light ? entries.lights[id] : entries.objects[id];
  if (!info) return null;
  const entry = edited(info.source, (light ? settings.lightEdits : settings.objectEdits)[id]);
  const names = [...entries.objects, ...entries.lights].map((e) => e.name);
  entry.name = freeName(`${info.name} copy`, names);
  if (beside) {
    // A little to the side, so the copy shows; a keyboard duplicate moves with the mouse instead.
    const shift = (p: unknown) => (Array.isArray(p) && p.length === 3 ? at(p as Vec3, 0.5) : p);
    for (const key of ["center", "position"]) if (key in entry) entry[key] = shift(entry[key]);
    if (Array.isArray(entry.points)) entry.points = (entry.points as Vec3[]).map((p) => at(p, 0.5));
  }
  return entry;
}

/** The settings with an entry added; its index is where it lands. */
export function withAdded(settings: Settings, entries: Entries, light: boolean, entry: Entry): { patch: Partial<Settings>; id: number } {
  return light
    ? { patch: { addedLights: [...settings.addedLights, entry] }, id: entries.lights.length }
    : { patch: { addedObjects: [...settings.addedObjects, entry] }, id: entries.objects.length };
}

/** The settings with an entry deleted: hidden until saved, so no index moves. */
export function withRemoved(settings: Settings, light: boolean, id: number): Partial<Settings> {
  return light
    ? { removedLights: [...new Set([...settings.removedLights, id])].sort((a, b) => a - b) }
    : { removedObjects: [...new Set([...settings.removedObjects, id])].sort((a, b) => a - b) };
}
