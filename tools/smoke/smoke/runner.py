"""Running launches: the game locks, the processes, the logs and the verdicts."""
from __future__ import annotations

import glob
import json
import os
import shutil
import signal
import subprocess
import sys
import threading
import time
from contextlib import contextmanager
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from .expect import Outcome, check_forbid, check_one, check_order, clean
from .front import Front
from .plan import Launch, fmt
from .scenarios import GAME, Scenario
from .stacks import dump as dump_stacks

AID_PREFIXES = ("NVF_PLAY_TEST_", "NVF_PLAY_DEBUG_", "NVF_PLAY_AIDS_")
# The world test aids (the console's lines, the script steps, keys, talks, screenshots…)
# count from when the scripts first run, so a busy machine's late start does not run them
# before the world can take them (engine/nvf-play/src/pause.rs WorldAidClock). Set for
# every `--play` launch; a scenario can set it to "0" for the old clock.
FROM_SCRIPTS = "NVF_PLAY_AIDS_FROM_SCRIPTS"
OWNER = "smoke-owner.txt"


# ---------------------------------------------------------------- locks

def pid_alive(pid: int) -> bool:
    if pid <= 0:
        return False
    if os.name == "nt":
        import ctypes

        k32 = ctypes.windll.kernel32
        h = k32.OpenProcess(0x1000, False, pid)  # PROCESS_QUERY_LIMITED_INFORMATION
        if not h:
            return k32.GetLastError() == 5  # access denied: it exists
        code = ctypes.c_ulong()
        ok = k32.GetExitCodeProcess(h, ctypes.byref(code))
        k32.CloseHandle(h)
        return bool(ok) and code.value == 259  # STILL_ACTIVE
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def process_times(pid: int) -> tuple[int, int] | None:
    """A Windows process's creation and exit times (FILETIME ticks; exit 0 while it
    runs), None when it cannot be opened."""
    import ctypes
    from ctypes import wintypes

    k32 = ctypes.windll.kernel32
    k32.OpenProcess.restype = wintypes.HANDLE
    h = k32.OpenProcess(0x1000, False, pid)  # PROCESS_QUERY_LIMITED_INFORMATION
    if not h:
        return None
    t = [ctypes.c_ulonglong() for _ in range(4)]
    ok = k32.GetProcessTimes(wintypes.HANDLE(h), *[ctypes.byref(x) for x in t])
    k32.CloseHandle(wintypes.HANDLE(h))
    return (t[0].value, t[1].value) if ok else None


def process_birth(pid: int) -> int | None:
    """When a process was made, as a number that stays the same for its whole life (Windows:
    its creation FILETIME; Linux: its start time in clock ticks from /proc), None when it
    cannot be read. With the pid it names one process, so a pid used again by a new
    process is told apart from the one that wrote an owner file (#2004)."""
    if os.name == "nt":
        t = process_times(pid)
        return t[0] if t else None
    try:
        stat = Path(f"/proc/{pid}/stat").read_text()
        # Field 22, counting from 1; the name (field 2) is in brackets and may hold spaces.
        return int(stat.rsplit(")", 1)[1].split()[19])
    except (OSError, ValueError, IndexError):
        return None


def owner_line(pid: int | None = None) -> str:
    """The owner file's text for a smoke runner's lock: the pid first (what older runners
    read), and `born=` its birth from [`process_birth`] when it can be read."""
    pid = os.getpid() if pid is None else pid
    born = process_birth(pid)
    tail = f" born={born}" if born is not None else ""
    return f"{pid} smoke runner {time.strftime('%Y-%m-%d %H:%M:%S')}{tail}\n"


def owner_gone(text: str) -> bool:
    """Whether the runner an owner file names has ended: its process is gone, or the pid
    now belongs to a process made at another time. A file without `born=` (an older
    runner's, or one whose birth could not be read) is judged by the pid alone."""
    words = text.split()
    try:
        pid = int(words[0])
    except (ValueError, IndexError):
        return False
    if not pid_alive(pid):
        return True
    born = next((w[5:] for w in words if w.startswith("born=")), None)
    if born is None:
        return False
    now = process_birth(pid)
    return now is not None and str(now) != born


