"""Self-tests for the smoke runner: parsing, selection, launch merging, expectation
matching and locks. Sample logs only, no game data.

Run:  python -m unittest discover -s tools/smoke/tests     (from the repo root)
"""
import io
import json
import os
import subprocess
import sys
import tempfile
import time
import unittest
from contextlib import redirect_stdout
from pathlib import Path

HERE = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(HERE))

from smoke import choose, cli, front  # noqa: E402
from smoke.expect import Expect, ScenarioError, check_forbid, check_one, check_order  # noqa: E402
from smoke.plan import merge, plan, shift  # noqa: E402
from smoke.runner import FROM_SCRIPTS, Locks, Queue, aid_free_environ, game_env, judge  # noqa: E402
from smoke.scenarios import Timed, load, load_text  # noqa: E402

LOG = [
    "2026-10-11T10:00:00.000000Z  INFO nvforever: world index built in 1.20s",
    '2026-10-11T10:00:03.000000Z  INFO nvf_play::corner_messages: corner message: "A" for 2 s, icon x.dds (0 waiting)',
    '2026-10-11T10:00:05.250000Z  INFO nvf_play::corner_messages: corner message: "B" for 10 s, icon default (0 waiting)',
    "2026-10-11T10:00:06.000000Z  INFO nvforever::alert: alert: 1EFiendViolet 000F570B Alert → Combat: detects the player, 2,651 units away",
    "2026-10-11T10:00:07.000000Z  INFO nvforever::speech_look: speech look: line 0010A1E6 over: SunnyRef has the player +2° off its front",
    "2026-10-11T10:00:08.000000Z  INFO nvforever::speech_look: speech look: line 0010A1E7 over: SunnyRef has CheyenneREF -3° off its front",
    "2026-10-11T10:00:09.000000Z  INFO nvforever::music: music summary: 1 started, 0 changes of layers, 0 files not played",
    "2026-10-11T10:00:10.000000Z  INFO nvforever: run finished after 40.0s: 9 cells loaded",
]

GAME = """
[[scenario]]
name = "{name}"
paths = ["engine/"]
args = {args}
run_seconds = {run}
env = {env}
{extra}
"""


def game(name, args='["--cell", "X"]', run=20, env="{}", extra=""):
    return GAME.format(name=name, args=args, run=run, env=env, extra=extra)


TIMED = """
untimed = ["NVF_PLAY_TEST_SCRIPT_MENUS"]
[timed.NVF_PLAY_TEST_SCRIPT]
sep = ","
[timed.NVF_PLAY_TEST_TALK]
sep = ";"
[timed.NVF_PLAY_TEST_PAUSE]
sep = ","
shift = "all"
"""


class ExpectTests(unittest.TestCase):
    def test_text_with_ellipsis(self):
        e = Expect.parse("music summary: 1 started, … 0 files not played")
        self.assertTrue(check_one(e, LOG).ok)
        self.assertFalse(check_one(Expect.parse("music summary: 2 started"), LOG).ok)

    def test_text_is_literal(self):
        self.assertFalse(check_one(Expect.parse("index built in 1x20s"), LOG).ok)
        self.assertTrue(check_one(Expect.parse("index built in 1.20s"), LOG).ok)

    def test_regex_and_counts(self):
        self.assertTrue(check_one(Expect.parse({"re": "corner message:", "count": 2}), LOG).ok)
        o = check_one(Expect.parse({"re": "corner message:", "count": 3}), LOG)
        self.assertFalse(o.ok)
        self.assertIn("2 lines", o.detail)
        self.assertTrue(check_one(Expect.parse({"re": "speech look:", "min": 1, "max": 2}), LOG).ok)
        self.assertFalse(check_one(Expect.parse({"re": "speech look:", "max": 1}), LOG).ok)
        self.assertTrue(check_one(Expect.parse({"re": "no such", "count": 0}), LOG).ok)

    def test_range_any_and_all(self):
        e = Expect.parse({"re": r"Combat: detects the player.*?(?P<n>[\d,]+) units", "range": [1987, 3313]})
        self.assertTrue(check_one(e, LOG).ok)
        e = Expect.parse({"re": r"Combat: detects the player.*?(?P<n>[\d,]+) units", "range": [1, 100]})
        o = check_one(e, LOG)
        self.assertFalse(o.ok)
        self.assertIn("2651", o.detail)
        each = {"re": r"over: SunnyRef has .* (?P<n>[+-]\d+)° off its front", "range": [-2, 2]}
        self.assertTrue(check_one(Expect.parse(each), LOG).ok)  # one line is in range
        self.assertFalse(check_one(Expect.parse(dict(each, all=True)), LOG).ok)  # not every line
        self.assertTrue(check_one(Expect.parse(dict(each, range=[-5, 5], all=True)), LOG).ok)

    def test_gap(self):
        e = Expect.parse({"gap": ['corner message: "A"', 'corner message: "B"'], "range": [1.7, 2.8]})
        o = check_one(e, LOG)
        self.assertTrue(o.ok, o.detail)
        e = Expect.parse({"gap": ['corner message: "A"', 'corner message: "B"'], "range": [3, 4]})
        self.assertFalse(check_one(e, LOG).ok)

    def test_order(self):
        seq = [Expect.parse('corner message: "A"'), Expect.parse('corner message: "B"'), Expect.parse("run finished")]
        self.assertTrue(all(o.ok for o in check_order(seq, LOG)))
        bad = [Expect.parse('corner message: "B"'), Expect.parse('corner message: "A"')]
        out = check_order(bad, LOG)
        self.assertFalse(out[-1].ok)
        self.assertEqual(out[-1].what, "order")

    def test_forbid(self):
        self.assertTrue(check_forbid(Expect.parse("Lost → Combat"), LOG).ok)
        o = check_forbid(Expect.parse("Alert → Combat"), LOG)
        self.assertFalse(o.ok)
        self.assertIn("Alert → Combat", o.context[0])

    def test_missing_shows_similar_lines(self):
        o = check_one(Expect.parse("music summary: 5 started"), LOG)
        self.assertEqual(o.what, "missing")
        self.assertTrue(o.context and "music summary" in o.context[0])
        self.assertNotIn("2026-10-11T", o.context[0])  # timestamps cut

    def test_bad_entries(self):
        with self.assertRaises(ScenarioError):
            Expect.parse({"re": "x", "range": [1, 2]})  # no group
        with self.assertRaises(ScenarioError):
            Expect.parse({"re": "("})
        with self.assertRaises(ScenarioError):
            Expect.parse({"txt": "x"})
        with self.assertRaises(ScenarioError):
            Expect.parse(3)


