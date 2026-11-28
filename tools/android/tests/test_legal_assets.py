import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("android_legal_assets", Path(__file__).resolve().parents[1] / "legal_assets.py")
legal = importlib.util.module_from_spec(spec)
spec.loader.exec_module(legal)


class LegalAssetsTests(unittest.TestCase):
    def test_packages_project_dependency_runtime_and_source_notices(self):
        with tempfile.TemporaryDirectory(prefix="nvf-android-legal-") as name:
            root = Path(name)
            ndk = root / "ndk"
            ndk.mkdir()
            (ndk / "NOTICE").write_text("authored NDK notice fixture")
            (ndk / "NOTICE.toolchain").write_text("authored toolchain notice fixture")
            def output(command, **kwargs):
                return "rustc authored-fixture" if command[0] == "rustc" else "deadbeef"
            with patch.object(legal.notices, "metadata", return_value={}), patch.object(legal.notices, "third_party", return_value=[]), patch.object(legal.subprocess, "check_output", side_effect=output):
                self.assertEqual(legal.stage(root / "assets", ndk), 0)
            names = {p.name for p in (root / "assets").iterdir()}
            self.assertEqual(names, {"LICENSE.txt", "LEGAL.txt", "THIRD-PARTY-LICENSES.txt", "NDK-NOTICE.txt", "NDK-NOTICE.toolchain.txt", "SOURCE.txt"})
            self.assertIn("GNU GENERAL PUBLIC LICENSE", (root / "assets/LICENSE.txt").read_text())
            self.assertIn("/tree/deadbeef", (root / "assets/SOURCE.txt").read_text())
            self.assertEqual((root / "assets/NDK-NOTICE.txt").read_text(), "authored NDK notice fixture")

    def test_missing_dependency_licence_refuses_packaging(self):
        with tempfile.TemporaryDirectory(prefix="nvf-android-legal-") as name:
            with patch.object(legal.notices, "metadata", return_value={}), patch.object(legal.notices, "third_party", return_value=[]), patch.object(legal.notices, "render", return_value=("", ["authored-missing-licence"])), patch.object(legal.subprocess, "check_output", return_value="rustc authored-fixture"):
                with self.assertRaisesRegex(ValueError, "Missing dependency licence"):
                    legal.stage(Path(name) / "assets", Path(name))


if __name__ == "__main__":
    unittest.main()
