/**
 * The building blocks of the settings panel, in the manner of a render app's property editor:
 * compact rows with the label on the left, number fields you drag or type into, and sections
 * that fold away. A new setting is one row inside a section; search, folding and the Advanced
 * area then work for it with no further code.
 */
import { Check, ChevronDown, ChevronRight } from "lucide-react";
import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from "react";

/** The search text; rows whose label does not contain it hide themselves. */
export const SearchContext = createContext("");

function matches(query: string, ...texts: (string | undefined)[]): boolean {
  const q = query.trim().toLowerCase();
  return !q || texts.some((text) => text?.toLowerCase().includes(q));
}

function stored(key: string, fallback: boolean): boolean {
  try {
    const value = localStorage.getItem(key);
    return value === null ? fallback : value === "1";
  } catch {
    return fallback;
  }
}

/** A folded or unfolded state the app remembers between sessions. */
function useRemembered(key: string, fallback: boolean): [boolean, (open: boolean) => void] {
  const [open, setOpen] = useState(() => stored(key, fallback));
  const change = (next: boolean) => {
    setOpen(next);
    try {
      localStorage.setItem(key, next ? "1" : "0");
    } catch {
      // Folding is a convenience; without storage it resets on the next launch.
    }
  };
  return [open, change];
}

interface SectionProps {
  id: string;
  title: string;
  /** A feature that can be off: a check box in the header, and the section folds while off. */
  enabled?: { checked: boolean; onChange: (checked: boolean) => void; disabled?: boolean };
  /** Shown at the right of the header, such as a preset menu. */
  extra?: ReactNode;
  defaultOpen?: boolean;
  /** Settings few people change, folded inside the section. */
  advanced?: ReactNode;
  children?: ReactNode;
}

export function Section({ id, title, enabled, extra, defaultOpen = true, advanced, children }: SectionProps) {
  const query = useContext(SearchContext);
  const [open, setOpen] = useRemembered(`far.section.${id}`, defaultOpen);
  const searching = query.trim() !== "";
  const off = enabled && !enabled.checked;
  const titleMatches = searching && matches(query, title);
  // While searching every section opens, so the matching rows show; a section whose title
  // matches shows all its rows.
  const expanded = searching || (open && !off);
  const body = (
    <SearchContext.Provider value={titleMatches ? "" : query}>
      {children}
      {advanced && <Advanced id={id}>{advanced}</Advanced>}
    </SearchContext.Provider>
  );
  return (
    <section className="prop-section" data-off={off || undefined} data-title-match={titleMatches || undefined}>
      <div className="prop-header">
        <button type="button" className="prop-fold" aria-expanded={expanded} disabled={searching}
          onClick={() => setOpen(!open)}>
          <ChevronRight size={13} aria-hidden="true" />
          {!enabled && <span>{title}</span>}
        </button>
        {enabled && (
          <CheckBox label={title} checked={enabled.checked} disabled={enabled.disabled}
            onChange={(checked) => {
              enabled.onChange(checked);
              if (checked && !open) setOpen(true);
            }}>
            <span>{title}</span>
          </CheckBox>
        )}
        <span className="prop-header-space" />
        {extra}
      </div>
      {expanded && <div className="prop-body">{body}</div>}
    </section>
  );
}

function Advanced({ id, children }: { id: string; children: ReactNode }) {
  const query = useContext(SearchContext);
  const [open, setOpen] = useRemembered(`far.advanced.${id}`, false);
  const expanded = open || query.trim() !== "";
  return (
    <div className="prop-advanced">
      {query.trim() === "" && (
        <button type="button" className="prop-advanced-toggle" aria-expanded={expanded} onClick={() => setOpen(!open)}>
          <ChevronDown size={12} aria-hidden="true" />
          Advanced
        </button>
      )}
      {expanded && children}
    </div>
  );
}

/** One setting: the label on the left, its control on the right. */
export function Row({ label, hint, children, keywords }: {
  label: string;
  /** A longer explanation, shown on hover. */
  hint?: string;
  /** Other words that find this row in a search. */
  keywords?: string;
  children: ReactNode;
}) {
  const query = useContext(SearchContext);
  if (!matches(query, label, keywords)) return null;
  return (
    <div className="prop-row" title={hint}>
      <span className="prop-label">{label}</span>
      <div className="prop-control">{children}</div>
    </div>
  );
}