class JsonExpectTests(unittest.TestCase):
    OUT = [
        'warning: something on stderr {not json',
        '{"schema":"fnv.nav.walk/1","summary":{"outcome":"arrived","seconds":11.7}}',
        '{"schema":"fnv.nav.walk/1","summary":{"outcome":"stuck","seconds":3.0}}',
        '{"schema":"fnv.actor.detect/1","situations":[{"situation":"sneaking, lit","detected_out_to":1840.0},'
        '{"situation":"sneaking, dark","detected_out_to":null}],"actors":[{"actor":"c","factions":[1,2,3,4]}]}',
        '{"format":"fnv.script.trace/1","observations":[{"label":"final: Q 80","stage":80,"done.80":true,"var.a.b":2}]}',
    ]

    def ok(self, item):
        return check_one(Expect.parse(item), self.OUT).ok

    def test_paths_filters_and_counts(self):
        self.assertTrue(self.ok({"json": "summary.outcome", "equals": "arrived"}))
        self.assertFalse(self.ok({"json": "summary.outcome", "equals": "arrived", "all": True}))
        self.assertFalse(self.ok({"json": "summary.outcome", "equals": "arrived", "count": 2}))
        self.assertTrue(self.ok({"json": "summary.outcome", "schema": "fnv.nav.walk/1", "count": 2}))
        self.assertTrue(self.ok({"json": "summary.seconds", "range": [11, 12]}))
        self.assertTrue(self.ok({"json": "situations.?situation=sneaking, lit.detected_out_to", "equals": 1840.0}))
        self.assertTrue(self.ok({"json": "situations.?situation=sneaking, dark.detected_out_to", "null": True}))
        self.assertFalse(self.ok({"json": "situations.?situation=sneaking, lit.detected_out_to", "null": True}))
        self.assertTrue(self.ok({"json": "situations.*.situation", "count": 2}))
        self.assertTrue(self.ok({"json": "actors.?actor=c.factions.#", "equals": 4}))
        self.assertTrue(self.ok({"json": "observations.?label=final: Q 80.done.80", "equals": True}))
        self.assertTrue(self.ok({"json": "observations.?label=final: Q 80.var.a.b", "equals": 2}))
        self.assertTrue(self.ok({"json": "observations.?label=final: Q 80.stage", "equals": 80}))
        o = check_one(Expect.parse({"json": "nope.x", "equals": 1}), self.OUT)
        self.assertEqual((o.ok, o.what), (False, "missing"))
        o = check_one(Expect.parse({"json": "summary.outcome", "equals": "lost"}), self.OUT)
        self.assertIn('"arrived"', o.detail)

    def test_json_only_in_expect(self):
        with self.assertRaises(ScenarioError):
            load_text('[[scenario]]\nname = "t"\nkind = "tool"\npaths = ["x"]\ncommands = [["a"]]\n'
                      'forbid = [{ json = "a", equals = 1 }]\n')


class ScenarioFileTests(unittest.TestCase):
    def test_game_scenario_expects_run_finished(self):
        s = load_text(game("a")).scenarios[0]
        self.assertEqual(s.expect[0].label, "run finished")

    def test_game_scenario_can_end_another_way(self):
        s = load_text(game("a", extra='ends = "the new run is up"\n')).scenarios[0]
        self.assertEqual(s.expect[0].label, "the new run is up")
        self.assertFalse(any(e.label == "run finished" for e in s.expect))

    def test_errors(self):
        with self.assertRaises(ScenarioError):
            load_text(game("a", run=0))
        with self.assertRaises(ScenarioError):
            load_text(game("a", extra='colour = "red"'))
        with self.assertRaises(ScenarioError):
            load_text(game("a") + game("a"))
        with self.assertRaises(ScenarioError):
            load_text(game("a", args='["--run-seconds", "3"]'))
        with self.assertRaises(ScenarioError):
            load_text('[[scenario]]\nname = "t"\nkind = "tool"\npaths = ["x"]\n')
        with self.assertRaises(ScenarioError):
            load_text('[[scenario]]\nname = "t"\nrun_seconds = 3\n')  # cannot be selected
        with self.assertRaises(ScenarioError):
            load_text('bogus = 1\n' + game("a"))

    def test_launch_groups_are_validated_at_load(self):
        text = TIMED + game("a", extra='launch = "L"') + game("b", args='["--cell", "Y"]', extra='launch = "L"')
        with self.assertRaises(ScenarioError):
            load_text(text)

    def test_repo_scenario_file(self):
        suite = load(HERE / "scenarios.toml")
        self.assertGreater(len(suite.scenarios), 40)
        for s in suite.scenarios:
            self.assertTrue(s.source, s.name)
            self.assertTrue(s.expect or s.order or s.manual, s.name)

    def test_repo_globs_match_files(self):
        """Every trigger glob names something that exists (catches typos and renames)."""
        def ls(*extra):
            r = subprocess.run(["git", "-C", str(HERE), "ls-files", "--full-name", *extra, ":/"],
                               capture_output=True, text=True)
            if r.returncode != 0:
                self.skipTest("no git checkout")
            return r.stdout.split()

        files = ls() + ls("--others", "--exclude-standard")
        suite = load(HERE / "scenarios.toml")
        dead = [(s.name, g) for s in suite.scenarios for g in s.paths if not any(choose.matches(f, [g]) for f in files)]
        dead += [(c.name, g) for c in suite.checks for g in c.paths if not any(choose.matches(f, [g]) for f in files)]
        self.assertEqual(dead, [])


class ExeTests(unittest.TestCase):
    def test_the_newer_of_release_and_release_quick(self):
        with tempfile.TemporaryDirectory() as d:
            t = Path(d)
            self.assertEqual(cli.newest_build(t, "x.exe"), t / "release" / "x.exe")
            for sub, when in (("release", 1000), ("release-quick", 2000)):
                (t / sub).mkdir()
                (t / sub / "x.exe").write_text("")
                os.utime(t / sub / "x.exe", (when, when))
            self.assertEqual(cli.newest_build(t, "x.exe"), t / "release-quick" / "x.exe")
            os.utime(t / "release" / "x.exe", (3000, 3000))
            self.assertEqual(cli.newest_build(t, "x.exe"), t / "release" / "x.exe")


class SelectionTests(unittest.TestCase):
    def test_globs(self):
        m = choose.matches
        self.assertTrue(m("engine/nvf-play/src/wait.rs", ["engine/nvf-play/src/wait*"]))
        self.assertTrue(m("engine/nvf-play/src/wait/model.rs", ["engine/nvf-play/src/wait*/**"]))
        self.assertFalse(m("engine/nvf-play/src/wait/model.rs", ["engine/nvf-play/src/wait*"]))
        self.assertTrue(m("crates/fnv-ai/src/x/y.rs", ["crates/fnv-ai/"]))
        self.assertTrue(m("a/b/c.rs", ["**/c.rs"]))
        self.assertTrue(m("c.rs", ["**/c.rs"]))
        self.assertFalse(m("crates/fnv-aix/src/a.rs", ["crates/fnv-ai/"]))
        self.assertTrue(m("engine\\nvforever\\src\\sky.rs", ["engine/nvforever/src/sky*"]))
        self.assertFalse(m("engine/nvforever/src/skyline/x.rs", ["engine/nvforever/src/sky?rs"]))

    def test_changed_selection_and_checks(self):
        suite = load_text(TIMED + game("a") + game("b", extra='paths = ["docs/x.md"]').replace(
            'paths = ["engine/"]\n', "", 1) + """
[[check]]
name = "engine clippy"
paths = ["engine/", "crates/"]
cmd = ["cargo", "clippy"]
[[check]]
name = "lab"
paths = ["parity-lab/"]
cmd = ["python"]
""")
        hits = choose.by_changes(suite.scenarios, ["engine/nvforever/src/main.rs", "README.md"])
        self.assertEqual([s.name for s, _ in hits], ["a"])
        self.assertEqual([c.name for c in choose.checks_for(suite.checks, ["crates/fnv-esm/src/lib.rs"])],
                         ["engine clippy"])
        self.assertEqual(choose.checks_for(suite.checks, ["docs/x.md"]), [])

    def test_changed_files_from_git(self):
        with tempfile.TemporaryDirectory() as d:
            def git(*a):
                subprocess.run(["git", "-C", d, *a], check=True, capture_output=True)
            git("init", "-q", "-b", "main")
            git("config", "user.email", "t@example.invalid")
            git("config", "user.name", "t")
            (Path(d) / "a.txt").write_text("1")
            git("add", "a.txt")
            git("commit", "-qm", "one")
            git("branch", "base")
            (Path(d) / "b.txt").write_text("2")
            git("add", "b.txt")
            git("commit", "-qm", "two")
            (Path(d) / "a.txt").write_text("changed")
            (Path(d) / "new.txt").write_text("untracked")
            self.assertEqual(choose.changed_files(Path(d), "base"), ["a.txt", "b.txt", "new.txt"])

    def test_area(self):
        suite = load_text(game("a", extra='areas = ["Audio"]') + game("b"))
        self.assertEqual([s.name for s in choose.by_area(suite.scenarios, ["audio"])], ["a"])


