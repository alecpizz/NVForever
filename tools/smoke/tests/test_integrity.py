"""Self-tests for the scenario file's merge checks (smoke/integrity.py, merge_driver.py)
and `expect_file`. No game data.

Run:  python -m unittest discover -s tools/smoke/tests     (from the repo root)
"""
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(HERE))

import merge_driver  # noqa: E402
from smoke import integrity  # noqa: E402
from smoke.expect import ScenarioError  # noqa: E402
from smoke.scenarios import load, load_text  # noqa: E402


def game(name, *lines):
    body = "".join(f"  {x},\n" for x in lines)
    return f'[[scenario]]\nname = "{name}"\nsource = "s"\nareas = ["x"]\nrun_seconds = 3\nexpect = [\n{body}]\n\n'


def tool(name, *lines):
    body = "".join(f"  {x},\n" for x in lines)
    return f'[[scenario]]\nname = "{name}"\nkind = "tool"\nsource = "s"\nareas = ["x"]\ncommands = [["a"]]\nexpect = [\n{body}]\n\n'


A = game("a", "{ re = 'one', label = \"one\" }", "{ re = 'two', label = \"two\" }")
B = game("b", "{ re = 'three', label = \"three\" }")
T = tool("t", '{ json = "summary.stubs", equals = 3 }')


class FileRules(unittest.TestCase):
    def test_repo_file_is_healthy(self):
        path = HERE / "scenarios.toml"
        self.assertEqual(integrity.problems(path.read_text(encoding="utf-8"), HERE), [])

    def test_healthy_sample(self):
        self.assertEqual(integrity.problems(A + B + T), [])

    def test_log_lines_spliced_into_a_tool_scenario(self):
        bad = tool("t", '{ json = "summary.stubs", equals = 3 }', "{ re = 'one', label = \"one\" }")
        self.assertIn("t: a tool scenario holds log-line expectations", integrity.problems(A + bad)[0])

    def test_json_spliced_into_a_game_scenario(self):
        bad = game("g", "{ re = 'x' }", '{ json = "summary.stubs", equals = 3 }')
        self.assertIn("g: a game scenario holds json expectations", integrity.problems(bad)[0])

    def test_doubled_tables(self):
        twice = game("c", "{ re = 'one', label = \"one\" }", "{ re = 'two', label = \"two\" }")
        found = integrity.problems(A + twice)
        self.assertEqual(len(found), 1)
        self.assertIn("appear again", found[0])

    def test_repeated_short_lines_are_allowed(self):
        seq = game("s", '"opened"', '"closed"', '"opened"', '"closed"')
        self.assertEqual(integrity.problems(seq + game("u", '"opened"', '"closed"', '"done"')), [])

    def test_lost_boundary_does_not_parse(self):
        fused = A.replace("]\n\n", "", 1) + B.replace("[[scenario]]\n", "", 1)
        with self.assertRaises(ValueError):
            integrity.problems(fused)


class MergeRules(unittest.TestCase):
    def test_both_sides_add(self):
        self.assertEqual(integrity.merge_problems(A, A + B, A + T, A + B + T), [])

    def test_splice_into_an_unchanged_scenario(self):
        spliced = game("a", "{ re = 'one', label = \"one\" }", "{ re = 'two', label = \"two\" }", "{ re = 'x' }")
        found = integrity.merge_problems(A + B, A + B, A + B + T, spliced + B + T)
        self.assertEqual(found, ["a: differs from the side that changed it (expect)"])

    def test_lost_scenario(self):
        self.assertEqual(integrity.merge_problems(A, A + B, A + T, A + T), ["b: lost in the merge"])

    def test_removed_on_one_side(self):
        self.assertEqual(integrity.merge_problems(A + B, A, A + B + T, A + T), [])
        self.assertEqual(integrity.merge_problems(A + B, A, A + B + T, A + B + T),
                         ["b: in the merge result but removed (or never there) on both sides"])

    def test_changed_differently_on_both_sides_is_the_mergers(self):
        a1, a2, a3 = (game("a", f"{{ re = '{x}' }}") for x in ("1", "2", "3"))
        self.assertEqual(integrity.merge_problems(A, a1, a2, a3), [])
        self.assertEqual(integrity.merge_problems(A + B, a1 + B, a2 + B, B),
                         ["a: changed on both sides and lost in the merge"])

    def test_unparsable_result(self):
        self.assertIn("does not parse", integrity.merge_problems(A, A, A + B, A + B[:-30])[0])


class Driver(unittest.TestCase):
    def setUp(self):
        if not shutil.which("git"):
            self.skipTest("no git")
        self.dir = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.dir, True)

    def merge(self, base, ours, theirs):
        paths = []
        for name, text in (("O", base), ("A", ours), ("B", theirs)):
            (self.dir / name).write_text(text, encoding="utf-8")
            paths.append(str(self.dir / name))
        rc = merge_driver.main(*paths)
        return rc, (self.dir / "A").read_text(encoding="utf-8")

    def test_clean_merge_passes(self):
        rc, out = self.merge(A + T, A + B + T, A + T.replace("equals = 3", "equals = 2"))
        self.assertEqual(rc, 0)
        self.assertEqual(out, A + B + T.replace("equals = 3", "equals = 2"))

    def test_both_change_one_line_conflicts(self):
        rc, out = self.merge(A + T, A + T.replace("3", "2"), A + T.replace("3", "1"))
        self.assertEqual(rc, 1)
        self.assertIn("<<<<<<<", out)


class ExpectFile(unittest.TestCase):
    def test_values_become_json_expectations(self):
        d = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, d, True)
        (d / "totals.toml").write_text("[summary]\nstubs = 3\n[summary.by_status]\nno_op = 4\n", encoding="utf-8")
        text = tool("t").replace("expect = [\n]", 'expect_file = "totals.toml"')
        s = load_text(text, d).scenarios[0]
        self.assertEqual([(".".join(e.json_path), e.equals) for e in s.expect],
                         [("summary.stubs", 3), ("summary.by_status.no_op", 4)])
        with self.assertRaises(ScenarioError):
            load_text(text)  # no folder to read it from
        with self.assertRaises(ScenarioError):
            load_text(text.replace("totals", "missing"), d)

    def test_script_stubs_reads_its_totals(self):
        s = load(HERE / "scenarios.toml").by_name()["script-stubs"]
        got = {".".join(e.json_path) for e in s.expect}
        self.assertEqual(got, {"summary.stubs", "summary.by_status.assigned", "summary.by_status.implementable", "summary.by_status.needs_system",
                               "summary.by_status.no_op", "summary.stub_calls", "summary.stub_condition_uses",
                               "summary.unhandled_used", "summary.unnamed_called"})
        self.assertTrue(all(e.has_equals for e in s.expect))


if __name__ == "__main__":
    unittest.main()