def write_owner(lock: Path, text: str) -> None:
    """Write the owner file whole (a reader never sees half a pid)."""
    tmp = lock / (OWNER + ".tmp")
    tmp.write_text(text)
    os.replace(tmp, lock / OWNER)


def child_pids(pid: int, born: tuple[int, int] | None, exe: str) -> list[int]:
    """The processes `pid` started that are still running (a load's new run, which goes on
    after the run that started it ends: engine/nvforever/src/handoff.rs). Windows keeps a
    process's parent id after the parent ends and reuses ids, so only a process running
    `exe` (its file name) created while `pid` ran (`born`: its times from
    [`process_times`], read while it was still open) counts. None elsewhere."""
    if os.name != "nt" or born is None:
        return []
    import ctypes
    from ctypes import wintypes

    class Entry(ctypes.Structure):
        _fields_ = [("dwSize", wintypes.DWORD), ("cntUsage", wintypes.DWORD), ("th32ProcessID", wintypes.DWORD),
                    ("th32DefaultHeapID", ctypes.c_size_t), ("th32ModuleID", wintypes.DWORD), ("cntThreads", wintypes.DWORD),
                    ("th32ParentProcessID", wintypes.DWORD), ("pcPriClassBase", ctypes.c_long), ("dwFlags", wintypes.DWORD),
                    ("szExeFile", ctypes.c_wchar * 260)]

    k32 = ctypes.windll.kernel32
    k32.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
    snap = k32.CreateToolhelp32Snapshot(0x2, 0)  # TH32CS_SNAPPROCESS
    if snap in (None, wintypes.HANDLE(-1).value):
        return []
    out = []
    e = Entry()
    e.dwSize = ctypes.sizeof(Entry)
    ok = k32.Process32FirstW(snap, ctypes.byref(e))
    while ok:
        if e.th32ParentProcessID == pid and e.th32ProcessID != pid and e.szExeFile.lower() == exe.lower():
            out.append(e.th32ProcessID)
        ok = k32.Process32NextW(snap, ctypes.byref(e))
    k32.CloseHandle(snap)
    start, end = born
    own = []
    for c in out:
        t = process_times(c)
        if t and t[0] >= start and (end == 0 or t[0] <= end):
            own.append(c)
    return own


def own_times(p: subprocess.Popen) -> tuple[int, int] | None:
    """`p`'s creation and exit times through its own handle (still open after it ends,
    so its id cannot have been reused). None elsewhere."""
    if os.name != "nt":
        return None
    import ctypes
    from ctypes import wintypes

    t = [ctypes.c_ulonglong() for _ in range(4)]
    ok = ctypes.windll.kernel32.GetProcessTimes(wintypes.HANDLE(int(p._handle)), *[ctypes.byref(x) for x in t])
    return (t[0].value, t[1].value) if ok else None


def wait_for_children(pid: int, children: list[int], deadline: float) -> str:
    """Wait for the processes `pid` started (their output is the launch's too) until
    `deadline` (time.monotonic()); past it, end them (they are this launch's own). An
    error text when they had to be ended."""
    while children and time.monotonic() < deadline:
        children = [c for c in children if pid_alive(c)]
        if children:
            time.sleep(0.2)
    if not children:
        return ""
    for c in children:
        try:
            os.kill(c, 9)
        except OSError:
            pass
    return f"a process it started (pid {', '.join(map(str, children))}) ran past the time limit"


