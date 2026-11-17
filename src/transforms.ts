/**
 * Moving, turning and scaling an object or light of the scene file, as the gizmo and the G, R
 * and S keys do: each change, measured from where the drag began, becomes fields of its entry.
 *
 * Rotations are the engine's: degrees about X, then Y, then Z (`solid::rotate`), so a turn
 * about a world axis is composed as a matrix and read back as those three angles.
 */
import { anchorOf, entryValue, middle, type Vec3 } from "./objectFields";

type Matrix = [Vec3, Vec3, Vec3];
type Patch = Record<string, unknown>;

/** A change the gizmo makes, from where the drag began. */
export type Change =
  | { type: "move"; offset: Vec3 }
  /** About `axis`, a unit vector, by `radians`, right-handed. */
  | { type: "rotate"; axis: Vec3; radians: number }
  /** By `factor`, along one of the entry's own axes, or all of them when null. */
  | { type: "scale"; factor: number; axis: 0 | 1 | 2 | null };

export interface Transform {
  /** Where it is, and what the gizmo turns and scales it about. */
  point: Vec3;
  canRotate: boolean;
  /** Axes it can be scaled along by themselves; it always scales as a whole. */
  scaleAxes: (0 | 1 | 2)[];
  canScale: boolean;
  /** The fields a change gives it. */
  apply: (change: Change) => Patch;
}

const round = (v: number) => Math.round(v * 1e5) / 1e5;
const roundAll = (v: Vec3): Vec3 => v.map(round) as Vec3;
const add = (a: Vec3, b: Vec3, s = 1): Vec3 => [a[0] + b[0] * s, a[1] + b[1] * s, a[2] + b[2] * s];
const times = (m: Matrix, v: Vec3): Vec3 => m.map((row) => row[0] * v[0] + row[1] * v[1] + row[2] * v[2]) as Vec3;
const product = (a: Matrix, b: Matrix): Matrix =>
  a.map((row) => [0, 1, 2].map((j) => row[0] * b[0][j] + row[1] * b[1][j] + row[2] * b[2][j])) as Matrix;

/** The engine's rotation for angles in degrees: about X, then Y, then Z. */
export function eulerMatrix([x, y, z]: Vec3): Matrix {
  const [a, b, c] = [x, y, z].map((d) => (d * Math.PI) / 180);
  const rx: Matrix = [[1, 0, 0], [0, Math.cos(a), -Math.sin(a)], [0, Math.sin(a), Math.cos(a)]];
  const ry: Matrix = [[Math.cos(b), 0, Math.sin(b)], [0, 1, 0], [-Math.sin(b), 0, Math.cos(b)]];
  const rz: Matrix = [[Math.cos(c), -Math.sin(c), 0], [Math.sin(c), Math.cos(c), 0], [0, 0, 1]];
  return product(rz, product(ry, rx));
}

/** The angles in degrees, about X then Y then Z, of a rotation matrix. */
export function eulerAngles(m: Matrix): Vec3 {
  const sinY = Math.max(-1, Math.min(1, -m[2][0]));
  const y = Math.asin(sinY);
  let x: number;
  let z: number;
  if (Math.abs(sinY) < 0.99999) {
    x = Math.atan2(m[2][1], m[2][2]);
    z = Math.atan2(m[1][0], m[0][0]);
  } else {
    // Straight up or down, X and Z turn about the same axis: put it all in Z.
    x = 0;
    z = Math.atan2(-m[0][1], m[1][1]);
  }
  return [x, y, z].map((r) => round((r * 180) / Math.PI)) as Vec3;
}

/** A turn about a unit axis, right-handed. */
export function axisMatrix([x, y, z]: Vec3, radians: number): Matrix {
  const [c, s] = [Math.cos(radians), Math.sin(radians)];
  const t = 1 - c;
  return [
    [t * x * x + c, t * x * y - s * z, t * x * z + s * y],
    [t * x * y + s * z, t * y * y + c, t * y * z - s * x],
    [t * x * z - s * y, t * y * z + s * x, t * z * z + c],
  ];
}

/** Fields scaled by a factor, those present in the entry. */
function scaled(keys: string[], value: (key: string) => unknown, factor: number): Patch {
  const patch: Patch = {};
  for (const key of keys) {
    const v = value(key);
    if (typeof v === "number") patch[key] = round(v * factor);
    else if (Array.isArray(v)) patch[key] = (v as number[]).map((x) => round(x * factor));
  }
  return patch;
}

