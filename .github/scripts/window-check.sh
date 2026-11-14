#!/usr/bin/env bash
# Opens the real manager window on a CI runner (macOS or Windows) with a few
# profiles of stand-in apps, checks it stays up, and takes screenshots:
# English + light, then Chinese + dark. Everything lives in a temporary
# VIBELAUNCHER_HOME. Expects release builds of vibelauncher, vibelauncher-gui
# and vl-fake-app in target/release.
set -euo pipefail

out="$PWD/window-shots"
mkdir -p "$out"
T="$( (cygpath -u "$RUNNER_TEMP" 2>/dev/null) || printf %s "$RUNNER_TEMP")/vlui"
rm -rf "$T"
mkdir -p "$T/claude" "$T/chatgpt"
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
  else
    screencapture -x "$1"
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
  if ! gui_alive; then
    echo "::error::the window exited ($name)"; cat "$out/gui-$name.log"; return 1
  fi
  if $win; then
    title=$(powershell -NoProfile -Command "(Get-Process vibelauncher-gui).MainWindowTitle" | tr -d '\r')
    echo "window title: $title"
    [ "$title" = "VibeLauncher" ] || { echo "::error::no VibeLauncher window ($name): '$title'"; return 1; }
  fi
  shot "$out/$name.png"
  stop_gui
}

open_and_shoot en-light
sed -i.bak -e 's/^language = "en"/language = "zh"/' -e 's/^theme = "light"/theme = "dark"/' "$cfg"
grep -q 'language = "zh"' "$cfg" || { echo "::error::couldn't switch the language"; cat "$cfg"; exit 1; }
open_and_shoot zh-dark

# Stop the stand-ins.
touch "$T/claude/fake-app-stop" "$T/chatgpt/fake-app-stop"
ls -la "$out"
