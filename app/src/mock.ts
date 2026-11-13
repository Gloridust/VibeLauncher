// Development-only stand-in for the Rust side (loaded only by `vite dev`
// outside Tauri). Pick a scenario with ?scenario=fresh|busy|noapps|route.

import type { AppView, Check, ProfileInput, ProfileView, RouteRequest, Settings, StateView } from "./api";

const scenario = new URLSearchParams(location.search).get("scenario") ?? "busy";

function svgIcon(bg: string, badge?: string): string {
  const b = badge
    ? `<circle cx="73" cy="73" r="20" fill="#fff"/><circle cx="73" cy="73" r="16" fill="${badge}"/>`
    : "";
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 96 96"><rect x="6" y="6" width="84" height="84" rx="20" fill="${bg}"/><path d="M48 24l5 17 17 5-17 5-5 17-5-17-17-5 17-5z" fill="#fff" opacity=".92"/>${b}</svg>`;
  return "data:image/svg+xml;base64," + btoa(svg);
}

const brand = { claude: "#D97757", chatgpt: "#10A37F" } as const;
const now = Date.now();
const iso = (minAgo: number) => new Date(now - minAgo * 60_000).toISOString();

function view(p: Partial<ProfileView> & Pick<ProfileView, "id" | "name" | "provider" | "color">): ProfileView {
  const isolated = p.isolated ?? true;
  return {
    icon: "sparkle",
    isolated,
    shortcut: false,
    createdAt: iso(60 * 24 * 9),
    env: {},
    extraArgs: [],
    dataDir: isolated ? `/home/me/.local/share/vibelauncher/profiles/${p.id}` : "",
    running: false,
    lostIsolation: false,
    shortcutState: { profileId: p.id, path: `/home/me/.local/share/applications/vibelauncher-${p.id}.desktop`, exists: !!p.shortcut, stale: false },
    shortcutName: isolated ? `${p.provider === "claude" ? "Claude" : "ChatGPT"} · ${p.name}` : `${p.name} (VibeLauncher)`,
    iconUrl: svgIcon(brand[p.provider], isolated ? p.color : undefined),
    ...p,
  };
}

let settings: Settings = {
  routeClaude: true,
  routeChatgpt: true,
  staggerSeconds: 20,
  language: (new URLSearchParams(location.search).get("lang") as Settings["language"]) ?? "auto",
  theme: (new URLSearchParams(location.search).get("theme") as Settings["theme"]) ?? "auto",
  appOverrides: {},
  onboarded: scenario !== "fresh",
};

let profiles: ProfileView[] = [
  view({ id: "claude-default", name: "Claude", provider: "claude", color: brand.claude, isolated: false, icon: "house", running: scenario === "busy", lastLaunchedAt: iso(3) }),
  view({ id: "chatgpt-default", name: "ChatGPT", provider: "chatgpt", color: brand.chatgpt, isolated: false, icon: "house" }),
];
if (scenario === "busy" || scenario === "route") {
  profiles.push(
    view({ id: "work", name: "Work", provider: "claude", color: "#3B82F6", icon: "briefcase", running: true, shortcut: true, lastLaunchedAt: iso(12) }),
    view({ id: "personal", name: "Personal", provider: "claude", color: "#A855F7", icon: "heart", lastLaunchedAt: iso(60 * 26), lostIsolation: false }),
    view({ id: "client-acme", name: "Client – Acme", provider: "claude", color: "#F59E0B", icon: "building", running: true, lostIsolation: true, lastLaunchedAt: iso(60 * 3) }),
    view({ id: "chatgpt-team", name: "Team", provider: "chatgpt", color: "#14B8A6", icon: "users", shortcut: true, lastLaunchedAt: iso(60 * 50) }),
  );
}

let pending: { provider: "claude" | "chatgpt"; profile: string; at: number } | null = null;

function apps(): AppView[] {
  return (["claude", "chatgpt"] as const).map((provider) => {
    const installed = scenario !== "noapps" && !(provider === "chatgpt" && scenario === "fresh");
    const isolated = profiles.some((p) => p.provider === provider && p.isolated);
    const enabled = provider === "claude" ? settings.routeClaude : settings.routeChatgpt;
    const left = pending && pending.provider === provider ? 300 - Math.floor((Date.now() - pending.at) / 1000) : -1;
    if (left <= 0 && pending?.provider === provider) pending = null;
    return {
      provider,
      name: provider === "claude" ? "Claude" : "ChatGPT",
      install: installed
        ? {
            provider,
            program: provider === "claude" ? "/usr/bin/claude-desktop" : "/opt/ChatGPT/chatgpt",
            baseArgs: [],
            version: provider === "claude" ? "1.4.2" : "1.2026.311",
            source: "desktop entry",
            matchExes: [],
          }
        : undefined,
      iconUrl: installed ? svgIcon(brand[provider]) : undefined,
      routing: {
        provider,
        strategy: "shadow-desktop-entry",
        supported: true,
        experimental: false,
        claimed: enabled && isolated,
        stale: false,
        handler: enabled && isolated ? `${provider === "claude" ? "com.anthropic.Claude" : "chatgpt"}.desktop (VibeLauncher)` : undefined,
      },
      routingEnabled: enabled,
      routingWanted: enabled && isolated,
      pending: left > 0 && pending ? { profile: pending.profile, secondsLeft: left } : undefined,
      unknownRunning: 0,
    };
  });
}

