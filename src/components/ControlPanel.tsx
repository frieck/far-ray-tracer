import {
  Camera, Cpu, Crosshair, Download, FolderOpen, Gauge, History, Move3d, Palette, Play, Repeat, RotateCcw, Save, Search, SlidersHorizontal, Square,
  Sun, X, type LucideIcon,
} from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { EnvironmentPreview } from "./EnvironmentPreview";
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
  cameraOf,
  exposureOf,
  ISO_RANGE,
  SHUTTER_RANGE,
  shutterLabel,
  withExposureMode,
  focalLength,
  markedIso,
  markedShutter,
  SENSORS,
  zoomFor,
  CAMERA_PRESETS,
  cameraPresetOf,
  withCameraPreset,
  withEntryEdit,
  fogOf,
  LOOKS,
  lutLabel,
  entryEdits,
} from "../settings";
import type {
  Bounces, DofPreset, CausticPreset, CausticReport, Comparison, Device, ExposureMode, Mode, Quality, Resolution, Settings, SkyConfig, SunTime, ToneMapping,
} from "../types";
import { MaterialEditor } from "./MaterialEditor";
import { CausticStats } from "./CausticStats";
import { GlassPanel } from "./GlassPanel";
import { NumberField } from "./NumberField";
import { Segmented } from "./Segmented";
import { ButtonRow, CheckRow, Choice, ChoiceRow, ColorRow, Hint, Row, ScrubRow, SearchContext, Section, VectorRow } from "./props";
import { fieldsFor, resizedEdge, valueOf, type Field, type Vec3 } from "../objectFields";
import { planePoints, planeState, type PlaneState } from "../transforms";
import { LightColorRows, LightsPanel } from "./LightControls";
import { sceneEntries } from "../sceneEntries";

interface ControlPanelProps {
  loadingEnvironment: boolean;
  onImportEnvironment: () => void;
  /** Picks a .cube file for the LUT. */
  onChooseLut: () => void;
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
  onSaveScene: () => void;
  /** The scene has edits not yet saved. */
  unsaved: boolean;
  /** The material the editor shows; null picks the first. */
  selectedMaterial: number | null;
  onSelectMaterial: (id: number) => void;
  onExpandMaterial: () => void;
  /** A tab to show, as the outliner asks; `serial` grows with each request. */
  openTab: { tab: string; serial: number } | null;
  /** What the outliner has selected, which the Object tab edits. */
  selection: import("../types").Selection;
  onSelect: (selection: import("../types").Selection) => void;
}

/** Most rebound limit the field shows as a number; one step past it means no limit. */
const MAX_REBOUND_LIMIT = 50;

type Tab = "render" | "object" | "light" | "camera" | "image" | "materials" | "system" | "evolution";

