#!/usr/bin/env python3
"""Verify an APK's architecture, runtime assets and absence of game data and the LAN uploader.

Signature verification is separate: use the Android SDK's apksigner verify command.
"""
from pathlib import Path
import hashlib
import json
import re
import struct
import sys
import zipfile

CLASSIC_CASING = "assets/dialogue/classic.png"

REQUIRED = {
    "AndroidManifest.xml", "classes.dex",
    "lib/arm64-v8a/libnvf_android.so", "lib/arm64-v8a/libc++_shared.so",
    "assets/schemas/fnv/common.json", "assets/schemas/nif/core.json",
    "assets/schemas/script/functions.json",
    CLASSIC_CASING,
    "assets/LICENSE.txt", "assets/LEGAL.txt", "assets/THIRD-PARTY-LICENSES.txt",
    "assets/NDK-NOTICE.txt", "assets/NDK-NOTICE.toolchain.txt", "assets/SOURCE.txt",
}
GAME_SUFFIXES = {".esm", ".esp", ".esl", ".bsa", ".dds", ".nif", ".kf", ".bik", ".fnt", ".lip", ".fos", ".ogg", ".wav", ".mp3"}
# Check every DEX, not just classes.dex. These ASCII descriptors and the route
# are stored verbatim in DEX string data, even in multi-DEX APKs.
LAN_DEX_STRINGS = (b"Lorg/nvforever/android/LanDiagnostics;",
                   b"Lorg/nvforever/android/DiagnosticsTransport;", b"/api/v1/log-bundles")


def verify(path: Path) -> dict:
    with zipfile.ZipFile(path) as apk:
        names = set(apk.namelist())
        missing = REQUIRED - names
        if missing:
            raise ValueError(f"APK is missing required files: {sorted(missing)}")
        forbidden = [name for name in names if Path(name).suffix.lower() in GAME_SUFFIXES]
        if forbidden:
            raise ValueError(f"APK contains game-data file names: {sorted(forbidden)}")
        if any(".." in Path(name).parts or name.startswith("/") for name in names):
            raise ValueError("APK contains an unsafe member path")
        for name in sorted(names):
            if re.fullmatch(r"classes(?:[0-9]+)?\.dex", name):
                dex = apk.read(name)
                if any(marker in dex for marker in LAN_DEX_STRINGS):
                    raise ValueError(f"APK contains the LAN reporting uploader: {name}")
        alignments = {}
        for name in sorted(n for n in names if n.endswith(".so")):
            if not name.startswith("lib/arm64-v8a/"):
                raise ValueError(f"Unexpected ABI: {name}")
            with apk.open(name) as library:
                header = library.read(64)
                if len(header) != 64 or header[:6] != b"\x7fELF\x02\x01" or struct.unpack_from("<HH", header, 16) != (3, 183):
                    raise ValueError(f"Not an ARM64 Android shared library: {name}")
                offset = struct.unpack_from("<Q", header, 32)[0]
                size, count = struct.unpack_from("<HH", header, 54)
                if size < 56 or count > 4096:
                    raise ValueError(f"Invalid ELF program header table: {name}")
                library.seek(offset)
                table = library.read(size * count)
                if len(table) != size * count:
                    raise ValueError(f"Truncated ELF program header table: {name}")
                loads = [struct.unpack_from("<Q", table, at + 48)[0] for at in range(0, len(table), size)
                         if len(table) - at >= 56 and struct.unpack_from("<I", table, at)[0] == 1]
                if not loads or min(loads) < 4096:
                    raise ValueError(f"Invalid ELF load alignment: {name}")
                alignments[name] = min(loads)
        # Verify archive CRCs, including classes, native code and schemas.
        bad = apk.testzip()
        if bad:
            raise ValueError(f"Corrupt APK member: {bad}")
        with apk.open(CLASSIC_CASING) as casing:
            casing_digest = hashlib.file_digest(casing, "sha256").hexdigest()
    with path.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    return {"apk": str(path.resolve()), "bytes": path.stat().st_size, "sha256": digest,
            "abi": "arm64-v8a", "elfLoadAlignments": alignments, "gameDataFiles": 0,
            "classicCasingSha256": casing_digest,
            "signature": "Verify separately with apksigner"}


if __name__ == "__main__":
    print(json.dumps(verify(Path(sys.argv[1])), indent=2))
