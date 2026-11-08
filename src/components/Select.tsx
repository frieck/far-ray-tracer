import { ChevronDown } from "lucide-react";
import type { ReactNode } from "react";

interface SelectProps<T extends string> {
  label: string;
  value: T;
  onChange: (value: T) => void;
  disabled?: boolean;
  children: ReactNode;
}

export function Select<T extends string>({ label, value, onChange, disabled, children }: SelectProps<T>) {
  return (
    <span className="select" data-disabled={disabled || undefined}>
      <select aria-label={label} value={value} disabled={disabled} onChange={(event) => onChange(event.target.value as T)}>
        {children}
      </select>
      <ChevronDown size={14} aria-hidden="true" />
    </span>
  );
}
