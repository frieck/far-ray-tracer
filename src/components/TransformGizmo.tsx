import { Move3d, Rotate3d, Scale3d } from "lucide-react";
import { useEffect, useRef, useState, type PointerEvent, type ReactNode } from "react";
import { project, rayThrough, unitsPerPixel } from "../orbit";
import type { Change } from "../transforms";
import type { Pose } from "../types";

type Vec3 = [number, number, number];
type AxisName = "X" | "Y" | "Z";
export type GizmoMode = "move" | "rotate" | "scale";

const AXES: { name: AxisName; index: 0 | 1 | 2; direction: Vec3; color: string }[] = [
  { name: "X", index: 0, direction: [1, 0, 0], color: "#ff5468" },
  { name: "Y", index: 1, direction: [0, 1, 0], color: "#7bd34f" },
  { name: "Z", index: 2, direction: [0, 0, 1], color: "#4f8fff" },
];

/** An arrow's length on screen, in CSS pixels, and the shortest that still shows. */
const ARROW = 72;
const SHORTEST = 14;
/** Steps with Shift held: a tenth of a unit, 5 degrees, a tenth of the size. */
const MOVE_STEP = 0.1;
const TURN_STEP = (5 * Math.PI) / 180;
const SCALE_STEP = 0.1;

interface TransformGizmoProps {
  pose: Pose;
  /** The image's width over its height. */
  aspect: number;
  /** Where the object or light is, and what it turns and scales about. */
  point: Vec3;
  mode: GizmoMode;
  onMode: (mode: GizmoMode) => void;
  canRotate: boolean;
  canScale: boolean;
  /** Its own axes it can be scaled along alone, as 0, 1 and 2 for X, Y and Z. */
  scaleAxes: (0 | 1 | 2)[];
  /** While dragging: the change so far, for the 3D view; null once it ends. */
  onPreview: (change: Change | null) => void;
  /** The change it ended with. */
  onCommit: (change: Change) => void;
  /** It was called off: nothing changes. */
  onCancel: () => void;
  /** Grows to start a keyboard move, as after Shift+D. */
  grab?: number;
}

