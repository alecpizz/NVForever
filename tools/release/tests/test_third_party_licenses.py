"""The release notices on synthetic package metadata (no cargo, no build, no network)."""
import re
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import third_party_licenses as tpl  # noqa: E402

STREAMLINE = tpl.NOTICES[0]
# NVIDIA's files a DLSS player supplies, and the updater NVForever refuses: none of them is
# shipped, so the notices must not name them as if they were.
NVIDIA_BINARIES = ("sl.interposer.dll", "sl.common.dll", "sl.dlss.dll", "nvngx_dlss.dll", "nvngx_update.exe", "sl_nvperf.dll")


def package(folder: Path, name: str, licence: str, text: str | None) -> dict:
    """A crates.io package as cargo metadata lists it, its files in `folder`."""
    folder.mkdir(parents=True)
    (folder / "Cargo.toml").write_text(f'[package]\nname = "{name}"\n', encoding="utf-8")
    if text is not None:
        (folder / "LICENSE-MIT").write_text(text, encoding="utf-8")
    return {
        "id": f"{name} 1.0.0",
        "name": name,
        "version": "1.0.0",
        "license": licence,
        "source": "registry+https://github.com/rust-lang/crates.io-index",
        "repository": f"https://example.invalid/{name}",
        "manifest_path": str(folder / "Cargo.toml"),
    }


