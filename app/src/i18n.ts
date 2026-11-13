// Interface text in English and Simplified Chinese. Diagnostics come from
// Rust with a stable `code`; they are translated here (English fallback is
// the text Rust sent).

import type { Check } from "./api";

const en = {
  "app.name": "VibeLauncher",
  "nav.new": "New profile",
  "nav.diagnostics": "Diagnostics",
  "nav.settings": "Settings",
  "nav.collapse": "Collapse",
  "nav.expand": "Expand",
  "nav.notInstalled": "Not installed",

  "status.running": "Running",
  "status.stopped": "Not running",
  "status.signingIn": "Signing in",
  "status.lost": "Not isolated",
  "status.default": "As installed",
  "status.separate": "Separate profile",

  "action.open": "Open",
  "action.focus": "Show",
  "action.signIn": "Sign in…",
  "action.quit": "Quit",
  "action.restart": "Restart",
  "action.edit": "Edit…",
  "action.delete": "Delete…",
  "action.showData": "Show data folder",
  "action.copyCommand": "Copy launch command",
  "action.cancel": "Cancel",
  "action.save": "Save",
  "action.create": "Create profile",
  "action.done": "Done",
  "action.close": "Close",
  "action.choose": "Choose…",
  "action.change": "Change…",
  "action.reset": "Use detected",
  "action.show": "Show",
  "action.runAgain": "Check again",
  "action.findAgain": "Look again",
  "action.more": "More",

  "detail.defaultTitle": "This is {app} exactly as installed",
  "detail.defaultBody": "It uses the app's usual data and sign-in. VibeLauncher only opens it; nothing about it is changed.",
  "detail.profile": "Profile",
  "detail.dataFolder": "Data folder",
  "detail.size": "Size on disk",
  "detail.lastOpened": "Last opened",
  "detail.never": "Never",
  "detail.created": "Created",
  "detail.shortcut": "Shortcut",
  "detail.shortcutHint.linux": "Shows “{name}” in your app menu.",
  "detail.shortcutHint.macos": "Adds “{name}” to ~/Applications/VibeLauncher.",
  "detail.shortcutHint.windows": "Adds “{name}” to the Start menu.",
  "detail.shortcutBroken": "The shortcut is missing or outdated.",
  "detail.repair": "Repair",
  "detail.advanced": "Advanced",
  "detail.env": "Environment variables",
  "detail.args": "Extra arguments",
  "detail.none": "None",
  "detail.isolation": "Isolation",
  "detail.isolationBody": "Opened with its own data folder; sign-ins, chats and settings stay separate from other profiles.",
  "detail.lostTitle": "{name} lost its profile",
  "detail.lostBody": "An update probably restarted it on its own, so it's now using the shared settings. Restart it from here to fix that.",
  "detail.pendingTitle": "Waiting for the sign-in link",
  "detail.pendingBody": "Finish signing in to {app} in your browser. When the browser asks to open {app}, allow it — the link comes back to this profile.",
  "detail.pendingOther": "Another profile, “{name}”, is waiting for a sign-in link. Starting a sign-in here will take over.",
  "detail.pasteLink": "Paste link…",
  "detail.notInstalled": "{app} wasn't found on this computer.",
  "detail.notInstalledBody": "Install it from the official website, or point VibeLauncher to it in Settings.",
  "detail.openSettings": "Open Settings",
  "copied": "Copied",

  "time.justNow": "Just now",
  "time.min": "{n} min ago",
  "time.hour": "{n} h ago",
  "time.day": "{n} d ago",

  "sheet.newTitle": "New profile",
  "sheet.editTitle": "Edit profile",
  "sheet.name": "Name",
  "sheet.namePlaceholder": "e.g. Work",
  "sheet.app": "App",
  "sheet.color": "Color",
  "sheet.shortcut": "Add a shortcut",
  "sheet.dataFolder": "Data folder",
  "sheet.dataDefault": "Default: {path}",
  "sheet.env": "Environment variables",
  "sheet.envHint": "One per line, NAME=value.",
  "sheet.args": "Extra arguments",
  "sheet.argsHint": "One per line, passed to the app.",
  "sheet.advanced": "Advanced",
  "sheet.defaultNote": "The default profile always runs the app as installed; only its name, color and shortcut can change.",
  "sheet.badEnv": "Line {n} should look like NAME=value.",
  "sheet.created": "Created “{name}”",
  "sheet.saved": "Saved",

  "delete.title": "Delete “{name}”?",
  "delete.body": "Its shortcut is removed. The app itself isn't touched.",
  "delete.data": "Also move its data to the trash",
  "delete.dataHint": "Sign-in, chats and settings in {path}. Leave this off to keep them; creating a profile with the same data folder brings them back.",
  "delete.confirm": "Delete",
  "delete.quitConfirm": "Quit and delete",
  "delete.bodyRunning": "It's open now, so it quits first. Its shortcut is removed; the app itself isn't touched.",
  "delete.done": "Deleted “{name}”",

  "login.title": "Sign in to {name}",
  "login.step1": "{app} opens with this profile.",
  "login.step2": "Sign in there. Your browser opens to finish.",
  "login.step3": "When the browser asks to open {app}, allow it. The link comes back here.",
  "login.waiting": "Waiting for the link · {time}",
  "login.received": "Signed-in link delivered",
  "login.expired": "Time's up. Start again if you still need to sign in.",
  "login.pasteTitle": "Didn't come back?",
  "login.pasteBody": "In the browser, copy the link of the “Open {app}” button (or the page's address) and paste it here.",
  "login.pastePlaceholder": "{scheme}://…",
  "login.pasteOpen": "Open in this profile",
  "login.again": "Start again",
  "login.routingOff": "Sign-in links aren't routed through VibeLauncher right now, so they will open in whichever {app} window the system picks. Use “Paste link” below, or turn routing on in Settings.",

  "route.title": "Which profile should open this link?",
  "route.loginTitle": "Which profile is signing in?",
  "route.body": "Several {app} profiles could have asked for {link}.",
  "route.delivered": "Opened in “{name}”",

  "diag.title": "Diagnostics",
  "diag.subtitle": "Things that can quietly mix profiles up, and one-click fixes.",
  "diag.checking": "Checking…",
  "diag.allGood": "Everything looks right",
  "diag.issues": "{n} things need attention",
  "diag.issue": "1 thing needs attention",
  "diag.fix": "Fix",
  "diag.fixing": "Fixing…",
  "diag.fixed": "Fixed",
  "diag.restartStarted": "Restarting, one at a time…",
  "diag.log": "Recent links",
  "diag.logEmpty": "No links routed yet.",
  "diag.logHint": "Where claude:// and codex:// links went, and why. Login codes are never written down.",

  "settings.title": "Settings",
  "settings.general": "General",
  "settings.language": "Language",
  "settings.theme": "Appearance",
  "settings.auto": "Automatic",
  "settings.light": "Light",
  "settings.dark": "Dark",
  "settings.apps": "Apps",
  "settings.appMissing": "Not found",
  "settings.manual": "Chosen by you",
  "settings.links": "Sign-in links",
  "settings.linksBody": "When you sign in, the browser sends a link (claude:// or codex://) back to the app. VibeLauncher can pass it to the profile that asked for it. It only takes over when you have more than one profile of that app, and gives it back when you turn this off.",
  "settings.route": "Route {scheme}:// links",
  "settings.routeOn": "Handled by VibeLauncher",
  "settings.routeIdle": "Not needed yet (one profile)",
  "settings.routeOff": "Opened by the app directly",
  "settings.routeUnsupported": "Not possible on this system",
  "settings.experimental": "experimental",
  "settings.restarts": "Restarts",
  "settings.stagger": "Wait between restarts",
  "settings.staggerHint": "Profiles restarted together are started this many seconds apart, so their automatic updates don't line up.",
  "settings.seconds": "{n} s",
  "settings.about": "About",
  "settings.aboutBody": "VibeLauncher opens the official desktop apps with separate profiles. It doesn't modify, copy or redistribute them and never reads your sign-in data.",
  "settings.notAffiliated": "Not affiliated with Anthropic or OpenAI.",
  "settings.website": "Website",
  "settings.issues": "Report a problem",
  "settings.command": "Command",
  "settings.config": "Settings file",
  "settings.chooseApp": "Choose {app}",

  "onb.title": "Welcome to VibeLauncher",
  "onb.body": "Keep separate Claude and ChatGPT desktop profiles: work and personal, or one per client. Each has its own sign-in, chats and settings.",
  "onb.found": "Found on this computer",
  "onb.missing": "Not installed",
  "onb.p1": "Uses the official apps you installed — updates keep working.",
  "onb.p2": "Nothing is copied: each profile signs in on its own.",
  "onb.p3": "Shortcuts open a profile directly, without this window.",
  "onb.start": "Create a profile",
  "onb.skip": "Not now",

  "empty.title": "No app found",
  "empty.body": "Install Claude or ChatGPT for desktop, then come back. If it's installed somewhere unusual, choose it in Settings.",

  "error.generic": "Something went wrong",
} as const;

