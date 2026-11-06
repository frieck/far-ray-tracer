import { useEffect, useState } from "react";

interface NumberFieldProps {
  label: string;
  value: number;
  min: number;
  max: number;
  onChange: (value: number) => void;
  disabled?: boolean;
}

/** Applies the value on Enter or when leaving the field, so typing does not start renders. */
export function NumberField({ label, value, min, max, onChange, disabled }: NumberFieldProps) {
  const [text, setText] = useState(String(value));

  useEffect(() => setText(String(value)), [value]);

  const commit = () => {
    const parsed = Math.round(Number(text));
    const next = Number.isFinite(parsed) && text.trim() !== "" ? Math.min(Math.max(parsed, min), max) : value;
    setText(String(next));
    if (next !== value) onChange(next);
  };

  return (
    <input
      className="number-field"
      type="text"
      inputMode="numeric"
      aria-label={label}
      value={text}
      disabled={disabled}
      onChange={(event) => setText(event.target.value.replace(/[^0-9]/g, ""))}
      onFocus={(event) => event.target.select()}
      onBlur={commit}
      onKeyDown={(event) => {
        if (event.key === "Enter") event.currentTarget.blur();
      }}
    />
  );
}
