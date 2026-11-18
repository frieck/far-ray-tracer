import { Headphones } from "lucide-react";
import { blackbody, MAX_LIGHT_KELVIN, MIN_LIGHT_KELVIN, shown } from "../kelvin";
import { aimable, aimAt, editedEntry as edited, fileName, LIGHT_KINDS, powerOf, switchKind, type SwitchableKind } from "../lightKinds";
import { entryValue } from "../objectFields";
import type { SceneLightInfo, Selection } from "../types";
import { CheckBox, CheckRow, Choice, ColorRow, Hint, Row, Scrub, ScrubRow, Section } from "./props";

type Vec3 = [number, number, number];
type Patch = Record<string, unknown>;

/** A power in watts as the panel shows it. */
const wattsLabel = (value: number) => `${Number(value.toPrecision(3))} W`;

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

/** A light as edited: its file's entry with the edits over it. */
const editedEntry = (light: SceneLightInfo, edits: Patch | undefined) => edited(light.source, edits);

/**
 * A lamp's light in watts, as Blender's Power: the light it gives in all, so resizing it keeps
 * it. A lamp the file gives an intensity is shown in watts, and is saved by its power once
 * edited.
 */
export function PowerRow({ light, edits, unit, onChange }: {
  light: SceneLightInfo;
  edits: Patch | undefined;
  /** A scene unit, in metres. */
  unit: number;
  onChange: (patch: Patch) => void;
}) {
  const power = powerOf(editedEntry(light, edits), unit);
  if (power === null) return null;
  return (
    <ScrubRow label="Power" keywords="intensity strength brightness watts" value={power} min={1} max={10000} hard={[0, 1e8]} step={0.1}
      log format={wattsLabel}
      hint="The light it gives in all, in watts, measured with the scene unit in the Camera tab: 40 W lights a white wall a metre away, 1000 W one five metres away. A smaller lamp of the same power is brighter to look at, not stronger."
      onChange={(value) => onChange({ power: Number(value.toPrecision(4)), intensity: null })} />
  );
}

/**
 * What kind of light it is, as V-Ray's light types: a sphere, a spotlight, an IES fixture or
 * an area light. Switching keeps its place, color and power, and where it points; an IES
 * light asks for its file first.
 */
export function LightKindRow({ light, edits, center, unit, onChange, onChooseIes }: {
  light: SceneLightInfo;
  edits: Patch | undefined;
  /** The scene's center, where a 2008 light turned physical keeps its brightness. */
  center: Vec3 | null;
  /** A scene unit, in metres. */
  unit: number;
  onChange: (patch: Patch) => void;
  onChooseIes: () => Promise<string | null>;
}) {
  const choose = async (kind: SwitchableKind) => {
    const entry = editedEntry(light, edits);
    if (kind !== "ies") {
      onChange(switchKind(entry, kind, undefined, center, unit));
      return;
    }
    const file = await onChooseIes();
    if (file) onChange(switchKind(entry, "ies", file, center, unit));
  };
  const point = light.kind === "point";
  return (
    <Row label="Kind" keywords="light type sphere spot spotlight ies area rectangle panel 2008 point"
      hint={point
        ? "A 2008 light does not fade with distance. Another kind turns it into a physical light as bright at the scene's center; the 2008 stage still renders it as before. Reset brings it back."
        : "Switching keeps where it is, its color and power, and where it points. An IES light asks for its file."}>
      <Choice<string> label="Kind" value={light.kind} onChange={(kind) => void choose(kind as SwitchableKind)}>
        {point && <option value="point" disabled>2008 point light</option>}
        {LIGHT_KINDS.map(({ kind, label }) => <option key={kind} value={kind}>{label}</option>)}
      </Choice>
    </Row>
  );
}

/** Aiming a light at an object of the scene, in the Object tab with the rest of its place. */
export function LightAimRow({ light, edits, objects, onChange }: {
  light: SceneLightInfo;
  edits: Patch | undefined;
  /** What it can be aimed at: each object's name and where it is. */
  objects: { name: string; point: Vec3 }[];
  onChange: (patch: Patch) => void;
}) {
  if (!aimable(light.kind) || objects.length === 0) return null;
  const entry = editedEntry(light, edits);
  return (
    <Row label="Aim at" keywords="point target look direction"
      hint={light.kind === "rectangle" ? "Turns the panel so its front faces the object." : "Points it at the object."}>
      <Choice<string> label="Aim at" value="" onChange={(value) => {
        const target = objects[Number(value)];
        const patch = target && aimAt(entry, target.point);
        if (patch) onChange(patch);
      }}>
        <option value="" disabled>An object…</option>
        {objects.map((object, k) => <option key={k} value={String(k)}>{object.name}</option>)}
      </Choice>
    </Row>
  );
}

