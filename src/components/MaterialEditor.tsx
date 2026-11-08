import { useState } from "react";
import type { MaterialOverride, MaterialPreset, SceneMaterial, SurfaceMaterial } from "../types";
import { Select } from "./Select";
import { Slider } from "./Slider";

const PRESETS: Record<MaterialPreset, { roughness: number; metallic: number; ior: number }> = {
  metal: { roughness: 0.2, metallic: 1, ior: 1.5 },
  plastic: { roughness: 0.3, metallic: 0, ior: 1.5 },
  ceramic: { roughness: 0.15, metallic: 0, ior: 1.5 },
  glass: { roughness: 0, metallic: 0, ior: 1.5 },
  custom: { roughness: 0.4, metallic: 0, ior: 1.5 },
};
const hex = (rgb: number[]) => "#" + rgb.map((v) => Math.round(Math.max(0, Math.min(1, v)) * 255).toString(16).padStart(2, "0")).join("");

export function MaterialEditor({ materials, overrides, onChange }: {
  materials: SceneMaterial[];
  overrides: MaterialOverride[];
  onChange: (value: MaterialOverride[]) => void;
}) {
  const [selected, setSelected] = useState(String(materials[0]?.id ?? ""));
  const material = materials.find((m) => String(m.id) === selected);
  if (!material) return null;
  const edit = overrides.find((m) => m.id === material.id);
  const current: SurfaceMaterial = edit?.surface ?? material.surface ?? {
    preset: "custom", ...PRESETS.custom, ior: Math.min(3, Math.max(1, material.ior)), color: material.color,
  };
  const color = current.color ?? material.color;
  const apply = (surface: SurfaceMaterial) => onChange([...overrides.filter((m) => m.id !== material.id), { id: material.id, surface }]);
  const active = edit !== undefined;
  return (
    <div className="group">
      <h2>Materials</h2>
      <Select label="Object material" value={selected} onChange={setSelected}>
        {materials.map((m) => <option key={m.id} value={String(m.id)}>{m.label}</option>)}
      </Select>
      <Select<MaterialPreset | "original"> label="Material preset" value={active ? current.preset : "original"} onChange={(preset) => {
        if (preset === "original") onChange(overrides.filter((m) => m.id !== material.id));
        else apply({ ...(preset === "custom" ? current : PRESETS[preset]), preset, color });
      }}>
        <option value="original">Original scene material</option>
        <option value="metal">Metal</option><option value="plastic">Plastic</option>
        <option value="ceramic">Ceramic</option><option value="glass">Glass</option><option value="custom">Custom</option>
      </Select>
      {active && (
        <div className="stack">
          <label className="field-row"><span>Base color</span>
            <input type="color" aria-label="Material base color" value={hex(color)} onChange={(e) => {
              const value = e.target.value;
              const rgb = [1, 3, 5].map((i) => parseInt(value.slice(i, i + 2), 16) / 255) as [number, number, number];
              apply({ ...current, color: rgb });
            }} />
          </label>
          {current.preset !== "glass" && <>
            <Slider label="Roughness" value={current.roughness} min={0} max={1} step={0.01} format={(v) => v.toFixed(2)}
              onChange={(roughness) => apply({ ...current, preset: "custom", roughness })} />
            <Slider label="Metallic" value={current.metallic} min={0} max={1} step={0.01} format={(v) => v.toFixed(2)}
              onChange={(metallic) => apply({ ...current, preset: "custom", metallic })} />
          </>}
          <Slider label="IOR" value={current.ior} min={1} max={3} step={0.01} disabled={current.metallic === 1}
            format={(v) => v.toFixed(2)} onChange={(ior) => apply({ ...current, ior })} />
          <p className="note">{current.preset === "glass" ? "Clear glass with the existing transmission model." : "GGX reflection. Higher roughness spreads reflections."} Changes apply to this session; Original restores the scene material.</p>
        </div>
      )}
    </div>
  );
}
