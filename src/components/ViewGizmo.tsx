import { cameraAxes } from "../orbit";
import type { Pose } from "../types";

type Vec = [number, number, number];

const SIZE = 84;
const REACH = 30;
const AXES: { name: string; direction: Vec; color: string }[] = [
  { name: "X", direction: [1, 0, 0], color: "#ff5468" },
  { name: "Y", direction: [0, 1, 0], color: "#7bd34f" },
  { name: "Z", direction: [0, 0, 1], color: "#4f8fff" },
];

const dot = (a: Vec, b: Vec) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

/** Scene axes as the camera sees them. Clicking an axis looks along it, when the camera can move. */
export function ViewGizmo({ pose, onAlign }: { pose: Pose; onAlign: ((axis: Vec) => void) | null }) {
  const { right, up, forward } = cameraAxes(pose);
  const ends = AXES.flatMap(({ name, direction, color }) =>
    [1, -1].map((sign) => {
      const d: Vec = [direction[0] * sign, direction[1] * sign, direction[2] * sign];
      return {
        name,
        positive: sign > 0,
        color,
        axis: d,
        x: SIZE / 2 + dot(d, right) * REACH,
        y: SIZE / 2 - dot(d, up) * REACH,
        // Toward the viewer is drawn last, on top.
        depth: -dot(d, forward),
      };
    }),
  ).sort((a, b) => a.depth - b.depth);

  return (
    <svg
      className="view-gizmo"
      data-interactive={onAlign ? true : undefined}
      width={SIZE}
      height={SIZE}
      viewBox={`0 0 ${SIZE} ${SIZE}`}
      role="group"
      aria-label="Scene axes"
      onClick={(event) => event.stopPropagation()}
      onPointerDown={(event) => event.stopPropagation()}
      onPointerMove={(event) => event.stopPropagation()}
    >
      <circle className="view-gizmo-back" cx={SIZE / 2} cy={SIZE / 2} r={SIZE / 2 - 1} />
      {ends.map((end) => {
        const facing = end.depth > 0 ? 1 : 0.55;
        const label = `${end.positive ? "" : "−"}${end.name}`;
        const view = `View from ${label}`;
        return (
          <g key={label} opacity={facing}>
            {end.positive && <line x1={SIZE / 2} y1={SIZE / 2} x2={end.x} y2={end.y} stroke={end.color} strokeWidth={2} />}
            <g
              role={onAlign ? "button" : undefined}
              aria-label={onAlign ? view : label}
              tabIndex={onAlign ? 0 : undefined}
              onClick={() => onAlign?.(end.axis)}
              onKeyDown={(event) => {
                if (onAlign && (event.key === "Enter" || event.key === " ")) onAlign(end.axis);
              }}
            >
              <title>{onAlign ? view : label}</title>
              <circle cx={end.x} cy={end.y} r={end.positive ? 8 : 6} fill={end.positive ? end.color : "rgba(15, 16, 22, 0.8)"}
                stroke={end.color} strokeWidth={end.positive ? 0 : 1.5} />
              {end.positive && (
                <text x={end.x} y={end.y} dy="0.35em" textAnchor="middle" className="view-gizmo-label">{end.name}</text>
              )}
            </g>
          </g>
        );
      })}
    </svg>
  );
}
