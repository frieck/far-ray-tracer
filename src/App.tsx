import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ControlPanel } from "./components/ControlPanel";
import { Viewport } from "./components/Viewport";
import { Renderer, type RenderJob, type RenderView } from "./renderer";
import { activeComparison, activeMode, buildRequest, defaultSettings, imageSize } from "./settings";
import type { MenuAction, SceneInfo, Settings, Startup } from "./types";

/** Waits for a pause in slider drags before starting a new render. */
const SETTINGS_DELAY_MS = 180;

function buildJob(settings: Settings): RenderJob {
  const comparison = activeComparison(settings);
  return {
    main: buildRequest(settings, activeMode(settings), true),
    comparison: comparison && buildRequest(settings, comparison, false),
  };
}

export function App() {
  const afterRef = useRef<HTMLCanvasElement>(null);
  const beforeRef = useRef<HTMLCanvasElement>(null);
  const ambientRef = useRef<HTMLCanvasElement>(null);

  const [cores, setCores] = useState(1);
  const [scene, setScene] = useState<SceneInfo | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [view, setView] = useState<RenderView>({ rendering: false, progress: 0, status: "Ready", hasResult: false, caustics: null });

  const publish = useCallback((patch: Partial<RenderView>) => setView((old) => ({ ...old, ...patch })), []);
  const renderer = useMemo(
    () =>
      new Renderer(() => {
        const [after, before, ambient] = [afterRef.current, beforeRef.current, ambientRef.current];
        return after && before && ambient ? { after, before, ambient } : null;
      }, publish),
    [publish],
  );

  const open = useCallback(
    async (command: string, args?: Record<string, unknown>) => {
      await renderer.stop();
      try {
        const info = await invoke<SceneInfo | null>(command, args);
        if (info) {
          publish({ hasResult: false });
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

  const job = useMemo(() => settings && buildJob(settings), [settings]);
  const [imageWidth, imageHeight] = settings ? imageSize(settings) : [4, 3];
  const jobKey = JSON.stringify(job);

  useEffect(() => {
    if (!scene || !job) return;
    const timer = setTimeout(() => void renderer.render(job), SETTINGS_DELAY_MS);
    return () => clearTimeout(timer);
    // `jobKey` stands for `job`, so a new object with the same values does not re-render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scene, jobKey, renderer]);

  const renderNow = useCallback(() => {
    if (job) void renderer.render(job);
  }, [job, renderer]);

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
      if (event.key === "Escape") void renderer.stop();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [renderer]);

  return (
    <>
      <canvas ref={ambientRef} className="ambient" width={48} height={36} aria-hidden="true" />
      <div className="veil" aria-hidden="true" />
      <div className="layout">
        <Viewport
          afterRef={afterRef}
          beforeRef={beforeRef}
          scene={scene}
          mode={settings ? activeMode(settings) : "enhanced"}
          comparison={settings && activeComparison(settings)}
          status={view.status}
          progress={view.progress}
          rendering={view.rendering}
          aspect={imageWidth / imageHeight}
        />
        {settings && (
          <ControlPanel
            settings={settings}
            cores={cores}
            rendering={view.rendering}
            canRender={scene !== null}
            canSave={canSave}
            report={view.caustics}
            onChange={setSettings}
            onOpen={() => void open("choose_scene")}
            onRender={renderNow}
            onCancel={() => void renderer.stop()}
            onSave={() => void save()}
          />
        )}
      </div>
    </>
  );
}
