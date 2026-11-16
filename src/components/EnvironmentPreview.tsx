import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState, type PointerEvent } from "react";
import type { EnvironmentConfig, Pose, ToneMapping } from "../types";

/** Width of the panorama the engine sends; the height is half. */
const WIDTH = 512;
const HEIGHT = WIDTH / 2;

interface EnvironmentPreviewProps {
  environment: EnvironmentConfig;
  exposure: number;
  toneMapping: ToneMapping;
  whiteBalance: number;
  tint: number;
  /** The camera, marked where it looks. */
  pose: Pose;
  /** Dragging sideways turns the environment, when given. */
  onRotate?: (rotation: number) => void;
}

/**
 * The whole environment as a panorama, as seen from inside: brightened, white balanced and tone-mapped as
 * renders show it, turned by its rotation, with a ring where the camera looks.
 */
export function EnvironmentPreview({ environment, exposure, toneMapping, whiteBalance, tint, pose, onRotate }: EnvironmentPreviewProps) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [pixels, setPixels] = useState<ImageData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const drag = useRef<{ x: number; rotation: number } | null>(null);

  // What the pixels depend on; rotation and the camera only move them.
  const { path, sky, intensity } = environment;
  const key = JSON.stringify({ path, sky, intensity, exposure, toneMapping, whiteBalance, tint });
  useEffect(() => {
    let current = true;
    setBusy(true);
    const timer = window.setTimeout(() => {
      invoke<ArrayBuffer>("environment_preview", { environment, width: WIDTH, exposure, toneMapping, whiteBalance, tint })
        .then((buffer) => {
          if (!current) return;
          setPixels(new ImageData(new Uint8ClampedArray(buffer), WIDTH, HEIGHT));
          setError(null);
        })
        .catch((e) => current && setError(String(e)))
        .finally(() => current && setBusy(false));
    }, 120);
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
  }, [key]);

  useEffect(() => {
    const context = canvas.current?.getContext("2d");
    if (!context) return;
    context.clearRect(0, 0, WIDTH, HEIGHT);
    if (pixels) {
      // A rotation of r degrees moves what the map shows r/360 of the way to the right.
      const shift = ((((environment.rotation / 360) * WIDTH) % WIDTH) + WIDTH) % WIDTH;
      context.putImageData(pixels, Math.round(shift), 0);
      context.putImageData(pixels, Math.round(shift) - WIDTH, 0);
    }
    const [x, y] = cameraPoint(pose);
    context.lineWidth = 2;
    context.strokeStyle = "rgba(0, 0, 0, 0.55)";
    context.beginPath();
    context.arc(x, y, 9, 0, 2 * Math.PI);
    context.stroke();
    context.lineWidth = 1.5;
    context.strokeStyle = "rgba(255, 255, 255, 0.95)";
    context.beginPath();
    context.arc(x, y, 9, 0, 2 * Math.PI);
    context.stroke();
  }, [pixels, environment.rotation, pose]);

  const start = (event: PointerEvent<HTMLCanvasElement>) => {
    if (!onRotate || !environment.enabled) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { x: event.clientX, rotation: environment.rotation };
  };
  const move = (event: PointerEvent<HTMLCanvasElement>) => {
    if (!drag.current || !onRotate) return;
    const width = event.currentTarget.getBoundingClientRect().width || 1;
    const rotation = drag.current.rotation + ((event.clientX - drag.current.x) / width) * 360;
    onRotate(Math.round(((rotation % 360) + 360) % 360));
  };
  const end = () => {
    drag.current = null;
  };

  return (
    <div className="environment-preview" data-busy={busy || undefined} data-off={!environment.enabled || undefined}
      title={onRotate ? "Drag sideways to turn the environment. The ring is where the camera looks." : "The ring is where the camera looks."}>
      <canvas ref={canvas} width={WIDTH} height={HEIGHT} data-turns={onRotate ? "" : undefined}
        onPointerDown={start} onPointerMove={move} onPointerUp={end} onPointerCancel={end} />
      {error && <span className="environment-preview-error">{error}</span>}
    </div>
  );
}

/** Where the camera's view direction falls on the panorama, in its pixels. */
function cameraPoint(pose: Pose): [number, number] {
  const d = pose.target.map((t, i) => t - pose.position[i]);
  const length = Math.hypot(d[0], d[1], d[2]) || 1;
  // The layout of the engine's maps: +Z at the center, +X to its right, +Y at the top.
  const u = Math.atan2(d[0], d[2]) / (2 * Math.PI) + 0.5;
  const v = Math.acos(Math.max(-1, Math.min(1, d[1] / length))) / Math.PI;
  return [(((u % 1) + 1) % 1) * WIDTH, v * HEIGHT];
}
