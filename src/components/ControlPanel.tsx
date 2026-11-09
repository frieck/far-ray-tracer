import {
  Camera, Cpu, Crosshair, Download, FolderOpen, Gauge, History, Palette, Play, RotateCcw, Search, SlidersHorizontal, Square,
  Sun, X, type LucideIcon,
} from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
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
  DEFAULT_SKY,
  DEFAULT_SUN_TIME,
} from "../settings";
import type {
  Bounces, DofPreset, CausticPreset, CausticReport, Comparison, Mode, Quality, Resolution, Settings, SkyConfig, SunTime, ToneMapping,
} from "../types";
import { MaterialEditor } from "./MaterialEditor";
import { CausticStats } from "./CausticStats";
import { GlassPanel } from "./GlassPanel";
import { NumberField } from "./NumberField";
import { Segmented } from "./Segmented";
import { ButtonRow, CheckBox, CheckRow, Choice, ChoiceRow, Hint, Row, ScrubRow, SearchContext, Section } from "./props";

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
  /** The material the editor shows; null picks the first. */
  selectedMaterial: number | null;
  onSelectMaterial: (id: number) => void;
  onExpandMaterial: () => void;
}

/** Most rebound limit the field shows as a number; one step past it means no limit. */
const MAX_REBOUND_LIMIT = 50;

type Tab = "render" | "light" | "camera" | "image" | "materials" | "system" | "evolution";

const TABS: { id: Tab; label: string; icon: LucideIcon; enhancedOnly: boolean }[] = [
  { id: "render", label: "Render", icon: Gauge, enhancedOnly: false },
  { id: "light", label: "Light", icon: Sun, enhancedOnly: true },
  { id: "camera", label: "Camera", icon: Camera, enhancedOnly: true },
  { id: "image", label: "Image", icon: SlidersHorizontal, enhancedOnly: true },
  { id: "materials", label: "Materials", icon: Palette, enhancedOnly: true },
  { id: "system", label: "System", icon: Cpu, enhancedOnly: false },
  { id: "evolution", label: "Evolution", icon: History, enhancedOnly: false },
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
  const { settings, rendering, canRender, canSave, onChange, onOpen, onRender, onCancel, onSave } = props;
  const [chosenTab, setChosenTab] = useState<Tab>(savedTab);
  const [query, setQuery] = useState("");
  const update = (patch: Partial<Settings>) => onChange({ ...settings, ...patch });
  const enhanced = activeMode(settings) === "enhanced";
  const tab = !enhanced && TABS.find((t) => t.id === chosenTab)?.enhancedOnly ? "render" : chosenTab;
  const searching = query.trim() !== "";
  const context: TabContext = { props, update, enhanced };

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
        <CheckBox label="Auto-render" checked={props.autoRender || !props.webgl} disabled={!props.webgl}
          onChange={props.onAutoRenderChange}>
          <span className="prop-check-text">Render after each change</span>
        </CheckBox>
        {props.outdated && <p className="prop-hint">{props.outdated}</p>}
      </div>

      <div className="panel-main">
        <nav className="rail" role="tablist" aria-label="Settings" aria-orientation="vertical">
          {TABS.map(({ id, label, icon: Icon, enhancedOnly }) => {
            const disabled = enhancedOnly && !enhanced;
            return (
              <button
                key={id}
                type="button"
                role="tab"
                id={`tab-${id}`}
                aria-controls="tab-body"
                aria-selected={!searching && tab === id}
                aria-label={label}
                title={disabled ? `${label} · only in the Enhanced stage` : label}
                disabled={disabled}
                onClick={() => {
                  setChosenTab(id);
                  setQuery("");
                }}
              >
                <Icon size={16} aria-hidden="true" />
              </button>
            );
          })}
        </nav>

        <div className="tab-body" id="tab-body" role="tabpanel" aria-labelledby={`tab-${tab}`} data-searching={searching || undefined}>
          <div className="prop-search">
            <Search size={13} aria-hidden="true" />
            <input type="search" placeholder="Search settings" aria-label="Search settings" value={query}
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={(event) => { if (event.key === "Escape") setQuery(""); }} />
            {searching && (
              <button type="button" aria-label="Clear search" onClick={() => setQuery("")}><X size={12} aria-hidden="true" /></button>
            )}
          </div>
          <SearchContext.Provider value={query}>
            {searching ? (
              TABS.filter((t) => t.id !== "materials" && !(t.enhancedOnly && !enhanced)).map((t) => (
                <div key={t.id} className="search-group">
                  <h2 className="search-group-title">{t.label}</h2>
                  {tabContent(t.id, context)}
                </div>
              ))
            ) : (
              <>
                <h1 className="tab-title">{TABS.find((t) => t.id === tab)!.label}</h1>
                {tabContent(tab, context)}
              </>
            )}
            {searching && <p className="prop-hint search-empty">No settings match “{query.trim()}”.</p>}
          </SearchContext.Provider>
        </div>
      </div>
    </GlassPanel>
  );
}

