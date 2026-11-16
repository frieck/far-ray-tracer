import { useEffect, useRef, useState, type PointerEvent } from "react";
import { project, rayThrough, unitsPerPixel } from "../orbit";
import type { Pose } from "../types";

type Vec3 = [number, number, number];
type AxisName = "X" | "Y" | "Z";

const AXES: { name: AxisName; direction: Vec3; color: string }[] = [
  { name: "X", direction: [1, 0, 0], color: "#ff5468" },
  { name: "Y", direction: [0, 1, 0], color: "#7bd34f" },
  { name: "Z", direction: [0, 0, 1], color: "#4f8fff" },
];

/** An arrow's length on screen, in CSS pixels, and the shortest that still shows. */
const ARROW = 72;
const SHORTEST = 14;

interface MoveGizmoProps {
  pose: Pose;
  /** The image's width over its height. */
  aspect: number;
  /** Where the object or light is. */
  point: Vec3;
  /** While moving: how far it has gone from where the move began; null once it ends. */
  onDrag: (offset: Vec3 | null) => void;
  /** The move ended with the object at this point. */
  onMove: (point: Vec3) => void;
  /** The move was called off: the object stays where it was. */
  onCancel: () => void;
}

const add = (a: Vec3, b: Vec3, s = 1): Vec3 => [a[0] + b[0] * s, a[1] + b[1] * s, a[2] + b[2] * s];
const dot = (a: Vec3, b: Vec3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

/** A move in progress: along an axis or across the view, from a point and a mouse position. */
interface Move {
  axis: AxisName | null;
  start: Vec3;
  /** Where the mouse was when it began, as a point of the image from 0 to 1. */
  from: [number, number];
  offset: Vec3;
  /** Begun from the keyboard, it follows the mouse with no button held, until a click. */
  keyboard: boolean;
}

/** Whether a key goes to a field being typed in rather than to the gizmo. */
function typing(target: EventTarget | null): boolean {
  const element = target as HTMLElement | null;
  return !!element && (element.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(element.tagName));
}

/**
 * Arrows along X, Y and Z at the selection, as in Blender: drag one to move along it, or the
 * ring in the middle to move across the view. While moving, X, Y or Z locks the move to that
 * axis, and the same key again frees it; with nothing moving, X, Y or Z starts a move along it
 * and G a free one, which follow the mouse until a click or Enter, while Esc calls them off.
 * Shift moves in steps of a tenth of a unit.
 */
export function MoveGizmo(props: MoveGizmoProps) {
  const { pose, aspect, point } = props;
  const svg = useRef<SVGSVGElement>(null);
  const [size, setSize] = useState<[number, number]>([0, 0]);
  const [move, setMove] = useState<Move | null>(null);
  const moving = useRef<Move | null>(null);
  // The latest values, for the window's listeners.
  const latest = useRef(props);
  latest.current = props;
  const mouse = useRef<[number, number] | null>(null);
  const shift = useRef(false);

  useEffect(() => {
    const element = svg.current;
    if (!element) return;
    const measure = () => {
      const box = element.getBoundingClientRect();
      setSize([box.width, box.height]);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  /** A mouse position as a point of the image, from 0 to 1. */
  const fraction = (x: number, y: number): [number, number] | null => {
    const box = svg.current?.getBoundingClientRect();
    return box && box.width > 0 ? [(x - box.left) / box.width, (y - box.top) / box.height] : null;
  };

  /** How far the move has gone with the mouse at `at`: along its axis, or across the view. */
  const offsetAt = (current: Move, at: [number, number]): Vec3 | null => {
    const { pose, aspect } = latest.current;
    const forward = add(pose.target, pose.position, -1);
    if (current.axis) {
      const axis = AXES.find((a) => a.name === current.axis)!.direction;
      // Where along the axis through the start each mouse ray passes closest.
      const along = (where: [number, number]) => {
        const { origin, direction } = rayThrough(pose, aspect, ...where);
        const b = dot(axis, direction);
        const denominator = 1 - b * b;
        if (denominator < 1e-6) return null;
        const w = add(current.start, origin, -1);
        return (b * dot(direction, w) - dot(axis, w)) / denominator;
      };
      const [t0, t1] = [along(current.from), along(at)];
      return t0 === null || t1 === null ? null : axis.map((v) => v * (t1 - t0)) as Vec3;
    }
    // Where each mouse ray meets the plane through the start that faces the camera.
    const plane = (where: [number, number]) => {
      const { origin, direction } = rayThrough(pose, aspect, ...where);
      const facing = dot(direction, forward);
      if (Math.abs(facing) < 1e-9) return null;
      const t = dot(add(current.start, origin, -1), forward) / facing;
      return t > 0 ? add(origin, direction, t) : null;
    };
    const [a, b] = [plane(current.from), plane(at)];
    return a && b ? add(b, a, -1) : null;
  };

  const follow = (current: Move, at: [number, number] | null) => {
    if (!at) return;
    let offset = offsetAt(current, at);
    if (!offset) return;
    if (shift.current) offset = offset.map((v) => Math.round(v * 10) / 10) as Vec3;
    current.offset = offset;
    latest.current.onDrag(offset);
  };

  const begin = (axis: AxisName | null, at: [number, number], keyboard: boolean) => {
    const current: Move = { axis, start: latest.current.point, from: at, offset: [0, 0, 0], keyboard };
    moving.current = current;
    setMove({ ...current });
    latest.current.onDrag([0, 0, 0]);
  };

  const finish = (keep: boolean) => {
    const current = moving.current;
    if (!current) return;
    moving.current = null;
    setMove(null);
    latest.current.onDrag(null);
    if (keep && current.offset.some((v) => v !== 0)) latest.current.onMove(add(current.start, current.offset));
    else latest.current.onCancel();
  };

  /** Locks the move to an axis, or frees it when it is locked to that one already. */
  const lock = (axis: AxisName) => {
    const current = moving.current;
    if (!current) return;
    current.axis = current.axis === axis ? null : axis;
    setMove({ ...current });
    follow(current, mouse.current ?? current.from);
  };

  // The keyboard, and the mouse for a move begun from it.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      shift.current = event.shiftKey;
      if (typing(event.target) || event.metaKey || event.ctrlKey || event.altKey) return;
      const key = event.key.toUpperCase();
      const current = moving.current;
      if (current) {
        if (key === "X" || key === "Y" || key === "Z") lock(key);
        else if (event.key === "Enter") finish(true);
        else if (event.key === "Escape") finish(false);
        else if (event.key !== "Shift") return;
        event.preventDefault();
        // The app's own Escape would also clear the selection.
        event.stopPropagation();
        return;
      }
      if (event.repeat || !(key === "X" || key === "Y" || key === "Z" || key === "G")) return;
      const { pose, aspect, point } = latest.current;
      // From the mouse when it is over the image, else from the object itself.
      const at = mouse.current ?? project(pose, aspect, point);
      if (!at) return;
      event.preventDefault();
      begin(key === "G" ? null : key, at, true);
    };
    const onKeyUp = (event: KeyboardEvent) => {
      shift.current = event.shiftKey;
    };
    const onPointerMove = (event: globalThis.PointerEvent) => {
      shift.current = event.shiftKey;
      const at = fraction(event.clientX, event.clientY);
      const box = svg.current?.getBoundingClientRect();
      const inside = box && event.clientX >= box.left && event.clientX <= box.right && event.clientY >= box.top && event.clientY <= box.bottom;
      mouse.current = inside ? at : moving.current ? at : null;
      if (moving.current?.keyboard) follow(moving.current, at);
    };
    // A move begun from the keyboard ends with a click, which then does nothing else.
    const onPointerDown = (event: globalThis.PointerEvent) => {
      if (!moving.current?.keyboard) return;
      event.preventDefault();
      event.stopPropagation();
      finish(event.button === 0);
      const swallow = (click: MouseEvent) => {
        click.stopPropagation();
        click.preventDefault();
      };
      window.addEventListener("click", swallow, { capture: true, once: true });
      window.addEventListener("contextmenu", swallow, { capture: true, once: true });
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("keyup", onKeyUp, true);
    window.addEventListener("pointermove", onPointerMove, true);
    window.addEventListener("pointerdown", onPointerDown, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("keyup", onKeyUp, true);
      window.removeEventListener("pointermove", onPointerMove, true);
      window.removeEventListener("pointerdown", onPointerDown, true);
    };
    // The listeners read everything that changes through refs.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // A move still going when the gizmo goes away, as when the selection changes, is called off.
  useEffect(() => () => {
    if (moving.current) {
      moving.current = null;
      latest.current.onDrag(null);
      latest.current.onCancel();
    }
  }, []);

  const [width, height] = size;
  const center = project(pose, aspect, point);
  if (!center || width === 0) return <svg ref={svg} className="move-gizmo" aria-hidden="true" />;
  const [cx, cy] = [center[0] * width, center[1] * height];
  const reach = ARROW * unitsPerPixel(pose, aspect, point, height);

  const startDrag = (event: PointerEvent<SVGElement>, axis: AxisName | null) => {
    if (moving.current) return;
    event.stopPropagation();
    event.preventDefault();
    const at = fraction(event.clientX, event.clientY);
    if (!at) return;
    try {
      (event.currentTarget as Element).setPointerCapture(event.pointerId);
    } catch {
      // A pointer the browser no longer tracks: the drag still follows its events here.
    }
    begin(axis, at, false);
  };
  const dragging = (event: PointerEvent<SVGElement>) => {
    const current = moving.current;
    if (!current || current.keyboard) return;
    shift.current = event.shiftKey;
    follow(current, fraction(event.clientX, event.clientY));
  };
  const endDrag = (event: PointerEvent<SVGElement>) => {
    if (!moving.current || moving.current.keyboard) return;
    event.stopPropagation();
    finish(true);
  };
  const handlers = { onPointerMove: dragging, onPointerUp: endDrag, onPointerCancel: endDrag };

  // The line a locked move runs along, through where it began, across the image.
  const locked = move?.axis ? AXES.find((a) => a.name === move.axis)! : null;
  let guide: [number, number, number, number] | null = null;
  if (locked && move) {
    const far = reach * 400;
    const a = project(pose, aspect, add(move.start, locked.direction, -far)) ?? project(pose, aspect, move.start);
    const b = project(pose, aspect, add(move.start, locked.direction, far)) ?? project(pose, aspect, move.start);
    if (a && b) guide = [a[0] * width, a[1] * height, b[0] * width, b[1] * height];
  }

  return (
    <svg ref={svg} className="move-gizmo" viewBox={`0 0 ${width} ${height}`} data-dragging={move ? (move.axis ?? "view") : undefined}>
      {guide && locked && (
        <line x1={guide[0]} y1={guide[1]} x2={guide[2]} y2={guide[3]} stroke={locked.color} strokeWidth={1.5} opacity={0.75}
          className="move-gizmo-guide" />
      )}
      {AXES.map(({ name, direction, color }) => {
        const tip = project(pose, aspect, add(point, direction, reach));
        if (!tip) return null;
        const [tx, ty] = [tip[0] * width, tip[1] * height];
        const length = Math.hypot(tx - cx, ty - cy);
        // An axis pointing at the camera has nothing to drag along.
        if (length < SHORTEST) return null;
        const [ux, uy] = [(tx - cx) / length, (ty - cy) / length];
        const head = `${tx},${ty} ${tx - ux * 11 - uy * 5},${ty - uy * 11 + ux * 5} ${tx - ux * 11 + uy * 5},${ty - uy * 11 - ux * 5}`;
        const lit = move?.axis === name;
        return (
          <g key={name} className="move-gizmo-axis" data-active={lit || undefined}
            onPointerDown={(event) => startDrag(event, name)} {...handlers}>
            <title>{`Drag to move along ${name}, or press ${name}`}</title>
            <line x1={cx} y1={cy} x2={tx} y2={ty} className="move-gizmo-hit" />
            <line x1={cx + ux * 10} y1={cy + uy * 10} x2={tx - ux * 9} y2={ty - uy * 9} stroke={color} strokeWidth={lit ? 3.5 : 2.5}
              strokeLinecap="round" />
            <polygon points={head} fill={color} />
          </g>
        );
      })}
      <g className="move-gizmo-axis" data-active={(move && !move.axis) || undefined} onPointerDown={(event) => startDrag(event, null)}
        {...handlers}>
        <title>Drag to move across the view, or press G</title>
        <circle cx={cx} cy={cy} r={12} className="move-gizmo-hit" />
        <circle cx={cx} cy={cy} r={7} fill="none" stroke="#fff" strokeWidth={move && !move.axis ? 2.5 : 1.8} />
      </g>
      {move && (
        <text x={cx + 16} y={cy + 28} className="move-gizmo-hint">
          {move.axis ? `Along ${move.axis}` : "Across the view"}
          {` · ${move.axis ? `${move.axis} again frees it` : "X, Y or Z locks an axis"}`}
          {move.keyboard ? " · click to place, Esc to cancel" : ""}
        </text>
      )}
    </svg>
  );
}
