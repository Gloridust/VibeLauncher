import { useEffect, useState } from "preact/hooks";
import {
  AlertCircle,
  AlertTriangle,
  Check as CheckIcon,
  CheckCircle2,
  Info,
  Loader2,
  Minus,
  Package,
  Plus,
  RefreshCw,
  Wrench,
} from "lucide-preact";
import type { AppView, Check, Fix, Provider, Settings } from "./api";
import { api, pickFile, PROVIDERS } from "./api";
import { checkText, t } from "./i18n";
import { Segmented, Switch, useApp } from "./ui";

function Topbar(props: { title: string; children?: preact.ComponentChildren }) {
  return (
    <div class="topbar drag" data-tauri-drag-region>
      <div class="title" data-tauri-drag-region>
        {props.title}
      </div>
      <div class="spacer" data-tauri-drag-region />
      {props.children}
    </div>
  );
}

function AppIcon(props: { app: AppView }) {
  return props.app.iconUrl ? (
    <img class="app-icon" src={props.app.iconUrl} alt="" draggable={false} />
  ) : (
    <span class="app-icon placeholder">
      <Package />
    </span>
  );
}

// ── diagnostics ──

const ORDER: Record<string, number> = { error: 0, warning: 1, info: 2, ok: 3 };

function LevelIcon(props: { level: Check["level"] }) {
  switch (props.level) {
    case "error":
      return <AlertCircle class="lvl-error" />;
    case "warning":
      return <AlertTriangle class="lvl-warning" />;
    case "info":
      return <Info class="lvl-info" />;
    default:
      return <CheckCircle2 class="lvl-ok" />;
  }
}

export function Diagnostics(props: { checks: Check[] | null; recheck: () => Promise<void>; onSettings: () => void }) {
  const { refresh, run, toast } = useApp();
  const [log, setLog] = useState<string[] | null>(null);
  const [fixing, setFixing] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);

  useEffect(() => {
    api.routeLog().then(setLog, () => setLog([]));
  }, [props.checks]);

  const recheck = async () => {
    setChecking(true);
    await props.recheck();
    setChecking(false);
  };

  const fix = async (c: Check, f: Fix) => {
    if (f.kind === "chooseApp") return props.onSettings();
    setFixing(c.id);
    const ok = await run(async () => {
      await api.applyFix(f);
      await refresh();
    });
    if (ok && f.kind === "restart") toast(t("diag.restartStarted"));
    else if (ok) {
      toast(t("diag.fixed"));
      await props.recheck();
    }
    setFixing(null);
  };

  const checks = props.checks ? [...props.checks].sort((a, b) => ORDER[a.level] - ORDER[b.level]) : null;
  const issues = checks?.filter((c) => c.level === "warning" || c.level === "error") ?? [];
  const hasErr = issues.some((c) => c.level === "error");

  return (
    <>
      <Topbar title={t("diag.title")}>
        <button class="btn sm" onClick={recheck} disabled={checking || !checks}>
          <RefreshCw class={checking ? "spin" : ""} />
          {t("action.runAgain")}
        </button>
      </Topbar>
      <div class="content">
        <div class="page">
          <p class="section-note" style={{ margin: "0 2px 16px" }}>
            {t("diag.subtitle")}
          </p>
          <div class="card summary">
            {!checks ? (
              <>
                <span class="badge-icon ok">
                  <Loader2 class="spin" />
                </span>
                <div class="s-title">{t("diag.checking")}</div>
              </>
            ) : (
              <>
                <span class={"badge-icon " + (issues.length ? (hasErr ? "err" : "warn") : "ok")}>
                  {issues.length ? hasErr ? <AlertCircle /> : <AlertTriangle /> : <CheckIcon />}
                </span>
                <div>
                  <div class="s-title">
                    {issues.length === 0 ? t("diag.allGood") : issues.length === 1 ? t("diag.issue") : t("diag.issues", { n: issues.length })}
                  </div>
                </div>
              </>
            )}
          </div>

          {checks && checks.length > 0 && (
            <div class="card">
              {checks.map((c) => {
                const { title, detail } = checkText(c);
                return (
                  <div class="row" key={c.id}>
                    <LevelIcon level={c.level} />
                    <div class="row-main">
                      <div class="row-title">{title}</div>
                      {detail && <div class="row-sub selectable">{detail}</div>}
                    </div>
                    {c.fix && c.level !== "ok" && (
                      <button class={"btn sm" + (c.level === "error" ? " primary" : "")} disabled={!!fixing} onClick={() => fix(c, c.fix!)}>
                        {fixing === c.id ? <Loader2 class="spin" /> : <Wrench />}
                        {fixing === c.id ? t("diag.fixing") : c.fix.kind === "chooseApp" ? t("action.choose") : t("diag.fix")}
                      </button>
                    )}
                  </div>
                );
              })}
            </div>
          )}

          <div class="section-title">{t("diag.log")}</div>
          <p class="section-note">{t("diag.logHint")}</p>
          <div class="card">
            {log === null ? (
              <div class="log">…</div>
            ) : log.length === 0 ? (
              <div class="row">
                <div class="row-main faint">{t("diag.logEmpty")}</div>
              </div>
            ) : (
              <div class="log selectable">{[...log].reverse().join("\n")}</div>
            )}
          </div>
        </div>
      </div>
    </>
  );
}

