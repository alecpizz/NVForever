"""Unit tests for tools/stress (no game needed): `python -m unittest discover -s tools/stress/tests`."""
from __future__ import annotations

import math
import os
import re
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from stress import battle, cells, lib, markers, soak, tour  # noqa: E402
from stress import cli  # noqa: E402

REPORT = textwrap.dedent("""\
    NVForever 0.2.0 crashed (panic)
    time (UTC): 2026-10-18 10:00:00
    thread: Compute Task Pool (3)
    message: index out of bounds: the len is 3 but the index is 7
    at: crates/fnv-nif/src/scene.rs:812:17
    where: WastelandNV cell -2,26 CrimsonCaravan, position 1, 2, 3
    command line: nvforever.exe
    run log: C:/h/logs/run.log

    backtrace:
       0: std::backtrace_rs::backtrace::trace::h0123456789abcdef
       1: std::panicking::begin_panic_handler
       2: core::panicking::panic_bounds_check
       3: fnv_nif::scene::Decoded::block::h89abcdef01234567
       4: nvf_assets::models::load
       5: nvforever::world::spawn_cell
       6: bevy_ecs::system::function_system::run
       7: BaseThreadInitThunk
    """)


class CrashReports(unittest.TestCase):
    def test_a_report_gives_message_place_and_the_engines_own_frames(self):
        c = lib.parse_crash(REPORT, "crash-1.txt")
        self.assertEqual(c.message, "index out of bounds: the len is 3 but the index is 7")
        self.assertEqual(c.location, "crates/fnv-nif/src/scene.rs:812:17")
        self.assertEqual(c.frames[:3], ["fnv_nif::scene::Decoded::block", "nvf_assets::models::load", "nvforever::world::spawn_cell"])
        self.assertNotIn("BaseThreadInitThunk", c.frames)

    def test_crashes_group_by_cause_whatever_the_numbers(self):
        a = lib.parse_crash(REPORT)
        b = lib.parse_crash(REPORT.replace("index is 7", "index is 9").replace(":812:17", ":812:30"))
        other = lib.parse_crash(REPORT.replace("index out of bounds", "called `Option::unwrap()` on a `None` value"))
        groups = lib.group_crashes([("cell A", a), ("cell B", b), ("cell C", other)])
        self.assertEqual([len(g["where"]) for g in groups], [2, 1])
        self.assertEqual(groups[0]["where"], ["cell A", "cell B"])


def fake_world():
    wl = {"worldspaces": [{"formid": "0x000DA726", "edid": "WastelandNV"}, {"formid": "0x01000F2B", "edid": "SierraMadre"}]}
    per = {
        "0x000DA726": {"cells": [{"formid": "0x000DA727", "edid": None, "grid": None},
                                 {"formid": "0x000DA7A6", "edid": "Goodsprings", "grid": [-17, 1]},
                                 {"formid": "0x000DA7A7", "edid": None, "grid": [-16, 2]}]},
        "0x01000F2B": {"cells": [{"formid": "0x01000F30", "edid": None, "grid": [0, 0]}]},
    }
    ints = {"cells": [{"formid": "0x00010C0E", "edid": "GSGeneralStore"}]}
    return cells.cells_from_json(wl, per, ints)


class CellsTest(unittest.TestCase):
    def test_the_list_has_interiors_then_grid_cells_without_persistent_ones(self):
        cs = fake_world()
        self.assertEqual([c.kind for c in cs], ["interior", "exterior", "exterior", "exterior"])
        self.assertEqual(cs[0].line(), "interior 00010C0E GSGeneralStore")
        self.assertEqual(cs[1].line(), "exterior 000DA726 -17 1 000DA7A6 Goodsprings")
        self.assertEqual(cs[2].label, "WastelandNV")  # a cell without its own Editor ID

    def test_choose_by_kind_stride_world_and_limit(self):
        cs = fake_world()
        self.assertEqual(len(cells.choose(cs, "interior")), 1)
        self.assertEqual([c.cell for c in cells.choose(cs, "exterior", stride=2)], ["000DA7A7", "01000F30"])
        self.assertEqual([c.world for c in cells.choose(cs, "exterior", worlds=["SierraMadre"])], ["01000F2B"])
        self.assertEqual(len(cells.choose(cs, limit=2)), 2)
        self.assertEqual([len(b) for b in cells.batches(cs, 3)], [3, 1])

    def test_a_batch_log_gives_loads_skips_and_the_last_cell_started(self):
        lines = ["INFO nvf_play::stress: stress cell: 1/3 start interior 00010C0E GSGeneralStore",
                 "INFO nvf_play::stress: stress cell: 1/3 loaded in 1.25 s interior 00010C0E GSGeneralStore",
                 "INFO nvf_play::stress: stress cell: 2/3 start exterior ...",
                 "INFO nvf_play::stress: stress cell: 2/3 skipped: not in the worldspace's grid",
                 "INFO nvf_play::stress: stress cell: 3/3 start exterior ..."]
        self.assertEqual(cells.read_batch(lines), {"loaded": {1: 1.25}, "skipped": {2: "not in the worldspace's grid"}, "last_started": 3})

    def test_missing_files_and_script_errors_go_to_the_cell_being_visited(self):
        lines = ["stress cell: 1/2 start interior 00010C0E GSGeneralStore",
                 "WARN nvf_assets::guard: model not loaded: meshes\\x\\a.nif: no such file",
                 "stress cell: 1/2 loaded in 1.25 s interior 00010C0E GSGeneralStore",
                 "WARN nvf_assets::guard: texture not loaded: textures\\b.dds: bad header",
                 "stress cell: 2/2 start interior 00010C0F Other",
                 "WARN nvf_play::script: script error in FalloutNV.esm|SCPT 0x00012345: step budget of 100000 statements exceeded",
                 "WARN nvf_assets::guard: model not loaded: meshes\\x\\a.nif: no such file"]
        p = cells.read_problems(lines)
        self.assertEqual(sorted(p), [1, 2])
        self.assertEqual(p[1]["model"], ["meshes\\x\\a.nif: no such file"])
        self.assertEqual(len(p[1]["texture"]), 1)
        self.assertEqual(sorted(p[2]), ["model", "script"])
        rows = [{"cell": "A", "status": "ok", "seconds": 1, "problems": p[1]}, {"cell": "B", "status": "ok", "seconds": 1, "problems": p[2]}]
        s = cells.problem_summary(rows)
        self.assertEqual((s["model"]["cells"], s["model"]["distinct"], s["model"]["top"][0]["cells"]), (2, 1, 2))
        self.assertEqual(s["script"]["top"][0]["what"], "FalloutNV.esm|SCPT 0x00012345: step budget of # statements exceeded")
        self.assertTrue(any("Missing files" in ln for ln in cells.report(cells.summarize(rows, [], 1))))

    def test_markers_come_from_the_map_marker_references(self):
        wl = {"worldspaces": [{"formid": "0x000DA726", "edid": "WastelandNV"}]}
        refs = {"0x000DA726": {"refs": [{"formid": "0x00090A7B", "base": "0x00000010"}, {"formid": "0x00090A7C", "base": "0x00000011"},
                                        {"formid": "0x00090A7D", "base": "0x00000010", "deleted": True}]}}
        ms = markers.markers_from_json(wl, refs)
        self.assertEqual([m.name() for m in ms], ["WastelandNV:00090A7B"])
        a = markers.aids(ms + [markers.Marker("00090A7E", "000DA726", "WastelandNV")], every=30)
        self.assertIn("ShowAllMapMarkers:1:1:0", a["NVF_PLAY_TEST_SCRIPT"])
        self.assertEqual(a["NVF_PLAY_TEST_PIPBOY"].split(",")[:4], ["8:open", "8.5:page=data", "9:travel=0x00090A7B", "10:key=use"])
        self.assertTrue(a["NVF_PLAY_TEST_PIPBOY"].split(",")[4].startswith("38:"))

    def test_a_markers_log_gives_refusals_trips_arrivals_and_problems(self):
        part = [markers.Marker("00090A7B", "000DA726", "W"), markers.Marker("00090A7C", "000DA726", "W"), markers.Marker("00090A7D", "000DA726", "W")]
        lines = [f"INFO test aid: pipboy Travel({0x90A7B}) at 9.0 s",
                 "INFO fast travel: to Goodsprings (00090A7B): arrive at the marker [1, 2, 3], 10 units away, 0.10 hours on the clock",
                 "INFO loading screen up: x for worldspace 000DA726",
                 "WARN model not loaded: meshes\\a.nif: gone",
                 "INFO loading screen down after 3.50 s",
                 f"INFO test aid: pipboy Travel({0x90A7C}) at 39.0 s",
                 "INFO fast travel: to Primm (00090A7C) refused (HostilesNear); the box says \"...\"",
                 f"INFO test aid: pipboy Travel({0x90A7D}) at 69.0 s"]
        seen = markers.read_batch(lines, part)
        self.assertEqual(seen[0]["arrived"], 3.5)
        self.assertEqual(seen[0]["problems"]["model"], ["meshes\\a.nif: gone"])
        self.assertEqual(seen[1]["refused"], "HostilesNear")
        self.assertEqual([markers.status(seen[i]) for i in range(3)], ["ok", "refused", "no trip"])

    def test_a_crash_ends_its_batch_and_the_sweep_goes_on_after_the_cell(self):
        cs = fake_world()
        calls = []

        def fake_run(ctx, launch):
            listed = Path(launch.aids["NVF_PLAY_TEST_CELLS"].split(";")[0]).read_text().splitlines()[1:]
            calls.append(listed)
            lines, status, crashes = [], "ok", []
            for i, ln in enumerate(listed, 1):
                lines.append(f"stress cell: {i}/{len(listed)} start {ln}")
                if "000DA7A6" in ln:  # Goodsprings crashes
                    status, crashes = "crashed", [lib.parse_crash(REPORT)]
                    break
                lines.append(f"stress cell: {i}/{len(listed)} loaded in 0.5 s {ln}")
            return lib.Result(launch.name, 101 if crashes else 0, 1.0, status, Path("x.log"), lines, crashes, [])

        with tempfile.TemporaryDirectory() as d:
            ctx = lib.Ctx(Path(d), Path("nvforever"), "Data", Path("fnvtool"), Path(d), say=lambda *_: None)
            real, lib.run = lib.run, fake_run
            try:
                res = cells.run(ctx, cs, batch=3, dwell=0, hang=10, start_args=[])
            finally:
                lib.run = real
        self.assertEqual(len(calls), 2)
        self.assertEqual(len(calls[1]), 2)  # the two cells after Goodsprings
        self.assertEqual((res["total"], res["ok"], res["crashed"]), (4, 3, 1))
        self.assertEqual(res["crash_groups"][0]["where"], [cs[1].name()])
        self.assertTrue(any("Crash groups" in ln for ln in cells.report(res)))


