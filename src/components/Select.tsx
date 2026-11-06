import { ChevronDown } from "lucide-react";
import type { ReactNode } from "react";

interface SelectProps<T extends string> {
  label: string;
  value: T;
  onChange: (value: T) => void;
  children: ReactNode;
}

export function Select<T extends string>({ label, value, onChange, children }: SelectProps<T>) {
  return (
    <span className="select">
      <select aria-label={label} value={value} onChange={(event) => onChange(event.target.value as T)}>
        {children}
      </select>
      <ChevronDown size={14} aria-hidden="true" />
    </span>
  );
}
