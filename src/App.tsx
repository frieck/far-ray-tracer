import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ControlPanel } from "./components/ControlPanel";
import { MaterialStage } from "./components/MaterialStage";
import { Viewport } from "./components/Viewport";
import { Renderer, type RenderJob, type RenderView } from "./renderer";
import { alignView, dolly, orbit, pan, retarget } from "./orbit";
import { SceneView, type PreviewScene } from "./sceneView";
import { activeComparison, activeMode, buildPreview, buildRequest, defaultSettings, imageSize } from "./settings";
import type { EnvironmentConfig, MenuAction, Pose, SceneInfo, Settings, Startup } from "./types";

/** Waits for a pause in slider drags before starting a new render. */
const SETTINGS_DELAY_MS = 180;
/** A scroll wheel has no release, so a pause this long ends the camera preview. */
const WHEEL_END_MS = 250;

const scenePose = ({ camera: { position, target, zoom } }: SceneInfo): Pose => ({ position, target, zoom });

function buildJob(settings: Settings, moving: boolean): RenderJob {
  if (moving) return { main: buildPreview(settings), comparison: null, preview: true };
  const comparison = activeComparison(settings);
  return {
    main: buildRequest(settings, activeMode(settings), true),
    comparison: comparison && buildRequest(settings, comparison, false),
  };
}

export function App() {
  const afterRef = useRef<HTMLCanvasElement>(null);
  const beforeRef = useRef<HTMLCanvasElement>(null);
  const photonRef = useRef<HTMLCanvasElement>(null);
  const liveRef = useRef<HTMLCanvasElement>(null);
  const sceneView = useRef<SceneView | null>(null);
  /** The scene whose geometry the 3D view holds. */
  const [liveScene, setLiveScene] = useState<SceneInfo | null>(null);
  const [webgl, setWebgl] = useState(true);
  const [autoRender, setAutoRender] = useState(false);
  const [renderedKey, setRenderedKey] = useState<string | null>(null);
  const [awaitingImage, setAwaitingImage] = useState(false);
  const [showPhotonMap, setShowPhotonMap] = useState(false);
  const [showMaterial, setShowMaterial] = useState(false);
  const [selectedMaterial, setSelectedMaterial] = useState<number | null>(null);
  const [pickingFocus, setPickingFocus] = useState(false);
  const [focusHint, setFocusHint] = useState("");
  const focusRequest = useRef(0);
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
          setSettings((old) => old && ({
            ...old,
            environment: info.environment,
            materialOverrides: [],
            pose: scenePose(info),
            depthOfField: info.camera.aperture > 0,
            dofPreset: info.camera.aperture > 0 ? "custom" : "portrait",
            dofPasses: 256,
            aperture: info.camera.aperture > 0 ? info.camera.aperture : 0.5,
            focusDistance: info.camera.focus_distance,
          }));
          pivotChosen.current = false;
          setRenderedKey(null);
          setAwaitingImage(true);
          publish({ painted: false });
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
  const job = useMemo(() => settings && buildJob(settings, movingCamera && !liveReady), [settings, movingCamera, liveReady]);
  const [imageWidth, imageHeight] = settings ? imageSize(settings) : [4, 3];
  const jobKey = JSON.stringify(job);
  // Without WebGL there is nothing else to show, so every change renders.
  const renderAlone = autoRender || !webgl;

  const start = useCallback((next: RenderJob) => {
    if (!next.preview) setRenderedKey(JSON.stringify(next));
    void renderer.render(next);
  }, [renderer]);

  useEffect(() => {
    if (!scene || !job) return;
    if (movingCamera && liveReady) {
      void renderer.stop();
      return;
    }
    if (!renderAlone) return;
    const timer = setTimeout(() => start(job), movingCamera ? 0 : SETTINGS_DELAY_MS);
    return () => clearTimeout(timer);
    // `jobKey` stands for `job`, so a new object with the same values does not re-render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scene, jobKey, renderer, movingCamera, liveReady, renderAlone, start]);

  useEffect(() => {
    if (!liveRef.current || sceneView.current) return;
    try {
      sceneView.current = new SceneView(liveRef.current);
    } catch {
      sceneView.current = null;
      setWebgl(false);
    }
  }, []);

  useEffect(() => {
    if (!scene || !sceneView.current) return;
    let current = true;
    invoke<PreviewScene>("scene_geometry").then((preview) => {
      if (!current) return;
      sceneView.current?.load(preview);
      setLiveScene(scene);
    }).catch(() => {
      // Without geometry the 3D view cannot stand in for the render.
      if (current) setWebgl(false);
    });
    return () => {
      current = false;
    };
  }, [scene]);

  const showLive = liveReady && (movingCamera || awaitingImage);
  // 2008 and Fixed always render from the scene's own camera.
  const livePose = settings && scene && (activeMode(settings) === "enhanced" ? settings.pose : scenePose(scene));
  useEffect(() => {
    if (showLive && livePose) sceneView.current?.draw(livePose, imageWidth / imageHeight);
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

  const pickFocus = async (x: number, y: number) => {
    if (!scene || !settings || !pickingFocus) return;
    const request = ++focusRequest.current;
    setFocusHint("Finding focus…");
    try {
      const distance = await invoke<number | null>("pick_focus", { pose: settings.pose, width: imageWidth, height: imageHeight, x, y, useKd: settings.useKd });
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

  // The ball needs Enhanced mode: its materials, environment and look only exist there.
  const canShowMaterial = scene !== null && scene.materials.length > 0 && settings !== null && activeMode(settings) === "enhanced";
  const ballMaterial = canShowMaterial ? scene.materials.find((m) => m.id === selectedMaterial) ?? scene.materials[0] : null;
  const materialStage = showMaterial && ballMaterial && settings ? (
    <MaterialStage
      key={scene?.path}
      id={ballMaterial.id}
      label={ballMaterial.label}
      surface={settings.materialOverrides.find((m) => m.id === ballMaterial.id)?.surface ?? null}
      onClose={() => setShowMaterial(false)}
    />
  ) : null;

  // The menu lives for the whole session, so it reads the latest handlers from a ref.
  const onMenu = useRef<(menu: MenuAction) => void>(() => {});
  onMenu.current = ({ action, path }) => {
    if (action === "open") void open("choose_scene");
    else if (action === "example") void open("load_scene", { path });
    else if (action === "save" && canSave) void save();
    else if (action === "render" && scene) renderNow();
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
        } else if (showMaterial) setShowMaterial(false);
        else void renderer.stop();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [renderer, pickingFocus, showMaterial]);

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
          navigable={scene !== null && settings !== null && activeMode(settings) === "enhanced"
            && !activeComparison(settings) && !pickingFocus && !(showPhotonMap && view.hasPhotonMap) && !materialStage}
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
        {settings && (
          <ControlPanel
            settings={settings}
            loadingEnvironment={loadingEnvironment}
            onImportEnvironment={() => void importEnvironment()}
            scene={scene}
            onResetCamera={resetCamera}
            cores={cores}
            rendering={view.rendering}
            canRender={scene !== null}
            canSave={canSave}
            report={view.caustics}
            pickingFocus={pickingFocus}
            onPickFocus={() => {
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
            selectedMaterial={selectedMaterial}
            onSelectMaterial={setSelectedMaterial}
            onExpandMaterial={() => {
              setShowPhotonMap(false);
              setShowMaterial(true);
            }}
          />
        )}
      </div>
    </>
  );
}