class PlanTests(unittest.TestCase):
    def test_shift(self):
        self.assertEqual(shift("3:menu:1074,4.5:raise", Timed(), 18), "21:menu:1074,22.5:raise")
        self.assertEqual(shift("20,28", Timed(), 2), "22,30")
        self.assertEqual(shift("66:69", Timed(shift="all"), 1), "67:70")
        self.assertEqual(shift("18", Timed(shift="value"), 2), "20")
        self.assertEqual(shift("10:GSChetRef=for sale|leave", Timed(sep=";"), 5), "15:GSChetRef=for sale|leave")

    def test_merge_joins_timed_and_keeps_shared(self):
        text = TIMED + game("a", env='{ NVF_PLAY_TEST_SCRIPT_MENUS = "0.3", NVF_PLAY_TEST_SCRIPT = "3:x" }',
                            extra='launch = "L"') \
            + game("b", run=30, env='{ NVF_PLAY_TEST_SCRIPT_MENUS = "0.3", NVF_PLAY_TEST_SCRIPT = "3:x,9:y" }',
                   extra='launch = "L"\noffset = 0') \
            + game("c", run=10, env='{ NVF_PLAY_TEST_SCRIPT = "1:z", NVF_PLAY_TEST_SCRIPT_MENUS = "0.3" }',
                   extra='launch = "L"\noffset = 25')
        suite = load_text(text)
        [x] = plan(suite, suite.scenarios)
        self.assertEqual(x.name, "L")
        self.assertEqual(x.env["NVF_PLAY_TEST_SCRIPT"], "3:x,9:y,26:z")  # same item once, c moved by 25
        self.assertEqual(x.run_seconds, 35)  # c: 10 + 25
        self.assertEqual([s.name for s in x.scenarios], ["a", "b", "c"])
        alone = plan(suite, suite.scenarios, no_merge=True)
        self.assertEqual([(y.name, y.run_seconds) for y in alone], [("a", 20), ("b", 30), ("c", 10)])
        self.assertEqual(alone[2].env["NVF_PLAY_TEST_SCRIPT"], "1:z")  # no offset when alone
        part = plan(suite, [suite.scenarios[2]])
        self.assertEqual((part[0].name, part[0].env["NVF_PLAY_TEST_SCRIPT"]), ("c", "1:z"))

    def test_conflicts(self):
        s = load_text(TIMED + game("a", env='{ NVF_PLAY_TEST_SCRIPT_MENUS = "1" }')
                      + game("b", env='{ NVF_PLAY_TEST_SCRIPT_MENUS = "0.3" }')).scenarios
        with self.assertRaises(ScenarioError):
            merge("L", s, {})
        s = load_text(game("a", env='{ NVF_PLAY_TEST_GOD = "1" }') + game("b")).scenarios
        x = merge("L", s, {})
        self.assertEqual(x.notes, ["NVF_PLAY_TEST_GOD=1 also applies to b"])
        s = load_text(game("a") + game("b", env='{ NVF_PLAY_TEST_NEAR = "1" }', extra="offset = 3")).scenarios
        with self.assertRaises(ScenarioError):
            merge("L", s, {}, untimed=set())  # an offset member with an aid of unknown kind


class MessagesTests(unittest.TestCase):
    """`--play` scenarios get --no-messages unless `messages = true` (smoke/scenarios.py)."""

    def test_play_gets_no_messages(self):
        [a] = load_text(game("a", args='["--cell", "X", "--play"]')).scenarios
        self.assertEqual(a.args, ["--cell", "X", "--play", "--no-messages"])
        self.assertFalse(a.messages)
        # Given already: not twice.
        [b] = load_text(game("b", args='["--play", "--no-messages", "--cell", "X"]')).scenarios
        self.assertEqual(b.args.count("--no-messages"), 1)
        # Not a --play run: untouched.
        [c] = load_text(game("c")).scenarios
        self.assertEqual(c.args, ["--cell", "X"])

    def test_messages_true_keeps_the_boxes(self):
        [a] = load_text(game("a", args='["--cell", "X", "--play"]', extra="messages = true")).scenarios
        self.assertEqual(a.args, ["--cell", "X", "--play"])
        self.assertTrue(a.messages)
        for bad in [
            game("a", args='["--play", "--no-messages"]', extra="messages = true"),  # contradicts itself
            game("a", extra="messages = true"),  # no --play
            game("a", args='["--play"]', extra='messages = "yes"'),
            '[[scenario]]\nname = "t"\nkind = "tool"\npaths = ["x"]\ncommands = [["a"]]\nmessages = true\n',
        ]:
            with self.assertRaises(ScenarioError):
                load_text(bad)