def group_alive(pgid: int) -> bool:
    """POSIX: whether any process of the process group `pgid` still runs."""
    try:
        os.killpg(pgid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def wait_for_group(pgid: int, deadline: float) -> str:
    """POSIX: wait for the rest of a launch's process group (a load's new run is started
    into it: engine/nvforever/src/handoff.rs sets no group of its own) until `deadline`;
    past it, end the group. An error text when it had to be ended."""
    while group_alive(pgid) and time.monotonic() < deadline:
        time.sleep(0.2)
    if not group_alive(pgid):
        return ""
    try:
        os.killpg(pgid, signal.SIGKILL)  # POSIX only: Windows has no SIGKILL
    except OSError:
        pass
    return f"a process it started (process group {pgid}) ran past the time limit"


def launch_popen_kw() -> dict:
    """Off Windows a game launch gets a process group of its own, so the runner can wait
    for (and stop) a load's new run with it. Windows finds them by parent ([`child_pids`])."""
    return {} if os.name == "nt" else {"start_new_session": True}


def wait_rest(p: subprocess.Popen, exe: str, deadline: float) -> str:
    """After `p` ended: wait for the processes it started (their output is the launch's
    too) until `deadline`, ending them past it. An error text when they had to be ended."""
    if os.name == "nt":
        return wait_for_children(p.pid, child_pids(p.pid, own_times(p), exe), deadline)
    return wait_for_group(p.pid, deadline)


GAME_PROCESSES = ("nvforever", "falloutnv")


def game_running() -> bool:
    """Whether an nvforever or FalloutNV process runs on this machine."""
    cmd = ["tasklist", "/FO", "CSV", "/NH"] if os.name == "nt" else ["ps", "-A", "-o", "comm="]
    try:
        out = subprocess.run(cmd, capture_output=True, text=True, timeout=30).stdout.lower()
    except (OSError, subprocess.SubprocessError):
        return True  # cannot tell: assume the lock is in use
    return any(name in out for name in GAME_PROCESSES)


QUEUE = "smoke-queue"


class Queue:
    """First come, first served among smoke runners (docs/outstanding.md §5): a runner
    that has to wait for the game locks puts a ticket in a folder beside the first lock
    (`smoke-queue`, named by the time it came and its process id) and only tries the
    locks while its ticket is the oldest. It keeps the ticket until its whole run is
    over, so a run of several launches is not split up by other runners' launches.
    Tickets of processes that are gone are cleared. Plain `mkdir` lockers (captures)
    take no ticket and are not held back by the queue."""

    def __init__(self, folder: Path, pid: int | None = None, alive=None, label: str = "smoke runner"):
        self.folder = Path(folder)
        self.pid = os.getpid() if pid is None else pid
        self.alive = alive or pid_alive
        self.label = label  # the ticket's text after the pid ("smoke runner slots": a slot runner)
        self.ticket: Path | None = None

    def join(self) -> None:
        if self.ticket is not None and self.ticket.exists():
            return
        self.folder.mkdir(parents=True, exist_ok=True)
        self.ticket = self.folder / f"{time.time_ns():020d}-{self.pid}.ticket"
        self.ticket.write_text(f"{self.pid} {self.label} {time.strftime('%Y-%m-%d %H:%M:%S')}\n")

    def leave(self) -> None:
        if self.ticket is not None:
            try:
                self.ticket.unlink()
            except OSError:
                pass
            self.ticket = None

    def ahead(self) -> list[Path]:
        """The live tickets before this runner's, oldest first (dead ones cleared)."""
        return self.live()[0]

    def live(self) -> tuple[list[Path], list[Path]]:
        """The live tickets before and after this runner's, oldest first (dead ones cleared)."""
        if self.ticket is None:
            return [], []
        out, behind, after = [], [], False
        for t in sorted(self.folder.glob("*.ticket")):
            if t == self.ticket:
                after = True
                continue
            try:
                pid = int(t.stem.split("-")[1])
            except (IndexError, ValueError):
                continue
            if pid == self.pid:
                continue  # this process's own older ticket (another Locks of the same run)
            if not self.alive(pid):
                try:
                    t.unlink()
                except OSError:
                    pass
                continue
            (behind if after else out).append(t)
        return out, behind


class Locks:
    """The game-window locks every agent takes by `mkdir` (CLAUDE.md, the agents' brief).
    Taken all or none, in order; released only if this runner made them. A lock left by
    a smoke runner that died (its owner file names a process that is gone) is cleared.
    Smoke runners wait their turn in a first-come, first-served [`Queue`]; `finish` gives
    up the place at the end of the run."""

    def __init__(self, paths: list[Path], poll: float = 30.0, wait_max: float = 4 * 3600.0, say=print,
                 stale_after: float = 900.0, running=game_running, queue: Queue | None = None):
        self.paths = [Path(p) for p in paths]
        self.poll, self.wait_max, self.say = poll, wait_max, say
        self.stale_after, self.running = stale_after, running
        self.held: list[Path] = []
        self.warned: set[tuple[Path, float]] = set()
        self.queue = queue if queue is not None else (Queue(self.paths[0].parent / QUEUE) if self.paths else None)

    def warn_if_abandoned(self, p: Path) -> None:
        """Another session's plain mkdir lock that is old while no game runs: say so loudly
        (once per lock). It is never removed: only its maker may do that."""
        try:
            made = p.stat().st_ctime
        except OSError:
            return
        if (p / OWNER).exists() or (p, made) in self.warned or time.time() - made < self.stale_after:
            return
        if self.running():
            return
        self.warned.add((p, made))
        mins = (time.time() - made) / 60
        self.say(f"!!! LOCK LOOKS ABANDONED: {p} was made {mins:.0f} min ago "
                 f"({time.strftime('%H:%M', time.localtime(made))}) and no nvforever or FalloutNV runs. "
                 f"Not removed: ask the session that made it, or remove it if that session is gone.")

    @staticmethod
    def _stale(p: Path) -> bool:
        """A lock whose owner file names a smoke runner that has ended."""
        try:
            return owner_gone((p / OWNER).read_text())
        except OSError:
            return False  # a plain mkdir lock, or one whose owner is still being written

    def _replace_gone(self, p: Path) -> bool:
        """Take over `p` when it was left by a smoke runner that ended, or is the slot
        runners' shared lock with no slot taken (#1933); whether it is now this runner's.
        The judging, removing, making and owner file all happen under the slots' mutex
        beside the lock (`smoke-slots/mutex.lock`), so two runners that both saw it left
        cannot both take it, and one cannot remove the lock the other just made (#2004)."""
        from .slots import FileLock, unused_shared  # slots imports this module

        mutex = FileLock(p.parent / "smoke-slots" / "mutex.lock")
        if not mutex.try_lock():
            return False  # another runner is changing it now: look again next time
        try:
            if not (self._stale(p) or unused_shared(p)):
                return False
            self.say(f"lock {p}: left by a smoke runner that is gone; cleared")
            shutil.rmtree(p, ignore_errors=True)
            try:
                p.mkdir()
            except FileExistsError:
                return False  # a plain mkdir locker took it in between
            write_owner(p, owner_line())
            return True
        finally:
            mutex.unlock()

    @staticmethod
    def holder(p: Path) -> str:
        """Who holds a lock, as far as the folder tells (no lock is taken by this)."""
        try:
            return "held by " + (p / OWNER).read_text().strip()
        except OSError:
            pass
        try:
            made = time.strftime("%H:%M:%S", time.localtime(p.stat().st_ctime))
        except OSError:
            return "just released"
        return f"another session's mkdir lock, made at {made}"

    def try_take(self) -> Path | None:
        """Take every lock or none; the lock that is held by someone else, if any."""
        for p in self.paths:
            p.parent.mkdir(parents=True, exist_ok=True)
            try:
                p.mkdir()
            except FileExistsError:
                if not self._replace_gone(p):
                    self.release()
                    return p
            else:
                write_owner(p, owner_line())
            self.held.append(p)
        return None

    def acquire(self) -> None:
        start, told, queued = time.monotonic(), None, None
        if self.queue is not None:
            self.queue.join()
        while True:
            ahead = self.queue.ahead() if self.queue is not None else []
            if ahead:
                if len(ahead) != queued:
                    self.say(f"waiting in the smoke queue: {len(ahead)} runner(s) first")
                    queued = len(ahead)
                busy = None
            else:
                busy = self.try_take()
                if busy is None:
                    return
                if busy != told:
                    self.say(f"waiting for {busy}: {self.holder(busy)}")
                    told = busy
                self.warn_if_abandoned(busy)
            if time.monotonic() - start > self.wait_max:
                what = f"lock {busy} still held" if busy else f"{len(ahead)} runner(s) still ahead in the smoke queue"
                raise TimeoutError(f"{what} after {self.wait_max:.0f} s")
            # First in the queue: look more often, so a plain mkdir locker that waits
            # without a ticket does not keep winning the race.
            time.sleep(self.poll if ahead else min(self.poll, 5.0))

    def release(self, _held=None) -> None:
        """Give the locks back between launches; the place in the queue is kept."""
        for p in reversed(self.held):
            shutil.rmtree(p, ignore_errors=True)
        self.held.clear()

    def finish(self) -> None:
        """The run is over: the locks and the place in the queue."""
        self.release()
        if self.queue is not None:
            self.queue.leave()

    def __enter__(self):
        self.acquire()
        return self

    def __exit__(self, *exc):
        self.finish()


def default_lock_paths(repo: Path) -> list[Path]:
    """NVF_SMOKE_LOCKS (paths separated by os.pathsep) or this machine's two locks: the
    capture lock (Windows, where the owner's captures run) and the main checkout's."""
    env = os.environ.get("NVF_SMOKE_LOCKS")
    if env is not None:
        return [Path(p) for p in env.split(os.pathsep) if p.strip()]
    out = []
    cap = Path(r"C:\fnv-private\capture")
    if os.name == "nt" and cap.is_dir():
        out.append(cap / "run.lock")
    out.append(main_checkout(repo) / ".claude" / "smoke.lock")
    return out


def main_checkout(repo: Path) -> Path:
    """The main checkout of a worktree (where the shared smoke lock lives)."""
    try:
        r = subprocess.run(["git", "-C", str(repo), "rev-parse", "--path-format=absolute", "--git-common-dir"],
                           capture_output=True, text=True, check=True)
        return Path(r.stdout.strip()).parent
    except (OSError, subprocess.CalledProcessError):
        return repo


# ---------------------------------------------------------------- verdicts

@dataclass
class Verdict:
    scenario: str
    status: str  # PASS FAIL KNOWN SKIP
    failures: list[Outcome] = field(default_factory=list)
    xpass: list[str] = field(default_factory=list)  # known-failing marks that now pass
    manual: str = ""
    reason: str = ""  # for SKIP / launch errors


def judge(s: Scenario, lines: list[str], exit_code: int | None, launch_error: str = "",
          platform: str | None = None) -> Verdict:
    outs: list[Outcome] = []
    if launch_error:
        outs.append(Outcome(False, None, "launch", launch_error))
    elif exit_code != s.exit:
        outs.append(Outcome(False, None, "exit", f"exit code {exit_code}, want {s.exit}",
                            context=[ln for ln in lines if "error" in ln.lower() or "panicked" in ln][-3:]))
    outs += [check_one(e, lines, platform) for e in s.expect]
    for seq in s.order:
        outs += check_order(seq, lines, platform)
    outs += [check_forbid(e, lines, platform) for e in s.forbid]
    hard = [o for o in outs if not o.ok and not o.known]
    known = [o for o in outs if not o.ok and o.known]
    xpass = [o.label for o in outs if o.ok and o.known]
    status = "FAIL" if hard else ("KNOWN" if known else "PASS")
    return Verdict(s.name, status, hard + known, xpass, s.manual)


def read_lines(path: Path) -> list[str]:
    with open(path, encoding="utf-8", errors="replace") as f:
        return [clean(ln) for ln in f]


# ---------------------------------------------------------------- running

@dataclass
class Env:
    repo: Path
    out: Path
    exe: Path | None
    data: str | None
    fnvtool: Path | None
    say: object = print
    masters: list[str] = field(default_factory=list)


LOCKED_NOTE = ("the Windows session was locked (LogonUI.exe running): frames wait 300-500 ms to be shown "
               "then, so frame times and timed expectations do not measure the engine (#3277)")


def session_locked() -> bool | None:
    """Whether the Windows session is locked: LogonUI.exe, the lock screen, is running
    [observed #3277: frames wait 300 to 500 ms in `window::prepare_windows` while it runs,
    also with display-sync off]. `None`: not Windows, or the process list could not be read."""
    if os.name != "nt":
        return None
    try:
        r = subprocess.run(["tasklist", "/FI", "IMAGENAME eq LogonUI.exe", "/FO", "CSV", "/NH"],
                           capture_output=True, text=True, timeout=10, errors="replace")
    except (OSError, subprocess.TimeoutExpired):
        return None
    if r.returncode != 0:
        return None
    return "logonui.exe" in r.stdout.lower()


def aid_free_environ(extra: dict[str, str]) -> dict[str, str]:
    env = {k: v for k, v in os.environ.items() if not k.upper().startswith(AID_PREFIXES)}
    env.update(extra)
    return env


def game_env(launch: Launch) -> dict[str, str]:
    """The test aids a game launch runs with: its own, and FROM_SCRIPTS on for `--play`."""
    env = dict(launch.env)
    if "--play" in launch.args:
        env.setdefault(FROM_SCRIPTS, "1")
    return env


def launch_timeout(launch: Launch) -> float:
    explicit = max((s.timeout for s in launch.scenarios), default=0)
    return explicit or launch.run_seconds * 3 + 240


def expand(arg: str, ctx: dict[str, str], masters: list[str] = ()) -> list[str] | None:
    """Placeholders in tool arguments: {data}, {repo}, {env:NAME}, and `{masters}` (the
    scenario file's list of masters, one argument each); a result with `*` is a glob
    (one argument per file). None when an env placeholder is unset."""
    if arg == "{masters}":
        return list(masters)
    out = arg
    while "{env:" in out:
        i = out.index("{env:")
        j = out.index("}", i)
        val = os.environ.get(out[i + 5:j])
        if val is None:
            return None
        out = out[:i] + val + out[j + 1:]
    for k, v in ctx.items():
        out = out.replace("{" + k + "}", v)
    if "*" in out:
        return sorted(glob.glob(out)) or [out]
    return [out]


def expand_game_value(value: str) -> str | None:
    """Substitute invoking-process env variables in one scalar, without re-expanding data."""
    parts = []
    while "{env:" in value:
        i = value.index("{env:")
        j = value.find("}", i)
        if j < 0:
            return None
        replacement = os.environ.get(value[i + 5:j])
        if not replacement:
            return None
        parts.extend((value[:i], replacement))
        value = value[j + 1:]
    return "".join(parts) + value


def launch_serve(launch: Launch) -> dict:
    """The `serve` table of a launch's members (load_text checked they agree), or {}."""
    return next((s.serve for s in launch.scenarios if s.serve), {})


@contextmanager
def serving(serve: dict):
    """A loopback HTTP server answering every request with `serve`'s status and body, for
    one launch; yields its address (`http://127.0.0.1:<port>`), or None without `serve`."""
    if not serve:
        yield None
        return
    status, body = serve["status"], serve.get("body", "").encode("utf-8")

    class Answer(BaseHTTPRequestHandler):
        def _answer(self, with_body: bool) -> None:
            self.send_response(status)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Connection", "close")
            self.end_headers()
            if with_body:
                self.wfile.write(body)

        def do_GET(self):  # noqa: N802 (the handler's names)
            self._answer(True)

        def do_HEAD(self):  # noqa: N802
            self._answer(False)

        def log_message(self, *_args):  # quiet: the game's log is the record
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Answer)
    server.daemon_threads = True
    thread = threading.Thread(target=server.serve_forever, name="smoke-serve", daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_address[1]}"
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


