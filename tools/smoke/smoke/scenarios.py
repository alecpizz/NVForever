"""The scenario file (tools/smoke/scenarios.toml): smoke tests as data.

Top-level tables:

- `[timed.<ENV VAR>]` `sep`, `shift`: a test aid whose value is a list of timed items
  (`<seconds>:<what>`). Lists of scenarios sharing a launch are joined with `sep`; a
  member's `offset` moves its items by that many seconds (`shift`: "lead" the first
  field of each item, "all" every field, "value" the whole value is one time).
- `[[check]]` the "checks before every push": `name`, `paths` (globs), `cmd`, `cwd`.
- `[[scenario]]` one smoke test (see `Scenario`).

A scenario's `expect_file` (a path from the scenario file's folder) adds a JSON expectation
`{ json = "<path>", equals = <value> }` for every value in that TOML file, its tables
giving the path (`[summary.by_status]` `no_op = 4` is `summary.by_status.no_op` = 4).
Totals that many branches change live there, in a file of their own, so a text merge of
two changes to them conflicts in a few lines instead of mangling scenarios.toml.

A game scenario's `serve` (`{ status = 404 }`, optionally `body = "..."`) starts a small
HTTP server on the loopback address for its launch, answering every request with that
status; `{serve}` in an env value becomes its address (`http://127.0.0.1:<port>`). For the
failure paths of code that downloads, without the network. Scenarios sharing a launch
that set `serve` must set the same one.

A `--play` game scenario runs with `--no-messages` (added to its arguments) unless it sets
`messages = true`: the add-on packs' OK-only start-up boxes, at the start and again on a new
game once the player leaves Doc Mitchell's house, are then answered as the scripts show
them and never open (engine/nvf-play/src/script.rs). A scenario whose checks read message
boxes (opens, closes, buttons, MenuMode 1001) sets `messages = true` to keep them.
"""
from __future__ import annotations

import tomllib
from dataclasses import dataclass, field
from pathlib import Path

from .expect import Expect, ScenarioError

GAME, TOOL = "game", "tool"
PLATFORMS = ("linux", "windows", "macos")  # what a scenario's `platforms` may name
SCENARIO_KEYS = {
    "name", "kind", "source", "areas", "paths", "args", "run_seconds", "env", "launch", "offset",
    "expect", "expect_file", "order", "forbid", "manual", "requires", "data_files", "platforms", "commands", "exit", "note",
    "timeout", "ends", "front", "serve", "messages",
}
NO_MESSAGES = "--no-messages"  # added to every `--play` game scenario without `messages = true`
SERVE_KEYS = {"status", "body"}


@dataclass
class Timed:
    sep: str = ","
    shift: str = "lead"  # lead | all | value


@dataclass
class Check:
    name: str
    paths: list[str]
    cmd: list[str]
    cwd: str = "."


@dataclass
class Scenario:
    name: str
    kind: str = GAME
    source: str = ""  # where the check came from (CLAUDE.md paragraph, doc section)
    areas: list[str] = field(default_factory=list)
    paths: list[str] = field(default_factory=list)  # globs over changed files that call for it
    args: list[str] = field(default_factory=list)  # nvforever arguments, without --data/--run-seconds
    run_seconds: float = 0.0
    env: dict[str, str] = field(default_factory=dict)
    launch: str = ""  # scenarios with the same launch name share one game run
    offset: float = 0.0  # seconds added to this scenario's timed test aids in a shared run
    expect: list[Expect] = field(default_factory=list)
    order: list[list[Expect]] = field(default_factory=list)
    forbid: list[Expect] = field(default_factory=list)
    manual: str = ""  # what a person must still compare by hand (printed, not checked)
    requires: list[str] = field(default_factory=list)  # env vars needed, or with "!" needed unset (else skipped)
    data_files: list[str] = field(default_factory=list)  # files the Data folder must hold (else skipped)
    platforms: list[str] = field(default_factory=list)  # systems it runs on (empty: any)
    commands: list[list[str]] = field(default_factory=list)  # tool scenarios: fnvtool arguments
    exit: int = 0
    note: str = ""
    timeout: float = 0.0  # seconds; 0 = derived from run_seconds
    ends: str = "run finished"  # game scenarios: the line the run ends with (always expected)
    # Game scenarios: hold the launch's window in front while it runs (smoke/front.py; Windows),
    # for checks that need the foreground (DLSS Frame Generation); needs --slots 1.
    front: bool = False
    serve: dict = field(default_factory=dict)  # a loopback HTTP server for the launch: status, body
    # Game scenarios with --play: keep message boxes as a player gets them (no --no-messages added).
    messages: bool = False

    def all_expectations(self):
        yield from self.expect
        for seq in self.order:
            yield from seq
        yield from self.forbid


