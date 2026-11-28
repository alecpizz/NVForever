"""Android staging tests: tracked runtime assets only; no game files or local extras."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import re
import unittest
from unittest.mock import patch

SOURCE = Path(__file__).resolve().parents[1] / "build.py"
spec = importlib.util.spec_from_file_location("android_build", SOURCE)
build = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build)


class StageTests(unittest.TestCase):
    def test_only_tracked_runtime_text_and_classic_casing_are_packaged(self):
        with tempfile.TemporaryDirectory(prefix="nvf-android-stage-") as name:
            root = Path(name)
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            casing = "engine/nvf-play/assets/dialogue/classic.png"
            for path, text in {"schemas/fnv/types.json": "{}", "lang/en.toml": "name='English'",
                               "schemas/local.json": "private", "schemas/image.png": "synthetic",
                               "schemas/game.bsa": "synthetic", casing: "casing fixture",
                               "engine/nvf-play/assets/dialogue/other.png": "not allow-listed"}.items():
                p = root / path
                p.parent.mkdir(parents=True, exist_ok=True)
                p.write_text(text)
            subprocess.run(["git", "add", "schemas/fnv/types.json", "lang/en.toml", "schemas/image.png",
                            "schemas/game.bsa", casing, "engine/nvf-play/assets/dialogue/other.png"],
                           cwd=root, check=True)
            with patch.object(build, "ROOT", root):
                build.stage_assets(root / "staged")
            actual = {str(p.relative_to(root / "staged")).replace("\\", "/") for p in (root / "staged").rglob("*") if p.is_file()}
            self.assertEqual(actual, {"schemas/fnv/types.json", "lang/en.toml", "dialogue/classic.png"})
            self.assertEqual((root / "staged/dialogue/classic.png").read_bytes(), (root / casing).read_bytes())
            self.assertEqual((root / "schemas/local.json").read_text(), "private")

    def test_android_import_requirements_match_the_engine(self):
        rust = (build.ROOT / "crates/fnv-install/src/lib.rs").read_text(encoding="utf-8")
        java = (build.ROOT / "android/app/src/main/java/org/nvforever/android/InstallConfig.java").read_text(encoding="utf-8")
        rust = re.search(r"pub const REQUIRED_DATA_FILES:.*?= \[(.*?)\];", rust, re.S).group(1)
        java = re.search(r"String\[\] REQUIRED = \{(.*?)\};", java, re.S).group(1)
        self.assertEqual(re.findall(r'"([^"\n]+)"', rust), re.findall(r'"([^"\n]+)"', java))

    def test_bad_ndk_fails_before_launching_a_build(self):
        with tempfile.TemporaryDirectory(prefix="nvf-android-ndk-") as name:
            with self.assertRaisesRegex(SystemExit, "compiler not found"):
                build.build_environment(Path(name), Path(name) / "target", 2)


if __name__ == "__main__":
    unittest.main()