/** The sizes of each kind that scale with it, and which of them each of its own axes holds. */
const SIZES: Record<string, { keys: string[]; axes?: Record<number, string[]> }> = {
  sphere: { keys: ["radius"] },
  box: { keys: ["size"] },
  cylinder: { keys: ["radius", "height"], axes: { 0: ["radius"], 1: ["height"], 2: ["radius"] } },
  tube: { keys: ["radius", "inner_radius", "height"], axes: { 0: ["radius", "inner_radius"], 1: ["height"], 2: ["radius", "inner_radius"] } },
  cone: { keys: ["radius", "height"], axes: { 0: ["radius"], 1: ["height"], 2: ["radius"] } },
  torus: { keys: ["major_radius", "minor_radius"] },
  mesh: { keys: ["size"] },
  model: { keys: ["scale", "size"] },
  volume: { keys: ["size", "radius"] },
  point: { keys: ["radius"] },
  sphereLight: { keys: ["radius"] },
  spot: { keys: ["radius"] },
  ies: { keys: ["radius"] },
};

/** How an object or light moves, turns and scales, from its entry as edited now. */
export function transformOf(kind: string, source: Record<string, unknown>, edits: Record<string, unknown> | undefined, light: boolean)
  : Transform | null {
  const anchor = anchorOf(kind, source, edits, light);
  if (!anchor) return null;
  const value = (key: string) => entryValue(key, source, edits);
  const point = anchor.point;
  const sizeKind = light && kind === "sphere" ? "sphereLight" : kind;
  const sizes = SIZES[sizeKind];

  const move = (offset: Vec3) => anchor.moveTo(add(point, offset));

  // What turns: a rotation, a plane's corners, a ground's facing, a light's edges or aim.
  let canRotate = false;
  let turn: (m: Matrix) => Patch = () => ({});
  const rotation = value("rotation");
  const hasRotation = !light && ["box", "cylinder", "tube", "cone", "torus", "model", "volume", "mesh"].includes(kind);
  if (hasRotation) {
    canRotate = true;
    const start = eulerMatrix((Array.isArray(rotation) ? rotation : [0, 0, 0]) as Vec3);
    turn = (m) => ({ rotation: eulerAngles(product(m, start)) });
  } else if (kind === "plane") {
    const points = value("points") as Vec3[] | undefined;
    if (points) {
      canRotate = true;
      const center = middle(points);
      turn = (m) => ({ points: points.map((p) => roundAll(add(center, times(m, add(p, center, -1))))) });
    }
  } else if (kind === "ground") {
    canRotate = true;
    const normal = (value("normal") as Vec3 | undefined) ?? [0, 1, 0];
    turn = (m) => ({ normal: roundAll(times(m, normal)) });
  } else if (light && (kind === "rectangle" || kind === "portal")) {
    const [u, v] = [value("edge_u"), value("edge_v")] as (Vec3 | undefined)[];
    if (u && v) {
      canRotate = true;
      turn = (m) => ({ edge_u: roundAll(times(m, u)), edge_v: roundAll(times(m, v)) });
    }
  } else if (light && (kind === "spot" || kind === "ies")) {
    canRotate = true;
    const direction = (value("direction") as Vec3 | undefined) ?? [0, -1, 0];
    turn = (m) => ({ direction: roundAll(times(m, direction)) });
  }

  // What scales: its sizes, a plane's corners about their middle, a panel's edges.
  let canScale = false;
  let scaleAxes: (0 | 1 | 2)[] = [];
  let grow: (factor: number, axis: 0 | 1 | 2 | null) => Patch = () => ({});
  if (kind === "plane") {
    const points = value("points") as Vec3[] | undefined;
    if (points) {
      canScale = true;
      scaleAxes = [0, 1, 2];
      const center = middle(points);
      grow = (f, axis) => ({
        points: points.map((p) => roundAll(p.map((c, k) => (axis === null || axis === k ? center[k] + (c - center[k]) * f : c)) as Vec3)),
      });
    }
  } else if (light && (kind === "rectangle" || kind === "portal")) {
    const [u, v] = [value("edge_u"), value("edge_v")] as (Vec3 | undefined)[];
    if (u && v) {
      canScale = true;
      scaleAxes = [0, 1];
      grow = (f, axis) => ({
        ...(axis === null || axis === 0 ? { edge_u: roundAll(u.map((c) => c * f) as Vec3) } : {}),
        ...(axis === null || axis === 1 ? { edge_v: roundAll(v.map((c) => c * f) as Vec3) } : {}),
      });
    }
  } else if (sizes) {
    canScale = true;
    const size = value("size");
    if (Array.isArray(size) && (kind === "box" || kind === "volume")) scaleAxes = [0, 1, 2];
    else if (sizes.axes) scaleAxes = [0, 1, 2];
    // A model is sized by `size` when its file gives one, else by `scale`, 1 by default.
    const keys = kind === "model" ? (typeof size === "number" ? ["size"] : ["scale"]) : sizes.keys;
    const current = (key: string) => value(key) ?? (key === "scale" ? 1 : undefined);
    grow = (f, axis) => {
      if (axis === null) return scaled(keys, current, f);
      if (Array.isArray(size) && (kind === "box" || kind === "volume")) {
        return { size: (size as number[]).map((c, k) => round(k === axis ? c * f : c)) };
      }
      return scaled(sizes.axes?.[axis] ?? sizes.keys, value, f);
    };
  }

  return {
    point,
    canRotate,
    scaleAxes,
    canScale,
    apply: (change) => {
      if (change.type === "move") return move(change.offset);
      if (change.type === "rotate") return canRotate ? turn(axisMatrix(change.axis, change.radians)) : {};
      return canScale ? grow(Math.max(0.01, change.factor), change.axis) : {};
    },
  };
}

