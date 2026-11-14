#!/usr/bin/env bash
# Collects what the in-app updater needs from one build: each package it can
# install from, with its signature. Writes update/<target>.json (merged into
# latest.json by update-manifest.py) and puts any file that isn't uploaded
# already (the macOS .app.tar.gz, renamed per architecture) in update/.
#
# On Linux it also checks that each package's window binary knows which kind
# of package it is: the updater relies on that to install the right way.
#
# Usage: update-files.sh <target>    (run from the repository root, after
# `tauri build` with createUpdaterArtifacts and TAURI_SIGNING_PRIVATE_KEY set)
set -euo pipefail

target=$1
b="target/$target/release/bundle"
version=$(jq -r .version app/src-tauri/tauri.conf.json)
mkdir -p update
pieces='{}'

case "$target" in
  x86_64-unknown-linux-gnu) os=linux arch=x86_64 ;;
  aarch64-apple-darwin) os=darwin arch=aarch64 ;;
  x86_64-apple-darwin) os=darwin arch=x86_64 ;;
  x86_64-pc-windows-msvc) os=windows arch=x86_64 ;;
  *) echo "::error::unknown target $target"; exit 1 ;;
esac

one() { # one file matching a pattern
  local m=( $1 )
  [ ${#m[@]} -eq 1 ] && [ -f "${m[0]}" ] || { echo "::error::expected one file for $1, found: ${m[*]}" >&2; exit 1; }
  printf '%s' "${m[0]}"
}

# add <installer> <file> [default]: record <file> for <os>-<arch>-<installer>
add() {
  local kind=$1 file=$2 default=${3:-}
  if [ ! -f "$file.sig" ]; then
    # Tauri signs some package types but not others; sign the rest the same way.
    (cd app && pnpm -s tauri signer sign "$(cd .. && pwd)/$file" >/dev/null)
  fi
  local sig name
  sig=$(cat "$file.sig")
  name=$(basename "$file")
  pieces=$(jq --arg k "$os-$arch-$kind" --arg f "$name" --arg s "$sig" '. + {($k): {file: $f, signature: $s}}' <<<"$pieces")
  [ -z "$default" ] || pieces=$(jq --arg k "$os-$arch" --arg f "$name" --arg s "$sig" '. + {($k): {file: $f, signature: $s}}' <<<"$pieces")
  echo "$os-$arch-$kind${default:+ (default)}: $name"
}

# marker <file> <expected>: the bundler writes the package kind into the
# binary, over the "_UNK" placeholder. Every kind's name is in the binary once
# (the code that reads it); the one written there makes it twice.
marker() {
  local counts
  counts=$(grep -aoE '__TAURI_BUNDLE_TYPE_VAR_[A-Z]{3}' "$1" | sort | uniq -c | tr -s ' \n' ' ')
  if grep -aq '__TAURI_BUNDLE_TYPE_VAR_UNK' "$1" || [ "$(grep -aoE "__TAURI_BUNDLE_TYPE_VAR_$2" "$1" | wc -l)" -ne 2 ]; then
    echo "::error::$1 doesn't say it is $2:$counts"; exit 1
  fi
}

case "$os" in
  linux)
    appimage=$(one "$b/appimage/*.AppImage")
    deb=$(one "$b/deb/*.deb")
    rpm=$(one "$b/rpm/*.rpm")
    t=$(mktemp -d)
    (cd "$t" && "$OLDPWD/$appimage" --appimage-extract >/dev/null) && marker "$t/squashfs-root/usr/bin/vibelauncher-gui" APP
    dpkg-deb -x "$deb" "$t/deb" && marker "$t/deb/usr/bin/vibelauncher-gui" DEB
    mkdir -p "$t/rpm" && (cd "$t/rpm" && rpm2cpio "$OLDPWD/$rpm" | cpio -idm --quiet) && marker "$t/rpm/usr/bin/vibelauncher-gui" RPM
    rm -rf "$t"
    add appimage "$appimage" default
    add deb "$deb"
    add rpm "$rpm"
    ;;
  darwin)
    tgz=$(one "$b/macos/*.app.tar.gz")
    [ "$arch" = aarch64 ] && suffix=aarch64 || suffix=x64
    out="update/VibeLauncher_${version}_${suffix}.app.tar.gz"
    cp "$tgz" "$out"
    cp "$tgz.sig" "$out.sig"
    add app "$out" default
    ;;
  windows)
    add nsis "$(one "$b/nsis/*-setup.exe")" default
    ;;
esac

printf '%s\n' "$pieces" > "update/$target.json"
