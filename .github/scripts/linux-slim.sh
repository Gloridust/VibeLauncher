#!/usr/bin/env bash
# Tauri's Linux bundler is made for WebKit windows: it lists WebKitGTK and GTK
# as dependencies of the deb and rpm, and its AppImage carries GTK (and
# WebKit's helper programs, when the build machine has them). VibeLauncher's
# window is drawn by Slint and needs neither, so this rewrites the three
# packages Tauri made without them. Their files, and the package kind Tauri
# writes into each window binary, are kept as they are.
#
# The window links only to libc and fontconfig; it loads xkbcommon (and the
# X11 or Wayland libraries every desktop has) when it starts.
#
# Usage: linux-slim.sh <target>    (run from the repository root, after
# `tauri bundle`, and before update-files.sh signs the packages)
set -euo pipefail

target=$1
b="target/$target/release/bundle"
case "$target" in
  x86_64-*) arch=x86_64 ;;
  aarch64-*) arch=aarch64 ;;
  *) echo "::error::unknown target $target"; exit 1 ;;
esac

one() { # one file matching a pattern
  local m=( $1 )
  [ ${#m[@]} -eq 1 ] && [ -f "${m[0]}" ] || { echo "::error::expected one file for $1, found: ${m[*]}" >&2; exit 1; }
  printf '%s' "${m[0]}"
}

t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT

# --- deb: replace the Depends line with what the binaries actually use.
deb=$(one "$b/deb/*.deb")
dpkg-deb -R "$deb" "$t/deb"
mkdir -p "$t/shlibs/debian"
printf 'Source: x\n\nPackage: x\nArchitecture: any\n' > "$t/shlibs/debian/control"
libs=$(cd "$t/shlibs" && dpkg-shlibdeps -O "$t/deb/usr/bin/vibelauncher-gui" "$t/deb/usr/bin/vibelauncher" | sed -n 's/^shlibs:Depends=//p')
[ -n "$libs" ] || { echo "::error::dpkg-shlibdeps found nothing"; exit 1; }
depends="$libs, libxkbcommon0, libxkbcommon-x11-0"
sed -i "s/^Depends: .*/Depends: $depends/" "$t/deb/DEBIAN/control"
grep -q "^Depends: $depends\$" "$t/deb/DEBIAN/control" || { echo "::error::the deb has no Depends line to replace"; exit 1; }
rm -f "$deb" "$deb.sig"
dpkg-deb --root-owner-group -Zxz --build "$t/deb" "$deb" >/dev/null
echo "deb: Depends: $depends"

# --- rpm: build it again from its own files and fields; rpm works out the
# libraries the binaries link to, and xkbcommon is added by hand.
rpm=$(one "$b/rpm/*.rpm")
q() { rpm -qp --qf "$1" "$rpm" 2>/dev/null; }
mkdir -p "$t/rpm/root" "$t/rpm/out"
# rpm2cpio (4.17) exits with 1 on Tauri's rpm even though it writes all of
# it, so check the files themselves.
rpm2cpio "$rpm" > "$t/rpm/payload.cpio" || true
(cd "$t/rpm/root" && cpio -idm --quiet < "$t/rpm/payload.cpio")
files=$(q '[%{FILENAMES}\n]')
while read -r f; do
  [ -f "$t/rpm/root$f" ] || { echo "::error::$f is missing from $rpm"; exit 1; }
done <<<"$files"
chmod -R go-w "$t/rpm/root"
{
  echo "Name: $(q '%{NAME}')"
  echo "Version: $(q '%{VERSION}')"
  echo "Release: $(q '%{RELEASE}')"
  echo "Summary: $(q '%{SUMMARY}')"
  echo "License: $(q '%{LICENSE}')"
  url=$(q '%{URL}'); [ "$url" = "(none)" ] || echo "URL: $url"
  echo "Requires: libxkbcommon.so.0()(64bit), libxkbcommon-x11.so.0()(64bit)"
  echo "%description"
  q '%{DESCRIPTION}'; echo
  echo "%install"
  echo "cp -a $t/rpm/root/. %{buildroot}/"
  echo "%files"
  echo "%defattr(-,root,root,-)"
  printf '%s\n' "$files"
} > "$t/rpm/vibelauncher.spec"
rpmbuild -bb --quiet \
  --define "_topdir $t/rpm/top" --define "_rpmdir $t/rpm/out" --define "_build_id_links none" \
  --define "__strip /bin/true" --define "__os_install_post %{nil}" --define "debug_package %{nil}" \
  --define "_binary_payload w9.gzdio" --target "$arch" "$t/rpm/vibelauncher.spec"
built=$(one "$t/rpm/out/*/*.rpm")
rm -f "$rpm" "$rpm.sig"
mv "$built" "$rpm"
echo "rpm: Requires:" $(rpm -qpR "$rpm" 2>/dev/null | grep -v '^rpmlib(' | sort -u)

# --- AppImage: pack Tauri's AppDir again without the GTK plugin's files.
appimage=$(one "$b/appimage/*.AppImage")
ad=$(one "$b/appimage/*.AppDir/AppRun")
ad=${ad%/AppRun}
# linuxdeploy put a script in front of the AppRun to load the GTK plugin's
# settings; put the original back.
[ ! -e "$ad/AppRun.wrapped" ] || mv -f "$ad/AppRun.wrapped" "$ad/AppRun"
# Keep our programs, menu entry and icons; the rest is GTK, WebKit and their
# libraries.
rm -rf "$ad/apprun-hooks" "$ad/usr/lib" "$ad/usr/libexec"
find "$ad/usr/share" -mindepth 1 -maxdepth 1 ! -name applications ! -name icons -exec rm -rf {} +
mkdir -p "$ad/usr/lib"
tools=${XDG_CACHE_HOME:-$HOME/.cache}/tauri
linuxdeploy=$(one "$tools/linuxdeploy-*-$arch.AppImage")
rm -f "$appimage" "$appimage.sig"
OUTPUT="$appimage" ARCH=$arch APPIMAGE_EXTRACT_AND_RUN=1 \
  "$linuxdeploy" --appimage-extract-and-run --verbosity 2 --appdir "$ad" --output appimage >/dev/null
[ -x "$appimage" ] || { echo "::error::linuxdeploy didn't make $appimage"; exit 1; }
echo "AppImage: $(du -h "$appimage" | cut -f1), bundled libraries:" $(ls "$ad/usr/lib")