const TABS: { id: Tab; label: string; icon: LucideIcon; enhancedOnly: boolean }[] = [
  { id: "render", label: "Render", icon: Gauge, enhancedOnly: false },
  { id: "object", label: "Object", icon: Move3d, enhancedOnly: true },
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
  const { openTab } = props;
  useEffect(() => {
    if (openTab && TABS.some((t) => t.id === openTab.tab)) {
      setChosenTab(openTab.tab as Tab);
      setQuery("");
    }
  }, [openTab]);

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
        {/* One row: render, render after each change, then the scene's and the image's files. */}
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
          <button type="button" className="icon" aria-pressed={props.autoRender || !props.webgl} disabled={!props.webgl}
            aria-label="Render after each change"
            title={props.webgl ? `Render after each change: ${props.autoRender ? "on" : "off"}` : "Renders after each change: there is no 3D view here"}
            onClick={() => props.onAutoRenderChange(!props.autoRender)}>
            <Repeat size={15} aria-hidden="true" />
          </button>
          <span className="actions-gap" />
          <button type="button" className="icon" aria-label="Open scene" title="Open scene (⌘O)" onClick={onOpen}>
            <FolderOpen size={15} aria-hidden="true" />
          </button>
          <button type="button" className="icon" disabled={!props.scene} data-unsaved={props.unsaved || undefined}
            aria-label={props.unsaved ? "Save scene, which has unsaved edits" : "Save scene"}
            title={props.unsaved ? "Save scene (⌘S) · unsaved edits" : "Save scene (⌘S)"} onClick={props.onSaveScene}>
            <Save size={15} aria-hidden="true" />
          </button>
          <button type="button" className="icon" disabled={!canSave} aria-label="Export image" title="Export image (⌘E)" onClick={onSave}>
            <Download size={15} aria-hidden="true" />
          </button>
        </div>
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
              TABS.filter((t) => t.id !== "materials" && t.id !== "object" && !(t.enhancedOnly && !enhanced)).map((t) => (
                <div key={t.id} className="search-group">
                  <h2 className="search-group-title">{t.label}</h2>
                  {tabContent(t.id, context)}
                </div>
              ))
            ) : (
              <>
                {/* The rail names the tab; the title is for screen readers, and saves a row. */}
                <h1 className="tab-title visually-hidden">{TABS.find((t) => t.id === tab)!.label}</h1>
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
    case "object": return <ObjectTab {...context} />;
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
        {enhanced && (
          <>
            <ScrubRow label="Reflection depth" keywords="glossy mirror depth" value={Math.min(settings.glossyDepth ?? settings.depth, settings.depth)}
              min={0} max={settings.depth} step={1} strict format={(v) => (v >= settings.depth ? "Ray depth" : String(v))}
              hint="Most mirror and glossy reflections in a path, within Ray depth. Lower is faster where reflections face each other."
              onChange={(v) => update({ glossyDepth: v >= settings.depth ? null : v })} />
            <ScrubRow label="Glass depth" keywords="transmission refraction depth" value={Math.min(settings.transmissionDepth ?? settings.depth, settings.depth)}
              min={0} max={settings.depth} step={1} strict format={(v) => (v >= settings.depth ? "Ray depth" : String(v))}
              hint="Most crossings of glass and other transparent surfaces in a path, within Ray depth. Glass behind glass needs several."
              onChange={(v) => update({ transmissionDepth: v >= settings.depth ? null : v })} />
          </>
        )}
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
      {scene && (
        <LightsPanel lights={sceneEntries(scene, settings).lights} removed={settings.removedLights} edits={settings.lightEdits}
          hidden={settings.hiddenLights}
          selection={props.selection} onSelect={props.onSelect}
          onEdit={(id, patch) => update(withEntryEdit(settings, true, id, patch))}
          onHiddenChange={(hiddenLights) => update({ hiddenLights })} />
      )}
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
            <option value="hdr">{props.loadingEnvironment ? "Loading…" : environment && !sky ? "Image" : "Image…"}</option>
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
            <EnvironmentPreview environment={environment} exposure={exposureOf(settings)} toneMapping={settings.toneMapping}
              whiteBalance={settings.whiteBalance} tint={settings.tint} pose={settings.pose} onRotate={sky ? undefined : (rotation) => setEnvironment({ rotation })} />
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
              <Row label="File" keywords="hdr exr panorama">
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
          <Hint>Light the scene from all around: a panorama of a real place (HDR, EXR, PNG or JPEG), or a physical sky with the sun.</Hint>
        )}
      </Section>

      {scene && <AtmosphereSection settings={settings} scene={scene} update={update} enhanced={enhanced} />}
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
        <ScrubRow label="Photons" keywords="caustics" value={caustic.photonMillions} min={0.25} max={16} hard={[0.01, 50]} step={0.01}
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
  const camera = cameraOf(settings);
  const focal = focalLength(settings.pose.zoom, settings.sensorWidth);
  const preset = cameraPresetOf(settings);
  return (
    <>
      <Section id="camera-preset" title="Camera">
        <ChoiceRow<string> label="Preset" keywords="camera body film look style lut" value={preset ?? "custom"} disabled={!enhanced}
          hint={preset
            ? `${CAMERA_PRESETS[preset].note} Keeps the framing and the exposure.`
            : "Sets the sensor, film, lens flaws, tone mapping and LUT together. Keeps the framing and the exposure."}
          onChange={(name) => update(withCameraPreset(name))}>
          {Object.entries(CAMERA_PRESETS).map(([name, p]) => <option key={name} value={name}>{p.label}</option>)}
          {!preset && <option value="custom" disabled>Custom</option>}
        </ChoiceRow>
      </Section>
      <Section id="lens" title="Lens"
        advanced={
          <>
            <ChoiceRow<string> label="Sensor" keywords="film gate crop camera" value={String(settings.sensorWidth)}
              hint="The sensor's width. A smaller one, with the same lens, frames tighter, as a crop does."
              onChange={(width) => update({
                sensorWidth: Number(width),
                pose: { ...settings.pose, zoom: zoomFor(focal, Number(width)) },
              })}>
              {SENSORS.map((s) => <option key={s.width} value={String(s.width)}>{s.label} ({s.width} mm)</option>)}
              {!SENSORS.some((s) => s.width === settings.sensorWidth) && (
                <option value={String(settings.sensorWidth)}>{settings.sensorWidth} mm</option>
              )}
            </ChoiceRow>
            <ScrubRow label="Scene unit" keywords="scale size metres meters centimetres" value={settings.sceneUnit * 100}
              min={1} max={100} hard={[0.001, 1e8]} step={0.001} log disabled={!enhanced}
              format={(value) => (value >= 100 ? `${Number((value / 100).toFixed(2))} m` : `${Number(value.toFixed(2))} cm`)}
              hint="How long one scene unit is, which sets how much an f-number blurs: a small scene blurs more, as close-ups do. The examples use 10 cm."
              onChange={(centimetres) => update({ sceneUnit: centimetres / 100 })} />
          </>
        }>
        <ScrubRow label="Focal length" keywords="zoom lens field of view" value={focal} min={10} max={200} hard={[0.5, 1e5]} step={0.1} log
          disabled={!enhanced} format={(value) => `${Number(value.toFixed(value < 100 ? 1 : 0))} mm`}
          hint="Longer lenses frame tighter and blur more. Drag the image to orbit, shift-drag to pan, scroll to move closer."
          onChange={(mm) => update({ pose: { ...settings.pose, zoom: zoomFor(mm, settings.sensorWidth) } })} />
        <ScrubRow label="f-number" keywords="aperture f-stop opening blur depth of field" value={dof.fNumber} min={1} max={22}
          hard={[0.1, 1000]} step={0.1} log disabled={!enhanced} format={(value) => `f/${Number(value.toFixed(value < 10 ? 1 : 0))}`}
          hint={`The opening is ${Number((focal / dof.fNumber).toFixed(1))} mm across. A lower number blurs what is out of focus more, when Depth of field is on, and in Physical exposure lets in more light.`}
          onChange={(fNumber) => update(withDof(settings, { fNumber }))} />
        <ScrubRow label="Vignette" keywords="corners dark falloff" value={settings.vignette} min={0} max={1} step={0.01} strict
          disabled={!enhanced} format={(value) => `${Math.round(value * 100)}%`}
          hint="Darkens the corners as a lens does, with light falling off away from its axis."
          onChange={(vignette) => update({ vignette })} />
        <ScrubRow label="Chromatic aberration" keywords="fringe color lens" value={settings.chromaticAberration} min={0} max={1} step={0.01} strict
          disabled={!enhanced} format={(value) => `${Math.round(value * 100)}%`}
          hint="Red and blue fringes toward the corners, as a simple lens bends colors apart. Applied when the render finishes."
          onChange={(chromaticAberration) => update({ chromaticAberration })} />
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
      <Section id="exposure" title="Exposure"
        extra={
          <Choice<ExposureMode> small label="Exposure mode" value={settings.exposureMode} disabled={!enhanced}
            onChange={(mode) => update(withExposureMode(settings, mode))}>
            <option value="simple">Simple</option>
            <option value="physical">Physical</option>
          </Choice>
        }>
        {settings.exposureMode === "simple" ? (
          <>
            <ScrubRow label="Brightness" keywords="exposure ev stops" value={settings.brightness} min={-6} max={6} hard={[-20, 20]} step={0.1}
              disabled={!enhanced} format={(value) => `${value > 0 ? "+" : ""}${value.toFixed(1)}`}
              hint="In stops, as cameras count light: +1 doubles it, −1 halves it."
              onChange={(brightness) => update({ brightness })} />
            {enhanced && (
              <Hint>Like a camera at ISO {markedIso(camera.iso)}, {markedShutter(camera.shutter)} and f/{Number(dof.fNumber.toFixed(1))}.</Hint>
            )}
          </>
        ) : (
          <>
            <ScrubRow label="ISO" keywords="exposure sensitivity film speed" value={settings.iso} min={ISO_RANGE[0]} max={ISO_RANGE[1]}
              hard={[1, 1e6]} step={1} log disabled={!enhanced} format={(value) => String(Math.round(value))}
              hint="The film's or sensor's sensitivity: twice the ISO is twice the light, and more grain."
              onChange={(iso) => update({ iso })} />
            <ScrubRow label="Shutter speed" keywords="exposure time" value={1 / settings.shutter} min={1 / SHUTTER_RANGE[1]}
              max={1 / SHUTTER_RANGE[0]} hard={[0.01, 1e5]} step={0.01} log disabled={!enhanced}
              format={(value) => shutterLabel(1 / value)}
              hint="Typed as the N of 1/N s: 60 is 1/60 s. A longer time lets in more light. ISO 100 at 1/125 s and f/8 show the light as rendered."
              onChange={(speed) => update({ shutter: 1 / speed })} />
          </>
        )}
        <ScrubRow label="White balance" keywords="temperature kelvin warm cool color" value={settings.whiteBalance}
          min={2500} max={10000} hard={[1500, 15000]} step={50} disabled={!enhanced} format={(value) => `${Math.round(value)} K`}
          hint="The color of light that comes out white: about 3200 K for lamps, 5500 K for the sun. 6500 K changes nothing; lower turns the picture bluer, higher warmer."
          onChange={(whiteBalance) => update({ whiteBalance })} />
        <ScrubRow label="Tint" keywords="white balance green magenta" value={settings.tint} min={-1} max={1} step={0.01} strict
          disabled={!enhanced} format={(value) => `${value > 0 ? "+" : ""}${Math.round(value * 100)}`}
          hint="White balance's other axis: toward magenta to take out the green of fluorescent tubes, or toward green."
          onChange={(tint) => update({ tint })} />
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
        <ChoiceRow<string> label="Shape" hint="Outline of out-of-focus highlights (bokeh)." value={String(settings.blades)}
          onChange={(blades) => update({ blades: Number(blades) })}>
          <option value="0">Round</option>
          <option value="5">5 blades</option>
          <option value="6">6 blades</option>
          <option value="8">8 blades</option>
          {![0, 5, 6, 8].includes(settings.blades) && <option value={settings.blades}>{settings.blades} blades</option>}
        </ChoiceRow>
      </Section>
      <Section id="film" title="Film">
        <ScrubRow label="Grain" keywords="noise film iso" value={settings.grain} min={0} max={1} step={0.01} strict
          disabled={!enhanced} format={(value) => `${Math.round(value * 100)}%`}
          hint="Fine specks, strongest in the mid-tones, as on film. A higher ISO makes them stronger."
          onChange={(grain) => update({ grain })} />
        <ScrubRow label="Halation" keywords="glow film red highlights" value={settings.halation} min={0} max={1} step={0.01} strict
          disabled={!enhanced} format={(value) => `${Math.round(value * 100)}%`}
          hint="A reddish glow around very bright light, as cinema film shows. Applied when the render finishes."
          onChange={(halation) => update({ halation })} />
        <ChoiceRow<string> label="LUT" keywords="look grade color grading cube film" value={settings.lut ?? "none"} disabled={!enhanced}
          hint="A color look graded over the finished picture, as in DaVinci Resolve or V-Ray's frame buffer: one of the program's own, or a .cube file made for sRGB or Rec. 709 images. Saved EXR files keep the light without it."
          onChange={(value) => {
            if (value === "load") props.onChooseLut();
            else update({ lut: value === "none" ? null : value });
          }}>
          <option value="none">None</option>
          {LOOKS.map((look) => <option key={look.name} value={look.name}>{look.label}</option>)}
          {settings.lut && !LOOKS.some((look) => look.name === settings.lut) && (
            <option value={settings.lut}>{lutLabel(settings.lut)}</option>
          )}
          <option value="load">Load .cube…</option>
        </ChoiceRow>
        {settings.lut && (
          <ScrubRow label="LUT strength" keywords="look grade mix" value={settings.lutStrength} min={0} max={1} step={0.01} strict
            disabled={!enhanced} format={(value) => `${Math.round(value * 100)}%`}
            hint="How much of the look to show over the picture's own colors."
            onChange={(lutStrength) => update({ lutStrength })} />
        )}
      </Section>
    </>
  );
}

