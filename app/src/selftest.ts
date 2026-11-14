// Click-through test of the real window, for CI on systems nobody here uses
// day to day. Only in builds made with VITE_SELFTEST=1 and the GUI's
// `selftest` feature, and only runs when $VL_SELFTEST_OUT names a file to
// write the results to. The shipped app has none of this.
//
// It drives the interface the way a person would — buttons, fields, menus —
// starting in English with stand-in apps, and reports one line per step. The
// script around it then checks the results with the `vibelauncher` command.

import { invoke } from "@tauri-apps/api/core";

const NAME = "UI Test";
const RENAMED = "UI Test Renamed";
const LINK = "claude://login/callback?code=UISECRET";

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const report = (line: string) => invoke<boolean>("selftest_report", { line }).catch(() => false);

async function waitFor<T>(what: string, find: () => T | null | undefined | false, ms = 20_000): Promise<T> {
  const end = Date.now() + ms;
  for (;;) {
    const v = find();
    if (v) return v;
    if (Date.now() > end) throw new Error(`timed out waiting for ${what}`);
    await sleep(150);
  }
}

const text = (el: Element) => (el.textContent ?? "").replace(/\s+/g, " ").trim();
const all = <E extends Element = HTMLElement>(sel: string, root: ParentNode = document) => Array.from(root.querySelectorAll<E>(sel));
const dialog = () => document.querySelector<HTMLElement>("[role=dialog]");

/** An enabled button (or menu item, radio) whose text is `label`. */
function button(label: string, root: ParentNode = document) {
  return all<HTMLButtonElement>("button", root).find((b) => !b.disabled && text(b) === label) ?? null;
}

async function click(what: string, find: () => HTMLElement | null) {
  const el = await waitFor(what, find);
  el.click();
  await sleep(250);
}

function type(el: HTMLInputElement, value: string) {
  el.focus();
  el.value = value;
  el.dispatchEvent(new Event("input", { bubbles: true }));
}

const sidebarItem = (name: string) => all("[data-profile]").find((b) => text(b).startsWith(name)) ?? null;
const heading = () => text(document.querySelector(".hero h1") ?? document.body);
const status = () => text(document.querySelector(".hero-sub .pill") ?? document.body);

async function moreMenu(item: (b: HTMLButtonElement) => boolean, what: string) {
  await click("the More button", () => document.querySelector<HTMLElement>(".topbar .icon-btn"));
  await click(what, () => all<HTMLButtonElement>(".menu button").find((b) => !b.disabled && item(b)) ?? null);
}

const steps: [string, () => Promise<void>][] = [
  ["window shows the profiles", async () => {
    await waitFor("the Work profile in the sidebar", () => sidebarItem("Work"));
    await waitFor("the Client A profile in the sidebar", () => sidebarItem("Client A"));
  }],
  ["create a profile", async () => {
    await click("New profile", () => button("New profile"));
    const name = await waitFor("the name field", () => document.querySelector<HTMLInputElement>("#pf-name"));
    type(name, NAME);
    await click("the Claude choice", () => all<HTMLButtonElement>("[role=radio]", dialog()!).find((b) => text(b).includes("Claude")) ?? null);
    await click("Create profile", () => button("Create profile", dialog()!));
    await waitFor("the new profile in the sidebar", () => sidebarItem(NAME));
    await waitFor("the new profile to be shown", () => heading() === NAME);
  }],
  ["open it", async () => {
    await click("Open", () => button("Open", document.querySelector(".hero")!));
    await waitFor("it to be running", () => status() === "Running");
  }],
  ["quit it", async () => {
    await click("Quit", () => button("Quit", document.querySelector(".hero")!));
    await waitFor("it to stop", () => status() === "Not running", 30_000);
  }],
  ["sign in with a pasted link", async () => {
    await click("Sign in…", () => button("Sign in…", document.querySelector(".hero")!));
    const field = await waitFor("the paste field", () => document.querySelector<HTMLInputElement>("#paste-link"));
    await waitFor("the sign-in to start", () => dialog() && text(dialog()!).includes("Waiting for the link"));
    type(field, LINK);
    await click("Open in this profile", () => button("Open in this profile", dialog()!));
    await waitFor("the link to be delivered", () => dialog() && text(dialog()!).includes("Signed-in link delivered"));
    await click("Done", () => button("Done", dialog()!));
    await waitFor("the sheet to close", () => !dialog());
  }],
  ["quit after signing in", async () => {
    await waitFor("it to be running", () => status() === "Running");
    await click("Quit", () => button("Quit", document.querySelector(".hero")!));
    await waitFor("it to stop", () => status() === "Not running", 30_000);
  }],
  ["rename it", async () => {
    await moreMenu((b) => text(b) === "Edit…", "Edit…");
    const name = await waitFor("the name field", () => document.querySelector<HTMLInputElement>("#pf-name"));
    type(name, RENAMED);
    await click("Save", () => button("Save", dialog()!));
    await waitFor("the new name", () => heading() === RENAMED && sidebarItem(RENAMED));
  }],
  ["diagnostics", async () => {
    await click("Diagnostics", () => button("Diagnostics"));
    const done = () => /Everything looks right|needs? attention/.test(text(document.querySelector(".content") ?? document.body));
    await waitFor("the checks", done, 30_000);
    await click("Check again", () => button("Check again"));
    await waitFor("the checks again", done, 30_000);
  }],
  ["settings show updates", async () => {
    await click("Settings", () => all<HTMLButtonElement>(".sidebar-bottom button")[0] ?? null);
    // The automatic check may still be running; any settled state will do.
    await waitFor("the Updates row", () => {
      const row = document.querySelector<HTMLElement>("[data-update]");
      return row && text(row).startsWith("Updates") && !/^(checking|downloading|installing)$/.test(row.dataset.update ?? "");
    }, 40_000);
    await waitFor("the automatic check switch", () => all("[role=switch]").find((b) => b.getAttribute("aria-label") === "Check for updates automatically"));
  }],
  ["switch to Chinese and dark", async () => {
    await click("Settings", () => all<HTMLButtonElement>(".sidebar-bottom button")[0] ?? null);
    await click("简体中文", () => button("简体中文"));
    await waitFor("the interface in Chinese", () => button("新建配置"));
    await click("深色", () => button("深色"));
    await waitFor("the dark theme", () => all("[role=radio][aria-checked=true]").some((b) => text(b) === "深色"));
  }],
  ["delete it", async () => {
    await click("the renamed profile", () => sidebarItem(RENAMED));
    await waitFor("it to be shown", () => heading() === RENAMED);
    await moreMenu((b) => b.classList.contains("danger"), "Delete");
    await click("the delete button", () => dialog()?.querySelector<HTMLElement>("button[type=submit].danger") ?? null);
    await waitFor("it to be gone", () => !sidebarItem(RENAMED) && !dialog());
  }],
];

export async function run() {
  // Only when the script around it asked for the test ($VL_SELFTEST_OUT).
  if (!(await report("start"))) return;
  for (const [name, fn] of steps) {
    try {
      await fn();
      await report(`ok ${name}`);
    } catch (e) {
      await report(`FAIL ${name}: ${e instanceof Error ? e.message : e}`);
      await report(`page: ${text(document.body).slice(0, 1500)}`);
      break;
    }
  }
  await report("end");
}
