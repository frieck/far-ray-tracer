/**
 * Changing what kind of light an entry is, and aiming one: the fields of its entry in the
 * `.far` file that make a sphere light a spotlight, an IES fixture or an area light, and back,
 * keeping where it is, how big it looks and where it points.
 */
import { axisMatrix } from "./transforms";

type Vec3 = [number, number, number];
type Entry = Record<string, unknown>;

/** The kinds a light can switch between, as `lightKind` names them. */
export type SwitchableKind = "sphere" | "spot" | "ies" | "rectangle";

export const LIGHT_KINDS: { kind: SwitchableKind; label: string }[] = [
  { kind: "sphere", label: "Sphere light" },
  { kind: "spot", label: "Spotlight" },
  { kind: "ies", label: "IES light" },
  { kind: "rectangle", label: "Area light" },
];

/** Kinds a light can switch from: those, and a 2008 point light, which turns into a physical one. */
export const switchable = (kind: string) => kind === "point" || LIGHT_KINDS.some((k) => k.kind === kind);

/** Lights that point somewhere, and so can be aimed. */
export const aimable = (kind: string) => kind === "spot" || kind === "ies" || kind === "rectangle";

const round = (v: number) => Math.round(v * 1e5) / 1e5;
const sub = (a: Vec3, b: Vec3): Vec3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const dot = (a: Vec3, b: Vec3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a: Vec3, b: Vec3): Vec3 => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const scale = (a: Vec3, s: number): Vec3 => a.map((v) => round(v * s)) as Vec3;
const unit = (a: Vec3): Vec3 | null => {
  const length = Math.hypot(...a);
  return length > 1e-9 ? a.map((v) => v / length) as Vec3 : null;
};
const times = (m: Vec3[], v: Vec3): Vec3 => m.map((row) => dot(row, v)) as Vec3;
const vector = (value: unknown): Vec3 | null => (Array.isArray(value) && value.length === 3 ? value as Vec3 : null);

/** Where a light shines: a panel's front, a spotlight's or fixture's aim, else down. */
function facing(entry: Entry): Vec3 {
  const [u, v] = [vector(entry.edge_u), vector(entry.edge_v)];
  if (entry.type === "rectangle" && u && v) return unit(cross(u, v)) ?? [0, -1, 0];
  return unit(vector(entry.direction) ?? [0, -1, 0]) ?? [0, -1, 0];
}

/** A sphere's radius, or half a panel's mean side, so it looks about as big. */
function radiusOf(entry: Entry): number {
  if (typeof entry.radius === "number") return entry.radius;
  const [u, v] = [vector(entry.edge_u), vector(entry.edge_v)];
  return u && v ? round((Math.hypot(...u) + Math.hypot(...v)) / 4) : 0.1;
}

/** A square panel of side `side` whose front faces `normal`: edge_u × edge_v points along it. */
function panel(normal: Vec3, side: number): { edge_u: Vec3; edge_v: Vec3 } {
  const helper: Vec3 = Math.abs(normal[1]) > 0.9 ? [1, 0, 0] : [0, 1, 0];
  const u = unit(cross(helper, normal)) ?? [1, 0, 0];
  const v = cross(normal, u);
  return { edge_u: scale(u, side), edge_v: scale(v, side) };
}

/** Fields only some kinds have, taken away unless the new kind sets them. */
const NONE: Entry = { radius: null, direction: null, spot_angle: null, spot_blend: null, ies: null, edge_u: null, edge_v: null };

/** A 2008 light's default radius, as scene_file.rs reads it. */
const POINT_RADIUS = 0.2;

/** A power in watts kept to four significant digits, so files stay readable. */
const watts = (power: number) => Number(power.toPrecision(4));

/** A light as edited: its entry with the edits over it, those set to null taken away. */
export function editedEntry(source: Entry, edits: Entry | undefined): Entry {
  const entry: Entry = { ...source, ...edits };
  for (const [key, value] of Object.entries(entry)) if (value === null) delete entry[key];
  return entry;
}

/**
 * The light a lamp gives in all, in watts, as scene_file.rs reads it: its `power`, or from its
 * `intensity`, its surface's glow, times π times its area for a panel and 4π² r² for a sphere,
 * in metres, `unit` to a scene unit. Null for a 2008 light or a portal.
 */
export function powerOf(entry: Entry, unit: number): number | null {
  if (typeof entry.power === "number") return entry.power;
  const intensity = typeof entry.intensity === "number" ? entry.intensity : 1;
  const [u, v] = [vector(entry.edge_u), vector(entry.edge_v)];
  if (entry.type === "rectangle" && u && v) return Math.PI * Math.hypot(...u) * Math.hypot(...v) * unit * unit * intensity;
  if (entry.type === "sphere" && typeof entry.radius === "number") return 4 * Math.PI * Math.PI * (entry.radius * unit) ** 2 * intensity;
  return null;
}

