import { useEffect, useState, type CSSProperties, type PointerEvent, type RefObject } from "react";
import { formatNumber, prettyName } from "../format";
import { MODE_NAMES } from "../settings";
import type { Mode, SceneInfo } from "../types";
import { GlassPanel } from "./GlassPanel";

interface ViewportProps {
  afterRef: RefObject<HTMLCanvasElement | null>;
  beforeRef: RefObject<HTMLCanvasElement | null>;
  scene: SceneInfo | null;
  mode: Mode;
  comparison: Mode | null;
  status: string;
  progress: number;
  rendering: boolean;
  aspect: number;
}

function plural(count: number, word: string): string {
  return `${formatNumber(count)} ${word}${count === 1 ? "" : "s"}`;
}

export function Viewport(props: ViewportProps) {
  const { afterRef, beforeRef, scene, mode, comparison, status, progress, rendering, aspect } = props;
  const [divider, setDivider] = useState(0.5);

  useEffect(() => setDivider(0.5), [comparison]);

  const moveDivider = (event: PointerEvent<HTMLDivElement>) => {
    if (!comparison) return;
    const box = event.currentTarget.getBoundingClientRect();
    setDivider(Math.min(Math.max((event.clientX - box.left) / box.width, 0), 1));
  };

  return (
    <main className="viewport">
      <div className="drag-strip" data-tauri-drag-region />
      <div className="stage-area">
        <div
          className="stage"
          data-comparing={comparison ? true : undefined}
          style={{ "--aspect": aspect } as CSSProperties}
          onPointerMove={moveDivider}
        >
          <canvas ref={afterRef} width={800} height={600} />
          <canvas
            ref={beforeRef}
            width={800}
            height={600}
            hidden={!comparison}
            style={{ clipPath: `inset(0 ${(1 - divider) * 100}% 0 0)` }}
          />
          {comparison && (
            <div className="divider" style={{ left: `${divider * 100}%` }}>
              <span className="tag left">{MODE_NAMES[comparison]}</span>
              <span className="tag right">{MODE_NAMES[mode]}</span>
            </div>
          )}
          {!scene && <p className="empty">Pick a scene to watch it render.</p>}
        </div>
      </div>

      <GlassPanel className="status" aria-label="Status">
        <div className="status-row">
          <p role="status">{status}</p>
          {scene && (
            <p className="scene-info">
              <strong>{prettyName(scene.name)}</strong>
              <span>
                {plural(scene.primitives, "primitive")} · {plural(scene.lights, "light")}
              </span>
            </p>
          )}
        </div>
        {scene && scene.warnings.length > 0 && <p className="warnings">{scene.warnings.join(" · ")}</p>}
        <div className="progress" data-active={rendering || undefined}>
          <div style={{ width: `${progress * 100}%` }} />
        </div>
      </GlassPanel>
    </main>
  );
}