class FrontTests(unittest.TestCase):
    """`front = true`: the launch's window held in front (smoke/front.py)."""

    def test_parse_and_merge(self):
        [a] = load_text(game("a", extra="front = true")).scenarios
        self.assertTrue(a.front)
        self.assertFalse(load_text(game("b")).scenarios[0].front)
        with self.assertRaises(ScenarioError):
            load_text(game("a", extra='front = "yes"'))
        tool = '[[scenario]]\nname = "t"\nkind = "tool"\npaths = ["x"]\ncommands = [["a"]]\nfront = true\n'
        with self.assertRaises(ScenarioError):
            load_text(tool)
        [x] = plan(load_text(game("a", extra="front = true")), load_text(game("a", extra="front = true")).scenarios)
        self.assertTrue(x.front)
        both = load_text(game("a", extra='launch = "L"\nfront = true') + game("b", extra='launch = "L"\nfront = true'))
        self.assertTrue(plan(both, both.scenarios)[0].front)
        with self.assertRaises(ScenarioError):
            load_text(game("a", extra='launch = "L"\nfront = true') + game("b", extra='launch = "L"'))

    def test_front_launches_run_last_on_their_own(self):
        suite = load_text(game("a") + game("b", extra="front = true") + game("c"))
        rest, fronts = front.split(plan(suite, suite.scenarios))
        self.assertEqual([x.name for x in rest], ["a", "c"])
        self.assertEqual([x.name for x in fronts], ["b"])
        # A chain (one NVFOREVER_HOME) with a front launch in it moves whole, in its order.
        chained = load_text(game("a", env='{ NVFOREVER_HOME = "h" }') + game("b", env='{ NVFOREVER_HOME = "h" }', extra="front = true") + game("c"))
        launches = plan(chained, chained.scenarios)
        rest, fronts = front.split(launches, cli.chains(launches))
        self.assertEqual(([x.name for x in rest], [x.name for x in fronts]), (["c"], ["a", "b"]))

    def test_what_it_did_goes_to_its_own_file(self):
        class Fake:
            def windows(self):
                return [(4, 7, True, False)]

            def foreground(self):
                return 1

            def bring_forward(self, hwnd):
                pass

        with tempfile.TemporaryDirectory() as d:
            log = Path(d) / "x.front.txt"
            holder = front.Front(7, api=Fake(), log=log)
            holder.once()
            holder.stop()
            text = log.read_text(encoding="utf-8")
            self.assertIn("front: process 7's window found", text)
            self.assertIn("front: brought forward (1)", text)
            self.assertEqual(front.last_line(log).split(" s ", 1)[1], "front: window held in front, brought forward 1 time(s) in 1 looks")
            missing = front.Front(8, api=Fake(), log=Path(d) / "y.front.txt")
            missing.once()
            missing.stop()
            self.assertIn("never found", front.last_line(Path(d) / "y.front.txt"))
        self.assertIsNone(front.last_line(Path("no-such-dir") / "z.front.txt"))

    def test_only_the_processs_own_window_is_brought_forward(self):
        class Fake:
            def __init__(self, fg):
                self.fg, self.brought = fg, []

            def windows(self):
                # (handle, process, visible, owned): another process's, an invisible one, an
                # owned dialog, then the game's main window.
                return [(1, 99, True, False), (2, 7, False, False), (3, 7, True, True), (4, 7, True, False)]

            def foreground(self):
                return self.fg

            def bring_forward(self, hwnd):
                self.brought.append(hwnd)
                self.fg = hwnd

        self.assertEqual(front.pick_window(7, Fake(1).windows()), 4)
        self.assertIsNone(front.pick_window(8, Fake(1).windows()))
        api = Fake(1)
        holder = front.Front(7, api=api)
        self.assertTrue(holder.once())
        self.assertTrue(holder.once())
        self.assertEqual(api.brought, [4], "brought forward once, then already in front")
        self.assertFalse(front.Front(8, api=api).once(), "no window of that process")
        holder = front.Front(7, period=0.01, api=Fake(1)).start()
        time.sleep(0.05)
        holder.stop()
        self.assertGreaterEqual(holder.brought, 1)


class JudgeTests(unittest.TestCase):
    def suite(self, extra):
        return load_text(TIMED + game("a", extra=extra)).scenarios[0]

    def test_pass_fail_known(self):
        s = self.suite('expect = ["music summary: 1 started"]')
        self.assertEqual(judge(s, LOG, 0).status, "PASS")
        self.assertEqual(judge(s, LOG, 3).status, "FAIL")
        self.assertEqual(judge(s, LOG, None, "timed out").status, "FAIL")
        s = self.suite('expect = [{ text = "music summary: 9 started", known_failing = "bug 1" }]')
        v = judge(s, LOG, 0)
        self.assertEqual(v.status, "KNOWN")
        s = self.suite('expect = [{ text = "music summary: 1 started", known_failing = "bug 1" }]')
        v = judge(s, LOG, 0)
        self.assertEqual((v.status, v.xpass), ("PASS", ["music summary: 1 started"]))
        s = self.suite('order = [["run finished", "music summary"]]')
        self.assertEqual(judge(s, LOG, 0).status, "FAIL")

    def known(self, reason='known_failing = { linux = "bug 1" }'):
        return self.suite(f'expect = [{{ text = "music summary: 9 started", {reason} }}]')

    def test_platform_mark_is_known_only_where_named(self):
        s = self.known()
        v = judge(s, LOG, 0, platform="linux")
        self.assertEqual((v.status, [o.known for o in v.failures]), ("KNOWN", ["bug 1"]))
        v = judge(s, LOG, 0, platform="windows")
        self.assertEqual((v.status, [o.known for o in v.failures], v.xpass), ("FAIL", [None], []))
        self.assertEqual(v.failures[0].what, "missing")

    def test_platform_mark_reports_a_pass_only_where_named(self):
        s = self.suite('expect = [{ text = "music summary: 1 started", known_failing = { linux = "bug 1" } }]')
        v = judge(s, LOG, 0, platform="linux")
        self.assertEqual((v.status, v.xpass), ("PASS", ["music summary: 1 started"]))
        v = judge(s, LOG, 0, platform="windows")
        self.assertEqual((v.status, v.xpass), ("PASS", []))

    def test_plain_string_is_known_on_every_platform(self):
        s = self.suite('expect = [{ text = "music summary: 9 started", known_failing = "bug 1" }]')
        for platform in ("linux", "windows", "macos"):
            self.assertEqual(judge(s, LOG, 0, platform=platform).status, "KNOWN", platform)

    def test_platform_mark_syntax(self):
        e = Expect.parse({"text": "x", "known_failing": {"linux": "why"}})
        self.assertEqual((e.known_reason("linux"), e.known_reason("windows")), ("why", None))
        self.assertEqual(e.known_reason("macos"), None)
        with self.assertRaises(ScenarioError):
            Expect.parse({"text": "x", "known_failing": {"linnux": "why"}})
        with self.assertRaises(ScenarioError):
            Expect.parse({"text": "x", "known_failing": {}})
        with self.assertRaises(ScenarioError):
            Expect.parse({"text": "x", "known_failing": {"linux": ""}})


class RoySayPauseTests(unittest.TestCase):
    def setUp(self):
        self.scenario = load(HERE / "scenarios.toml").by_name()["roy-say-pause-resume"]

    def test_say_and_pause_inputs_share_the_world_clock(self):
        # [design, #2714: nvf-play/script.rs, test_aid.rs] SCRIPT and KEYS use
        # world_secs; PAUSE uses elapsed_secs and can open before a late Say.
        launch, = plan(load(HERE / "scenarios.toml"), [self.scenario])
        env = game_env(launch)
        self.assertEqual(env[FROM_SCRIPTS], "1")
        self.assertNotIn("NVF_PLAY_TEST_PAUSE", env)
        say_at = float(env["NVF_PLAY_TEST_SCRIPT"].split(":", 1)[0])
        pauses = [item.split(":") for item in env["NVF_PLAY_TEST_KEYS"].split(",")]
        self.assertEqual([key for _, key in pauses], ["esc", "esc"])
        opened, resumed = [float(at) for at, _ in pauses]
        self.assertEqual(opened - say_at, 1)
        self.assertEqual(resumed - opened, 5)

    def test_subtitle_clear_is_required_after_completion(self):
        # [design, #2714] Authored trace of the final two transitions only; no
        # subtitle words or voice assets from the game are needed for this check.
        final = self.scenario.order[1][-2:]
        ended = "script: VFSRoyREF ended line 0x0011361F"
        cleared = "subtitle: cleared"
        self.assertTrue(all(o.ok for o in check_order(final, [ended, cleared])))
        self.assertFalse(all(o.ok for o in check_order(final, [ended])))
        self.assertFalse(all(o.ok for o in check_order(final, [cleared, ended])))


