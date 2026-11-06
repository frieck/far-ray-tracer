import { Channel, invoke } from "@tauri-apps/api/core";
import { formatRays, formatTime } from "./format";
import { MODE_NAMES } from "./settings";
import type { CausticReport, RenderRequest, RenderResult } from "./types";

const MESSAGE_BUILDING_KD = 0;
const HEADER_SIZE = 24;

export interface Surfaces {
  after: HTMLCanvasElement;
  before: HTMLCanvasElement;
  ambient: HTMLCanvasElement;
}

export interface RenderJob {
  main: RenderRequest;
  comparison: RenderRequest | null;
}

export interface RenderView {
  rendering: boolean;
  progress: number;
  status: string;
  hasResult: boolean;
  caustics: CausticReport | null;
}

type Header = Uint32Array;

function summarize(result: RenderResult): string {
  const parts: string[] = [];
  if (result.kd_ms !== null) parts.push(`KD-Tree ${formatTime(result.kd_ms)}`);
  parts.push(`Render ${formatTime(result.render_ms)}`, formatRays(result.rays));
  return (result.cancelled ? "Cancelled · " : "") + parts.join(" · ");
}

function describe([, done, total, ms, pass, passes]: Header): { progress: number; status: string } {
  const progress = total ? done / total : 0;
  if (passes > 1 && pass > 0) {
    const left = progress > 0.02 ? ` · about ${formatTime(ms * (1 / progress - 1))} left` : "";
    return { progress, status: `Pass ${pass} of ${passes} · ${formatTime(ms)}${left}` };
  }
  return { progress, status: `Rendering ${Math.round(progress * 100)}% · ${formatTime(ms)}` };
}

/**
 * Streams a render from the engine onto `canvas`. Frames are painted as they arrive:
 * the engine already limits them to about 30 per second, and `requestAnimationFrame`
 * would stop painting while the window is covered, leaving a stale image behind.
 */
function paintRender(
  canvas: HTMLCanvasElement,
  request: RenderRequest,
  onHeader?: (header: Header) => void,
  ambient?: HTMLCanvasElement,
): Promise<RenderResult> {
  // The previous image stays until the first frame replaces it, which avoids a black
  // flash on every change. Resizing a canvas clears it, so that only happens if needed.
  if (canvas.width !== request.width || canvas.height !== request.height) {
    canvas.width = request.width;
    canvas.height = request.height;
  }
  const context = canvas.getContext("2d")!;
  const ambientContext = ambient?.getContext("2d");

  const channel = new Channel<ArrayBuffer>();
  channel.onmessage = (buffer) => {
    const header = new Uint32Array(buffer, 0, 6);
    onHeader?.(header);
    if (header[0] === MESSAGE_BUILDING_KD) return;
    // A late frame of another resolution does not fit this canvas.
    if (buffer.byteLength !== HEADER_SIZE + canvas.width * canvas.height * 4) return;
    const pixels = new Uint8ClampedArray(buffer, HEADER_SIZE);
    context.putImageData(new ImageData(pixels, canvas.width, canvas.height), 0, 0);
    ambientContext?.drawImage(canvas, 0, 0, ambient!.width, ambient!.height);
  };
  return invoke<RenderResult>("render_scene", { request, channel });
}

/** Runs one render at a time. A new request cancels the current one, and the newest wins. */
export class Renderer {
  private latestRun = 0;
  private active: Promise<void> | null = null;
  private stopping = false;

  constructor(
    private readonly surfaces: () => Surfaces | null,
    private readonly publish: (view: Partial<RenderView>) => void,
  ) {}

  async stop(): Promise<void> {
    if (!this.active) return;
    this.stopping = true;
    await invoke("cancel");
    await this.active;
  }

  async render(job: RenderJob): Promise<void> {
    this.latestRun += 1;
    const run = this.latestRun;
    await this.stop();
    if (run !== this.latestRun) return;
    this.stopping = false;
    this.active = this.execute(job).finally(() => {
      this.active = null;
    });
    await this.active;
  }

  private async execute(job: RenderJob): Promise<void> {
    const surfaces = this.surfaces();
    if (!surfaces) return;
    this.publish({ rendering: true, progress: 0 });
    try {
      // The comparison goes first: it is quick, and it would otherwise keep showing
      // the previous scene while the main image builds up.
      if (job.comparison) {
        this.publish({ status: `Rendering the ${MODE_NAMES[job.comparison.mode]} image…` });
        await paintRender(surfaces.before, job.comparison);
      }
      if (this.stopping) {
        this.publish({ status: "Cancelled" });
        return;
      }
      // The last frame can arrive after the command returns. It still updates the
      // image, but it must not overwrite the summary in the status bar.
      let live = true;
      const onHeader = (header: Header) => {
        if (!live) return;
        const tracingPhotons = job.main.caustics && header[4] === 0 && header[5] > 1;
        if (header[0] === MESSAGE_BUILDING_KD) this.publish({ status: "Building the KD-Tree…" });
        else if (tracingPhotons) this.publish({ status: "Tracing photons…" });
        else this.publish(describe(header));
      };
      const result = await paintRender(surfaces.after, job.main, onHeader, surfaces.ambient);
      live = false;
      this.publish({
        status: summarize(result),
        hasResult: !result.cancelled,
        caustics: result.caustics,
        progress: result.cancelled ? 0 : 1,
      });
    } catch (error) {
      this.publish({ status: `Error: ${String(error)}` });
    } finally {
      this.publish({ rendering: false });
    }
  }
}
