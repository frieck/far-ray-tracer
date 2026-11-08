import { invoke } from "@tauri-apps/api/core";
import { RotateCcw, X } from "lucide-react";
import { useEffect, useRef, useState, type PointerEvent } from "react";
import { drawPreview, renderMaterialBall } from "../materialPreview";
import { dolly, orbit, pan, viewHalfSize } from "../orbit";
import type { MaterialOverride, Pose } from "../types";

/** Most pixels on the longer side of the image. */
const MAX_SIDE = 1000;
/**
 * Sizes are in screen points, not Retina pixels: a quarter of the pixels pays for four times the
 * samples, and a clean image matters more here than the last bit of sharpness.
 * While the camera moves, a half-size image with few samples keeps up with the mouse. Once it
 * stops, each pass refines the last; glass needs the many samples of the last one, because the
 * denoiser cannot see the floor through it.
 */
const MOVING = [{ scale: 0.5, samples: 2 }];
const STILL = [{ scale: 0.5, samples: 4 }, { scale: 1, samples: 32 }, { scale: 1, samples: 128 }];
/** Waits this long after the last wheel step before the final pass. */
const WHEEL_END_MS = 200;

/**
 * The ball is framed for a square image. The lens keeps the image height for wider images, which
 * would crop the ball; a lower zoom keeps the square's view inside any shape.
 */
function framed(pose: Pose, [width, height]: [number, number]): Pose {
  const squareHalf = viewHalfSize(1)[1];
  return { ...pose, zoom: pose.zoom * Math.min(1, viewHalfSize(width / height)[1] / squareHalf) };
}

/**
 * The material ball rendered large over the image, with the same orbit, pan and zoom as the
 * scene camera. Renders progressively: coarse while moving, refined once the camera stops.
 */
export function MaterialStage({ id, edit, label, onClose }: {
  id: number;
  edit: MaterialOverride | null;
  label: string;
  onClose: () => void;
}) {
  const stage = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const [home, setHome] = useState<Pose | null>(null);
  const [pose, setPose] = useState<Pose | null>(null);
  const [size, setSize] = useState<[number, number] | null>(null);
  const [moving, setMoving] = useState(false);
  const [busy, setBusy] = useState(true);
  const drag = useRef<{ x: number; y: number; panning: boolean } | null>(null);
  const wheelTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const request = useRef(0);

  useEffect(() => {
    void invoke<Pose>("material_ball_pose").then((value) => {
      setHome(value);
      setPose(value);
    });
  }, []);

  // The canvas follows the stage, in screen points up to MAX_SIDE.
  useEffect(() => {
    const element = stage.current;
    if (!element) return;
    const measure = () => {
      const [width, height] = [element.clientWidth, element.clientHeight];
      const fit = Math.min(1, MAX_SIDE / Math.max(width, height));
      if (width > 0 && height > 0) setSize([Math.round(width * fit), Math.round(height * fit)]);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const key = JSON.stringify([id, edit, pose, size, moving]);
  useEffect(() => {
    if (!pose || !size) return;
    const current = ++request.current;
    const passes = moving ? MOVING : STILL;
    setBusy(true);
    void (async () => {
      for (const { scale, samples } of passes) {
        const [width, height] = size.map((side) => Math.max(1, Math.round(side * scale)));
        try {
          const pixels = await renderMaterialBall({ id, edit, width, height, samples, pose: framed(pose, size), view: "stage" });
          if (current !== request.current) return;
          drawPreview(canvas.current, pixels);
        } catch {
          // A newer camera or edit replaced this image.
          return;
        }
      }
      if (current === request.current) setBusy(false);
    })();
    // `key` stands for every input of the image.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  const move = (change: (pose: Pose) => Pose) => setPose((old) => old && change(old));

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { x: event.clientX, y: event.clientY, panning: event.shiftKey || event.button !== 0 };
    setMoving(true);
  };
  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    if (!drag.current || !size) return;
    const { x, y, panning } = drag.current;
    const [dx, dy] = [event.clientX - x, event.clientY - y];
    drag.current = { x: event.clientX, y: event.clientY, panning };
    const height = event.currentTarget.clientHeight;
    move((old) => (panning ? pan(old, dx, dy, height, size[0] / size[1]) : orbit(old, dx, dy)));
  };
  const onPointerUp = () => {
    drag.current = null;
    setMoving(false);
  };

  return (
    <div
      ref={stage}
      className="material-stage"
      data-busy={busy || undefined}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      onContextMenu={(event) => event.preventDefault()}
      onClick={(event) => event.stopPropagation()}
      onWheel={(event) => {
        event.stopPropagation();
        setMoving(true);
        move((old) => dolly(old, event.deltaY));
        clearTimeout(wheelTimer.current);
        wheelTimer.current = setTimeout(() => setMoving(false), WHEEL_END_MS);
      }}
    >
      <canvas ref={canvas} width={size?.[0] ?? 1} height={size?.[1] ?? 1} role="img" aria-label={`Material ball: ${label}`} />
      <div className="material-stage-bar glass" onPointerDown={(event) => event.stopPropagation()}>
        <span>{label}</span>
        <button type="button" className="icon" aria-label="Reset ball camera" title="Reset camera"
          disabled={!home} onClick={() => setPose(home)}>
          <RotateCcw size={15} aria-hidden="true" />
        </button>
        <button type="button" className="icon" aria-label="Close material ball" title="Back to the render" onClick={onClose}>
          <X size={16} aria-hidden="true" />
        </button>
      </div>
      <p className="material-stage-hint">Drag to orbit · Shift-drag to pan · Scroll to zoom</p>
    </div>
  );
}
