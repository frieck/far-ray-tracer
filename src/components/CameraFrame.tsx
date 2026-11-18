import { cameraAxes, viewHalfSize } from "../orbit";
import type { Pose } from "../types";

type Vec = [number, number, number];

/** The engine's distance from the eye to the image plane, in orbit.ts's units. */
const IMAGE_DISTANCE = 5;
/** How long the drawn camera is, as a share of its distance to what it looks at. */
const LENGTH = 0.15;
/** Points nearer the view than this, along its axis, are cut away. */
const NEAR = 1e-3;

const add = (a: Vec, b: Vec, scale = 1): Vec => [a[0] + b[0] * scale, a[1] + b[1] * scale, a[2] + b[2] * scale];
const dot = (a: Vec, b: Vec) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const mix = (a: Vec, b: Vec, t: number): Vec => add(a, add(b, a, -1), t);

/**
 * The scene camera as lines in space: from its eye to the corners of its picture, the picture's
 * edge, and a triangle on top that says which way is up. Its size follows how far it looks.
 */
export function cameraLines(camera: Pose, aspect: number): [Vec, Vec][] {
  const { right, up, forward } = cameraAxes(camera);
  const [halfWidth, halfHeight] = viewHalfSize(aspect).map((h) => h / camera.zoom);
  const reach = Math.hypot(...add(camera.target, camera.position, -1)) * LENGTH;
  const scale = reach / IMAGE_DISTANCE;
  const corner = (x: number, y: number): Vec =>
    add(add(add(camera.position, forward, reach), right, x * halfWidth * scale), up, y * halfHeight * scale);
  const corners = [corner(-1, 1), corner(1, 1), corner(1, -1), corner(-1, -1)];
  const lines: [Vec, Vec][] = corners.map((c) => [camera.position, c]);
  corners.forEach((c, k) => lines.push([c, corners[(k + 1) % 4]]));
  const [left, top] = [corner(-0.5, 1.1), corner(0, 1.6)];
  lines.push([left, top], [top, corner(0.5, 1.1)], [corner(0.5, 1.1), left]);
  return lines;
}

/**
 * A line in space as the view sees it, from 0 to 1 across and down, as orbit.ts's `project`;
 * the part behind the view is cut away, and null when none of it is in front.
 */
export function projectLine(view: Pose, aspect: number, [a, b]: [Vec, Vec]): [number, number, number, number] | null {
  const { right, up, forward } = cameraAxes(view);
  const depth = (p: Vec) => dot(add(p, view.position, -1), forward);
  let [da, db] = [depth(a), depth(b)];
  if (da <= NEAR && db <= NEAR) return null;
  if (da <= NEAR) [a, da] = [mix(a, b, (NEAR - da) / (db - da)), NEAR];
  if (db <= NEAR) [b, db] = [mix(b, a, (NEAR - db) / (da - db)), NEAR];
  const [halfWidth, halfHeight] = viewHalfSize(aspect).map((h) => h / view.zoom);
  const flat = (p: Vec, d: number): [number, number] => {
    const relative = add(p, view.position, -1);
    const s = IMAGE_DISTANCE / d;
    return [(dot(relative, right) * s / halfWidth + 1) / 2, (1 - dot(relative, up) * s / halfHeight) / 2];
  };
  return [...flat(a, da), ...flat(b, db)];
}

/** The scene camera drawn over the image while the view looks from elsewhere. */
export function CameraFrame({ camera, view, aspect }: { camera: Pose; view: Pose; aspect: number }) {
  const lines = cameraLines(camera, aspect).map((line) => projectLine(view, aspect, line)).filter((l) => l !== null);
  return (
    <svg className="camera-frame" viewBox="0 0 1 1" preserveAspectRatio="none" aria-hidden="true">
      {lines.map(([x1, y1, x2, y2], k) => <line key={k} x1={x1} y1={y1} x2={x2} y2={y2} />)}
    </svg>
  );
}
