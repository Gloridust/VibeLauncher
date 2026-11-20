#!/usr/bin/env bash
# Opens the real manager window on a CI runner (Linux under Xvfb, macOS or
# Windows) with a few profiles of stand-in apps, checks it stays up, and takes
# screenshots: English + light, then Chinese + dark. Then the built-in
# click-through test (app/src-tauri/src/selftest.rs, run off-screen with the
# real window logic) creates, opens, quits, signs in, renames and deletes a
# profile, runs Diagnostics and changes the settings, and this script checks
# what it did with the `vibelauncher` command.
# Everything lives in a temporary VIBELAUNCHER_HOME. Expects release builds of
# vibelauncher, vibelauncher-gui (with the `selftest` feature) and vl-fake-app
# in target/release.
#
# To try it locally on Linux, use private XDG_* folders and a scratch HOME:
# RUNNER_TEMP=<folder> xvfb-run -a bash .github/scripts/window-check.sh
set -euo pipefail

out="$PWD/window-shots"
mkdir -p "$out"
T="$( (cygpath -u "$RUNNER_TEMP" 2>/dev/null) || printf %s "$RUNNER_TEMP")/vlui"
rm -rf "$T"
mkdir -p "$T/claude" "$T/chatgpt" "$T/notes/resources"
win=false
[ "${RUNNER_OS:-}" = "Windows" ] && win=true
ext=""
$win && ext=".exe"
cli="$PWD/target/release/vibelauncher$ext"
gui="$PWD/target/release/vibelauncher-gui$ext"
fake="$PWD/target/release/vl-fake-app$ext"

native() { if $win; then cygpath -w "$1"; else printf '%s' "$1"; fi; }
export VIBELAUNCHER_HOME
VIBELAUNCHER_HOME="$(native "$T/home")"

if $win; then
  claude="$T/claude/Claude.exe"; chatgpt="$T/chatgpt/ChatGPT.exe"
else
  claude="$T/claude/claude"; chatgpt="$T/chatgpt/chatgpt"
fi
cp "$fake" "$claude"
cp "$fake" "$chatgpt"
# An app to add with "Other app…": the stand-in, laid out like an Electron app.
notes="$T/notes/notes$ext"
cp "$fake" "$notes"
: > "$T/notes/resources/app.asar"

cfg=$("$cli" list --json | jq -r .config | tr -d '\r')
$win && cfg=$(cygpath -u "$cfg")
mkdir -p "$(dirname "$cfg")"
cat > "$cfg" <<EOF
schema = 1

[settings]
onboarded = true
staggerSeconds = 0
language = "en"
theme = "light"

[settings.appOverrides]
claude = '$(native "$claude")'
chatgpt = '$(native "$chatgpt")'
EOF

"$cli" create Work --app claude
"$cli" create Personal --app claude
"$cli" create "Client A" --app chatgpt
id_of() { "$cli" list --json | jq -r --arg n "$1" '.profiles[] | select(.name == $n) | .id'; }
"$cli" launch "$(id_of Work)"
"$cli" launch "$(id_of "Client A")"
sleep 3
"$cli" list | tee "$out/list.txt"
"$cli" doctor | tee "$out/doctor.txt" || true

shot() {
  if $win; then
    powershell -NoProfile -Command "
      Add-Type -AssemblyName System.Windows.Forms, System.Drawing
      \$b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
      \$bmp = New-Object System.Drawing.Bitmap \$b.Width, \$b.Height
      \$g = [System.Drawing.Graphics]::FromImage(\$bmp)
      \$g.CopyFromScreen(\$b.Location, [System.Drawing.Point]::Empty, \$b.Size)
      \$bmp.Save('$(native "$1")')"
  elif [ "$(uname)" = Darwin ]; then
    screencapture -x "$1"
  else
    import -window root "$1"
  fi
}

gui_alive() {
  if $win; then
    tasklist //FI "IMAGENAME eq vibelauncher-gui.exe" | grep -qi vibelauncher-gui
  else
    pgrep -f "$gui" >/dev/null
  fi
}

stop_gui() {
  if $win; then taskkill //F //IM vibelauncher-gui.exe >/dev/null 2>&1 || true
  else pkill -f "$gui" || true; fi
  sleep 2
}

open_and_shoot() {
  local name=$1
  "$gui" > "$out/gui-$name.log" 2>&1 &
  sleep 15
  check_window "$name"
  shot "$out/$name.png"
  stop_gui
}

check_window() {
  local name=$1
  if ! gui_alive; then
    echo "::error::the window exited ($name)"; cat "$out/gui-$name.log"; return 1
  fi
  if $win; then
    title=$(powershell -NoProfile -Command "(Get-Process vibelauncher-gui).MainWindowTitle" | tr -d '\r')
    echo "window title: $title"
    [ "$title" = "VibeLauncher" ] || { echo "::error::no VibeLauncher window ($name): '$title'"; return 1; }
  fi
}

open_and_shoot en-light
sed -i.bak -e 's/^language = "en"/language = "zh"/' -e 's/^theme = "light"/theme = "dark"/' "$cfg"
grep -q 'language = "zh"' "$cfg" || { echo "::error::couldn't switch the language"; cat "$cfg"; exit 1; }
open_and_shoot zh-dark

# ── click through the window ──
sed -i.bak -e 's/^language = "zh"/language = "en"/' -e 's/^theme = "dark"/theme = "light"/' "$cfg"
report="$out/selftest.txt"
rm -f "$report"
# Runs off-screen and exits by itself; a screenshot after every step.
VL_SELFTEST_OUT="$(native "$report")" VL_SHOT="$(native "$out/steps")" VL_SELFTEST_PICK="$(native "$notes")" \
  "$gui" > "$out/gui-selftest.log" 2>&1 || true
echo "── click-through test ──"
cat "$report" || { echo "::error::the click-through test didn't run"; exit 1; }
fail=0
grep -q '^start' "$report" || { echo "::error::the click-through test didn't start"; fail=1; }
grep '^FAIL' "$report" | sed 's/^/::error::/' && fail=1
grep -q '^end' "$report" || { echo "::error::the click-through test didn't finish"; fail=1; }

# What it did, as the command sees it.
"$cli" list --json > "$out/after.json"
if jq -e '.profiles[] | select(.name | startswith("UI Test"))' "$out/after.json" >/dev/null; then
  echo "::error::the profile the test deleted is still there"; fail=1
fi
grep -q 'language = "zh"' "$cfg" && grep -q 'theme = "dark"' "$cfg" \
  || { echo "::error::the settings the test chose weren't saved"; cat "$cfg"; fail=1; }
# The pasted sign-in link reached the stand-in, in the test profile's folder.
grep UISECRET "$T/claude/fake-app-log.jsonl" | grep -q 'ui-test' \
  || { echo "::error::the pasted sign-in link didn't reach the profile"; fail=1; }

# The added app ran with the profile's own folder, and was removed again.
grep -q 'user-data-dir=.*notes' "$T/notes/fake-app-log.jsonl" \
  || { echo "::error::the added app's profile didn't start with its own folder"; fail=1; }
if jq -e '.apps | length > 0' "$out/after.json" >/dev/null; then
  echo "::error::the app the test added is still there"; fail=1
fi

# Stop the stand-ins.
touch "$T/claude/fake-app-stop" "$T/chatgpt/fake-app-stop" "$T/notes/fake-app-stop"
ls -la "$out"
exit $fail