/**
 * A change as a 4 × 4 matrix, row by row, about `pivot`, for the 3D view to show it while it is
 * dragged. Scaling along an axis is shown along the world's, close enough for a preview.
 */
export function changeMatrix(change: Change, pivot: Vec3): number[][] {
  let linear: Matrix = [[1, 0, 0], [0, 1, 0], [0, 0, 1]];
  let offset: Vec3 = [0, 0, 0];
  if (change.type === "move") offset = change.offset;
  else if (change.type === "rotate") linear = axisMatrix(change.axis, change.radians);
  else {
    const f = Math.max(0.01, change.factor);
    linear = [0, 1, 2].map((r) => [0, 1, 2].map((c) => (r === c ? (change.axis === null || change.axis === r ? f : 1) : 0))) as Matrix;
  }
  // p' = L (p - pivot) + pivot + offset.
  const moved = add(add(pivot, times(linear, pivot), -1), offset);
  return [[...linear[0], moved[0]], [...linear[1], moved[1]], [...linear[2], moved[2]], [0, 0, 0, 1]];
}

/** A plane as the Object tab shows it: where its middle is, how it is turned from the file's
 * corners, in the engine's angles, and how long its first and last edges are. */
export interface PlaneState {
  position: Vec3;
  rotation: Vec3;
  width: number;
  depth: number;
}

const sub = (a: Vec3, b: Vec3): Vec3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const cross = (a: Vec3, b: Vec3): Vec3 => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const unit = (a: Vec3): Vec3 => {
  const length = Math.hypot(...a) || 1;
  return a.map((v) => v / length) as Vec3;
};

/** The plane's own axes, as rows: along its first edge, across it in its plane, and its normal. */
function frame(points: Vec3[]): Matrix {
  const u = unit(sub(points[1], points[0]));
  const n = unit(cross(u, sub(points[3], points[0])));
  return [u, cross(n, u), n];
}

/** A plane's state, its corners `current` measured against the file's, `original`. */
export function planeState(original: Vec3[], current: Vec3[]): PlaneState {
  const [from, to] = [frame(original), frame(current)];
  // The turn that takes the file's axes to the current ones: R = Toᵀ · From.
  const turn = [0, 1, 2].map((r) => [0, 1, 2].map((c) => to[0][r] * from[0][c] + to[1][r] * from[1][c] + to[2][r] * from[2][c])) as Matrix;
  return {
    position: middle(current).map(round) as Vec3,
    rotation: eulerAngles(turn),
    width: round(Math.hypot(...sub(current[1], current[0]))),
    depth: round(Math.hypot(...sub(current[3], current[0]))),
  };
}

/** The corners of a plane in `state`, built from the file's: turned, stretched and moved. */
export function planePoints(original: Vec3[], state: PlaneState): Vec3[] {
  const axes = frame(original);
  const center = middle(original);
  const [width, depth] = [Math.hypot(...sub(original[1], original[0])) || 1, Math.hypot(...sub(original[3], original[0])) || 1];
  const turn = eulerMatrix(state.rotation);
  return original.map((p) => {
    // Its place along the plane's own axes, stretched, then put back in the world, turned.
    const local = times(axes, sub(p, center));
    const stretched: Vec3 = [local[0] * (state.width / width), local[1] * (state.depth / depth), local[2]];
    const world: Vec3 = [0, 1, 2].map((k) => axes[0][k] * stretched[0] + axes[1][k] * stretched[1] + axes[2][k] * stretched[2]) as Vec3;
    return roundAll(add(state.position, times(turn, world)));
  });
}