class StreamlineNotice(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = Path(self.tmp.name)
        self.pkgs = [package(root / "alpha", "alpha", "MIT", "MIT text of alpha"), package(root / "beta", "beta", "Apache-2.0", None)]

    def test_the_licence_file_beside_the_translation_is_streamlines_mit_text(self):
        text = STREAMLINE.file.read_text(encoding="utf-8")
        self.assertEqual(STREAMLINE.file, tpl.REPO / "engine" / "nvforever" / "src" / "dlss" / "LICENSE-MIT")
        self.assertTrue(text.startswith("The FFI declarations in this folder"), "its first line says what it covers")
        self.assertIn("Streamline v2.14.16", text)
        self.assertIn("Copyright (c) 2023 NVIDIA CORPORATION. All rights reserved", text)
        self.assertIn("Permission is hereby granted, free of charge, to any person obtaining a copy", text)
        self.assertIn('THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND', text)
        self.assertEqual((STREAMLINE.licence, STREAMLINE.source), ("MIT", "https://github.com/NVIDIA-RTX/Streamline"))

    def test_the_output_lists_streamline_and_holds_its_whole_text(self):
        out, missing = tpl.render(self.pkgs, "rustc 1.0.0 (test)")
        self.assertEqual(missing, [])
        self.assertIn("- alpha 1.0.0: MIT (https://example.invalid/alpha)", out)
        self.assertIn(
            "- NVIDIA Streamline 2.14.16 (FFI declarations translated from its headers, in engine/nvforever/src/dlss/ and crates/nvf-dlss-files/): MIT "
            "(https://github.com/NVIDIA-RTX/Streamline)",
            out,
        )
        self.assertIn("From: NVIDIA Streamline 2.14.16 (engine/nvforever/src/dlss/LICENSE-MIT)", out)
        whole = STREAMLINE.file.read_text(encoding="utf-8").replace("\r\n", "\n").strip()
        self.assertEqual(out.count(whole), 1, "the notice's full text, once")
        self.assertIn("MIT text of alpha", out, "the packages' texts are still there")

    def test_the_output_names_no_nvidia_binary(self):
        out, _ = tpl.render(self.pkgs, "rustc 1.0.0 (test)")
        lowered = out.lower()
        for binary in NVIDIA_BINARIES:
            self.assertNotIn(binary, lowered)
        self.assertEqual(re.findall(r"\b[\w.-]+\.(?:dll|exe)\b", lowered), [], "no program or library file named at all")

    def test_without_notices_the_output_is_the_packages_alone(self):
        out, _ = tpl.render(self.pkgs, "rustc 1.0.0 (test)", notices=())
        self.assertNotIn("Streamline", out)
        self.assertNotIn("not Rust packages", out)

    def test_main_writes_the_notice_for_every_target(self):
        meta = {
            "packages": [{"id": "root", "name": "nvforever", "version": "0.1.0", "license": "GPL-3.0-or-later", "source": None, "manifest_path": str(tpl.REPO / "engine" / "nvforever" / "Cargo.toml")}, *self.pkgs],
            "workspace_members": ["root"],
            "resolve": {"nodes": [
                {"id": "root", "deps": [{"pkg": p["id"], "dep_kinds": [{"kind": None}]} for p in self.pkgs]},
                *({"id": p["id"], "deps": []} for p in self.pkgs),
            ]},
        }
        for target in ("x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu"):
            with self.subTest(target=target):
                out_file = Path(self.tmp.name) / f"{target}.txt"
                rustc = mock.Mock(stdout="rustc 1.0.0 (test)\n")
                with mock.patch.object(tpl, "WORKSPACES", ((tpl.REPO / "engine", ("nvforever",)),)), \
                        mock.patch.object(tpl, "metadata", return_value=meta), \
                        mock.patch.object(tpl.subprocess, "run", return_value=rustc), \
                        mock.patch("builtins.print"):
                    self.assertEqual(tpl.main(["--target", target, "--out", str(out_file)]), 0)
                text = out_file.read_text(encoding="utf-8")
                self.assertIn("NVIDIA Streamline 2.14.16", text)
                self.assertIn("Permission is hereby granted", text)

    def test_shared_root_vendor_runtime_is_listed_with_its_licence(self):
        repo = Path(self.tmp.name) / "checkout"
        app = package(repo / "engine" / "nvforever", "nvforever", "GPL-3.0-or-later", None)
        own = package(repo / "crates" / "fnv-phys", "fnv-phys", "GPL-3.0-or-later", None)
        shared = package(repo / "vendor" / "rapier3d", "rapier3d", "Apache-2.0", "original shared runtime licence")
        engine = package(repo / "engine" / "vendor" / "gpu-allocator", "gpu-allocator", "MIT", "original engine runtime licence")
        for pkg in [app, own, shared, engine]:
            pkg["source"] = None
        meta = {"packages": [app, own, shared, engine], "workspace_members": [app["id"]],
                "resolve": {"nodes": [
                    {"id": app["id"], "deps": [{"pkg": p["id"], "dep_kinds": [{"kind": None}]} for p in [own, shared, engine]]},
                    *({"id": p["id"], "deps": []} for p in [own, shared, engine])
                ]}}
        with mock.patch.object(tpl, "REPO", repo):
            selected = tpl.third_party(meta, ("nvforever",))
            self.assertEqual({p["name"] for p in selected}, {"rapier3d", "gpu-allocator"})
            out, missing = tpl.render(sorted(selected, key=lambda p: p["name"]), "rustc test", notices=())
        self.assertEqual(missing, [])
        self.assertIn("- rapier3d 1.0.0: Apache-2.0", out)
        self.assertIn("original shared runtime licence", out)
        self.assertIn("original engine runtime licence", out)



class BundledFontLicences(unittest.TestCase):
    def test_a_packages_font_licences_are_listed_with_its_own(self):
        with tempfile.TemporaryDirectory() as tmp:
            pkg = package(Path(tmp) / "fonts-crate", "fonts-crate", "(MIT OR Apache-2.0) AND OFL-1.1", "MIT text")
            fonts = Path(tmp) / "fonts-crate" / "fonts"
            fonts.mkdir()
            (fonts / "OFL.txt").write_text("SIL OPEN FONT LICENSE", encoding="utf-8")
            (fonts / "Font.ttf").write_bytes(b"font data")
            names = [n for n, _ in tpl.license_texts(pkg)]
            self.assertEqual(names, ["LICENSE-MIT", "fonts/OFL.txt"])


if __name__ == "__main__":
    unittest.main()
