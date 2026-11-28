"""Strict original SVG family profiles, semantic routing, fills and review artifacts."""
import copy
import importlib.util
import json
from pathlib import Path
import shutil
import tempfile
import unittest
import xml.etree.ElementTree as ET

spec = importlib.util.spec_from_file_location("svg_icons", Path(__file__).resolve().parents[1] / "svg_icons.py")
svg = importlib.util.module_from_spec(spec)
spec.loader.exec_module(svg)
MANIFEST = svg.DEFAULT_MANIFEST
MANIFEST_DATA = json.loads(MANIFEST.read_text(encoding="utf-8"))


class SvgTests(unittest.TestCase):
    style = MANIFEST_DATA["families"][0]

    def source(self, name="pause", family="precise-line"):
        return (MANIFEST.parent / family / (name + ".svg")).read_bytes()

    def parse(self, data, label="Pause", style=None):
        return svg.parse_svg(data, label, style or self.style)

    def test_four_families_have_same_27_stable_semantic_routes(self):
        families, digest = svg.compile_manifest(MANIFEST)
        self.assertEqual([f["id"] for f in families], ["precise-line", "bold-filled", "rounded", "industrial"])
        self.assertEqual(len(digest), 64)
        expected = ["pause", "pip-boy", "vats", "reload", "jump", "interact", "aim", "fire", "sneak", "menu-up", "menu-down", "menu-left", "menu-right", "confirm", "back", "tab-next", "page-up", "page-down", "settings", "help", "sprint", "quickwheel", "close", "companion", "aid", "block", "gyro"]
        for family in families:
            self.assertEqual([i["id"] for i in family["icons"]], expected)
            self.assertEqual(len({json.dumps(i["primitives"], sort_keys=True) for i in family["icons"]}), 27)
        self.assertEqual([i["legacyIndex"] for i in MANIFEST_DATA["icons"]], list(range(27)))

    def test_primary_family_drawings_differ_in_geometry_not_just_stroke_or_fill(self):
        families, _ = svg.compile_manifest(MANIFEST)
        for key in ["fire", "aim", "sneak", "jump", "pip-boy", "vats", "quickwheel", "companion", "aid", "block", "gyro"]:
            geometry = []
            for family in families:
                icon = next(i for i in family["icons"] if i["id"] == key)
                geometry.append(json.dumps([{k:v for k,v in p.items() if k not in ("stroke", "fill")} for p in icon["primitives"]], sort_keys=True))
            with self.subTest(icon=key):
                self.assertEqual(len(set(geometry)), 4)

    def test_generation_is_deterministic_and_records_all_explicit_paint_flags(self):
        families, digest = svg.compile_manifest(MANIFEST)
        text = svg.rust_geometry(families, digest)
        self.assertEqual(text, svg.rust_geometry(*svg.compile_manifest(MANIFEST)))
        self.assertIn(digest, text)
        self.assertEqual(text.count("IconFamily {"), 4)
        self.assertEqual(text.count("IconSource {"), 108)
        self.assertIn("fill: true, stroke: false", text)
        self.assertIn("fill: false, stroke: true", text)
        self.assertNotIn("64 * 64", text)

    def test_committed_icons_generated_rs_matches_svg_sources(self):
        families, digest = svg.compile_manifest(MANIFEST)
        expected = svg.rust_geometry(families, digest)
        actual = svg.DEFAULT_OUT.read_text(encoding="utf-8").replace("\r\n", "\n")
        self.assertEqual(
            actual,
            expected,
            "engine/nvforever/src/touch/icons_generated.rs is out of sync with assets/touch; "
            "run: python tools/android/svg_icons.py --out engine/nvforever/src/touch/icons_generated.rs",
        )

    def test_sources_match_exact_family_style_and_original_licence(self):
        for family in MANIFEST_DATA["families"]:
            for icon in MANIFEST_DATA["icons"]:
                data = self.source(icon["id"], family["id"])
                with self.subTest(family=family["id"], icon=icon["id"]):
                    self.assertEqual(ET.fromstring(data).attrib, svg.root_attributes(family))
                    self.assertIn(b"SPDX-License-Identifier: GPL-3.0-or-later -->", data)
                    self.assertIn(b"Original NVForever geometry", data)
                    self.assertLess(len(data), svg.MAX_SOURCE_BYTES)

    def test_only_explicit_absolute_supported_path_commands(self):
        for path in ["M 5 5 A 5 5 0 0 1 20 20", "M5 5 L 10 10", "m 5 5 l 10 10", "L 5 5", "M 5 5", "M 5 5 L 6", "M 5 5 L nan 10"]:
            with self.subTest(path=path), self.assertRaises(ValueError):
                svg.path_commands(path)

    def test_style_and_paint_extensions_are_rejected(self):
        data = self.source()
        for before, after in [(b'viewBox="0 0 32 32"', b'viewBox="0 0 64 64"'), (b'stroke-width="1.6"', b'stroke-width="2"'), (b'fill="none"', b'fill="white"'), (b'fill-rule="evenodd"', b'fill-rule="nonzero"'), (b'stroke="currentColor"', b'stroke="red"'), (b'stroke-linecap="round"', b'stroke-linecap="butt"'), (b'<path ', b'<path opacity="0.5" '), (b'<path ', b'<path stroke-width="8" ')]:
            with self.subTest(after=after), self.assertRaises(ValueError):
                self.parse(data.replace(before, after))
        for field, value in [("fill", "url(#external)"), ("strokeWidth", 3.1), ("strokeWidth", float("nan")), ("strokeWidth", True), ("strokeLinecap", "butt"), ("strokeLinejoin", "miter")]:
            changed = {**self.style, field:value}
            with self.subTest(field=field), self.assertRaises(ValueError):
                svg.validate_style(changed)

    def test_malformed_xml_entities_scripts_references_and_ignored_semantics(self):
        data = self.source()
        cases = [data[:-10], b"", b"x" * (svg.MAX_SOURCE_BYTES + 1), data.replace(b'<svg ', b'<svg onload="bad()" ', 1), data.replace(b'</svg>', b'<script>alert(1)</script></svg>'), data.replace(b'</svg>', b'<use href="file:///external.svg"/></svg>'), data.replace(b'</svg>', b'<path d="M 5 5 L 10 10" transform="scale(2)"/></svg>'), data.replace(b'</svg>', b'<style>path{stroke:red}</style></svg>'), data.replace(b'</svg>', b'<g><path d="M 5 5 L 10 10"/></g></svg>'), data.replace(b'<title>Pause', b'<title>&external;'), b'<!DOCTYPE svg [<!ENTITY external SYSTEM "file:///external">]>' + data, b'<?xml-stylesheet href="file:///external"?>' + data, data.replace(b'</svg>', b'<text x="8" y="8">text</text></svg>'), data.replace(b'Pause', b'Other'), data.replace(b'</svg>', b'junk</svg>')]
        for i, case in enumerate(cases):
            with self.subTest(case=i), self.assertRaises(ValueError):
                self.parse(case)

    def test_bounds_include_paint_width_and_square_cap_extension(self):
        for value in ["nan", "inf", "1e999", "-1", "32.1"]:
            with self.subTest(value=value), self.assertRaises(ValueError):
                svg.number(value)
        for values in [[0.0], [2.0], [30.0], [32.0]]:
            with self.assertRaises(ValueError):
                svg.painted_bounds(values, self.style, True)
        industrial = MANIFEST_DATA["families"][3]
        with self.assertRaises(ValueError):
            svg.painted_bounds([2.5], industrial, True)
        svg.painted_bounds([2.5], self.style, True)
        with self.assertRaises(ValueError):
            svg.path_commands("M 5 5 " + "L 10 10 " * svg.MAX_COMMANDS)
        with self.assertRaises(ValueError):
            self.parse(self.source().replace(b'</svg>', b'<circle cx="16" cy="16" r="14"/></svg>'))
        with self.assertRaises(ValueError):
            self.parse(self.source().replace(b'</svg>', b'<circle cx="16" cy="16" r="2"/>' * svg.MAX_PRIMITIVES + b'</svg>'))

    def test_filled_holes_are_explicit_compound_paths_and_no_hairline_strokes(self):
        bold = MANIFEST_DATA["families"][1]
        aim = self.parse(self.source("aim", "bold-filled"), "Aim", bold)
        self.assertTrue(all(p["fill"] and not p["stroke"] for p in aim))
        self.assertGreaterEqual(sum(c == "M" for c, _ in aim[0]["commands"]), 6)
        data = self.source("pause", "bold-filled")
        for before, after in [(b' Z"', b'"'), (b'<path ', b'<path stroke="currentColor" '), (b'<path ', b'<path fill="none" ')]:
            with self.subTest(after=after), self.assertRaises(ValueError):
                self.parse(data.replace(before, after), style=bold)

    def test_all_owned_geometry_types_compile_with_explicit_paint(self):
        data = self.source();start = data.index(b'<title>');end = data.index(b'</svg>')
        data = data[:start] + b'<title>Pause</title><line x1="5" y1="5" x2="20" y2="20"/><polyline points="5,5 10,10 20,5"/><polygon points="5,5 10,10 20,5" fill="currentColor"/>' + data[end:]
        geometry = self.parse(data)
        self.assertEqual(len(geometry), 3)
        self.assertTrue(geometry[-1]["fill"])
        self.assertTrue(geometry[-1]["stroke"])
        self.assertEqual(geometry[-1]["commands"][-1], ("Z", []))

    def test_manifest_identity_paths_counts_and_duplicate_families(self):
        with tempfile.TemporaryDirectory(prefix="nvf-svg-tests-") as name:
            root = Path(name)
            for style in MANIFEST_DATA["families"]:
                shutil.copytree(MANIFEST.parent / style["directory"], root / style["directory"])
            mutations = [[], {**MANIFEST_DATA, "schema": True}, {**MANIFEST_DATA, "unknown": 1}, {**MANIFEST_DATA, "icons": []}, {**MANIFEST_DATA, "icons": [None]}, {**MANIFEST_DATA, "families": MANIFEST_DATA["families"] * 3}]
            for field, value in [("file", "../pause.svg"), ("legacyIndex", True), ("legacyIndex", 9), ("id", "../pause"), ("label", "<script>"), ("unknown", 1)]:
                mutation = copy.deepcopy(MANIFEST_DATA);mutation["icons"][0][field] = value;mutations.append(mutation)
            mutation = copy.deepcopy(MANIFEST_DATA);mutation["families"][1] = mutation["families"][0];mutations.append(mutation)
            mutation = copy.deepcopy(MANIFEST_DATA);mutation["families"][0]["directory"] = "../outside";mutations.append(mutation)
            mutation = copy.deepcopy(MANIFEST_DATA);mutation["icons"][1].update({"id":"pause", "file":"pause.svg", "label":"Pause"});mutations.append(mutation)
            path = root / "manifest.json"
            for i, mutation in enumerate(mutations):
                path.write_text(json.dumps(mutation), encoding="utf-8")
                with self.subTest(case=i), self.assertRaises(ValueError):
                    svg.compile_manifest(path)
            path.write_text(json.dumps(MANIFEST_DATA), encoding="utf-8")
            (root / "precise-line/pause.svg").write_bytes(b"x" * (svg.MAX_SOURCE_BYTES + 1))
            with self.assertRaises(ValueError):
                svg.compile_manifest(path)

    def test_preview_has_actual_24_32_48_sizes_and_owned_synthetic_backdrops(self):
        for family in MANIFEST_DATA["families"]:
            for backdrop in ("light", "dark", "complex"):
                text = svg.preview_svg(MANIFEST, family["id"], backdrop)
                root = ET.fromstring(text)
                groups = [e for e in root.findall(svg.NS + "g") if "data-icon-size" in e.attrib]
                self.assertEqual(len(groups), 81)
                self.assertEqual({e.attrib["data-icon-size"] for e in groups}, {"24", "32", "48"})
                self.assertIn("synthetic backdrop / not a game capture", text)
                self.assertIn("GPL-3.0-or-later", text)
                self.assertEqual(text, svg.preview_svg(MANIFEST, family["id"], backdrop))


if __name__ == "__main__":
    unittest.main()
