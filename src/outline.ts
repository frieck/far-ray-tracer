/**
 * The selection's outline, drawn from an ID map at the display's own resolution: each shape is
 * a mask, softened with a small blur, and its edge is where the blurred mask crosses one half.
 * The band around that crossing is the line, so it comes out smooth, antialiased and the same
 * width in screen pixels whatever the shape, where a pixel-by-pixel edge would show steps.
 */

/** The accent of the interface, as RGB. */
const ACCENT: [number, number, number] = [108, 141, 255];
const WHITE: [number, number, number] = [255, 255, 255];

/** How a shape is drawn: the line's color and strength, its fill's, and the line's width. */
interface Style {
  color: [number, number, number];
  line: number;
  fill: number;
  /** Half the line's width, as a share of the blurred mask's ramp: larger is wider. */
  width: number;
}

const SELECTED: Style = { color: ACCENT, line: 1, fill: 0.16, width: 0.62 };
/** What of the selection lies behind other things: fainter, as Blender's X-ray. */
const BEHIND: Style = { color: ACCENT, line: 0.5, fill: 0.06, width: 0.5 };
const HOVER: Style = { color: WHITE, line: 0.55, fill: 0, width: 0.45 };

/** Blur radius in pixels, applied twice; the line is about this wide on either side. */
const RADIUS = 1;

function smoothstep(a: number, b: number, x: number): number {
  const t = Math.min(1, Math.max(0, (x - a) / (b - a)));
  return t * t * (3 - 2 * t);
}

/** Scratch buffers, kept between calls: the overlay draws often while the camera moves. */
let scratch = new Float32Array(0);
let spare = new Float32Array(0);

/**
 * Draws a shape over `out`, RGBA `width` by `height`: the pixels `inside` says are in it.
 * Only the shape's box, and a margin for the line, is worked on.
 */
function drawShape(out: Uint8ClampedArray, width: number, height: number, inside: (p: number) => boolean, style: Style) {
  let [x0, y0, x1, y1] = [width, height, -1, -1];
  for (let y = 0, p = 0; y < height; y++) {
    for (let x = 0; x < width; x++, p++) {
      if (inside(p)) {
        if (x < x0) x0 = x;
        if (x > x1) x1 = x;
        if (y < y0) y0 = y;
        if (y > y1) y1 = y;
      }
    }
  }
  if (x1 < 0) return;
  const margin = 2 * RADIUS + 2;
  x0 = Math.max(0, x0 - margin);
  y0 = Math.max(0, y0 - margin);
  x1 = Math.min(width - 1, x1 + margin);
  y1 = Math.min(height - 1, y1 + margin);
  const w = x1 - x0 + 1;
  const h = y1 - y0 + 1;
  if (scratch.length < w * h) {
    scratch = new Float32Array(w * h);
    spare = new Float32Array(w * h);
  }
  const mask = scratch;
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) mask[y * w + x] = inside((y + y0) * width + x + x0) ? 1 : 0;
  }
  // Two box blurs, each across then down: close to a Gaussian.
  for (let pass = 0; pass < 2; pass++) {
    blur(mask, spare, w, h, 1, w);
    blur(spare, mask, h, w, w, 1);
  }
  const [r, g, b] = style.color;
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const v = mask[y * w + x];
      if (v <= 0.002) continue;
      // 0 on the edge, 1 well inside or outside.
      const away = Math.abs(2 * v - 1);
      const line = style.line * (1 - smoothstep(style.width * 0.55, style.width, away));
      const fill = style.fill * smoothstep(0.45, 0.6, v);
      const alpha = Math.max(line, fill);
      if (alpha <= 0.004) continue;
      const q = ((y + y0) * width + x + x0) * 4;
      // Over what is there, as a premultiplied blend.
      const under = out[q + 3] / 255;
      const total = alpha + under * (1 - alpha);
      out[q] = (r * alpha + out[q] * under * (1 - alpha)) / total;
      out[q + 1] = (g * alpha + out[q + 1] * under * (1 - alpha)) / total;
      out[q + 2] = (b * alpha + out[q + 2] * under * (1 - alpha)) / total;
      out[q + 3] = total * 255;
    }
  }
}

/** A box blur of `RADIUS` along lines of `length` values `step` apart, `lines` of them `stride` apart. */
function blur(from: Float32Array, to: Float32Array, length: number, lines: number, step: number, stride: number) {
  const span = 2 * RADIUS + 1;
  for (let line = 0; line < lines; line++) {
    const base = line * stride;
    let sum = 0;
    for (let i = -RADIUS; i <= RADIUS; i++) sum += from[base + Math.min(length - 1, Math.max(0, i)) * step];
    for (let i = 0; i < length; i++) {
      to[base + i * step] = sum / span;
      const add = Math.min(length - 1, i + RADIUS + 1);
      const drop = Math.max(0, i - RADIUS);
      sum += from[base + add * step] - from[base + drop * step];
    }
  }
}

/**
 * The outline of `selected` and of `hover`, IDs in `ids`, and of `behind`, where the selected
 * object would show with nothing in front of it, as RGBA `width` by `height`.
 */
export function drawOutline(ids: ArrayLike<number>, width: number, height: number, selected: number | null, hover: number | null,
  behind: Uint8Array | null): Uint8ClampedArray<ArrayBuffer> {
  const out = new Uint8ClampedArray(width * height * 4);
  if (behind && behind.length === width * height && selected !== null) {
    drawShape(out, width, height, (p) => behind[p] === 1, BEHIND);
  }
  if (hover !== null && hover !== 0 && hover !== selected) {
    drawShape(out, width, height, (p) => ids[p] === hover, HOVER);
  }
  if (selected !== null) {
    drawShape(out, width, height, (p) => ids[p] === selected, SELECTED);
  }
  return out;
}
