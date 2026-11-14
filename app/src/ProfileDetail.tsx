import { useEffect, useState } from "preact/hooks";
import {
  AlertTriangle,
  Copy,
  FolderOpen,
  KeyRound,
  MoreHorizontal,
  Pencil,
  Play,
  Power,
  RotateCw,
  ShieldCheck,
  Trash2,
  ChevronRight,
  PackageX,
} from "lucide-preact";
import type { ProfileView } from "./api";
import { api } from "./api";
import { formatBytes, relativeTime, t } from "./i18n";
import { MenuButton, ProfileIcon, Switch, useApp } from "./ui";

export async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const ta = document.createElement("textarea");
    ta.value = text;
    document.body.appendChild(ta);
    ta.select();
    document.execCommand("copy");
    ta.remove();
  }
}

function quote(s: string) {
  return /[\s"'$`\\]/.test(s) ? `"${s.replace(/(["\\$`])/g, "\\$1")}"` : s;
}

export function ProfileDetail(props: {
  p: ProfileView;
  onEdit: () => void;
  onDelete: () => void;
  onLogin: () => void;
  onSettings: () => void;
}) {
  const { p } = props;
  const { state, refresh, run, toast } = useApp();
  const app = state.apps.find((a) => a.provider === p.provider)!;
  const [size, setSize] = useState<number | null | undefined>(undefined);
  const [advanced, setAdvanced] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);

  useEffect(() => {
    setSize(undefined);
    setAdvanced(false);
    let live = true;
    if (p.isolated) api.dataSize(p.id).then((s) => live && setSize(s), () => live && setSize(null));
    return () => {
      live = false;
    };
  }, [p.id]);

  const act = async (name: string, fn: () => Promise<unknown>, ok?: string) => {
    setBusy(name);
    await run(async () => {
      await fn();
      await refresh();
    }, ok);
    setBusy(null);
  };

  const pendingHere = app.pending?.profile === p.id;
  // A profile that was never opened needs signing in first.
  const firstRun = p.isolated && !p.lastLaunchedAt && !p.running;
  const shortcutHint = t(`detail.shortcutHint.${state.platform}` as "detail.shortcutHint.linux", { name: p.shortcutName });
  const shortcutBroken = p.shortcut && (!p.shortcutState.exists || p.shortcutState.stale);
  const envEntries = Object.entries(p.env ?? {});

  const menu = [
    { label: t("action.edit"), icon: Pencil, onClick: props.onEdit },
    { label: t("action.showData"), icon: FolderOpen, onClick: () => run(() => api.reveal(p.dataDir)), disabled: !p.isolated },
    {
      label: t("action.copyCommand"),
      icon: Copy,
      onClick: async () => {
        await copyText(`${quote(state.cli)} launch ${p.id}`);
        toast(t("copied"));
      },
    },
    { separator: true },
    { label: t("action.delete"), icon: Trash2, onClick: props.onDelete, danger: true, disabled: !p.isolated },
  ];

  return (
    <>
      <div class="topbar drag" data-tauri-drag-region>
        <div class="spacer" data-tauri-drag-region />
        <MenuButton
          align="right"
          items={menu}
          button={(open, isOpen) => (
            <button class={"icon-btn" + (isOpen ? " active" : "")} onClick={open} aria-label={t("action.more")} title={t("action.more")}>
              <MoreHorizontal />
            </button>
          )}
        />
      </div>
      <div class="content">
        <div class="page">
          <div class="hero">
            <ProfileIcon p={p} size={64} class="big-icon" />
            <div class="hero-main">
              <h1 class="selectable">{p.name}</h1>
              <div class="hero-sub">
                <span>
                  {app.name} · {p.isolated ? t("status.separate") : t("status.default")}
                </span>
                {p.running ? (
                  <span class={"pill " + (p.lostIsolation ? "amber" : "green")}>
                    <span class={"dot" + (p.lostIsolation ? " amber" : "")} />
                    {p.lostIsolation ? t("status.lost") : t("status.running")}
                  </span>
                ) : pendingHere ? (
                  <span class="pill amber">
                    <KeyRound size={12} />
                    {t("status.signingIn")}
                  </span>
                ) : (
                  <span class="pill">{t("status.stopped")}</span>
                )}
              </div>
            </div>
            <div class="actions">
              {p.running && (
                <button class="btn" disabled={!!busy} onClick={() => act("quit", () => api.quit(p.id))}>
                  <Power />
                  {t("action.quit")}
                </button>
              )}
              {p.isolated && (
                <button class={"btn" + (firstRun ? " primary" : "")} disabled={!app.install || !!busy} onClick={props.onLogin}>
                  <KeyRound />
                  {t("action.signIn")}
                </button>
              )}
              <button class={"btn" + (firstRun ? "" : " primary")} disabled={!app.install || !!busy} onClick={() => act("open", () => api.launch(p.id))}>
                <Play />
                {p.running ? t("action.focus") : t("action.open")}
              </button>
            </div>
          </div>

          {!app.install && (
            <div class="banner amber mt16">
              <PackageX />
              <div class="banner-main">
                <div class="banner-title">{t("detail.notInstalled", { app: app.name })}</div>
                <div class="banner-body">{t("detail.notInstalledBody")}</div>
                <div class="banner-actions">
                  <button class="btn sm" onClick={props.onSettings}>
                    {t("detail.openSettings")}
                  </button>
                </div>
              </div>
            </div>
          )}

          {p.lostIsolation && (
            <div class="banner amber mt16">
              <AlertTriangle />
              <div class="banner-main">
                <div class="banner-title">{t("detail.lostTitle", { name: p.name })}</div>
                <div class="banner-body">{t("detail.lostBody")}</div>
                <div class="banner-actions">
                  <button
                    class="btn sm primary"
                    disabled={!!busy}
                    onClick={() =>
                      act("restart", async () => {
                        await api.applyFix({ kind: "restart", profiles: [p.id] });
                      }, t("diag.restartStarted"))
                    }
                  >
                    <RotateCw />
                    {t("action.restart")}
                  </button>
                </div>
              </div>
            </div>
          )}

          {pendingHere && (
            <div class="banner accent mt16">
              <KeyRound />
              <div class="banner-main">
                <div class="banner-title">{t("detail.pendingTitle")}</div>
                <div class="banner-body">{t("detail.pendingBody", { app: app.name })}</div>
                <div class="banner-actions">
                  <button class="btn sm" onClick={props.onLogin}>
                    {t("detail.pasteLink")}
                  </button>
                  <button class="btn sm ghost" onClick={() => act("cancel", () => api.cancelLogin(p.provider))}>
                    {t("action.cancel")}
                  </button>
                </div>
              </div>
            </div>
          )}

          {!p.isolated ? (
            <>
              <div class="section-title mt16" style={{ marginTop: "28px" }}>
                {t("detail.profile")}
              </div>
              <div class="card">
                <div class="row">
                  <ShieldCheck class="faint" />
                  <div class="row-main">
                    <div class="row-title">{t("detail.defaultTitle", { app: app.name })}</div>
                    <div class="row-sub">{t("detail.defaultBody")}</div>
                  </div>
                </div>
                <ShortcutRow p={p} hint={shortcutHint} broken={shortcutBroken} />
                <div class="row">
                  <div class="row-main">{t("detail.lastOpened")}</div>
                  <div class="row-value">{relativeTime(p.lastLaunchedAt)}</div>
                </div>
              </div>
            </>
          ) : (
            <>
              <div class="section-title" style={{ marginTop: "28px" }}>
                {t("detail.profile")}
              </div>
              <div class="card">
                <div class="row">
                  <div class="row-main">
                    <div class="row-title">{t("detail.dataFolder")}</div>
                    <div class="path selectable">{p.dataDir}</div>
                  </div>
                  <button class="btn sm" onClick={() => run(() => api.reveal(p.dataDir))}>
                    {t("action.show")}
                  </button>
                </div>
                <div class="row">
                  <div class="row-main">{t("detail.size")}</div>
                  <div class="row-value">{size === undefined ? "…" : size === null ? "—" : formatBytes(size)}</div>
                </div>
                <ShortcutRow p={p} hint={shortcutHint} broken={shortcutBroken} />
                <div class="row">
                  <div class="row-main">{t("detail.lastOpened")}</div>
                  <div class="row-value">{relativeTime(p.lastLaunchedAt)}</div>
                </div>
              </div>

              <div class="section-title" style={{ marginTop: "28px" }}>
                <button class={"disclosure" + (advanced ? " open" : "")} type="button" onClick={() => setAdvanced(!advanced)} aria-expanded={advanced}>
                  <ChevronRight />
                  {t("detail.advanced")}
                </button>
              </div>
              {advanced && (
                <div class="card">
                  <div class="row">
                    <div class="row-main">
                      <div class="row-title">{t("detail.isolation")}</div>
                      <div class="row-sub">{t("detail.isolationBody")}</div>
                    </div>
                  </div>
                  <div class="row">
                    <div class="row-main">
                      <div class="row-title">{t("detail.env")}</div>
                      <div class="kv selectable">
                        {envEntries.length ? envEntries.map(([k, v]) => <div key={k}>{`${k}=${v}`}</div>) : <span class="faint">{t("detail.none")}</span>}
                      </div>
                    </div>
                  </div>
                  <div class="row">
                    <div class="row-main">
                      <div class="row-title">{t("detail.args")}</div>
                      <div class="kv selectable">
                        {p.extraArgs?.length ? p.extraArgs.join(" ") : <span class="faint">{t("detail.none")}</span>}
                      </div>
                    </div>
                    <button class="btn sm" onClick={props.onEdit}>
                      {t("action.edit")}
                    </button>
                  </div>
                </div>
              )}
            </>
          )}
        </div>
      </div>
    </>
  );
}

function ShortcutRow(props: { p: ProfileView; hint: string; broken: boolean }) {
  const { refresh, run } = useApp();
  const { p } = props;
  const set = (on: boolean) =>
    run(async () => {
      await api.updateProfile(p.id, {
        name: p.name,
        provider: p.provider,
        color: p.color,
        icon: p.icon,
        dataRoot: p.dataRoot ?? null,
        env: p.env ?? {},
        extraArgs: p.extraArgs ?? [],
        shortcut: on,
      });
      await refresh();
    });
  return (
    <div class="row">
      <div class="row-main">
        <div class="row-title">{t("detail.shortcut")}</div>
        <div class="row-sub">
          {props.broken ? (
            <span class="lvl-warning">
              {t("detail.shortcutBroken")}{" "}
              <a
                href="#"
                onClick={(e) => {
                  e.preventDefault();
                  run(() => api.applyFix({ kind: "syncShortcuts" }).then(refresh));
                }}
              >
                {t("detail.repair")}
              </a>
            </span>
          ) : (
            props.hint
          )}
        </div>
      </div>
      <Switch on={p.shortcut} onChange={set} label={t("detail.shortcut")} />
    </div>
  );
}
