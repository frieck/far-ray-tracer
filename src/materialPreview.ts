import { invoke } from "@tauri-apps/api/core";
import type { Pose, SurfaceMaterial } from "./types";

export interface PreviewRequest {
  id: number;
  surface: SurfaceMaterial | null;
  width: number;
  height: number;
  samples: number;
  /** Camera around the ball; null uses the ball's own. */
  pose: Pose | null;
  /** A newer request with the same view cancels this one, which then rejects. */
  view: "thumbnail" | "stage";
}

/** Renders a material on the shader ball with the engine. */
export async function renderMaterialBall(request: PreviewRequest): Promise<ImageData> {
  const buffer = await invoke<ArrayBuffer>("material_preview", { ...request });
  return new ImageData(new Uint8ClampedArray(buffer), request.width, request.height);
}

/** Draws pixels over the whole canvas, scaling a smaller quick pass up smoothly. */
export function drawPreview(canvas: HTMLCanvasElement | null, pixels: ImageData) {
  const context = canvas?.getContext("2d");
  if (!canvas || !context) return;
  if (pixels.width === canvas.width && pixels.height === canvas.height) {
    context.putImageData(pixels, 0, 0);
    return;
  }
  const small = document.createElement("canvas");
  [small.width, small.height] = [pixels.width, pixels.height];
  small.getContext("2d")?.putImageData(pixels, 0, 0);
  context.imageSmoothingQuality = "high";
  context.drawImage(small, 0, 0, canvas.width, canvas.height);
}
