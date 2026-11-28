"""python tools/smoke <command> — the smoke tests of CLAUDE.md as data.

  run    [selection] [--checks] [--no-merge] [--exe X] [--data D] [--fnvtool T]
         [--size WxH] [--extra-args ARGS]
  plan   [selection] [--size WxH] [--extra-args ARGS]
                                what would run, launch by launch (nothing is started)
  check  [selection]            judge the logs of the last run again (no launch)
  list                          every scenario with its areas
  verify [REV]                  the scenario file after a merge: rules a healthy file keeps
                                and, when REV (default HEAD) is a merge commit, that each
                                scenario equals the side that changed it (smoke/integrity.py)

Selection (default --changed origin/main): --changed [BASE] | --all | --only NAME… |
--area AREA… | --failed (the scenarios the last run or check failed, from its
summary.json: fix, rebuild, rerun only those). Logs go to <repo>/target/smoke/ (or --out / NVF_SMOKE_OUT); only a
compact summary is printed. Exit code 1 when a scenario or check fails. A game that
runs past its time leaves every thread's stack in <launch>.stacks.txt before it is
killed (Windows; `python -m smoke.stacks PID` takes them from a game that hangs now).
Every `--play` launch runs with NVF_PLAY_AIDS_FROM_SCRIPTS=1: the world test aids wait
for the scripts' first frame, so a late start on a busy machine cannot run them first
(scenarios.toml's header; engine/nvf-play/src/pause.rs WorldAidClock).
Up to NVF_SMOKE_SLOTS (default 3) games run on the machine at once, across every
runner (smoke/slots.py), dealt first come, first served; --slots N / --jobs N let this
run use at most N of them. Launches sharing an NVFOREVER_HOME run in order, and a
selection (not --all) takes the scenarios before a chosen one in its home's chain too,
the ones that write what it reads (`with_home_writers`); the
longest start first, so a long launch does not start last and hold up the end of the
run (the report stays in plan order). --fail-fast starts no launch after the first FAIL.
--exclusive takes both game locks per launch, one game on the machine at a time.
`run`/`plan --size WxH` (e.g. 3840x2160) starts every nvforever launch with that window
size: it replaces a scenario's `--size X` / `--size=X`, or is added. `--extra-args ARGS`
(split like a shell, repeatable; `--extra-args=--no-music` for one flag alone) adds
arguments to every nvforever launch, e.g. "--graphics enhanced"; fnvtool runs get neither.
Both go on the summary line and into every game scenario's summary.json row
(`size`, `extra_args`); <launch>.cmd.txt has the full command line.
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from . import choose, front, integrity, slots
from .expect import ScenarioError
from .plan import Launch, plan, split_args, window_size
from .runner import (Env, Locks, Verdict, default_lock_paths, exe_is_stale, judge, missing_data_files,
                     missing_requirements, read_lines, report, run_launch, unmet_platforms)
from .scenarios import GAME, Suite, load

HERE = Path(__file__).resolve().parent.parent  # tools/smoke
REPO = HERE.parent.parent
SCENARIOS = HERE / "scenarios.toml"
EXE_NAME = "nvforever.exe" if os.name == "nt" else "nvforever"
TOOL_NAME = "fnvtool.exe" if os.name == "nt" else "fnvtool"


def parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="python tools/smoke", description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    for name in ("run", "plan", "check", "list", "verify"):
        q = sub.add_parser(name)
        q.add_argument("--scenarios", type=Path, default=SCENARIOS)
        q.add_argument("--repo", type=Path, default=REPO)
        q.add_argument("--out", type=Path, default=None, help="logs (default <repo>/target/smoke or NVF_SMOKE_OUT)")
        if name == "verify":
            q.add_argument("rev", nargs="?", default="HEAD", help="the merge commit to judge (default HEAD)")
        if name in ("list", "verify"):
            continue
        g = q.add_mutually_exclusive_group()
        g.add_argument("--changed", nargs="?", const="origin/main", metavar="BASE",
                       help="scenarios whose paths match files changed against BASE (default)")
        g.add_argument("--all", action="store_true")
        g.add_argument("--only", nargs="+", metavar="NAME")
        g.add_argument("--area", nargs="+", metavar="AREA")
        g.add_argument("--failed", action="store_true",
                       help="the scenarios that failed in the last run or check (its summary.json)")
        q.add_argument("--no-merge", action="store_true", help="one launch per scenario")
        q.add_argument("--games-only", action="store_true", help="leave out fnvtool scenarios")
        q.add_argument("--tools-only", action="store_true", help="only fnvtool scenarios")
        if name in ("run", "plan"):
            q.add_argument("--size", type=size_arg, default=None, metavar="WxH",
                           help="every nvforever launch's window size (e.g. 3840x2160): replaces a "
                                "scenario's --size, or is added")
            q.add_argument("--extra-args", action="append", default=[], metavar="ARGS",
                           help='more nvforever arguments for every game launch, split like a shell '
                                '(--extra-args "--graphics enhanced"; one flag alone: --extra-args=--no-music); '
                                "repeatable; not for fnvtool runs")
        if name == "run":
            q.add_argument("--checks", action="store_true", help="also run the push checks the change calls for")
            q.add_argument("--exe", type=Path, default=None,
                           help="nvforever (default NVF_EXE, or the newer of <repo>/engine/target/release and release-quick)")
            q.add_argument("--data", default=None, help="game Data folder (default NVF_DATA or FNVLAB_DATA_DIR)")
            q.add_argument("--fnvtool", type=Path, default=None,
                           help="fnvtool (default NVF_FNVTOOL, or the newer of <repo>/target/release and release-quick)")
            q.add_argument("--lock-poll", type=float, default=30.0)
            q.add_argument("--no-locks", action="store_true", help="for machines where nobody else runs the game")
            q.add_argument("--slots", type=int, default=None,
                           help="nvforever smoke games this machine runs at once, across all runners "
                                f"(default NVF_SMOKE_SLOTS or {slots.DEFAULT_SLOTS}; 1 = the machine to itself, "
                                "as --exclusive)")
            q.add_argument("--jobs", type=int, default=None,
                           help="this run's launches at once (default: --slots or the machine's slots; "
                                "the queue's share caps it)")
            q.add_argument("--exclusive", action="store_true",
                           help="the old way: both game locks per launch, one game on the machine at a time "
                                "(for a check that needs the screen to itself)")
            q.add_argument("--context", type=int, default=3, help="log lines shown per failed expectation")
            q.add_argument("--fail-fast", action="store_true",
                           help="start no launch after the first FAIL (the rest are reported as SKIP)")
    return p


def size_arg(text: str) -> str:
    try:
        return window_size(text)
    except ValueError as e:
        raise argparse.ArgumentTypeError(str(e)) from None


def window_options(a) -> dict:
    """The run's `--size` and `--extra-args` (split), as summary.json records them; {} without."""
    size = getattr(a, "size", None)
    extra = [x for s in (getattr(a, "extra_args", None) or []) for x in split_args(s)]
    opts = {}
    if size:
        opts["size"] = size
    if extra:
        opts["extra_args"] = extra
    return opts


