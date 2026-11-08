import { Camera, Cpu, Crosshair, Download, FolderOpen, Gauge, Palette, Play, RotateCcw, SlidersHorizontal, Square, Sun, type LucideIcon } from "lucide-react";
import { useEffect, useState } from "react";
import {
  activeMode,
  causticSettings,
  dofValues,
  QUALITY_PRESETS,
  qualityPasses,
  withCaustics,
  withDof,
  withPasses,
  effectivePasses,
  MAX_IMAGE_SIDE,
  MAX_PASSES,
  MIN_IMAGE_SIDE,
  MODE_NOTES,
  RESOLUTION_GROUPS,
  withEvolution,
  withMode,
} from "../settings";
import type { Bounces, DofPreset, CausticPreset, CausticReport, Comparison, Mode, Quality, Resolution, Settings } from "../types";
import { EnvironmentPanel } from "./EnvironmentPanel";
import { MaterialEditor } from "./MaterialEditor";
import { CausticStats } from "./CausticStats";
import { GlassPanel } from "./GlassPanel";
import { NumberField } from "./NumberField";
import { Segmented } from "./Segmented";
import { Select } from "./Select";
import { Slider } from "./Slider";
import { Toggle } from "./Toggle";

interface ControlPanelProps {
  loadingEnvironment: boolean;
  onImportEnvironment: () => void;
  scene: import("../types").SceneInfo | null;
  pickingFocus: boolean;
  onPickFocus: () => void;
  onResetCamera: () => void;
  settings: Settings;
  cores: number;
  rendering: boolean;
  canRender: boolean;
  canSave: boolean;
  report: CausticReport | null;
  onChange: (settings: Settings) => void;
  onOpen: () => void;
  onRender: () => void;
  autoRender: boolean;
  onAutoRenderChange: (value: boolean) => void;
  webgl: boolean;
  /** Why the image on screen does not match the settings, when it does not. */
  outdated: string | false;
  onCancel: () => void;
  onSave: () => void;
}

/** Most rebound limit the slider shows as a number; one step past it means no limit. */
const MAX_REBOUND_LIMIT = 50;

function bounceSummary(auto: Bounces): string {
  return auto.strength >= 1
    ? "Automatic: full path tracing, physically correct."
    : "Automatic: this scene has a 2008 light, which does not fall off with distance, so its bounces are weaker and fewer.";
}

function BounceControls({ bounces, auto, onChange }: { bounces: Bounces | null; auto: Bounces; onChange: (bounces: Bounces | null) => void }) {
  const current = bounces ?? auto;
  const edit = (change: Partial<Bounces>) => {
    const next = { ...current, ...change };
    const automatic = next.strength === auto.strength && next.max === auto.max && next.clamp === auto.clamp;
    onChange(automatic ? null : next);
  };
  return (
    <>
      <Select<"auto" | "custom">
        label="Bounce settings"
        value={bounces ? "custom" : "auto"}
        onChange={(value) => onChange(value === "custom" ? { ...auto } : null)}
      >
        <option value="auto">Automatic</option>
        <option value="custom">Custom</option>
      </Select>
      <Slider
        label="Bounce strength"
        value={current.strength}
        min={0}
        max={1.5}
        step={0.05}
        format={(value) => value.toFixed(2)}
        onChange={(strength) => edit({ strength })}
      />
      <Slider label="Bounces" value={current.max} min={0} max={64} step={1} onChange={(max) => edit({ max })} />
      <Slider
        label="Rebound limit"
        value={current.clamp === 0 ? MAX_REBOUND_LIMIT + 1 : current.clamp}
        min={1}
        max={MAX_REBOUND_LIMIT + 1}
        step={0.5}
        format={(value) => (value > MAX_REBOUND_LIMIT ? "None" : value.toFixed(1))}
        onChange={(value) => edit({ clamp: value > MAX_REBOUND_LIMIT ? 0 : value })}
      />
      <p className="note">
        {bounces ? "Strength 1 is physically correct." : bounceSummary(auto)} From the third bounce, Russian roulette ends
        dim paths early, so many bounces cost little. A lower rebound limit removes white dots but darkens bright bounces.
      </p>
    </>
  );
}

type Tab = "render" | "light" | "camera" | "image" | "materials" | "system";

