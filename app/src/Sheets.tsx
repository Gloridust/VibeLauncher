import { useEffect, useRef, useState } from "preact/hooks";
import { CheckCircle2, ChevronRight, Clipboard, Loader2, AlertTriangle } from "lucide-preact";
import type { Provider, ProfileInput, ProfileView, RouteRequest } from "./api";
import { api, errText, pickFile, PROVIDERS } from "./api";
import { relativeTime, t } from "./i18n";
import { ColorPicker, COLORS, ProfileIcon, Segmented, Sheet, Switch, useApp } from "./ui";

// ── create / edit ──

function parseEnv(text: string): { env: Record<string, string>; bad?: number } {
  const env: Record<string, string> = {};
  const lines = text.split("\n");
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i].trim();
    if (!line || line.startsWith("#")) continue;
    const eq = line.indexOf("=");
    if (eq <= 0 || !/^[A-Za-z_][A-Za-z0-9_]*$/.test(line.slice(0, eq).trim())) return { env, bad: i + 1 };
    env[line.slice(0, eq).trim()] = line.slice(eq + 1);
  }
  return { env };
}

function slug(name: string) {
  return (
    name
      .toLowerCase()
      .normalize("NFKD")
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "") || "profile"
  );
}

export function ProfileSheet(props: { editing?: ProfileView; initialProvider?: Provider; onClose: () => void; onSaved: (id: string) => void }) {
  const { state, refresh, toast } = useApp();
  const ed = props.editing;
  const installed = PROVIDERS.filter((p) => state.apps.find((a) => a.provider === p)?.install);
  const [name, setName] = useState(ed?.name ?? "");
  const [provider, setProvider] = useState<Provider>(ed?.provider ?? props.initialProvider ?? installed[0] ?? "claude");
  const used = new Set(state.profiles.map((p) => p.color.toLowerCase()));
  const [color, setColor] = useState(ed?.color ?? COLORS.slice(1).find((c) => !used.has(c.toLowerCase())) ?? COLORS[5]);
  const [shortcut, setShortcut] = useState(ed?.shortcut ?? true);
  const [advanced, setAdvanced] = useState(!!(ed && (ed.dataRoot || Object.keys(ed.env ?? {}).length || ed.extraArgs?.length)));
  const [dataRoot, setDataRoot] = useState(ed?.dataRoot ?? "");
  const [envText, setEnvText] = useState(Object.entries(ed?.env ?? {}).map(([k, v]) => `${k}=${v}`).join("\n"));
  const [argsText, setArgsText] = useState((ed?.extraArgs ?? []).join("\n"));
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const isDefault = ed && !ed.isolated;
  const envParsed = parseEnv(envText);
  const sep = state.platform === "windows" ? "\\" : "/";
  const defaultPath = `${state.profilesDir}${sep}${ed?.id ?? slug(name)}`;

  const save = async () => {
    if (!name.trim()) return setError(t("sheet.namePlaceholder"));
    if (envParsed.bad) return setError(t("sheet.badEnv", { n: envParsed.bad }));
    setSaving(true);
    setError(null);
    const input: ProfileInput = {
      name: name.trim(),
      provider,
      color,
      icon: ed?.icon,
      dataRoot: dataRoot.trim() || null,
      env: envParsed.env,
      extraArgs: argsText.split("\n").map((s) => s.trim()).filter(Boolean),
      shortcut,
    };
    try {
      const p = ed ? await api.updateProfile(ed.id, input) : await api.createProfile(input);
      await refresh();
      toast(ed ? t("sheet.saved") : t("sheet.created", { name: p.name }));
      props.onSaved(p.id);
    } catch (e) {
      setError(errText(e));
      setSaving(false);
    }
  };

  return (
    <Sheet
      title={ed ? t("sheet.editTitle") : t("sheet.newTitle")}
      onClose={props.onClose}
      onSubmit={save}
      footer={
        <>
          <button type="button" class="btn" onClick={props.onClose}>
            {t("action.cancel")}
          </button>
          <button type="submit" class="btn primary" disabled={saving || !name.trim()}>
            {saving && <Loader2 class="spin" />}
            {ed ? t("action.save") : t("action.create")}
          </button>
        </>
      }
    >
      <div class="form-row">
        <label for="pf-name">{t("sheet.name")}</label>
        <input
          id="pf-name"
          class="field"
          autofocus
          maxLength={60}
          placeholder={t("sheet.namePlaceholder")}
          value={name}
          onInput={(e) => setName((e.target as HTMLInputElement).value)}
        />
      </div>
      <div class="form-row">
        <span class="form-label">{t("sheet.app")}</span>
        <Segmented
          value={provider}
          onChange={setProvider}
          options={PROVIDERS.map((pv) => {
            const a = state.apps.find((x) => x.provider === pv)!;
            return {
              value: pv,
              disabled: !!ed || !a.install,
              label: (
                <>
                  {a.iconUrl && <img src={a.iconUrl} alt="" />}
                  {a.name}
                </>
              ),
            };
          })}
        />
      </div>
      <div class="form-row">
        <span class="form-label">{t("sheet.color")}</span>
        <ColorPicker value={color} onChange={setColor} />
      </div>
      <div class="inline">
        <div class="grow">
          <div style={{ fontWeight: 500 }}>{t("sheet.shortcut")}</div>
          <div class="hint" style={{ marginTop: "1px" }}>
            {t(`detail.shortcutHint.${state.platform}` as "detail.shortcutHint.linux", {
              name: isDefault ? `${ed!.name} (VibeLauncher)` : `${state.apps.find((a) => a.provider === provider)!.name} · ${name.trim() || "…"}`,
            })}
          </div>
        </div>
        <Switch on={shortcut} onChange={setShortcut} label={t("sheet.shortcut")} />
      </div>

      {isDefault ? (
        <div class="hint">{t("sheet.defaultNote")}</div>
      ) : (
        <div>
          <button type="button" class={"disclosure" + (advanced ? " open" : "")} onClick={() => setAdvanced(!advanced)} aria-expanded={advanced}>
            <ChevronRight />
            {t("sheet.advanced")}
          </button>
          {advanced && (
            <div style={{ display: "flex", flexDirection: "column", gap: "14px", marginTop: "12px" }}>
              <div class="form-row">
                <label for="pf-data">{t("sheet.dataFolder")}</label>
                <div class="inline">
                  <input
                    id="pf-data"
                    class="field mono grow"
                    placeholder={defaultPath}
                    value={dataRoot}
                    onInput={(e) => setDataRoot((e.target as HTMLInputElement).value)}
                  />
                  <button
                    type="button"
                    class="btn"
                    onClick={async () => {
                      const d = await pickFile({ directory: true, title: t("sheet.dataFolder") });
                      if (d) setDataRoot(d);
                    }}
                  >
                    {t("action.choose")}
                  </button>
                </div>
                {!dataRoot && <div class="hint">{t("sheet.dataDefault", { path: defaultPath })}</div>}
              </div>
              <div class="form-row">
                <label for="pf-env">{t("sheet.env")}</label>
                <textarea id="pf-env" class="field" rows={3} spellcheck={false} placeholder="HTTPS_PROXY=http://127.0.0.1:7890" value={envText} onInput={(e) => setEnvText((e.target as HTMLTextAreaElement).value)} />
                <div class="hint">{t("sheet.envHint")}</div>
              </div>
              <div class="form-row">
                <label for="pf-args">{t("sheet.args")}</label>
                <textarea id="pf-args" class="field" rows={2} spellcheck={false} placeholder="--lang=en-US" value={argsText} onInput={(e) => setArgsText((e.target as HTMLTextAreaElement).value)} />
                <div class="hint">{t("sheet.argsHint")}</div>
              </div>
            </div>
          )}
        </div>
      )}
      {error && <div class="error-text">{error}</div>}
    </Sheet>
  );
}