export type Key = keyof typeof en;

const zh: Record<Key, string> = {
  "app.name": "VibeLauncher",
  "nav.new": "新建配置",
  "nav.diagnostics": "诊断",
  "nav.settings": "设置",
  "nav.collapse": "收起",
  "nav.expand": "展开",
  "nav.notInstalled": "未安装",

  "status.running": "运行中",
  "status.stopped": "未运行",
  "status.signingIn": "登录中",
  "status.lost": "未隔离",
  "status.default": "原样运行",
  "status.separate": "独立配置",

  "action.open": "打开",
  "action.focus": "显示",
  "action.signIn": "登录…",
  "action.quit": "退出",
  "action.restart": "重新启动",
  "action.edit": "编辑…",
  "action.delete": "删除…",
  "action.showData": "显示数据文件夹",
  "action.copyCommand": "复制启动命令",
  "action.cancel": "取消",
  "action.save": "保存",
  "action.create": "创建",
  "action.done": "完成",
  "action.close": "关闭",
  "action.choose": "选择…",
  "action.change": "更改…",
  "action.reset": "使用自动找到的",
  "action.show": "显示",
  "action.runAgain": "重新检查",
  "action.findAgain": "重新查找",
  "action.more": "更多",

  "detail.defaultTitle": "这是原样安装的 {app}",
  "detail.defaultBody": "使用应用自己的数据和登录状态。VibeLauncher 只负责打开它，不做任何改动。",
  "detail.profile": "配置",
  "detail.dataFolder": "数据文件夹",
  "detail.size": "占用空间",
  "detail.lastOpened": "上次打开",
  "detail.never": "从未",
  "detail.created": "创建于",
  "detail.shortcut": "快捷方式",
  "detail.shortcutHint.linux": "在应用菜单里显示“{name}”。",
  "detail.shortcutHint.macos": "添加“{name}”到 ~/Applications/VibeLauncher。",
  "detail.shortcutHint.windows": "添加“{name}”到开始菜单。",
  "detail.shortcutBroken": "快捷方式丢失或已过期。",
  "detail.repair": "修复",
  "detail.advanced": "高级",
  "detail.env": "环境变量",
  "detail.args": "额外参数",
  "detail.none": "无",
  "detail.isolation": "隔离",
  "detail.isolationBody": "使用自己的数据文件夹打开；登录、对话和设置都与其他配置分开。",
  "detail.lostTitle": "“{name}”脱离了自己的配置",
  "detail.lostBody": "多半是更新时应用自己重启了，现在用的是共享设置。在这里重新启动即可恢复。",
  "detail.pendingTitle": "正在等待登录链接",
  "detail.pendingBody": "请在浏览器里完成 {app} 登录。浏览器询问是否打开 {app} 时选择允许，链接会回到这个配置。",
  "detail.pendingOther": "另一个配置“{name}”正在等待登录链接。在这里开始登录会接管它。",
  "detail.pasteLink": "粘贴链接…",
  "detail.notInstalled": "这台电脑上没有找到 {app}。",
  "detail.notInstalledBody": "请从官网安装，或在设置里告诉 VibeLauncher 它在哪里。",
  "detail.openSettings": "打开设置",
  "copied": "已复制",

  "time.justNow": "刚刚",
  "time.min": "{n} 分钟前",
  "time.hour": "{n} 小时前",
  "time.day": "{n} 天前",

  "sheet.newTitle": "新建配置",
  "sheet.editTitle": "编辑配置",
  "sheet.name": "名称",
  "sheet.namePlaceholder": "例如：工作",
  "sheet.app": "应用",
  "sheet.color": "颜色",
  "sheet.shortcut": "添加快捷方式",
  "sheet.dataFolder": "数据文件夹",
  "sheet.dataDefault": "默认：{path}",
  "sheet.env": "环境变量",
  "sheet.envHint": "每行一个，格式 NAME=value。",
  "sheet.args": "额外参数",
  "sheet.argsHint": "每行一个，传给应用。",
  "sheet.advanced": "高级",
  "sheet.defaultNote": "默认配置总是原样运行应用，只能修改名称、颜色和快捷方式。",
  "sheet.badEnv": "第 {n} 行应为 NAME=value 格式。",
  "sheet.created": "已创建“{name}”",
  "sheet.saved": "已保存",

  "delete.title": "删除“{name}”？",
  "delete.body": "会移除它的快捷方式，不会动应用本身。",
  "delete.data": "同时把它的数据移到废纸篓",
  "delete.dataHint": "{path} 里的登录状态、对话和设置。不勾选则保留；以后新建配置并选同一个数据文件夹即可恢复。",
  "delete.confirm": "删除",
  "delete.quitConfirm": "退出并删除",
  "delete.bodyRunning": "它正在运行，会先退出。会移除它的快捷方式，不会动应用本身。",
  "delete.done": "已删除“{name}”",

  "login.title": "登录“{name}”",
  "login.step1": "用这个配置打开 {app}。",
  "login.step2": "在 {app} 里登录，浏览器会打开来完成登录。",
  "login.step3": "浏览器询问是否打开 {app} 时选择允许，链接会回到这里。",
  "login.waiting": "等待链接 · {time}",
  "login.received": "登录链接已送达",
  "login.expired": "已超时。如仍需登录，请重新开始。",
  "login.pasteTitle": "链接没回来？",
  "login.pasteBody": "在浏览器里复制“打开 {app}”按钮的链接（或页面地址），粘贴到这里。",
  "login.pastePlaceholder": "{scheme}://…",
  "login.pasteOpen": "用这个配置打开",
  "login.again": "重新开始",
  "login.routingOff": "现在登录链接不经过 VibeLauncher，会由系统交给任意一个 {app} 窗口。请用下面的“粘贴链接”，或在设置里打开链接转发。",

  "route.title": "用哪个配置打开这个链接？",
  "route.loginTitle": "哪个配置在登录？",
  "route.body": "有多个 {app} 配置可能请求了 {link}。",
  "route.delivered": "已在“{name}”中打开",

  "diag.title": "诊断",
  "diag.subtitle": "可能悄悄弄混配置的问题，以及一键修复。",
  "diag.checking": "正在检查…",
  "diag.allGood": "一切正常",
  "diag.issues": "有 {n} 项需要处理",
  "diag.issue": "有 1 项需要处理",
  "diag.fix": "修复",
  "diag.fixing": "正在修复…",
  "diag.fixed": "已修复",
  "diag.restartStarted": "正在逐个重新启动…",
  "diag.log": "最近的链接",
  "diag.logEmpty": "还没有转发过链接。",
  "diag.logHint": "claude:// 和 codex:// 链接去了哪里、为什么。登录码不会被记录。",

  "settings.title": "设置",
  "settings.general": "通用",
  "settings.language": "语言",
  "settings.theme": "外观",
  "settings.auto": "自动",
  "settings.light": "浅色",
  "settings.dark": "深色",
  "settings.apps": "应用",
  "settings.appMissing": "未找到",
  "settings.manual": "手动选择",
  "settings.links": "登录链接",
  "settings.linksBody": "登录时，浏览器会把一个链接（claude:// 或 codex://）发回应用。VibeLauncher 可以把它交给发起登录的配置。只有当这个应用有多个配置时才会接管，关闭后会原样交还。",
  "settings.route": "转发 {scheme}:// 链接",
  "settings.routeOn": "由 VibeLauncher 处理",
  "settings.routeIdle": "暂不需要（只有一个配置）",
  "settings.routeOff": "由应用直接打开",
  "settings.routeUnsupported": "这个系统上无法实现",
  "settings.experimental": "实验性",
  "settings.restarts": "重启",
  "settings.stagger": "重启间隔",
  "settings.staggerHint": "一起重启的配置会按这个间隔依次启动，避免它们的自动更新撞在一起。",
  "settings.seconds": "{n} 秒",
  "settings.about": "关于",
  "settings.aboutBody": "VibeLauncher 用独立配置打开官方桌面应用。它不修改、不复制、不分发这些应用，也从不读取你的登录数据。",
  "settings.notAffiliated": "与 Anthropic 或 OpenAI 无关。",
  "settings.website": "项目主页",
  "settings.issues": "反馈问题",
  "settings.command": "命令",
  "settings.config": "设置文件",
  "settings.chooseApp": "选择 {app}",

  "onb.title": "欢迎使用 VibeLauncher",
  "onb.body": "为 Claude 和 ChatGPT 桌面版准备多个独立配置：工作和个人分开，或每个客户一个。各自有自己的登录、对话和设置。",
  "onb.found": "已在这台电脑上找到",
  "onb.missing": "未安装",
  "onb.p1": "使用你安装的官方应用，更新照常进行。",
  "onb.p2": "不复制任何东西：每个配置各自登录。",
  "onb.p3": "快捷方式直接打开配置，不需要这个窗口。",
  "onb.start": "创建配置",
  "onb.skip": "以后再说",

  "empty.title": "没有找到应用",
  "empty.body": "请先安装 Claude 或 ChatGPT 桌面版。如果装在特殊位置，可以在设置里手动选择。",

  "error.generic": "出了点问题",
};