/**
 * An edit of a lamp that still gives its light by intensity, made to give it by power, as it
 * is now: so resizing it, or switching its kind, keeps the light it gives.
 */
export function inWatts(entry: Entry, patch: Entry, unit: number): Entry {
  if (typeof entry.power === "number" || "power" in patch || "intensity" in patch) return patch;
  const power = powerOf(entry, unit);
  return power === null ? patch : { power: watts(power), intensity: null, ...patch };
}

/**
 * A 2008 point light turned into a physical one: its color, out of its old material, and the
 * power that gives the same light at the scene's center, `center`. A 2008 light does not fade
 * with distance and a physical one does, so they agree there and drift apart elsewhere. A
 * diffuse surface `d` away takes color × glow × r²/d² from a sphere light of radius r, and
 * color × glow × area/(π d²) from a panel facing it, against the 2008 light's color: a glow of
 * d²/r² or π d²/area, which is a power of 4π² d² or π² d², whatever its size, d in metres,
 * `unit` to a scene unit. Classic mode's classic_light_scales undoes it, so the 2008 stage
 * renders it as before.
 */
function fromPoint(entry: Entry, to: SwitchableKind, center: Vec3 | null, unit: number): Entry {
  const material = entry.material as Entry | undefined;
  const color = vector(entry.color) ?? vector(material?.color) ?? [1, 1, 1];
  const radius = typeof entry.radius === "number" ? entry.radius : POINT_RADIUS;
  const position = vector(entry.position) ?? [0, 0, 0];
  const distance = center ? Math.hypot(...sub(position, center)) : 10 * radius;
  const squared = Math.max((distance * unit) ** 2, 1e-12);
  const power = (to === "rectangle" ? Math.PI * Math.PI : 4 * Math.PI * Math.PI) * squared;
  return { material: null, color, power: watts(power), intensity: null, radius };
}

/**
 * The edit that turns `entry`, a light as edited, into `to`; `ies` is the profile's file for
 * an IES light. Its place, color and power stay; a 2008 light's color and brightness are
 * carried over as `fromPoint` says, measured at the scene's `center`.
 */
export function switchKind(entry: Entry, to: SwitchableKind, ies?: string, center: Vec3 | null = null, unit = 1): Entry {
  if (entry.type === undefined) {
    const carried = fromPoint(entry, to, center, unit);
    return { ...switchKind({ ...entry, ...carried, type: "sphere" }, to, ies), ...carried, ...(to === "rectangle" ? { radius: null } : {}) };
  }
  const direction = facing(entry).map(round) as Vec3;
  const radius = radiusOf(entry);
  switch (to) {
    case "sphere":
      // A direction alone changes nothing, and is kept for turning it back into a spotlight.
      return { ...NONE, type: "sphere", radius, direction: entry.type === "sphere" ? entry.direction ?? null : direction };
    case "spot":
      return { ...NONE, type: "sphere", radius, direction, spot_angle: typeof entry.spot_angle === "number" ? entry.spot_angle : 45,
        spot_blend: entry.spot_blend ?? null };
    case "ies":
      return { ...NONE, type: "sphere", radius, direction, ies: ies ?? entry.ies ?? null };
    case "rectangle":
      return { ...NONE, type: "rectangle", ...panel(direction, round(Math.max(2 * radius, 1e-3))) };
  }
}

/**
 * The edit that points `entry`, a light as edited, at `target`: a spotlight's or fixture's
 * direction, a panel turned so its front faces it. Null when it is where the target is.
 */
export function aimAt(entry: Entry, target: Vec3): Entry | null {
  const position = vector(entry.position);
  if (!position) return null;
  const toward = unit(sub(target, position));
  if (!toward) return null;
  if (entry.type !== "rectangle") return { direction: toward.map(round) };
  const [u, v] = [vector(entry.edge_u), vector(entry.edge_v)];
  if (!u || !v) return null;
  // The shortest turn from where its front faces now to the target.
  const now = facing(entry);
  const cosine = Math.max(-1, Math.min(1, dot(now, toward)));
  let axis = unit(cross(now, toward));
  if (!axis) {
    if (cosine > 0) return null;
    // Facing straight away: turn half round any axis across it.
    axis = unit(cross(now, Math.abs(now[0]) < 0.9 ? [1, 0, 0] : [0, 1, 0]))!;
  }
  const turn = axisMatrix(axis, Math.acos(cosine));
  return { edge_u: times(turn, u).map(round), edge_v: times(turn, v).map(round) };
}

/** A file's name, without its folders. */
export const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;
