#!/usr/bin/env python3
"""Stage source/licence notices for the Android binary, never game content."""
from __future__ import annotations
import argparse
import importlib.util
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("nvf_release_notices", ROOT / "tools/release/third_party_licenses.py")
notices = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = notices
spec.loader.exec_module(notices)


def stage(output: Path, ndk: Path) -> int:
    output.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(ROOT / "LICENSE", output / "LICENSE.txt")
    shutil.copyfile(ROOT / "LEGAL.md", output / "LEGAL.txt")
    metadata = notices.metadata(ROOT / "engine", "aarch64-linux-android")
    packages = notices.third_party(metadata, ("nvf-android",))
    rustc = subprocess.check_output(["rustc", "--version"], text=True).strip()
    text, missing = notices.render(sorted(packages, key=lambda p: (p["name"], p["version"])), rustc)
    if missing:
        raise ValueError("Missing dependency licence: " + ", ".join(missing))
    (output / "THIRD-PARTY-LICENSES.txt").write_text(text, encoding="utf-8")
    # The shared libc++ runtime is copied from this exact NDK, outside Cargo's graph.
    for name in ("NOTICE", "NOTICE.toolchain"):
        source = ndk / name
        if not source.is_file():
            raise ValueError(f"NDK runtime notices not found: {source}")
        shutil.copyfile(source, output / ("NDK-" + name + ".txt"))
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    (output / "SOURCE.txt").write_text(
        "NVForever is GPL-3.0-or-later. Corresponding source and build instructions:\n"
        f"https://github.com/LootJunkieAzza/NVForever/tree/{revision}\n"
        "Android build instructions are in docs/android.md. No game content is included.\n",
        encoding="utf-8")
    return len(packages)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--ndk", type=Path, required=True)
    args = parser.parse_args()
    print(f"Android notices: {stage(args.out, args.ndk)} dependencies")
