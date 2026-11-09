import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ExternalLink } from "lucide-react";
import { useEffect, useState } from "react";

const ICON = new URL("../../src-tauri/icons/128x128@2x.png", import.meta.url).href;
const REPOSITORY = "github.com/frieck/far-ray-tracer";
const TECHNIQUES = ["Path tracing", "GGX materials", "Photon-mapped caustics", "OIDN denoising"];

/** The About box, opened from the app menu (macOS) or the Help menu (Windows). */
export function AboutDialog() {
  const [open, setOpen] = useState(false);
  const [version, setVersion] = useState("");

  useEffect(() => {
    const stopListening = listen("show-about", () => setOpen(true));
    getVersion().then(setVersion, () => {});
    return () => void stopListening.then((stop) => stop(), () => {});
  }, []);

  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" || event.key === "Enter") {
        event.stopPropagation();
        setOpen(false);
      }
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [open]);

  if (!open) return null;

  return (
    <div className="about-backdrop" onPointerDown={() => setOpen(false)}>
      <section className="about glass" role="dialog" aria-modal="true" aria-labelledby="about-title"
        onPointerDown={(event) => event.stopPropagation()}>
        <img className="about-icon" src={ICON} alt="" width={96} height={96} draggable={false} />
        <h1 id="about-title">Far Ray Tracer</h1>
        {version && <p className="about-version">Version {version}</p>}
        <p className="about-description">
          A ray tracer that began in 2008 as a final project at UDESC and was ported from C++ to Rust.
        </p>
        <ul className="about-techniques" aria-label="Techniques">
          {TECHNIQUES.map((technique) => <li key={technique}>{technique}</li>)}
        </ul>
        <dl className="about-facts">
          <div><dt>Author</dt><dd>FARSystems / Felipe Rieck</dd></div>
          <div><dt>License</dt><dd>MIT</dd></div>
        </dl>
        <div className="about-actions">
          <button type="button" className="ghost" onClick={() => void invoke("open_repository").catch(() => {})}>
            <ExternalLink size={14} />
            {REPOSITORY}
          </button>
          <button type="button" className="primary" autoFocus onClick={() => setOpen(false)}>OK</button>
        </div>
        <p className="about-credits">
          Includes Intel Open Image Denoise and oneTBB, © Intel Corporation, under the Apache License 2.0.
          See THIRD_PARTY_NOTICES.md.
        </p>
        <p className="about-copyright">© 2026 FARSystems</p>
      </section>
    </div>
  );
}