/** A short explanation under the rows; hidden during a search. */
export function Hint({ children, tone }: { children: ReactNode; tone?: "warning" }) {
  const query = useContext(SearchContext);
  if (query.trim()) return null;
  return <p className="prop-hint" data-tone={tone}>{children}</p>;
}

export function CheckBox({ label, checked, onChange, disabled, children }: {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  children?: ReactNode;
}) {
  return (
    <label className="prop-check" data-disabled={disabled || undefined}>
      <input type="checkbox" aria-label={label} checked={checked} disabled={disabled}
        onChange={(event) => onChange(event.target.checked)} />
      <span className="prop-check-box" aria-hidden="true">{checked && <Check size={11} strokeWidth={3} />}</span>
      {children}
    </label>
  );
}

/** A boolean setting as a row. */
export function CheckRow({ label, hint, checked, onChange, disabled }: {
  label: string;
  hint?: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <Row label={label} hint={hint}>
      <CheckBox label={label} checked={checked} onChange={onChange} disabled={disabled} />
    </Row>
  );
}

/** A compact menu, for a row or a section header. */
export function Choice<T extends string>({ label, value, onChange, disabled, children, small }: {
  label: string;
  value: T;
  onChange: (value: T) => void;
  disabled?: boolean;
  children: ReactNode;
  small?: boolean;
}) {
  return (
    <span className={small ? "prop-choice small" : "prop-choice"} data-disabled={disabled || undefined}>
      <select aria-label={label} value={value} disabled={disabled} onChange={(event) => onChange(event.target.value as T)}>
        {children}
      </select>
      <ChevronDown size={12} aria-hidden="true" />
    </span>
  );
}

export function ChoiceRow<T extends string>(props: {
  label: string;
  hint?: string;
  keywords?: string;
  value: T;
  onChange: (value: T) => void;
  disabled?: boolean;
  children: ReactNode;
}) {
  return (
    <Row label={props.label} hint={props.hint} keywords={props.keywords}>
      <Choice label={props.label} value={props.value} onChange={props.onChange} disabled={props.disabled}>
        {props.children}
      </Choice>
    </Row>
  );
}

interface ScrubProps {
  label: string;
  value: number;
  /** The range the fill shows and a drag covers. Typed values may go beyond it. */
  min: number;
  max: number;
  /** Values the field rounds to; integers when 1. */
  step: number;
  onChange: (value: number) => void;
  format?: (value: number) => string;
  disabled?: boolean;
  /**
   * The values the setting can take at all, for typing and the arrow keys. By default nothing
   * caps it above, and it stays at zero or more unless `min` is negative.
   */
  hard?: [number, number];
  /** Keeps every value within min and max: for settings that have no meaning beyond them. */
  strict?: boolean;
  /** Spreads the range evenly in ratio instead of in value, for counts such as passes. */
  log?: boolean;
}

const DRAG_THRESHOLD = 3;

/**
 * A number you drag sideways to change, or click to type, as in Blender. A drag stays within
 * the usual range; typing can go past it, up to the setting's hard limits. The fill shows where
 * the value sits in the usual range, and turns amber beyond it. Shift drags finely; the arrow
 * keys step.
 */
