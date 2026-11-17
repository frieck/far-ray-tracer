import {
  Box, Camera, ChevronRight, Circle, Cloud, CloudFog, Cone, Cylinder, Eye, EyeOff, Flashlight, Globe, Grid2x2, Lamp, Lightbulb,
  AppWindow, Copy, Move3d, Package, Plus, RectangleHorizontal, ScanEye, Search, Sparkles, Square, Sun, Torus, Trash2, Triangle, X, type LucideIcon,
} from "lucide-react";
import { useEffect, useRef, useState, type CSSProperties } from "react";
import { NEW_LIGHTS, NEW_OBJECTS, type NewKind } from "../sceneEntries";
import type { SceneInfo, SceneLightInfo, SceneObjectInfo, Selection } from "../types";
import { GlassPanel } from "./GlassPanel";

const OBJECT_KINDS: Record<string, { icon: LucideIcon; label: string }> = {
  sphere: { icon: Circle, label: "Sphere" },
  box: { icon: Box, label: "Box" },
  cylinder: { icon: Cylinder, label: "Cylinder" },
  tube: { icon: Cylinder, label: "Tube" },
  cone: { icon: Cone, label: "Cone" },
  torus: { icon: Torus, label: "Torus" },
  ground: { icon: Grid2x2, label: "Endless ground" },
  plane: { icon: Square, label: "Plane" },
  mesh: { icon: Triangle, label: "3DS mesh" },
  model: { icon: Package, label: "Model" },
  volume: { icon: Cloud, label: "Volume" },
  fog: { icon: CloudFog, label: "Fog" },
};

const LIGHT_KINDS: Record<string, { icon: LucideIcon; label: string }> = {
  point: { icon: Lightbulb, label: "Point light" },
  rectangle: { icon: RectangleHorizontal, label: "Area light" },
  sphere: { icon: Sun, label: "Sphere light" },
  spot: { icon: Flashlight, label: "Spotlight" },
  ies: { icon: Lamp, label: "IES light" },
  portal: { icon: AppWindow, label: "Light portal" },
};

/** Groups of the list that can be folded, remembered between runs. */
const FOLDED_KEY = "far.outliner.folded";

function savedFolded(): string[] {
  try {
    const value = JSON.parse(localStorage.getItem(FOLDED_KEY) ?? "[]");
    return Array.isArray(value) ? value.filter((v) => typeof v === "string") : [];
  } catch {
    return [];
  }
}

interface OutlinerProps {
  scene: SceneInfo;
  /** The file's objects and lights then the added ones, by index, and those deleted. */
  objects: SceneObjectInfo[];
  lights: SceneLightInfo[];
  removedObjects: number[];
  removedLights: number[];
  onAdd: (kind: NewKind) => void;
  onDuplicate: () => void;
  onDelete: () => void;
  hiddenObjects: number[];
  hiddenLights: number[];
  onHiddenChange: (objects: number[], lights: number[]) => void;
  selection: Selection;
  onSelect: (selection: Selection) => void;
  /** Opens the camera's or the environment's settings. */
  onOpenCamera: () => void;
  onOpenEnvironment: () => void;
  /** The scene has fog, edited in Light > Atmosphere, which this opens. */
  fog: boolean;
  onOpenAtmosphere: () => void;
  /** Opens a material in the Materials tab. */
  onEditMaterial: (id: number) => void;
  /** Opens the selection in the Object tab. */
  onEditObject: () => void;
  /** Folds the whole panel to its header. */
  collapsed: boolean;
  onCollapsedChange: (collapsed: boolean) => void;
  /** Outlines the selection, and what the pointer is over, on the image. */
  outline: boolean;
  onOutlineChange: (outline: boolean) => void;
  style?: CSSProperties;
}

/**
 * The scene as a list, always in view above the settings, as in Blender: the camera, the
 * environment, the lights and the objects, each selected from here or the image and shown or
 * hidden.
 */