// Diagnostics, by code: [title, detail].
const checksEn: Record<string, [string, string]> = {
  "app-missing": ["{app} wasn't found", "Your profiles can't start until VibeLauncher knows where the app is. Install it, or choose it by hand."],
  "app-not-installed": ["{app} isn't installed", "Nothing to do unless you want to use it."],
  "app-found": ["{app} {version}", "{path}"],
  "routing-unsupported": ["{scheme}:// links can't be routed", "{app} has no menu entry VibeLauncher can stand in for, so sign-in links open its default profile. Use “Paste link” when signing in to another profile."],
  "routing-off": ["{scheme}:// links aren't going through VibeLauncher", "Sign-in links will open whichever {app} window the system picks."],
  "routing-stale": ["{scheme}:// link handler is out of date", "It points at an old copy of VibeLauncher or an old version of the app."],
  "routing-conflict": ["Another link handler is in the way", "{detail}"],
  "routing-ok": ["{scheme}:// links are routed", "Links open in the profile that asked for them."],
  "lost-env": ["“{name}” is running without its own {var}", "Usually after an update restarted the app by itself; it now reads the shared settings. Restart the profile to fix it."],
  "unknown-instance": ["A {app} window isn't from any profile", "Process {pid} uses {dir}."],
  "updated": ["{app} was updated ({old} → {new})", "Profiles you open from VibeLauncher get the new version automatically."],
  "updated-running": ["{app} was updated ({old} → {new})", "If the update reopened windows on its own, they may have come back without their profile. Restart your profiles from here to be sure."],
  "simultaneous": ["{count} {app} profiles started at the same moment", "Instances that start together tend to update and restart together, which is when profiles get mixed up. Restarting them a few seconds apart avoids that."],
  cowork: ["Cowork is shared between profiles", "Claude's Cowork virtual machine belongs to your system account, not to a profile: only one profile at a time can use it."],
  shortcuts: ["Some shortcuts are missing or out of date", "{names}"],
  "data-not-dir": ["“{name}” can't use its data folder", "{path} exists but isn't a folder."],
};
const checksZh: Record<string, [string, string]> = {
  "app-missing": ["没有找到 {app}", "在 VibeLauncher 知道应用位置之前，配置无法启动。请安装它，或手动选择。"],
  "app-not-installed": ["未安装 {app}", "如果不用它，无需处理。"],
  "app-found": ["{app} {version}", "{path}"],
  "routing-unsupported": ["无法转发 {scheme}:// 链接", "{app} 没有可供 VibeLauncher 代理的菜单项，登录链接会打开默认配置。登录其他配置时请使用“粘贴链接”。"],
  "routing-off": ["{scheme}:// 链接没有经过 VibeLauncher", "登录链接会被系统交给任意一个 {app} 窗口。"],
  "routing-stale": ["{scheme}:// 链接处理程序已过期", "它指向旧的 VibeLauncher 或旧版本的应用。"],
  "routing-conflict": ["有其他链接处理程序挡在前面", "{detail}"],
  "routing-ok": ["{scheme}:// 链接已转发", "链接会在发起它的配置里打开。"],
  "lost-env": ["“{name}”没有带着自己的 {var} 运行", "通常是更新时应用自己重启了，现在读的是共享设置。重新启动这个配置即可修复。"],
  "unknown-instance": ["有一个 {app} 窗口不属于任何配置", "进程 {pid} 使用 {dir}。"],
  "updated": ["{app} 已更新（{old} → {new}）", "从 VibeLauncher 打开的配置会自动使用新版本。"],
  "updated-running": ["{app} 已更新（{old} → {new}）", "如果更新时窗口自己重新打开了，它们可能已脱离各自的配置。保险起见，请在这里重新启动这些配置。"],
  "simultaneous": ["{count} 个 {app} 配置在同一时刻启动", "同时启动的实例往往会同时更新、同时重启，这正是配置被弄混的时候。错开几秒重新启动可以避免。"],
  cowork: ["Cowork 由所有配置共享", "Claude 的 Cowork 虚拟机属于系统账户而不是配置：同一时间只有一个配置能用它。"],
  shortcuts: ["有快捷方式丢失或已过期", "{names}"],
  "data-not-dir": ["“{name}”无法使用它的数据文件夹", "{path} 存在，但不是文件夹。"],
};