/** An IES light's file, with a button to load another. */
export function LightIesRow({ light, edits, onChange, onChooseIes }: {
  light: SceneLightInfo;
  edits: Patch | undefined;
  onChange: (patch: Patch) => void;
  onChooseIes: () => Promise<string | null>;
}) {
  if (light.kind !== "ies") return null;
  const entry = editedEntry(light, edits);
  const ies = typeof entry.ies === "string" ? entry.ies : null;
  return (
    <Row label="IES file" keywords="profile photometric fixture" hint={ies ?? undefined}>
      <div className="prop-buttons">
        <span className="light-file" title={ies ?? undefined}>{ies ? fileName(ies) : "None"}</span>
        <button type="button" className="prop-mini" onClick={() => void onChooseIes().then((file) => file && onChange({ ies: file }))}>
          Choose…
        </button>
      </div>
    </Row>
  );
}

/** Whether the camera sees a light, as V-Ray's Invisible; a 2008 point light has no body to hide. */
export function LightCameraRow({ light, edits, onChange }: { light: SceneLightInfo; edits: Patch | undefined; onChange: (patch: Patch) => void }) {
  if (light.kind === "point" || light.kind === "portal") return null;
  const entry = editedEntry(light, edits);
  return (
    <CheckRow label="Seen by camera" keywords="invisible visible hidden camera"
      hint="Off, the camera looks through it: its light, its reflections and what it shows through glass stay."
      checked={entry.invisible !== true} onChange={(seen) => onChange({ invisible: seen ? null : true })} />
  );
}

interface LightsPanelProps {
  lights: SceneLightInfo[];
  /** A scene unit, in metres, which lamps' watts are measured with. */
  unit: number;
  /** Deleted lights, left out of the list. */
  removed: number[];
  edits: Record<number, Patch>;
  hidden: number[];
  selection: Selection;
  onEdit: (id: number, patch: Patch) => void;
  onHiddenChange: (hidden: number[]) => void;
  onSelect: (selection: Selection) => void;
}

/**
 * Every light of the scene in one list, as V-Ray's Light Lister: each one on or off, its color
 * and its power, and solo to see its light alone.
 */
export function LightsPanel({ lights, unit, removed, edits, hidden, selection, onEdit, onHiddenChange, onSelect }: LightsPanelProps) {
  const all = lights.map((_, id) => id);
  const soloed = (id: number) => hidden.length === lights.length - 1 && !hidden.includes(id);
  return (
    <Section id="lights" title="Lights">
      {lights.length === 0 && <Hint>This scene has no lights: glowing objects and the environment light it.</Hint>}
      <ul className="light-list" aria-label="Lights">
        {lights.map((light, id) => {
          // A portal gives no light of its own: it is in the outliner and the Object tab.
          if (removed.includes(id) || light.kind === "portal") return null;
          const on = !hidden.includes(id);
          const { kelvin, rgb } = colorOf(light, edits[id]);
          const power = powerOf(editedEntry(light, edits[id]), unit);
          const selected = selection?.kind === "light" && selection.id === id;
          return (
            <li key={id} className="light-item" data-off={!on || undefined} data-selected={selected || undefined}>
              <div className="light-head">
                <CheckBox label={`${light.name} on`} checked={on}
                  onChange={(checked) => onHiddenChange(checked ? hidden.filter((h) => h !== id) : [...hidden, id].sort((a, b) => a - b))} />
                <button type="button" className="light-name" onClick={() => onSelect({ kind: "light", id })}
                  title="Select it, to edit it below or move it in the Object tab">
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
                {power !== null && (
                  <Scrub label={`${light.name} power`} value={power} min={1} max={10000} hard={[0, 1e8]} step={0.1} log
                    format={wattsLabel} onChange={(value) => onEdit(id, { power: Number(value.toPrecision(4)), intensity: null })} />
                )}
              </div>
            </li>
          );
        })}
      </ul>
      {lights.length > 0 && (
        <Hint>Solo leaves glowing objects and the environment on. Select a light to edit it below, and move it in the Object tab.</Hint>
      )}
    </Section>
  );
}
