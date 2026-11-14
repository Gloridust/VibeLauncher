// Checking for a new version of VibeLauncher and installing it.
//
// The window checks once when it opens (unless turned off in Settings); a
// failed automatic check stays quiet, a failed manual one says why.

import { useCallback, useEffect, useRef, useState } from "preact/hooks";
import type { StateView, UpdateInfo } from "./api";
import { api, errText, on, UPDATE_CANCELLED } from "./api";

export type UpdatePhase = "idle" | "checking" | "current" | "available" | "downloading" | "installing" | "error";

export interface Updater {
  phase: UpdatePhase;
  info: UpdateInfo | null;
  /** 0–1 while downloading, when the size is known. */
  progress: number | null;
  error: string | null;
  check: () => Promise<void>;
  install: () => Promise<void>;
}

export function useUpdater(state: StateView | null): Updater {
  const [phase, setPhase] = useState<UpdatePhase>("idle");
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [progress, setProgress] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const autoChecked = useRef(false);

  const run = useCallback(async (manual: boolean) => {
    setPhase("checking");
    setError(null);
    try {
      const found = await api.checkUpdate();
      setInfo(found);
      setPhase(found ? "available" : "current");
    } catch (e) {
      setInfo(null);
      if (manual) {
        setError(errText(e));
        setPhase("error");
      } else setPhase("idle");
    }
  }, []);

  useEffect(() => {
    if (!state || autoChecked.current) return;
    autoChecked.current = true;
    if (state.settings.checkUpdates) run(false);
  }, [state]);

  useEffect(() => {
    const offs = [
      on<{ downloaded: number; total?: number | null }>("update-progress", (p) => setProgress(p.total ? Math.min(1, p.downloaded / p.total) : null)),
      on("update-installing", () => setPhase("installing")),
    ];
    return () => offs.forEach((p) => p.then((off) => off()));
  }, []);

  const install = useCallback(async () => {
    setPhase("downloading");
    setProgress(null);
    setError(null);
    try {
      // On success the app restarts (or, on Windows, the installer takes over).
      await api.installUpdate();
      setPhase("installing");
    } catch (e) {
      const msg = errText(e);
      if (msg === UPDATE_CANCELLED) setPhase("available");
      else {
        setError(msg);
        setPhase("error");
      }
    }
  }, []);

  return { phase, info, progress, error, check: () => run(true), install };
}