let lang: "en" | "zh" = "en";

export function setLanguage(pref: "auto" | "en" | "zh") {
  lang = pref === "auto" ? (navigator.language.toLowerCase().startsWith("zh") ? "zh" : "en") : pref;
  document.documentElement.lang = lang === "zh" ? "zh-CN" : "en";
}

export function currentLanguage() {
  return lang;
}

function fill(s: string, p?: Record<string, string | number>): string {
  if (!p) return s;
  return s.replace(/\{(\w+)\}/g, (m, k) => (k in p ? String(p[k]) : m));
}

export function t(key: Key, params?: Record<string, string | number>): string {
  return fill(lang === "zh" ? zh[key] : en[key], params);
}

export function checkText(c: Check): { title: string; detail: string } {
  let code = c.code;
  if (code === "updated" && c.params.defaultRunning === "true") code = "updated-running";
  const table = lang === "zh" ? checksZh : checksEn;
  const row = table[code];
  if (!row) return { title: c.title, detail: c.detail };
  let detail = fill(row[1], c.params);
  if (code === "routing-ok" && c.params.handler) detail += (lang === "zh" ? " 当前处理程序：" : " Current handler: ") + c.params.handler + (lang === "zh" ? "。" : ".");
  if (code === "routing-ok" && c.params.experimental === "true") detail += lang === "zh" ? "（在这个系统上为实验性功能。）" : " (Experimental on this system.)";
  return { title: fill(row[0], c.params).trim(), detail };
}

