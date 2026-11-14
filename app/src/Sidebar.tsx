import { useState } from "preact/hooks";
import { ChevronDown, KeyRound, Plus, Settings as Gear, Stethoscope } from "lucide-preact";
import type { Provider, ProfileView } from "./api";
import { api, PROVIDERS } from "./api";
import { t } from "./i18n";
import { ProfileIcon, useApp } from "./ui";

export type View = { kind: "profile"; id: string } | { kind: "diagnostics" } | { kind: "settings" } | { kind: "welcome" };

const COLLAPSE_KEY = "vl.collapsed";

function loadCollapsed(): Record<string, boolean> {
  try {
    return JSON.parse(localStorage.getItem(COLLAPSE_KEY) ?? "{}");
  } catch {
    return {};
  }
}

export function Sidebar(props: { view: View; onSelect: (v: View) => void; onNew: () => void; issues: { warn: number; err: number } }) {
  const { state, refresh, run } = useApp();
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>(loadCollapsed);
  const [drag, setDrag] = useState<{ id: string; over?: string } | null>(null);

  const toggle = (p: Provider) => {
    const next = { ...collapsed, [p]: !collapsed[p] };
    setCollapsed(next);
    try {
      localStorage.setItem(COLLAPSE_KEY, JSON.stringify(next));
    } catch {
      /* private mode: not remembered */
    }
  };

  const drop = (target: ProfileView) => {
    if (!drag || drag.id === target.id) return setDrag(null);
    const ids = state.profiles.map((p) => p.id).filter((id) => id !== drag.id);
    ids.splice(ids.indexOf(target.id), 0, drag.id);
    setDrag(null);
    run(async () => {
      await api.reorder(ids);
      await refresh();
    });
  };

  const total = props.issues.warn + props.issues.err;
  const isActive = (v: View) => JSON.stringify(v) === JSON.stringify(props.view);

  return (
    <nav class="sidebar" aria-label="VibeLauncher">
      <div class="sidebar-top drag" data-tauri-drag-region>
        <div class="brand" data-tauri-drag-region>
          <img src="/logo.svg" alt="" draggable={false} />
          <span data-tauri-drag-region>VibeLauncher</span>
        </div>
        <button class="nav-item" onClick={props.onNew} title={navigator.platform.startsWith("Mac") ? "⌘N" : "Ctrl+N"}>
          <Plus />
          <span class="label">{t("nav.new")}</span>
        </button>
        <button class={"nav-item" + (isActive({ kind: "diagnostics" }) ? " active" : "")} onClick={() => props.onSelect({ kind: "diagnostics" })}>
          <Stethoscope />
          <span class="label">{t("nav.diagnostics")}</span>
          {total > 0 && <span class={"count" + (props.issues.err ? " red" : "")}>{total}</span>}
        </button>
      </div>

      <div class="sidebar-scroll">
        {PROVIDERS.map((prov) => {
          const app = state.apps.find((a) => a.provider === prov)!;
          const rows = state.profiles.filter((p) => p.provider === prov);
          if (!app.install && !rows.some((p) => p.isolated)) return null;
          const isCollapsed = !!collapsed[prov];
          return (
            <div key={prov} role="group" aria-label={app.name}>
              <button
                class={"section-head" + (isCollapsed ? " collapsed" : "")}
                onClick={() => toggle(prov)}
                aria-expanded={!isCollapsed}
                title={isCollapsed ? t("nav.expand") : t("nav.collapse")}
              >
                <span>{app.name}</span>
                <ChevronDown class="chev" />
                {!app.install && <span class="muted">{t("nav.notInstalled")}</span>}
              </button>
              {!isCollapsed &&
                rows.map((p) => {
                  const pending = app.pending?.profile === p.id;
                  return (
                    <button
                      key={p.id}
                      class={
                        "profile-row" +
                        (isActive({ kind: "profile", id: p.id }) ? " active" : "") +
                        (drag?.id === p.id ? " dragging" : "") +
                        (drag?.over === p.id && drag.id !== p.id ? " drop-before" : "")
                      }
                      data-profile={p.id}
                      draggable
                      onDragStart={(e) => {
                        e.dataTransfer?.setData("text/plain", p.id);
                        setDrag({ id: p.id });
                      }}
                      onDragOver={(e) => {
                        const src = state.profiles.find((x) => x.id === drag?.id);
                        if (!src || src.provider !== p.provider) return;
                        e.preventDefault();
                        if (drag?.over !== p.id) setDrag({ id: drag!.id, over: p.id });
                      }}
                      onDragEnd={() => setDrag(null)}
                      onDrop={(e) => {
                        e.preventDefault();
                        drop(p);
                      }}
                      onClick={() => props.onSelect({ kind: "profile", id: p.id })}
                      onDblClick={() => run(() => api.launch(p.id).then(refresh))}
                    >
                      <ProfileIcon p={p} size={20} />
                      <span class="label">{p.name}</span>
                      {pending && <KeyRound aria-label={t("status.signingIn")} />}
                      {p.running && <span class={"dot" + (p.lostIsolation ? " amber" : "")} aria-label={t("status.running")} />}
                    </button>
                  );
                })}
            </div>
          );
        })}
      </div>

      <div class="sidebar-bottom">
        <button class={"nav-item" + (isActive({ kind: "settings" }) ? " active" : "")} onClick={() => props.onSelect({ kind: "settings" })}>
          <Gear />
          <span class="label">{t("nav.settings")}</span>
          <span class="faint" style={{ fontSize: "11px" }}>
            {state.version}
          </span>
        </button>
      </div>
    </nav>
  );
}
