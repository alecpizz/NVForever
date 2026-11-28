"""`data_files`: a scenario that loads a mod the repository cannot ship is skipped, not
failed, on a machine whose Data folder lacks it (#4722); no game assets."""
import argparse
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from smoke import cli, runner  # noqa: E402
from smoke.plan import plan  # noqa: E402
from smoke.scenarios import ScenarioError, load_text  # noqa: E402


def suite(data_files: str = '["JustAssortedMods.esp"]'):
    return load_text("\n".join([
        "[[scenario]]", 'name = "jam"', 'paths = ["tools/smoke/"]',
        'args = ["--play", "--plugins", "FalloutNV.esm", "JustAssortedMods.esp"]',
        "run_seconds = 1", f"data_files = {data_files}", 'expect = ["run finished"]',
    ]))


class DataFilesTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="nvf data files ")
        self.addCleanup(self.temp.cleanup)
        self.data = Path(self.temp.name) / "Data"
        self.data.mkdir()
        (self.data / "FalloutNV.esm").write_bytes(b"")
        self.out = Path(self.temp.name) / "out"
        self.out.mkdir()

    def env(self):
        return runner.Env(repo=Path(self.temp.name), out=self.out, exe=Path("absent.exe"), data=str(self.data), fnvtool=None)

    def test_parsed(self):
        self.assertEqual(suite().scenarios[0].data_files, ["JustAssortedMods.esp"])
        self.assertEqual(suite("[]").scenarios[0].data_files, [])

    def test_absent_file_is_missing_and_present_one_is_not_whatever_its_case(self):
        s = suite().scenarios[0]
        self.assertEqual(runner.missing_data_files(s, self.data), ["JustAssortedMods.esp"])
        (self.data / "justassortedmods.ESP").write_bytes(b"")
        self.assertEqual(runner.missing_data_files(s, self.data), [])

    def test_no_data_folder_given_or_readable_is_not_a_skip_reason_by_itself(self):
        s = suite().scenarios[0]
        self.assertEqual(runner.missing_data_files(s, None), [])
        self.assertEqual(runner.missing_data_files(s, self.data / "gone"), ["JustAssortedMods.esp"])

    def test_run_skips_without_launching(self):
        s = suite()
        launch = plan(s, s.scenarios)[0]
        with mock.patch.object(runner.subprocess, "Popen", side_effect=AssertionError("launched without its mod")) as start:
            verdicts = cli.run_launches([launch], self.env(), None, 1, argparse.Namespace(context=3), lambda _: None)
        start.assert_not_called()
        self.assertEqual([v.status for v in verdicts], ["SKIP"])
        self.assertIn("JustAssortedMods.esp", verdicts[0].reason)

    def test_unknown_key_still_refused(self):
        with self.assertRaises(ScenarioError):
            load_text('[[scenario]]\nname = "x"\npaths = ["a"]\nargs = ["--play"]\ndata_file = ["a.esp"]\n')


if __name__ == "__main__":
    unittest.main()