def run_launch(launch: Launch, env: Env, locks: Locks | None) -> tuple[int | None, str, float]:
    """Run one launch; its log goes to out/<name>.log. (exit code, error, seconds)."""
    log = env.out / f"{launch.name}.log"
    started = time.monotonic()
    if launch.kind == GAME:
        aids = {}
        for key, value in game_env(launch).items():
            expanded = expand_game_value(value)
            if expanded is None:
                return None, f"{key}: needs {value}", 0.0
            aids[key] = expanded.replace("{repo}", str(env.repo))
        with serving(launch_serve(launch)) as served:
            # `{repo}` in an argument: the repository (a fixture's path).
            args = [a.replace("{repo}", str(env.repo)) for a in launch.args]
            cmd = [str(env.exe), "--data", str(env.data), *args, "--run-seconds", fmt(launch.run_seconds)]
            # Environment values stay scalar: filenames, XML and script source are not
            # tool arguments, so they must not be split or globbed by expand().
            if served:
                aids = {k: v.replace("{serve}", served) for k, v in aids.items()}
            (env.out / f"{launch.name}.cmd.txt").write_text(
                "".join(f"{k}={v}\n" for k, v in sorted(aids.items())) + subprocess.list2cmdline(cmd) + "\n")
            (env.out / f"{launch.name}.stacks.txt").unlink(missing_ok=True)
            (env.out / f"{launch.name}.front.txt").unlink(missing_ok=True)
            held = locks.acquire() if locks else None  # a smoke slot, or None for the old locks
            started = time.monotonic()  # the launch's own time, without waiting for the locks
            locked = session_locked()
            try:
                with open(log, "w", encoding="utf-8", errors="replace") as f:
                    p = subprocess.Popen(cmd, stdout=f, stderr=subprocess.STDOUT, cwd=env.out,
                                         env=aid_free_environ(aids), **launch_popen_kw())
                    exe = Path(env.exe).name
                    # `front = true`: the launch's own window held in front until it ends.
                    front = Front(p.pid, log=env.out / f"{launch.name}.front.txt").start() if launch.front else None
                    try:
                        code = p.wait(timeout=launch_timeout(launch))
                        # A load hands over to a new run that goes on writing to this log.
                        err = wait_rest(p, exe, started + launch_timeout(launch))
                    except subprocess.TimeoutExpired:
                        # Where it stopped, before it goes: every thread's stack.
                        stacks = env.out / f"{launch.name}.stacks.txt"
                        stacks.write_text(dump_stacks(p.pid, Path(env.exe).parent), encoding="utf-8")
                        born = own_times(p)
                        p.kill()
                        p.wait()
                        # A new run it started goes too: it is this launch's.
                        if os.name == "nt":
                            wait_for_children(p.pid, child_pids(p.pid, born, exe), time.monotonic())
                        else:
                            wait_for_group(p.pid, time.monotonic())
                        code, err = None, f"timed out after {launch_timeout(launch):.0f} s; thread stacks in {stacks}"
                    finally:
                        if front:
                            front.stop()
            finally:
                if locks:
                    locks.release(held)
            locked = bool(locked) or bool(session_locked())
            if locked:
                env.say(f"warning: {launch.name}: {LOCKED_NOTE}")
    else:
        s = launch.scenarios[0]
        ctx = {"data": str(env.data), "repo": str(env.repo)}
        code, err = 0, ""
        cmds = []
        with open(log, "w", encoding="utf-8", errors="replace") as f:
            for c in s.commands:
                args = []
                for a in c:
                    x = expand(a, ctx, env.masters)
                    if x is None:
                        return None, f"needs {a}", 0.0
                    args += x
                cmd = [str(env.fnvtool), *args]
                cmds.append(subprocess.list2cmdline(cmd))
                f.flush()
                try:
                    r = subprocess.run(cmd, stdout=f, stderr=subprocess.STDOUT, cwd=env.repo,
                                       timeout=launch_timeout(launch))
                except subprocess.TimeoutExpired:
                    code, err = None, "timed out"
                    break
                except OSError as e:
                    # No fnvtool built (or not runnable): this launch fails with the reason, and
                    # the run goes on to the rest and its summary (#4740).
                    code, err = None, f"cannot start fnvtool at {env.fnvtool}: {e}"
                    break
                if r.returncode != 0:
                    code = r.returncode
                    break
        (env.out / f"{launch.name}.cmd.txt").write_text("\n".join(cmds) + "\n")
    secs = time.monotonic() - started
    meta = {"exit": code, "error": err, "seconds": round(secs, 1)}
    if launch.kind == GAME and locked:
        meta["session_locked"] = True
    (env.out / f"{launch.name}.exit").write_text(json.dumps(meta))
    return code, err, secs