def many_cells(n: int) -> list[cells.Cell]:
    return [cells.Cell("exterior", f"{0x100000 + i:08X}", "WastelandNV", "000DA726", i % 40 - 20, i // 40 - 20) for i in range(n)]


def fake_game(crash_on: set[str], crash_after_load: bool = False, calls: list | None = None):
    """A stand-in for lib.run: logs every cell of the batch's list as loaded, and crashes
    on the cells named (before their load line, or after it)."""
    def run(ctx, launch):
        listed = Path(launch.aids["NVF_PLAY_TEST_CELLS"].split(";")[0]).read_text().splitlines()[1:]
        if calls is not None:
            calls.append(len(listed))
        lines, status, crashes = [], "ok", []
        for i, ln in enumerate(listed, 1):
            lines.append(f"stress cell: {i}/{len(listed)} start {ln}")
            bad = any(c in ln for c in crash_on)
            if bad and not crash_after_load:
                status, crashes = "crashed", [lib.parse_crash(REPORT)]
                break
            lines.append(f"stress cell: {i}/{len(listed)} loaded in 0.5 s {ln}")
            if bad:
                status, crashes = "crashed", [lib.parse_crash(REPORT)]
                break
        return lib.Result(launch.name, 101 if crashes else 0, 1.0, status, Path("x.log"), lines, crashes, [])
    return run


class SweepTest(unittest.TestCase):
    def sweep(self, out: Path, cs, batch: int, game, on_batch=None):
        ctx = lib.Ctx(out, Path("nvforever"), "Data", Path("fnvtool"), out, say=lambda *_: None)
        real, lib.run = lib.run, game
        try:
            return cells.run(ctx, cs, batch=batch, dwell=0, hang=10, start_args=[], on_batch=on_batch)
        finally:
            lib.run = real

    def test_a_big_batch_is_one_launch_and_stride_takes_negative_grid_lines(self):
        cs = many_cells(250)
        calls = []
        with tempfile.TemporaryDirectory() as d:
            res = self.sweep(Path(d), cs, 250, fake_game(set(), calls=calls))
        self.assertEqual((calls, res["total"], res["ok"]), ([250], 250, 250))
        strided = cells.choose(cs, "exterior", stride=4)
        self.assertTrue(strided and all(c.x % 4 == 0 and c.y % 4 == 0 for c in strided))
        self.assertIn(-20, {c.x for c in strided})

    def test_a_crash_after_the_load_line_is_still_that_cells(self):
        cs = many_cells(5)
        with tempfile.TemporaryDirectory() as d:
            res = self.sweep(Path(d), cs, 5, fake_game({cs[2].cell}, crash_after_load=True))
        self.assertEqual((res["total"], res["ok"], res["crashed"]), (5, 4, 1))
        bad = [r for r in res["cells"] if r["status"] == "crashed"]
        self.assertEqual((bad[0]["cell"], bad[0]["loaded_in"]), (cs[2].name(), 0.5))

    def test_a_stopped_sweep_resumes_with_its_whole_state(self):
        cs = many_cells(9)
        reports = []
        with tempfile.TemporaryDirectory() as d:
            out = Path(d)
            # The runner stops after its second batch (as if the machine went down).
            class Stop(Exception):
                pass

            def stop_after_two(res):
                reports.append(res["total"])
                if len(reports) == 2:
                    raise Stop

            with self.assertRaises(Stop):
                self.sweep(out, cs, 3, fake_game({cs[1].cell}), stop_after_two)
            self.assertTrue((out / cells.STATE).exists())
            calls = []
            res = self.sweep(out, cs, 3, fake_game({cs[1].cell}, calls=calls))
        # First run: batch 1 crashed on cell 2 (cells 1-2 done), batch 2 did cells 3-5.
        self.assertEqual(reports, [2, 5])
        self.assertEqual(calls, [3, 1])  # the rest: cells 6-8, then 9
        self.assertEqual((res["total"], res["ok"], res["crashed"], res["launches"]), (9, 8, 1, 4))
        self.assertEqual(res["crash_groups"][0]["where"], [cs[1].name()])
        self.assertEqual(len({r["cell"] for r in res["cells"]}), 9)


def no_start(failures: list[int]):
    """A game that does not start (0xC0000142, DLL init failed) for the launches whose
    numbers are listed (1-based), and works on the others."""
    count = [0]
    good = fake_game(set())

    def run(ctx, launch):
        count[0] += 1
        if count[0] in failures:
            return lib.Result(launch.name, 3221225794, 0.5, "failed", Path("x.log"), ["(nothing)"], [], [])
        return good(ctx, launch)
    return run


class StartFailureTest(unittest.TestCase):
    def sweep(self, out: Path, cs, game, abort_after=3):
        ctx = lib.Ctx(out, Path("nvforever"), "Data", Path("fnvtool"), out, say=lambda *_: None)
        real, lib.run = lib.run, game
        try:
            return cells.run(ctx, cs, batch=4, dwell=0, hang=10, start_args=[], abort_after=abort_after)
        finally:
            lib.run = real

    def test_a_game_that_does_not_start_aborts_and_blames_no_cell(self):
        cs = many_cells(10)
        with tempfile.TemporaryDirectory() as d:
            out = Path(d)
            res = self.sweep(out, cs, no_start(list(range(1, 100))))
            self.assertEqual((res["status"], res["launches"], res["crashed"], res["ok"]), ("aborted", 3, 0, 0))
            self.assertEqual((res["not_reached"], res["remaining"], res["total"], res["cells"]), (10, 10, 10, []))
            self.assertEqual(len(res["start_failures"]), 3)
            self.assertEqual(res["crash_groups"], [])
            self.assertIn("ABORTED", cells.report(res)[2])
            # Fixed, and resumed: every cell is visited, the earlier launches are kept.
            res = self.sweep(out, cs, no_start([]))
        self.assertEqual((res["status"], res["ok"], res["crashed"], res["launches"]), ("done", 10, 0, 6))
        self.assertEqual(len(res["start_failures"]), 3)

    def test_starts_that_fail_now_and_then_are_tried_again(self):
        cs = many_cells(8)
        with tempfile.TemporaryDirectory() as d:
            # Launches 1, 2 and 4 fail to start: never three in a row.
            res = self.sweep(Path(d), cs, no_start([1, 2, 4]))
        self.assertEqual((res["status"], res["ok"], res["not_reached"], res["launches"]), ("done", 8, 0, 5))
        self.assertEqual([f["batch"] for f in res["start_failures"]], ["batch-001", "batch-002", "batch-004"])


class TourTest(unittest.TestCase):
    def test_the_ring_lies_inside_the_grid(self):
        pts = tour.ring((-30, -30), (29, 29), n=4, inset=0.5)
        self.assertEqual(len(pts), 4)
        for x, y in pts:
            self.assertLess(abs(x), 30 * 4096 * 0.51)
            self.assertLess(abs(y), 30 * 4096 * 0.51)
        text = tour.route_text("0x000DA726", pts, 1024, 1, 2)
        self.assertIn("laps 2", text)
        self.assertIn("world 0x000DA726", text)

    def test_the_ring_stays_on_the_walkable_cells(self):
        # An L-shaped walkable area (a wide band along the south, a tall one up the west)
        # inside a grid that runs far beyond it, as WastelandNV's does.
        cells = [[x, y, 50] for x in range(-30, 11) for y in range(-20, -9)]
        cells += [[x, y, 50] for x in range(-30, -19) for y in range(-9, 21)]
        walkable = {(x, y) for x, y, _ in cells}
        pts = tour.ring_on_cells(cells, n=12, inset=0.2)
        self.assertGreaterEqual(len(pts), 6)
        cell = lambda p: (math.floor(p[0] / tour.CELL), math.floor(p[1] / tour.CELL))  # noqa: E731
        for p in pts:
            self.assertIn(cell(p), walkable, p)
        # Consecutive points (the closing leg too) a few cells apart at most, so a
        # straight step between them barely leaves the area.
        for a, b in zip(pts, pts[1:] + pts[:1]):
            self.assertLessEqual(math.hypot(b[0] - a[0], b[1] - a[1]), 3.5 * tour.CELL, (a, b))
        # The old ring, on the grid's bounds, left the area.
        old = tour.ring((-60, -60), (59, 59), 12, 0.3)
        self.assertTrue(any(cell(p) not in walkable for p in old))
        with self.assertRaises(ValueError):
            tour.ring_on_cells([])

    def test_the_town_route_goes_through_the_towns_on_walkable_cells(self):
        # A U of walkable cells: the towns at its ends and bottom; the gap inside is not
        # walkable, so the way from A to C goes round by B.
        nav = [[x, y, 9] for x in range(0, 3) for y in range(0, 10)]
        nav += [[x, y, 9] for x in range(3, 8) for y in range(0, 3)]
        nav += [[x, y, 9] for x in range(8, 11) for y in range(0, 10)]
        cells = [{"edid": "TownA", "grid": [1, 9]}, {"edid": "TownB", "grid": [5, 1]}, {"edid": "TownC", "grid": [9, 9]},
                 {"edid": "Persistent", "grid": None}]
        pts, stops = tour.town_route(cells, nav, ["TownA", "TownB", "TownC"])
        on = {(x, y) for x, y, _ in nav}
        cell = lambda p: (math.floor(p[0] / tour.CELL), math.floor(p[1] / tour.CELL))  # noqa: E731
        self.assertTrue(all(cell(p) in on for p in pts))
        self.assertEqual(cell(pts[0]), (1, 9))
        self.assertEqual(stops[0], "TownA at point 1")
        for a, b in zip(pts, pts[1:] + pts[:1]):
            self.assertLessEqual(math.hypot(b[0] - a[0], b[1] - a[1]), 3 * tour.CELL, (a, b))
        # The gap (x 3..7, y 3..9) is never stepped on.
        self.assertFalse(any(3 <= cell(p)[0] <= 7 and cell(p)[1] >= 3 for p in pts))
        # A town whose own cell has no navmesh is taken at the nearest navmesh cell.
        pts2, _ = tour.town_route(cells + [{"edid": "Off", "grid": [5, 6]}], nav, ["TownA", "Off"])
        self.assertTrue(all(cell(p) in on for p in pts2))
        with self.assertRaises(ValueError):
            tour.town_route(cells, nav, ["TownA", "Nowhere"])

    def test_a_slice_of_the_route(self):
        pts = [(i, i) for i in range(10)]
        self.assertEqual(tour.slice_points(pts, "3:5"), [(2, 2), (3, 3), (4, 4)])
        self.assertEqual(tour.slice_points(pts, "9:"), [(8, 8), (9, 9)])
        with self.assertRaises(ValueError):
            tour.slice_points(pts, "7:3")

    def test_a_slice_of_a_route_file(self):
        text = "# tools/stress tour\nworld 0x000DA726\nstep 1024\ndwell 1\nlaps 1\n1 1\n2 2\n3 3\n4 4\n"
        self.assertEqual(tour.slice_route_text(text, "2:3"), "# tools/stress tour\nworld 0x000DA726\nstep 1024\ndwell 1\nlaps 1\n2 2\n3 3\n")
        with self.assertRaises(ValueError):
            tour.slice_route_text(text, "4:2")

    def test_stray_entity_commands_are_counted_and_named(self):
        lines = [
            "WARN nvforever::stray_commands: stray entity command #1 (ignored; the entity is gone): command bevy_ecs::system::commands::entity_command::insert<Visibility>: Entity despawned: The entity with ID 4742v5 is invalid; where: WastelandNV cell -1,28, position -2958, 116277, 4443",
            "WARN nvforever::stray_commands: stray entity command #2 (ignored; the entity is gone): command bevy_ecs::system::commands::entity_command::insert<Visibility>: Entity despawned: The entity with ID 99v2 is invalid",
            "INFO other line",
        ]
        s = lib.stray_commands(lines)
        self.assertEqual(s["count"], 2)
        self.assertEqual(len(s["distinct"]), 1)
        self.assertTrue(s["distinct"][0]["what"].startswith("command bevy_ecs::system::commands::entity_command::insert<Visibility>"))
        self.assertEqual(s["distinct"][0]["where"], "WastelandNV cell -1,28, position -2958, 116277, 4443")
        self.assertIn("Stray entity commands (2 logged, ignored)", "\n".join(lib.stray_report(s)))
        self.assertEqual(lib.stray_report(lib.stray_commands([])), [])

    def sample(self, t, lap_entities):
        return (f"INFO nvforever::stress_sample: stress sample: t {t:.1f} s, frames 60, frame mean 16.70 ms, frame worst 30.10 ms, "
                f"entities {lap_entities}, refs drawn 2000, meshes 900, images 1200, materials 300, audio 4, cells 25, "
                f"bodies 3000/10/40, actors 12, nan actors 0, ragdolls 0, projectiles 0, at WastelandNV cell -17,1 Goodsprings, position 1, 2, 3")

    def test_a_sample_line_reads_into_numbers(self):
        s = tour.parse_sample(self.sample(5, 10000))
        self.assertEqual((s["t"], s["entities"], s["bodies"], s["refs drawn"], s["frame mean"]), (5.0, 10000, 3050, 2000, 16.7))
        self.assertTrue(s["at"].startswith("WastelandNV cell -17,1"))

    def test_current_sample_solver_fields_and_loading_exclusion_suffix(self):
        line = self.sample(5, 10000).replace(
            "bodies 3000/10/40,", "bodies 3000/10/40, bodies awake 6, contact pairs 7, manifolds 8, "
            "solver contacts 9, joints 2, solver ms 1/2/3/4/5/6,")
        line = line.replace("audio 4,", "models cached 19, textures cached 23, audio 4,")
        sample = tour.parse_sample(line)
        self.assertEqual((sample["bodies"], sample["bodies awake"], sample["solver ms"]), (3050, 6, 21))
        self.assertEqual((sample["models cached"], sample["textures cached"]), (19, 23))
        match = tour.FRAMES.search("stress frames: 120 frames, mean 12.34 ms, 1% low 67.89 ms "
                                   "(14.7 fps), worst 123.45 ms; 11 behind loading screens left out")
        self.assertEqual(match.groups(), ("120", "12.34", "67.89", "14.7", "123.45"))

    def test_solver_fields_and_phase_window_fields_read_together(self):
        line = (self.sample(5, 10000)
                .replace("audio 4", "models cached 7, textures cached 9, audio 4")
                .replace("bodies 3000/10/40", "bodies 3000/10/40, bodies awake 12, contact pairs 13, manifolds 14, "
                         "solver contacts 15, joints 16, solver ms 1/2/3/4/5/6")
                .replace("projectiles 0", "projectiles 0, gpu allocated 101, gpu reserved 202, gpu blocks 3, "
                         "startup frames 0, hidden frames 12, phase changes 1, window 2004 ms"))
        self.assertEqual(tour.parse_sample(line), {
            "t": 5, "frames": 60, "frame mean": 16.7, "frame worst": 30.1,
            "entities": 10000, "refs drawn": 2000, "meshes": 900, "images": 1200, "materials": 300,
            "models cached": 7, "textures cached": 9, "audio": 4, "cells": 25, "bodies": 3050,
            "bodies awake": 12, "contact pairs": 13, "manifolds": 14, "solver contacts": 15,
            "joints": 16, "solver ms": 21, "actors": 12, "nan actors": 0, "ragdolls": 0,
            "projectiles": 0, "gpu allocated": 101, "gpu reserved": 202, "gpu blocks": 3,
            "startup frames": 0, "hidden frames": 12, "phase changes": 1, "window": 2004,
            "at": "WastelandNV cell -17,1 Goodsprings, position 1, 2, 3",
        })

    def test_mesh_allocator_fields_read(self):
        # #3057: the mesh allocator's work over the window.
        line = self.sample(5, 10000).replace("projectiles 0", "projectiles 0, window 2004 ms, mesh alloc 12.5 ms, "
                                             "mesh alloc worst 8.25 ms, meshes written 340, meshes freed 210, "
                                             "slabs made 3, slab mb 24.0, slab mb copied 16.0")
        s = tour.parse_sample(line)
        self.assertEqual((s["mesh alloc"], s["mesh alloc worst"], s["meshes written"], s["meshes freed"]), (12.5, 8.25, 340, 210))
        self.assertEqual((s["slabs made"], s["slab mb"], s["slab mb copied"]), (3, 24.0, 16.0))

    def test_a_window_across_a_phase_change_is_flagged(self):
        """Issue #1415: a window a loading screen came down in says so; the old line (no
        window fields) still reads."""
        line = (self.sample(5.1, 9000).replace(", at WastelandNV", ", startup frames 0, hidden frames 12, phase changes 1, "
                                                "window 2004 ms, at WastelandNV"))
        s = tour.parse_sample(line)
        self.assertEqual((s["hidden frames"], s["phase changes"], s["window"], s["startup frames"]), (12, 1, 2004, 0))
        self.assertTrue(s["at"].startswith("WastelandNV cell -17,1"))
        self.assertEqual(s["entities"], 9000)
        old = tour.parse_sample(self.sample(7, 9000))
        self.assertNotIn("phase changes", old)
        self.assertEqual(tour.windows_across_changes([s, old]), 1)

    def test_hitches_across_a_phase_change_are_kept_apart(self):
        lines = [
            "INFO nvforever::stress_sample: stress hitch (across a phase change, hidden to seen): 674 ms at t 4.6 s, "
            "0 cells loaded in it (0 ms), 0 actors drawn in it (0 ms), from WastelandNV cell -17,1 Goodsprings, "
            "position 1, 2, 3 to WastelandNV cell -17,1 Goodsprings, position 1, 2, 3",
            "INFO nvforever::stress_sample: stress hitch: 300 ms at t 9.0 s, 1 cells loaded in it (180 ms), "
            "0 actors drawn in it (0 ms), at WastelandNV cell -16,1",
            "INFO nvforever::stress_sample: stress frames: 500 frames, mean 16.00 ms, 1% low 40.00 ms (25.0 fps), "
            "worst 300.00 ms; 40 behind loading screens left out, 1 across a phase change left out",
        ]
        crossing = tour.crossing_hitches(lines)
        self.assertEqual(len(crossing), 1)
        h = crossing[0]
        self.assertEqual((h["from"], h["to"], h["ms"], h["t"], h["cells"]), ("hidden", "seen", 674.0, 4.6, 0))
        self.assertEqual(h["from_at"], "WastelandNV cell -17,1 Goodsprings, position 1, 2, 3")
        self.assertEqual(h["at"], "WastelandNV cell -17,1 Goodsprings, position 1, 2, 3")
        # Not a seen hitch: the slowest-hitches table and the soak's causes leave it out.
        _, seen = tour.startup_and_hitches(lines)
        self.assertEqual([x["ms"] for x in seen], [300.0])
        m = tour.LEFT_OUT.search(lines[2])
        self.assertEqual((m.group(1), m.group(2)), ("40", "1"))
        self.assertTrue(tour.FRAMES.search(lines[2]))
        old = tour.LEFT_OUT.search("stress frames: 5 frames, mean 1.00 ms, 1% low 1.00 ms (1.0 fps), worst 1.00 ms; 3 behind loading screens left out")
        self.assertEqual((old.group(1), old.group(2)), ("3", None))

    def test_a_count_that_stays_up_on_coming_back_is_a_possible_leak(self):
        lines = []
        for lap in (1, 2, 3):
            lines.append(f"INFO stress tour: lap {lap} starts")
            for wp in (1, 2):
                lines.append(f"INFO stress tour: lap {lap} at waypoint {wp}/2 (0, 0)")
                lines.append(self.sample(lap * 10 + wp, 10000 + (lap - 1) * 2000))
        samples = tour.tag_laps(lines)
        self.assertEqual([(s["lap"], s["waypoint"]) for s in samples][:2], [(1, 1), (1, 2)])
        leaks = tour.growth(samples)
        self.assertIn("entities", leaks)
        self.assertTrue(leaks["entities"]["rising_every_lap"])
        self.assertEqual(leaks["entities"]["grew_at"][0]["first"], 10000)
        self.assertNotIn("meshes", leaks)

    def test_meshes_kept_at_a_laps_end_are_not_released(self):
        # Lap 1: meshes rise in a town and stay up back at the start; lap 2: they fall back.
        held = [800] * 5 + [4000] * 10 + [4700] * 5
        freed = [800] * 5 + [4000] * 10 + [850] * 5
        samples = ([{"lap": 1, "meshes": v, "images": 600} for v in held]
                   + [{"lap": 2, "meshes": v, "images": 600} for v in freed])
        bad = tour.not_released(samples)
        self.assertEqual(list(bad), ["meshes"])
        self.assertEqual(bad["meshes"], [{"lap": 1, "start": 800, "end": 4700, "peak": 4700}])

    def test_start_up_and_hitches_read_from_the_log(self):
        lines = ["INFO nvforever::stress_sample: stress start-up: 61 frames, 4210 ms, slowest 2545.84 ms",
                 "INFO stress hitch: 300 ms at t 1633.2 s, 2 cells loaded in it (180 ms), at WastelandNV cell -4,-12, position 1, 2, 3",
                 "INFO stress hitch: 340 ms at t 1663.3 s, 0 cells loaded in it (0 ms), at WastelandNV cell 2,-13",
                 "INFO stress hitch: 260 ms at t 1700.0 s, 0 cells loaded in it (0 ms), 3 actors drawn in it (190 ms), at WastelandNV cell 2,-13"]
        startup, hitches = tour.startup_and_hitches(lines)
        self.assertEqual(startup, {"frames": 61, "ms": 4210.0, "slowest_ms": 2545.84})
        self.assertEqual([(h["ms"], h["cells"]) for h in hitches], [(340.0, 0), (300.0, 2), (260.0, 0)])
        self.assertEqual((hitches[2]["actors"], hitches[2]["actor_ms"], hitches[2]["at"]), (3, 190.0, "WastelandNV cell 2,-13"))
        self.assertTrue(hitches[1]["at"].endswith("position 1, 2, 3"))

    def test_actor_drawing_summary_read_from_the_log(self):
        ln = ("INFO stress actors: 212 drawn in 150 frames (900 ms, most in a frame 48.5 ms); "
              "those frames mean 31.2 ms, worst 140 ms, 3 over 100 ms")
        m = tour.ACTORS.search(ln)
        self.assertEqual([m.group(i) for i in (1, 2, 4, 6, 7)], ["212", "150", "48.5", "140", "3"])

    def test_memory_goes_on_the_nearest_sample(self):
        samples = [{"t": 1.0}, {"t": 10.0}]
        tour.add_memory(samples, [(0.5, 100, 200), (9.0, 300, 400)])
        self.assertEqual((samples[0]["working set"], samples[1]["private bytes"]), (100, 400))


class BattleTest(unittest.TestCase):
    def test_the_layout_puts_the_sides_across_the_centre(self):
        aid, player = battle.layout(10, 0, (0.0, 0.0), 3000.0)
        self.assertEqual(aid, "5;LvlNCRTrooper:10:-1500:0;LvlLegionRaiderMelee:10:1500:0")
        self.assertEqual(player, (0.0, -1800.0))
        self.assertEqual(battle.layout(10, 5, (0.0, 0.0))[0].count(";"), 3)

    def test_the_log_gives_kills_hits_lines_switches_and_spacing(self):
        t = "2026-10-18T10:00:{:02d}.000000Z  INFO "
        lines = [
            t.format(0) + "nvf_play::stress: stress battle: group 1 LvlNCRTrooper (0x0010A4F2): 10 of 10 placed around (-1500, 0): 0xFF000800..0xFF000809",
            t.format(0) + "nvf_play::stress: stress battle: group 2 LvlLegionRaiderMelee (0x001252E6): 10 of 10 placed around (1500, 0): 0xFF00080A..0xFF000813",
            t.format(0) + "nvf_play::stress: stress battle: 2 groups, 20 fighters, 20 set on the nearest enemy",
            t.format(3) + "nvforever::npc_combat: npc combat: LvlNCRTrooper 0xFF000801 fights LvlLegionRaiderMelee 0xFF000810 with WeapNVServiceRifle (gun)",
            t.format(9) + "nvforever::npc_combat: npc combat: LvlNCRTrooper 0xFF000801 fights LvlLegionRaiderMelee 0xFF000811 with WeapNVServiceRifle (gun)",
            t.format(10) + "nvforever::npc_combat: npc combat: LvlLegionRaiderMelee 0xFF000810 swings at LvlNCRTrooper 0xFF000801 from 84 units (feet to feet, reach 96)",
            t.format(12) + "nvf_play::combat: combat line: 0xFF000801 says Attack line 0015B6C4",
            t.format(20) + "nvforever::combat_tactics: battle: LvlLegionRaiderMelee 0xFF000810 killed by LvlNCRTrooper 0xFF000801",
            # Created actors have no Editor ID: FormIDs alone.
            t.format(25) + "nvforever::combat_tactics: battle: 0xFF000802 killed by 0xFF000811",
            t.format(30) + "nvforever::npc_combat: npc combat: LvlLegionRaiderMelee 0xFF000811 takes cover 300 units away",
            t.format(59) + "nvforever::combat_tactics: battle summary: 2 sides, 20 actors; NCRFactionNV (10): 9 standing, 1 dead, 30 hits, 4 kills, 0 flights; CaesarsLegionFaction (10): 6 standing, 4 dead, 10 hits, 1 kills, 2 flights; first fighter at 1 s",
            t.format(59) + "nvforever::npc_combat: npc combat summary: 20 actors fought (...); 200 shots (200 from a muzzle, 0 from the eyes; 6 down the sights, 194 from the hip, sights raised 2 times), 12 blows (1 went on a combo, the longest 2; 0 missed), 7 reloads (0 round by round); 9 flee checks, 2 fled; the player was hit 0 times",
            t.format(59) + "nvf_play::combat: combat: 0 shots (0 drawn from the muzzle, 0 from behind cover), 200 projectiles, 50 actor hits, 150 world hits (0 objects' hit blocks, 0 objects knocked away), 0 explosions, 0 kills, 12 melee impacts",
            t.format(59) + "nvforever: actor animation: up to 17 actors posed at once, 0.799 ms per frame on average",
        ]
        c = battle.analyse(lines)
        self.assertEqual(c["placed"][0], {"group": 1, "base": "LvlNCRTrooper", "placed": 10, "asked": 10, "ids": [0xFF000800, 0xFF000809]})
        self.assertEqual((c["time_to_kill_s"]["first"], c["time_to_kill_s"]["count"]), (20.0, 2))
        self.assertEqual(c["deaths_by_group"], {"LvlLegionRaiderMelee": {"dead": 1, "median_s": 20.0, "last_s": 20.0},
                                                "LvlNCRTrooper": {"dead": 1, "median_s": 25.0, "last_s": 25.0}})
        self.assertEqual(c["melee_impacts"], 12)
        self.assertEqual((c["shots"], c["actor_hits"], c["hit_rate"], c["fled"]), (200, 50, 0.25, 2))
        self.assertEqual((c["target_switches"], c["cover_taken"]), (1, 1))
        self.assertEqual(c["melee_spacing"]["closest"], 84)
        self.assertEqual(c["combat_lines_per_minute"], {"Attack": 1.0})
        self.assertEqual([s["faction"] for s in c["sides"]], ["NCRFactionNV", "CaesarsLegionFaction"])
        self.assertIn("nvforever", c["system_times"])


class SoakTest(unittest.TestCase):
    REFS = [
        {"formid": "0x1", "base": "0xB1", "base_type": "CONT", "base_edid": "Box", "position": [100, 0, 0]},
        {"formid": "0x2", "base": "0xD1", "base_type": "DOOR", "base_edid": "Door", "position": [8000, 0, 0]},
        {"formid": "0x3", "base": "0xD2", "base_type": "DOOR", "base_edid": "Gate", "position": [0, 0, 0]},
        {"formid": "0x4", "base": "0xN1", "base_type": "NPC_", "base_edid": "LvlPowderGangerA", "persistent": True, "position": [0, 100, 0]},
        {"formid": "0x5", "base": "0xN2", "base_type": "NPC_", "base_edid": "GoodspringsSettler", "persistent": True, "position": [0, 100, 0]},
        {"formid": "0x6", "base": "0xB1", "base_type": "CONT", "base_edid": "Box", "position": [90000, 0, 0]},
        {"formid": "0x7", "base": "0xB1", "base_type": "CONT", "base_edid": "Box", "position": [0, 0, 0], "initially_disabled": True},
    ]

    def test_candidates_near_the_route_by_kind(self):
        doors = soak.teleport_bases({"doors": [{"door": "0xD1", "refs": 2, "teleport_refs": 2}, {"door": "0xD2", "refs": 3, "teleport_refs": 1}]})
        self.assertEqual(doors, {"0xD1"})
        c = soak.candidates(self.REFS, doors, [(0.0, 0.0), (8000.0, 0.0)])
        self.assertEqual([x["ref"] for x in c["container"]], ["0x1"])
        self.assertEqual([(x["ref"], x["route_point"]) for x in c["door"]], [("0x2", 1)])
        self.assertEqual([x["ref"] for x in c["fight"]], ["0x4"])

    def test_schedule_takes_turns(self):
        c = {"container": [{"ref": "0x1", "base": "Box"}], "door": [{"ref": "0x2", "base": "Door"}], "fight": [{"ref": "0x4", "base": "G"}]}
        steps, keys, events = soak.schedule(c, 20, 10, 50)
        self.assertEqual([e["kind"] for e in events], ["container", "door", "fight"])
        self.assertEqual(steps, ["20:moveto:player:0x1", "23:activate:0x1", "30:moveto:player:0x2", "33:activate:0x2",
                                 "40:moveto:0x4:player:300:0:0", "41:combat:0x4:player", "55:kill:0x4"])
        self.assertEqual(keys, ["26:tab"])
        self.assertEqual(soak.schedule({"container": []}, 0, 10, 100), ([], [], []))

    def test_hitches_put_down_to_the_event_before(self):
        lines = [
            "2026-10-27T20:00:00.000000Z  INFO nvf_play::script: test aids: clock starts when the scripts first run, at 0.0 s",
            "2026-10-27T20:00:23.000000Z  INFO nvforever::stress_sample: stress hitch: 400 ms at t 23.0 s, 0 cells loaded in it (0 ms), 0 actors drawn in it (0 ms), at X",
            "2026-10-27T20:00:23.100000Z  INFO nvforever::profile: profile hitch: 380 ms; spans ms: a 300.0 (1)",
            "2026-10-27T20:00:45.000000Z  INFO nvforever::stress_sample: stress hitch: 260 ms at t 45.0 s, 1 cells loaded in it (2 ms), 0 actors drawn in it (0 ms), at Y",
        ]
        by = soak.attribute(lines, [{"at": 20, "kind": "door"}, {"at": 30, "kind": "fight"}])
        self.assertEqual((by["door"]["stress"], by["door"]["profile"], by["door"]["worst_ms"]), (1, 1, 400.0))
        self.assertEqual(by["walking"]["stress"], 1)
        self.assertNotIn("fight", by)
        self.assertEqual(soak.outcomes(["menu: open Container(0x1)", "x"])["containers opened"], 1)


class PerfTest(unittest.TestCase):
    """The span profile's own hitch lines and the frame-time budget."""

    LINES = [
        "2026-10-27T19:46:58.857134Z  INFO nvforever::profile: profile hitch: 988 ms; spans ms: update 980.1 (1), "
        "Update 950.0 (1), world::load_cell 12.5 (3)",
        "2026-10-27T19:46:50.000000Z  INFO nvforever::profile: profile hitch: 120 ms; spans ms: render 110.0 (1)",
        "2026-10-27T19:46:51.000000Z  INFO nvforever::profile: profile hitch: 150 ms; no spans seen (build with --features profile)",
    ]

    def test_hitches_slowest_first_with_their_spans(self):
        h = lib.profile_hitches(self.LINES)
        self.assertEqual([x["ms"] for x in h], [988.0, 120.0])
        self.assertEqual(h[0]["spans"][2], {"span": "world::load_cell", "ms": 12.5, "entries": 3})
        rep = "\n".join(lib.profile_hitch_report(h))
        self.assertIn("| 988 | 8.9 | update 980.1 (1); Update 950 (1); world::load_cell 12.5 (3) |", rep)
        self.assertEqual(lib.profile_hitch_report([]), [])

    def test_current_pipeline_suffix_keeps_the_last_span(self):
        line = self.LINES[0] + "; pipelines 45 (2 new since the frame before, 3 waiting)"
        current = lib.profile_hitches([line])[0]
        legacy = lib.profile_hitches([self.LINES[0]])[0]
        self.assertEqual(current, legacy)

    def test_machine_load(self):
        self.assertIsNone(lib.machine_load([]))
        # Equal intervals: the plain means.
        load = lib.machine_load([(2, 0.1, 0.5, 2.0), (4, 0.1, 0.1, 2.0), (6, 0.2, 0.0, 2.0), (8, 0.2, 0.3, 2.0)])
        self.assertEqual(load, {"game_mean": 0.15, "others_mean": 0.225, "busy_fraction": 0.5, "busy_over": 0.25,
                                "scope": "launch intervals", "seconds": 8.0, "intervals": 4, "skipped": 0})
        report = "\n".join(lib.load_report(load))
        self.assertIn("The machine was busy", report)
        self.assertIn("over 8 s of the launch's sampled intervals, 4 kept; not a trace's steady-state window", report)
        quiet = lib.load_report(lib.machine_load([(2, 0.2, 0.01, 2.0)]))
        self.assertEqual(len(quiet), 2)
        self.assertEqual(lib.load_report(None), [])
        self.assertIsNone(lib.machine_load([(0.0, 0.5, 0.5, 0.0)]))

    def test_machine_load_weights_intervals_by_their_time(self):
        # One second at 10 % and three at 50 %: the mean is 40 %, not 30 %; others over the
        # bar for 3 of the 4 seconds.
        load = lib.machine_load([(1, 0.1, 0.1, 1.0), (4, 0.5, 0.5, 3.0)], skipped=2)
        self.assertEqual((load["game_mean"], load["others_mean"], load["busy_fraction"]), (0.4, 0.4, 0.75))
        self.assertEqual((load["seconds"], load["intervals"], load["skipped"]), (4.0, 2, 2))
        self.assertIn("2 kept, 2 left out", "\n".join(lib.load_report(load)))

    def test_cpu_load_first_read_is_a_baseline(self):
        """Issue #1412: the counters' first read gave 195 % and 390 % of the machine over the
        moment since their creation. The first read starts the first interval only."""
        cpu = lib.CpuLoad(cores=4, started=100.0)
        # Baseline right at the launch: no sample, whatever the counters say.
        self.assertIsNone(cpu.read(100.0, 1000.0, 50.0))
        # 2 s on 4 cores = 8 core-seconds: the game 2 (25 %), the machine 4 (others 25 %).
        self.assertEqual(cpu.read(102.0, 1004.0, 52.0), (2.0, 0.25, 0.25, 2.0))
        # 0.5 s: the game 0.5 (25 %), the machine 1.8 (others 65 %).
        self.assertEqual(cpu.read(102.5, 1005.8, 52.5), (2.5, 0.25, 0.65, 0.5))
        self.assertEqual(len(cpu.samples), 2)
        self.assertEqual(cpu.skipped, 0)
        # No sample at the launch's second 0; the summary weights the two by their time.
        self.assertTrue(all(s[0] > 0 for s in cpu.samples))
        load = lib.machine_load(cpu.samples, cpu.skipped)
        self.assertEqual((load["game_mean"], load["others_mean"], load["busy_fraction"], load["seconds"]),
                         (0.25, 0.33, 0.2, 2.5))

    def test_cpu_load_failed_reads_and_impossible_intervals(self):
        cpu = lib.CpuLoad(cores=2, started=0.0)
        self.assertIsNone(cpu.read(0.0, None, 1.0))  # a failed read is no baseline
        self.assertIsNone(cpu.read(0.1, 10.0, 1.0))  # the first good one is
        self.assertIsNone(cpu.read(1.0, None, 1.5))  # a failed read keeps the baseline
        # 1.9 s since the baseline, 3.8 core-seconds: the game 0.95 (25 %), the machine 1.9.
        self.assertEqual(cpu.read(2.0, 11.9, 1.95), (2.0, 0.25, 0.25, 1.9))
        # No time passed: no sample, the baseline kept, and nothing counted as left out
        # (no time is lost).
        self.assertIsNone(cpu.read(2.0, 12.0, 2.0))
        self.assertEqual(cpu.skipped, 0)
        # More processor time than the cores have (an impossible share): left out, not zero.
        self.assertIsNone(cpu.read(3.0, 20.0, 2.0))
        self.assertEqual(cpu.skipped, 1)
        self.assertEqual(len(cpu.samples), 1)
        # The discard moved the baseline to its end (3.0 s): the next interval is 1 s from
        # there, not 2 s from the last kept sample. 2 core-seconds: the game 0.4 (20 %), the
        # machine 0.8 (others 20 %).
        self.assertEqual(cpu.read(4.0, 20.8, 2.4), (4.0, 0.2, 0.2, 1.0))
        self.assertEqual((cpu.skipped, len(cpu.samples)), (1, 2))

    def test_budget(self):
        frames = {"mean_ms": 17.0, "low1_ms": 53.0, "worst_ms": 988.0}
        self.assertEqual(lib.over_budget(frames, lib.Budget()), [])
        self.assertEqual(lib.over_budget(frames, lib.Budget(mean_ms=20, low1_ms=60)), [])
        failed = lib.over_budget(frames, lib.Budget(low1_ms=33.4, worst_ms=250))
        self.assertEqual(failed, ["1% low frame 53 ms over the budget's 33.4 ms", "worst frame 988 ms over the budget's 250 ms"])
        self.assertEqual(lib.over_budget(None, lib.Budget(worst_ms=250)), ["no frame statistics in the log (the run ended early?)"])
        self.assertIn("Budget: FAIL (1% low 33.4 ms, worst 250 ms)", lib.budget_report(lib.Budget(low1_ms=33.4, worst_ms=250), failed))
        self.assertEqual(lib.budget_report(lib.Budget(worst_ms=250), []), ["", "Budget: PASS (worst 250 ms)."])


class ComparisonTest(unittest.TestCase):
    def frames(self, **more):
        return {"status": "ok", "machine": {"busy_fraction": 0.0},
                "frames": {"count": 100, "mean_ms": 10.0, "low1_ms": 20.0, "worst_ms": 30.0}, **more}

    def test_repetition_option_is_positive_and_legacy_default_is_one(self):
        args = cli.parser().parse_args(["run", "tour"])
        self.assertEqual((args.repetitions, args.build_label), (1, None))
        args = cli.parser().parse_args(["run", "battle", "--repetitions", "3", "--build-label", "candidate"])
        self.assertEqual((args.repetitions, args.build_label), (3, "candidate"))
        with self.assertRaises(SystemExit):
            cli.parser().parse_args(["run", "soak", "--repetitions", "0"])

    def test_quality_keeps_below_busy_threshold_distinct_from_quiet(self):
        self.assertEqual(cli.comparison_quality(self.frames()), ("below_busy_threshold", True, None))
        busy = self.frames(machine={"busy_fraction": 0.101})
        self.assertEqual(cli.comparison_quality(busy), ("busy", False, "other programs exceeded the busy threshold"))
        self.assertEqual(cli.comparison_quality(self.frames(machine=None))[0:2], ("unknown", False))
        self.assertEqual(cli.comparison_quality(self.frames(status="failed"))[0:2], ("unknown", False))

    def test_quality_rejects_incomplete_cpu_samples_and_empty_frames(self):
        for machine in ({}, {"busy_fraction": "0.0"}, {"busy_fraction": float("nan")}, {"busy_fraction": float("inf")}):
            self.assertEqual(cli.comparison_quality(self.frames(machine=machine))[0:2], ("unknown", False))
        self.assertEqual(cli.comparison_quality(self.frames(frames={"count": 0})),
                         ("below_busy_threshold", False, "no positive frame count"))

    def test_aggregate_excludes_busy_unknown_and_failed_rows(self):
        out = Path("run-001")
        good = cli.comparison_row(1, out, self.frames())
        busy = cli.comparison_row(2, Path("run-002"), self.frames(machine={"busy_fraction": 0.5}))
        unknown = cli.comparison_row(3, Path("run-003"), self.frames(machine=None))
        failed = cli.comparison_row(4, Path("run-004"), self.frames(status="failed"))
        index = cli.comparison_index("tour", None, [good, busy, unknown, failed])
        self.assertEqual((index["aggregate"]["status"], index["aggregate"]["included_repetitions"]), ("available", 1))
        self.assertEqual(index["aggregate"]["metrics"]["mean_ms"], {"mean": 10.0, "min": 10.0, "max": 10.0})
        empty = cli.comparison_index("tour", None, [busy, unknown, failed])
        self.assertEqual((empty["aggregate"]["status"], empty["aggregate"]["metrics"]), ("unavailable", None))
        self.assertIn("Unavailable", "\n".join(cli.comparison_report(empty)))

    def test_repetitions_use_isolated_children_and_leave_legacy_result_names_there(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td) / "comparison"
            root.mkdir()
            ctx = lib.Ctx(Path(td), Path("engine"), "Data", Path("fnvtool"), root, say=lambda *_: None)
            seen = []

            def fake_once(args, child):
                seen.append(child.out)
                return self.frames(), ["# synthetic"], 0

            with mock.patch.object(cli, "context", return_value=ctx), \
                 mock.patch.object(cli, "run_once", side_effect=fake_once), \
                 mock.patch.object(cli, "write_comparison", wraps=cli.write_comparison) as written:
                self.assertEqual(cli.main(["run", "tour", "--repetitions", "2"]), 0)

            self.assertEqual([p.name for p in seen], ["run-001", "run-002"])
            self.assertEqual([call.args[1]["status"] for call in written.call_args_list], ["incomplete", "incomplete", "complete"])
            self.assertTrue(all((p / "results.json").is_file() and (p / "report.md").is_file() for p in seen))
            self.assertTrue((root / "comparison.json").is_file())
            self.assertTrue((root / "comparison.md").is_file())
            self.assertFalse((root / "results.json").exists())

    def test_manifest_is_collected_before_the_scenario_runs(self):
        args = cli.parser().parse_args(["run", "battle"])
        ctx = lib.Ctx(Path("repo"), Path("engine"), "Data", Path("fnvtool"), Path("out"), say=lambda *_: None)
        order = []
        result = self.frames()
        with mock.patch.object(cli, "collect_manifest", side_effect=lambda *_, **__: order.append("manifest") or {"at": "start"}), \
             mock.patch.object(cli.battle, "run", side_effect=lambda *_, **__: order.append("scenario") or result), \
             mock.patch.object(cli.battle, "report", return_value=[]):
            got, _, _ = cli.run_once(args, ctx)
        self.assertEqual(order, ["manifest", "scenario"])
        self.assertEqual(got["manifest"], {"at": "start"})

    def test_cells_and_markers_keep_their_existing_checkpoint_results(self):
        ctx = lib.Ctx(Path("repo"), Path("engine"), "Data", Path(__file__), Path("out"), say=lambda *_: None)
        cells_result = {"status": "ok", "crashed": 0, "hung": 0}
        markers_result = {"status": "ok", "crashed": 0, "hung": 0}
        with mock.patch.object(cli, "collect_manifest", side_effect=AssertionError("not a comparison run")), \
             mock.patch.object(cli.cells, "enumerate_cells", return_value=[]), \
             mock.patch.object(cli.cells, "choose", return_value=[]), \
             mock.patch.object(cli.cells, "run", return_value=cells_result), \
             mock.patch.object(cli.cells, "report", return_value=[]):
            got, _, _ = cli.run_once(cli.parser().parse_args(["run", "cells"]), ctx)
        self.assertNotIn("manifest", got)
        with mock.patch.object(cli, "collect_manifest", side_effect=AssertionError("not a comparison run")), \
             mock.patch.object(cli.markers, "enumerate_markers", return_value=[]), \
             mock.patch.object(cli.markers, "run", return_value=markers_result), \
             mock.patch.object(cli.markers, "report", return_value=[]):
            got, _, _ = cli.run_once(cli.parser().parse_args(["run", "markers"]), ctx)
        self.assertNotIn("manifest", got)


class RunFoldersTest(unittest.TestCase):
    def test_named_runs_are_new_single_components(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            first = lib.new_folder(root, "trial")
            self.assertTrue(first.is_dir())
            with self.assertRaises(FileExistsError):
                lib.new_folder(root, "trial")
            for bad in ("", ".", "..", "a/b", "a\\b", "C:other"):
                with self.assertRaises(ValueError):
                    lib.new_folder(root, bad)


@unittest.skipUnless(os.name == "nt", "the fake game is a .cmd")
class LaunchTest(unittest.TestCase):
    """lib.run against a fake game: a script that prints lines, then crashes or hangs."""

    def fake(self, d: Path, body: str) -> Path:
        script = d / "fake_game.py"
        script.write_text(textwrap.dedent(body), encoding="utf-8")
        cmd = d / "fake_game.cmd"
        cmd.write_text(f'@"{sys.executable}" "{script}" %*\n', encoding="utf-8")
        return cmd

    def test_a_crash_report_in_the_launch_home_is_collected(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            exe = self.fake(d, f"""
                import os, pathlib, sys
                print("stress cell: 1/1 start interior 00010C0E", flush=True)
                logs = pathlib.Path(os.environ["NVFOREVER_HOME"]) / "logs"
                logs.mkdir(parents=True, exist_ok=True)
                (logs / "crash-20261018-100000-1.txt").write_text({REPORT!r})
                assert os.environ["NVF_PLAY_TEST_STRESS"] == "1"
                sys.exit(101)
                """)
            ctx = lib.Ctx(d, exe, "Data", Path("fnvtool"), d)
            r = lib.run(ctx, lib.Launch("one", [], {}, run_seconds=5, progress=re.compile("stress"), hang_seconds=30))
            self.assertEqual((r.status, r.exit), ("crashed", 101))
            self.assertEqual(r.crashes[0].message, "index out of bounds: the len is 3 but the index is 7")
            self.assertIn("stress cell: 1/1 start", r.lines[0])
            self.assertTrue((d / "one.cmd.txt").exists())

    def test_no_progress_is_a_hang(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            exe = self.fake(d, """
                import time
                print("stress cell: 1/1 start", flush=True)
                time.sleep(60)
                """)
            ctx = lib.Ctx(d, exe, "Data", Path("fnvtool"), d)
            r = lib.run(ctx, lib.Launch("hang", [], {}, run_seconds=120, progress=re.compile("stress"), hang_seconds=2, memory_every=0.5))
            self.assertEqual(r.status, "hung")
            self.assertTrue(r.stacks and r.stacks.exists())


class ProcSamplesTest(unittest.TestCase):
    """The Linux readers behind the memory and CPU columns (the Windows arms are unchanged)."""

    def test_kb_field_reads_the_second_column(self):
        self.assertEqual(lib._kb_field("VmRSS:\t   20480 kB\nVmHWM:\t 30720 kB\n", "VmRSS:"), 20480)
        self.assertIsNone(lib._kb_field("VmRSS:\n", "VmRSS:"))
        self.assertIsNone(lib._kb_field("VmHWM: 1 kB\n", "VmRSS:"))

    def test_process_memory_is_rss_and_private_pages(self):
        # A pid that is not there: the reader must say None, not raise.
        self.assertIsNone(lib._linux_process_memory(2 ** 22 + 7))

    def test_the_cpu_line_counts_every_core_less_idle_and_iowait(self):
        stat = "cpu  100 10 50 800 40 5 5 0 0 0\ncpu0 1 2 3 4 5 6 7 8 9 10\n"
        # user 100 + nice 10 + system 50 + idle 800 + iowait 40 + irq 5 + softirq 5 + steal 0
        busy = lib._proc_cpu_busy_secs(stat, 100)
        self.assertIsNotNone(busy)
        self.assertAlmostEqual(busy or 0.0, (100 + 10 + 50 + 5 + 5) / 100)
        self.assertIsNone(lib._proc_cpu_busy_secs("cpu0 1 2 3\n", 100), "no whole-machine line")

    def test_a_process_stat_survives_a_command_name_with_spaces(self):
        # Fields after the command name: state ppid pgrp session tty tpgid flags minflt
        # cminflt majflt cmajflt utime stime. utime 250, stime 50 at 100 Hz.
        stat = "1234 (nvforever (dev)) S 1 1234 1234 0 -1 4194560 1 2 3 4 250 50 0 0 20 0\n"
        busy = lib._proc_pid_busy_secs(stat, 100)
        self.assertIsNotNone(busy)
        self.assertAlmostEqual(busy or 0.0, 3.0)
        self.assertIsNone(lib._proc_pid_busy_secs("1234 (truncated", 100))

    @unittest.skipUnless(sys.platform.startswith("linux"), "rss and private pages are the same accounting on Linux only")
    def test_a_live_process_reports_bytes(self):
        # This interpreter's own pid: real /proc, so the reader is exercised end to end.
        sample = lib.process_memory(os.getpid())
        self.assertIsNotNone(sample)
        assert sample is not None
        rss, private = sample
        self.assertGreater(rss, 0)
        self.assertGreater(private, 0)
        self.assertGreaterEqual(rss, private, "private pages are part of the resident set")
        self.assertIsNotNone(lib.process_busy_secs(os.getpid()))
        self.assertIsNotNone(lib.system_busy_secs())


class LockedReportTest(unittest.TestCase):
    """#3277: a launch with the Windows session locked says so in its report."""

    def test_only_a_locked_check_warns(self):
        from stress import lib
        self.assertEqual(lib.locked_report(None), [])
        self.assertEqual(lib.locked_report((12, 0)), [])
        out = " ".join(lib.locked_report((12, 3)))
        self.assertIn("Locked in 3 of 12 checks", out)
        self.assertIn("LogonUI.exe", out)


if __name__ == "__main__":
    unittest.main()