class GameEnvTests(unittest.TestCase):
    def test_play_runs_count_the_world_aids_from_the_scripts(self):
        suite = load_text(game("p", args='["--cell", "X", "--play"]', env='{ NVF_PLAY_TEST_CONSOLE = "3:tgm" }')
                          + game("v", env='{ NVF_PLAY_TEST_SHOTS = "3" }')
                          + game("old", args='["--play"]', env=f'{{ {FROM_SCRIPTS} = "0" }}'))
        p, v, old = plan(suite, suite.scenarios)
        self.assertEqual(game_env(p), {"NVF_PLAY_TEST_CONSOLE": "3:tgm", FROM_SCRIPTS: "1"})
        self.assertNotIn(FROM_SCRIPTS, game_env(v))  # no --play: no scripts to wait for
        self.assertEqual(game_env(old)[FROM_SCRIPTS], "0")  # a scenario keeps the old clock
        self.assertNotIn(FROM_SCRIPTS, p.env)  # the plan itself is unchanged

    def test_the_callers_switch_does_not_leak_in(self):
        os.environ[FROM_SCRIPTS] = "1"
        try:
            self.assertNotIn(FROM_SCRIPTS, aid_free_environ({}))
        finally:
            del os.environ[FROM_SCRIPTS]


class PlatformTests(unittest.TestCase):
    """`platforms`: a scenario naming another system is skipped before anything is started,
    and the parse refuses a name the runner does not know, so a typo cannot silently skip
    the scenario everywhere (or run it where it fails for want of the platform's code)."""

    def run_suite(self, extra, here):
        import argparse
        from unittest import mock
        from smoke.runner import Env
        suite = load_text(game("gated", extra=extra))
        launches = plan(suite, suite.scenarios)
        said = []
        with tempfile.TemporaryDirectory() as d:
            env = Env(repo=Path(d), out=Path(d), exe=Path(d) / "nvforever.exe", data="D", fnvtool=None)
            def fake_run(x, e, locks):
                (e.out / f"{x.name}.log").write_text("\n".join(LOG), encoding="utf-8")
                return 0, "", 1.0

            with mock.patch.object(cli, "run_launch", side_effect=fake_run) as run, \
                    mock.patch("smoke.runner.platform_name", return_value=here):
                verdicts = cli.run_launches(launches, env, None, 1, argparse.Namespace(context=3), said.append)
        return verdicts, run, said

    def test_another_system_skips_and_starts_nothing(self):
        verdicts, run, said = self.run_suite('platforms = ["linux"]', "windows")
        self.assertEqual([(v.scenario, v.status) for v in verdicts], [("gated", "SKIP")])
        self.assertEqual(verdicts[0].reason, "linux only")
        run.assert_not_called()
        self.assertTrue(said[0].startswith("SKIP  gated"), said)

    def test_the_same_system_runs(self):
        verdicts, run, said = self.run_suite('platforms = ["linux"]', "linux")
        self.assertEqual([(v.scenario, v.status) for v in verdicts], [("gated", "PASS")])
        run.assert_called_once()

    def test_no_platforms_runs_anywhere(self):
        verdicts, run, said = self.run_suite("", "windows")
        self.assertEqual([(v.scenario, v.status) for v in verdicts], [("gated", "PASS")])
        run.assert_called_once()

    def test_an_unknown_platform_is_refused(self):
        with self.assertRaisesRegex(ScenarioError, "platforms takes"):
            load_text(game("gated", extra='platforms = ["linx"]'))

    def test_repo_native_crash_scenarios_are_gated_each_to_its_own_side(self):
        """The signal line is Linux's, the minidump Windows': on the owner's Windows smoke
        machine the Linux one would end with 0xC0000005 and fail both its expectations, and
        every later change to crash.rs would select it there again."""
        suite = load(HERE / "scenarios.toml")
        by_name = suite.by_name()
        self.assertEqual(by_name["native-crash-signal"].platforms, ["linux"])
        self.assertEqual(by_name["native-crash-dump"].requires, ["WINDIR"])
        self.assertEqual(by_name["native-crash-dump"].platforms, [])


