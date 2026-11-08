import type { Pose } from "./types";

type Vec = [number, number, number];

const ORBIT_SPEED = 0.008;
const DOLLY_SPEED = 0.0015;
const MAX_PITCH = 1.5;
const MIN_DISTANCE = 0.2;
const IMAGE_DISTANCE = 5;

const add = (a: Vec, b: Vec, scale = 1): Vec => [a[0] + b[0] * scale, a[1] + b[1] * scale, a[2] + b[2] * scale];
const dot = (a: Vec, b: Vec) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const length = (a: Vec) => Math.sqrt(dot(a, a));
const unit = (a: Vec): Vec => {
  const size = length(a) || 1;
  return [a[0] / size, a[1] / size, a[2] / size];
};
const cross = (a: Vec, b: Vec): Vec => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
];

/** Half size of the image plane, matching the engine's `view_half_size`. */
export function viewHalfSize(aspect: number): [number, number] {
  return aspect >= 4 / 3 ? [3 * aspect, 3] : [4, 4 / aspect];
}

/** Moves the target along the view axis to the depth of `center`, without changing the image. */
export function retarget(pose: Pose, center: Vec | null): Pose {
  if (!center) return pose;
  const forward = unit(add(pose.target, pose.position, -1));
  const depth = dot(add(center, pose.position, -1), forward);
  return depth > MIN_DISTANCE ? { ...pose, target: add(pose.position, forward, depth) } : pose;
}

/** Turns the camera around its target. Dragging right turns the scene to the right. */
export function orbit(pose: Pose, dx: number, dy: number): Pose {
  const offset = add(pose.position, pose.target, -1);
  const distance = length(offset);
  const yaw = Math.atan2(offset[0], offset[2]) + dx * ORBIT_SPEED;
  const pitch = Math.asin(offset[1] / distance) + dy * ORBIT_SPEED;
  const limited = Math.min(Math.max(pitch, -MAX_PITCH), MAX_PITCH);
  const flat = Math.cos(limited) * distance;
  return { ...pose, position: add(pose.target, [Math.sin(yaw) * flat, Math.sin(limited) * distance, Math.cos(yaw) * flat]) };
}

/** Slides the camera so the point under the cursor follows it. */
export function pan(pose: Pose, dx: number, dy: number, stageHeight: number, aspect: number): Pose {
  const forward = unit(add(pose.target, pose.position, -1));
  const right = unit(cross([0, 1, 0], forward));
  const up = cross(forward, right);
  const halfHeight = viewHalfSize(aspect)[1] / pose.zoom;
  const distance = length(add(pose.target, pose.position, -1));
  const perPixel = (2 * halfHeight * distance) / (IMAGE_DISTANCE * stageHeight);
  const move = add(add([0, 0, 0], right, -dx * perPixel), up, dy * perPixel);
  return { ...pose, position: add(pose.position, move), target: add(pose.target, move) };
}

/** Moves the camera toward or away from its target. */
export function dolly(pose: Pose, amount: number): Pose {
  const offset = add(pose.position, pose.target, -1);
  const distance = Math.max(length(offset) * Math.exp(amount * DOLLY_SPEED), MIN_DISTANCE);
  return { ...pose, position: add(pose.target, unit(offset), distance) };
}

/** The camera's right, up and forward directions, as the engine builds them. */
export function cameraAxes(pose: Pose): { right: Vec; up: Vec; forward: Vec } {
  const forward = unit(add(pose.target, pose.position, -1));
  const right = unit(cross([0, 1, 0], forward));
  return { right, up: cross(forward, right), forward };
}

/**
 * Puts the camera on `axis` from its target, at the same distance, looking back at the target.
 * Straight up or down is tilted to the orbit's pitch limit, where "up" on screen is still defined.
 */
export function alignView(pose: Pose, axis: Vec): Pose {
  const distance = length(add(pose.position, pose.target, -1));
  const direction: Vec = axis[1] === 0 ? axis : [0, Math.sign(axis[1]) * Math.sin(MAX_PITCH), -Math.cos(MAX_PITCH)];
  return { ...pose, position: add(pose.target, direction, distance) };
}
