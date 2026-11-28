#!/usr/bin/env python3
"""Writes THIRD-PARTY-LICENSES.txt for the release archives.

    python tools/release/third_party_licenses.py --target TRIPLE --out FILE

The release programs come from two Cargo workspaces: fnvtool and nvforever-launcher from the root one,
nvforever and nvf-viewer from engine/. This lists every third-party package those
programs are built from (normal and build dependencies for TRIPLE, not dev
dependencies, not our own workspace crates) with its licence and source, followed by
the licence texts found in the packages. MIT, BSD and Apache-2.0 ask for their notices
to travel with the binaries; MPL-2.0 asks that users are told where the source is.
The capture plugin has its own list (tools/capture/package_plugin.py).

Code translated from sources that are not Cargo packages is not found that way: each such
source is listed in NOTICES with the licence file kept beside the translation, and its text
follows with the packages' (DLSS's FFI declarations, translated from NVIDIA Streamline's
MIT headers). The notices ship no third-party program or library.

Some packages ship no licence file, only the licence name in their manifest; they are
listed with that name and marked, and the same licence's text from other packages
follows. Exits non-zero only when a package names no licence at all.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
# (workspace folder, the release packages built from it)
WORKSPACES = ((REPO, ("fnvtool", "nvf-launcher")), (REPO / "engine", ("nvforever", "nvf-viewer")))
LICENSE_FILE = re.compile(r"^(licen[sc]e|copying|unlicense|notice)", re.IGNORECASE)


@dataclass(frozen=True)
class Notice:
    """Code in NVForever's own sources translated from a source that is no Cargo package, or a
    file built into the program from one (the menus' font)."""

    name: str
    what: str
    licence: str
    source: str
    file: Path  # the licence text kept beside the translation


# Listed for every target: the Streamline declarations are compiled on Windows, the DLSS mode
# values they come with on every system.
NOTICES = (
    Notice(
        name="NVIDIA Streamline 2.14.16",
        what="FFI declarations translated from its headers, in engine/nvforever/src/dlss/ and crates/nvf-dlss-files/",
        licence="MIT",
        source="https://github.com/NVIDIA-RTX/Streamline",
        file=REPO / "engine" / "nvforever" / "src" / "dlss" / "LICENSE-MIT",
    ),
    Notice(
        name="Roboto Condensed (variable font, weight axis)",
        what="the menus' sharp letters, engine/nvf-play/assets/fonts/, built into nvforever",
        licence="OFL-1.1",
        source="https://github.com/google/fonts/tree/main/ofl/robotocondensed",
        file=REPO / "engine" / "nvf-play" / "assets" / "fonts" / "OFL.txt",
    ),
)


def metadata(folder: Path, target: str) -> dict:
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", target],
        cwd=folder, check=True, capture_output=True, text=True,
    ).stdout
    return json.loads(out)


def third_party(meta: dict, roots: tuple[str, ...]) -> list[dict]:
    """Packages reached from the named workspace packages, minus the workspace's own."""
    by_id = {p["id"]: p for p in meta["packages"]}
    members = set(meta["workspace_members"])
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    todo = [i for i in members if by_id[i]["name"] in roots]
    missing = set(roots) - {by_id[i]["name"] for i in todo}
    if missing:
        raise SystemExit(f"release packages not in the workspace: {', '.join(sorted(missing))}")
    seen: set[str] = set()
    while todo:
        i = todo.pop()
        if i in seen:
            continue
        seen.add(i)
        for d in nodes[i]["deps"]:
            if any(k.get("kind") != "dev" for k in d["dep_kinds"]):
                todo.append(d["pkg"])
    return [by_id[i] for i in seen if i not in members and not ours(by_id[i])]


def ours(pkg: dict) -> bool:
    """A path package of this repository (the root crates the engine uses), not one of
    the patched third-party copies in either vendor folder."""
    if pkg.get("source"):
        return False
    path = Path(pkg["manifest_path"]).resolve()
    return REPO in path.parents and vendor_root(path) is None


def vendor_root(path: Path) -> Path | None:
    """Both workspaces may share a patched dependency kept beside the root crates."""
    return next((root for root in (REPO / "vendor", REPO / "engine" / "vendor")
                 if root in path.parents), None)