// ── delete ──

export function DeleteSheet(props: { p: ProfileView; onClose: () => void; onDeleted: () => void }) {
  const { refresh, toast } = useApp();
  const [withData, setWithData] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const go = async () => {
    setBusy(true);
    try {
      await api.deleteProfile(props.p.id, withData);
      await refresh();
      toast(t("delete.done", { name: props.p.name }));
      props.onDeleted();
    } catch (e) {
      setError(errText(e));
      setBusy(false);
    }
  };
  return (
    <Sheet
      title={t("delete.title", { name: props.p.name })}
      subtitle={t("delete.body")}
      onClose={props.onClose}
      onSubmit={go}
      footer={
        <>
          <button type="button" class="btn" onClick={props.onClose} autofocus>
            {t("action.cancel")}
          </button>
          <button type="submit" class="btn danger" disabled={busy}>
            {busy && <Loader2 class="spin" />}
            {t("delete.confirm")}
          </button>
        </>
      }
    >
      <label class="check">
        <input type="checkbox" checked={withData} onChange={(e) => setWithData((e.target as HTMLInputElement).checked)} />
        <span>
          <div style={{ fontWeight: 500 }}>{t("delete.data")}</div>
          <div class="hint" style={{ marginTop: "2px" }}>
            {t("delete.dataHint", { path: props.p.dataDir })}
          </div>
        </span>
      </label>
      {error && <div class="error-text">{error}</div>}
    </Sheet>
  );
}

