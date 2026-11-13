// Every call into the Rust side goes through here. In a plain browser (the
// Vite dev server without Tauri) a mock backend is used instead, so the UI
// can be worked on and screenshotted on its own; it is never shipped.

import { invoke as tauriInvoke } from "@tauri-apps/api/core";

export type Provider = "claude" | "chatgpt";
export const PROVIDERS: Provider[] = ["claude", "chatgpt"];

export interface Profile {
  id: string;
  name: string;
  provider: Provider;
  color: string;
  icon: string;
  isolated: boolean;
  dataRoot?: string;
  env?: Record<string, string>;
  extraArgs?: string[];
  shortcut: boolean;
  createdAt: string;
  lastLaunchedAt?: string;
}

export interface ShortcutState {
  profileId: string;
  path: string;
  exists: boolean;
  stale: boolean;
}

export interface ProfileView extends Profile {
  dataDir: string;
  running: boolean;
  lostIsolation: boolean;
  shortcutState: ShortcutState;
  shortcutName: string;
  iconUrl?: string;
}

export interface AppInstall {
  provider: Provider;
  program: string;
  baseArgs: string[];
  bundle?: string;
  version?: string;
  source: string;
  desktopId?: string;
  desktopFile?: string;
  icon?: string;
  matchExes: string[];
}

export interface SchemeStatus {
  provider: Provider;
  strategy: string;
  supported: boolean;
  experimental: boolean;
  claimed: boolean;
  stale: boolean;
  conflict?: string;
  handler?: string;
}

export interface AppView {
  provider: Provider;
  name: string;
  install?: AppInstall;
  iconUrl?: string;
  routing: SchemeStatus;
  routingEnabled: boolean;
  routingWanted: boolean;
  pending?: { profile: string; secondsLeft: number };
  unknownRunning: number;
}

export interface Settings {
  routeClaude: boolean;
  routeChatgpt: boolean;
  staggerSeconds: number;
  language: "auto" | "en" | "zh";
  theme: "auto" | "light" | "dark";
  appOverrides: Record<string, string>;
  onboarded: boolean;
}

export type Platform = "linux" | "macos" | "windows";

export interface StateView {
  version: string;
  platform: Platform;
  profiles: ProfileView[];
  apps: AppView[];
  settings: Settings;
  profilesDir: string;
  configFile: string;
  cli: string;
}

export type Fix =
  | { kind: "claimScheme"; provider: Provider }
  | { kind: "syncShortcuts" }
  | { kind: "restart"; profiles: string[] }
  | { kind: "chooseApp"; provider: Provider };

export type Level = "ok" | "info" | "warning" | "error";

export interface Check {
  id: string;
  code: string;
  params: Record<string, string>;
  level: Level;
  title: string;
  detail: string;
  fix?: Fix;
}

export interface RouteRequest {
  url: string;
  redacted: string;
  provider: Provider;
  isLogin: boolean;
  candidates: string[];
}

export interface ProfileInput {
  name: string;
  provider: Provider;
  color: string;
  icon?: string;
  dataRoot?: string | null;
  env: Record<string, string>;
  extraArgs: string[];
  shortcut: boolean;
}

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri) return tauriInvoke<T>(cmd, args);
  if (import.meta.env.DEV) {
    const { mockInvoke } = await import("./mock");
    return mockInvoke(cmd, args) as Promise<T>;
  }
  throw new Error("VibeLauncher's window must run inside the app.");
}

export const api = {
  state: () => call<StateView>("get_state"),
  launch: (id: string) => call<void>("launch", { id }),
  login: (id: string) => call<void>("login", { id }),
  cancelLogin: (provider: Provider) => call<void>("cancel_login", { provider }),
  quit: (id: string) => call<boolean>("quit", { id }),
  openLink: (id: string, url: string) => call<void>("open_link", { id, url }),
  createProfile: (input: ProfileInput) => call<Profile>("create_profile", { input }),
  updateProfile: (id: string, input: ProfileInput) => call<Profile>("update_profile", { id, input }),
  deleteProfile: (id: string, deleteData: boolean) => call<void>("delete_profile", { id, deleteData }),
  reorder: (ids: string[]) => call<void>("reorder", { ids }),
  dataSize: (id: string) => call<number | null>("data_size", { id }),
  setSettings: (settings: Settings) => call<string[]>("set_settings", { settings }),
  appCandidates: (provider: Provider) => call<AppInstall[]>("app_candidates", { provider }),
  setApp: (provider: Provider, path: string | null) => call<void>("set_app", { provider, path }),
  refreshApps: () => call<void>("refresh_apps"),
  doctor: () => call<Check[]>("run_doctor"),
  applyFix: (fix: Fix) => call<boolean>("apply_fix", { fix }),
  routeLog: () => call<string[]>("route_log"),
  takeRoute: () => call<RouteRequest | null>("take_route"),
  deliverChoice: (id: string, url: string) => call<void>("deliver_choice", { id, url }),
  reveal: (path: string) => call<void>("reveal", { path }),
  openWebsite: (page: "home" | "issues" | "releases") => call<void>("open_website", { page }),
  openedForRoute: () => call<boolean>("opened_for_route"),
};

/** Subscribe to an event from the Rust side; no-op outside Tauri. */
export async function on<T>(event: string, cb: (payload: T) => void): Promise<() => void> {
  if (!isTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<T>(event, (e) => cb(e.payload));
}

export async function pickFile(opts: { directory: boolean; title: string }): Promise<string | null> {
  if (!isTauri) return opts.directory ? "/home/me/Profiles/picked" : "/opt/Claude/claude-desktop";
  const { open } = await import("@tauri-apps/plugin-dialog");
  const r = await open({ directory: opts.directory, multiple: false, title: opts.title });
  return typeof r === "string" ? r : null;
}

export async function windowCall(fn: "show" | "close" | "setTheme", arg?: unknown): Promise<void> {
  if (!isTauri) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  const w = getCurrentWindow();
  if (fn === "show") {
    await w.show();
    await w.setFocus();
  } else if (fn === "close") await w.close();
  else await w.setTheme((arg as "light" | "dark" | null) ?? null);
}

export function errText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
