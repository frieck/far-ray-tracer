import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import type { MaterialOverride, MaterialPreset, SceneMaterial, SurfaceMaterial } from "../types";
import { Select } from "./Select";
import { Slider } from "./Slider";

const PRESETS: Record<Exclude<MaterialPreset, "custom">, { roughness: number; metallic: number; ior: number }> = {
  metal: { roughness: 0.2, metallic: 1, ior: 1.5 },
  plastic: { roughness: 0.3, metallic: 0, ior: 1.5 },
  ceramic: { roughness: 0.15, metallic: 0, ior: 1.5 },
  glass: { roughness: 0, metallic: 0, ior: 1.5 },
};
const PRESET_NAMES: Record<MaterialPreset, string> = {
  metal: "Metal", plastic: "Plastic", ceramic: "Ceramic", glass: "Glass", custom: "Custom",
};
/** Pixels of the rendered preview; the panel shows it at about half, sharp on Retina screens. */
const PREVIEW_SIZE = 384;
/** Waits this long after the last edit, so dragging a slider renders once it settles. */
const PREVIEW_DELAY_MS = 120;

const hex = (rgb: number[]) => "#" + rgb.map((v) => Math.round(Math.max(0, Math.min(1, v)) * 255).toString(16).padStart(2, "0")).join("");

/** The preset these values match, glass aside, or Custom. */
function matchingPreset(surface: SurfaceMaterial): MaterialPreset {
  const found = (["metal", "plastic", "ceramic"] as const).find((name) => {
    const preset = PRESETS[name];
    return preset.roughness === surface.roughness && preset.metallic === surface.metallic && preset.ior === surface.ior;
  });
  return found ?? "custom";
}

/** Renders the material on a ball with the engine, again after each edit. */
function MaterialPreview({ id, surface, scenePath }: { id: number; surface: SurfaceMaterial | null; scenePath: string }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const request = useRef(0);
  const [busy, setBusy] = useState(true);
  const key = JSON.stringify(surface);

  useEffect(() => {
    const current = ++request.current;
    setBusy(true);
    const timer = setTimeout(() => {
      invoke<ArrayBuffer>("material_preview", { id, surface, size: PREVIEW_SIZE })
        .then((buffer) => {
          if (current !== request.current || !canvas.current) return;
          const pixels = new ImageData(new Uint8ClampedArray(buffer), PREVIEW_SIZE, PREVIEW_SIZE);
          canvas.current.getContext("2d")?.putImageData(pixels, 0, 0);
          setBusy(false);
        })
        .catch(() => {
          if (current === request.current) setBusy(false);
        });
    }, PREVIEW_DELAY_MS);
    return () => clearTimeout(timer);
    // `key` stands for `surface`, so an equal object does not render again.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, key, scenePath]);

  return (
    <div className="material-preview" data-busy={busy || undefined}>
      <canvas ref={canvas} width={PREVIEW_SIZE} height={PREVIEW_SIZE} aria-label="Material preview" role="img" />
    </div>
  );
}

export function MaterialEditor({ materials, overrides, scenePath, onChange }: {
  materials: SceneMaterial[];
  overrides: MaterialOverride[];
  scenePath: string;
  onChange: (value: MaterialOverride[]) => void;
}) {
  const [selected, setSelected] = useState(String(materials[0]?.id ?? ""));
  const material = materials.find((m) => String(m.id) === selected);
  if (!material) return <p className="note">This scene has no materials to edit.</p>;
  const edit = overrides.find((m) => m.id === material.id);
  const original: SurfaceMaterial = material.surface ?? {
    preset: "custom", roughness: 0.4, metallic: 0, ior: Math.min(3, Math.max(1, material.ior)), color: material.color,
  };
  const current = edit?.surface ?? original;
  const color = current.color ?? material.color;
  const glass = current.preset === "glass";
  const apply = (surface: SurfaceMaterial) => onChange([...overrides.filter((m) => m.id !== material.id), { id: material.id, surface }]);
  /** Any edit makes an override; the preset follows the values. */
  const change = (values: Partial<SurfaceMaterial>) => {
    const next = { ...current, color, ...values };
    apply({ ...next, preset: glass ? "glass" : matchingPreset(next) });
  };

  return (
    <div className="group">
      <Select label="Object material" value={selected} onChange={setSelected}>
        {materials.map((m) => <option key={m.id} value={String(m.id)}>{m.label}</option>)}
      </Select>
      <MaterialPreview id={material.id} surface={edit?.surface ?? null} scenePath={scenePath} />
      <Select<MaterialPreset | "original"> label="Material preset" value={edit ? current.preset : "original"} onChange={(preset) => {
        if (preset === "original") onChange(overrides.filter((m) => m.id !== material.id));
        else if (preset === "custom") apply({ ...current, color, preset: "custom" });
        else apply({ ...PRESETS[preset], preset, color });
      }}>
        <option value="original">Original{material.surface ? ` (${PRESET_NAMES[material.surface.preset]})` : ""}</option>
        <option value="metal">Metal</option>
        <option value="plastic">Plastic</option>
        <option value="ceramic">Ceramic</option>
        <option value="glass">Glass</option>
        <option value="custom">Custom</option>
      </Select>
      <label className="field-row"><span>Base color</span>
        <input type="color" aria-label="Material base color" value={hex(color)} onChange={(e) => {
          const value = e.target.value;
          change({ color: [1, 3, 5].map((i) => parseInt(value.slice(i, i + 2), 16) / 255) as [number, number, number] });
        }} />
      </label>
      <Slider label="Roughness" value={current.roughness} min={0} max={1} step={0.01} disabled={glass}
        format={(v) => v.toFixed(2)} onChange={(roughness) => change({ roughness })} />
      <Slider label="Metallic" value={current.metallic} min={0} max={1} step={0.01} disabled={glass}
        format={(v) => v.toFixed(2)} onChange={(metallic) => change({ metallic })} />
      <Slider label="IOR" value={current.ior} min={1} max={3} step={0.01} disabled={current.metallic === 1}
        format={(v) => v.toFixed(2)} onChange={(ior) => change({ ior })} />
      <p className="note">
        {glass ? "Clear glass with the existing transmission model." : "GGX reflection. Higher roughness spreads reflections."}{" "}
        Edits last for this session; Original restores the scene material.
      </p>
    </div>
  );
}