export function Outliner(props: OutlinerProps) {
  const { scene, hiddenObjects, hiddenLights, selection } = props;
  const [filter, setFilter] = useState("");
  const [searching, setSearching] = useState(false);
  const [folded, setFolded] = useState<string[]>(savedFolded);
  const list = useRef<HTMLUListElement>(null);

  useEffect(() => {
    try {
      localStorage.setItem(FOLDED_KEY, JSON.stringify(folded));
    } catch {
      // Folding is a convenience; without storage the groups open again next time.
    }
  }, [folded]);

  // What is selected in the image scrolls into view.
  useEffect(() => {
    list.current?.querySelector("[data-selected]")?.scrollIntoView({ block: "nearest" });
  }, [selection]);

  const toggle = (items: number[], id: number) => (items.includes(id) ? items.filter((v) => v !== id) : [...items, id].sort((a, b) => a - b));
  const glows = (materials: number[]) => materials.some((id) => scene.materials.find((m) => m.id === id)?.emission);
  const query = filter.trim().toLowerCase();
  const shows = (name: string) => query === "" || name.toLowerCase().includes(query);
  const hiddenCount = hiddenObjects.length + hiddenLights.length;
  const selected = selection?.kind === "object" ? props.objects[selection.id] : null;
  const selectedLight = selection?.kind === "light" ? props.lights[selection.id] : null;
  const isHidden = selection ? (selection.kind === "object" ? hiddenObjects : hiddenLights).includes(selection.id) : false;
  const lights = props.lights.map((light, id) => ({ light, id }))
    .filter(({ light, id }) => !props.removedLights.includes(id) && shows(light.name));
  const objects = props.objects.map((object, id) => ({ object, id }))
    .filter(({ object, id }) => !props.removedObjects.includes(id) && shows(object.name));
  const [adding, setAdding] = useState(false);
  const open = (group: string) => query !== "" || !folded.includes(group);
  const fold = (group: string) => setFolded(folded.includes(group) ? folded.filter((g) => g !== group) : [...folded, group]);

  return (
    <GlassPanel className="outliner-panel" aria-label="Outliner" data-collapsed={props.collapsed || undefined} style={props.style}>
      <div className="outliner-head">
        <button type="button" className="prop-fold" aria-expanded={!props.collapsed} onClick={() => props.onCollapsedChange(!props.collapsed)}>
          <ChevronRight size={13} aria-hidden="true" />
          <span>Outliner</span>
        </button>
        <span className="prop-header-space" />
        {hiddenCount > 0 && (
          <button type="button" className="prop-mini" onClick={() => props.onHiddenChange([], [])} title="Show every hidden object and light">
            Show all
          </button>
        )}
        <div className="outliner-add">
          <button type="button" className="outliner-tool" aria-expanded={adding} aria-label="Add an object or a light"
            title="Add an object or a light where the camera looks" onClick={() => setAdding(!adding)}>
            <Plus size={14} aria-hidden="true" />
          </button>
          {adding && (
            <>
              <div className="outliner-add-backdrop" onClick={() => setAdding(false)} />
              <div className="outliner-add-menu glass" role="menu">
                {[["Objects", NEW_OBJECTS], ["Lights", NEW_LIGHTS]].map(([title, items]) => (
                  <div key={title as string} role="group" aria-label={title as string}>
                    <p className="outliner-add-title">{title as string}</p>
                    {(items as typeof NEW_OBJECTS).map(({ kind, label }) => (
                      <button key={kind} type="button" role="menuitem" onClick={() => {
                        setAdding(false);
                        props.onAdd(kind);
                      }}>
                        {label}
                      </button>
                    ))}
                  </div>
                ))}
              </div>
            </>
          )}
        </div>
        <button type="button" className="outliner-tool" aria-pressed={props.outline} aria-label="Outline the selection on the image"
          title={`Outline the selection on the image: ${props.outline ? "on" : "off"}`} onClick={() => props.onOutlineChange(!props.outline)}>
          <ScanEye size={13} aria-hidden="true" />
        </button>
        {!props.collapsed && (
          <button type="button" className="outliner-tool" aria-pressed={searching} aria-label="Find by name" title="Find by name"
            onClick={() => {
              setSearching(!searching);
              setFilter("");
            }}>
            <Search size={13} aria-hidden="true" />
          </button>
        )}
      </div>
      {!props.collapsed && (
        <>
          {searching && (
            <div className="prop-search outliner-search">
              <Search size={12} aria-hidden="true" />
              <input type="search" autoFocus placeholder="Find by name" aria-label="Find objects and lights by name" value={filter}
                onChange={(event) => setFilter(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Escape") {
                    setSearching(false);
                    setFilter("");
                  }
                }} />
              {filter && <button type="button" aria-label="Clear" onClick={() => setFilter("")}><X size={11} aria-hidden="true" /></button>}
            </div>
          )}
          <ul className="outliner" ref={list} aria-label="Scene">
            {query === "" && <Item icon={Camera} name="Camera" onSelect={props.onOpenCamera} />}
            {query === "" && scene.environment && (
              <Item icon={Globe} name={scene.environment.sky ? "Physical sky" : "Environment"} onSelect={props.onOpenEnvironment} />
            )}
            {query === "" && props.fog && <Item icon={CloudFog} name="Fog" onSelect={props.onOpenAtmosphere} />}
            <Group name="Lights" count={lights.length} open={open("lights")} onFold={() => fold("lights")} />
            {open("lights") && lights.length === 0 && query === "" && (
              <li className="outliner-empty">Only glowing objects and the environment light this scene.</li>
            )}
            {open("lights") && lights.map(({ light, id }) => (
              <Item key={`l${id}`} icon={LIGHT_KINDS[light.kind]?.icon ?? Lightbulb} name={light.name}
                selected={selection?.kind === "light" && selection.id === id}
                hidden={hiddenLights.includes(id)}
                onSelect={() => props.onSelect({ kind: "light", id })}
                onToggle={() => props.onHiddenChange(hiddenObjects, toggle(hiddenLights, id))} />
            ))}
            <Group name="Objects" count={objects.length} open={open("objects")} onFold={() => fold("objects")} />
            {open("objects") && objects.map(({ object, id }) => (
              <Item key={`o${id}`} icon={OBJECT_KINDS[object.kind]?.icon ?? Triangle} name={object.name}
                glow={glows(object.materials)}
                selected={selection?.kind === "object" && selection.id === id}
                hidden={hiddenObjects.includes(id)}
                onSelect={() => props.onSelect({ kind: "object", id })}
                onToggle={() => props.onHiddenChange(toggle(hiddenObjects, id), hiddenLights)} />
            ))}
          </ul>
          {selection && (selected || selectedLight) ? (
            <div className="outliner-selected">
              <div className="outliner-selected-head">
                <span className="outliner-name">{selected?.name ?? selectedLight!.name}</span>
                <span className="outliner-kind">
                  {selected ? OBJECT_KINDS[selected.kind]?.label ?? selected.kind : LIGHT_KINDS[selectedLight!.kind]?.label ?? selectedLight!.kind}
                </span>
                <button type="button" className="outliner-eye" aria-label="Move, turn or resize it" title="Move, turn or resize it"
                  onClick={props.onEditObject}>
                  <Move3d size={13} aria-hidden="true" />
                </button>
                <button type="button" className="outliner-eye" aria-label="Duplicate it" title="Duplicate it (⇧D)" onClick={props.onDuplicate}>
                  <Copy size={13} aria-hidden="true" />
                </button>
                <button type="button" className="outliner-eye" aria-label="Delete it" title="Delete it (Delete)" onClick={props.onDelete}>
                  <Trash2 size={13} aria-hidden="true" />
                </button>
                <button type="button" className="outliner-eye" aria-pressed={!isHidden}
                  aria-label={isHidden ? "Show it" : "Hide it"} title={isHidden ? "Show" : "Hide"}
                  onClick={() => selection.kind === "object"
                    ? props.onHiddenChange(toggle(hiddenObjects, selection.id), hiddenLights)
                    : props.onHiddenChange(hiddenObjects, toggle(hiddenLights, selection.id))}>
                  {isHidden ? <EyeOff size={13} aria-hidden="true" /> : <Eye size={13} aria-hidden="true" />}
                </button>
                <button type="button" className="outliner-eye" aria-label="Clear the selection" title="Clear the selection (Esc)"
                  onClick={() => props.onSelect(null)}>
                  <X size={13} aria-hidden="true" />
                </button>
              </div>
              {selected && selected.materials.length > 0 && (
                <div className="outliner-materials" aria-label="Materials">
                  {selected.materials.map((id) => {
                    const material = scene.materials.find((m) => m.id === id);
                    if (!material) return null;
                    const [r, g, b] = material.color.map((c) => Math.round(Math.min(1, Math.max(0, c)) ** (1 / 2.2) * 255));
                    return (
                      <button key={id} type="button" className="outliner-material" onClick={() => props.onEditMaterial(id)}
                        title={`Edit ${material.label} in Materials`}>
                        <span className="swatch" style={{ background: `rgb(${r}, ${g}, ${b})` }} aria-hidden="true" />
                        <span className="outliner-name">{material.label}</span>
                      </button>
                    );
                  })}
                </div>
              )}
            </div>
          ) : (
            <p className="outliner-note">Click an object in the image or the list to select it.</p>
          )}
        </>
      )}
    </GlassPanel>
  );
}