// ── settings ──

const SCHEME: Record<Provider, string> = { claude: "claude", chatgpt: "codex" };
const ROUTE_KEY: Record<Provider, "routeClaude" | "routeChatgpt"> = { claude: "routeClaude", chatgpt: "routeChatgpt" };

export function SettingsPage(props: { onSettingsChanged: (s: Settings) => void }) {
  const { state, refresh, run, toast } = useApp();
  const s = state.settings;
  const [busy, setBusy] = useState<string | null>(null);

  const save = async (patch: Partial<Settings>) => {
    const next = { ...s, ...patch };
    props.onSettingsChanged(next);
    await run(async () => {
      const problems = await api.setSettings(next);
      for (const p of problems) toast(p, "error");
      await refresh();
    });
  };

  const chooseApp = async (app: AppView) => {
    const path = await pickFile({ directory: false, title: t("settings.chooseApp", { app: app.name }) });
    if (!path) return;
    setBusy(app.provider);
    await run(async () => {
      await api.setApp(app.provider, path);
      await refresh();
    });
    setBusy(null);
  };

  const lookAgain = async () => {
    setBusy("refresh");
    await run(async () => {
      await api.refreshApps();
      await refresh();
    });
    setBusy(null);
  };

  return (
    <>
      <Topbar title={t("settings.title")} />
      <div class="content">
        <div class="page">
          <div class="section-title">{t("settings.general")}</div>
          <div class="card">
            <div class="row">
              <div class="row-main">{t("settings.language")}</div>
              <Segmented
                label={t("settings.language")}
                value={s.language}
                onChange={(language) => save({ language })}
                options={[
                  { value: "auto", label: t("settings.auto") },
                  { value: "en", label: "English" },
                  { value: "zh", label: "简体中文" },
                ]}
              />
            </div>
            <div class="row">
              <div class="row-main">{t("settings.theme")}</div>
              <Segmented
                label={t("settings.theme")}
                value={s.theme}
                onChange={(theme) => save({ theme })}
                options={[
                  { value: "auto", label: t("settings.auto") },
                  { value: "light", label: t("settings.light") },
                  { value: "dark", label: t("settings.dark") },
                ]}
              />
            </div>
          </div>

          <div class="section-title" style={{ display: "flex", alignItems: "center" }}>
            <span style={{ flex: 1 }}>{t("settings.apps")}</span>
            <button class="btn sm ghost" onClick={lookAgain} disabled={busy === "refresh"}>
              <RefreshCw class={busy === "refresh" ? "spin" : ""} />
              {t("action.findAgain")}
            </button>
          </div>
          <div class="card">
            {state.apps.map((app) => {
              const manual = !!s.appOverrides[app.provider];
              const inst = app.install;
              return (
                <div class="row" key={app.provider}>
                  <AppIcon app={app} />
                  <div class="row-main">
                    <div class="row-title">
                      {app.name}
                      {inst?.version && <span class="faint"> {inst.version}</span>}
                      {manual && <span class="pill" style={{ marginLeft: "8px" }}>{t("settings.manual")}</span>}
                    </div>
                    {inst ? (
                      <div class="path selectable">{inst.bundle ?? inst.program}</div>
                    ) : (
                      <div class="row-sub lvl-warning">{t("settings.appMissing")}</div>
                    )}
                  </div>
                  {manual && (
                    <button class="btn sm ghost" disabled={busy === app.provider} onClick={() => run(() => api.setApp(app.provider, null).then(refresh))}>
                      {t("action.reset")}
                    </button>
                  )}
                  <button class="btn sm" disabled={busy === app.provider} onClick={() => chooseApp(app)}>
                    {busy === app.provider && <Loader2 class="spin" />}
                    {inst ? t("action.change") : t("action.choose")}
                  </button>
                </div>
              );
            })}
          </div>

          <div class="section-title">{t("settings.links")}</div>
          <p class="section-note">{t("settings.linksBody")}</p>
          <div class="card">
            {PROVIDERS.map((pv) => {
              const app = state.apps.find((a) => a.provider === pv)!;
              const key = ROUTE_KEY[pv];
              const r = app.routing;
              const status = !r.supported
                ? t("settings.routeUnsupported")
                : !s[key]
                  ? t("settings.routeOff")
                  : r.claimed
                    ? t("settings.routeOn")
                    : app.routingWanted
                      ? t("settings.routeOff")
                      : t("settings.routeIdle");
              return (
                <div class="row" key={pv}>
                  <div class="row-main">
                    <div class="row-title">
                      {t("settings.route", { scheme: SCHEME[pv] })}
                      {r.experimental && r.supported && <span class="pill" style={{ marginLeft: "8px" }}>{t("settings.experimental")}</span>}
                    </div>
                    <div class={"row-sub" + (r.conflict ? " lvl-warning" : "")}>{r.conflict ?? status}</div>
                  </div>
                  <Switch on={s[key] && r.supported} disabled={!r.supported} onChange={(v) => save({ [key]: v })} label={t("settings.route", { scheme: SCHEME[pv] })} />
                </div>
              );
            })}
          </div>

          <div class="section-title">{t("settings.restarts")}</div>
          <div class="card">
            <div class="row">
              <div class="row-main">
                <div class="row-title">{t("settings.stagger")}</div>
                <div class="row-sub">{t("settings.staggerHint")}</div>
              </div>
              <div class="stepper" role="group" aria-label={t("settings.stagger")}>
                <button class="icon-btn" disabled={s.staggerSeconds <= 0} onClick={() => save({ staggerSeconds: Math.max(0, s.staggerSeconds - 1) })} aria-label="−">
                  <Minus />
                </button>
                <span class="stepper-value">{t("settings.seconds", { n: s.staggerSeconds })}</span>
                <button class="icon-btn" disabled={s.staggerSeconds >= 30} onClick={() => save({ staggerSeconds: Math.min(30, s.staggerSeconds + 1) })} aria-label="+">
                  <Plus />
                </button>
              </div>
            </div>
          </div>

          <div class="section-title">{t("settings.about")}</div>
          <div class="card">
            <div class="row">
              <img class="app-icon" src="/logo.svg" alt="" />
              <div class="row-main">
                <div class="row-title">
                  VibeLauncher <span class="faint">{state.version}</span>
                </div>
                <div class="row-sub">{t("settings.aboutBody")}</div>
                <div class="row-sub faint" style={{ marginTop: "4px" }}>
                  {t("settings.notAffiliated")}
                </div>
              </div>
            </div>
            <div class="row">
              <div class="row-main">{t("settings.website")}</div>
              <button class="btn sm" onClick={() => run(() => api.openWebsite("home"))}>
                GitHub
              </button>
              <button class="btn sm" onClick={() => run(() => api.openWebsite("issues"))}>
                {t("settings.issues")}
              </button>
            </div>
            <div class="row">
              <div class="row-main">
                <div class="row-title">{t("settings.command")}</div>
                <div class="path selectable">{state.cli}</div>
              </div>
            </div>
            <div class="row">
              <div class="row-main">
                <div class="row-title">{t("settings.config")}</div>
                <div class="path selectable">{state.configFile}</div>
              </div>
              <button class="btn sm" onClick={() => run(() => api.reveal(state.configFile))}>
                {t("action.show")}
              </button>
            </div>
          </div>
        </div>
      </div>
    </>
  );
}