def window_line(opts: dict) -> str | None:
    if not opts:
        return None
    parts = (["--size", opts["size"]] if "size" in opts else []) + opts.get("extra_args", [])
    return "every game launch with: " + subprocess.list2cmdline(parts)


def planned(suite: Suite, chosen: list, a, opts: dict) -> list[Launch]:
    return plan(suite, chosen, a.no_merge, opts.get("size"), opts.get("extra_args", []))


def out_dir(a) -> Path:
    d = a.out or (Path(os.environ["NVF_SMOKE_OUT"]) if os.environ.get("NVF_SMOKE_OUT") else a.repo / "target" / "smoke")
    d.mkdir(parents=True, exist_ok=True)
    return d


def selection(a, suite: Suite, say) -> tuple[list, list[str] | None]:
    """The chosen scenarios and, for --changed, the changed files. A chosen scenario
    whose NVFOREVER_HOME earlier scenarios write brings them along (`with_home_writers`)."""
    chosen, files = _selection(a, suite, say)
    if a.all:
        return chosen, files
    return with_home_writers(chosen, suite.scenarios, say), files


def with_home_writers(chosen: list, scen: list, say) -> list:
    """`chosen` with the game scenarios earlier in the file that share a chosen game
    scenario's NVFOREVER_HOME: they write the settings or the save it reads (ui-scale-write
    before ui-scale-menus), so it reads what it was written for, not what an earlier run
    left in the home (or nothing, in a new tree). Homes under `{repo}` are fixtures kept in
    the repository, which no scenario writes first. In the scenario file's order."""
    def home(s) -> str | None:
        h = s.env.get("NVFOREVER_HOME") if s.kind == GAME else None
        return None if h is None or h.startswith("{repo}") else h
    names = {s.name for s in chosen}
    last: dict[str, int] = {}
    for i, s in enumerate(scen):
        if s.name in names and home(s):
            last[home(s)] = i
    added = [s for i, s in enumerate(scen) if s.name not in names and home(s) in last and i < last[home(s)]]
    if not added:
        return chosen
    say(f"with the scenarios that write their NVFOREVER_HOME first: {', '.join(s.name for s in added)}")
    keep = names | {s.name for s in added}
    return [s for s in scen if s.name in keep]


