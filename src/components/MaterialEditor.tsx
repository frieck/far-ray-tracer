import { Maximize2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { drawPreview, renderMaterialBall } from "../materialPreview";
import type { BumpInfo, MaterialOverride, MaterialPreset, SceneMaterial, SurfaceMaterial } from "../types";
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
/**
 * Each edit renders a quick, coarse ball first and then the final one. The quick pass takes tens
 * of milliseconds, so the ball follows a dragged slider; the final pass refines it once it stops.
 */
const PREVIEW_PASSES = [
  { size: PREVIEW_SIZE / 2, samples: 4 },
  { size: PREVIEW_SIZE, samples: 24 },
];
/** Final previews already rendered, by scene, material and edit. */
const previewCache = new Map<string, ImageData>();
const MAX_CACHED_PREVIEWS = 64;

const hex = (rgb: number[]) => "#" + rgb.map((v) => Math.round(Math.max(0, Math.min(1, v)) * 255).toString(16).padStart(2, "0")).join("");

/** Whether a surface uses any layer beyond plain GGX, which no preset has. */
const layered = (s: SurfaceMaterial) =>
  !!(s.clearcoat || s.sheen || s.anisotropy || s.subsurface);

/** The preset these values match, glass aside, or Custom. */
function matchingPreset(surface: SurfaceMaterial): MaterialPreset {
  if (layered(surface)) return "custom";
  const found = (["metal", "plastic", "ceramic"] as const).find((name) => {
    const preset = PRESETS[name];
    return preset.roughness === surface.roughness && preset.metallic === surface.metallic && preset.ior === surface.ior;
  });
  return found ?? "custom";
}

/** Renders the material on a shader ball with the engine, again after each edit. */
function MaterialPreview({ id, edit, scenePath, onExpand }: {
  id: number;
  edit: MaterialOverride | null;
  scenePath: string;
  onExpand: () => void;
}) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const request = useRef(0);
  const [busy, setBusy] = useState(true);
  const key = `${scenePath}|${id}|${JSON.stringify(edit)}`;

  useEffect(() => {
    const current = ++request.current;
    const cached = previewCache.get(key);
    if (cached) {
      drawPreview(canvas.current, cached);
      setBusy(false);
      return;
    }
    setBusy(true);
    void (async () => {
      for (const { size, samples } of PREVIEW_PASSES) {
        let pixels: ImageData;
        try {
          pixels = await renderMaterialBall({ id, edit, width: size, height: size, samples, pose: null, view: "thumbnail" });
        } catch {
          // A newer edit replaced this preview, or the scene closed.
          if (current === request.current) setBusy(false);
          return;
        }
        if (current !== request.current) return;
        drawPreview(canvas.current, pixels);
        if (size === PREVIEW_SIZE) {
          if (previewCache.size >= MAX_CACHED_PREVIEWS) previewCache.delete(previewCache.keys().next().value!);
          previewCache.set(key, pixels);
        }
      }
      if (current === request.current) setBusy(false);
    })();
    // `key` stands for `id`, `edit` and the scene, so an equal edit does not render again.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  return (
    <div className="material-preview" data-busy={busy || undefined}>
      <canvas ref={canvas} width={PREVIEW_SIZE} height={PREVIEW_SIZE} aria-label="Material preview" role="img" />
      <button type="button" className="icon floating" aria-label="Show large" title="Show large, and orbit around it" onClick={onExpand}>
        <Maximize2 size={15} aria-hidden="true" />
      </button>
    </div>
  );
}

export function MaterialEditor({ materials, overrides, scenePath, selected, onSelect, onExpand, onChange }: {
  materials: SceneMaterial[];
  overrides: MaterialOverride[];
  scenePath: string;
  /** The material being edited; null picks the first. */
  selected: number | null;
  onSelect: (id: number) => void;
  onExpand: () => void;
  onChange: (value: MaterialOverride[]) => void;
}) {
  const material = materials.find((m) => m.id === selected) ?? materials[0];
  if (!material) return <p className="note">This scene has no materials to edit.</p>;
  const edit = overrides.find((m) => m.id === material.id);
  const original: SurfaceMaterial = material.surface ?? {
    preset: "custom", roughness: 0.4, metallic: 0, ior: Math.min(3, Math.max(1, material.ior)), color: material.color,
  };
  const current = edit?.surface ?? original;
  const color = current.color ?? material.color;
  const glass = current.preset === "glass";
  /** Stores the edit, or drops it when nothing is left to change. */
  const store = (next: Omit<MaterialOverride, "id">) => {
    const others = overrides.filter((m) => m.id !== material.id);
    onChange(next.surface || next.bump_strength !== null ? [...others, { id: material.id, ...next }] : others);
  };
  const bumpStrength = edit?.bump_strength ?? null;
  const apply = (surface: SurfaceMaterial) => store({ surface, bump_strength: bumpStrength });
  /** Any edit makes an override; the preset follows the values. */
  const change = (values: Partial<SurfaceMaterial>) => {
    const next = { ...current, color, ...values };
    if (glass) {
      apply({ ...next, preset: "glass", metallic: 0, clearcoat: 0, sheen: 0, anisotropy: 0, subsurface: 0 });
    } else {
      apply({ ...next, absorption_depth: null, abbe: null, preset: matchingPreset(next) });
    }
  };

  return (
    <div className="group">
      <Select label="Object material" value={String(material.id)} onChange={(id) => onSelect(Number(id))}>
        {materials.map((m) => <option key={m.id} value={String(m.id)}>{m.label}</option>)}
      </Select>
      <MaterialPreview id={material.id} edit={edit ?? null} scenePath={scenePath} onExpand={onExpand} />
      <Select<MaterialPreset | "original"> label="Material preset" value={edit?.surface ? current.preset : "original"} onChange={(preset) => {
        if (preset === "original") store({ surface: null, bump_strength: bumpStrength });
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
      <Slider label="Roughness" value={current.roughness} min={0} max={1} step={0.01}
        format={(v) => v.toFixed(2)} onChange={(roughness) => change({ roughness })} />
      <Slider label="Metallic" value={current.metallic} min={0} max={1} step={0.01} disabled={glass}
        format={(v) => v.toFixed(2)} onChange={(metallic) => change({ metallic })} />
      <Slider label="IOR" value={current.ior} min={1} max={3} step={0.01} disabled={current.metallic === 1}
        format={(v) => v.toFixed(2)} onChange={(ior) => change({ ior })} />
      {glass ? <GlassControls surface={current} onChange={change} /> : <LayerControls surface={current} onChange={change} />}
      {material.bump && (
        <BumpControl bump={material.bump} strength={bumpStrength}
          onChange={(strength) => store({ surface: edit?.surface ?? null, bump_strength: strength })} />
      )}
      <p className="note">
        {glass ? "Glass: roughness frosts it." : "GGX reflection. Higher roughness spreads reflections."}{" "}
        Edits last for this session; Original restores the scene material.
      </p>
    </div>
  );
}

/** Strength of the material's bump or normal map; null keeps the scene's. */
function BumpControl({ bump, strength, onChange }: {
  bump: BumpInfo;
  strength: number | null;
  onChange: (strength: number | null) => void;
}) {
  const height = bump.kind === "height";
  const value = strength ?? bump.strength;
  // A height is a depth in scene units, so its range follows the scene's own value.
  const max = height ? Math.max(0.2, bump.strength * 4) : 3;
  return (
    <>
      <Slider label={height ? "Bump depth" : "Normal map strength"} value={value} min={0} max={max}
        step={height ? max / 200 : 0.05} format={(v) => (height ? v.toFixed(3) : v.toFixed(2))}
        onChange={(next) => onChange(next === bump.strength ? null : next)} />
      <p className="note">
        {height ? "Height map: depth of the relief, in scene units." : "Normal map: 1 uses the map as drawn."} 0 turns it off.
      </p>
    </>
  );
}

/** Past the end of a slider: the setting is off. */
const OFF_TINT = 5.05;

/** Tint depth and dispersion of glass. */
function GlassControls({ surface, onChange }: { surface: SurfaceMaterial; onChange: (v: Partial<SurfaceMaterial>) => void }) {
  const depth = surface.absorption_depth ?? null;
  const abbe = surface.abbe ?? null;
  // Dispersion from 0, none, to 1, Abbe number 5, far stronger than any real glass.
  const dispersion = abbe === null ? 0 : (100 - abbe) / 95;
  return (
    <>
      <Slider label="Tint depth" value={depth ?? OFF_TINT} min={0.05} max={OFF_TINT} step={0.05}
        format={(v) => (v >= OFF_TINT ? "Off" : v.toFixed(2))}
        onChange={(v) => onChange({ absorption_depth: v >= OFF_TINT ? null : v })} />
      <Slider label="Dispersion" value={dispersion} min={0} max={1} step={0.01}
        format={(v) => (v <= 0 ? "Off" : `Abbe ${Math.round(100 - 95 * v)}`)}
        onChange={(v) => onChange({ abbe: v <= 0 ? null : 100 - 95 * v })} />
      <p className="note">
        Tint depth: how far light travels keeping the glass color; thicker glass is darker. Dispersion splits light
        into a spectrum, best seen in caustics.
      </p>
    </>
  );
}

/** Clear coat, sheen, anisotropy and subsurface of a GGX surface. */
function LayerControls({ surface, onChange }: { surface: SurfaceMaterial; onChange: (v: Partial<SurfaceMaterial>) => void }) {
  const clearcoat = surface.clearcoat ?? 0;
  const sheen = surface.sheen ?? 0;
  const anisotropy = surface.anisotropy ?? 0;
  const subsurface = surface.subsurface ?? 0;
  const sheenColor = surface.sheen_color ?? [1, 1, 1];
  return (
    <>
      <h2>Coat and sheen</h2>
      <Slider label="Clear coat" value={clearcoat} min={0} max={1} step={0.01} format={(v) => v.toFixed(2)}
        onChange={(v) => onChange({ clearcoat: v })} />
      <Slider label="Coat roughness" value={surface.clearcoat_roughness ?? 0.03} min={0} max={1} step={0.01}
        disabled={clearcoat === 0} format={(v) => v.toFixed(2)} onChange={(v) => onChange({ clearcoat_roughness: v })} />
      <Slider label="Sheen" value={sheen} min={0} max={1} step={0.01} format={(v) => v.toFixed(2)}
        onChange={(v) => onChange({ sheen: v })} />
      <label className="field-row"><span>Sheen color</span>
        <input type="color" aria-label="Sheen color" value={hex(sheenColor)} disabled={sheen === 0} onChange={(e) => {
          const value = e.target.value;
          onChange({ sheen_color: [1, 3, 5].map((i) => parseInt(value.slice(i, i + 2), 16) / 255) as [number, number, number] });
        }} />
      </label>
      <h2>Grain</h2>
      <Slider label="Anisotropy" value={anisotropy} min={-1} max={1} step={0.02} format={(v) => v.toFixed(2)}
        onChange={(v) => onChange({ anisotropy: v })} />
      <Slider label="Grain rotation" value={surface.anisotropy_rotation ?? 0} min={0} max={180} step={1}
        disabled={anisotropy === 0} format={(v) => `${v}°`} onChange={(v) => onChange({ anisotropy_rotation: v })} />
      <h2>Subsurface</h2>
      <Slider label="Subsurface" value={subsurface} min={0} max={1} step={0.01} format={(v) => v.toFixed(2)}
        onChange={(v) => onChange({ subsurface: v })} />
      <Slider label="Scatter distance" value={surface.subsurface_radius ?? 0.1} min={0.01} max={1} step={0.01}
        disabled={subsurface === 0} format={(v) => v.toFixed(2)} onChange={(v) => onChange({ subsurface_radius: v })} />
      <p className="note">
        Clear coat varnishes the surface; sheen gives cloth its soft edges; anisotropy stretches highlights along
        the texture direction, as on brushed metal; subsurface lets light travel inside, as in wax or skin.
      </p>
    </>
  );
}
