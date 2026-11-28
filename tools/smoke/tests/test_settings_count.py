"""Every settings registry key in `KNOWN` (engine/nvf-play/src/settings_layout.rs) must stay
in exact 1:1 sync with its per-row entry in `docs/settings-reference.md` (#3793), and
`docs/design/settings-page.md` must keep its registry-sync note without a drifting hardcoded
integer count. Pull requests that add or remove a row in `KNOWN` without updating the
reference — or leave a stale row in the reference — fail this test immediately without
building the Rust engine."""

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
LAYOUT = ROOT / "engine" / "nvf-play" / "src" / "settings_layout.rs"
REFERENCE = ROOT / "docs" / "settings-reference.md"
PAGE = ROOT / "docs" / "design" / "settings-page.md"

_CONST_BLOCK_RE = re.compile(
    r"(?:pub(?:\([^)]*\))?\s+)?const\s+(?P<name>[A-Z0-9_]+)\s*:\s*"
    r"(?:&\[\(&str,\s*&str\)\]\s*=\s*&\[|\[\(&str,\s*&str\);\s*\d+\]\s*=\s*\[)"
    r"(?P<body>.*?)\];",
    re.DOTALL,
)
_ENTRY_RE = re.compile(r'\(\s*"([^"]+)"\s*,\s*"([^"]+)"\s*\)')
_HARDCODED_COUNT_RE = re.compile(
    r"The table names every row the registry can hold[^\n]*\n(\d+) with "
)


def parse_const_keys(text: str, const_name: str = "KNOWN") -> list[str]:
    """Extract ordered row keys from a `const <NAME>: &[(&str, &str)]` table."""
    matches = [m for m in _CONST_BLOCK_RE.finditer(text) if m.group("name") == const_name]
    if not matches:
        raise AssertionError(f"no `const {const_name}: &[(&str, &str)]` in settings_layout.rs")
    if len(matches) > 1:
        raise AssertionError(
            f"expected a single `const {const_name}` in settings_layout.rs, found {len(matches)}"
        )
    keys = [m.group(1) for m in _ENTRY_RE.finditer(matches[0].group("body"))]
    if not keys:
        raise AssertionError(f"`const {const_name}` in settings_layout.rs has no entries")
    seen: set[str] = set()
    dups: list[str] = []
    for key in keys:
        if key in seen:
            dups.append(key)
        seen.add(key)
    if dups:
        raise AssertionError(f"duplicate key(s) in `const {const_name}`: {sorted(set(dups))}")
    return keys


def reference_keys(text: str) -> list[str]:
    """Extract ordered `### `<key>`` headings from docs/settings-reference.md."""
    keys: list[str] = []
    seen: set[str] = set()
    dups: list[str] = []
    for line in text.splitlines():
        if line.startswith("## ") and "the tab's own rows" in line:
            break
        if line.startswith("### `"):
            rest = line[len("### `") :]
            key, sep, _ = rest.partition("`")
            if sep and key:
                if key in seen:
                    dups.append(key)
                seen.add(key)
                keys.append(key)
    if not keys:
        raise AssertionError("no `### `<key>`` headings found in docs/settings-reference.md")
    if dups:
        raise AssertionError(
            f"duplicate key(s) in docs/settings-reference.md: {sorted(set(dups))}"
        )
    return keys


def check_key_sync(known: list[str], reference: list[str]) -> tuple[list[str], list[str]]:
    """Return `(undocumented_in_reference, stale_in_reference)`."""
    known_set = set(known)
    ref_set = set(reference)
    undocumented = sorted(known_set - ref_set)
    stale = sorted(ref_set - known_set)
    return undocumented, stale


