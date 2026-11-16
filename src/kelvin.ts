/**
 * The color of a black body, as the engine's `lens::blackbody` gives a light at a color
 * temperature: linear sRGB of unit luminance, from Kim et al.'s fit of the Planckian locus.
 */
export const MIN_LIGHT_KELVIN = 1000;
export const MAX_LIGHT_KELVIN = 40000;

export function blackbody(kelvin: number): [number, number, number] {
  const t = Math.min(25000, Math.max(1667, Number.isFinite(kelvin) ? kelvin : 6500));
  const [t2, t3] = [t * t, t * t * t];
  const x = t <= 4000
    ? -0.2661239e9 / t3 - 0.2343589e6 / t2 + 0.8776956e3 / t + 0.179910
    : -3.0258469e9 / t3 + 2.1070379e6 / t2 + 0.2226347e3 / t + 0.240390;
  const y = t <= 2222
    ? -1.1063814 * x ** 3 - 1.3481102 * x * x + 2.18555832 * x - 0.20219683
    : t <= 4000
      ? -0.9549476 * x ** 3 - 1.37418593 * x * x + 2.09137015 * x - 0.16748867
      : 3.081758 * x ** 3 - 5.8733867 * x * x + 3.75112997 * x - 0.37001483;
  const [X, Z] = [x / y, (1 - x - y) / y];
  const rgb = [
    3.2404542 * X - 1.5371385 - 0.4985314 * Z,
    -0.969266 * X + 1.8760108 + 0.041556 * Z,
    0.0556434 * X - 0.2040259 + 1.0572252 * Z,
  ].map((v) => Math.max(0, v));
  const luminance = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
  return rgb.map((v) => v / Math.max(1e-6, luminance)) as [number, number, number];
}

/** A light's color brought within 0 to 1, its brightest channel 1, for a swatch or a picker. */
export function shown(rgb: number[]): [number, number, number] {
  const top = Math.max(...rgb, 1e-6);
  return rgb.map((v) => Math.round((v / top) * 1e4) / 1e4) as [number, number, number];
}
