"""A tool launch with no fnvtool fails its scenarios with the reason; the run goes on to
the rest (#4740). No game assets, no fnvtool."""
import argparse
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from smoke import cli, runner  # noqa: E402
from smoke.plan import plan  # noqa: E402
from smoke.scenarios import load_text  # noqa: E402

TOOLS = "\n".join(
    f'[[scenario]]\nname = "{n}"\nkind = "tool"\npaths = ["x"]\ncommands = [["info"]]\n' for n in ("first", "second"))


class MissingFnvtoolTests(unittest.TestCase):
    def test_every_tool_scenario_fails_with_the_reason_and_none_raises(self):
        with tempfile.TemporaryDirectory(prefix="nvf no fnvtool ") as temp:
            root = Path(temp)
            absent = root / "target" / "release" / "fnvtool.exe"
            env = runner.Env(repo=root, out=root, exe=None, data="data", fnvtool=absent)
            suite = load_text(TOOLS)
            said: list[str] = []
            verdicts = cli.run_launches(plan(suite, suite.scenarios), env, None, 1, argparse.Namespace(context=3), said.append)
        self.assertEqual([(v.scenario, v.status) for v in verdicts], [("first", "FAIL"), ("second", "FAIL")])
        text = "\n".join(said)
        self.assertIn("cannot start fnvtool at", text)
        self.assertIn(str(Path("target", "release", "fnvtool.exe")).replace("\\", "\\\\"), text)


if __name__ == "__main__":
    unittest.main()