function ImageTab({ props, update, enhanced }: TabContext) {
  const { settings } = props;
  return (
    <>
      <Section id="color" title="Color">
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

/** A fog when the scene had none: a light haze. */
const NEW_FOG = { density: 0.03, color: [0.9, 0.9, 0.9], anisotropy: 0.3 };

/**
 * What fog, smoke or haze is made of, as the scene file writes it for the fog and for volume
 * objects: how dense, its color, how it scatters, whether it thins with height and whether it
 * billows in clouds. A null in a change takes that field away.
 */
function MediumRows({ medium, onChange, disabled }: {
  medium: Record<string, unknown>;
  onChange: (patch: Record<string, unknown>) => void;
  disabled?: boolean;
}) {
  const number = (key: string, fallback: number) => (typeof medium[key] === "number" ? (medium[key] as number) : fallback);
  const noise = (medium.noise ?? null) as Record<string, number> | null;
  const setNoise = (fields: Record<string, number>) => onChange({ noise: { scale: 1, coverage: 0.5, octaves: 4, ...noise, ...fields } });
  return (
    <>
      <ScrubRow label="Density" keywords="fog haze smoke thick" value={number("density", 0.03)} min={0.001} max={10} hard={[0, 1000]}
        step={0.001} log disabled={disabled}
        hint="How much light it scatters per scene unit: 0.01 is a faint haze across a room, 0.1 a thick fog, and smoke or clouds take more."
        onChange={(density) => onChange({ density })} />
      <ColorRow label="Color" value={(medium.color as Vec3 | undefined) ?? [1, 1, 1]} disabled={disabled} onChange={(color) => onChange({ color })} />
      <ScrubRow label="Forward scatter" keywords="anisotropy haze glow" value={number("anisotropy", 0)} min={-0.95} max={0.95} step={0.01}
        strict disabled={disabled}
        hint="Above 0, light carries on forward, so beams glow toward the sun or a lamp; below, it scatters back."
        onChange={(anisotropy) => onChange({ anisotropy })} />
      <CheckRow label="Thins with height" checked={typeof medium.falloff === "number"} disabled={disabled}
        onChange={(on) => onChange({ falloff: on ? 2 : null })} />
      {typeof medium.falloff === "number" && (
        <ScrubRow label="Height" keywords="falloff fog ground" value={medium.falloff as number} min={0.1} max={20} hard={[0.001, 1e6]} step={0.01} log
          disabled={disabled} hint="Every this many scene units above its bottom, it is about a third as dense: ground fog when small."
          onChange={(falloff) => onChange({ falloff })} />
      )}
      <CheckRow label="Clouds" checked={noise !== null} disabled={disabled}
        onChange={(on) => onChange({ noise: on ? { scale: 1, coverage: 0.5, octaves: 4 } : null })} />
      {noise && (
        <>
          <ScrubRow label="Cloud size" keywords="noise scale smoke" value={noise.scale ?? 1} min={0.05} max={20} hard={[0.001, 1e6]} step={0.01} log
            disabled={disabled} hint="How wide each puff is, in scene units." onChange={(scale) => setNoise({ scale })} />
          <ScrubRow label="Coverage" keywords="noise clouds" value={noise.coverage ?? 0.5} min={0} max={1} step={0.01} strict disabled={disabled}
            hint="How much of the space the puffs fill; the rest is clear." onChange={(coverage) => setNoise({ coverage })} />
          <ScrubRow label="Detail" keywords="noise octaves" value={noise.octaves ?? 4} min={1} max={8} step={1} strict disabled={disabled}
            hint="How many finer layers of puffs it has: more gives ragged, smoky edges." onChange={(octaves) => setNoise({ octaves })} />
        </>
      )}
    </>
  );
}

/** The scene's fog, as its file's `fog`. */
function AtmosphereSection({ settings, scene, update, enhanced }: {
  settings: Settings;
  scene: import("../types").SceneInfo;
  update: (patch: Partial<Settings>) => void;
  enhanced: boolean;
}) {
  const fog = fogOf(scene, settings);
  const change = (fields: Record<string, unknown>) => {
    const next: Record<string, unknown> = { ...(fog ?? NEW_FOG), ...fields };
    for (const [key, value] of Object.entries(next)) if (value === null || value === undefined) delete next[key];
    update({ fogEdit: next });
  };
  return (
    <Section id="atmosphere" title="Atmosphere"
      enabled={{
        checked: fog !== null, disabled: !enhanced,
        onChange: (on) => update({ fogEdit: on ? (scene.fog && settings.fogEdit === false ? null : { ...NEW_FOG }) : false }),
      }}>
      <MediumRows medium={fog ?? NEW_FOG} onChange={change} disabled={!enhanced} />
      <Hint>It fills the box around the objects, or the bounds the scene file gives it, and shows light shafts from lamps and the sun. Saved into the scene file with Save Scene. Techniques &gt; Fog clouds shows clouds in it.</Hint>
    </Section>
  );
}

/** The selected object's or light's place, size and light, as fields of its entry in the file. */
function ObjectTab({ props, update }: TabContext) {
  const { settings, scene, selection } = props;
  if (!scene) return <Hint>Open a scene to edit its objects.</Hint>;
  if (!selection) return <Hint>Select an object or a light in the outliner or the image to move, turn or resize it.</Hint>;
  const light = selection.kind === "light";
  const all = sceneEntries(scene, settings);
  const entry = light ? all.lights[selection.id] : all.objects[selection.id];
  if (!entry) return null;
  const key = light ? "lightEdits" : "objectEdits";
  const allEdits = settings[key];
  const edits = allEdits[selection.id];
  const fields = fieldsFor(entry.kind, entry.source, light);
  const set = (field: string, value: unknown) =>
    update({ [key]: { ...allEdits, [selection.id]: { ...edits, [field]: value } } } as Partial<Settings>);
  const reset = () => {
    const rest = { ...allEdits };
    delete rest[selection.id];
    update({ [key]: rest } as Partial<Settings>);
  };
  const changed = edits !== undefined && Object.keys(edits).length > 0;
  return (
    <Section id={light ? "object-light" : "object-transform"} title={entry.name}
      extra={
        <button type="button" className="prop-mini" disabled={!changed} onClick={reset} title="Back to the scene file's values">
          <RotateCcw size={11} aria-hidden="true" />
          Reset
        </button>
      }>
      {fields.length === 0 && <Hint>Nothing to place here.</Hint>}
      {fields.map((field) => field.type === "lightColor" && light
        ? <LightColorRows key={field.key} light={entry} edits={edits}
            onChange={(patch) => update(withEntryEdit(settings, true, selection.id, patch))} />
        : <FieldRow key={field.key} field={field} source={entry.source} edits={edits} onChange={set} />)}
      {!light && entry.kind === "volume" && (
        // What the volume is made of, as the fog: the same settings.
        <MediumRows medium={{ ...entry.source, ...edits }} onChange={(patch) => update(withEntryEdit(settings, false, selection.id, patch))} />
      )}
      <Hint>Saved into the scene file with Save Scene.</Hint>
    </Section>
  );
}

function FieldRow({ field, source, edits, onChange }: {
  field: Field;
  source: Record<string, unknown>;
  edits: Record<string, unknown> | undefined;
  onChange: (key: string, value: unknown) => void;
}) {
  const value = valueOf(field, source, edits);
  const degrees = (v: number) => `${Number(v.toFixed(1))}°`;
  switch (field.type) {
    case "vector":
      return (
        <VectorRow label={field.label} value={value as Vec3} min={field.min} max={field.max} step={field.step} hard={field.hard}
          hint={field.hint} format={field.angle ? degrees : undefined} onChange={(v) => onChange(field.key, v)} />
      );
    case "number":
      return (
        <ScrubRow label={field.label} value={value as number} min={field.min} max={field.max} step={field.step} hard={field.hard}
          strict={field.strict} log={field.log} hint={field.hint} format={field.angle ? degrees : undefined}
          onChange={(v) => onChange(field.key, v)} />
      );
    case "color":
      return <ColorRow label={field.label} value={value as Vec3} onChange={(v) => onChange(field.key, v)} />;
    case "corners": {
      // A plane is its four corners: shown as where it is, how it is turned from the file's
      // corners and how long its edges are, and written back as corners.
      const original = source.points as Vec3[] | undefined;
      const points = (edits?.points ?? source.points) as Vec3[] | undefined;
      if (!original || !points || original.length !== 4 || points.length !== 4) return null;
      const state = planeState(original, points);
      const set = (change: Partial<PlaneState>) => onChange("points", planePoints(original, { ...state, ...change }));
      return (
        <>
          <VectorRow label="Position" value={state.position} min={-20} max={20} step={0.01} hard={[-1e6, 1e6]}
            hint="Where the middle of its four corners is." onChange={(position) => set({ position })} />
          <VectorRow label="Rotation" value={state.rotation} min={-180} max={180} step={1} hard={[-3600, 3600]}
            format={degrees} hint="Degrees about X, then Y, then Z, from how the scene file lays it."
            onChange={(rotation) => set({ rotation })} />
          <ScrubRow label="Width" value={state.width} min={0.01} max={20} step={0.01} hard={[1e-4, 1e6]}
            hint="Its first edge, from the first corner to the second." onChange={(width) => set({ width })} />
          <ScrubRow label="Depth" value={state.depth} min={0.01} max={20} step={0.01} hard={[1e-4, 1e6]}
            hint="Its last edge, from the first corner to the fourth." onChange={(depth) => set({ depth })} />
        </>
      );
    }
    case "edge": {
      const edge = (edits?.[field.key] ?? source[field.key]) as Vec3 | undefined;
      if (!edge) return null;
      return (
        <ScrubRow label={field.label} value={Math.hypot(...edge)} min={0.01} max={20} step={0.01} hard={[1e-4, 1e6]}
          onChange={(v) => onChange(field.key, resizedEdge(edge, v))} />
      );
    }
  }
}

function MaterialsTab({ props, update }: TabContext) {
  const { settings, scene } = props;
  if (!scene) return <Hint>Open a scene to edit its materials.</Hint>;
  return (
    <MaterialEditor key={scene.path} materials={scene.materials} overrides={settings.materialOverrides}
      scenePath={scene.path} device={settings.device} entries={settings.addedObjects.length > 0 ? entryEdits(settings) : null}
      selected={props.selectedMaterial} onSelect={props.onSelectMaterial}
      onExpand={props.onExpandMaterial} onChange={(materialOverrides) => update({ materialOverrides })} />
  );
}

function SystemTab({ props, update }: TabContext) {
  const { settings, cores } = props;
  return (
    <Section id="performance" title="Performance">
      <Row label="Device" keywords="gpu graphics card cpu metal directx vulkan"
        hint="Auto renders on the graphics card when it gives the CPU's image, and on the CPU otherwise. The 2008 and Fixed stages always use the CPU; with caustics, the CPU traces the photons first.">
        <Segmented<Device>
          label="Device"
          value={settings.device}
          options={[
            { value: "auto", label: "Auto" },
            { value: "gpu", label: "GPU" },
            { value: "cpu", label: "CPU" },
          ]}
          onChange={(device) => update({ device })}
        />
      </Row>
      <ScrubRow label="Threads" keywords="cores cpu" value={settings.threads} min={1} max={cores} step={1} strict
        onChange={(threads) => update({ threads })} />
      <CheckRow label="Acceleration" keywords="bvh kd-tree"
        hint="Skips objects a ray cannot hit: a BVH, or the original KD-Tree in the 2008 stage. Turn it off only to compare speed."
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
