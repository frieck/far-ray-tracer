import { Channel, invoke } from "@tauri-apps/api/core";
import { formatRays, formatTime } from "./format";
import { MODE_NAMES } from "./settings";
import type { CausticReport, RenderRequest, RenderResult } from "./types";

const MESSAGE_BUILDING_KD = 0;
const MESSAGE_PHOTON_MAP = 2;
const HEADER_SIZE = 24;

export interface Surfaces {
  after: HTMLCanvasElement;
  before: HTMLCanvasElement;
  ambient: HTMLCanvasElement;
  photons: HTMLCanvasElement;
}

export interface RenderJob {
  /** A camera preview: shown, but it does not count as a finished image. */
  preview?: boolean;
  main: RenderRequest;
  comparison: RenderRequest | null;
}

export interface RenderView {
  rendering: boolean;
  progress: number;
  status: string;
  hasResult: boolean;
  caustics: CausticReport | null;
  hasPhotonMap: boolean;
  /** The current render has a whole image on screen. */
  painted: boolean;
}

type Header = Uint32Array;

/** The structure that finds what a ray hits: the 2008 stage's KD-Tree, or the BVH of the others. */
const structureName = (mode: string) => (mode === "classic" ? "KD-Tree" : "BVH");

function summarize(result: RenderResult, mode: string): string {
  const parts: string[] = [];
  if (result.redisplayed) {
    parts.push(`Shown again in ${formatTime(result.render_ms)}, without rendering`);
  } else {
    if (result.kd_ms !== null) parts.push(`${structureName(mode)} ${formatTime(result.kd_ms)}`);
    if (result.continued_from) parts.push(`Continued from pass ${result.continued_from}`);
    parts.push(`Render ${formatTime(result.render_ms)}`, formatRays(result.rays));
  }
  if (result.denoiser) parts.push(result.denoiser === "oidn" ? "Denoised by Open Image Denoise" : "Denoised");
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
  onPhotonMap?: (buffer: ArrayBuffer, header: Header) => void,
  isCurrent: () => boolean = () => true,
): Promise<RenderResult> {
  // The previous image stays until the first frame replaces it, which avoids a black
  // flash on every change. Resizing a canvas clears it, so that only happens if needed.
  if (canvas.width !== request.width || canvas.height !== request.height) {
    const previous = document.createElement("canvas");
    previous.width = canvas.width;
    previous.height = canvas.height;
    previous.getContext("2d")!.drawImage(canvas, 0, 0);
    canvas.width = request.width;
    canvas.height = request.height;
    canvas.getContext("2d")!.drawImage(previous, 0, 0, canvas.width, canvas.height);
  }
  const context = canvas.getContext("2d")!;
  const ambientContext = ambient?.getContext("2d");

  const channel = new Channel<ArrayBuffer>();
  channel.onmessage = (buffer) => {
    if (!isCurrent() || buffer.byteLength < HEADER_SIZE) return;
    const header = new Uint32Array(buffer, 0, 6);
    if (header[0] === MESSAGE_PHOTON_MAP) {
      onPhotonMap?.(buffer, header);
      return;
    }
    onHeader?.(header);
    if (header[0] === MESSAGE_BUILDING_KD) return;
    // A late frame of another resolution does not fit this canvas.
    if (buffer.byteLength !== HEADER_SIZE + canvas.width * canvas.height * 4) return;
    const pixels = new ImageData(new Uint8ClampedArray(buffer, HEADER_SIZE), canvas.width, canvas.height);
    // A region render paints only its rectangle; the last image stays around it.
    const region = request.region;
    if (region) context.putImageData(pixels, 0, 0, region[0], region[1], region[2] - region[0], region[3] - region[1]);
    else context.putImageData(pixels, 0, 0);
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
    this.publish({ status: "Cancelling…" });
    await invoke("cancel");
    await this.active;
  }

  async render(job: RenderJob): Promise<void> {
    this.latestRun += 1;
    const run = this.latestRun;
    await this.stop();
    if (run !== this.latestRun) return;
    this.stopping = false;
    this.active = this.execute(job, run).finally(() => {
      this.active = null;
    });
    await this.active;
  }

  private async execute(job: RenderJob, run: number): Promise<void> {
    const surfaces = this.surfaces();
    if (!surfaces) return;
    const isCurrent = () => run === this.latestRun && !this.stopping;
    this.publish({ rendering: true, progress: 0, hasPhotonMap: false, caustics: null, painted: false });
    try {
      // The comparison goes first: it is quick, and it would otherwise keep showing
      // the previous scene while the main image builds up.
      if (job.comparison) {
        this.publish({ status: `Rendering the ${MODE_NAMES[job.comparison.mode]} image…` });
        await paintRender(surfaces.before, job.comparison, undefined, undefined, undefined, isCurrent);
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
        if (header[0] === MESSAGE_BUILDING_KD) this.publish({ status: `Building the ${structureName(job.main.mode)}…` });
        else if (tracingPhotons) this.publish({ status: "Tracing photons…" });
        else this.publish({ ...describe(header), painted: header[4] >= 2 || header[1] === header[2] });
      };
      const onPhotonMap = (buffer: ArrayBuffer, header: Header) => {
        const [, width, height] = header;
        if (!width || !height || buffer.byteLength !== HEADER_SIZE + width * height * 4) return;
        const canvas = surfaces.photons;
        canvas.width = width;
        canvas.height = height;
        canvas.getContext("2d")!.putImageData(new ImageData(new Uint8ClampedArray(buffer, HEADER_SIZE), width, height), 0, 0);
        this.publish({ hasPhotonMap: true });
      };
      const result = await paintRender(surfaces.after, job.main, onHeader, surfaces.ambient, onPhotonMap, isCurrent);
      live = false;
      this.publish({
        status: job.preview ? "Moving the camera…" : summarize(result, job.main.mode),
        hasResult: !job.preview && !result.cancelled,
        caustics: result.caustics,
        progress: result.cancelled ? 0 : 1,
        painted: !result.cancelled,
      });
    } catch (error) {
      this.publish({ status: `Error: ${String(error)}`, painted: true });
    } finally {
      this.publish({ rendering: false });
    }
  }
}
