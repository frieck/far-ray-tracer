const number = new Intl.NumberFormat("en-US", { maximumFractionDigits: 1 });

export function formatNumber(n: number): string {
  return number.format(n);
}

export function formatTime(ms: number): string {
  return ms < 1000 ? `${Math.round(ms)} ms` : `${number.format(ms / 1000)} s`;
}

export function formatRays(n: number): string {
  if (n >= 1e9) return `${number.format(n / 1e9)} billion rays`;
  if (n >= 1e6) return `${number.format(n / 1e6)} million rays`;
  return `${number.format(n)} rays`;
}

/** Turns a file name such as `plane-and-mesh` into `Plane and mesh`. */
export function prettyName(name: string): string {
  const text = name.replace(/-/g, " ");
  return text.charAt(0).toUpperCase() + text.slice(1);
}
