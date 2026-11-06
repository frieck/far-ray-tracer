import type { CSSProperties } from "react";

interface SliderProps {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  format?: (value: number) => string;
  onChange: (value: number) => void;
  disabled?: boolean;
}

export function Slider({ label, value, min, max, step, format = String, onChange, disabled }: SliderProps) {
  const fill = max > min ? ((value - min) / (max - min)) * 100 : 100;
  return (
    <label className="slider" data-disabled={disabled || undefined}>
      <span className="row">
        <span>{label}</span>
        <output>{format(value)}</output>
      </span>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        style={{ "--fill": `${fill}%` } as CSSProperties}
        onChange={(event) => onChange(Number(event.target.value))}
      />
    </label>
  );
}
