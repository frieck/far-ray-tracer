import type { HTMLAttributes } from "react";

export function GlassPanel({ className = "", ...props }: HTMLAttributes<HTMLElement>) {
  return <section className={`glass ${className}`} {...props} />;
}