class RequiresTests(unittest.TestCase):
    """`requires`: a scenario whose environment variable is unset or empty is skipped before
    anything is started; with it set the scenario runs and is judged as usual (the DLSS
    scenarios, NVF_DLSS_DIR)."""

    VAR = "NVF_SMOKE_SELFTEST_REQUIRED_DIR"

    def setUp(self):
        self.saved = os.environ.pop(self.VAR, None)

    def tearDown(self):
        os.environ.pop(self.VAR, None)
        if self.saved is not None:
            os.environ[self.VAR] = self.saved

    def run_suite(self, fake_run):
        import argparse
        from unittest import mock
        from smoke.runner import Env
        suite = load_text(game("gated", extra=f'requires = ["{self.VAR}"]\nexpect = ["music summary: 1 started"]'))
        launches = plan(suite, suite.scenarios)
        said = []
        with tempfile.TemporaryDirectory() as d:
            env = Env(repo=Path(d), out=Path(d), exe=Path(d) / "nvforever.exe", data="D", fnvtool=None)
            with mock.patch.object(cli, "run_launch", side_effect=lambda x, e, locks: fake_run(x, e)) as run, \
                    mock.patch("subprocess.Popen", side_effect=AssertionError("a process was started")):
                verdicts = cli.run_launches(launches, env, None, 1, argparse.Namespace(context=3), said.append)
        return verdicts, run, said

    def never(self, x, e):
        raise AssertionError("a skipped scenario was launched")

    def test_an_absent_variable_skips_and_starts_nothing(self):
        verdicts, run, said = self.run_suite(self.never)
        self.assertEqual([(v.scenario, v.status) for v in verdicts], [("gated", "SKIP")])
        self.assertEqual(verdicts[0].reason, f"set {self.VAR}")
        run.assert_not_called()
        self.assertTrue(said[0].startswith("SKIP  gated"), said)

    def test_an_empty_variable_skips_too(self):
        os.environ[self.VAR] = ""
        verdicts, run, _ = self.run_suite(self.never)
        self.assertEqual([v.status for v in verdicts], ["SKIP"])
        run.assert_not_called()

    def test_with_the_variable_it_runs_and_a_failure_stays_a_failure(self):
        os.environ[self.VAR] = "C:/somewhere"

        def passing(x, e):
            (e.out / f"{x.name}.log").write_text("\n".join(LOG), encoding="utf-8")
            return 0, "", 1.0

        def failing(x, e):
            (e.out / f"{x.name}.log").write_text("\n".join(LOG[:-1]), encoding="utf-8")  # no "run finished"
            return 3, "", 1.0

        verdicts, run, _ = self.run_suite(passing)
        self.assertEqual([v.status for v in verdicts], ["PASS"])
        run.assert_called_once()
        verdicts, run, _ = self.run_suite(failing)
        self.assertEqual([v.status for v in verdicts], ["FAIL"])
        run.assert_called_once()

    def test_a_negated_variable_skips_when_it_is_set_and_runs_when_it_is_not(self):
        import argparse
        from unittest import mock
        from smoke.runner import Env
        suite = load_text(game("barred", extra=f'requires = ["!{self.VAR}"]\nexpect = ["music summary: 1 started"]'))
        launches = plan(suite, suite.scenarios)

        def run(fake):
            with tempfile.TemporaryDirectory() as d:
                env = Env(repo=Path(d), out=Path(d), exe=Path(d) / "nvforever.exe", data="D", fnvtool=None)
                with mock.patch.object(cli, "run_launch", side_effect=lambda x, e, locks: fake(x, e)):
                    return cli.run_launches(launches, env, None, 1, argparse.Namespace(context=3), lambda _: None)

        def passing(x, e):
            (e.out / f"{x.name}.log").write_text("\n".join(LOG), encoding="utf-8")
            return 0, "", 1.0

        self.assertEqual([v.status for v in run(passing)], ["PASS"])
        os.environ[self.VAR] = ""
        self.assertEqual([v.status for v in run(passing)], ["PASS"])
        os.environ[self.VAR] = "1"
        verdicts = run(self.never)
        self.assertEqual([(v.status, v.reason) for v in verdicts], [("SKIP", f"unset {self.VAR}")])

    def test_repo_developer_aid_scenarios(self):
        """A scenario of the volumetric clouds or the light in the air that sets a developer aid (a debug variable a release
        build does not read: engine/nvforever/src/dev_aids.rs) says which build it is for."""
        suite = load(HERE / "scenarios.toml")
        aids = ("NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS", "NVF_PLAY_DEBUG_MEDIUM", "NVF_PLAY_DEBUG_FAIR_SUN", "NVF_PLAY_DEBUG_CLOUD_SHADOWS_SOURCE",
                "NVF_PLAY_DEBUG_SOLID_CORES", "NVF_PLAY_TEST_SHOT_DIFF")
        using = [s for s in suite.scenarios if any(k.startswith(aids) for k in s.env)]
        self.assertTrue(using)
        for s in using:
            if s.name in ("volumetric-clouds-no-aids", "volumetric-light-no-aids"):
                self.assertEqual(s.requires, ["!NVF_SMOKE_DEV_AIDS"])
            else:
                self.assertEqual(s.requires, ["NVF_SMOKE_DEV_AIDS"], s.name)

    def test_the_variable_reaches_the_game_unless_the_scenario_sets_its_own(self):
        os.environ[self.VAR] = "C:/bundle"
        self.assertEqual(aid_free_environ({})[self.VAR], "C:/bundle")
        self.assertEqual(aid_free_environ({self.VAR: "elsewhere"})[self.VAR], "elsewhere")

    def test_repo_dlss_scenarios(self):
        """Every DLSS hardware scenario waits for NVF_DLSS_DIR and inherits it; dlss-no-files
        runs everywhere on the empty fixture, which holds none of DLSS's four files."""
        suite = load(HERE / "scenarios.toml")
        # The download's scenarios name folders of their own (test_repo_dlss_download_scenarios).
        dlss = {s.name: s for s in suite.scenarios if s.name.startswith("dlss-") and not s.name.startswith("dlss-download-")}
        self.assertIn("dlss-no-files", dlss)
        self.assertGreaterEqual(len(dlss), 9)
        for name, s in dlss.items():
            if name == "dlss-no-files":
                self.assertEqual(s.requires, [])
            else:
                self.assertEqual(s.requires, ["NVF_DLSS_DIR"], name)
                self.assertNotIn("NVF_DLSS_DIR", s.env, name)
        free = dlss["dlss-no-files"]
        self.assertEqual(free.env["NVF_DLSS_PRELOAD"], "0")
        folder = Path(free.env["NVF_DLSS_DIR"].replace("{repo}", str(HERE.parent.parent)))
        self.assertTrue(folder.is_dir(), folder)
        for f in ("sl.interposer.dll", "sl.common.dll", "sl.dlss.dll", "nvngx_dlss.dll"):
            self.assertFalse((folder / f).exists(), f)


    def test_repo_dlss_download_scenarios(self):
        """Download DLSS... (#2250): every scenario names its own DLSS folder under target/
        whose name starts smoke-dlss- (the only folders the aid's clear empties); the ones
        that need no network download from this machine only, the real ones wait for
        NVF_DLSS_DOWNLOAD_REAL."""
        suite = load(HERE / "scenarios.toml")
        down = {s.name: s for s in suite.scenarios if s.name.startswith("dlss-download-")}
        self.assertEqual(set(down), {"dlss-download-decline", "dlss-download-offline", "dlss-download-404",
                                     "dlss-download-real", "dlss-download-real-restart"})
        for name, s in down.items():
            folder = s.env["NVF_DLSS_DIR"]
            self.assertTrue(folder.startswith("{repo}/target/smoke-dlss-"), name)
            if name.startswith("dlss-download-real"):
                self.assertEqual(s.requires, ["NVF_DLSS_DOWNLOAD_REAL"], name)
                self.assertNotIn("NVF_DLSS_DOWNLOAD_URL", s.env, name)
            else:
                self.assertEqual(s.requires, [], name)
                self.assertRegex(s.env["NVF_DLSS_DOWNLOAD_URL"], r"^(http://127\.0\.0\.1:\d+|\{serve\})/", name)
        self.assertEqual(down["dlss-download-404"].serve, {"status": 404, "body": ""})
        self.assertEqual(down["dlss-download-real"].env["NVFOREVER_HOME"], down["dlss-download-real-restart"].env["NVFOREVER_HOME"])
        # Every one runs in a home of its own, never the player's (the real pair shares one).
        homes = {s.env["NVFOREVER_HOME"] for s in down.values()}
        self.assertEqual(len(homes), len(down) - 1)