// ── welcome / nothing installed ──

export function Welcome(props: { onCreate: (p?: Provider) => void; onSkip: () => void; onSettings: () => void }) {
  const { state, refresh, run } = useApp();
  const [looking, setLooking] = useState(false);
  const anyInstalled = state.apps.some((a) => a.install);
  return (
    <>
      <div class="topbar drag" data-tauri-drag-region>
        <div class="spacer" data-tauri-drag-region />
      </div>
      <div class="content">
        <div class="welcome">
          <img class="logo" src="/logo.svg" alt="" draggable={false} />
          <h1>{anyInstalled ? t("onb.title") : t("empty.title")}</h1>
          <p class="lead">{anyInstalled ? t("onb.body") : t("empty.body")}</p>
          <div class="card">
            {state.apps.map((app) => (
              <div class="row" key={app.provider}>
                <AppIcon app={app} />
                <div class="row-main">
                  <div class="row-title">{app.name}</div>
                  <div class={"row-sub" + (app.install ? "" : " faint")}>
                    {app.install ? `${t("onb.found")}${app.install.version ? ` · ${app.install.version}` : ""}` : t("onb.missing")}
                  </div>
                </div>
                {app.install ? <CheckCircle2 class="lvl-ok" /> : <Minus class="faint" />}
              </div>
            ))}
          </div>
          {anyInstalled ? (
            <ul class="points">
              {[t("onb.p1"), t("onb.p2"), t("onb.p3")].map((p, i) => (
                <li key={i}>
                  <CheckIcon />
                  <span>{p}</span>
                </li>
              ))}
            </ul>
          ) : (
            <div style={{ height: "24px" }} />
          )}
          <div class="buttons">
            {anyInstalled ? (
              <>
                <button class="btn lg" onClick={props.onSkip}>
                  {t("onb.skip")}
                </button>
                <button class="btn lg primary" onClick={() => props.onCreate()}>
                  <Plus />
                  {t("onb.start")}
                </button>
              </>
            ) : (
              <>
                <button class="btn lg" onClick={props.onSettings}>
                  {t("detail.openSettings")}
                </button>
                <button
                  class="btn lg primary"
                  disabled={looking}
                  onClick={async () => {
                    setLooking(true);
                    await run(async () => {
                      await api.refreshApps();
                      await refresh();
                    });
                    setLooking(false);
                  }}
                >
                  <RefreshCw class={looking ? "spin" : ""} />
                  {t("action.findAgain")}
                </button>
              </>
            )}
          </div>
        </div>
      </div>
    </>
  );
}

