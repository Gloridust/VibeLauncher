#!/usr/bin/env bash
# Windows only: installs the built -setup.exe, makes a profile of a stand-in
# Claude (so VibeLauncher takes claude:// and adds a Start-menu shortcut), then
# checks what the installer hooks (app/src-tauri/windows/hooks.nsh) do:
#
#   1. installing again silently keeps the link and the shortcut;
#   2. the old uninstaller run the way an upgrade runs it (in place) gives the
#      link back and removes the shortcut, and installing afterwards puts both
#      back;
#   3. a real uninstall gives the link back and removes the shortcut, and keeps
#      the profile.
#
# Usage: installer-check.sh <target>   (expects target/<target>/release/vl-fake-app.exe)
set -euo pipefail

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
  timeout 180 "$@" || rc=$?
  if [ $rc = 124 ]; then
    echo "::error::timed out: $*" >&2
    tasklist //v | grep -iE 'vibelauncher|setup|uninstall|Au_|Claude' >&2 || true
    exit 1
  fi
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