class LockTests(unittest.TestCase):
    def test_all_or_none_and_release(self):
        with tempfile.TemporaryDirectory() as d:
            a, b = Path(d) / "x" / "run.lock", Path(d) / "smoke.lock"
            b.mkdir()  # someone else holds the second
            locks = Locks([a, b], poll=0.01, wait_max=0.05, say=lambda s: None)
            self.assertEqual(locks.try_take(), b)
            self.assertFalse(a.exists())  # the first is given back
            with self.assertRaises(TimeoutError):
                locks.acquire()
            b.rmdir()
            with locks:
                self.assertTrue(a.is_dir() and b.is_dir())
            self.assertFalse(a.exists() or b.exists())

    def test_stale_runner_lock_is_cleared_but_not_others(self):
        with tempfile.TemporaryDirectory() as d:
            mine, theirs = Path(d) / "a.lock", Path(d) / "b.lock"
            mine.mkdir()
            (mine / "smoke-owner.txt").write_text("999999999 smoke runner\n")  # no such process
            locks = Locks([mine], poll=0.01, wait_max=0.05, say=lambda s: None)
            self.assertIsNone(locks.try_take())
            locks.release()
            theirs.mkdir()  # a plain mkdir lock of another agent: never cleared
            locks = Locks([theirs], poll=0.01, wait_max=0.05, say=lambda s: None)
            self.assertEqual(locks.try_take(), theirs)
            self.assertTrue(theirs.is_dir())

    def test_a_pid_used_again_does_not_keep_a_lock_held(self):
        from smoke.runner import owner_line, process_birth

        born = process_birth(os.getpid())
        if born is None:
            self.skipTest("no process birth time on this system")
        with tempfile.TemporaryDirectory() as d:
            lock = Path(d) / "smoke.lock"
            lock.mkdir()
            # This process's pid with another birth: the runner that wrote it has ended.
            (lock / "smoke-owner.txt").write_text(f"{os.getpid()} smoke runner 2026-01-01 00:00:00 born={born + 1}\n")
            locks = Locks([lock], poll=0.01, wait_max=0.05, say=lambda s: None)
            self.assertIsNone(locks.try_take())
            self.assertIn(f"born={born}", (lock / "smoke-owner.txt").read_text())
            locks.release()
            # The same pid and birth: a live runner's lock, kept.
            lock.mkdir()
            (lock / "smoke-owner.txt").write_text(owner_line())
            self.assertEqual(Locks([lock], say=lambda s: None).try_take(), lock)
            self.assertTrue((lock / "smoke-owner.txt").exists())

    def test_a_left_lock_is_replaced_only_under_the_mutex(self):
        from smoke.slots import FileLock

        with tempfile.TemporaryDirectory() as d:
            lock = Path(d) / "smoke.lock"
            lock.mkdir()
            (lock / "smoke-owner.txt").write_text("999999999 smoke runner\n")  # no such process
            other = FileLock(Path(d) / "smoke-slots" / "mutex.lock")
            self.assertTrue(other.try_lock())  # another runner is replacing it now
            try:
                locks = Locks([lock], say=lambda s: None)
                self.assertEqual(locks.try_take(), lock)
                self.assertIn("999999999", (lock / "smoke-owner.txt").read_text())  # left alone
            finally:
                other.unlock()
            self.assertIsNone(locks.try_take())
            self.assertIn(str(os.getpid()), (lock / "smoke-owner.txt").read_text())
            locks.release()


    def test_old_mkdir_lock_without_a_game_is_reported_not_removed(self):
        with tempfile.TemporaryDirectory() as d:
            lock = Path(d) / "run.lock"
            lock.mkdir()
            said = []
            locks = Locks([lock], poll=0.01, wait_max=0.05, say=said.append, stale_after=0.0, running=lambda: False)
            with self.assertRaises(TimeoutError):
                locks.acquire()
            self.assertTrue(lock.is_dir())
            self.assertEqual(sum("ABANDONED" in x for x in said), 1)  # once, not every poll
            said.clear()
            busy = Locks([lock], poll=0.01, wait_max=0.05, say=said.append, stale_after=0.0, running=lambda: True)
            with self.assertRaises(TimeoutError):
                busy.acquire()
            self.assertFalse(any("ABANDONED" in x for x in said))  # a game runs: just waiting
            self.assertTrue(any("mkdir lock, made at" in x for x in said))


class QueueTests(unittest.TestCase):
    def test_first_come_first_served(self):
        with tempfile.TemporaryDirectory() as d:
            lock = Path(d) / "run.lock"
            live = {101, 102}
            first = Queue(Path(d) / "smoke-queue", pid=101, alive=lambda p: p in live)
            second = Queue(Path(d) / "smoke-queue", pid=102, alive=lambda p: p in live)
            first.join()
            time.sleep(0.002)
            second.join()
            self.assertEqual(first.ahead(), [])
            self.assertEqual(len(second.ahead()), 1)
            # The second runner does not take the free lock while the first is queued.
            waiting = Locks([lock], poll=0.01, wait_max=0.05, say=lambda s: None, queue=second)
            with self.assertRaises(TimeoutError):
                waiting.acquire()
            self.assertFalse(lock.exists())
            # The first takes it, gives it back between launches and keeps its place.
            ahead = Locks([lock], poll=0.01, wait_max=0.05, say=lambda s: None, queue=first)
            ahead.acquire()
            ahead.release()
            self.assertEqual(len(second.ahead()), 1)
            ahead.finish()
            self.assertEqual(second.ahead(), [])
            waiting.acquire()
            self.assertTrue(lock.is_dir())
            waiting.finish()
            self.assertFalse(lock.exists())
            self.assertEqual(list((Path(d) / "smoke-queue").glob("*.ticket")), [])

    def test_a_dead_runners_ticket_is_cleared(self):
        with tempfile.TemporaryDirectory() as d:
            gone = Queue(Path(d) / "q", pid=7, alive=lambda p: False)
            gone.join()
            time.sleep(0.002)
            me = Queue(Path(d) / "q", pid=8, alive=lambda p: p == 8)
            me.join()
            self.assertEqual(me.ahead(), [])
            self.assertFalse(gone.ticket.exists())


class CheckCommandTests(unittest.TestCase):
    def test_check_rejudges_saved_logs(self):
        with tempfile.TemporaryDirectory() as d:
            sc = Path(d) / "s.toml"
            sc.write_text(game("a", extra='expect = ["music summary: 1 started"]')
                          + game("b", extra='expect = ["music summary: 7 started"]'), encoding="utf-8")
            out = Path(d) / "out"
            out.mkdir()
            for n in ("a", "b"):
                (out / f"{n}.log").write_text("\n".join(LOG), encoding="utf-8")
                (out / f"{n}.exit").write_text(json.dumps({"exit": 0, "error": "", "seconds": 41.0}))
            buf = io.StringIO()
            with redirect_stdout(buf):
                code = cli.main(["check", "--all", "--scenarios", str(sc), "--out", str(out)])
            text = buf.getvalue()
            self.assertEqual(code, 1)
            self.assertIn("PASS  a 41 s", text)
            self.assertIn("FAIL  b", text)
            self.assertIn("music summary: 1 started", text)  # the similar line is shown
            self.assertIn("1 passed, 1 failed", text)
            self.assertLessEqual(len(text.splitlines()), 6)  # compact


@unittest.skipUnless(sys.platform == "win32", "thread stacks are read on Windows only")
class StackTests(unittest.TestCase):
    def test_a_running_process_shows_every_thread_with_frames(self):
        from smoke.stacks import dump
        p = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])
        try:
            time.sleep(0.5)
            text = dump(p.pid)
        finally:
            p.kill()
            p.wait()
        self.assertIn("=== thread", text)
        self.assertRegex(text, r"\n  [0-9a-f]{16} \S")

    def test_a_launch_past_its_time_leaves_its_stacks(self):
        from smoke.plan import Launch
        from smoke.runner import Env, run_launch
        from smoke.scenarios import GAME as GAME_KIND
        s = load_text(game("hangs", extra="timeout = 1")).scenarios[0]
        with tempfile.TemporaryDirectory() as d:
            out = Path(d)
            exe = out / "hangs.cmd"
            exe.write_text(f'@"{sys.executable}" -c "import time; time.sleep(4)"\n')
            launch = Launch(name="hangs", kind=GAME_KIND, scenarios=[s], run_seconds=1)
            code, err, _ = run_launch(launch, Env(repo=out, out=out, exe=exe, data="D", fnvtool=None), None)
            self.assertIsNone(code)
            self.assertIn("thread stacks in", err)
            self.assertIn("=== thread", (out / "hangs.stacks.txt").read_text(encoding="utf-8"))
            time.sleep(4)  # the orphaned sleeper lets go of the log