export function Scrub({ label, value, min, max, step, onChange, format, disabled, hard, strict, log }: ScrubProps) {
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState("");
  const drag = useRef<{ x: number; start: number; moved: boolean; width: number } | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const [low, high] = strict ? [min, max] : hard ?? [min < 0 ? -Infinity : 0, Infinity];
  const decimals = Math.max(0, Math.ceil(-Math.log10(step) - 1e-9));
  const show = format ?? ((v: number) => v.toFixed(decimals));
  const toUnit = (v: number) => {
    const clamped = Math.min(Math.max(v, min), max);
    if (log) return Math.log(clamped / min) / Math.log(max / min);
    return max > min ? (clamped - min) / (max - min) : 1;
  };
  const tidy = (v: number) => {
    const rounded = Math.round(v / step) * step;
    return Number(Math.min(Math.max(rounded, low), high).toFixed(decimals));
  };
  const outside = value < min || value > max;

  useEffect(() => {
    if (editing) input.current?.select();
  }, [editing]);

  const commit = () => {
    const parsed = Number(text.replace(",", "."));
    if (text.trim() !== "" && Number.isFinite(parsed)) {
      const next = tidy(parsed);
      if (next !== value) onChange(next);
    }
    setEditing(false);
  };

  if (editing) {
    return (
      <input ref={input} autoFocus className="prop-scrub-input" aria-label={label} value={text} inputMode="decimal"
        title={Number.isFinite(high) || low > -Infinity
          ? `From ${low > -Infinity ? low : "−∞"} to ${Number.isFinite(high) ? high : "∞"}` : undefined}
        onChange={(event) => setText(event.target.value)} onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === "Enter") event.currentTarget.blur();
          if (event.key === "Escape") setEditing(false);
        }} />
    );
  }

  const startEditing = () => {
    setText(String(Number(value.toFixed(decimals))));
    setEditing(true);
  };

  return (
    <div
      className="prop-scrub"
      role="slider"
      tabIndex={disabled ? -1 : 0}
      aria-label={label}
      aria-valuemin={low}
      aria-valuemax={high}
      aria-valuenow={value}
      aria-valuetext={show(value)}
      aria-disabled={disabled || undefined}
      data-outside={outside || undefined}
      onPointerDown={(event) => {
        if (disabled || event.button !== 0) return;
        event.currentTarget.setPointerCapture(event.pointerId);
        drag.current = { x: event.clientX, start: value, moved: false, width: event.currentTarget.clientWidth };
      }}
      onPointerMove={(event) => {
        const d = drag.current;
        if (!d) return;
        const dx = event.clientX - d.x;
        if (!d.moved && Math.abs(dx) < DRAG_THRESHOLD) return;
        d.moved = true;
        // The drag moves from where the value was, the whole usual range across the field.
        const share = (dx / Math.max(40, d.width)) * (event.shiftKey ? 0.1 : 1);
        const moved = log ? Math.max(d.start, 1e-9) * Math.pow(max / min, share) : d.start + share * (max - min);
        // A value typed beyond the range can be dragged back into it, but a drag never leaves it.
        const next = tidy(Math.min(Math.max(moved, Math.min(min, d.start)), Math.max(max, d.start)));
        if (next !== value) onChange(next);
      }}
      onPointerUp={() => {
        const d = drag.current;
        drag.current = null;
        if (d && !d.moved) startEditing();
      }}
      onPointerCancel={() => { drag.current = null; }}
      onKeyDown={(event) => {
        if (disabled) return;
        const direction = event.key === "ArrowRight" || event.key === "ArrowUp" ? 1
          : event.key === "ArrowLeft" || event.key === "ArrowDown" ? -1 : 0;
        if (direction) {
          event.preventDefault();
          onChange(tidy(value + direction * step * (event.shiftKey ? 10 : 1)));
        } else if (event.key === "Enter") {
          startEditing();
        }
      }}
    >
      <span className="prop-scrub-fill" style={{ width: `${toUnit(value) * 100}%` }} />
      <span className="prop-scrub-value">{show(value)}</span>
    </div>
  );
}

/** A number setting as a row. */
export function ScrubRow(props: ScrubProps & { hint?: string; keywords?: string }) {
  const { hint, keywords, ...scrub } = props;
  return (
    <Row label={props.label} hint={hint} keywords={keywords}>
      <Scrub {...scrub} />
    </Row>
  );
}

const hex = (rgb: number[]) => "#" + rgb.map((v) => Math.round(Math.max(0, Math.min(1, v)) * 255).toString(16).padStart(2, "0")).join("");

/** A color setting as a row: a swatch that opens the system color picker. */
export function ColorRow({ label, value, onChange, disabled }: {
  label: string;
  value: [number, number, number];
  onChange: (value: [number, number, number]) => void;
  disabled?: boolean;
}) {
  const color = hex(value);
  return (
    <Row label={label}>
      <label className="prop-color" data-disabled={disabled || undefined}>
        <span className="prop-color-swatch" style={{ background: color }} />
        <span className="prop-color-text">{color.toUpperCase()}</span>
        <input type="color" aria-label={label} value={color} disabled={disabled} onChange={(event) => {
          const v = event.target.value;
          onChange([1, 3, 5].map((i) => parseInt(v.slice(i, i + 2), 16) / 255) as [number, number, number]);
        }} />
      </label>
    </Row>
  );
}

/** Small buttons in a row, such as Reset camera. */
export function ButtonRow({ label, children }: { label: string; children: ReactNode }) {
  return <Row label={label}><div className="prop-buttons">{children}</div></Row>;
}