interface TabContext {
  props: ControlPanelProps;
  update: (patch: Partial<Settings>) => void;
  enhanced: boolean;
}

function tabContent(tab: Tab, context: TabContext): ReactNode {
  switch (tab) {
    case "render": return <RenderTab {...context} />;
    case "light": return <LightTab {...context} />;
    case "camera": return <CameraTab {...context} />;
    case "image": return <ImageTab {...context} />;
    case "materials": return <MaterialsTab {...context} />;
    case "system": return <SystemTab {...context} />;
    case "evolution": return <EvolutionTab {...context} />;
  }
}

function RenderTab({ props, update, enhanced }: TabContext) {
  const { settings } = props;
  return (
    <>
      <Section id="sampling" title="Sampling" extra={
        <Choice<string> small label="Quality" value={String(settings.quality)} disabled={!enhanced}
          onChange={(value) => update(value === "custom"
            ? { quality: "custom", customPasses: qualityPasses(settings) }
            : { quality: Number(value) as Quality })}>
          {QUALITY_PRESETS.map((preset) => <option key={preset.value} value={preset.value}>{preset.label}</option>)}
          <option value="custom">Custom</option>
        </Choice>
      }>
        <ScrubRow label="Passes" keywords="samples quality" value={qualityPasses(settings)} min={1} max={MAX_PASSES} hard={[1, 65536]} step={1} log
          disabled={!enhanced} hint="Samples per pixel. More passes, less noise."
          onChange={(passes) => update(withPasses(passes))} />
        <ScrubRow label="Ray depth" keywords="reflection refraction" value={settings.depth} min={1} max={8} hard={[1, 64]} step={1}
          hint="Reflection and refraction depth. At 1, reflection and refraction bounces are disabled."
          onChange={(depth) => update({ depth })} />
      </Section>
      <Section id="resolution" title="Resolution">
        <ChoiceRow<Resolution> label="Size" value={settings.resolution} onChange={(resolution) => update({ resolution })}>
          {RESOLUTION_GROUPS.map((group) => (
            <optgroup key={group.label} label={group.label}>
              {group.options.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
            </optgroup>
          ))}
          <option value="custom">Custom…</option>
        </ChoiceRow>
        {settings.resolution === "custom" && (
          <Row label="Pixels" keywords="width height">
            <div className="size-fields">
              <NumberField label="Width in pixels" value={settings.customWidth} min={MIN_IMAGE_SIDE} max={MAX_IMAGE_SIDE}
                onChange={(customWidth) => update({ customWidth })} />
              <span aria-hidden="true">×</span>
              <NumberField label="Height in pixels" value={settings.customHeight} min={MIN_IMAGE_SIDE} max={MAX_IMAGE_SIDE}
                onChange={(customHeight) => update({ customHeight })} />
            </div>
          </Row>
        )}
      </Section>
    </>
  );
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** A day of the year as a date, in a year without February 29. */
function dayLabel(day: number): string {
  const date = new Date(Date.UTC(2025, 0, Math.round(day)));
  return `${MONTHS[date.getUTCMonth()]} ${date.getUTCDate()}`;
}

function bounceSummary(auto: Bounces): string {
  return auto.strength >= 1
    ? "Automatic: full path tracing, physically correct."
    : "Automatic: this scene has a 2008 light, which does not fall off with distance, so its bounces are weaker and fewer.";
}

function LightTab({ props, update, enhanced }: TabContext) {
  const { settings, scene, report } = props;
  const auto = scene?.bounces;
  const bounces = settings.bounces;
  const current = bounces ?? auto;
  const editBounces = (change: Partial<Bounces>) => {
    if (!auto || !current) return;
    const next = { ...current, ...change };
    const automatic = next.strength === auto.strength && next.max === auto.max && next.clamp === auto.clamp;
    update({ bounces: automatic ? null : next });
  };
  const environment = settings.environment;
  const setEnvironment = (change: Partial<NonNullable<typeof environment>>) =>
    environment && update({ environment: { ...environment, ...change } });
  const sky = environment?.sky ?? null;
  const setSky = (change: Partial<SkyConfig>) => sky && setEnvironment({ sky: { ...sky, ...change } });
  const setTime = (change: Partial<SunTime>) => sky?.time && setSky({ time: { ...sky.time, ...change } });
  const caustic = causticSettings(settings);
  return (
    <>
      <Section id="indirect" title="Indirect light"
        enabled={{ checked: settings.indirect, onChange: (indirect) => update({ indirect }) }}
        extra={auto && (
          <Choice<"auto" | "custom"> small label="Bounce settings" value={bounces ? "custom" : "auto"}
            onChange={(value) => update({ bounces: value === "custom" ? { ...auto } : null })}>
            <option value="auto">Automatic</option>
            <option value="custom">Custom</option>
          </Choice>
        )}
        advanced={current && (
          <ScrubRow label="Rebound limit" keywords="clamp fireflies"
            value={current.clamp === 0 ? MAX_REBOUND_LIMIT + 1 : current.clamp} min={1} max={MAX_REBOUND_LIMIT + 1} step={0.5} strict
            format={(value) => (value > MAX_REBOUND_LIMIT ? "None" : value.toFixed(1))}
            hint="Most light one rebound may bring back. Lower removes white dots but darkens bright bounces."
            onChange={(value) => editBounces({ clamp: value > MAX_REBOUND_LIMIT ? 0 : value })} />
        )}>
        {current ? (
          <>
            <ScrubRow label="Strength" keywords="bounce" value={current.strength} min={0} max={1.5} hard={[0, 10]} step={0.05}
              hint="1 is physically correct." onChange={(strength) => editBounces({ strength })} />
            <ScrubRow label="Bounces" value={current.max} min={0} max={64} hard={[0, 1024]} step={1}
              hint="From the third bounce, Russian roulette ends dim paths early, so many bounces cost little."
              onChange={(max) => editBounces({ max })} />
            {auto && !bounces && <Hint>{bounceSummary(auto)}</Hint>}
          </>
        ) : (
          <Hint>Open a scene to set its bounces.</Hint>
        )}
      </Section>

      <Section id="environment" title="Environment"
        enabled={environment ? { checked: environment.enabled, onChange: (enabled) => setEnvironment({ enabled }) } : undefined}
        extra={
          <Choice<"none" | "hdr" | "sky"> small label="Environment source" value={!environment ? "none" : sky ? "sky" : "hdr"}
            disabled={props.loadingEnvironment}
            onChange={(source) => {
              if (source === "none") update({ environment: null });
              else if (source === "sky") update({ environment: {
                path: "", sky: { ...DEFAULT_SKY }, intensity: 1, rotation: 0, visible: true, enabled: true } });
              else props.onImportEnvironment();
            }}>
            <option value="none">None</option>
            <option value="hdr">{props.loadingEnvironment ? "Loading…" : environment && !sky ? "HDR image" : "HDR image…"}</option>
            <option value="sky">Physical sky</option>
          </Choice>
        }
        advanced={environment && (
          <>
            {sky && (
              <>
                <ScrubRow label="Sun size" keywords="sun soft shadows" value={sky.sun_size} min={0.2} max={5} hard={[0.05, 20]}
                  step={0.01} format={(value) => `${value.toFixed(2)}°`} hint="The sun's apparent diameter; 0.53° is the real sun. Larger softens shadows."
                  onChange={(sun_size) => setSky({ sun_size })} />
                <ScrubRow label="Sun strength" keywords="sun" value={sky.sun_strength} min={0} max={3} hard={[0, 100]} step={0.05}
                  hint="Multiplies the sun's light and leaves the sky as it is." onChange={(sun_strength) => setSky({ sun_strength })} />
              </>
            )}
            {!sky && (
              <ButtonRow label="Image">
                <button type="button" className="prop-mini" onClick={props.onImportEnvironment}>Replace…</button>
              </ButtonRow>
            )}
          </>
        )}>
        {environment ? (
          <>
            {sky ? (
              <>
                <ChoiceRow<"angles" | "time"> label="Sun from" keywords="sun sky" value={sky.time ? "time" : "angles"}
                  onChange={(mode) => setSky({ time: mode === "time" ? { ...DEFAULT_SUN_TIME } : null })}>
                  <option value="angles">Angles</option>
                  <option value="time">Time of day</option>
                </ChoiceRow>
                {sky.time ? (
                  <>
                    <ScrubRow label="Hour" keywords="time sun" value={sky.time.hour} min={5} max={20} hard={[0, 24]} step={0.05}
                      format={(value) => `${Math.floor(value)}:${String(Math.round((value % 1) * 60) % 60).padStart(2, "0")}`}
                      hint="Local solar time: 12:00 is noon." onChange={(hour) => setTime({ hour })} />
                    <ScrubRow label="Latitude" keywords="sun place" value={sky.time.latitude} min={-60} max={60} hard={[-90, 90]} step={0.5}
                      format={(value) => `${Math.abs(value).toFixed(1)}° ${value < 0 ? "S" : "N"}`}
                      hint="North of the equator is positive. North is +Z in the scene." onChange={(latitude) => setTime({ latitude })} />
                    <ScrubRow label="Day of year" keywords="season date" value={sky.time.day} min={1} max={365} step={1} strict
                      format={dayLabel} onChange={(day) => setTime({ day })} />
                  </>
                ) : (
                  <>
                    <ScrubRow label="Sun elevation" keywords="sun height" value={sky.elevation} min={-10} max={90} step={0.5} strict
                      format={(value) => `${value.toFixed(1)}°`} hint="Height above the horizon. Below 0 the sun has set."
                      onChange={(elevation) => setSky({ elevation })} />
                    <ScrubRow label="Sun direction" keywords="sun azimuth" value={sky.azimuth} min={0} max={360} hard={[-36000, 36000]} step={1}
                      format={(value) => `${value.toFixed(0)}°`} hint="0° toward +Z, 90° toward +X."
                      onChange={(azimuth) => setSky({ azimuth: ((azimuth % 360) + 360) % 360 })} />
                  </>
                )}
                <ScrubRow label="Haze" keywords="turbidity sky" value={sky.turbidity} min={2} max={10} hard={[1.7, 12]} step={0.1}
                  hint="Turbidity: 2 is a very clear sky, 3 a clear day, 6 hazy, 10 smog." onChange={(turbidity) => setSky({ turbidity })} />
              </>
            ) : (
              <Row label="File" keywords="hdr">
                <span className="prop-value-text" title={environment.path}>{environment.path.split(/[\\/]/).pop()}</span>
              </Row>
            )}
            <ScrubRow label="Intensity" keywords="hdr sky" value={environment.intensity} min={0} max={10} step={0.05}
              disabled={!environment.enabled} onChange={(intensity) => setEnvironment({ intensity })} />
            {!sky && (
              <ScrubRow label="Rotation" keywords="hdr" value={((environment.rotation % 360) + 360) % 360} min={0} max={360} hard={[-36000, 36000]} step={1}
                disabled={!environment.enabled} format={(value) => `${value}°`} onChange={(rotation) => setEnvironment({ rotation })} />
            )}
            <CheckRow label="Background" hint="Hiding the background keeps lighting and reflections."
              checked={environment.visible} disabled={!environment.enabled} onChange={(visible) => setEnvironment({ visible })} />
          </>
        ) : (
          <Hint>Light the scene from all around: an HDR image of a real place, or a physical sky with the sun.</Hint>
        )}
      </Section>

      <Section id="caustics" title="Caustics"
        enabled={{ checked: settings.caustics, onChange: (caustics) => update({ caustics }), disabled: !enhanced }}
        extra={
          <Choice<CausticPreset> small label="Caustic preset" value={settings.causticPreset}
            onChange={(causticPreset) => update(causticPreset === "custom"
              ? { ...withCaustics(settings, {}), causticPreset }
              : { causticPreset })}>
            <option value="soft">Soft</option>
            <option value="balanced">Balanced</option>
            <option value="sharp">Sharp</option>
            <option value="custom">Custom</option>
          </Choice>
        }
        advanced={report && <CausticStats report={report} />}>
        <ScrubRow label="Photons" keywords="caustics" value={caustic.photonMillions} min={0.25} max={16} hard={[0.01, 200]} step={0.01}
          format={(value) => `${value} M`} hint="Millions of photons traced through glass and mirrors."
          onChange={(photonMillions) => update(withCaustics(settings, { photonMillions }))} />
        <ScrubRow label="Radius" keywords="caustics" value={caustic.radius} min={0.02} max={0.4} hard={[0.001, 10]} step={0.001}
          hint="Smaller is sharper but noisier." onChange={(radius) => update(withCaustics(settings, { radius }))} />
        <ScrubRow label="Strength" keywords="caustics" value={caustic.strength} min={0} max={3} step={0.05}
          onChange={(strength) => update(withCaustics(settings, { strength }))} />
      </Section>
    </>
  );
}

function CameraTab({ props, update, enhanced }: TabContext) {
  const { settings } = props;
  const dof = dofValues(settings);
  return (
    <>
      <Section id="lens" title="Lens">
        <ScrubRow label="Zoom" value={settings.pose.zoom} min={0.25} max={4} hard={[0.01, 100]} step={0.01} disabled={!enhanced}
          format={(value) => `${value.toFixed(2)}×`} hint="Drag the image to orbit. Shift-drag to pan, scroll to move closer."
          onChange={(zoom) => update({ pose: { ...settings.pose, zoom } })} />
        <ButtonRow label="View">
          <button type="button" className="prop-mini" disabled={!enhanced || !props.canRender} onClick={props.onResetCamera}
            title="Back to the scene's camera">
            <RotateCcw size={12} aria-hidden="true" />
            Reset
          </button>
          <button type="button" className="prop-mini" disabled={!enhanced || !props.canRender} aria-pressed={props.pickingFocus}
            onClick={props.onPickFocus} title="Click a surface in the image to focus on it">
            <Crosshair size={12} aria-hidden="true" />
            {props.pickingFocus ? "Cancel" : "Focus"}
          </button>
        </ButtonRow>
      </Section>
      <Section id="dof" title="Depth of field"
        enabled={{ checked: settings.depthOfField, onChange: (depthOfField) => update({ depthOfField }), disabled: !enhanced }}
        extra={
          <Choice<DofPreset> small label="Depth of field preset" value={settings.dofPreset}
            onChange={(dofPreset) => update(dofPreset === "custom" ? { ...withDof(settings, {}), dofPreset } : { dofPreset })}>
            <option value="subtle">Subtle</option>
            <option value="portrait">Portrait</option>
            <option value="strong">Strong</option>
            <option value="custom">Custom</option>
          </Choice>
        }
        advanced={
          <>
            {settings.blades > 0 && (
              <ScrubRow label="Blade rotation" keywords="bokeh" value={settings.bladeRotation} min={0} max={360 / settings.blades} hard={[-360, 360]} step={1}
                format={(value) => `${value.toFixed(0)}°`} onChange={(bladeRotation) => update({ bladeRotation })} />
            )}
            <ScrubRow label="Min. passes" keywords="depth of field passes" value={dof.passes} min={1} max={MAX_PASSES} hard={[1, 65536]} step={1} log
              hint={`Renders ${effectivePasses(settings)} passes, the higher of Quality and this minimum.`}
              onChange={(passes) => update(withDof(settings, { passes }))} />
          </>
        }>
        <ScrubRow label="Focus distance" keywords="focus" value={settings.focusDistance} min={0.1} max={50} hard={[0.01, 1e6]} step={0.01}
          hint="Scene units, measured from the camera." onChange={(focusDistance) => update({ focusDistance })} />
        <ScrubRow label="Aperture" keywords="blur" value={dof.aperture} min={0.01} max={2} hard={[0.001, 1000]} step={0.001}
          hint="Lens diameter in scene units. Larger softens the foreground and background more."
          onChange={(aperture) => update(withDof(settings, { aperture }))} />
        <ChoiceRow<string> label="Shape" hint="Outline of out-of-focus highlights (bokeh)." value={String(settings.blades)}
          onChange={(blades) => update({ blades: Number(blades) })}>
          <option value="0">Round</option>
          <option value="5">5 blades</option>
          <option value="6">6 blades</option>
          <option value="8">8 blades</option>
          {![0, 5, 6, 8].includes(settings.blades) && <option value={settings.blades}>{settings.blades} blades</option>}
        </ChoiceRow>
      </Section>
    </>
  );
}

function ImageTab({ props, update, enhanced }: TabContext) {
  const { settings } = props;
  return (
    <>
      <Section id="color" title="Color">
        <ScrubRow label="Exposure" keywords="brightness" value={settings.exposure} min={0.5} max={2} hard={[0.01, 1000]} step={0.01}
          disabled={!enhanced} onChange={(exposure) => update({ exposure })} />
        <ChoiceRow<ToneMapping> label="Tone mapping" keywords="agx aces film" value={settings.toneMapping} disabled={!enhanced}
          hint="Film curves hold bright light and turn it toward white, as a camera does."
          onChange={(toneMapping) => update({ toneMapping })}>
          <option value="standard">Standard</option>
          <option value="agx">AgX</option>
          <option value="agx_punchy">AgX Punchy</option>
          <option value="aces">ACES</option>
        </ChoiceRow>
      </Section>
      <Section id="post" title="Post-processing">
        <CheckRow label="Denoise" hint="Removes noise with Open Image Denoise, or the built-in filter."
          checked={settings.denoise} disabled={!enhanced} onChange={(denoise) => update({ denoise })} />
        <CheckRow label="Bloom" hint="Spreads very bright light into a soft halo."
          checked={settings.bloom} disabled={!enhanced} onChange={(bloom) => update({ bloom })} />
        {enhanced && !settings.denoise && settings.environment?.enabled && settings.environment.intensity > 0 && (
          <Hint tone="warning">HDR lighting renders grainier. Turn on Denoise for a cleaner image.</Hint>
        )}
      </Section>
    </>
  );
}

function MaterialsTab({ props, update }: TabContext) {
  const { settings, scene } = props;
  if (!scene) return <Hint>Open a scene to edit its materials.</Hint>;
  return (
    <MaterialEditor key={scene.path} materials={scene.materials} overrides={settings.materialOverrides}
      scenePath={scene.path} selected={props.selectedMaterial} onSelect={props.onSelectMaterial}
      onExpand={props.onExpandMaterial} onChange={(materialOverrides) => update({ materialOverrides })} />
  );
}

function SystemTab({ props, update }: TabContext) {
  const { settings, cores } = props;
  return (
    <Section id="performance" title="Performance">
      <ScrubRow label="Threads" keywords="cores cpu" value={settings.threads} min={1} max={cores} step={1} strict
        onChange={(threads) => update({ threads })} />
      <CheckRow label="KD-Tree" hint="Skips objects a ray cannot hit. Turn it off only to compare speed."
        checked={settings.useKd} onChange={(useKd) => update({ useKd })} />
    </Section>
  );
}

function EvolutionTab({ props }: TabContext) {
  const { settings, onChange } = props;
  return (
    <Section id="evolution" title="Show evolution"
      enabled={{ checked: settings.evolution, onChange: (evolution) => onChange(withEvolution(settings, evolution)) }}>
      <Row label="Stage" keywords="2008 classic fixed enhanced">
        <Segmented<Mode>
          label="Stage"
          value={settings.mode}
          options={[
            { value: "classic", label: "2008" },
            { value: "fixed", label: "Fixed" },
            { value: "enhanced", label: "Enh." },
          ]}
          onChange={(next) => onChange(withMode(settings, next))}
        />
      </Row>
      <Row label="Compare with" keywords="comparison">
        <Segmented<Comparison>
          label="Compare with"
          value={settings.comparison}
          disabled={settings.mode === "classic"}
          options={[
            { value: "off", label: "Off" },
            { value: "classic", label: "2008" },
            { value: "fixed", label: "Fixed", disabled: settings.mode !== "enhanced" },
          ]}
          onChange={(comparison) => props.onChange({ ...settings, comparison })}
        />
      </Row>
      <Hint>{MODE_NOTES[settings.mode]}</Hint>
    </Section>
  );
}
