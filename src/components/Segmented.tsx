import type { CSSProperties } from "react";

interface Option<T> {
  value: T;
  label: string;
  disabled?: boolean;
}

interface SegmentedProps<T> {
  label: string;
  value: T;
  options: Option<T>[];
  onChange: (value: T) => void;
  disabled?: boolean;
}

export function Segmented<T extends string | number>({ label, value, options, onChange, disabled }: SegmentedProps<T>) {
  const index = options.findIndex((option) => option.value === value);
  const style = { "--count": options.length, "--index": index } as CSSProperties;
  return (
    <div className="segmented" role="radiogroup" aria-label={label} style={style} data-disabled={disabled || undefined}>
      <span className="segmented-thumb" />
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          role="radio"
          aria-checked={option.value === value}
          disabled={disabled || option.disabled}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}
