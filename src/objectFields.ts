/**
 * What the Object tab edits for each kind of object and light: fields of its entry in the
 * `.far` file, which the engine reads again with the edits; see `Scene::edited`. A field's
 * value is the edit, else the file's, else the default the format gives.
 */

export type Vec3 = [number, number, number];

export type Field =
  | { type: "vector"; key: string; label: string; default: Vec3; min: number; max: number; step: number; hard?: [number, number]; hint?: string; angle?: boolean }
  | { type: "number"; key: string; label: string; default: number; min: number; max: number; step: number; hard?: [number, number]; strict?: boolean; log?: boolean; hint?: string; angle?: boolean }
  | { type: "color"; key: string; label: string; default: Vec3 }
  /** A plane's four corners, moved together by where their middle goes. */
  | { type: "corners"; key: string; label: string }
  /** One of a rectangular light's edges, by its length; its direction stays. */
  | { type: "edge"; key: string; label: string };

/** The usual reach of a drag; typing goes past it. */
const PLACE = { min: -20, max: 20, step: 0.01, hard: [-1e6, 1e6] as [number, number] };
const SIZE = { min: 0.01, max: 20, step: 0.01, hard: [1e-4, 1e6] as [number, number] };

const at = (key: string, label = "Position"): Field => ({ type: "vector", key, label, default: [0, 0, 0], ...PLACE });
const turned: Field = {
  type: "vector", key: "rotation", label: "Rotation", default: [0, 0, 0], min: -180, max: 180, step: 1, hard: [-3600, 3600], angle: true,
  hint: "Degrees around X, then Y, then Z.",
};
const length = (key: string, label: string): Field => ({ type: "number", key, label, default: 1, ...SIZE });
const color: Field = { type: "color", key: "color", label: "Color", default: [1, 1, 1] };
const intensity: Field = {
  type: "number", key: "intensity", label: "Intensity", default: 1, min: 0, max: 50, step: 0.05, hard: [0, 1e6],
  hint: "The light's strength, times its color.",
};
const aimed: Field = {
  type: "vector", key: "direction", label: "Direction", default: [0, -1, 0], min: -1, max: 1, step: 0.01, hard: [-1e3, 1e3],
  hint: "Where it points; straight down by default.",
};

export const OBJECT_FIELDS: Record<string, Field[]> = {
  sphere: [at("center"), length("radius", "Radius")],
  box: [at("center"), { type: "vector", key: "size", label: "Size", default: [1, 1, 1], ...SIZE }, turned],
  cylinder: [at("center"), length("radius", "Radius"), length("height", "Height"), turned],
  tube: [at("center"), length("radius", "Radius"), length("inner_radius", "Inner radius"), length("height", "Height"), turned],
  cone: [at("center"), length("radius", "Radius"), length("height", "Height"), turned],
  torus: [at("center"), length("major_radius", "Ring radius"), length("minor_radius", "Tube radius"), turned],
  ground: [at("position", "Height point"), { type: "vector", key: "normal", label: "Facing", default: [0, 1, 0], min: -1, max: 1, step: 0.01, hard: [-1e3, 1e3] }],
  plane: [{ type: "corners", key: "points", label: "Position" }],
  mesh: [at("position"), length("size", "Size")],
  model: [at("position"), turned, { type: "number", key: "scale", label: "Scale", default: 1, min: 0.01, max: 10, step: 0.01, hard: [1e-4, 1e6], log: true }],
  volume: [
    at("center"), turned,
    { type: "number", key: "density", label: "Density", default: 1, min: 0, max: 5, step: 0.01, hard: [0, 1e4] },
    color,
    { type: "number", key: "anisotropy", label: "Anisotropy", default: 0, min: -0.95, max: 0.95, step: 0.01, strict: true,
      hint: "Above 0, light scatters forward, as in haze; below, back." },
  ],
};