def license_texts(pkg: dict) -> list[tuple[str, str]]:
    folder = Path(pkg["manifest_path"]).resolve().parent
    found = licence_files(folder)
    # A vendored workspace (engine/vendor/wgpu-ffx) keeps one licence at its root for its
    # crates: look up to the vendor folder, never past it into NVForever's own licence.
    vendor = vendor_root(folder)
    for parent in folder.parents if not found and vendor is not None else ():
        if parent == vendor:
            break
        found = [((parent.relative_to(vendor) / name).as_posix(), text) for name, text in licence_files(parent)]
        if found:
            break
    # Fonts a package bundles carry their own licences beside them (egui's
    # epaint_default_fonts: fonts/OFL.txt, fonts/UFL.txt), which the font licences require to
    # ship with the fonts.
    fonts = folder / "fonts"
    if fonts.is_dir():
        found += [
            (f"fonts/{f.name}", f.read_text(encoding="utf-8", errors="replace").replace("\r\n", "\n").strip())
            for f in sorted(fonts.iterdir())
            if f.is_file() and f.suffix.lower() == ".txt"
        ]
    return found


def licence_files(folder: Path) -> list[tuple[str, str]]:
    return [
        (f.name, f.read_text(encoding="utf-8", errors="replace").replace("\r\n", "\n").strip())
        for f in sorted(folder.iterdir())
        if f.is_file() and LICENSE_FILE.match(f.name)
    ]


def render(pkgs: list[dict], rustc: str, notices: tuple[Notice, ...] = NOTICES) -> tuple[str, list[str]]:
    """The file's text and the packages that name no licence and ship no licence file."""
    lines = [
        "NVForever's programs (fnvtool, nvforever-launcher, nvforever, nvf-viewer) are built from the following",
        "third-party Rust packages. Each is used under the licence it names; where a package",
        "offers a choice, either applies. The source of each is at the address given. The",
        "licence texts found in the packages follow the list.",
        "",
        f"The programs also hold parts of the Rust standard library ({rustc}; MIT OR",
        "Apache-2.0, https://github.com/rust-lang/rust).",
        "",
    ]
    texts: dict[str, list[str]] = {}
    missing = []
    for p in pkgs:
        name = f"{p['name']} {p['version']}"
        source = p.get("repository") or (f"https://crates.io/crates/{p['name']}" if p.get("source") else None)
        found = license_texts(p)
        if not p.get("license") and not found:
            missing.append(name)
        note = "" if found else "; no licence file in the package"
        lines.append(f"- {name}: {p.get('license') or 'see its files'}" + (f" ({source}{note})" if source else note))
        for file, text in found:
            texts.setdefault(text, []).append(f"{name} ({file})")
    if notices:
        lines += ["", "They also hold code translated from these sources, which are not Rust packages:", ""]
    for n in notices:
        lines.append(f"- {n.name} ({n.what}): {n.licence} ({n.source})")
        text = n.file.read_text(encoding="utf-8").replace("\r\n", "\n").strip()
        texts.setdefault(text, []).append(f"{n.name} ({n.file.relative_to(REPO).as_posix()})")
    for text, users in texts.items():
        lines += ["", "=" * 78, "From: " + ", ".join(users), "=" * 78, "", text]
    return "\n".join(lines) + "\n", missing


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--target", required=True, help="target triple the programs are built for")
    ap.add_argument("--out", required=True, type=Path)
    args = ap.parse_args(argv)
    pkgs: dict[tuple[str, str], dict] = {}
    for folder, roots in WORKSPACES:
        for p in third_party(metadata(folder, args.target), roots):
            pkgs.setdefault((p["name"], p["version"]), p)
    rustc = subprocess.run(["rustc", "--version"], check=True, capture_output=True, text=True).stdout.strip()
    text, missing = render([pkgs[k] for k in sorted(pkgs)], rustc)
    args.out.write_text(text, encoding="utf-8", newline="\n")
    print(f"{args.out}: {len(pkgs)} packages")
    if missing:
        print("no licence named by: " + ", ".join(missing), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
