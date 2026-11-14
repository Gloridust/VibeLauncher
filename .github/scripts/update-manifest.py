#!/usr/bin/env python3
"""Write latest.json, the file the in-app updater reads, from the pieces each
build job left (see the "Gather the update files" step in release.yml).

Usage: update-manifest.py <pieces-dir> <tag> <repo> <out-file>

Every platform must be there: a release whose manifest lacked one would leave
that platform's users on the old version without saying so.
"""

import datetime
import json
import pathlib
import re
import sys

REQUIRED = [
    "linux-x86_64-appimage",
    "linux-x86_64-deb",
    "linux-x86_64-rpm",
    "linux-x86_64",
    "darwin-aarch64-app",
    "darwin-aarch64",
    "darwin-x86_64-app",
    "darwin-x86_64",
    "windows-x86_64-nsis",
    "windows-x86_64",
]


def main() -> int:
    pieces, tag, repo, out = sys.argv[1:5]
    version = tag.removeprefix("v")
    conf = json.loads(pathlib.Path("app/src-tauri/tauri.conf.json").read_text())
    if conf["version"] != version:
        print(f"::error::tag {tag} doesn't match the app version {conf['version']}")
        return 1

    platforms = {}
    for f in sorted(pathlib.Path(pieces).glob("*.json")):
        for key, entry in json.loads(f.read_text()).items():
            if key in platforms:
                print(f"::error::{key} appears twice")
                return 1
            if not re.fullmatch(r"[A-Za-z0-9._+-]+", entry["file"]):
                print(f"::error::unexpected file name {entry['file']!r}")
                return 1
            if version not in entry["file"]:
                print(f"::error::{entry['file']} isn't version {version}")
                return 1
            platforms[key] = {
                "signature": entry["signature"],
                "url": f"https://github.com/{repo}/releases/download/{tag}/{entry['file']}",
            }

    missing = [k for k in REQUIRED if k not in platforms]
    if missing:
        print(f"::error::no update for {', '.join(missing)}")
        return 1

    manifest = {
        "version": version,
        "notes": "",
        "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": dict(sorted(platforms.items())),
    }
    pathlib.Path(out).write_text(json.dumps(manifest, indent=2) + "\n")
    for k, v in manifest["platforms"].items():
        print(f"{k:24} {v['url']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
