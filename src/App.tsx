import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AboutDialog } from "./components/AboutDialog";
import { ControlPanel } from "./components/ControlPanel";
import { Outliner } from "./components/Outliner";
import { LIGHT_ID, SelectionOverlay, type IdMap } from "./components/SelectionOverlay";
import { TransformGizmo, type GizmoMode } from "./components/TransformGizmo";
import type { Vec3 } from "./objectFields";
import { changeMatrix, transformOf, type Change } from "./transforms";
import { duplicate, newEntry, sceneEntries, withAdded, withRemoved, type NewKind } from "./sceneEntries";
import * as history from "./history";
import { MaterialStage } from "./components/MaterialStage";
import { Viewport } from "./components/Viewport";
import { Renderer, type RenderJob, type RenderView } from "./renderer";
import { alignView, dolly, orbit, pan, retarget } from "./orbit";
import { SceneView, type PreviewScene } from "./sceneView";
import {
  activeComparison, activeMode, buildPreview, buildRequest, defaultSettings, DOF_PRESETS, fNumberFor, focalLength, imageSize,
  sceneSaveRequest, sceneView, withSavedRenderSettings, fogOf,
} from "./settings";
import type { Area, EnvironmentConfig, MenuAction, Pose, RenderRequest, SceneInfo, Selection, Settings, Startup } from "./types";

/** Waits for a pause in slider drags before starting a new render. */
const SETTINGS_DELAY_MS = 180;
/** A scroll wheel has no release, so a pause this long ends the camera preview. */
const WHEEL_END_MS = 250;

const scenePose = ({ camera: { position, target, zoom } }: SceneInfo): Pose => ({ position, target, zoom });

/**
 * The job without how the light is shown (exposure, tone mapping, the camera's white balance,
 * tint, vignetting, chromatic aberration, grain and halation, the LUT, denoise and bloom): two jobs
 * with the same light differ only in what the engine shows again at once, without rendering.
 */
const OUTLINER_HEIGHT_KEY = "far.outliner.height";
const OUTLINE_KEY = "far.outliner.outline";
/** How long settings stay put before a change becomes an undo step. */
const SETTLE_MS = 400;
/** Largest side of the ID map the outline is drawn from, in screen pixels. */
const MAX_ID_SIDE = 2048;

/**
 * What `load` gives for the latest `key`, one request at a time: keys that come while one runs
 * wait, and only the newest is loaded next, so a moving camera never queues up requests.
 */
function useLatest<T>(key: string | null, load: () => Promise<T>): { key: string; value: T } | null {
  const [result, setResult] = useState<{ key: string; value: T } | null>(null);
  const wanted = useRef<{ key: string; load: () => Promise<T> } | null>(null);
  const busy = useRef(false);
  const done = useRef<string | null>(null);
  wanted.current = key === null ? null : { key, load };
  useEffect(() => {
    if (key === null || busy.current) return;
    busy.current = true;
    void (async () => {
      while (wanted.current && wanted.current.key !== done.current) {
        const next = wanted.current;
        try {
          const value = await next.load();
          setResult({ key: next.key, value });
        } catch {
          // Shown without it; the same key is not asked again.
        }
        done.current = next.key;
      }
      busy.current = false;
    })();
  }, [key]);
  return result;
}
const OUTLINER_COLLAPSED_KEY = "far.outliner.collapsed";
const MIN_OUTLINER_HEIGHT = 120;
/** What the outliner leaves the settings below it, at least. */
const MIN_SETTINGS_HEIGHT = 280;

/** A number kept in local storage, or `fallback`. */
function remembered(key: string, fallback: number): number {
  try {
    const value = Number(localStorage.getItem(key));
    return localStorage.getItem(key) !== null && Number.isFinite(value) ? value : fallback;
  } catch {
    return fallback;
  }
}

function lightKey(job: RenderJob): string {
  const strip = (request: RenderRequest | null) => request && ({
    ...request, exposure: 0, tone_mapping: null, white_balance: 0, tint: 0, vignette: 0, chromatic_aberration: 0, grain: 0, iso: 0,
    halation: 0, lut: null, lut_strength: 0, denoise: false, bloom: false,
  });
  return JSON.stringify({ ...job, main: strip(job.main), comparison: strip(job.comparison) });
}

function buildJob(settings: Settings, moving: boolean, area: Area | null): RenderJob {
  if (moving) return { main: buildPreview(settings), comparison: null, preview: true };
  const comparison = activeComparison(settings);
  const [width, height] = imageSize(settings);
  // Whole pixels that cover the area.
  const region: [number, number, number, number] | null = area && [
    Math.floor(area.x0 * width), Math.floor(area.y0 * height), Math.ceil(area.x1 * width), Math.ceil(area.y1 * height),
  ];
  return {
    main: { ...buildRequest(settings, activeMode(settings), true), region },
    comparison: comparison && { ...buildRequest(settings, comparison, false), region },
  };
}