const add = (a: Vec3, b: Vec3, s = 1): Vec3 => [a[0] + b[0] * s, a[1] + b[1] * s, a[2] + b[2] * s];
const dot = (a: Vec3, b: Vec3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const unit = (a: Vec3): Vec3 => {
  const length = Math.hypot(...a) || 1;
  return a.map((v) => v / length) as Vec3;
};

/** A change in progress, measured from where it began. */
interface Drag {
  mode: GizmoMode;
  axis: AxisName | null;
  start: Vec3;
  /** Where the mouse was when it began, as a point of the image from 0 to 1. */
  from: [number, number];
  /** For turning: the angle the mouse has gone round, unwrapped, and the last one seen. */
  turned: number;
  lastAngle: number;
  change: Change | null;
  /** Begun from the keyboard, it follows the mouse with no button held, until a click. */
  keyboard: boolean;
}

/** Whether a key goes to a field being typed in rather than to the gizmo. */
function typing(target: EventTarget | null): boolean {
  const element = target as HTMLElement | null;
  return !!element && (element.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(element.tagName));
}

/**
 * The gizmo of the selected object or light, as in Blender, in three modes: arrows to move it
 * along X, Y or Z and a ring to move it across the view; rings to turn it about each axis and
 * one about the view; and handles to scale it along its own axes, or as a whole from the
 * middle. G, R and S begin moving, turning or scaling from the keyboard, X, Y or Z the
 * current mode along that axis; while one goes on, X, Y or Z locks it to that axis and the
 * same key frees it. A keyboard change follows the mouse until a click or Enter, and Esc
 * calls it off. Shift moves in tenths, turns in 5 degree steps and scales in tenths.
 */
export function TransformGizmo(props: TransformGizmoProps) {
  const { pose, aspect, point, mode } = props;
  const svg = useRef<SVGSVGElement>(null);
  const [size, setSize] = useState<[number, number]>([0, 0]);
  const [drag, setDrag] = useState<Drag | null>(null);
  const dragging = useRef<Drag | null>(null);
  // The latest props, for the window's listeners.
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
  /** The image's size in pixels, for angles and distances on screen. */
  const pixels = (): [number, number] => {
    const box = svg.current?.getBoundingClientRect();
    return [box?.width || 1, box?.height || 1];
  };

  /** The change with the mouse at `at`. */
  const changeAt = (current: Drag, at: [number, number]): Change | null => {
    const { pose, aspect, scaleAxes } = latest.current;
    const axis = current.axis ? AXES.find((a) => a.name === current.axis)! : null;
    const center = project(pose, aspect, current.start);
    if (current.mode === "move") {
      const forward = add(pose.target, pose.position, -1);
      let offset: Vec3 | null = null;
      if (axis) {
        // Where along the axis through the start each mouse ray passes closest.
        const along = (where: [number, number]) => {
          const { origin, direction } = rayThrough(pose, aspect, ...where);
          const b = dot(axis.direction, direction);
          const denominator = 1 - b * b;
          if (denominator < 1e-6) return null;
          const w = add(current.start, origin, -1);
          return (b * dot(direction, w) - dot(axis.direction, w)) / denominator;
        };
        const [t0, t1] = [along(current.from), along(at)];
        if (t0 !== null && t1 !== null) offset = axis.direction.map((v) => v * (t1 - t0)) as Vec3;
      } else {
        // Where each mouse ray meets the plane through the start that faces the camera.
        const plane = (where: [number, number]) => {
          const { origin, direction } = rayThrough(pose, aspect, ...where);
          const facing = dot(direction, forward);
          if (Math.abs(facing) < 1e-9) return null;
          const t = dot(add(current.start, origin, -1), forward) / facing;
          return t > 0 ? add(origin, direction, t) : null;
        };
        const [a, b] = [plane(current.from), plane(at)];
        if (a && b) offset = add(b, a, -1);
      }
      if (!offset) return null;
      if (shift.current) offset = offset.map((v) => Math.round(v / MOVE_STEP) * MOVE_STEP) as Vec3;
      return { type: "move", offset };
    }
    if (!center) return null;
    const [w, h] = pixels();
    const [cx, cy] = [center[0] * w, center[1] * h];
    const [mx, my] = [at[0] * w, at[1] * h];
    if (current.mode === "rotate") {
      // The angle the mouse goes round the middle, counterclockwise on screen, unwrapped.
      const angle = Math.atan2(-(my - cy), mx - cx);
      let delta = angle - current.lastAngle;
      if (delta > Math.PI) delta -= 2 * Math.PI;
      if (delta < -Math.PI) delta += 2 * Math.PI;
      current.turned += delta;
      current.lastAngle = angle;
      const toCamera = unit(add(pose.position, current.start, -1));
      // Counterclockwise on screen is a positive turn about an axis that faces the camera.
      const about = axis ? axis.direction : toCamera;
      const sign = dot(about, toCamera) >= 0 ? 1 : -1;
      let radians = sign * current.turned;
      if (shift.current) radians = Math.round(radians / TURN_STEP) * TURN_STEP;
      return { type: "rotate", axis: about, radians };
    }
    // Scaling: how much farther from the middle the mouse is than where it began.
    const [fx, fy] = [current.from[0] * w, current.from[1] * h];
    const first = Math.hypot(fx - cx, fy - cy);
    if (first < 2) return null;
    let factor = Math.hypot(mx - cx, my - cy) / first;
    if (shift.current) factor = Math.round(factor / SCALE_STEP) * SCALE_STEP;
    const along = axis && scaleAxes.includes(axis.index) ? axis.index : null;
    return { type: "scale", factor: Math.max(0.01, factor), axis: along };
  };

  const follow = (current: Drag, at: [number, number] | null) => {
    if (!at) return;
    const change = changeAt(current, at);
    if (!change) return;
    current.change = change;
    setDrag({ ...current });
    latest.current.onPreview(change);
  };

  const begin = (kind: GizmoMode, axis: AxisName | null, at: [number, number], keyboard: boolean) => {
    const { pose, aspect, point } = latest.current;
    const center = project(pose, aspect, point);
    const [w, h] = pixels();
    const lastAngle = center ? Math.atan2(-(at[1] * h - center[1] * h), at[0] * w - center[0] * w) : 0;
    const current: Drag = { mode: kind, axis, start: point, from: at, turned: 0, lastAngle, change: null, keyboard };
    dragging.current = current;
    setDrag({ ...current });
  };

  const finish = (keep: boolean) => {
    const current = dragging.current;
    if (!current) return;
    dragging.current = null;
    setDrag(null);
    latest.current.onPreview(null);
    if (keep && current.change) latest.current.onCommit(current.change);
    else latest.current.onCancel();
  };

  /** Locks the change to an axis, or frees it when it is locked to that one already. */
  const lock = (axis: AxisName) => {
    const current = dragging.current;
    if (!current) return;
    current.axis = current.axis === axis ? null : axis;
    setDrag({ ...current });
    follow(current, mouse.current ?? current.from);
  };

  const allowed = (kind: GizmoMode) =>
    kind === "move" || (kind === "rotate" && latest.current.canRotate) || (kind === "scale" && latest.current.canScale);

  // The keyboard, and the mouse for a change begun from it.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      shift.current = event.shiftKey;
      if (typing(event.target) || event.metaKey || event.ctrlKey || event.altKey) return;
      const key = event.key.toUpperCase();
      const current = dragging.current;
      if (current) {
        if (key === "X" || key === "Y" || key === "Z") lock(key);
        else if (event.key === "Enter") finish(true);
        else if (event.key === "Escape") finish(false);
        else if (event.key === "Shift") follow(current, mouse.current ?? current.from);
        else return;
        event.preventDefault();
        // The app's own Escape would also clear the selection.
        event.stopPropagation();
        return;
      }
      if (event.repeat) return;
      const kind: GizmoMode | null = key === "G" ? "move" : key === "R" ? "rotate" : key === "S" ? "scale" : null;
      const axis = key === "X" || key === "Y" || key === "Z" ? key : null;
      if (!kind && !axis) return;
      const chosen = kind ?? latest.current.mode;
      if (!allowed(chosen)) return;
      const { pose, aspect, point } = latest.current;
      // From the mouse when it is over the image, else from beside the object.
      const center = project(pose, aspect, point);
      const at = mouse.current ?? (center && [center[0] + 0.05, center[1]] as [number, number]);
      if (!at) return;
      event.preventDefault();
      if (kind) latest.current.onMode(kind);
      begin(chosen, axis, at, true);
    };
    const onKeyUp = (event: KeyboardEvent) => {
      if (shift.current !== event.shiftKey && dragging.current) {
        shift.current = event.shiftKey;
        follow(dragging.current, mouse.current ?? dragging.current.from);
      }
      shift.current = event.shiftKey;
    };
    const onPointerMove = (event: globalThis.PointerEvent) => {
      shift.current = event.shiftKey;
      const at = fraction(event.clientX, event.clientY);
      const box = svg.current?.getBoundingClientRect();
      const inside = box && event.clientX >= box.left && event.clientX <= box.right && event.clientY >= box.top && event.clientY <= box.bottom;
      mouse.current = inside || dragging.current ? at : null;
      if (dragging.current?.keyboard) follow(dragging.current, at);
    };
    // A change begun from the keyboard ends with a click, which then does nothing else.
    const onPointerDown = (event: globalThis.PointerEvent) => {
      if (!dragging.current?.keyboard) return;
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

  // A keyboard move asked for from outside, as after duplicating: when the count grows while
  // the gizmo shows, not for the count it finds when it appears.
  const grabSeen = useRef(props.grab);
  useEffect(() => {
    if (props.grab === grabSeen.current) return;
    grabSeen.current = props.grab;
    if (!props.grab || dragging.current) return;
    const { pose, aspect, point } = latest.current;
    const center = project(pose, aspect, point);
    const at = mouse.current ?? (center && [center[0] + 0.05, center[1]] as [number, number]);
    if (at) begin("move", null, at, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.grab]);

  // A change still going when the gizmo goes away, as when the selection changes, is called off.
  useEffect(() => () => {
    if (dragging.current) {
      dragging.current = null;
      latest.current.onPreview(null);
      latest.current.onCancel();
    }
  }, []);

  const [width, height] = size;
  const center = project(pose, aspect, point);
  const toolbar = (
    <div className="gizmo-modes glass" role="group" aria-label="Gizmo"
      onPointerDown={(event) => event.stopPropagation()} onClick={(event) => event.stopPropagation()}>
      {([["move", Move3d, "Move (G)"], ["rotate", Rotate3d, "Rotate (R)"], ["scale", Scale3d, "Scale (S)"]] as const).map(([kind, Icon, title]) => (
        <button key={kind} type="button" className="icon" aria-pressed={mode === kind} aria-label={title} title={title}
          disabled={!allowed(kind)} onClick={() => props.onMode(kind)}>
          <Icon size={14} aria-hidden="true" />
        </button>
      ))}
    </div>
  );
  if (!center || width === 0) return <>{toolbar}<svg ref={svg} className="move-gizmo" aria-hidden="true" /></>;
  const [cx, cy] = [center[0] * width, center[1] * height];
  const reach = ARROW * unitsPerPixel(pose, aspect, point, height);

  const startDrag = (event: PointerEvent<SVGElement>, kind: GizmoMode, axis: AxisName | null) => {
    if (dragging.current) return;
    event.stopPropagation();
    event.preventDefault();
    const at = fraction(event.clientX, event.clientY);
    if (!at) return;
    try {
      (event.currentTarget as Element).setPointerCapture(event.pointerId);
    } catch {
      // A pointer the browser no longer tracks: the drag still follows its events here.
    }
    begin(kind, axis, at, false);
  };
  const moveDrag = (event: PointerEvent<SVGElement>) => {
    const current = dragging.current;
    if (!current || current.keyboard) return;
    shift.current = event.shiftKey;
    follow(current, fraction(event.clientX, event.clientY));
  };
  const endDrag = (event: PointerEvent<SVGElement>) => {
    if (!dragging.current || dragging.current.keyboard) return;
    event.stopPropagation();
    finish(true);
  };
  const handlers = { onPointerMove: moveDrag, onPointerUp: endDrag, onPointerCancel: endDrag };
  const at = (p: Vec3) => {
    const q = project(pose, aspect, p);
    return q ? [q[0] * width, q[1] * height] as [number, number] : null;
  };
  const shown = drag ? drag.mode : mode;
  const lit = (name: AxisName | null) => drag !== null && drag.axis === name;

  // The line a locked change runs along, through where it began, across the image.
  const locked = drag?.axis ? AXES.find((a) => a.name === drag.axis)! : null;
  let guide: [number, number, number, number] | null = null;
  if (locked && drag && drag.mode !== "scale") {
    const far = reach * 400;
    const a = at(add(drag.start, locked.direction, -far)) ?? at(drag.start);
    const b = at(add(drag.start, locked.direction, far)) ?? at(drag.start);
    if (a && b) guide = [a[0], a[1], b[0], b[1]];
  }

  let handles: ReactNode = null;
  if (shown === "move") {
    handles = (
      <>
        {AXES.map(({ name, direction, color }) => {
          const tip = at(add(point, direction, reach));
          if (!tip) return null;
          const [tx, ty] = tip;
          const length = Math.hypot(tx - cx, ty - cy);
          // An axis pointing at the camera has nothing to drag along.
          if (length < SHORTEST) return null;
          const [ux, uy] = [(tx - cx) / length, (ty - cy) / length];
          const head = `${tx},${ty} ${tx - ux * 11 - uy * 5},${ty - uy * 11 + ux * 5} ${tx - ux * 11 + uy * 5},${ty - uy * 11 - ux * 5}`;
          return (
            <g key={name} className="move-gizmo-axis" data-active={lit(name) || undefined}
              onPointerDown={(event) => startDrag(event, "move", name)} {...handlers}>
              <title>{`Drag to move along ${name}, or press ${name}`}</title>
              <line x1={cx} y1={cy} x2={tx} y2={ty} className="move-gizmo-hit" />
              <line x1={cx + ux * 10} y1={cy + uy * 10} x2={tx - ux * 9} y2={ty - uy * 9} stroke={color}
                strokeWidth={lit(name) ? 3.5 : 2.5} strokeLinecap="round" />
              <polygon points={head} fill={color} />
            </g>
          );
        })}
        <g className="move-gizmo-axis" data-active={(drag && !drag.axis) || undefined}
          onPointerDown={(event) => startDrag(event, "move", null)} {...handlers}>
          <title>Drag to move across the view, or press G</title>
          <circle cx={cx} cy={cy} r={12} className="move-gizmo-hit" />
          <circle cx={cx} cy={cy} r={7} fill="none" stroke="#fff" strokeWidth={drag && !drag.axis ? 2.5 : 1.8} />
        </g>
      </>
    );
  } else if (shown === "rotate") {
    // A circle about each axis, drawn through points of it.
    const ring = (normal: Vec3) => {
      const helper: Vec3 = Math.abs(normal[0]) > 0.9 ? [0, 1, 0] : [1, 0, 0];
      const u = unit([helper[1] * normal[2] - helper[2] * normal[1], helper[2] * normal[0] - helper[0] * normal[2], helper[0] * normal[1] - helper[1] * normal[0]]);
      const v: Vec3 = [normal[1] * u[2] - normal[2] * u[1], normal[2] * u[0] - normal[0] * u[2], normal[0] * u[1] - normal[1] * u[0]];
      const points: string[] = [];
      for (let i = 0; i <= 64; i++) {
        const angle = (i / 64) * 2 * Math.PI;
        const p = at(add(add(point, u, reach * Math.cos(angle)), v, reach * Math.sin(angle)));
        if (p) points.push(`${p[0]},${p[1]}`);
      }
      return points.join(" ");
    };
    handles = (
      <>
        <g className="move-gizmo-axis" data-active={(drag && !drag.axis) || undefined}
          onPointerDown={(event) => startDrag(event, "rotate", null)} {...handlers}>
          <title>Drag to turn about the view, or press R</title>
          <circle cx={cx} cy={cy} r={ARROW * 1.25} className="move-gizmo-hit" fill="none" />
          <circle cx={cx} cy={cy} r={ARROW * 1.25} fill="none" stroke="#fff" strokeWidth={drag && !drag.axis ? 2.5 : 1.5} opacity={0.85} />
        </g>
        {AXES.map(({ name, direction, color }) => (
          <g key={name} className="move-gizmo-axis" data-active={lit(name) || undefined}
            onPointerDown={(event) => startDrag(event, "rotate", name)} {...handlers}>
            <title>{`Drag to turn about ${name}, or press R then ${name}`}</title>
            <polyline points={ring(direction)} className="move-gizmo-hit" fill="none" />
            <polyline points={ring(direction)} fill="none" stroke={color} strokeWidth={lit(name) ? 3.5 : 2.2} />
          </g>
        ))}
      </>
    );
  } else {
    handles = (
      <>
        {AXES.filter((a) => props.scaleAxes.includes(a.index)).map(({ name, direction, color }) => {
          const tip = at(add(point, direction, reach));
          if (!tip) return null;
          const [tx, ty] = tip;
          if (Math.hypot(tx - cx, ty - cy) < SHORTEST) return null;
          return (
            <g key={name} className="move-gizmo-axis" data-active={lit(name) || undefined}
              onPointerDown={(event) => startDrag(event, "scale", name)} {...handlers}>
              <title>{`Drag to scale along ${name}, or press S then ${name}`}</title>
              <line x1={cx} y1={cy} x2={tx} y2={ty} className="move-gizmo-hit" />
              <line x1={cx} y1={cy} x2={tx} y2={ty} stroke={color} strokeWidth={lit(name) ? 3.5 : 2.5} strokeLinecap="round" />
              <rect x={tx - 5} y={ty - 5} width={10} height={10} fill={color} />
            </g>
          );
        })}
        <g className="move-gizmo-axis" data-active={(drag && !drag.axis) || undefined}
          onPointerDown={(event) => startDrag(event, "scale", null)} {...handlers}>
          <title>Drag to scale it as a whole, or press S</title>
          <circle cx={cx} cy={cy} r={ARROW * 1.25} className="move-gizmo-hit" fill="none" />
          <circle cx={cx} cy={cy} r={ARROW * 1.25} fill="none" stroke="#fff" strokeWidth={drag && !drag.axis ? 2.5 : 1.5}
            strokeDasharray="5 4" opacity={0.85} />
        </g>
      </>
    );
  }

  const describe = (current: Drag) => {
    const change = current.change;
    const verb = current.mode === "move" ? "Move" : current.mode === "rotate" ? "Turn" : "Scale";
    let amount = "";
    if (change?.type === "rotate") amount = ` ${((change.radians * 180) / Math.PI).toFixed(1)}°`;
    if (change?.type === "scale") amount = ` × ${change.factor.toFixed(2)}`;
    const where = current.axis
      ? `${current.mode === "rotate" ? " about" : " along"} ${current.axis}`
      : current.mode === "move" ? " across the view" : current.mode === "rotate" ? " about the view" : "";
    const keys = current.axis ? `${current.axis} again frees it` : "X, Y or Z locks an axis";
    return `${verb}${where}${amount} · ${keys}${current.keyboard ? " · click to place, Esc to cancel" : ""}`;
  };

  return (
    <>
      {toolbar}
      <svg ref={svg} className="move-gizmo" viewBox={`0 0 ${width} ${height}`} data-dragging={drag ? (drag.axis ?? "view") : undefined}>
        {guide && locked && (
          <line x1={guide[0]} y1={guide[1]} x2={guide[2]} y2={guide[3]} stroke={locked.color} strokeWidth={1.5} opacity={0.75}
            className="move-gizmo-guide" />
        )}
        {handles}
        {drag && (
          <text x={cx + 16} y={cy + 28} className="move-gizmo-hint">{describe(drag)}</text>
        )}
      </svg>
    </>
  );
}
