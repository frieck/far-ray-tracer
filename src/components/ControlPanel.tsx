import { ChevronDown, Download, FolderOpen, Play, Square } from "lucide-react";
import { useState } from "react";
import {
  activeMode,
  MAX_IMAGE_SIDE,
  MAX_PASSES,
  MIN_IMAGE_SIDE,
  MODE_NOTES,
  RESOLUTION_GROUPS,
  withEvolution,
  withMode,
} from "../settings";
import type { CausticPreset, CausticReport, Comparison, Mode, Quality, Resolution, Settings } from "../types";
import { CausticStats } from "./CausticStats";
import { GlassPanel } from "./GlassPanel";
import { NumberField } from "./NumberField";
import { Segmented } from "./Segmented";
import { Select } from "./Select";
import { Slider } from "./Slider";
import { Toggle } from "./Toggle";

interface ControlPanelProps {
  settings: Settings;
  cores: number;
  rendering: boolean;
  canRender: boolean;
  canSave: boolean;
  report: CausticReport | null;
  onChange: (settings: Settings) => void;
  onOpen: () => void;
  onRender: () => void;
  onCancel: () => void;
  onSave: () => void;
}

export function ControlPanel(props: ControlPanelProps) {
  const { settings, cores, rendering, canRender, canSave, report, onChange, onOpen, onRender, onCancel, onSave } = props;
  const [advanced, setAdvanced] = useState(false);
  const update = (patch: Partial<Settings>) => onChange({ ...settings, ...patch });
  const mode = activeMode(settings);
  const enhanced = mode === "enhanced";

  return (
    <GlassPanel className="panel" aria-label="Render options">
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

      <div className="group">
        <h2>Quality</h2>
        <Segmented<Quality>
          label="Quality"
          value={settings.quality}
          disabled={!enhanced}
          options={[
            { value: 16, label: "Draft" },
            { value: 64, label: "Good" },
            { value: 256, label: "High" },
            { value: "custom", label: "Custom" },
          ]}
          onChange={(quality) => update({ quality })}
        />
        {settings.quality === "custom" && (
          <label className="field-row">
            <span>Passes</span>
            <NumberField
              label="Passes"
              value={settings.customPasses}
              min={1}
              max={MAX_PASSES}
              disabled={!enhanced}
              onChange={(customPasses) => update({ customPasses })}
            />
          </label>
        )}
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
      </div>

      <div className="group">
        <Toggle
          label="Caustics"
          checked={settings.caustics}
          disabled={!enhanced}
          onChange={(caustics) => update({ caustics })}
        />
        {settings.caustics && enhanced && (
          <div className="stack">
            <Segmented<CausticPreset>
              label="Caustic preset"
              value={settings.causticPreset}
              options={[
                { value: "soft", label: "Soft" },
                { value: "balanced", label: "Balanced" },
                { value: "sharp", label: "Sharp" },
                { value: "custom", label: "Custom" },
              ]}
              onChange={(causticPreset) => update({ causticPreset })}
            />
            {settings.causticPreset === "custom" && (
              <>
                <Slider
                  label="Photons"
                  value={settings.photonMillions}
                  min={0.25}
                  max={16}
                  step={0.25}
                  format={(value) => `${value} million`}
                  onChange={(photonMillions) => update({ photonMillions })}
                />
                <Slider
                  label="Radius"
                  value={settings.causticRadius}
                  min={0.02}
                  max={0.4}
                  step={0.01}
                  format={(value) => value.toFixed(2)}
                  onChange={(causticRadius) => update({ causticRadius })}
                />
                <Slider
                  label="Strength"
                  value={settings.causticStrength}
                  min={0}
                  max={3}
                  step={0.05}
                  format={(value) => value.toFixed(2)}
                  onChange={(causticStrength) => update({ causticStrength })}
                />
              </>
            )}
            <Toggle
              label="Caustics only"
              checked={settings.causticsOnly}
              onChange={(causticsOnly) => update({ causticsOnly })}
            />
            {report && <CausticStats report={report} />}
          </div>
        )}
      </div>

      <div className="group">
        <button
          type="button"
          className="disclosure"
          aria-expanded={advanced}
          onClick={() => setAdvanced(!advanced)}
        >
          Advanced
          <ChevronDown size={14} aria-hidden="true" />
        </button>
        {advanced && (
          <div className="stack">
            <Slider
              label="Ray depth"
              value={settings.depth}
              min={1}
              max={8}
              step={1}
              onChange={(depth) => update({ depth })}
            />
            <Slider
              label="Threads"
              value={settings.threads}
              min={1}
              max={cores}
              step={1}
              onChange={(threads) => update({ threads })}
            />
            <Toggle
              label="Indirect light"
              checked={settings.indirect}
              disabled={!enhanced}
              onChange={(indirect) => update({ indirect })}
            />
            <Toggle label="KD-Tree" checked={settings.useKd} onChange={(useKd) => update({ useKd })} />
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