def check_page_note(text: str) -> None:
    """Verify settings-page.md points to KNOWN and settings-reference.md without a drifting count."""
    if "The table names every row the registry can hold" not in text:
        raise AssertionError(
            "missing registry-sync paragraph ('The table names every row the registry can hold') "
            "in docs/design/settings-page.md"
        )
    m = _HARDCODED_COUNT_RE.search(text)
    if m:
        raise AssertionError(
            f"docs/design/settings-page.md still has hardcoded registry count '{m.group(1)} with'; "
            "rely on key-level sync with docs/settings-reference.md instead"
        )
    if "docs/settings-reference.md" not in text or "`KNOWN`" not in text:
        raise AssertionError(
            "docs/design/settings-page.md must reference both `KNOWN` and docs/settings-reference.md"
        )


class SettingsCount(unittest.TestCase):
    def test_known_keys_match_settings_reference(self):
        layout_text = LAYOUT.read_text(encoding="utf-8")
        ref_text = REFERENCE.read_text(encoding="utf-8")
        known = parse_const_keys(layout_text, "KNOWN")
        ref = reference_keys(ref_text)
        undocumented, stale = check_key_sync(known, ref)
        self.assertEqual(
            (undocumented, stale),
            ([], []),
            "KNOWN in engine/nvf-play/src/settings_layout.rs and docs/settings-reference.md "
            f"disagree: missing from reference={undocumented}, stale in reference={stale}",
        )

    def test_android_only_known_has_no_overlap_with_desktop_known(self):
        layout_text = LAYOUT.read_text(encoding="utf-8")
        desktop = set(parse_const_keys(layout_text, "KNOWN"))
        android_only = set(parse_const_keys(layout_text, "ANDROID_ONLY_KNOWN"))
        overlap = sorted(desktop & android_only)
        self.assertEqual(
            overlap,
            [],
            f"ANDROID_ONLY_KNOWN duplicates desktop KNOWN key(s): {overlap}",
        )

    def test_settings_page_uses_non_brittle_reference_note(self):
        check_page_note(PAGE.read_text(encoding="utf-8"))

    def test_parsers_and_sync_checks(self):
        sample_layout = '''
            pub(super) const KNOWN: &[(&str, &str)] = &[
                ("alpha", "Gameplay"),
                ("beta", "Display"),
            ];
        '''
        self.assertEqual(parse_const_keys(sample_layout, "KNOWN"), ["alpha", "beta"])

        sample_ref = (
            "## Gameplay › Game\n"
            "### `alpha`\n"
            "## Display › Screen\n"
            "### `beta`\n"
            "## Controls — the tab's own rows\n"
            "### `controls-only`\n"
        )
        self.assertEqual(reference_keys(sample_ref), ["alpha", "beta"])
        self.assertEqual(check_key_sync(["alpha", "beta"], ["alpha", "beta"]), ([], []))

        # Catches both undocumented keys added to KNOWN and stale keys left in reference.
        self.assertEqual(
            check_key_sync(["alpha", "beta", "gamma"], ["alpha", "beta"]),
            (["gamma"], []),
        )
        self.assertEqual(
            check_key_sync(["alpha"], ["alpha", "stale-row"]),
            ([], ["stale-row"]),
        )

        # Rejects duplicate keys in KNOWN or reference.
        dup_layout = 'const KNOWN: &[(&str, &str)] = &[("a", "G"), ("a", "G")];'
        with self.assertRaises(AssertionError):
            parse_const_keys(dup_layout, "KNOWN")
        dup_ref = "## Gameplay › Game\n### `a`\n### `a`\n"
        with self.assertRaises(AssertionError):
            reference_keys(dup_ref)

        # Rejects missing paragraph or reintroduced hardcoded count in settings-page.md.
        check_page_note(
            "The table names every row the registry can hold, matching `KNOWN` and "
            "docs/settings-reference.md.\n"
        )
        with self.assertRaises(AssertionError):
            check_page_note("no paragraph here")
        with self.assertRaises(AssertionError):
            check_page_note(
                "The table names every row the registry can hold [x],\n9 with `KNOWN` and "
                "docs/settings-reference.md\n"
            )


if __name__ == "__main__":
    unittest.main()
