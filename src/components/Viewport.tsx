import { useEffect, useState, type CSSProperties, type PointerEvent, type RefObject, useRef } from "react";
import { formatNumber, prettyName } from "../format";
import { MODE_NAMES } from "../settings";
import type { Mode, Pose, SceneInfo } from "../types";
import { GlassPanel } from "./GlassPanel";
import { ViewGizmo } from "./ViewGizmo";
import { Image, Sparkles } from "lucide-react";

interface ViewportProps {
  pickingFocus: boolean;
  focusHint: string;
  onPickFocus: (x: number, y: number) => void;
  navigable: boolean;
  onCameraDrag: (panning: boolean, dx: number, dy: number, stageHeight: number) => void;
  onCameraRelease: () => void;
  onCameraWheel: (amount: number) => void;
  /** The pose the image shows, for the axes gizmo. */
  pose: Pose | null;
  onAlignView: (axis: [number, number, number]) => void;
  afterRef: RefObject<HTMLCanvasElement | null>;
  beforeRef: RefObject<HTMLCanvasElement | null>;
  photonRef: RefObject<HTMLCanvasElement | null>;
  liveRef: RefObject<HTMLCanvasElement | null>;
  showLive: boolean;
  scene: SceneInfo | null;
  mode: Mode;
  comparison: Mode | null;
  status: string;
  progress: number;
  rendering: boolean;
  aspect: number;
  hasPhotonMap: boolean;
  showPhotonMap: boolean;
  onPhotonMapChange: (value: boolean) => void;
}

function plural(count: number, word: string): string {
  return `${formatNumber(count)} ${word}${count === 1 ? "" : "s"}`;
}

export function Viewport(props: ViewportProps) {
  const { afterRef, beforeRef, photonRef, scene, mode, status, progress, rendering, aspect, hasPhotonMap, showPhotonMap, onPhotonMapChange } = props;
  const comparison = showPhotonMap || props.pickingFocus ? null : props.comparison;
  const [divider, setDivider] = useState(0.5);
  const drag = useRef<{ x: number; y: number; panning: boolean } | null>(null);

  useEffect(() => setDivider(0.5), [comparison]);

  const startDrag = (event: PointerEvent<HTMLDivElement>) => {
    if (!props.navigable) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { x: event.clientX, y: event.clientY, panning: event.shiftKey || event.button !== 0 };
  };

  const endDrag = () => {
    if (!drag.current) return;
    drag.current = null;
    props.onCameraRelease();
  };

  const moveDivider = (event: PointerEvent<HTMLDivElement>) => {
    if (drag.current) {
      const { x, y, panning } = drag.current;
      drag.current = { x: event.clientX, y: event.clientY, panning };
      props.onCameraDrag(panning, event.clientX - x, event.clientY - y, event.currentTarget.clientHeight);
      return;
    }
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
          data-picking-focus={props.pickingFocus || undefined}
          data-navigable={props.navigable || undefined}
          onPointerDown={startDrag}
          onPointerUp={endDrag}
          onPointerCancel={endDrag}
          onContextMenu={(event) => props.navigable && event.preventDefault()}
          onWheel={(event) => props.navigable && props.onCameraWheel(event.deltaY)}
          onClick={(event) => {
            if (!props.pickingFocus) return;
            const box = event.currentTarget.getBoundingClientRect();
            props.onPickFocus((event.clientX - box.left) / box.width, (event.clientY - box.top) / box.height);
          }}
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
          <canvas ref={photonRef} width={800} height={600} hidden={!showPhotonMap} aria-label="Photon map: projected photon deposits" />
          <canvas ref={props.liveRef} hidden={!props.showLive} aria-label="Real-time view of the scene" />
          {comparison && (
            <div className="divider" style={{ left: `${divider * 100}%` }}>
              <span className="tag left">{MODE_NAMES[comparison]}</span>
              <span className="tag right">{MODE_NAMES[mode]}</span>
            </div>
          )}
          {props.pose && !comparison && !showPhotonMap && (
            <ViewGizmo pose={props.pose} onAlign={props.navigable ? props.onAlignView : null} />
          )}
          {props.pickingFocus && <p className="focus-hint" role="status">{props.focusHint}</p>}
          {!scene && <p className="empty">Pick a scene to watch it render.</p>}
          {hasPhotonMap && (
            <div className="render-views glass" role="group" aria-label="Render view" onClick={(event) => event.stopPropagation()} onPointerDown={(event) => event.stopPropagation()} onPointerMove={(event) => event.stopPropagation()}>
              <button type="button" className="icon" aria-label="Full render" title="Full render" aria-pressed={!showPhotonMap} onClick={() => onPhotonMapChange(false)}>
                <Image size={18} aria-hidden="true" />
              </button>
              <button type="button" className="icon" aria-label="Photon map" title="Photon map · photon deposits, including hidden surfaces" aria-pressed={showPhotonMap} onClick={() => onPhotonMapChange(true)}>
                <Sparkles size={18} aria-hidden="true" />
              </button>
            </div>
          )}
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
