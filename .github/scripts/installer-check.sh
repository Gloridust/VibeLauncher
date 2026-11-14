#!/usr/bin/env bash
# Windows only: installs the built -setup.exe, makes a profile of a stand-in
# Claude (so VibeLauncher takes claude:// and adds a Start-menu shortcut), then
# checks what the installer hooks (app/src-tauri/windows/hooks.nsh) do:
#
#   1. installing again silently, and the way the in-app updater runs the
#      installer, keeps the link and the shortcut;
#   2. the old uninstaller run the way an upgrade runs it (in place) gives the
#      link back and removes the shortcut, and installing afterwards puts both
#      back;
#   3. a real uninstall gives the link back and removes the shortcut, and keeps
#      the profile.
#
# Usage: installer-check.sh <target>   (expects target/<target>/release/vl-fake-app.exe)
set -euo pipefail
# Git Bash rewrites arguments that look like paths when it starts a Windows
# program, so "/S" reached the installer as "S:/" and it opened its wizard
# instead of installing silently. Pass every argument as written; paths that
# Windows programs need are converted with cygpath -w below.
export MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*'

target=$1
setup=$(ls target/"$target"/release/bundle/nsis/*-setup.exe | head -1)
fake="$PWD/target/$target/release/vl-fake-app.exe"
T="$(cygpath -u "$RUNNER_TEMP")/vlinst"
rm -rf "$T"
mkdir -p "$T/claude"
cp "$fake" "$T/claude/Claude.exe"
export VIBELAUNCHER_HOME
VIBELAUNCHER_HOME="$(cygpath -w "$T/home")"

inst="$(cygpath -u "$LOCALAPPDATA")/VibeLauncher"
cli="$inst/vibelauncher.exe"
links="$(cygpath -u "$APPDATA")/Microsoft/Windows/Start Menu/Programs/VibeLauncher"
fail=0
# Every program gets a time limit; on a hang, show what was still running.
run() {
  echo "+ $(date +%T) $*" >&2
  local rc=0
  "$@" &
  local pid=$! waited=0
  while kill -0 $pid 2>/dev/null && [ $waited -lt 180 ]; do sleep 2; waited=$((waited + 2)); done
  if kill -0 $pid 2>/dev/null; then
    echo "::error::timed out: $*" >&2
    mkdir -p installer-shots
    powershell -NoProfile -Command "
      Add-Type -AssemblyName System.Windows.Forms, System.Drawing
      \$b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
      \$bmp = New-Object System.Drawing.Bitmap \$b.Width, \$b.Height
      [System.Drawing.Graphics]::FromImage(\$bmp).CopyFromScreen(\$b.Location, [System.Drawing.Point]::Empty, \$b.Size)
      \$bmp.Save('$(cygpath -w "$PWD/installer-shots/hang.png")')" >&2 || true
    powershell -NoProfile -Command "Get-Process | Where-Object { \$_.MainWindowTitle } | Select-Object Id,Name,MainWindowTitle | Format-Table -AutoSize" >&2 || true
    ls -la "$inst" >&2 || true
    reg query 'HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\VibeLauncher' >&2 || true
    powershell -NoProfile -Command "Get-CimInstance Win32_Process | Where-Object { \$_.CreationDate -gt (Get-Date).AddMinutes(-5) } | Select-Object ProcessId,ParentProcessId,Name,CommandLine | Format-List" >&2 || true
    exit 1
  fi
  wait $pid || rc=$?
  return $rc
}
check() { if eval "$2"; then echo "ok   $1"; else echo "FAIL $1"; fail=1; fi; }

handler() { reg query 'HKCU\Software\Classes\claude\shell\open\command' /ve 2>/dev/null | tr -d '\r' || true; }
ours() { handler | grep -qi 'VibeLauncher\\vibelauncher.exe'; }
has_link() { ls "$links" 2>/dev/null | grep -q 'Work.*\.lnk$'; }

install() { run "$setup" /S; [ -x "$cli" ] || { echo "::error::nothing installed at $inst"; exit 1; }; }

echo "== install"
install
cfg=$(run "$cli" list --json | jq -r .config | tr -d '\r')
cfg=$(cygpath -u "$cfg")
mkdir -p "$(dirname "$cfg")"
cat > "$cfg" <<EOF
schema = 1

[settings]
onboarded = true

[settings.appOverrides]
claude = '$(cygpath -w "$T/claude/Claude.exe")'
EOF
run "$cli" create Work --app claude --shortcut
handler
check "creating a profile takes claude://" ours
check "creating a profile adds its shortcut" has_link

echo "== install again (silent: nothing is uninstalled)"
install
check "the link is kept" ours
check "the shortcut is kept" has_link

echo "== in-app update (how the updater runs the installer: passive, update mode)"
run "$setup" /P /UPDATE
[ -x "$cli" ] || { echo "::error::nothing installed at $inst"; exit 1; }
check "the link is kept" ours
check "the shortcut is kept" has_link

echo "== upgrade: the old uninstaller runs in place, then the new version installs"
run "$inst/uninstall.exe" /S "_?=$(cygpath -w "$inst")"
handler
check "the old uninstaller gives claude:// back" '! ours'
check "the old uninstaller removes the shortcut" '! has_link'
install
handler
check "installing takes claude:// back" ours
check "installing puts the shortcut back" has_link

echo "== uninstall"
run "$inst/uninstall.exe" /S
# The uninstaller copies itself to a temporary folder and returns at once.
for _ in $(seq 1 60); do [ -e "$cli" ] || break; sleep 1; done
check "the program is removed" '[ ! -e "$cli" ]'
handler
check "uninstalling gives claude:// back" '! ours'
check "uninstalling removes the shortcut" '! has_link'
check "uninstalling keeps the profile" 'grep -q "name = \"Work\"" "$cfg"'

exit $fail