@dataclass
class Suite:
    scenarios: list[Scenario]
    checks: list[Check]
    timed: dict[str, Timed]
    untimed: set[str] = field(default_factory=set)  # test aids known to hold no times
    masters: list[str] = field(default_factory=list)  # `{masters}` in tool commands

    def by_name(self) -> dict[str, Scenario]:
        return {s.name: s for s in self.scenarios}


def _expects(items, where) -> list[Expect]:
    if not isinstance(items, list):
        raise ScenarioError(f"{where}: expected a list")
    return [Expect.parse(x, f"{where}[{i}]") for i, x in enumerate(items)]


def _leaves(d: dict, path: list[str]):
    for k, v in d.items():
        if isinstance(v, dict):
            yield from _leaves(v, path + [k])
        else:
            yield ".".join(path + [k]), v


def _expect_file(name: str, rel, base: Path | None) -> list[Expect]:
    if not isinstance(rel, str):
        raise ScenarioError(f"{name}.expect_file: expected a path")
    if base is None:
        raise ScenarioError(f"{name}.expect_file: no folder to read {rel} from")
    try:
        data = tomllib.loads((base / rel).read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as e:
        raise ScenarioError(f"{name}.expect_file {rel}: {e}") from e
    out = [Expect.parse({"json": k, "equals": v}, f"{name}.expect_file {rel}") for k, v in _leaves(data, [])]
    if not out:
        raise ScenarioError(f"{name}.expect_file {rel}: holds no values")
    return out


def _serve(name: str, d) -> dict:
    if not isinstance(d, dict):
        raise ScenarioError(f"{name}.serve: expected a table such as {{ status = 404 }}")
    extra = set(d) - SERVE_KEYS
    if extra:
        raise ScenarioError(f"{name}.serve: unknown keys {sorted(extra)}")
    status = d.get("status")
    if not isinstance(status, int) or isinstance(status, bool) or not 200 <= status <= 599:
        raise ScenarioError(f"{name}.serve.status: expected an HTTP status from 200 to 599")
    body = d.get("body", "")
    if not isinstance(body, str):
        raise ScenarioError(f"{name}.serve.body: expected text")
    return {"status": status, "body": body}


def parse_scenario(d: dict, base: Path | None = None) -> Scenario:
    name = d.get("name")
    if not name or not isinstance(name, str):
        raise ScenarioError(f"scenario without a name: {d!r:.80}")
    extra = set(d) - SCENARIO_KEYS
    if extra:
        raise ScenarioError(f"{name}: unknown keys {sorted(extra)}")
    s = Scenario(name=name)
    s.kind = d.get("kind", GAME)
    if s.kind not in (GAME, TOOL):
        raise ScenarioError(f"{name}: kind must be game or tool")
    for key in ("source", "launch", "manual", "note"):
        setattr(s, key, d.get(key, ""))
    s.areas = list(d.get("areas", []))
    s.paths = list(d.get("paths", []))
    s.args = [str(a) for a in d.get("args", [])]
    s.run_seconds = float(d.get("run_seconds", 0))
    s.env = {k: str(v) for k, v in d.get("env", {}).items()}
    s.ends = str(d.get("ends", s.ends))
    s.offset = float(d.get("offset", 0))
    s.requires = list(d.get("requires", []))
    s.data_files = [str(f) for f in d.get("data_files", [])]
    s.platforms = list(d.get("platforms", []))
    bad = [p for p in s.platforms if p not in PLATFORMS]
    if bad:
        raise ScenarioError(f"{name}: platforms takes {sorted(PLATFORMS)}, not {bad}")
    s.commands = [[str(a) for a in c] for c in d.get("commands", [])]
    s.exit = int(d.get("exit", 0))
    s.timeout = float(d.get("timeout", 0))
    front = d.get("front", False)
    if not isinstance(front, bool):
        raise ScenarioError(f"{name}: front must be true or false")
    s.front = front
    messages = d.get("messages", False)
    if not isinstance(messages, bool):
        raise ScenarioError(f"{name}: messages must be true or false")
    s.messages = messages
    if "serve" in d:
        s.serve = _serve(name, d["serve"])
    s.expect = _expects(d.get("expect", []), f"{name}.expect")
    if "expect_file" in d:
        s.expect += _expect_file(name, d["expect_file"], base)
    s.order = [_expects(seq, f"{name}.order[{i}]") for i, seq in enumerate(d.get("order", []))]
    s.forbid = _expects(d.get("forbid", []), f"{name}.forbid")
    for e in [x for seq in s.order for x in seq] + s.forbid:
        if e.pattern is None:
            raise ScenarioError(f"{name}: order and forbid take text or re expectations, not {e.label!r}")
    if s.kind == GAME:
        if s.run_seconds <= 0:
            raise ScenarioError(f"{name}: a game scenario needs run_seconds")
        if any(a.startswith("--run-seconds") or a == "--data" for a in s.args):
            raise ScenarioError(f"{name}: give run_seconds as its own key and leave --data to the runner")
        if "--play" in s.args and not s.messages and NO_MESSAGES not in s.args:
            s.args.append(NO_MESSAGES)
        if s.messages and NO_MESSAGES in s.args:
            raise ScenarioError(f"{name}: messages = true keeps the boxes; drop {NO_MESSAGES} from args")
        # Every unattended run ends with this line (nvforever main.rs), or with the one the
        # scenario names for a run that ends another way (`ends`); always expected.
        s.expect.insert(0, Expect.parse(s.ends))
    elif not s.commands:
        raise ScenarioError(f"{name}: a tool scenario needs commands")
    if s.front and s.kind != GAME:
        raise ScenarioError(f"{name}: front applies to game scenarios only")
    if s.messages and (s.kind != GAME or "--play" not in s.args):
        raise ScenarioError(f"{name}: messages applies to --play game scenarios only")
    if s.serve and s.kind != GAME:
        raise ScenarioError(f"{name}: serve is for game scenarios")
    if not s.serve and any("{serve}" in v for v in s.env.values()):
        raise ScenarioError(f"{name}: {{serve}} in env needs a serve table")
    if not s.paths and not s.areas:
        raise ScenarioError(f"{name}: needs paths or areas so it can be selected")
    return s


def load_text(text: str, base: Path | None = None) -> Suite:
    """`base`: the folder `expect_file` paths start from (the scenario file's)."""
    data = tomllib.loads(text)
    try:
        timed = {k: Timed(**v) for k, v in data.get("timed", {}).items()}
        checks = [Check(**c) for c in data.get("check", [])]
    except TypeError as e:
        raise ScenarioError(f"timed or check table: {e}") from e
    extra = set(data) - {"timed", "check", "scenario", "untimed", "masters"}
    if extra:
        raise ScenarioError(f"unknown top-level keys {sorted(extra)}")
    for k, t in timed.items():
        if t.shift not in ("lead", "all", "value"):
            raise ScenarioError(f"timed.{k}: shift must be lead, all or value")
    scenarios = [parse_scenario(d, base) for d in data.get("scenario", [])]
    names = [s.name for s in scenarios]
    dup = {n for n in names if names.count(n) > 1}
    if dup:
        raise ScenarioError(f"duplicate scenario names: {sorted(dup)}")
    served: dict[str, dict] = {}
    for s in scenarios:
        if s.serve:
            key = s.launch or s.name
            if served.setdefault(key, s.serve) != s.serve:
                raise ScenarioError(f"launch {key}: {s.name} serves differently from another member")
    suite = Suite(scenarios, checks, timed, set(data.get("untimed", [])), list(data.get("masters", [])))
    from .plan import validate_launches  # late import: plan uses this module's types

    validate_launches(suite)
    return suite


def load(path: Path) -> Suite:
    path = Path(path)
    return load_text(path.read_text(encoding="utf-8"), path.parent)