export function App() {
  const afterRef = useRef<HTMLCanvasElement>(null);
  const beforeRef = useRef<HTMLCanvasElement>(null);
  const photonRef = useRef<HTMLCanvasElement>(null);
  const liveRef = useRef<HTMLCanvasElement>(null);
  const sceneView3d = useRef<SceneView | null>(null);
  /** The scene whose geometry the 3D view holds. */
  const [liveScene, setLiveScene] = useState<SceneInfo | null>(null);
  const [webgl, setWebgl] = useState(true);
  const [autoRender, setAutoRender] = useState(false);
  const [renderedKey, setRenderedKey] = useState<string | null>(null);
  const [awaitingImage, setAwaitingImage] = useState(false);
  const [showPhotonMap, setShowPhotonMap] = useState(false);
  /** The part of the image to render, or null for all of it. */
  const [region, setRegion] = useState<Area | null>(null);
  const [drawingRegion, setDrawingRegion] = useState(false);
  /** Set when a region is drawn without auto-render: the next job renders at once. */
  const renderOnNextJob = useRef(false);
  const [showMaterial, setShowMaterial] = useState(false);
  const [selectedMaterial, setSelectedMaterial] = useState<number | null>(null);
  // The outliner's selection, and whether its tab shows, when a click in the image selects.
  const [selection, setSelection] = useState<Selection>(null);
  // A tab the outliner asks the settings to show, and the outliner's height and folding.
  const [openTab, setOpenTab] = useState<{ tab: string; serial: number } | null>(null);
  const showTab = (tab: string) => setOpenTab((old) => ({ tab, serial: (old?.serial ?? 0) + 1 }));
  const [outlinerHeight, setOutlinerHeight] = useState(() => remembered(OUTLINER_HEIGHT_KEY, 260));
  const [outlinerCollapsed, setOutlinerCollapsed] = useState(() => remembered(OUTLINER_COLLAPSED_KEY, 0) === 1);
  const sidebarRef = useRef<HTMLElement>(null);
  const splitDrag = useRef<{ y: number; height: number } | null>(null);
  const [pickingFocus, setPickingFocus] = useState(false);
  const [focusHint, setFocusHint] = useState("");
  const focusRequest = useRef(0);
  /** What Save Scene would write when the scene was opened or last saved. */
  const savedState = useRef<string | null>(null);
  const snapshotPending = useRef(false);
  const [, setSavedVersion] = useState(0);
  const unsavedRef = useRef(false);
  const keepGoingRef = useRef<() => Promise<boolean>>(async () => true);
  const environmentRequest = useRef(0);
  const [loadingEnvironment, setLoadingEnvironment] = useState(false);
  const [movingCamera, setMovingCamera] = useState(false);
  const pivotChosen = useRef(false);
  const wheelTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const ambientRef = useRef<HTMLCanvasElement>(null);

  const [cores, setCores] = useState(1);
  const [scene, setScene] = useState<SceneInfo | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [view, setView] = useState<RenderView>({ rendering: false, progress: 0, status: "Ready", hasResult: false, caustics: null, hasPhotonMap: false, painted: false });

  const publish = useCallback((patch: Partial<RenderView>) => setView((old) => ({ ...old, ...patch })), []);
  const renderer = useMemo(
    () =>
      new Renderer(() => {
        const [after, before, ambient, photons] = [afterRef.current, beforeRef.current, ambientRef.current, photonRef.current];
        return after && before && ambient && photons ? { after, before, ambient, photons } : null;
      }, publish),
    [publish],
  );

  const open = useCallback(
    async (command: string, args?: Record<string, unknown>) => {
      if (!(await keepGoingRef.current())) return;
      focusRequest.current += 1;
      setPickingFocus(false);
      setFocusHint("");
      environmentRequest.current += 1;
      setLoadingEnvironment(false);
      await renderer.stop();
      try {
        const info = await invoke<SceneInfo | null>(command, args);
        if (info) {
          publish({ hasResult: false, caustics: null, hasPhotonMap: false });
          setShowPhotonMap(false);
          setShowMaterial(false);
          setSelectedMaterial(null);
          setSelection(null);
          setRegion(null);
          setDrawingRegion(false);
          setSettings((old) => old && ({
            // What another scene hid means nothing here.
            ...withSavedRenderSettings({ ...old, hiddenObjects: [], hiddenLights: [], objectEdits: {}, lightEdits: {},
              addedObjects: [], addedLights: [], removedObjects: [], removedLights: [], fogEdit: null }, info.render),
            environment: info.environment,
            materialOverrides: [],
            pose: scenePose(info),
            depthOfField: info.camera.aperture > 0,
            dofPreset: info.camera.aperture > 0 ? "custom" : "portrait",
            dofPasses: 256,
            // The scene keeps the opening's diameter; the panel shows it as an f-number.
            fNumber: info.camera.aperture > 0
              ? fNumberFor(focalLength(info.camera.zoom, info.camera.sensor_width), info.camera.aperture, info.camera.scene_unit)
              : DOF_PRESETS.portrait.fNumber,
            sensorWidth: info.camera.sensor_width,
            sceneUnit: info.camera.scene_unit,
            focusDistance: info.camera.focus_distance,
            blades: info.camera.blades,
            bladeRotation: info.camera.blade_rotation,
          }));
          pivotChosen.current = false;
          setRenderedKey(null);
          setAwaitingImage(true);
          publish({ painted: false });
          savedState.current = null;
          snapshotPending.current = true;
          setScene(info);
        }
      } catch (error) {
        publish({ status: `Error: ${String(error)}` });
      }
    },
    [publish, renderer],
  );

  useEffect(() => {
    let cancelled = false;
    invoke<Startup>("startup").then((startup) => {
      if (cancelled) return;
      setCores(startup.cores);
      setSettings(defaultSettings(startup.cores));
      if (startup.initial_scene) void open("load_scene", { path: startup.initial_scene });
    });
    return () => {
      cancelled = true;
    };
  }, [open]);

  // Ready only once the 3D view holds this scene: a flag reset and set again could be batched
  // into no change at all, and the view would keep the previous scene's geometry.
  const liveReady = scene !== null && liveScene === scene;

  // Without WebGL, a small ray traced image follows the camera instead.
  const job = useMemo(() => settings && buildJob(settings, movingCamera && !liveReady, region),
    [settings, movingCamera, liveReady, region]);
  const [imageWidth, imageHeight] = settings ? imageSize(settings) : [4, 3];
  const jobKey = JSON.stringify(job);
  // The file's objects and lights, then those added here.
  const entries = scene && settings ? sceneEntries(scene, settings) : null;
  const onDeleteKey = useRef(() => {});
  const onDuplicateKey = useRef(() => {});
  const openRef = useRef(open);
  openRef.current = open;

  // What each pixel shows, from the engine, for the selection's outline, the pointer's
  // highlight and picking without waiting: a few milliseconds, so it follows the camera.
  const [outline, setOutline] = useState(() => remembered(OUTLINE_KEY, 1) === 1);
  const [hover, setHover] = useState<number | null>(null);
  // The map is as sharp as the overlay is on screen, in its pixels, with the image's shape.
  const [overlayWidth, setOverlayWidth] = useState(0);
  const onOverlaySize = useCallback((width: number) => setOverlayWidth(width), []);
  const wantedWidth = Math.min(overlayWidth || imageWidth, MAX_ID_SIDE, (MAX_ID_SIDE * imageWidth) / imageHeight);
  const idWidth = Math.max(1, Math.round(wantedWidth));
  const idHeight = Math.max(1, Math.round((idWidth * imageHeight) / imageWidth));
  const outlinable = scene !== null && settings !== null && activeMode(settings) === "enhanced" && !activeComparison(settings);
  const idArgs = outlinable && settings ? {
    pose: settings.pose, width: idWidth, height: idHeight, view: sceneView(settings),
  } : null;
  const idKey = idArgs && scene ? JSON.stringify({ path: scene.path, ...idArgs }) : null;
  const ids = useLatest(idKey, async () => {
    const buffer = await invoke<ArrayBuffer>("object_ids", idArgs!);
    return { path: scene!.path, width: idArgs!.width, height: idArgs!.height, ids: new Uint16Array(buffer) } as IdMap & { path: string };
  });
  // The last map of this scene and size, even a step behind a moving camera.
  const idMap = ids && scene && ids.value.path === scene.path && ids.value.width === idWidth && ids.value.height === idHeight
    ? ids.value : null;
  const xrayObject = outline && selection?.kind === "object" ? selection.id : null;
  const xrayKey = idKey && xrayObject !== null ? `${idKey}|${xrayObject}` : null;
  const xrays = useLatest(xrayKey, async () => ({
    path: scene!.path, object: xrayObject!, width: idWidth,
    mask: new Uint8Array(await invoke<ArrayBuffer>("object_xray", { pose: settings!.pose, width: idWidth, height: idHeight, object: xrayObject, view: sceneView(settings!) })),
  }));
  const xray = xrays && idMap && xrays.value.path === idMap.path && xrays.value.object === xrayObject && xrays.value.width === idWidth
    ? xrays.value.mask : null;
  const selectedCode = selection ? (selection.kind === "object" ? selection.id + 1 : LIGHT_ID + selection.id) : null;
  /** The ID under a point of the image, from the map. */
  const idAt = (x: number, y: number) => {
    if (!idMap) return null;
    const px = Math.min(idMap.width - 1, Math.max(0, Math.floor(x * idMap.width)));
    const py = Math.min(idMap.height - 1, Math.max(0, Math.floor(y * idMap.height)));
    return idMap.ids[py * idMap.width + px];
  };
  // Without WebGL there is nothing else to show, so every change renders.
  const renderAlone = autoRender || !webgl;

  const start = useCallback((next: RenderJob) => {
    if (!next.preview) setRenderedKey(JSON.stringify(next));
    void renderer.render(next);
  }, [renderer]);

  useEffect(() => {
    if (!renderOnNextJob.current || !scene || !job) return;
    renderOnNextJob.current = false;
    start(job);
    // `jobKey` stands for `job`: this runs once the job holds the new region.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [jobKey]);

  useEffect(() => {
    if (!scene || !job) return;
    if (movingCamera && liveReady) {
      void renderer.stop();
      return;
    }
    // Without auto-render, a change of how the light is shown (exposure, tone mapping, the lens,
    // denoise or bloom) still applies at
    // once: the last finished render is shown again with it, without rendering.
    const lookOnly = !renderAlone && !job.preview && view.hasResult && renderedKey !== null && renderedKey !== jobKey
      && lightKey(JSON.parse(renderedKey) as RenderJob) === lightKey(job);
    if (!renderAlone && !lookOnly) return;
    const timer = setTimeout(() => start(job), movingCamera ? 0 : SETTINGS_DELAY_MS);
    return () => clearTimeout(timer);
    // `jobKey` stands for `job`, so a new object with the same values does not re-render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scene, jobKey, renderer, movingCamera, liveReady, renderAlone, start]);

  useEffect(() => {
    if (!liveRef.current || sceneView3d.current) return;
    try {
      sceneView3d.current = new SceneView(liveRef.current);
    } catch {
      sceneView3d.current = null;
      setWebgl(false);
    }
  }, []);

  // The 3D view shows the scene as edited and hidden here; edits wait a moment, as a drag
  // changes them many times a second.
  const viewArgs = settings ? sceneView(settings) : null;
  const viewKey = JSON.stringify(viewArgs);
  const geometryScene = useRef<SceneInfo | null>(null);
  useEffect(() => {
    if (!scene || !sceneView3d.current) return;
    let current = true;
    const first = geometryScene.current !== scene;
    const timer = window.setTimeout(() => {
      invoke<PreviewScene>("scene_geometry", { view: viewArgs }).then((preview) => {
        if (!current) return;
        sceneView3d.current?.load(preview);
        geometryScene.current = scene;
        setLiveScene(scene);
      }).catch(() => {
        // Without geometry the 3D view cannot stand in for the render; an edit the engine
        // refuses keeps the last view.
        if (current && first) setWebgl(false);
      });
    }, first ? 0 : 120);
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
  }, [scene, viewKey]);

  const showLive = liveReady && (movingCamera || awaitingImage);
  // 2008 and Fixed always render from the scene's own camera.
  const livePose = settings && scene && (activeMode(settings) === "enhanced" ? settings.pose : scenePose(scene));

  // The gizmo, at the selected object or light: moving, turning or scaling it. While it is
  // dragged only the 3D view changes the selection, at once; letting go writes the edit, and
  // the engine reads the scene again.
  const [preview, setPreview] = useState<Change | null>(null);
  const [grab, setGrab] = useState(0);
  // Adding, duplicating and deleting: changes to the settings, so undo covers them.
  const change = (patch: Partial<Settings>) => {
    publish({ caustics: null, hasPhotonMap: false });
    setSettings((old) => old && { ...old, ...patch });
  };
  const addEntry = (kind: NewKind) => {
    if (!settings || !entries) return;
    const { light, entry } = newEntry(kind, settings.pose, entries);
    const { patch, id } = withAdded(settings, entries, light, entry);
    change(patch);
    setSelection({ kind: light ? "light" : "object", id });
  };
  const duplicateSelection = (follow: boolean) => {
    if (!settings || !entries || !selection) return;
    const light = selection.kind === "light";
    const entry = duplicate(entries, settings, light, selection.id, !follow);
    if (!entry) return;
    const { patch, id } = withAdded(settings, entries, light, entry);
    change(patch);
    setSelection({ kind: selection.kind, id });
    // As Blender's Shift+D, the copy then moves with the mouse.
    if (follow) setGrab((n) => n + 1);
  };
  onDuplicateKey.current = () => duplicateSelection(true);
  onDeleteKey.current = () => deleteSelection();
  const deleteSelection = () => {
    if (!settings || !selection) return;
    change(withRemoved(settings, selection.kind === "light", selection.id));
    setSelection(null);
  };
  const [gizmoMode, setGizmoMode] = useState<GizmoMode>("move");
  const selectedEntry = selection && entries
    ? (selection.kind === "light" ? entries.lights[selection.id] : entries.objects[selection.id]) : null;
  const editsKey = selection?.kind === "light" ? "lightEdits" : "objectEdits";
  const transform = selection && selectedEntry && settings
    ? transformOf(selectedEntry.kind, selectedEntry.source, settings[editsKey][selection.id], selection.kind === "light") : null;
  const pieceKey = selection ? `${selection.kind === "light" ? "l" : "o"}${selection.id}` : null;
  const previewGizmo = (change: Change | null) => {
    setPreview(change);
    setMovingCamera(change !== null);
    if (change && settings && transform) {
      sceneView3d.current?.place(pieceKey, changeMatrix(change, transform.point));
      sceneView3d.current?.draw(settings.pose, imageWidth / imageHeight);
    }
  };
  const commitGizmo = (change: Change) => {
    if (!selection || !transform) return;
    const patch = transform.apply(change);
    if (Object.keys(patch).length === 0) return;
    // Until the 3D view reads the edited scene, it keeps the piece where it was dropped. As any
    // edit, a change moves the light, and the photons with it.
    publish({ caustics: null, hasPhotonMap: false });
    setSettings((old) => {
      if (!old) return old;
      const all = old[editsKey];
      return { ...old, [editsKey]: { ...all, [selection.id]: { ...all[selection.id], ...patch } } };
    });
  };
  // The gizmo follows a move as it goes; it turns and scales in place.
  const gizmoPoint: Vec3 | null = transform
    ? (preview?.type === "move" ? transform.point.map((v, k) => v + preview.offset[k]) as Vec3 : transform.point) : null;
  useEffect(() => {
    if (showLive && livePose) sceneView3d.current?.draw(livePose, imageWidth / imageHeight);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [showLive, liveScene, JSON.stringify(livePose), imageWidth, imageHeight]);

  // The live view stays until the render has replaced the old framing.
  useEffect(() => {
    if (!movingCamera && view.painted) setAwaitingImage(false);
  }, [movingCamera, view.painted]);

  const renderNow = useCallback(() => {
    if (job) start(job);
  }, [job, start]);

  const outdated = scene !== null && !renderAlone && !view.rendering && renderedKey !== jobKey;

  /** The first move orbits around the objects, not around the original target. */
  const moveCamera = (move: (pose: Pose) => Pose) => {
    const center = pivotChosen.current ? null : (scene?.center ?? null);
    pivotChosen.current = true;
    if (!movingCamera) publish({ painted: false });
    setMovingCamera(true);
    setAwaitingImage(true);
    setSettings((old) => old && { ...old, pose: move(retarget(old.pose, center)) });
  };

  const resetCamera = () => {
    if (!scene) return;
    pivotChosen.current = false;
    publish({ painted: false });
    setAwaitingImage(true);
    setSettings((old) => old && { ...old, pose: scenePose(scene) });
  };

  /** Looks along a scene axis, around the objects the first time, like an orbit. */
  const alignCamera = (axis: [number, number, number]) => {
    if (!scene) return;
    const center = pivotChosen.current ? null : scene.center;
    pivotChosen.current = true;
    publish({ painted: false });
    setAwaitingImage(true);
    setSettings((old) => old && { ...old, pose: alignView(retarget(old.pose, center), axis) });
  };

  /** Selects in the outliner what is under a point of the image, and its first material. */
  const selectAt = async (x: number, y: number) => {
    if (!scene || !settings) return;
    // The map of this very view answers at once.
    const id = ids && ids.key === idKey ? idAt(x, y) : null;
    if (id !== null) {
      select(id === 0 ? null : id >= LIGHT_ID ? { kind: "light", id: id - LIGHT_ID } : { kind: "object", id: id - 1 });
      return;
    }
    try {
      const picked = await invoke<Selection>("pick_object", { pose: settings.pose, width: imageWidth, height: imageHeight, x, y,
        useKd: settings.useKd, view: sceneView(settings) });
      select(picked);
    } catch (error) {
      publish({ status: `Error: ${String(error)}` });
    }
  };

  const select = (picked: Selection) => {
    setSelection(picked);
    const first = picked?.kind === "object" ? entries?.objects[picked.id]?.materials[0] : undefined;
    if (first !== undefined && scene?.materials.some((m) => m.id === first)) setSelectedMaterial(first);
  };

  const pickFocus = async (x: number, y: number) => {
    if (!scene || !settings || !pickingFocus) return;
    const request = ++focusRequest.current;
    setFocusHint("Finding focus…");
    try {
      const distance = await invoke<number | null>("pick_focus", { pose: settings.pose, width: imageWidth, height: imageHeight, x, y,
        useKd: settings.useKd, view: sceneView(settings) });
      if (request !== focusRequest.current) return;
      if (distance === null) {
        setFocusHint("No surface here. Click an object, or press Esc to cancel.");
        return;
      }
      setPickingFocus(false);
      setFocusHint("");
      publish({ caustics: null, hasPhotonMap: false });
      setSettings((old) => old && ({ ...old, depthOfField: true, focusDistance: distance }));
    } catch (error) {
      if (request === focusRequest.current) setFocusHint(`Could not pick focus: ${String(error)}`);
    }
  };

  const importEnvironment = async () => {
    const request = ++environmentRequest.current;
    setLoadingEnvironment(true);
    try {
      const environment = await invoke<EnvironmentConfig | null>("choose_environment");
      if (request !== environmentRequest.current || !environment) return;
      publish({ caustics: null, hasPhotonMap: false });
      setShowPhotonMap(false);
      setSettings((old) => old && ({ ...old, environment }));
    } catch (error) {
      if (request === environmentRequest.current) publish({ status: `Error: ${String(error)}` });
    } finally {
      if (request === environmentRequest.current) setLoadingEnvironment(false);
    }
  };

  const chooseLut = async () => {
    try {
      const lut = await invoke<string | null>("choose_lut");
      if (lut) setSettings((old) => old && ({ ...old, lut }));
    } catch (error) {
      publish({ status: `Error: ${String(error)}` });
    }
  };

  const save = useCallback(async () => {
    if (!scene) return;
    try {
      const path = await invoke<string | null>("save_image", { name: scene.name });
      if (path) publish({ status: `Saved to ${path}` });
    } catch (error) {
      publish({ status: `Error: ${String(error)}` });
    }
  }, [publish, scene]);

  const canSave = view.hasResult && !view.rendering;

  /** Saves the scene with the edits made here: over its file, or as a new one. True if saved. */
  const saveScene = useCallback(async (choose: boolean, reopen = true) => {
    if (!scene || !settings) return false;
    const request = sceneSaveRequest(settings);
    try {
      const saved = await invoke<{ path: string; warnings: string[]; reopen: boolean } | null>("save_scene", { choose, request });
      if (!saved) return false;
      savedState.current = JSON.stringify(request);
      setSavedVersion((v) => v + 1);
      publish({ status: [`Saved scene to ${saved.path}`, ...saved.warnings].join(". ") });
      // Objects or lights added or deleted moved the indices: read the saved file again.
      if (saved.reopen && reopen) {
        unsavedRef.current = false;
        void openRef.current("load_scene", { path: saved.path });
      }
      return true;
    } catch (error) {
      publish({ status: `Error: could not save the scene: ${String(error)}` });
      return false;
    }
  }, [publish, scene, settings]);

  // Edits not yet saved: what Save Scene would write differs from what was opened or saved.
  const current = scene && settings ? JSON.stringify(sceneSaveRequest(settings)) : null;
  const unsaved = current !== null && savedState.current !== null && current !== savedState.current;
  unsavedRef.current = unsaved;
  // The state of a newly opened scene, once its settings are in place, is the saved one.
  useEffect(() => {
    if (snapshotPending.current && current !== null) {
      savedState.current = current;
      snapshotPending.current = false;
      setSavedVersion((v) => v + 1);
    }
  }, [current]);

  /** Asks what to do with unsaved edits before they would be lost; false keeps them and stops. */
  const keepGoing = useCallback(async () => {
    if (!unsavedRef.current || !scene) return true;
    const answer = await invoke<"save" | "discard" | "cancel">("ask_unsaved", { name: scene.name });
    if (answer === "cancel") return false;
    if (answer === "save") return saveScene(false, false);
    return true;
  }, [scene, saveScene]);
  keepGoingRef.current = keepGoing;

  useEffect(() => {
    void invoke("set_unsaved", { unsaved });
  }, [unsaved]);

  useEffect(() => {
    const stop = listen("quit-requested", async () => {
      if (await keepGoingRef.current()) await invoke("quit");
    });
    return () => void stop.then((unlisten) => unlisten());
  }, []);

  useEffect(() => {
    const window = getCurrentWindow();
    const stop = window.onCloseRequested(async (event) => {
      if (!unsavedRef.current) return;
      event.preventDefault();
      // Quitting rather than closing the window: the app has one window, and closing it
      // while edits are unsaved would otherwise be held back again.
      if (await keepGoingRef.current()) await invoke("quit");
    });
    return () => void stop.then((unlisten) => unlisten());
  }, []);

  // The ball needs Enhanced mode: its materials, environment and look only exist there.
  const canShowMaterial = scene !== null && scene.materials.length > 0 && settings !== null && activeMode(settings) === "enhanced";
  const ballMaterial = canShowMaterial ? scene.materials.find((m) => m.id === selectedMaterial) ?? scene.materials[0] : null;
  const materialStage = showMaterial && ballMaterial && settings ? (
    <MaterialStage
      key={scene?.path}
      id={ballMaterial.id}
      label={ballMaterial.label}
      device={settings.device}
      edit={settings.materialOverrides.find((m) => m.id === ballMaterial.id) ?? null}
      onClose={() => setShowMaterial(false)}
    />
  ) : null;

  // The menu lives for the whole session, so it reads the latest handlers from a ref.
  const onMenu = useRef<(menu: MenuAction) => void>(() => {});
  onMenu.current = ({ action, path }) => {
    if (action === "open") void open("choose_scene");
    else if (action === "example") void open("load_scene", { path });
    else if (action === "save" && canSave) void save();
    else if (action === "save_scene") void saveScene(false);
    else if (action === "save_scene_as") void saveScene(true);
    else if (action === "render" && scene) renderNow();
    else if (action === "undo" || action === "redo") step(action);
  };

  // Undo and redo. A change is recorded once settings stay put for a moment, so a drag is one
  // step; a new scene starts a new history.
  const steps = useRef<history.History | null>(null);
  const stepsScene = useRef<SceneInfo | null>(null);
  const undoable = settings && scene ? history.snapshot(settings) : null;
  useEffect(() => {
    if (undoable === null) return;
    if (stepsScene.current !== scene || !steps.current) {
      steps.current = history.start(undoable);
      stepsScene.current = scene;
      return;
    }
    const timer = window.setTimeout(() => {
      if (steps.current) steps.current = history.record(steps.current, undoable);
    }, SETTLE_MS);
    return () => window.clearTimeout(timer);
  }, [undoable, scene]);
  const step = (direction: "undo" | "redo") => {
    // In a text field, the menu undoes the typing, as it would anywhere else.
    const focused = document.activeElement as HTMLElement | null;
    if (focused && (focused.tagName === "INPUT" || focused.tagName === "TEXTAREA")) {
      document.execCommand(direction);
      return;
    }
    if (!steps.current || undoable === null) return;
    const { history: next, to } = history[direction](steps.current, undoable);
    steps.current = next;
    if (to === null) {
      publish({ status: direction === "undo" ? "Nothing to undo" : "Nothing to redo" });
      return;
    }
    focusRequest.current += 1;
    setPickingFocus(false);
    publish({ caustics: null, hasPhotonMap: false, status: direction === "undo" ? "Undone" : "Redone" });
    setShowPhotonMap(false);
    setSettings((old) => old && history.restore(old, to));
  };

  useEffect(() => {
    const stopListening = listen<MenuAction>("menu-action", (event) => onMenu.current(event.payload));
    return () => void stopListening.then((stop) => stop());
  }, []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        if (pickingFocus) {
          focusRequest.current += 1;
          setPickingFocus(false);
          setFocusHint("");
        } else if (drawingRegion) setDrawingRegion(false);
        else if (showMaterial) setShowMaterial(false);
        else if (selection) setSelection(null);
        else void renderer.stop();
        return;
      }
      const target = event.target as HTMLElement | null;
      if (target && (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName))) return;
      if (!selection || event.metaKey || event.ctrlKey || event.altKey) return;
      if (event.key === "Delete" || event.key === "Backspace") {
        event.preventDefault();
        onDeleteKey.current();
      } else if (event.shiftKey && event.key.toUpperCase() === "D") {
        event.preventDefault();
        onDuplicateKey.current();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [renderer, pickingFocus, showMaterial, drawingRegion, selection]);

  useEffect(() => {
    try {
      localStorage.setItem(OUTLINER_COLLAPSED_KEY, outlinerCollapsed ? "1" : "0");
    } catch {
      // The layout is a convenience; without storage it opens as it starts.
    }
  }, [outlinerCollapsed]);

  /** The outliner's height within what the sidebar leaves the settings. */
  const fitOutliner = (height: number) => {
    const room = (sidebarRef.current?.clientHeight ?? 800) - MIN_SETTINGS_HEIGHT;
    return Math.round(Math.max(MIN_OUTLINER_HEIGHT, Math.min(height, room)));
  };
  const resizeOutliner = (height: number) => {
    const fitted = fitOutliner(height);
    setOutlinerHeight(fitted);
    try {
      localStorage.setItem(OUTLINER_HEIGHT_KEY, String(fitted));
    } catch {
      // As above.
    }
  };

  return (
    <>
      <canvas ref={ambientRef} className="ambient" width={48} height={36} aria-hidden="true" />
      <div className="veil" aria-hidden="true" />
      <div className="layout">
        <Viewport
          afterRef={afterRef}
          beforeRef={beforeRef}
          photonRef={photonRef}
          liveRef={liveRef}
          showLive={showLive}
          scene={scene}
          pickingFocus={pickingFocus}
          focusHint={focusHint}
          onPickFocus={(x, y) => void pickFocus(x, y)}
          onSelectAt={(x, y) => void selectAt(x, y)}
          onHover={(point) => {
            const id = point && outline ? idAt(point.x, point.y) : null;
            setHover(id ? id : null);
          }}
          overlay={outlinable ? (
            <>
              {outline && !preview && (
                <SelectionOverlay map={idMap} selected={selectedCode} hover={hover} xray={xray} onSize={onOverlaySize} />
              )}
              {gizmoPoint && transform && settings && !pickingFocus && !drawingRegion && (
                <TransformGizmo pose={settings.pose} aspect={imageWidth / imageHeight} point={gizmoPoint}
                  mode={gizmoMode === "rotate" && !transform.canRotate || gizmoMode === "scale" && !transform.canScale ? "move" : gizmoMode}
                  onMode={setGizmoMode} canRotate={transform.canRotate} canScale={transform.canScale} scaleAxes={transform.scaleAxes}
                  grab={grab} onPreview={previewGizmo} onCommit={commitGizmo} onCancel={() => {
                    // Back as it was, in the 3D view too.
                    sceneView3d.current?.place(null);
                    if (settings) sceneView3d.current?.draw(settings.pose, imageWidth / imageHeight);
                  }} />
              )}
            </>
          ) : null}
          navigable={scene !== null && settings !== null && activeMode(settings) === "enhanced"
            && !activeComparison(settings) && !pickingFocus && !(showPhotonMap && view.hasPhotonMap) && !materialStage
            && !drawingRegion}
          onCameraDrag={(panning, dx, dy, stageHeight) => moveCamera((pose) => panning
            ? pan(pose, dx, dy, stageHeight, imageWidth / imageHeight)
            : orbit(pose, dx, dy))}
          onCameraRelease={() => setMovingCamera(false)}
          pose={livePose}
          onAlignView={alignCamera}
          onCameraWheel={(amount) => {
            moveCamera((pose) => dolly(pose, amount));
            clearTimeout(wheelTimer.current);
            wheelTimer.current = setTimeout(() => setMovingCamera(false), WHEEL_END_MS);
          }}
          mode={settings ? activeMode(settings) : "enhanced"}
          comparison={settings && activeComparison(settings)}
          status={view.status}
          progress={view.progress}
          rendering={view.rendering}
          aspect={imageWidth / imageHeight}
          hasPhotonMap={view.hasPhotonMap}
          showPhotonMap={showPhotonMap && view.hasPhotonMap}
          canShowMaterial={canShowMaterial}
          region={region}
          drawingRegion={drawingRegion}
          onDrawingRegionChange={(value) => {
            focusRequest.current += 1;
            setPickingFocus(false);
            setDrawingRegion(value);
          }}
          onRegionChange={(area) => {
            setRegion(area);
            // Without auto-render, a new region renders at once; that is what it was drawn for.
            if (area && !renderAlone) renderOnNextJob.current = true;
          }}
          materialStage={materialStage}
          onMaterialChange={(value) => {
            focusRequest.current += 1;
            setPickingFocus(false);
            setShowMaterial(value);
          }}
          onPhotonMapChange={(value) => {
            focusRequest.current += 1;
            setPickingFocus(false);
            setShowPhotonMap(value);
          }}
        />
        <aside className="sidebar" ref={sidebarRef}>
        {settings && scene && (
          <>
            <Outliner scene={scene} hiddenObjects={settings.hiddenObjects} hiddenLights={settings.hiddenLights}
              objects={entries!.objects} lights={entries!.lights}
              removedObjects={settings.removedObjects} removedLights={settings.removedLights}
              onAdd={addEntry} onDuplicate={() => duplicateSelection(false)} onDelete={deleteSelection}
              onHiddenChange={(hiddenObjects, hiddenLights) => {
                // As any edit: what hides changes the light, and the photons with it.
                publish({ caustics: null, hasPhotonMap: false });
                setShowPhotonMap(false);
                setSettings((old) => old && { ...old, hiddenObjects, hiddenLights });
              }}
              selection={selection} onSelect={select}
              onOpenCamera={() => showTab("camera")} onOpenEnvironment={() => showTab("light")}
              fog={fogOf(scene, settings) !== null} onOpenAtmosphere={() => showTab("light")}
              onEditMaterial={(id) => {
                setSelectedMaterial(id);
                showTab("materials");
              }}
              onEditObject={() => showTab("object")}
              collapsed={outlinerCollapsed} onCollapsedChange={setOutlinerCollapsed}
              outline={outline} onOutlineChange={(value) => {
                setOutline(value);
                try {
                  localStorage.setItem(OUTLINE_KEY, value ? "1" : "0");
                } catch {
                  // A convenience: without storage it is on again next time.
                }
              }}
              style={outlinerCollapsed ? undefined : { height: fitOutliner(outlinerHeight) }} />
            {!outlinerCollapsed && (
              <div className="sidebar-split" role="separator" aria-orientation="horizontal" aria-label="Outliner height"
                aria-valuenow={outlinerHeight} tabIndex={0} title="Drag to resize the outliner"
                onPointerDown={(event) => {
                  event.currentTarget.setPointerCapture(event.pointerId);
                  splitDrag.current = { y: event.clientY, height: fitOutliner(outlinerHeight) };
                }}
                onPointerMove={(event) => {
                  if (splitDrag.current) setOutlinerHeight(fitOutliner(splitDrag.current.height + event.clientY - splitDrag.current.y));
                }}
                onPointerUp={() => {
                  if (splitDrag.current) resizeOutliner(outlinerHeight);
                  splitDrag.current = null;
                }}
                onPointerCancel={() => { splitDrag.current = null; }}
                onKeyDown={(event) => {
                  if (event.key === "ArrowUp") resizeOutliner(outlinerHeight - 24);
                  if (event.key === "ArrowDown") resizeOutliner(outlinerHeight + 24);
                }} />
            )}
          </>
        )}
        {settings && (
          <ControlPanel
            settings={settings}
            loadingEnvironment={loadingEnvironment}
            onImportEnvironment={() => void importEnvironment()}
            onChooseLut={() => void chooseLut()}
            scene={scene}
            onResetCamera={resetCamera}
            cores={cores}
            rendering={view.rendering}
            canRender={scene !== null}
            canSave={canSave}
            report={view.caustics}
            pickingFocus={pickingFocus}
            onPickFocus={() => {
              setDrawingRegion(false);
              focusRequest.current += 1;
              setPickingFocus(!pickingFocus);
              setFocusHint("Click a surface to focus. Esc cancels.");
              setShowPhotonMap(false);
            }}
            onChange={(next) => {
              focusRequest.current += 1;
              setPickingFocus(false);
              publish({ caustics: null, hasPhotonMap: false });
              setShowPhotonMap(false);
              setSettings(next);
            }}
            onOpen={() => void open("choose_scene")}
            onRender={renderNow}
            autoRender={autoRender}
            onAutoRenderChange={setAutoRender}
            webgl={webgl}
            outdated={outdated && (view.hasResult ? "Changes are not rendered yet." : "This is a quick 3D view. Render to ray trace it.")}
            onCancel={() => void renderer.stop()}
            onSave={() => void save()}
            onSaveScene={() => void saveScene(false)}
            unsaved={unsaved}
            selectedMaterial={selectedMaterial}
            onSelectMaterial={setSelectedMaterial}
            openTab={openTab}
            selection={selection}
            onSelect={select}
            onExpandMaterial={() => {
              setShowPhotonMap(false);
              setShowMaterial(true);
            }}
          />
        )}
        </aside>
      </div>
      <AboutDialog />
    </>
  );
}