export const LIGHT_FIELDS: Record<string, Field[]> = {
  point: [at("position"), length("radius", "Radius"), color],
  rectangle: [at("position"), color, intensity, { type: "edge", key: "edge_u", label: "Width" }, { type: "edge", key: "edge_v", label: "Height" }],
  sphere: [at("position"), length("radius", "Radius"), color, intensity],
  spot: [
    at("position"), length("radius", "Radius"), color, intensity, aimed,
    { type: "number", key: "spot_angle", label: "Cone", default: 45, min: 1, max: 179, step: 1, strict: true, angle: true,
      hint: "The cone's full angle, in degrees." },
    { type: "number", key: "spot_blend", label: "Soft edge", default: 0.15, min: 0, max: 1, step: 0.01, strict: true,
      hint: "The share of the cone that fades out at its edge." },
  ],
  ies: [at("position"), length("radius", "Radius"), color, intensity, aimed],
};

/**
 * The fields an entry shows: a model sized by `size` shows that instead of its scale; a
 * volume its box or its radius, as its file gives it.
 */
export function fieldsFor(kind: string, source: Record<string, unknown>, light: boolean): Field[] {
  const fields = (light ? LIGHT_FIELDS : OBJECT_FIELDS)[kind] ?? [];
  if (kind === "model" && typeof source.size === "number") {
    return fields.map((f) => (f.key === "scale" ? { ...length("size", "Size"), hint: "Its longest side, in scene units." } : f));
  }
  if (kind === "volume") {
    const extent = Array.isArray(source.size)
      ? { type: "vector", key: "size", label: "Size", default: [1, 1, 1], ...SIZE } as Field
      : length("radius", "Radius");
    return [fields[0], extent, ...fields.slice(1)];
  }
  return fields;
}

/** A field's value: the edit, else the file's, else the format's default. */
export function valueOf(field: Field, source: Record<string, unknown>, edits: Record<string, unknown> | undefined): unknown {
  const value = edits?.[field.key] ?? source[field.key];
  if (value !== undefined) return value;
  return "default" in field ? field.default : undefined;
}

/** The middle of a plane's corners. */
export function middle(points: Vec3[]): Vec3 {
  const sum = points.reduce<Vec3>((s, p) => [s[0] + p[0], s[1] + p[1], s[2] + p[2]], [0, 0, 0]);
  return sum.map((v) => v / Math.max(1, points.length)) as Vec3;
}

/** The corners moved so their middle is `to`. */
export function movedCorners(points: Vec3[], to: Vec3): Vec3[] {
  const from = middle(points);
  return points.map((p) => p.map((v, k) => round(v + to[k] - from[k])) as Vec3);
}

/** An edge vector with a new length, its direction kept. */
export function resizedEdge(edge: Vec3, size: number): Vec3 {
  const current = Math.hypot(...edge) || 1;
  return edge.map((v) => round((v / current) * size)) as Vec3;
}

/**
 * Where an object or light is, for the move gizmo, and the fields that put it elsewhere:
 * its center or position, or the middle of a plane's corners. Null for fog, which has none.
 */
export function anchorOf(kind: string, source: Record<string, unknown>, edits: Record<string, unknown> | undefined, light: boolean)
  : { point: Vec3; moveTo: (point: Vec3) => Record<string, unknown> } | null {
  const fields = fieldsFor(kind, source, light);
  const first = fields[0];
  if (!first) return null;
  if (first.type === "corners") {
    const points = (edits?.points ?? source.points) as Vec3[] | undefined;
    if (!points) return null;
    return { point: middle(points), moveTo: (point) => ({ points: movedCorners(points, point) }) };
  }
  if (first.type !== "vector") return null;
  const point = valueOf(first, source, edits) as Vec3;
  return { point, moveTo: (to) => ({ [first.key]: to.map(round) }) };
}

/** Rounds away float noise, so files stay readable. */
function round(v: number): number {
  return Math.round(v * 1e5) / 1e5;
}