class ServeTests(unittest.TestCase):
    """`serve`: a loopback HTTP server for one launch, its address in `{serve}` (the DLSS
    download's failure paths without the network)."""

    def test_parse_and_errors(self):
        s = load_text(game("s", env='{ URL = "{serve}/x.zip" }', extra="serve = { status = 404 }")).scenarios[0]
        self.assertEqual(s.serve, {"status": 404, "body": ""})
        for extra in ('serve = { status = 99 }', 'serve = { status = "404" }', 'serve = { status = 404, x = 1 }',
                      'serve = 404', 'serve = { status = 200, body = 3 }'):
            with self.assertRaises(ScenarioError, msg=extra):
                load_text(game("s", extra=extra))
        with self.assertRaises(ScenarioError):  # {serve} without a server
            load_text(game("s", env='{ URL = "{serve}/x.zip" }'))
        with self.assertRaises(ScenarioError):  # a shared launch serving two ways
            load_text(game("a", extra='launch = "L"\nserve = { status = 404 }')
                      + game("b", extra='launch = "L"\nserve = { status = 500 }'))
        suite = load_text(game("a", extra='launch = "L"\nserve = { status = 404 }') + game("b", extra='launch = "L"'))
        self.assertEqual(len(plan(suite, suite.scenarios)), 1)

    def test_the_server_answers_every_request_and_stops(self):
        import urllib.error
        import urllib.request
        from smoke.runner import serving
        with serving({}) as none:
            self.assertIsNone(none)
        with serving({"status": 404, "body": "gone"}) as url:
            self.assertRegex(url, r"^http://127\.0\.0\.1:\d+$")
            with self.assertRaises(urllib.error.HTTPError) as e:
                urllib.request.urlopen(url + "/streamline.zip", timeout=10)
            self.assertEqual(e.exception.code, 404)
            self.assertEqual(e.exception.read(), b"gone")
            e.exception.close()
        with self.assertRaises(OSError):
            urllib.request.urlopen(url + "/again", timeout=2)

    def test_the_launch_gets_the_address(self):
        from smoke.runner import Env, run_launch
        suite = load_text(game("served", env='{ NVF_SMOKE_SELFTEST_URL = "{serve}/x.zip" }',
                               extra="serve = { status = 404 }"))
        launch = plan(suite, suite.scenarios)[0]
        fetch = "\n".join([
            "import os, urllib.request, urllib.error",
            "u = os.environ['NVF_SMOKE_SELFTEST_URL']",
            "try:",
            "    urllib.request.urlopen(u, timeout=10)",
            "except urllib.error.HTTPError as e:",
            "    print('answer', e.code, u)",
            "",
        ])
        with tempfile.TemporaryDirectory() as d:
            out = Path(d)
            (out / "fetch.py").write_text(fetch, encoding="utf-8")
            if os.name == "nt":
                exe = out / "fetch.cmd"
                exe.write_text(f'@"{sys.executable}" "{out / "fetch.py"}"\n')
            else:
                exe = out / "fetch.sh"
                exe.write_text(f'#!/bin/sh\nexec "{sys.executable}" "{out / "fetch.py"}"\n')
                exe.chmod(0o755)
            code, err, _ = run_launch(launch, Env(repo=out, out=out, exe=exe, data="D", fnvtool=None), None)
            self.assertEqual((code, err), (0, ""))
            self.assertRegex((out / "served.log").read_text(encoding="utf-8"),
                             r"answer 404 http://127\.0\.0\.1:\d+/x\.zip")
            self.assertIn("NVF_SMOKE_SELFTEST_URL=http://127.0.0.1:", (out / "served.cmd.txt").read_text())


@unittest.skipUnless(os.name == "nt", "Windows process ids")
class ChildProcessTests(unittest.TestCase):
    """A load's new run outlives the run the runner started (engine/nvforever/src/handoff.rs):
    the runner waits for it, and for nothing it did not start."""

    @unittest.skipUnless(os.name != "nt", "POSIX process groups")
    def test_posix_waits_for_the_launchs_process_group(self):
        from smoke.runner import launch_popen_kw, wait_for_group, wait_rest

        starter = "import subprocess, sys; subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(2)'])"
        p = subprocess.Popen([sys.executable, "-c", starter], **launch_popen_kw())
        p.wait()
        t = time.monotonic()
        self.assertEqual(wait_rest(p, Path(sys.executable).name, t + 20), "")
        self.assertGreater(time.monotonic() - t, 0.5)
        # Past the deadline the group is ended.
        p = subprocess.Popen([sys.executable, "-c", starter.replace("sleep(2)", "sleep(60)")], **launch_popen_kw())
        p.wait()
        self.assertIn("process group", wait_for_group(p.pid, time.monotonic() + 0.3))

    @unittest.skipUnless(os.name == "nt", "Windows finds a launch's children by parent")
    def test_only_the_launchs_own_children(self):
        from smoke.runner import child_pids, own_times, wait_for_children

        exe = Path(sys.executable).name
        starter = "import subprocess, sys; subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(2)'])"
        p = subprocess.Popen([sys.executable, "-c", starter])
        p.wait()
        born = own_times(p)
        kids = child_pids(p.pid, born, exe)
        self.assertEqual(len(kids), 1)
        # Another program's name, or a process made outside the parent's lifetime: not its own.
        self.assertEqual(child_pids(p.pid, born, "nvforever.exe"), [])
        self.assertEqual(child_pids(p.pid, (born[0], born[0] + 1), exe), [])
        t = time.monotonic()
        self.assertEqual(wait_for_children(p.pid, kids, t + 20), "")
        self.assertGreater(time.monotonic() - t, 0.5)


if __name__ == "__main__":
    unittest.main()


class SessionLockedTest(unittest.TestCase):
    """#3277: a locked Windows session (LogonUI.exe running) is told apart."""

    def run_with(self, stdout, code=0):
        from unittest import mock
        from smoke import runner
        done = subprocess.CompletedProcess([], code, stdout=stdout, stderr="")
        with mock.patch.object(runner.os, "name", "nt"), mock.patch.object(runner.subprocess, "run", return_value=done):
            return runner.session_locked()

    def test_logonui_running_is_locked(self):
        self.assertTrue(self.run_with('"LogonUI.exe","1234","Console","1","40,000 K"\n'))

    def test_no_logonui_is_unlocked(self):
        self.assertFalse(self.run_with("INFO: No tasks are running which match the specified criteria.\n"))

    def test_a_failed_list_tells_nothing(self):
        self.assertIsNone(self.run_with("", code=1))

    def test_not_windows_tells_nothing(self):
        from unittest import mock
        from smoke import runner
        with mock.patch.object(runner.os, "name", "posix"):
            self.assertIsNone(runner.session_locked())