// ── sign in ──

function Ring(props: { left: number; total: number }) {
  const r = 9;
  const c = 2 * Math.PI * r;
  return (
    <svg class="ring" viewBox="0 0 22 22" aria-hidden="true">
      <circle class="track" cx="11" cy="11" r={r} />
      <circle class="bar" cx="11" cy="11" r={r} stroke-dasharray={c} stroke-dashoffset={c * (1 - props.left / props.total)} transform="rotate(-90 11 11)" />
    </svg>
  );
}

const mmss = (s: number) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;

export function LoginSheet(props: { p: ProfileView; onClose: () => void }) {
  const { state, refresh } = useApp();
  const { p } = props;
  const app = state.apps.find((a) => a.provider === p.provider)!;
  const scheme = p.provider === "claude" ? "claude" : "codex";
  const [phase, setPhase] = useState<"starting" | "waiting" | "done" | "expired" | "error">("starting");
  const [left, setLeft] = useState(300);
  const [link, setLink] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const started = useRef(false);
  const deadline = useRef(0);

  const start = async () => {
    setPhase("starting");
    setError(null);
    try {
      await api.login(p.id);
      deadline.current = Date.now() + 300_000;
      setLeft(300);
      setPhase("waiting");
      await refresh();
    } catch (e) {
      setError(errText(e));
      setPhase("error");
    }
  };

  useEffect(() => {
    // Opened from the "waiting" banner: don't start a second sign-in.
    if (started.current) return;
    started.current = true;
    if (app.pending?.profile === p.id) {
      deadline.current = Date.now() + app.pending.secondsLeft * 1000;
      setLeft(app.pending.secondsLeft);
      setPhase("waiting");
    } else start();
  }, []);

  // Countdown, and notice when the marker was used (the link arrived).
  useEffect(() => {
    if (phase !== "waiting") return;
    const tick = setInterval(() => {
      const s = Math.max(0, Math.round((deadline.current - Date.now()) / 1000));
      setLeft(s);
      if (s === 0) setPhase("expired");
    }, 1000);
    return () => clearInterval(tick);
  }, [phase]);
  useEffect(() => {
    if (phase === "waiting" && deadline.current - Date.now() < 295_000 && app.pending?.profile !== p.id) setPhase("done");
  }, [app.pending, phase]);

  const sendLink = async () => {
    setSending(true);
    setError(null);
    try {
      await api.openLink(p.id, link);
      await refresh();
      setPhase("done");
    } catch (e) {
      setError(errText(e));
    }
    setSending(false);
  };

  const routed = app.routing.claimed && !app.routing.stale;

  return (
    <Sheet
      title={t("login.title", { name: p.name })}
      onClose={props.onClose}
      onSubmit={link.trim() ? sendLink : undefined}
      footer={
        <>
          {phase === "waiting" && (
            <button
              type="button"
              class="btn ghost left"
              onClick={async () => {
                await api.cancelLogin(p.provider);
                await refresh();
                props.onClose();
              }}
            >
              {t("action.cancel")}
            </button>
          )}
          {(phase === "expired" || phase === "error") && (
            <button type="button" class="btn left" onClick={start}>
              {t("login.again")}
            </button>
          )}
          <button type="button" class={"btn" + (phase === "done" ? " primary" : "")} onClick={props.onClose} autofocus>
            {phase === "done" ? t("action.done") : t("action.close")}
          </button>
        </>
      }
    >
      <ol class="steps">
        {[t("login.step1", { app: app.name }), t("login.step2", { app: app.name }), t("login.step3", { app: app.name })].map((s, i) => (
          <li key={i}>
            <span class="num">{i + 1}</span>
            <span>{s}</span>
          </li>
        ))}
      </ol>

      {phase === "done" ? (
        <div class="waiting done">
          <CheckCircle2 size={20} />
          {t("login.received")}
        </div>
      ) : phase === "expired" ? (
        <div class="waiting">
          <AlertTriangle size={18} class="lvl-warning" />
          <span class="muted">{t("login.expired")}</span>
        </div>
      ) : phase === "waiting" ? (
        <div class="waiting">
          <Ring left={left} total={300} />
          {t("login.waiting", { time: mmss(left) })}
        </div>
      ) : phase === "starting" ? (
        <div class="waiting">
          <Loader2 class="spin" size={18} />
          {t("action.open")}…
        </div>
      ) : null}

      {!routed && phase !== "done" && <div class="hint" style={{ marginTop: 0 }}>{t("login.routingOff", { app: app.name })}</div>}

      {phase !== "done" && (
        <div class="form-row">
          <label for="paste-link">{t("login.pasteTitle")}</label>
          <div class="hint" style={{ margin: "-2px 0 8px" }}>
            {t("login.pasteBody", { app: app.name })}
          </div>
          <div class="inline">
            <input
              id="paste-link"
              class="field mono grow"
              spellcheck={false}
              autocomplete="off"
              placeholder={t("login.pastePlaceholder", { scheme })}
              value={link}
              onInput={(e) => setLink((e.target as HTMLInputElement).value)}
            />
            {!link && navigator.clipboard?.readText && (
              <button
                type="button"
                class="btn"
                title={t("detail.pasteLink")}
                onClick={async () => {
                  try {
                    setLink((await navigator.clipboard.readText()).trim());
                  } catch {
                    /* clipboard not readable here: type or paste by hand */
                  }
                }}
              >
                <Clipboard />
              </button>
            )}
            {link && (
              <button type="submit" class="btn primary" disabled={sending}>
                {sending && <Loader2 class="spin" />}
                {t("login.pasteOpen")}
              </button>
            )}
          </div>
        </div>
      )}
      {error && <div class="error-text">{error}</div>}
    </Sheet>
  );
}

