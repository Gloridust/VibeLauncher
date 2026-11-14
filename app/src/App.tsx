import { useCallback, useEffect, useMemo, useRef, useState } from "preact/hooks";
import type { Check, Provider, RouteRequest, Settings, StateView } from "./api";
import { api, errText, on, PROVIDERS, windowCall } from "./api";
import { setLanguage, t } from "./i18n";
import { ProfileDetail } from "./ProfileDetail";
import { Diagnostics, SettingsPage, Welcome } from "./Pages";
import { DeleteSheet, LoginSheet, ProfileSheet, RouteSheet } from "./Sheets";
import { Sidebar, type View } from "./Sidebar";
import { AppCtx, type Ctx, type Toast, Toasts } from "./ui";
import { useUpdater } from "./update";

type SheetState =
  | { kind: "new"; provider?: Provider }
  | { kind: "edit"; id: string }
  | { kind: "delete"; id: string }
  | { kind: "login"; id: string }
  | null;

const POLL_MS = 2500;
const DOCTOR_MS = 30_000;

function applyTheme(theme: Settings["theme"]) {
  const dark = theme === "dark" || (theme === "auto" && matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}

export function App() {
  const [state, setState] = useState<StateView | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [view, setView] = useState<View | null>(null);
  const [sheet, setSheet] = useState<SheetState>(null);
  const [route, setRoute] = useState<RouteRequest | null>(null);
  const [checks, setChecks] = useState<Check[] | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const openedForRoute = useRef(false);
  const toastId = useRef(0);
  const shown = useRef(false);
  const update = useUpdater(state);

  const refresh = useCallback(async () => {
    try {
      const s = await api.state();
      setState(s);
      setLoadError(null);
    } catch (e) {
      setLoadError(errText(e));
    }
  }, []);

  const toast = useCallback((text: string, kind: "ok" | "error" = "ok") => {
    const id = ++toastId.current;
    setToasts((ts) => [...ts.slice(-2), { id, text, kind }]);
    setTimeout(() => setToasts((ts) => ts.filter((x) => x.id !== id)), kind === "error" ? 6000 : 2800);
  }, []);

  const run = useCallback(
    async (fn: () => Promise<unknown>, okText?: string) => {
      try {
        await fn();
        if (okText) toast(okText);
        return true;
      } catch (e) {
        toast(errText(e) || t("error.generic"), "error");
        return false;
      }
    },
    [toast],
  );

  const runDoctor = useCallback(async () => {
    try {
      setChecks(await api.doctor());
    } catch {
      /* diagnostics are best effort; the page shows "checking" */
    }
  }, []);

  const takeRoute = useCallback(async () => {
    const r = await api.takeRoute().catch(() => null);
    if (r) setRoute(r);
  }, []);

  // ── start-up ──
  useEffect(() => {
    (async () => {
      openedForRoute.current = await api.openedForRoute().catch(() => false);
      await refresh();
      await takeRoute();
      runDoctor();
    })();
    const offs = [
      on("route-requested", () => takeRoute()),
      on<string | null>("fix-done", (err) => {
        if (err) toast(err, "error");
        refresh();
        runDoctor();
      }),
    ];
    return () => offs.forEach((p) => p.then((off) => off()));
  }, []);

  // Poll only while the window is in front; refresh everything when it comes back.
  useEffect(() => {
    let timer: number | undefined;
    let doctorAt = Date.now();
    const tick = () => {
      if (document.visibilityState === "visible" && document.hasFocus()) {
        refresh();
        if (Date.now() - doctorAt > DOCTOR_MS) {
          doctorAt = Date.now();
          runDoctor();
        }
      }
    };
    const focus = async () => {
      await api.refreshApps().catch(() => {});
      await refresh();
      doctorAt = Date.now();
      runDoctor();
    };
    timer = window.setInterval(tick, POLL_MS);
    window.addEventListener("focus", focus);
    return () => {
      clearInterval(timer);
      window.removeEventListener("focus", focus);
    };
  }, []);

  // ── appearance ──
  const theme = state?.settings.theme ?? "auto";
  if (state) setLanguage(state.settings.language);
  useEffect(() => {
    applyTheme(theme);
    windowCall("setTheme", theme === "auto" ? null : theme).catch(() => {});
    if (theme !== "auto") return;
    const mq = matchMedia("(prefers-color-scheme: dark)");
    const change = () => applyTheme("auto");
    mq.addEventListener("change", change);
    return () => mq.removeEventListener("change", change);
  }, [theme]);
  useEffect(() => {
    document.documentElement.classList.toggle("mac", state?.platform === "macos");
  }, [state?.platform]);

  // Show the (initially hidden) window once there's something to show.
  useEffect(() => {
    if ((state || loadError) && !shown.current) {
      shown.current = true;
      requestAnimationFrame(() => windowCall("show").catch(() => {}));
    }
  }, [state, loadError]);

  // ── which page ──
  const visibleProfiles = useMemo(() => {
    if (!state) return [];
    return PROVIDERS.flatMap((pv) => {
      const app = state.apps.find((a) => a.provider === pv)!;
      const rows = state.profiles.filter((p) => p.provider === pv);
      return !app.install && !rows.some((p) => p.isolated) ? [] : rows;
    });
  }, [state]);

  const current: View | null = useMemo(() => {
    if (!state) return null;
    if (view?.kind === "profile" && state.profiles.some((p) => p.id === view.id)) return view;
    if (view && view.kind !== "profile") return view;
    if (!state.settings.onboarded && !openedForRoute.current) return { kind: "welcome" };
    const first = visibleProfiles.find((p) => p.running) ?? visibleProfiles[0];
    return first ? { kind: "profile", id: first.id } : { kind: "welcome" };
  }, [state, view, visibleProfiles]);

  const finishOnboarding = async () => {
    if (state && !state.settings.onboarded) {
      await api.setSettings({ ...state.settings, onboarded: true }).catch(() => {});
      await refresh();
    }
  };

  // ── keyboard ──
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (sheet || route || !state) return;
      const mod = e.metaKey || e.ctrlKey;
      if (mod && e.key.toLowerCase() === "n") {
        e.preventDefault();
        setSheet({ kind: "new" });
        return;
      }
      if (mod && e.key === ",") {
        e.preventDefault();
        setView({ kind: "settings" });
        return;
      }
      const target = e.target as HTMLElement;
      if (/^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName) || target.isContentEditable) return;
      if ((e.key === "ArrowDown" || e.key === "ArrowUp") && !target.closest(".menu")) {
        const ids = visibleProfiles.map((p) => p.id);
        if (!ids.length) return;
        const at = current?.kind === "profile" ? ids.indexOf(current.id) : -1;
        const next = e.key === "ArrowDown" ? Math.min(ids.length - 1, at + 1) : Math.max(0, at - 1);
        e.preventDefault();
        setView({ kind: "profile", id: ids[next] });
        requestAnimationFrame(() => document.querySelector<HTMLElement>(`[data-profile="${ids[next]}"]`)?.focus());
      }
    };
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, [sheet, route, state, visibleProfiles, current]);

  if (!state) {
    return loadError ? (
      <div class="welcome" style={{ marginTop: "20vh" }}>
        <h1>{t("error.generic")}</h1>
        <p class="lead selectable">{loadError}</p>
        <button class="btn" onClick={refresh}>
          {t("action.runAgain")}
        </button>
      </div>
    ) : null;
  }

  const ctx: Ctx = { state, refresh, toast, run, update };
  const issues = {
    warn: checks?.filter((c) => c.level === "warning").length ?? 0,
    err: checks?.filter((c) => c.level === "error").length ?? 0,
  };
  const byId = (id: string) => state.profiles.find((p) => p.id === id);
  const selected = current?.kind === "profile" ? byId(current.id) : undefined;

  const closeRoute = () => {
    setRoute(null);
    // Opened only to ask which profile gets a link: go away again.
    if (openedForRoute.current) {
      openedForRoute.current = false;
      windowCall("close").catch(() => {});
    }
  };

  return (
    <AppCtx.Provider value={ctx}>
      <div class="shell">
        <Sidebar view={current ?? { kind: "welcome" }} onSelect={setView} onNew={() => setSheet({ kind: "new" })} issues={issues} />
        <main class="main">
          {current?.kind === "diagnostics" ? (
            <Diagnostics checks={checks} recheck={runDoctor} onSettings={() => setView({ kind: "settings" })} />
          ) : current?.kind === "settings" ? (
            <SettingsPage
              onSettingsChanged={(s) => {
                setLanguage(s.language);
                setState({ ...state, settings: s });
              }}
            />
          ) : selected ? (
            <ProfileDetail
              key={selected.id}
              p={selected}
              onEdit={() => setSheet({ kind: "edit", id: selected.id })}
              onDelete={() => setSheet({ kind: "delete", id: selected.id })}
              onLogin={() => setSheet({ kind: "login", id: selected.id })}
              onSettings={() => setView({ kind: "settings" })}
            />
          ) : (
            <Welcome
              onCreate={(provider) => {
                finishOnboarding();
                setSheet({ kind: "new", provider });
              }}
              onSkip={async () => {
                await finishOnboarding();
                setView(null);
              }}
              onSettings={() => {
                finishOnboarding();
                setView({ kind: "settings" });
              }}
            />
          )}
        </main>
      </div>

      {sheet?.kind === "new" && (
        <ProfileSheet
          initialProvider={sheet.provider}
          onClose={() => setSheet(null)}
          onSaved={(id) => {
            setSheet(null);
            setView({ kind: "profile", id });
            runDoctor();
          }}
        />
      )}
      {sheet?.kind === "edit" && byId(sheet.id) && (
        <ProfileSheet editing={byId(sheet.id)} onClose={() => setSheet(null)} onSaved={() => setSheet(null)} />
      )}
      {sheet?.kind === "delete" && byId(sheet.id) && (
        <DeleteSheet
          p={byId(sheet.id)!}
          onClose={() => setSheet(null)}
          onDeleted={() => {
            setSheet(null);
            setView(null);
            runDoctor();
          }}
        />
      )}
      {sheet?.kind === "login" && byId(sheet.id) && <LoginSheet p={byId(sheet.id)!} onClose={() => setSheet(null)} />}
      {route && <RouteSheet req={route} onClose={closeRoute} onDone={closeRoute} />}

      <Toasts items={toasts} />
    </AppCtx.Provider>
  );
}