const TABS: { id: Tab; label: string; icon: LucideIcon; enhancedOnly: boolean }[] = [
  { id: "render", label: "Render", icon: Gauge, enhancedOnly: false },
  { id: "light", label: "Light", icon: Sun, enhancedOnly: true },
  { id: "camera", label: "Camera", icon: Camera, enhancedOnly: true },
  { id: "image", label: "Image", icon: SlidersHorizontal, enhancedOnly: true },
  { id: "materials", label: "Materials", icon: Palette, enhancedOnly: true },
  { id: "system", label: "System", icon: Cpu, enhancedOnly: false },
];

const TAB_KEY = "far.controlTab";

function savedTab(): Tab {
  try {
    const value = localStorage.getItem(TAB_KEY);
    return TABS.some((tab) => tab.id === value) ? (value as Tab) : "render";
  } catch {
    return "render";
  }
}

export function ControlPanel(props: ControlPanelProps) {
  const { settings, cores, rendering, canRender, canSave, report, onChange, onOpen, onRender, onCancel, onSave } = props;
  const [chosenTab, setChosenTab] = useState<Tab>(savedTab);
  const update = (patch: Partial<Settings>) => onChange({ ...settings, ...patch });
  const mode = activeMode(settings);
  const enhanced = mode === "enhanced";
  const tab = !enhanced && TABS.find((t) => t.id === chosenTab)?.enhancedOnly ? "render" : chosenTab;
  const current = TABS.find((t) => t.id === tab)!;
  const dof = dofValues(settings);
  const caustic = causticSettings(settings);

  useEffect(() => {
    try {
      localStorage.setItem(TAB_KEY, chosenTab);
    } catch {
      // The tab is a convenience; without storage the panel opens on Render.
    }
  }, [chosenTab]);

  return (
    <GlassPanel className="panel" aria-label="Render options">
      <div className="panel-top">
        <button type="button" className="ghost" onClick={onOpen}>
          <FolderOpen size={15} aria-hidden="true" />
          Open scene…
        </button>

        <div className="actions">
          <button
            type="button"
            className={rendering ? "primary stop" : "primary"}
            disabled={!canRender}
            onClick={rendering ? onCancel : onRender}
          >
            {rendering ? <Square size={14} aria-hidden="true" /> : <Play size={14} aria-hidden="true" />}
            {rendering ? "Cancel" : "Render"}
          </button>
          <button type="button" className="icon" disabled={!canSave} aria-label="Save PNG" title="Save PNG" onClick={onSave}>
            <Download size={16} aria-hidden="true" />
          </button>
        </div>
        <Toggle label="Auto-render" checked={props.autoRender || !props.webgl} disabled={!props.webgl}
          onChange={props.onAutoRenderChange} />
        {props.outdated && <p className="note">{props.outdated}</p>}

        <div className="tabs" role="tablist" aria-label="Settings">
          {TABS.map(({ id, label, icon: Icon, enhancedOnly }) => {
            const disabled = enhancedOnly && !enhanced;
            return (
              <button
                key={id}
                type="button"
                role="tab"
                id={`tab-${id}`}
                aria-controls="tab-body"
                aria-selected={tab === id}
                aria-label={label}
                title={disabled ? `${label} · only in the Enhanced stage` : label}
                disabled={disabled}
                onClick={() => setChosenTab(id)}
              >
                <Icon size={16} aria-hidden="true" />
              </button>
            );
          })}
        </div>
      </div>

      <div className="tab-body" id="tab-body" role="tabpanel" aria-labelledby={`tab-${tab}`}>
        <h1 className="tab-title">{current.label}</h1>

        {tab === "render" && (
          <>
          <div className="group">
            <h2>Quality</h2>
            <Select<string>
              label="Quality"
              value={String(settings.quality)}
              disabled={!enhanced}
              onChange={(value) => update(value === "custom"
                ? { quality: "custom", customPasses: qualityPasses(settings) }
                : { quality: Number(value) as Quality })}
            >
              {QUALITY_PRESETS.map((preset) => (
                <option key={preset.value} value={preset.value}>{preset.label}</option>
              ))}
              <option value="custom">Custom</option>
            </Select>
            <label className="field-row">
              <span>Passes</span>
              <NumberField
                label="Passes"
                value={qualityPasses(settings)}
                min={1}
                max={MAX_PASSES}
                disabled={!enhanced}
                onChange={(passes) => update(withPasses(passes))}
              />
            </label>
            <Slider
              label="Ray depth"
              value={settings.depth}
              min={1}
              max={8}
              step={1}
              onChange={(depth) => update({ depth })}
            />
            <p className="note">Reflection and refraction depth. At 1, reflection and refraction bounces are disabled.</p>
          </div>

          <div className="group">
            <h2>Resolution</h2>
            <Select<Resolution>
              label="Resolution"
              value={settings.resolution}
              onChange={(resolution) => update({ resolution })}
            >
              {RESOLUTION_GROUPS.map((group) => (
                <optgroup key={group.label} label={group.label}>
                  {group.options.map((option) => (
                    <option key={option.value} value={option.value}>
                      {option.label}
                    </option>
                  ))}
                </optgroup>
              ))}
              <option value="custom">Custom…</option>
            </Select>
            {settings.resolution === "custom" && (
              <div className="size-fields">
                <NumberField
                  label="Width in pixels"
                  value={settings.customWidth}
                  min={MIN_IMAGE_SIDE}
                  max={MAX_IMAGE_SIDE}
                  onChange={(customWidth) => update({ customWidth })}
                />
                <span aria-hidden="true">×</span>
                <NumberField
                  label="Height in pixels"
                  value={settings.customHeight}
                  min={MIN_IMAGE_SIDE}
                  max={MAX_IMAGE_SIDE}
                  onChange={(customHeight) => update({ customHeight })}
                />
              </div>
            )}
          </div>

          </>
        )}

        {tab === "light" && (
          <>
            <div className="group">
              <Toggle
                label="Indirect light"
                checked={settings.indirect}
                onChange={(indirect) => update({ indirect })}
              />
              {settings.indirect && props.scene && (
                <BounceControls
                  bounces={settings.bounces}
                  auto={props.scene.bounces}
                  onChange={(bounces) => update({ bounces })}
                />
              )}
            </div>

            <EnvironmentPanel environment={settings.environment} loading={props.loadingEnvironment}
              disabled={!canRender} onImport={props.onImportEnvironment}
              onChange={(environment) => update({ environment })} />

          <div className="group">
            <Toggle
              label="Caustics"
              checked={settings.caustics}
              disabled={!enhanced}
              onChange={(caustics) => update({ caustics })}
            />
            {settings.caustics && enhanced && (
              <div className="stack">
                <Select<CausticPreset>
                  label="Caustic preset"
                  value={settings.causticPreset}
                  onChange={(causticPreset) => update(causticPreset === "custom"
                    ? { ...withCaustics(settings, {}), causticPreset }
                    : { causticPreset })}
                >
                  <option value="soft">Soft</option>
                  <option value="balanced">Balanced</option>
                  <option value="sharp">Sharp</option>
                  <option value="custom">Custom</option>
                </Select>
                <Slider
                  label="Photons"
                  value={caustic.photonMillions}
                  min={0.25}
                  max={16}
                  step={0.25}
                  format={(value) => `${value} million`}
                  onChange={(photonMillions) => update(withCaustics(settings, { photonMillions }))}
                />
                <Slider
                  label="Radius"
                  value={caustic.radius}
                  min={0.02}
                  max={0.4}
                  step={0.01}
                  format={(value) => value.toFixed(2)}
                  onChange={(radius) => update(withCaustics(settings, { radius }))}
                />
                <Slider
                  label="Strength"
                  value={caustic.strength}
                  min={0}
                  max={3}
                  step={0.05}
                  format={(value) => value.toFixed(2)}
                  onChange={(strength) => update(withCaustics(settings, { strength }))}
                />
                {report && <CausticStats report={report} />}
              </div>
            )}
          </div>

          </>
        )}

        {tab === "camera" && (
        <div className="group">
          <h2>Camera</h2>
          {enhanced && <p className="note">Drag the image to orbit. Shift-drag to pan, scroll to move closer.</p>}
          <Slider label="Zoom" value={settings.pose.zoom} min={0.25} max={Math.max(4, settings.pose.zoom)} step={0.05}
            disabled={!enhanced} format={(value) => `${value.toFixed(2)}×`}
            onChange={(zoom) => update({ pose: { ...settings.pose, zoom } })} />
          <button type="button" className="ghost" disabled={!enhanced || !canRender} onClick={props.onResetCamera}>
            <RotateCcw size={15} aria-hidden="true" />
            Reset camera
          </button>
          <button type="button" className="ghost" disabled={!enhanced || !canRender} aria-pressed={props.pickingFocus} onClick={props.onPickFocus}>
            <Crosshair size={15} aria-hidden="true" />
            {props.pickingFocus ? "Cancel focus picker" : "Pick focus in image"}
          </button>
          <Toggle label="Depth of field" checked={settings.depthOfField} disabled={!enhanced}
            onChange={(depthOfField) => update({ depthOfField })} />
          {settings.depthOfField && enhanced && (
            <div className="stack">
              <Select<DofPreset>
                label="Depth of field preset" value={settings.dofPreset}
                onChange={(dofPreset) => update(dofPreset === "custom" ? { ...withDof(settings, {}), dofPreset } : { dofPreset })}
              >
                <option value="subtle">Subtle</option>
                <option value="portrait">Portrait</option>
                <option value="strong">Strong</option>
                <option value="custom">Custom</option>
              </Select>
              <Slider label="Aperture" value={dof.aperture} min={0.01} max={Math.max(2, dof.aperture)} step={0.01}
                format={(value) => value.toFixed(2)} onChange={(aperture) => update(withDof(settings, { aperture }))} />
              <label className="field-row">
                <span>Minimum passes</span>
                <NumberField label="Depth of field minimum passes" value={dof.passes} min={1} max={MAX_PASSES}
                  onChange={(passes) => update(withDof(settings, { passes }))} />
              </label>
              <p className="note">Renders {effectivePasses(settings)} passes, the higher of Quality and this minimum.</p>
              <Slider label="Focus distance" value={settings.focusDistance} min={0.1} max={Math.max(50, settings.focusDistance)} step={0.1}
                format={(value) => value.toFixed(1)} onChange={(focusDistance) => update({ focusDistance })} />
              <p className="note">Larger apertures soften the foreground and background. Distances are in scene units, measured from the camera.</p>
            </div>
          )}
        </div>

        )}

        {tab === "image" && (
        <div className="group">
          <Slider
            label="Exposure"
            value={settings.exposure}
            min={0.5}
            max={2}
            step={0.05}
            disabled={!enhanced}
            format={(value) => value.toFixed(2)}
            onChange={(exposure) => update({ exposure })}
          />
          <Toggle
            label="Denoise"
            checked={settings.denoise}
            disabled={!enhanced}
            onChange={(denoise) => update({ denoise })}
          />
          {enhanced && !settings.denoise && settings.environment?.enabled && settings.environment.intensity > 0 && (
            <p className="note">HDR lighting renders grainier. Turn on Denoise for a cleaner image.</p>
          )}
          <Toggle
            label="Bloom"
            checked={settings.bloom}
            disabled={!enhanced}
            onChange={(bloom) => update({ bloom })}
          />
        </div>

        )}

        {tab === "materials" && (props.scene ? (
          <MaterialEditor key={props.scene.path} materials={props.scene.materials} overrides={settings.materialOverrides}
            onChange={(materialOverrides) => update({ materialOverrides })} />
        ) : (
          <p className="note">Open a scene to edit its materials.</p>
        ))}

        {tab === "system" && (
          <div className="group">
            <Slider
              label="Threads"
              value={settings.threads}
              min={1}
              max={cores}
              step={1}
              onChange={(threads) => update({ threads })}
            />
            <Toggle label="KD-Tree" checked={settings.useKd} onChange={(useKd) => update({ useKd })} />
            <p className="note">The KD-Tree skips objects a ray cannot hit. Turn it off only to compare speed.</p>
          </div>
        )}
      </div>

      <div className="group evolution">
        <Toggle
          label="Show evolution"
          checked={settings.evolution}
          onChange={(evolution) => onChange(withEvolution(settings, evolution))}
        />
        {settings.evolution && (
          <div className="stack">
            <Segmented<Mode>
              label="Stage"
              value={settings.mode}
              options={[
                { value: "classic", label: "2008" },
                { value: "fixed", label: "Fixed" },
                { value: "enhanced", label: "Enhanced" },
              ]}
              onChange={(next) => onChange(withMode(settings, next))}
            />
            <p className="note">{MODE_NOTES[settings.mode]}</p>
            <h2>Compare with</h2>
            <Segmented<Comparison>
              label="Compare with"
              value={settings.comparison}
              disabled={settings.mode === "classic"}
              options={[
                { value: "off", label: "Off" },
                { value: "classic", label: "2008" },
                { value: "fixed", label: "Fixed", disabled: settings.mode !== "enhanced" },
              ]}
              onChange={(comparison) => update({ comparison })}
            />
          </div>
        )}
      </div>
    </GlassPanel>
  );
}