export function relativeTime(iso?: string): string {
  if (!iso) return t("detail.never");
  const s = (Date.now() - new Date(iso).getTime()) / 1000;
  if (s < 60) return t("time.justNow");
  if (s < 3600) return t("time.min", { n: Math.floor(s / 60) });
  if (s < 86400) return t("time.hour", { n: Math.floor(s / 3600) });
  if (s < 86400 * 30) return t("time.day", { n: Math.floor(s / 86400) });
  return new Date(iso).toLocaleDateString(lang === "zh" ? "zh-CN" : undefined);
}

/** A link-log line starts with a UTC timestamp; show it in local time. */
export function localizeLogLine(line: string): string {
  const m = /^(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?Z)\s+(.*)$/.exec(line);
  if (!m) return line;
  const d = new Date(m[1]);
  if (Number.isNaN(d.getTime())) return line;
  const locale = lang === "zh" ? "zh-CN" : undefined;
  const sameDay = d.toDateString() === new Date().toDateString();
  const time = d.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  const when = sameDay ? time : `${d.toLocaleDateString(locale, { month: "short", day: "numeric" })} ${time}`;
  return `${when}  ${m[2]}`;
}

export function formatBytes(n: number): string {
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let v = n;
  while (v >= 1000 && i < u.length - 1) {
    v /= 1000;
    i++;
  }
  return `${v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1)} ${u[i]}`;
}
