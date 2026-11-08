import { FolderOpen } from "lucide-react";
import type { EnvironmentConfig } from "../types";
import { Slider } from "./Slider";
import { Toggle } from "./Toggle";

export function EnvironmentPanel({ environment, loading, disabled, onImport, onChange }: {
  environment: EnvironmentConfig | null;
  loading: boolean;
  disabled: boolean;
  onImport: () => void;
  onChange: (environment: EnvironmentConfig | null) => void;
}) {
  return (
    <div className="group">
      <h2>Environment</h2>
      <button type="button" className="ghost" disabled={disabled || loading} onClick={onImport}>
        <FolderOpen size={15} aria-hidden="true" />
        {loading ? "Loading HDR…" : environment ? "Replace HDR…" : "Import HDR…"}
      </button>
      {environment && <>
        <p className="note" title={environment.path} style={{ overflowWrap: "anywhere" }}>
          {environment.path.split(/[\\/]/).pop()}
        </p>
        <Toggle label="Environment lighting" checked={environment.enabled}
          onChange={(enabled) => onChange({ ...environment, enabled })} />
        <Slider label="Intensity" value={environment.intensity} min={0} max={Math.max(10, environment.intensity)} step={0.05}
          disabled={!environment.enabled} format={(value) => value.toFixed(2)}
          onChange={(intensity) => onChange({ ...environment, intensity })} />
        <Slider label="Rotation" value={((environment.rotation % 360) + 360) % 360} min={0} max={360} step={1}
          disabled={!environment.enabled} format={(value) => `${value}°`}
          onChange={(rotation) => onChange({ ...environment, rotation })} />
        <Toggle label="Show background" checked={environment.visible} disabled={!environment.enabled}
          onChange={(visible) => onChange({ ...environment, visible })} />
        <p className="note">Hiding the background keeps lighting and reflections. Changes apply to this session.</p>
        <button type="button" className="ghost" onClick={() => onChange(null)}>Remove environment</button>
      </>}
    </div>
  );
}