def _selection(a, suite: Suite, say) -> tuple[list, list[str] | None]:
    scen = suite.scenarios
    if a.games_only:
        scen = [s for s in scen if s.kind == GAME]
    if a.tools_only:
        scen = [s for s in scen if s.kind != GAME]
    if a.all:
        return scen, None
    if a.only:
        names = {s.name: s for s in scen}
        unknown = [n for n in a.only if n not in names]
        if unknown:
            raise SystemExit(f"unknown scenarios: {unknown} (python tools/smoke list)")
        return [names[n] for n in a.only], None
    if a.area:
        return choose.by_area(scen, a.area), None
    if getattr(a, "failed", False):
        return last_failed(scen, out_dir(a) / "summary.json", say), None
    base = a.changed or "origin/main"
    files = choose.changed_files(a.repo, base)
    hits = choose.by_changes(scen, files)
    say(f"changed against {base}: {len(files)} files; {len(hits)} scenarios")
    for s, why in hits:
        more = f" (+{len(why) - 1})" if len(why) > 1 else ""
        say(f"  {s.name}: {why[0]}{more}")
    return [s for s, _ in hits], files


def last_failed(scen: list, summary: Path, say) -> list:
    """The scenarios the last run or check reported as FAIL (its summary.json)."""
    try:
        rows = json.loads(summary.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        raise SystemExit(f"--failed: no summary of a last run at {summary}")
    failed = {r.get("scenario") for r in rows if r.get("status") == "FAIL"}
    gone = sorted(failed - {s.name for s in scen})
    if gone:
        say(f"--failed: no longer in the scenario file: {', '.join(gone)}")
    chosen = [s for s in scen if s.name in failed]
    say(f"--failed: {len(chosen)} scenarios failed in the last run ({summary})")
    return chosen


def print_checks(suite: Suite, files: list[str] | None, say) -> list:
    if files is None:
        return []
    cs = choose.checks_for(suite.checks, files)
    say("push checks for this change: " + (", ".join(c.name for c in cs) if cs else "none"))
    return cs


def merge_findings(a, say) -> list[str]:
    """The merge check of the scenario file for `rev` (printed; empty when not a merge)."""
    path = a.scenarios.resolve().relative_to(a.repo).as_posix() if a.scenarios.resolve().is_relative_to(a.repo) else None
    found = integrity.merge_check(a.repo, getattr(a, "rev", "HEAD"), path) if path else None
    if found is None:
        say(f"merge check: {getattr(a, 'rev', 'HEAD')} is no merge commit (or its parents are not here)")
        return []
    for f in found:
        say(f"merge check: {f}")
    return found


def cmd_verify(a, suite: Suite, say) -> int:
    found = merge_findings(a, say)
    say(f"scenario file: {len(suite.scenarios)} scenarios, fine" if not found else
        f"scenario file: {len(found)} scenarios the merge got wrong; compare them with both parents "
        "(a change the merge commit made on purpose is reported too)")
    return 1 if found else 0


def cmd_list(a, suite: Suite, say) -> int:
    for s in suite.scenarios:
        share = f" [launch {s.launch}]" if s.launch else ""
        say(f"{s.name:<28} {s.kind:<4} {','.join(s.areas)}{share}")
    launches = plan(suite, suite.scenarios)
    games = [x for x in launches if x.kind == GAME]
    alone = sum(1 for s in suite.scenarios if s.kind == GAME)
    say(f"{len(suite.scenarios)} scenarios; game launches: {alone} one by one, {len(games)} with shared runs")
    return 0


def cmd_plan(a, suite: Suite, say) -> int:
    chosen, files = selection(a, suite, say)
    print_checks(suite, files, say)
    opts = window_options(a)
    launches = planned(suite, chosen, a, opts)
    if window_line(opts):
        say(window_line(opts))
    total = 0.0
    for x in launches:
        if x.kind == GAME:
            total += x.run_seconds
            env = " ".join(f"{k.replace('NVF_PLAY_TEST_', '')}={v}" for k, v in sorted(x.env.items()))
            say(f"{x.name}: {' '.join(s.name for s in x.scenarios)} | {' '.join(x.args)} --run-seconds {x.run_seconds:g}"
                + (f" | {env}" if env else ""))
            for n in x.notes:
                say(f"    note: {n}")
        else:
            say(f"{x.name}: fnvtool x{len(x.scenarios[0].commands)}")
    g = sum(1 for x in launches if x.kind == GAME)
    say(f"{len(chosen)} scenarios, {g} game launches ({total / 60:.1f} min of game time), {len(launches) - g} tool runs")
    return 0


def newest_build(target: Path, name: str) -> Path:
    """`name` in target/release or target/release-quick (`cargo quick`, the incremental profile for
    iterating, #1956), whichever was built last; target/release's when neither exists."""
    found = [p for p in (target / "release" / name, target / "release-quick" / name) if p.exists()]
    return max(found, key=lambda p: p.stat().st_mtime) if found else target / "release" / name


def resolve(a, suite: Suite, say) -> Env:
    exe = a.exe or (Path(os.environ["NVF_EXE"]) if os.environ.get("NVF_EXE") else
                    newest_build(a.repo / "engine" / "target", EXE_NAME))
    tool = a.fnvtool or (Path(os.environ["NVF_FNVTOOL"]) if os.environ.get("NVF_FNVTOOL") else
                         newest_build(a.repo / "target", TOOL_NAME))
    data = a.data or os.environ.get("NVF_DATA") or os.environ.get("FNVLAB_DATA_DIR")
    return Env(a.repo, out_dir(a), exe, data, tool, say, suite.masters)


def run_checks(checks, env: Env, say) -> bool:
    ok = True
    (env.out / "checks").mkdir(exist_ok=True)
    for c in checks:
        log = env.out / "checks" / (c.name.replace(" ", "_").replace("/", "_") + ".log")
        t = time.monotonic()
        with open(log, "w", encoding="utf-8", errors="replace") as f:
            r = subprocess.run(c.cmd, cwd=env.repo / c.cwd, stdout=f, stderr=subprocess.STDOUT)
        secs = time.monotonic() - t
        if r.returncode == 0:
            say(f"PASS  check {c.name} {secs:.0f} s")
        else:
            ok = False
            say(f"FAIL  check {c.name} {secs:.0f} s (exit {r.returncode})  log {log}")
            tail = read_lines(log)
            keep = [ln for ln in tail if ln.startswith(("error", "warning", "FAIL", "test ", "---- ", "thread '"))
                    or "panicked" in ln][:12] or tail[-8:]
            for ln in keep:
                say(f"        > {ln[:220]}")
    return ok


def judge_launch(x: Launch, env: Env, code, err, secs, a, say, verdicts: list[Verdict]) -> None:
    log = env.out / f"{x.name}.log"
    lines = read_lines(log) if log.exists() else []
    for s in x.scenarios:
        v = judge(s, lines, code, err)
        verdicts.append(v)
        report(v, x, secs, log, say, getattr(a, "context", 3))


def cmd_run(a, suite: Suite, say) -> int:
    chosen, files = selection(a, suite, say)
    checks = print_checks(suite, files, say)
    env = resolve(a, suite, say)
    opts = window_options(a)
    launches = planned(suite, chosen, a, opts)
    games = [x for x in launches if x.kind == GAME]
    if games:
        if not env.data:
            raise SystemExit("no game data: pass --data or set NVF_DATA")
        if not env.exe.exists():
            raise SystemExit(f"no nvforever at {env.exe}: build it (engine workspace) or pass --exe / NVF_EXE")
        stale = exe_is_stale(env.exe, a.repo, files or [])
        if stale:
            say(f"warning: {env.exe.name} is older than {len(stale)} changed sources (e.g. {stale[0]}); rebuild first")
    if any(x.kind != GAME for x in launches) and not env.fnvtool.exists():
        say(f"warning: no fnvtool at {env.fnvtool}; tool scenarios will fail (build it or pass --fnvtool)")
    say(f"{len(chosen)} scenarios in {len(games)} game launches and {len(launches) - len(games)} tool runs; logs in {env.out}")
    if games:
        say(f"nvforever: {env.exe}")
        if window_line(opts):
            say(window_line(opts))
    # `front = true` launches need the screen to themselves: run after the others, one at a
    # time under both game locks (smoke/front.py).
    rest, fronts = front.split(launches, chains(launches))
    started = time.monotonic()
    verdicts = []
    if rest:
        locks, jobs = game_gate(a, say, game_groups(rest))
        if jobs > 1 and len(rest) > 1:
            say(f"up to {jobs} launches at once (smoke slots: {locks.count if isinstance(locks, slots.Slots) else 'none'})")
        try:
            verdicts = run_launches(rest, env, locks, jobs, a, say)
        finally:
            if locks:
                locks.finish()
    if fronts:
        say(f"{len(fronts)} launches run last, one at a time with the machine to themselves (front = true, and the rest of their NVFOREVER_HOME chains)")
        paths = default_lock_paths(a.repo)
        alone = None if a.no_locks or not paths else Locks(paths, poll=a.lock_poll, say=say)
        try:
            verdicts += run_launches(fronts, env, alone, 1, a, say)
        finally:
            if alone:
                alone.finish()
        for x in [x for x in fronts if x.front]:
            line = front.last_line(env.out / f"{x.name}.front.txt")
            say(f"  {x.name}: {line or 'front: no record (not on Windows, or the launch did not start)'}")
    checks_ok = run_checks(checks, env, say) if (a.checks and checks) else True
    in_games = {s.name for x in games for s in x.scenarios}
    return finish(verdicts, env, time.monotonic() - started, checks_ok, say, opts if games else {}, in_games)


def game_gate(a, say, groups: int | None = None):
    """What a game launch waits for, and how many launches this run starts at once.
    `groups`: the run's game launch groups (`chains`), the most slots it can use."""
    paths = default_lock_paths(a.repo)
    if a.no_locks or not paths:
        return None, max(1, a.jobs or 1)
    count = a.slots or slots.default_count()
    if a.exclusive or count == 1:
        # One game on the machine: both locks, as before the slots. A one-slot pool would
        # only cap this runner, and other runners' games could still run beside it.
        return Locks(paths, poll=a.lock_poll, say=say), 1
    jobs = max(1, a.jobs or count)
    pool = slots.Slots(paths[-1], paths[:-1], count=count, poll=min(a.lock_poll, 10.0), say=say,
                       want=min(jobs, groups or jobs))
    return pool, jobs


def game_groups(launches: list[Launch]) -> int:
    """The groups of `chains` with a game launch in them (each holds one slot at a time)."""
    return sum(1 for g in chains(launches) if any(launches[i].kind == GAME for i in g))


def chains(launches: list[Launch]) -> list[list[int]]:
    """The launches (by index) in groups that run in order on one worker: game launches
    with the same NVFOREVER_HOME (one writes the save or the settings the next reads,
    e.g. save-write then save-load); every other launch is a group of its own."""
    out: list[list[int]] = []
    home_at: dict[str, int] = {}
    for i, x in enumerate(launches):
        home = x.env.get("NVFOREVER_HOME") if x.kind == GAME else None
        if home is None:
            out.append([i])
        elif home in home_at:
            out[home_at[home]].append(i)
        else:
            home_at[home] = len(out)
            out.append([i])
    return out


def longest_first(launches: list[Launch], groups: list[list[int]]) -> list[list[int]]:
    """The groups by their game time, longest first (ties in plan order): with a few
    workers, the run ends sooner when a long launch is not the last to start."""
    return sorted(groups, key=lambda g: -sum(launches[i].run_seconds for i in g))


def run_launches(launches: list[Launch], env: Env, locks, jobs: int, a, say) -> list[Verdict]:
    """Run every launch, up to `jobs` at once; the verdicts in plan order (each launch's
    lines are printed as it ends)."""
    results: list[list[Verdict]] = [[] for _ in launches]
    out = threading.Lock()
    failed = threading.Event()
    fail_fast = getattr(a, "fail_fast", False)

    def skip(i: int, reason: str) -> None:
        with out:
            for s in launches[i].scenarios:
                v = Verdict(s.name, "SKIP", reason=reason)
                results[i].append(v)
                report(v, launches[i], None, env.out, say)

    def one(i: int) -> None:
        x = launches[i]
        if fail_fast and failed.is_set():
            skip(i, "not started: --fail-fast after a FAIL")
            return
        missing = sorted({r for s in x.scenarios for r in missing_requirements(s)})
        if missing:
            skip(i, "; ".join(f"unset {r[1:]}" if r.startswith("!") else f"set {r}" for r in missing))
            return
        absent = sorted({f for s in x.scenarios if s.data_files for f in missing_data_files(s, env.data)})
        if absent:
            skip(i, f"not in the Data folder: {', '.join(absent)}")
            return
        others = sorted({p for s in x.scenarios for p in unmet_platforms(s)})
        if others:
            skip(i, f"{', '.join(others)} only")
            return
        code, err, secs = run_launch(x, env, locks)
        with out:
            judge_launch(x, env, code, err, secs, a, say, results[i])
            if any(v.status == "FAIL" for v in results[i]):
                failed.set()

    def chain(ix: list[int]) -> None:
        for i in ix:
            one(i)

    groups = chains(launches)
    if jobs <= 1 or len(groups) <= 1:
        for g in groups:
            chain(g)
    else:
        with ThreadPoolExecutor(max_workers=jobs) as pool:
            futures = [pool.submit(chain, g) for g in longest_first(launches, groups)]
            try:
                for f in futures:
                    f.result()
            except BaseException:
                pool.shutdown(wait=True, cancel_futures=True)  # games in flight end on their own
                raise
    return [v for r in results for v in r]


def cmd_check(a, suite: Suite, say) -> int:
    """Judge saved logs again: each scenario's own log or its shared launch's, the newer."""
    chosen, _ = selection(a, suite, say)
    env = Env(a.repo, out_dir(a), None, None, None, say)
    verdicts: list[Verdict] = []
    for s in chosen:
        names = [s.name] + ([s.launch] if s.launch else [])
        logs = [env.out / f"{n}.log" for n in names if (env.out / f"{n}.log").exists()]
        x = Launch(s.name, s.kind, [s])
        if not logs:
            v = Verdict(s.name, "SKIP", reason=f"no log {s.name}.log")
            verdicts.append(v)
            report(v, x, None, env.out, say)
            continue
        log = max(logs, key=lambda p: p.stat().st_mtime)
        x.name = log.stem
        x.scenarios = [s, s] if log.stem != s.name else [s]  # marks a shared log in the report
        meta = {}
        try:
            meta = json.loads((env.out / f"{log.stem}.exit").read_text())
        except (OSError, ValueError):
            pass
        lines = read_lines(log)
        v = judge(s, lines, meta.get("exit", 0), meta.get("error", ""))
        verdicts.append(v)
        report(v, x, meta.get("seconds"), log, say, getattr(a, "context", 3))
    return finish(verdicts, env, None, True, say)


def finish(verdicts: list[Verdict], env: Env, secs, checks_ok: bool, say,
           opts: dict | None = None, in_games: set[str] = frozenset()) -> int:
    """The summary line and summary.json (one row per scenario; a game scenario's row
    also holds the run's `size` and `extra_args` when it had them, `window_options`)."""
    opts = opts or {}
    n = {k: sum(1 for v in verdicts if v.status == k) for k in ("PASS", "FAIL", "KNOWN", "SKIP")}
    t = f" in {secs / 60:.1f} min" if secs is not None else ""
    w = ""
    if opts:
        w = (f" at {opts['size']}" if "size" in opts else "") + (
            f" with {subprocess.list2cmdline(opts['extra_args'])}" if "extra_args" in opts else "")
    say(f"smoke: {n['PASS']} passed, {n['FAIL']} failed, {n['KNOWN']} known failing, {n['SKIP']} skipped{t}{w}")
    (env.out / "summary.json").write_text(json.dumps(
        [{"scenario": v.scenario, "status": v.status,
          "failures": [{"what": o.what, "label": o.label, "detail": o.detail, "known": o.known} for o in v.failures],
          **(opts if v.scenario in in_games else {})}
         for v in verdicts], indent=1))
    return 1 if n["FAIL"] or not checks_ok else 0


def main(argv=None) -> int:
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass
    a = parser().parse_args(argv)
    say = lambda s: print(s, flush=True)  # noqa: E731
    try:
        suite = load(a.scenarios)
        damage = integrity.problems(a.scenarios.read_text(encoding="utf-8"), a.scenarios.parent)
    except (ValueError, OSError) as e:  # ScenarioError, tomllib.TOMLDecodeError
        print(f"scenario file: {e}", file=sys.stderr)
        return 2
    for d in damage:
        print(f"scenario file: {d}", file=sys.stderr)
    if damage:
        return 2
    a.repo = a.repo.resolve()
    if a.cmd in ("run", "plan") and merge_findings(a, say):
        say("merge check: warning only here; `python tools/smoke verify` fails on it")
    return {"run": cmd_run, "plan": cmd_plan, "check": cmd_check, "list": cmd_list, "verify": cmd_verify}[a.cmd](a, suite, say)