function state(): StateView {
  return {
    version: "0.1.0",
    platform: (new URLSearchParams(location.search).get("platform") as StateView["platform"]) ?? "linux",
    profiles: profiles.map((p) => ({ ...p })),
    apps: apps(),
    settings: { ...settings },
    profilesDir: "/home/me/.local/share/vibelauncher/profiles",
    configFile: "/home/me/.config/vibelauncher/profiles.toml",
    cli: "/usr/bin/vibelauncher",
  };
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

function slug(name: string) {
  return name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "profile";
}

function fromInput(id: string, input: ProfileInput, old?: ProfileView): ProfileView {
  return view({
    ...(old ?? {}),
    id,
    name: input.name.trim(),
    provider: input.provider,
    color: input.color,
    icon: input.icon ?? old?.icon ?? "sparkle",
    shortcut: input.shortcut,
    env: input.env,
    extraArgs: input.extraArgs,
    dataRoot: input.dataRoot ?? undefined,
    iconUrl: svgIcon(brand[input.provider], old && !old.isolated ? undefined : input.color),
    isolated: old ? old.isolated : true,
  });
}

let routeTaken = scenario !== "route";

export async function mockInvoke(cmd: string, a: Record<string, any> = {}): Promise<unknown> {
  await sleep(cmd === "get_state" ? 20 : 180);
  const find = (id: string) => {
    const p = profiles.find((x) => x.id === id);
    if (!p) throw `no profile named "${id}"`;
    return p;
  };
  switch (cmd) {
    case "get_state":
      return state();
    case "launch":
      find(a.id).running = true;
      find(a.id).lastLaunchedAt = new Date().toISOString();
      return;
    case "login": {
      const p = find(a.id);
      p.running = true;
      pending = { provider: p.provider, profile: p.id, at: Date.now() };
      return;
    }
    case "cancel_login":
      pending = null;
      return;
    case "quit": {
      const p = find(a.id);
      const was = p.running;
      p.running = false;
      p.lostIsolation = false;
      return was;
    }
    case "open_link": {
      const p = find(a.id);
      const scheme = p.provider === "claude" ? "claude" : "codex";
      if (!String(a.url).trim().startsWith(scheme + "://")) throw `That isn't a ${p.provider === "claude" ? "Claude" : "ChatGPT"} link. It should start with ${scheme}://`;
      pending = null;
      p.running = true;
      return;
    }
    case "create_profile": {
      const input = a.input as ProfileInput;
      if (!input.name.trim()) throw "a profile needs a name";
      let id = slug(input.name);
      while (profiles.some((p) => p.id === id)) id += "-2";
      const p = fromInput(id, input);
      p.createdAt = new Date().toISOString();
      profiles.push(p);
      return p;
    }
    case "update_profile": {
      const old = find(a.id);
      const p = fromInput(old.id, a.input, old);
      p.running = old.running;
      profiles = profiles.map((x) => (x.id === old.id ? p : x));
      return p;
    }
    case "delete_profile": {
      const p = find(a.id);
      if (p.running) throw `quit ${p.name} first`;
      profiles = profiles.filter((x) => x.id !== a.id);
      return;
    }
    case "reorder":
      profiles = (a.ids as string[]).map((id) => profiles.find((p) => p.id === id)!).filter(Boolean);
      return;
    case "data_size":
      return find(a.id).isolated ? 312_000_000 + a.id.length * 41_000_000 : null;
    case "set_settings":
      settings = a.settings;
      return [];
    case "app_candidates":
      return apps().find((x) => x.provider === a.provider)?.install ? [apps().find((x) => x.provider === a.provider)!.install] : [];
    case "set_app":
    case "refresh_apps":
      return;
    case "run_doctor": {
      await sleep(400);
      const checks: Check[] = [
        { id: "claude-found", code: "app-found", params: { app: "Claude", version: "1.4.2", path: "/usr/bin/claude-desktop" }, level: "ok", title: "Claude 1.4.2", detail: "/usr/bin/claude-desktop (desktop entry)" },
        { id: "claude-routing", code: "routing-ok", params: { app: "Claude", scheme: "claude", handler: "com.anthropic.Claude.desktop (VibeLauncher)", experimental: "false" }, level: "ok", title: "claude:// links are routed", detail: "" },
      ];
      if (profiles.some((p) => p.lostIsolation))
        checks.unshift({ id: "client-acme-env", code: "lost-env", params: { name: "Client – Acme", var: "CLAUDE_CONFIG_DIR", app: "Claude" }, level: "warning", title: "", detail: "", fix: { kind: "restart", profiles: ["client-acme"] } });
      if (profiles.some((p) => p.provider === "claude" && p.isolated))
        checks.push({ id: "claude-cowork", code: "cowork", params: {}, level: "info", title: "", detail: "" });
      if (scenario === "noapps")
        checks.unshift({ id: "claude-missing", code: "app-missing", params: { app: "Claude" }, level: "error", title: "", detail: "", fix: { kind: "chooseApp", provider: "claude" } });
      return checks;
    }
    case "apply_fix":
      if (a.fix.kind === "restart") {
        for (const id of a.fix.profiles) find(id).lostIsolation = false;
        return true;
      }
      return false;
    case "route_log":
      return [
        "2026-11-14 09:12:40  claude://login/callback → work (a login was started from this profile)",
        "2026-11-14 09:30:02  claude://claude.ai/new → work (it is the only one running)",
        "2026-11-14 10:01:17  codex://auth/callback → chatgpt-team (chosen by you)",
      ];
    case "take_route": {
      if (routeTaken) return null;
      routeTaken = true;
      const r: RouteRequest = { url: "claude://login/callback?code=abc", redacted: "claude://login/callback", provider: "claude", isLogin: true, candidates: ["work", "personal", "client-acme"] };
      return r;
    }
    case "deliver_choice":
      find(a.id).running = true;
      return;
    case "reveal":
    case "open_website":
      return;
    case "opened_for_route":
      return scenario === "route";
  }
  throw `mock: unknown command ${cmd}`;
}
