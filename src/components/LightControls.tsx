import { Headphones } from "lucide-react";
import { blackbody, MAX_LIGHT_KELVIN, MIN_LIGHT_KELVIN, shown } from "../kelvin";
import { entryValue } from "../objectFields";
import type { SceneLightInfo, Selection } from "../types";
import { CheckBox, Choice, ColorRow, Hint, Row, Scrub, ScrubRow, Section } from "./props";

type Vec3 = [number, number, number];
type Patch = Record<string, unknown>;

/** Lights that take an intensity; a 2008 point light's color alone sets how bright it is. */
const HAS_INTENSITY = new Set(["rectangle", "sphere", "spot", "ies"]);

/** A light's color as the file gives it: a temperature in kelvin, or an RGB color. */
function colorOf(light: SceneLightInfo, edits: Patch | undefined): { kelvin: number | null; rgb: Vec3 } {
  const kelvin = entryValue("temperature", light.source, edits);
  const rgb = (entryValue("color", light.source, edits) as Vec3 | undefined) ?? [1, 1, 1];
  return { kelvin: typeof kelvin === "number" ? kelvin : null, rgb };
}

/** The color a light gives off, its temperature's times its own, within 0 to 1 for a swatch. */
function swatch(light: SceneLightInfo, edits: Patch | undefined): string {
  const { kelvin, rgb } = colorOf(light, edits);
  const tint = kelvin === null ? [1, 1, 1] : blackbody(kelvin);
  const [r, g, b] = shown(rgb.map((v, k) => v * tint[k]));
  return `rgb(${Math.round(r * 255)}, ${Math.round(g * 255)}, ${Math.round(b * 255)})`;
}

const kelvinLabel = (value: number) => `${Math.round(value)} K`;

/**
 * A light's color, as V-Ray offers it: an RGB color, or a color temperature in kelvin, warm
 * below daylight's 6500 K and cool above. Switching keeps the color as it looks.
 */
export function LightColorRows({ light, edits, onChange }: { light: SceneLightInfo; edits: Patch | undefined; onChange: (patch: Patch) => void }) {
  const { kelvin, rgb } = colorOf(light, edits);
  return (
    <>
      <Row label="Color by" keywords="light temperature kelvin">
        <Choice<"rgb" | "kelvin"> label="Color by" value={kelvin === null ? "rgb" : "kelvin"}
          onChange={(mode) => onChange(mode === "kelvin"
            ? { temperature: 6500, color: null }
            : { temperature: null, color: shown(blackbody(kelvin ?? 6500)) })}>
          <option value="rgb">Color</option>
          <option value="kelvin">Temperature</option>
        </Choice>
      </Row>
      {kelvin === null ? (
        <ColorRow label="Color" value={rgb} onChange={(color) => onChange({ color })} />
      ) : (
        <ScrubRow label="Temperature" keywords="kelvin warm cool light color" value={kelvin} min={1500} max={12000}
          hard={[MIN_LIGHT_KELVIN, MAX_LIGHT_KELVIN]} step={50} format={kelvinLabel}
          hint="About 1900 K for a candle, 2700 K a household bulb, 4000 K a cool white tube, 5500 K the midday sun and 6500 K daylight, the screen's white."
          onChange={(temperature) => onChange({ temperature })} />
      )}
    </>
  );
}

interface LightsPanelProps {
  lights: SceneLightInfo[];
  edits: Record<number, Patch>;
  hidden: number[];
  selection: Selection;
  onEdit: (id: number, patch: Patch) => void;
  onHiddenChange: (hidden: number[]) => void;
  onSelect: (selection: Selection) => void;
}

/**
 * Every light of the scene in one list, as V-Ray's Light Lister: each one on or off, its color
 * and its intensity, and solo to see its light alone.
 */
export function LightsPanel({ lights, edits, hidden, selection, onEdit, onHiddenChange, onSelect }: LightsPanelProps) {
  const all = lights.map((_, id) => id);
  const soloed = (id: number) => hidden.length === lights.length - 1 && !hidden.includes(id);
  return (
    <Section id="lights" title="Lights">
      {lights.length === 0 && <Hint>This scene has no lights: glowing objects and the environment light it.</Hint>}
      <ul className="light-list" aria-label="Lights">
        {lights.map((light, id) => {
          const on = !hidden.includes(id);
          const { kelvin, rgb } = colorOf(light, edits[id]);
          const intensity = entryValue("intensity", light.source, edits[id]);
          const selected = selection?.kind === "light" && selection.id === id;
          return (
            <li key={id} className="light-item" data-off={!on || undefined} data-selected={selected || undefined}>
              <div className="light-head">
                <CheckBox label={`${light.name} on`} checked={on}
                  onChange={(checked) => onHiddenChange(checked ? hidden.filter((h) => h !== id) : [...hidden, id].sort((a, b) => a - b))} />
                <button type="button" className="light-name" onClick={() => onSelect({ kind: "light", id })}
                  title="Select it, to move it or edit the rest in the Object tab">
                  {light.name}
                </button>
                {lights.length > 1 && (
                  <button type="button" className="prop-mini light-solo" aria-pressed={soloed(id)}
                    title={soloed(id) ? "Turn every light back on" : "Turn every other light off"}
                    onClick={() => onHiddenChange(soloed(id) ? [] : all.filter((other) => other !== id))}>
                    <Headphones size={11} aria-hidden="true" />
                    Solo
                  </button>
                )}
              </div>
              <div className="light-controls">
                {kelvin === null ? (
                  <label className="light-swatch" title="Color">
                    <span style={{ background: swatch(light, edits[id]) }} />
                    <input type="color" aria-label={`${light.name} color`}
                      value={"#" + rgb.map((v) => Math.round(Math.max(0, Math.min(1, v)) * 255).toString(16).padStart(2, "0")).join("")}
                      onChange={(event) => {
                        const v = event.target.value;
                        onEdit(id, { color: [1, 3, 5].map((i) => parseInt(v.slice(i, i + 2), 16) / 255) });
                      }} />
                  </label>
                ) : (
                  <div className="light-kelvin" title="Color temperature">
                    <span className="light-swatch-dot" style={{ background: swatch(light, edits[id]) }} />
                    <Scrub label={`${light.name} temperature`} value={kelvin} min={1500} max={12000}
                      hard={[MIN_LIGHT_KELVIN, MAX_LIGHT_KELVIN]} step={50} format={kelvinLabel}
                      onChange={(temperature) => onEdit(id, { temperature })} />
                  </div>
                )}
                {HAS_INTENSITY.has(light.kind) && (
                  <Scrub label={`${light.name} intensity`} value={typeof intensity === "number" ? intensity : 1} min={0} max={50}
                    hard={[0, 1e6]} step={0.05} format={(v) => `× ${Number(v.toFixed(2))}`}
                    onChange={(value) => onEdit(id, { intensity: value })} />
                )}
              </div>
            </li>
          );
        })}
      </ul>
      {lights.length > 0 && (
        <Hint>Solo leaves glowing objects and the environment on. Select a light to move it, or switch its color to a temperature in the Object tab.</Hint>
      )}
    </Section>
  );
}