function Group(props: { name: string; count: number; open: boolean; onFold: () => void }) {
  return (
    <li className="outliner-group">
      <button type="button" className="prop-fold" aria-expanded={props.open} onClick={props.onFold}>
        <ChevronRight size={12} aria-hidden="true" />
        <span>{props.name}</span>
      </button>
      <span className="outliner-count">{props.count}</span>
    </li>
  );
}

function Item(props: {
  icon: LucideIcon;
  name: string;
  selected?: boolean;
  hidden?: boolean;
  glow?: boolean;
  onSelect: () => void;
  onToggle?: () => void;
}) {
  const Icon = props.icon;
  return (
    <li className="outliner-row" data-selected={props.selected || undefined} data-hidden={props.hidden || undefined}>
      <button type="button" className="outliner-pick" aria-current={props.selected || undefined} onClick={props.onSelect}>
        <Icon size={13} aria-hidden="true" />
        <span className="outliner-name">{props.name}</span>
        {props.glow && <Sparkles size={11} className="outliner-glow" aria-label="glows" />}
      </button>
      {props.onToggle && (
        <button type="button" className="outliner-eye" aria-pressed={!props.hidden}
          aria-label={props.hidden ? `Show ${props.name}` : `Hide ${props.name}`} title={props.hidden ? "Show" : "Hide"}
          onClick={props.onToggle}>
          {props.hidden ? <EyeOff size={13} aria-hidden="true" /> : <Eye size={13} aria-hidden="true" />}
        </button>
      )}
    </li>
  );
}
