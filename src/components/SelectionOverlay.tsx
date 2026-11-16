import { useEffect, useRef } from "react";
import { drawOutline } from "../outline";

/** What each pixel of the image shows, as the app's `object_ids` gives it. */
export interface IdMap {
  width: number;
  height: number;
  /** 0 for nothing; an object's index plus one; a light's index with `LIGHT_ID` set. */
  ids: Uint16Array;
}

/** In an ID map, a light's index has this bit set, as in the app's `ID_LIGHT`. */
export const LIGHT_ID = 0x8000;

interface SelectionOverlayProps {
  /** The map to draw from, at `size` or a step behind it while the camera moves; null for none yet. */
  map: IdMap | null;
  /** The ID of what is selected and of what is under the pointer, if anything. */
  selected: number | null;
  hover: number | null;
  /** Where the selected object would show with nothing in front of it, one byte per pixel. */
  xray: Uint8Array | null;
  /** The overlay's size in screen pixels, for a map as sharp as the screen. */
  onSize: (width: number, height: number) => void;
}

/**
 * Outlines the selected object over the image, which stays as rendered: a line and a light fill
 * where it shows, a fainter line where something hides it, and a white line around what the
 * pointer is over. Drawn from an ID map at the screen's resolution, so the line is smooth.
 */
export function SelectionOverlay({ map, selected, hover, xray, onSize }: SelectionOverlayProps) {
  const canvas = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const element = canvas.current;
    if (!element) return;
    const report = () => {
      const box = element.getBoundingClientRect();
      const scale = window.devicePixelRatio || 1;
      if (box.width > 0 && box.height > 0) onSize(Math.round(box.width * scale), Math.round(box.height * scale));
    };
    report();
    const observer = new ResizeObserver(report);
    observer.observe(element);
    return () => observer.disconnect();
  }, [onSize]);

  useEffect(() => {
    const element = canvas.current;
    const context = element?.getContext("2d");
    if (!element || !context) return;
    if (!map) {
      context.clearRect(0, 0, element.width, element.height);
      return;
    }
    const pixels = drawOutline(map.ids, map.width, map.height, selected, hover, xray);
    element.width = map.width;
    element.height = map.height;
    context.putImageData(new ImageData(pixels, map.width, map.height), 0, 0);
  }, [map, selected, hover, xray]);

  // A new selection flashes, to catch the eye.
  useEffect(() => {
    if (selected === null || window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    canvas.current?.animate([{ opacity: 1 }, { opacity: 0.2 }, { opacity: 1 }, { opacity: 0.2 }, { opacity: 1 }],
      { duration: 700, easing: "ease-out" });
  }, [selected]);

  return <canvas ref={canvas} className="selection-overlay" aria-hidden="true" />;
}
