/**
 * Undo and redo: the settings as they were after each change, as snapshots. A change counts
 * once it settles, so a drag or a slider is one step, not one per pixel. Moving the scene
 * camera is an edit, saved with the scene, and counts; the free view, which is only looking,
 * lives outside the settings. How the app runs is left out.
 */
import type { Settings } from "./types";

/** Settings undo does not bring back. */
const NOT_UNDONE = ["threads", "device", "evolution", "mode", "comparison"] as const;

/** Most steps kept. */
const DEPTH = 200;

export interface History {
  /** Snapshots, oldest first, and the one the settings are at. */
  stack: string[];
  index: number;
}

/** What of the settings undo keeps, as a string to compare and store. */
export function snapshot(settings: Settings): string {
  const kept: Record<string, unknown> = { ...settings };
  for (const key of NOT_UNDONE) delete kept[key];
  return JSON.stringify(kept);
}

/** The settings with a snapshot brought back, how the app runs left as it is. */
export function restore(settings: Settings, snapshot: string): Settings {
  return { ...settings, ...(JSON.parse(snapshot) as Partial<Settings>) };
}

export function start(current: string): History {
  return { stack: [current], index: 0 };
}

/** A new step, unless nothing changed; steps undone before it are dropped. */
export function record(history: History, current: string): History {
  if (history.stack[history.index] === current) return history;
  const stack = [...history.stack.slice(0, history.index + 1), current].slice(-DEPTH);
  return { stack, index: stack.length - 1 };
}

/** One step back: the snapshot to bring back, if any. A change not yet recorded counts first. */
export function undo(history: History, current: string): { history: History; to: string | null } {
  const settled = record(history, current);
  if (settled.index === 0) return { history: settled, to: null };
  const back = { ...settled, index: settled.index - 1 };
  return { history: back, to: back.stack[back.index] };
}

/** One step forward again, if the last thing done was undoing. */
export function redo(history: History, current: string): { history: History; to: string | null } {
  // A change since the undo starts a new branch, and there is nothing to redo.
  if (history.stack[history.index] !== current || history.index >= history.stack.length - 1) {
    return { history: record(history, current), to: null };
  }
  const forward = { ...history, index: history.index + 1 };
  return { history: forward, to: forward.stack[forward.index] };
}