def missing_requirements(s: Scenario) -> list[str]:
    """The `requires` entries not met: a variable that is unset or empty, or a `!variable`
    that is set."""
    return [r for r in s.requires if bool(os.environ.get(r.removeprefix("!"))) == r.startswith("!")]


def missing_data_files(s: Scenario, data: str | Path | None) -> list[str]:
    """The `data_files` the Data folder does not hold: a mod's plugin the scenario loads
    that the repository cannot ship. Names are compared without case, as Windows finds
    them. With no Data folder given none are missing (nothing is launched then)."""
    if not s.data_files or not data:
        return []
    try:
        here = {n.lower() for n in os.listdir(data)}
    except OSError:
        here = set()
    return [f for f in s.data_files if f.lower() not in here]


def platform_name() -> str:
    """The system this runner is on, as a scenario's `platforms` names it."""
    if sys.platform.startswith("win"):
        return "windows"
    if sys.platform == "darwin":
        return "macos"
    return "linux"


def unmet_platforms(s: Scenario, here: str | None = None) -> list[str]:
    """`platforms` this scenario asks for that this system is not (empty: it runs anywhere)."""
    here = here or platform_name()
    return [p for p in s.platforms if p != here]


def report(v: Verdict, launch: Launch, secs: float | None, log: Path, say=print, context: int = 3) -> None:
    where = f" [{launch.name}]" if launch.shared else ""
    t = f" {secs:.0f} s" if secs is not None else ""
    if v.status == "SKIP":
        say(f"SKIP  {v.scenario}{where}: {v.reason}")
        return
    say(f"{v.status:<5} {v.scenario}{where}{t}" + (f"  log {log}" if v.status == "FAIL" else ""))
    for o in v.failures:
        tag = "known" if o.known else o.what
        say(f"      {tag}: {o.label!r}" + (f" ({o.detail})" if o.detail else "")
            + (f" -- {o.known}" if o.known else ""))
        if not o.known:
            for c in o.context[:context]:
                say(f"        > {c}")
    for x in v.xpass:
        say(f"      now passes, drop its known_failing: {x!r}")
    if v.manual and v.status != "PASS":
        say(f"      by hand: {v.manual}")


def exe_is_stale(exe: Path, repo: Path, files: list[str]) -> list[str]:
    """Changed engine/crate sources newer than the exe."""
    try:
        t = exe.stat().st_mtime
    except OSError:
        return []
    out = []
    for f in files:
        if f.startswith(("engine/", "crates/")) and f.endswith((".rs", ".wgsl", ".wesl", ".toml")):
            try:
                if (repo / f).stat().st_mtime > t:
                    out.append(f)
            except OSError:
                pass
    return out


def python() -> str:
    return sys.executable or "python"