// ── which profile gets this link? ──

export function RouteSheet(props: { req: RouteRequest; onClose: () => void; onDone: () => void }) {
  const { state, refresh, toast } = useApp();
  const app = state.apps.find((a) => a.provider === props.req.provider)!;
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const candidates = props.req.candidates
    .map((id) => state.profiles.find((p) => p.id === id))
    .filter((p): p is ProfileView => !!p)
    .sort((a, b) => Number(b.running) - Number(a.running));
  const pick = async (p: ProfileView) => {
    setBusy(p.id);
    try {
      await api.deliverChoice(p.id, props.req.url);
      await refresh();
      toast(t("route.delivered", { name: p.name }));
      props.onDone();
    } catch (e) {
      setError(errText(e));
      setBusy(null);
    }
  };
  return (
    <Sheet
      title={props.req.isLogin ? t("route.loginTitle") : t("route.title")}
      subtitle={t("route.body", { app: app.name, link: props.req.redacted })}
      onClose={props.onClose}
      footer={
        <button type="button" class="btn" onClick={props.onClose}>
          {t("action.cancel")}
        </button>
      }
    >
      <div class="choices">
        {candidates.map((p) => (
          <button type="button" class="choice" key={p.id} disabled={!!busy} onClick={() => pick(p)}>
            <ProfileIcon p={p} size={32} />
            <div class="grow">
              <div style={{ fontWeight: 500 }}>{p.name}</div>
              <div class="sub">
                {p.running ? (
                  <>
                    <span class="dot" />
                    {t("status.running")}
                  </>
                ) : (
                  <>
                    {t("detail.lastOpened")} · {relativeTime(p.lastLaunchedAt)}
                  </>
                )}
              </div>
            </div>
            {busy === p.id ? <Loader2 class="spin" /> : <ChevronRight />}
          </button>
        ))}
      </div>
      {error && <div class="error-text">{error}</div>}
    </Sheet>
  );
}
