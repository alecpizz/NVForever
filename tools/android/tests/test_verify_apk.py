import importlib.util
import hashlib
from pathlib import Path
import struct
import tempfile
import unittest
import zipfile

spec = importlib.util.spec_from_file_location("verify_apk", Path(__file__).resolve().parents[1] / "verify_apk.py")
verify_apk = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify_apk)


def library(machine=183):
    data = bytearray(120)
    data[:6] = b"\x7fELF\x02\x01"
    struct.pack_into("<HH", data, 16, 3, machine)
    struct.pack_into("<Q", data, 32, 64)
    struct.pack_into("<HH", data, 54, 56, 1)
    struct.pack_into("<I", data, 64, 1)
    struct.pack_into("<Q", data, 112, 16384)
    return data


class ApkTests(unittest.TestCase):
    def apk(self, root, *, machine=183, missing=None, extra=None, secondary_dex=None):
        path = Path(root) / "fixture.apk"
        with zipfile.ZipFile(path, "w") as archive:
            for name in verify_apk.REQUIRED - {missing}:
                archive.writestr(name, library(machine) if name.endswith(".so") else b"authored fixture")
            if extra:
                archive.writestr(extra, b"authored fixture")
            if secondary_dex is not None:
                archive.writestr("classes2.dex", secondary_dex)
        return path

    def test_accepts_arm64_runtime_and_reports_alignment(self):
        with tempfile.TemporaryDirectory(prefix="nvf-apk-test-") as root:
            result = verify_apk.verify(self.apk(root))
            self.assertEqual(result["gameDataFiles"], 0)
            self.assertEqual(set(result["elfLoadAlignments"].values()), {16384})
            self.assertEqual(len(result["sha256"]), 64)
            self.assertEqual(result["classicCasingSha256"], hashlib.sha256(b"authored fixture").hexdigest())

    def test_rejects_x86_library_in_arm64_directory(self):
        with tempfile.TemporaryDirectory(prefix="nvf-apk-test-") as root:
            with self.assertRaisesRegex(ValueError, "Not an ARM64"):
                verify_apk.verify(self.apk(root, machine=62))

    def test_rejects_missing_runtime_schema(self):
        with tempfile.TemporaryDirectory(prefix="nvf-apk-test-") as root:
            with self.assertRaisesRegex(ValueError, "missing required"):
                verify_apk.verify(self.apk(root, missing="assets/schemas/script/functions.json"))

    def test_rejects_missing_classic_casing(self):
        with tempfile.TemporaryDirectory(prefix="nvf-apk-test-") as root:
            with self.assertRaisesRegex(ValueError, "dialogue/classic.png"):
                verify_apk.verify(self.apk(root, missing=verify_apk.CLASSIC_CASING))

    def test_rejects_accidental_game_content(self):
        with tempfile.TemporaryDirectory(prefix="nvf-apk-test-") as root:
            with self.assertRaisesRegex(ValueError, "game-data"):
                verify_apk.verify(self.apk(root, extra="assets/forbidden.bsa"))

    def test_rejects_the_lan_uploader_in_secondary_dex(self):
        with tempfile.TemporaryDirectory(prefix="nvf-apk-test-") as root:
            path = self.apk(root, secondary_dex=b"\x00Lorg/nvforever/android/LanDiagnostics;\x00")
            with self.assertRaisesRegex(ValueError, "LAN reporting uploader: classes2.dex"):
                verify_apk.verify(path)


if __name__ == "__main__":
    unittest.main()
