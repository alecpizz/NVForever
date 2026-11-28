"""Offline stdlib unittest suite for `tools/capture/nvse/` (no game install required)."""

from __future__ import annotations

from contextlib import redirect_stderr, redirect_stdout
import hashlib
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest

NVSE_DIR = Path(__file__).resolve().parent.parent
if str(NVSE_DIR) not in sys.path:
    sys.path.insert(0, str(NVSE_DIR))

import harness as nvse_harness  # noqa: E402


def _snapshot_tree(root: Path) -> dict[str, str]:
    snap: dict[str, str] = {}
    for p in sorted(root.rglob("*")):
        rel = p.relative_to(root).as_posix()
        snap[rel] = "DIR" if p.is_dir() else hashlib.sha256(p.read_bytes()).hexdigest()
    return snap


def _minimal_probe_catalog(
    *,
    probe_id: str = "demo_probe_one",
    spec_file: str = "docs/spec-notes/ext-showoff.md",
    script_lines: list[str] | None = None,
    match: dict[str, str] | None = None,
    fixtures: list[dict[str, str]] | None = None,
) -> dict:
    probe: dict = {
        "id": probe_id,
        "bucket": "A",
        "extender": "ShowOffNVSE 184",
        "required_plugins": ["jip_nvse.dll", "ShowOffNVSE.dll"],
        "optional_extender": False,
        "requires_world": False,
        "mutates_state": False,
        "requires_fixture": bool(fixtures),
        "requires_isolated_staging": bool(fixtures),
        "spec_file": spec_file,
        "spec_line": 10,
        "functions_tested": ["Matrix_IsMatrix"],
        "claim": "Sample clean-room claim.",
        "expected": "1,0,0",
        "script_lines": script_lines
        or [
            "array_var aList = ar_List 1 2 3",
            f'PrintToConsole "PROBE|{probe_id}|%g" (Matrix_IsMatrix aList)',
        ],
    }
    if match is not None:
        probe["match"] = match
    if fixtures is not None:
        probe["fixtures"] = fixtures
    return {
        "schema": nvse_harness.PROBE_CATALOG_SCHEMA,
        "catalog_id": "demo_catalog",
        "description": "Synthetic test catalog.",
        "game_version": "Fallout: New Vegas 1.4.0.525 (040020D0)",
        "xnvse_version": "6.4.9",
        "official_masters": list(nvse_harness.OFFICIAL_MASTERS),
        "extender_versions": {"JIP LN NVSE": "57.30", "ShowOffNVSE Plugin": "184"},
        "batches": [
            {
                "id": "demo_batch",
                "title": "Demo Batch",
                "extender": "ShowOffNVSE 184",
                "required_plugins": ["jip_nvse.dll", "ShowOffNVSE.dll"],
                "probes": [probe],
            }
        ],
    }


def _minimal_persistence_catalog() -> dict:
    return {
        "schema": nvse_harness.PERSISTENCE_CATALOG_SCHEMA,
        "catalog_id": "synthetic_persistence",
        "description": "Synthetic offline read-only persistence verifier catalog.",
        "game_version": "Fallout: New Vegas 1.4.0.525 (040020D0)",
        "xnvse_version": "6.4.9",
        "official_masters": list(nvse_harness.OFFICIAL_MASTERS),
        "extender_versions": {"JIP LN NVSE": "57.30", "SUP NVSE Plugin": "855"},
        "items": [
            {
                "id": "p2_auxvar_perm",
                "key": "auxvar",
                "extender": "JIP LN 57.30",
                "spec_file": "docs/spec-notes/jip-variable-stores.md",
                "spec_line": 62,
                "functions_tested": ["AuxVarSetFlt", "AuxVarGetFlt"],
                "expected_tier": "SAVED",
                "field": "perm",
                "baseline": "0.0",
            },
            {
                "id": "p2_auxvar_temp",
                "key": "auxvar",
                "extender": "JIP LN 57.30",
                "spec_file": "docs/spec-notes/jip-variable-stores.md",
                "spec_line": 62,
                "functions_tested": ["AuxVarSetFlt", "AuxVarGetFlt"],
                "expected_tier": "SESSION_ONLY",
                "field": "temp",
                "baseline": "0.0",
            },
            {
                "id": "p2_combat_disabled",
                "key": "combat_disabled",
                "extender": "xNVSE 6.4.9 core",
                "spec_file": "docs/spec-notes/ext-last-core.md",
                "spec_line": 175,
                "functions_tested": ["SetCombatDisabled", "GetCombatDisabled"],
                "expected_tier": "CLEARED_ON_LOAD",
                "baseline": "0",
            },
            {
                "id": "p2_persistent",
                "key": "persistent",
                "extender": "xNVSE 6.4.9 core",
                "spec_file": "docs/spec-notes/nvse-stubs-persistent-fall-spread-unique.md",
                "spec_line": 116,
                "functions_tested": ["SetPersistent", "IsPersistent"],
                "expected_tier": "SAVED",
                "field": "pers",
                "baseline": "0",
            },
            {
                "id": "p2_hide_barter",
                "key": "hide_barter",
                "extender": "JohnnyGuitarNVSE 528",
                "spec_file": "docs/spec-notes/ext-jg7.md",
                "spec_line": 32,
                "functions_tested": ["HideItemBarterEx", "IsItemBarterHiddenEx"],
                "expected_tier": "UNCHANGED",
                "baseline": "0",
            },
            {
                "id": "p2_sup_save_meta",
                "key": "sup_save_meta",
                "extender": "SUP NVSE 855",
                "spec_file": "docs/spec-notes/ext-sup4.md",
                "spec_line": 26,
                "functions_tested": ["GetSaveName"],
                "expected_tier": "OBSERVE_ONLY",
            },
        ],
        "mutate_script_lines": ['PrintToConsole "P2|BEFORE|auxvar|perm=123.5,temp=456.5"'],
        "readback_script_lines": ['PrintToConsole "P2|%z|auxvar|perm=123.5,temp=456.5" svTag'],
    }


class TestCatalogValidation(unittest.TestCase):
    def test_committed_catalogs_validate_cleanly(self) -> None:
        paths = nvse_harness.iter_catalog_paths(catalog_dir=nvse_harness.DEFAULT_CATALOG_DIR)
        names = [p.name for p in paths]
        self.assertGreaterEqual(len(paths), 10)
        for baseline in (
            "wave1_parity_probes.json",
            "wave4a_extenders.json",
            "wave4b_extenders.json",
            "wave4c_extenders.json",
            "wave4d_extenders.json",
            "wave4e_mods.json",
            "wave4g_f2_extenders.json",
            "wave4h_reconcile.json",
            "wave6_point_raycast.json",
            "wave6_t1_jg_colors.json",
        ):
            self.assertIn(baseline, names)
        self.assertFalse((nvse_harness.DEFAULT_CATALOG_DIR / "persistence_matrix.json").exists())
        errors, counts = nvse_harness.validate_catalogs(paths)
        self.assertEqual(errors, [], f"Catalog validation errors: {errors[:5]}")

        expected_batches = 0
        expected_probes = 0
        probes_by_id: dict[str, dict] = {}
        for p in paths:
            data = nvse_harness.load_catalog(p)
            self.assertEqual(data.get("schema"), nvse_harness.PROBE_CATALOG_SCHEMA, p.name)
            batches = data.get("batches", [])
            self.assertGreater(len(batches), 0, p.name)
            expected_batches += len(batches)
            for b in batches:
                b_probes = b.get("probes", [])
                self.assertGreater(len(b_probes), 0, f"{p.name}:{b.get('id')}")
                expected_probes += len(b_probes)
                for pr in b_probes:
                    probes_by_id[pr["id"]] = pr

        self.assertEqual(counts["catalogs"], len(paths))
        self.assertEqual(counts["probe_batches"], expected_batches)
        self.assertEqual(counts["probes"], expected_probes)
        self.assertGreaterEqual(counts["probe_batches"], 28)
        self.assertGreaterEqual(counts["probes"], 543)
        self.assertEqual(counts["persistence_items"], 0)
        self.assertEqual(len(probes_by_id), expected_probes)

        # Verify Wave 1 parity probe IDs and corrected Wave 4A/4D/4E/4G/4H targeted probes are present verbatim.

        for req_id in (
            "w1_3495_compound_assign_and_datan2",
            "w1_3606_hit_3d_dir_and_base_damage",
            "w1_3632_onfireweapon_and_forcejam",
            "w1_3656_objectives_unholster_imod",
            "w1_2704_setcritdam_alias",
            "sup_19_getfiletime_keys",
            "core_13_generic_add_form_split",
            "core_24_eval_string_var",
            "anh_11_zone_owner_readback",
            "w4d_01_tagskills_chargen",
            "w4d_02_has_action_ref_triggers",
            "w4d_03_eval_empty_str_lit",
            "w4d_04_generic_form_neg_idx",
            "w4d_09_spread_all_anim_types",
            "w4d_43_nif_selector_and_ui_entities",
            "w4e_01_ysi_string_alias",
            "w4e_c10_uio_230_conditions_and_3line",
            "w4e_c14_script_runner_gx_exit",
            "w4g_01_getisformtype_case",
            "w4g_13_isidleplaying_and_idle_replace",
            "w4h_01_quest_added_button2_parent",
            "w4h_11_sup_lockpick_menu_tiers",
            "w6_01_point_raycast_angles",
            "w6_02_point_raycast_miss_and_range",
            "w6_03_point_raycast_collision_flags",
        ):
            self.assertIn(req_id, probes_by_id)

        anh02_src = "\n".join(probes_by_id["anh_02_angle_quadrant_edges"]["script_lines"])
        for deg in ("0", "45", "90", "135", "180", "225", "270", "315", "-45"):
            self.assertIn(f"(GetAngleQuadrant {deg})", anh02_src)
        anh03_src = "\n".join(probes_by_id["anh_03_scancode_to_char_layout"]["script_lines"])
        self.assertIn("1=[%z],59=[%z],18=[%z],30=[%z]", anh03_src)
        anh05_src = "\n".join(probes_by_id["anh_05_q_axis_angle_and_mult_vec3"]["script_lines"])
        self.assertIn("QFromAxisAngleAlt aAxis 90", anh05_src)
        anh11_src = "\n".join(probes_by_id["anh_11_zone_owner_readback"]["script_lines"])
        self.assertIn("GetZoneOwner rZ", anh11_src)
        self.assertIn("SetZoneOwner rZ rNCR", anh11_src)
        sup19_src = "\n".join(probes_by_id["sup_19_getfiletime_keys"]["script_lines"])
        self.assertIn("foreach aIter <- aDef", sup19_src)
        self.assertIn('GetFileTime "Data\\FalloutNV.esm" 2', sup19_src)
        core13_src = "\n".join(probes_by_id["core_13_generic_add_form_split"]["script_lines"])
        self.assertIn("GenericReplaceForm 1 rDoc", core13_src)
        self.assertIn("GenericAddForm 13 rFlst", core13_src)
        core24_eval_src = "\n".join(probes_by_id["core_24_eval_string_var"]["script_lines"])
        self.assertIn("eval (!sEmp)", core24_eval_src)
        self.assertIn("eval (sEmp && 5)", core24_eval_src)
        self.assertIn("eval ar_Null", core24_eval_src)
        jip39 = probes_by_id["jip_39_is_in_chargen_without_flag"]
        jip39_all = "\n".join(
            jip39["script_lines"]
            + [ln for st in jip39.get("extra_steps", []) for ln in st["script_lines"]]
        )
        self.assertIn("ShowTraitMenu", jip39_all)
        self.assertIn("traitMM=%g,cgInTrait=%g", jip39_all)
        core24_src = "\n".join(probes_by_id["core_24_string_var_destruct_zero"]["script_lines"])
        self.assertIn("eval s", core24_src)
        self.assertIn("sv_Destruct s", core24_src)
        self.assertNotIn("core_21_cached_ini_quotes_stripping", probes_by_id)
        self.assertNotIn("so_24_json_write_read_missing_pointer", probes_by_id)

    def test_dynamic_catalog_discovery_validates_added_catalogs(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            cdir = Path(td)
            (cdir / "a_first.json").write_text(
                json.dumps(_minimal_probe_catalog(probe_id="dyn_probe_a")),
                encoding="utf-8",
            )
            (cdir / "b_second.json").write_text(
                json.dumps(_minimal_probe_catalog(probe_id="dyn_probe_b")),
                encoding="utf-8",
            )
            paths = nvse_harness.iter_catalog_paths(catalog_dir=cdir)
            self.assertEqual([p.name for p in paths], ["a_first.json", "b_second.json"])
            errors, counts = nvse_harness.validate_catalogs(paths)
            self.assertEqual(errors, [], errors)
            self.assertEqual(counts["catalogs"], 2)
            self.assertEqual(counts["probe_batches"], 2)
            self.assertEqual(counts["probes"], 2)

            # Adding a third catalog with a colliding probe ID is caught automatically.
            (cdir / "c_dup.json").write_text(
                json.dumps(_minimal_probe_catalog(probe_id="dyn_probe_a")),
                encoding="utf-8",
            )
            dup_paths = nvse_harness.iter_catalog_paths(catalog_dir=cdir)
            dup_errors, dup_counts = nvse_harness.validate_catalogs(dup_paths)
            self.assertEqual(dup_counts["catalogs"], 3)
            self.assertTrue(
                any("duplicates another catalog" in e for e in dup_errors),
                dup_errors,
            )

    def test_rejects_bare_editorids_and_allows_quoted_or_commented(self) -> None:
        bad_cat = _minimal_probe_catalog(
            script_lines=[
                "Player.AddItem Weap10mmPistol 1 1",
                "ref rD = DocMitchellRef",
                'PrintToConsole "PROBE|demo_probe_one|ok"',
            ]
        )
        errs = nvse_harness.validate_catalog_data(bad_cat, repo_root=None)
        self.assertTrue(any("bare EditorID" in e for e in errs), errs)

        ok_cat = _minimal_probe_catalog(
            script_lines=[
                "; Weap10mmPistol comment is ignored",
                'ref rG = GetFormFromMod "FalloutNV.esm" "434F"',
                'string_var sNote = "DocMitchellRef in string literal is ignored"',
                'PrintToConsole "PROBE|demo_probe_one|%i" rG',
            ]
        )
        self.assertEqual(nvse_harness.validate_catalog_data(ok_cat, repo_root=None), [])

    def test_rejects_all_game_and_saves_file_writing_commands(self) -> None:
        forbidden_samples = [
            ('WriteStringToFile', 'WriteStringToFile "nvseprobe_out.log" 1 "x%r"'),
            ('WriteArrayToFile', 'WriteArrayToFile "out.txt" aArr'),
            ('con_Save', 'con_Save "nvseprobe_p2"'),
            ('con_SaveINI', 'con_SaveINI'),
            ('con_SCOF', 'con_SCOF "out.log"'),
            ('SaveGame', 'SaveGame "slot1"'),
            ('Autosave', 'Autosave'),
            ('WriteToJSON', 'int w1 = WriteToJSON 123 "test.json" "/a/b/c" 0 1 1 0'),
            ('SetINIFloat_Cached', 'SetINIFloat_Cached "General:fX" 1.0 "cfg.ini"'),
            ('SetINIString_Cached', 'SetINIString_Cached "General:sX" "v" "cfg.ini"'),
            ('SetINISection', 'SetINISection "Sec" aSec "cfg.ini"'),
            ('SaveCachedIniFile', 'SaveCachedIniFile "cfg.ini"'),
            ('CloseFileSO', 'CloseFileSO "cfg.ini" 1'),
            ('SetINIFloat', 'SetINIFloat "General:fX" 1.0 "cfg.ini"'),
            ('SetINIString', 'SetINIString "General:sX" "v" "cfg.ini"'),
            ('RemoveINIKey', 'RemoveINIKey "Sec:k" "cfg.ini"'),
            ('RemoveINISection', 'RemoveINISection "Sec" "cfg.ini"'),
            ('ExcelWriteFloat', 'ExcelWriteFloat "xl" 0 0 0 1.0'),
            ('ExcelWriteString', 'ExcelWriteString "xl" 0 0 1 "abc"'),
            ('ExcelArrayToFile', 'ExcelArrayToFile "xl" 0 0 0 aIn 0'),
            ('ExcelEraseSheet', 'ExcelEraseSheet "xl" 0'),
            ('ExcelEraseCell', 'ExcelEraseCell "xl" 0 0 0'),
            ('ExcelSetSheetName', 'ExcelSetSheetName "xl" 0 "Sheet"'),
            ('TextToQRCode', 'TextToQRCode "qr" "hello" 0'),
            ('DumpLoadOrder', 'DumpLoadOrder'),
            ('ModLogPrint', 'ModLogPrint 1 "msg"'),
            ('FileFromString', 'FileFromString "out.txt" "abc"'),
        ]
        for cmd_name, line in forbidden_samples:
            bad_cat = _minimal_probe_catalog(
                script_lines=[
                    line,
                    'PrintToConsole "PROBE|demo_probe_one|1"',
                ]
            )
            errs = nvse_harness.validate_catalog_data(bad_cat, repo_root=None)
            self.assertTrue(
                any("forbidden game/Saves file-writing command" in e and cmd_name in e for e in errs),
                f"Expected validate to reject {cmd_name}, got: {errs}",
            )

        # Also rejected in extra_steps, helper_scripts, and persistence mutate_script_lines.
        bad_extra = _minimal_probe_catalog()
        bad_extra["batches"][0]["probes"][0]["extra_steps"] = [
            {"suffix": "step2", "script_lines": ['con_Save "bad_save"']}
        ]
        self.assertTrue(
            any("con_Save" in e for e in nvse_harness.validate_catalog_data(bad_extra, repo_root=None))
        )

        bad_hlp = _minimal_probe_catalog()
        bad_hlp["batches"][0]["probes"][0]["helper_scripts"] = {
            "nvseprobe_s_hlp_bad.gek": ['WriteToJSON 1 "x.json" "/a" 0 1 1 0']
        }
        self.assertTrue(
            any("WriteToJSON" in e for e in nvse_harness.validate_catalog_data(bad_hlp, repo_root=None))
        )

        bad_pers = _minimal_persistence_catalog()
        bad_pers["mutate_script_lines"].append('con_Save "nvseprobe_p2"')
        self.assertTrue(
            any("con_Save" in e for e in nvse_harness.validate_catalog_data(bad_pers, repo_root=None))
        )

        # Comments and quoted string literals mentioning writer names must NOT false-positive.
        ok_cat = _minimal_probe_catalog(
            script_lines=[
                "; con_Save WriteToJSON ExcelWriteFloat DumpLoadOrder comment ignored",
                'string_var sMsg = "SetINIFloat_Cached TextToQRCode in string ignored"',
                'PrintToConsole "PROBE|demo_probe_one|%z" sMsg',
            ]
        )
        self.assertEqual(nvse_harness.validate_catalog_data(ok_cat, repo_root=None), [])

    def test_rejects_duplicate_ids_and_bad_spec_paths_and_privacy_leaks(self) -> None:
        cat1 = _minimal_probe_catalog(probe_id="dup_probe")
        cat2 = _minimal_probe_catalog(probe_id="dup_probe")
        seen: set[str] = set()
        self.assertEqual(nvse_harness.validate_catalog_data(cat1, seen_ids=seen), [])
        errs_dup = nvse_harness.validate_catalog_data(cat2, seen_ids=seen)
        self.assertTrue(any("duplicates another catalog" in e for e in errs_dup), errs_dup)

        bad_spec = _minimal_probe_catalog(spec_file="../outside.md")
        errs_spec = nvse_harness.validate_catalog_data(bad_spec, repo_root=None)
        self.assertTrue(any("clean relative POSIX path" in e for e in errs_spec), errs_spec)

        leaky = _minimal_probe_catalog()
        leaky["description"] = r"Captured from C:\Users\alice\FalloutNV"
        errs_leak = nvse_harness.validate_catalog_data(leaky, repo_root=None)
        self.assertTrue(any("privacy leak" in e for e in errs_leak), errs_leak)

    def test_cli_list_and_validate(self) -> None:
        buf = io.StringIO()
        with redirect_stdout(buf):
            rc_list = nvse_harness.main(["list", "--json"])
            rc_val = nvse_harness.main(["validate"])
        self.assertEqual(rc_list, 0)
        self.assertEqual(rc_val, 0)
        out = buf.getvalue()
        self.assertIn("wave1_parity_probes", out)
        self.assertIn("wave4a_extenders", out)
        self.assertIn("wave4b_extenders", out)
        self.assertIn("wave4c_extenders", out)
        self.assertIn("wave4d_extenders", out)
        self.assertIn("wave4e_mods", out)
        self.assertIn("wave4g_f2_extenders", out)
        self.assertIn("wave4h_reconcile", out)
        self.assertNotIn("persistence_matrix", out)


class TestEmitAndReadOnlyBoundary(unittest.TestCase):
    def test_no_auto_launcher_or_ini_patcher_present(self) -> None:
        self.assertFalse(hasattr(nvse_harness, "subprocess"))
        self.assertFalse(hasattr(nvse_harness, "patch_ini"))
        self.assertFalse(hasattr(nvse_harness, "run_probe_session"))

    def test_cli_emit_has_no_file_output_mode(self) -> None:
        ap = nvse_harness.build_arg_parser()
        with redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit) as ctx:
                ap.parse_args(
                    [
                        "emit",
                        "--catalog",
                        "demo.json",
                        "--out-dir",
                        "out",
                        "--output-mode",
                        "file",
                    ]
                )
        self.assertNotEqual(ctx.exception.code, 0)

    def test_emit_console_mode_and_game_dir_boundary_guard(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            tdp = Path(td)
            gdir = tdp / "game"
            sdir = tdp / "saves"
            (gdir / "Data").mkdir(parents=True)
            (gdir / "FalloutNV.exe").write_bytes(b"MZ-stub")
            (gdir / "Data" / "FalloutNV.esm").write_bytes(b"TES4-stub")
            sdir.mkdir()
            (sdir / "autosave.fos").write_bytes(b"FO3SAVEGAME-stub")

            game_before = _snapshot_tree(gdir)
            saves_before = _snapshot_tree(sdir)

            cat_path = tdp / "demo.json"
            cat_path.write_text(
                json.dumps(
                    _minimal_probe_catalog(
                        fixtures=[{"rel_path": "Data/Config/demo.ini", "content": "[Main]\na=1\n"}]
                    )
                ),
                encoding="utf-8",
            )

            # Reject --out-dir == --game-dir, inside --game-dir, ancestor of --game-dir, inside --saves-dir
            for bad_out in (
                gdir,
                gdir / "Data" / "NVSE",
                gdir / "sub" / ".." / "Data",
                tdp,
                sdir,
                sdir / "nested",
            ):
                with self.assertRaises(nvse_harness.ReadOnlyBoundaryError):
                    nvse_harness.emit_catalog(
                        cat_path,
                        bad_out,
                        game_dir=gdir,
                        saves_dir=sdir,
                    )

            # Even WITHOUT --game-dir, refuse a directory containing FalloutNV.exe / Data/FalloutNV.esm
            with self.assertRaises(nvse_harness.ReadOnlyBoundaryError):
                nvse_harness.emit_catalog(cat_path, gdir)

            # Valid emit into explicit clean out-dir (strictly console-only PrintToConsole)
            out_console = tdp / "staged_console"
            info = nvse_harness.emit_catalog(
                cat_path,
                out_console,
                game_dir=gdir,
                saves_dir=sdir,
            )
            self.assertEqual(info["emitted_probes"], 1)
            self.assertEqual(info["output_mode"], "console")
            probe_gek = out_console / "Data" / "NVSE" / "user_defined_functions" / "nvseprobe_s_demo_probe_one.gek"
            loop_gek = out_console / "Data" / "NVSE" / "user_defined_functions" / "nvseprobe_loop.gek"
            gr_txt = out_console / "Data" / "NVSE" / "Plugins" / "Scripts" / "gr_nvseprobe.txt"
            setup_txt = out_console / "MANUAL_SETUP.txt"
            fixture_file = out_console / "fixtures" / "Data" / "Config" / "demo.ini"
            for p in (probe_gek, loop_gek, gr_txt, setup_txt, fixture_file):
                self.assertTrue(p.is_file(), f"Missing emitted file: {p}")
            for p in (probe_gek, loop_gek, gr_txt):
                txt = p.read_text(encoding="ascii")
                self.assertEqual(nvse_harness.find_forbidden_writer_commands(txt.splitlines()), [])

            # Emit refuses any catalog containing a forbidden writer command directly.
            bad_writer_path = tdp / "bad_writer.json"
            bad_writer_path.write_text(
                json.dumps(
                    _minimal_probe_catalog(
                        script_lines=[
                            'WriteToJSON 1 "bad.json" "/k" 0 1 1 0',
                            'PrintToConsole "PROBE|demo_probe_one|1"',
                        ]
                    )
                ),
                encoding="utf-8",
            )
            with self.assertRaises(nvse_harness.ReadOnlyBoundaryError):
                nvse_harness.emit_catalog(bad_writer_path, tdp / "staged_bad_writer")
            self.assertFalse((tdp / "staged_bad_writer").exists())

            # Assert ZERO modifications anywhere in game_dir or saves_dir
            self.assertEqual(_snapshot_tree(gdir), game_before)
            self.assertEqual(_snapshot_tree(sdir), saves_before)

    def test_emit_all_committed_catalogs_are_strictly_read_only_console(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            tdp = Path(td)
            for cat_path in nvse_harness.iter_catalog_paths(catalog_dir=nvse_harness.DEFAULT_CATALOG_DIR):
                out_dir = tdp / cat_path.stem
                info = nvse_harness.emit_catalog(cat_path, out_dir)
                self.assertEqual(info["output_mode"], "console")
                self.assertGreater(info["emitted_probes"], 0)
                script_files = list((out_dir / "Data").rglob("*.gek")) + list((out_dir / "Data").rglob("*.txt"))
                self.assertTrue(script_files)
                for sf in script_files:
                    txt = sf.read_text(encoding="ascii")
                    writers = nvse_harness.find_forbidden_writer_commands(txt.splitlines())
                    self.assertEqual(writers, [], f"Forbidden writer in emitted {sf.name}: {writers}")
            # Confirm multi-step extra_steps + *np_keepmenu emitted cleanly for Wave 1 & jip_39
            w1_loop = (tdp / "wave1_parity_probes" / "Data" / "NVSE" / "user_defined_functions" / "nvseprobe_loop.gek").read_text(encoding="ascii")
            self.assertIn("nvseprobe_s_w1_3606_hit_3d_dir_and_base_damage_summary.gek", w1_loop)
            self.assertIn("nvseprobe_s_w1_2704_setcritdam_alias_shot_200.gek", w1_loop)
            w4a_loop = (tdp / "wave4a_extenders" / "Data" / "NVSE" / "user_defined_functions" / "nvseprobe_loop.gek").read_text(encoding="ascii")
            self.assertIn("*np_keepmenu", w4a_loop)
            self.assertIn("nvseprobe_s_jip_39_is_in_chargen_without_flag_read_trait.gek", w4a_loop)

    def test_emit_refuses_persistence_catalog_con_save(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            tdp = Path(td)
            pers_path = tdp / "pers.json"
            pers_path.write_text(json.dumps(_minimal_persistence_catalog()), encoding="utf-8")
            out_dir = tdp / "staged_p2"
            with self.assertRaises(nvse_harness.ReadOnlyBoundaryError):
                nvse_harness.emit_catalog(pers_path, out_dir)
            self.assertFalse(out_dir.exists())

    def test_ancestor_sentinel_blocks_nested_game_and_ini_dirs_without_flags(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            tdp = Path(td)
            gdir = tdp / "fnv_install"
            (gdir / "Data").mkdir(parents=True)
            (gdir / "FalloutNV.exe").write_bytes(b"MZ-stub")
            (gdir / "Data" / "FalloutNV.esm").write_bytes(b"TES4-stub")

            ini_dir = tdp / "my_games_fnv"
            (ini_dir / "Saves" / "profile1").mkdir(parents=True)
            (ini_dir / "Fallout.ini").write_bytes(b"[General]\r\n")

            cat_path = tdp / "demo.json"
            cat_path.write_text(json.dumps(_minimal_probe_catalog()), encoding="utf-8")

            game_before = _snapshot_tree(gdir)
            ini_before = _snapshot_tree(ini_dir)

            # Even when game_dir=None and saves_dir=None, nested paths under a live
            # game install or My Games\FalloutNV INI/Saves directory MUST be rejected.
            for nested_bad in (
                gdir / "Data" / "NVSE",
                gdir / "Data" / "NVSE" / "user_defined_functions",
                ini_dir / "Saves",
                ini_dir / "Saves" / "profile1" / "staged",
            ):
                with self.assertRaises(nvse_harness.ReadOnlyBoundaryError):
                    nvse_harness.emit_catalog(cat_path, nested_bad, game_dir=None, saves_dir=None)

            self.assertEqual(_snapshot_tree(gdir), game_before)
            self.assertEqual(_snapshot_tree(ini_dir), ini_before)

    def test_emit_rejects_path_traversal_and_unsafe_names(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            tdp = Path(td)
            out_dir = tdp / "staged"

            # 1. Fixture rel_path traversal
            bad_fx = _minimal_probe_catalog(
                fixtures=[{"rel_path": "../../escape.ini", "content": "x=1\n"}]
            )
            bad_fx_path = tdp / "bad_fx.json"
            bad_fx_path.write_text(json.dumps(bad_fx), encoding="utf-8")
            with self.assertRaises(ValueError):
                nvse_harness.emit_catalog(bad_fx_path, out_dir)
            self.assertFalse((tdp / "escape.ini").exists())

            # 2. Helper script filename traversal
            bad_hlp = _minimal_probe_catalog()
            bad_hlp["batches"][0]["probes"][0]["helper_scripts"] = {
                "../../escape.gek": ["int i = 1"]
            }
            bad_hlp_path = tdp / "bad_hlp.json"
            bad_hlp_path.write_text(json.dumps(bad_hlp), encoding="utf-8")
            with self.assertRaises(ValueError):
                nvse_harness.emit_catalog(bad_hlp_path, out_dir)
            self.assertFalse((tdp / "escape.gek").exists())

            # 3. Unsafe probe id
            bad_id = _minimal_probe_catalog(probe_id="../escape_probe")
            bad_id_path = tdp / "bad_id.json"
            bad_id_path.write_text(json.dumps(bad_id), encoding="utf-8")
            with self.assertRaises(ValueError):
                nvse_harness.emit_catalog(bad_id_path, out_dir)

            # 4. Unsafe extra_steps suffix traversal
            bad_sfx = _minimal_probe_catalog()
            bad_sfx["batches"][0]["probes"][0]["extra_steps"] = [
                {"suffix": "../escape", "script_lines": ['PrintToConsole "ok"']}
            ]
            bad_sfx_path = tdp / "bad_sfx.json"
            bad_sfx_path.write_text(json.dumps(bad_sfx), encoding="utf-8")
            with self.assertRaises(ValueError):
                nvse_harness.emit_catalog(bad_sfx_path, out_dir)

    def test_emit_helper_script_does_not_duplicate_begin_function_header(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            tdp = Path(td)
            cat = _minimal_probe_catalog()
            cat["batches"][0]["probes"][0]["helper_scripts"] = {
                "nvseprobe_s_hlp_noargs.gek": [
                    "begin Function {}",
                    'Player.AuxVarSetFlt "*x" 1',
                ],
                "nvseprobe_s_hlp_params.gek": [
                    "begin Function { ref rRef }",
                    'Player.AuxVarSetRef "*r" rRef',
                ],
                "nvseprobe_s_hlp_bare.gek": [
                    'Player.AuxVarSetFlt "*y" 2',
                ],
            }
            cat_path = tdp / "helpers.json"
            cat_path.write_text(json.dumps(cat), encoding="utf-8")
            out_dir = tdp / "staged"
            nvse_harness.emit_catalog(cat_path, out_dir)

            udf = out_dir / "Data" / "NVSE" / "user_defined_functions"
            noargs_txt = (udf / "nvseprobe_s_hlp_noargs.gek").read_text(encoding="ascii")
            params_txt = (udf / "nvseprobe_s_hlp_params.gek").read_text(encoding="ascii")
            bare_txt = (udf / "nvseprobe_s_hlp_bare.gek").read_text(encoding="ascii")

            self.assertEqual(noargs_txt.lower().count("begin function"), 1, noargs_txt)
            self.assertEqual(params_txt.lower().count("begin function"), 1, params_txt)
            self.assertIn("begin Function { ref rRef }", params_txt)
            self.assertEqual(bare_txt.lower().count("begin function"), 1, bare_txt)
            self.assertIn("begin Function {}", bare_txt)


class TestPersistenceClassifierAndVerify(unittest.TestCase):
    def test_classify_persistence_tier_all_states(self) -> None:
        self.assertEqual(nvse_harness.classify_persistence_tier("123.5", "123.5", "123.5", baseline="0.0"), "SAVED")
        self.assertEqual(nvse_harness.classify_persistence_tier("456.5", "456.5", "0.0", baseline="0.0"), "SESSION_ONLY")
        self.assertEqual(nvse_harness.classify_persistence_tier("1", "0", "0", baseline="0"), "CLEARED_ON_LOAD")
        self.assertEqual(nvse_harness.classify_persistence_tier("0", "0", "0", baseline="0"), "UNCHANGED")
        self.assertEqual(nvse_harness.classify_persistence_tier("1", "1", None, baseline="0"), "INCOMPLETE")
        self.assertEqual(nvse_harness.classify_persistence_tier("1", "0", "1", baseline="0"), "ANOMALOUS")
        # Multi-field extraction
        self.assertEqual(
            nvse_harness.classify_persistence_tier(
                "perm=123.5,temp=456.5",
                "perm=123.5,temp=456.5",
                "perm=123.5,temp=0.0",
                baseline="0.0",
                field="perm",
            ),
            "SAVED",
        )
        self.assertEqual(
            nvse_harness.classify_persistence_tier(
                "perm=123.5,temp=456.5",
                "perm=123.5,temp=456.5",
                "perm=123.5,temp=0.0",
                baseline="0.0",
                field="temp",
            ),
            "SESSION_ONLY",
        )

    def test_verify_probes_and_two_boot_persistence_read_only(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            tdp = Path(td)
            log_dir = tdp / "logs"
            out_dir = tdp / "reports"
            gdir = tdp / "game"
            log_dir.mkdir()
            gdir.mkdir()
            (gdir / "FalloutNV.exe").write_bytes(b"MZ-stub")

            cat_path = tdp / "probe_cat.json"
            cat_path.write_text(
                json.dumps(
                    _minimal_probe_catalog(
                        probe_id="so_01_mat_is_stringmap",
                        match={"type": "exact", "value": "1,0,0"},
                    )
                ),
                encoding="utf-8",
            )
            (log_dir / "run1.log").write_text(
                "\n".join(
                    [
                        "[12:00:00] PROBE|so_01_mat_is_stringmap|1,0,0",
                        "SR|boot1|gr_fired",
                        "SR|boot1|ln_fired",
                        "SR|boot1|gn_fired",
                        "P2|BEFORE|auxvar|perm=123.5,temp=456.5",
                        "P2|SAMERUN|auxvar|perm=123.5,temp=456.5",
                        "P2|COLD|auxvar|perm=123.5,temp=0.0",
                        "P2|BEFORE|combat_disabled|1",
                        "P2|SAMERUN|combat_disabled|0",
                        "P2|COLD|combat_disabled|0",
                        "P2|BEFORE|persistent|ref=FF001321,pers=1",
                        "P2|SAMERUN|persistent|ref=FF001321,pers=1",
                        "P2|COLD|persistent|ref=FF001321,pers=1",
                        "P2|BEFORE|hide_barter|0",
                        "P2|SAMERUN|hide_barter|0",
                        "P2|COLD|hide_barter|0",
                        "P2|BEFORE|sup_save_meta|sav=[],ld=[]",
                        "P2|SAMERUN|sup_save_meta|sav=[nvseprobe_p2.fos],ld=[nvseprobe_p2.fos]",
                        "P2|COLD|sup_save_meta|sav=[],ld=[nvseprobe_p2.fos]",
                    ]
                )
                + "\n",
                encoding="utf-8",
            )
            logs_before = _snapshot_tree(log_dir)

            # Without --redact-save-names, raw nvseprobe_p2.fos fails the privacy gate!
            pers_cat = tdp / "synthetic_pers.json"
            pers_cat.write_text(json.dumps(_minimal_persistence_catalog()), encoding="utf-8")
            with self.assertRaises(nvse_harness.PrivacyLeakError):
                nvse_harness.verify_logs([cat_path, pers_cat], log_dir=log_dir, redact_saves=False)

            # With --redact-save-names, privacy gate passes and tiers classify accurately!
            sb = nvse_harness.verify_logs([cat_path, pers_cat], log_dir=log_dir, redact_saves=True)
            self.assertEqual(sb["counts"]["match"], 1)
            by_id = {p["id"]: p for p in sb["persistence"]}
            self.assertEqual(by_id["p2_auxvar_perm"]["observed_tier"], "SAVED")
            self.assertEqual(by_id["p2_auxvar_temp"]["observed_tier"], "SESSION_ONLY")
            self.assertEqual(by_id["p2_combat_disabled"]["observed_tier"], "CLEARED_ON_LOAD")
            self.assertEqual(by_id["p2_persistent"]["observed_tier"], "SAVED")
            self.assertEqual(by_id["p2_hide_barter"]["observed_tier"], "UNCHANGED")
            self.assertIn("<save>.fos", by_id["p2_sup_save_meta"]["samerun"])
            self.assertEqual(sb["script_runner"]["boot1"], ["gr_fired", "ln_fired", "gn_fired"])

            # Verify CLI refuses writing --json-out inside --game-dir
            with self.assertRaises(nvse_harness.ReadOnlyBoundaryError):
                nvse_harness.main(
                    [
                        "verify",
                        "--catalog",
                        str(cat_path),
                        "--log-dir",
                        str(log_dir),
                        "--game-dir",
                        str(gdir),
                        "--json-out",
                        str(gdir / "bad.json"),
                    ]
                )

            # Verify CLI writes clean JSON + MD into out_dir while leaving log_dir untouched
            json_out = out_dir / "score.json"
            md_out = out_dir / "score.md"
            rc = nvse_harness.main(
                [
                    "verify",
                    "--catalog",
                    str(cat_path),
                    "--log-dir",
                    str(log_dir),
                    "--game-dir",
                    str(gdir),
                    "--json-out",
                    str(json_out),
                    "--md-out",
                    str(md_out),
                    "--strict",
                ]
            )
            self.assertEqual(rc, 0)
            self.assertTrue(json_out.is_file())
            self.assertTrue(md_out.is_file())
            self.assertEqual(_snapshot_tree(log_dir), logs_before)


class TestCitationAndPrivacyGate(unittest.TestCase):
    def test_privacy_gate_catches_all_leak_classes(self) -> None:
        leaky_samples = [
            r"Path C:\Users\alice\Documents\log.txt",
            "Path D:/ExampleInstall/Game/Data",
            "Path /home/bob/fnv/log.txt",
            "Path /Users/bob/fnv/log.txt",
            r"UNC \\fileserver\share\save",
            "Save personal_run_01.fos loaded",
            "Symbol FalloutNV.pdb leaked",
            "Control\x07bell",
        ]
        for sample in leaky_samples:
            issues = nvse_harness.check_privacy_leaks(sample)
            self.assertTrue(issues, f"Expected privacy leak to be flagged for: {sample!r}")

        clean_sample = (
            "[observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE 184 "
            "with the 10 official base masters, clean-room black-box script probe `so_01` "
            "(Matrix_IsMatrix): sav=[<save>.fos], 1,0,0; no binary, DLL, or source consulted]"
        )
        self.assertEqual(nvse_harness.check_privacy_leaks(clean_sample), [])

    def test_citation_formatting_and_cli_cite(self) -> None:
        meta = {
            "game_version": "Fallout: New Vegas 1.4.0.525 (040020D0)",
            "xnvse_version": "6.4.9",
        }
        entry = {
            "id": "so_01_mat_is_stringmap",
            "extender": "ShowOffNVSE 184",
            "spec_file": "docs/spec-notes/ext-showoff.md",
            "spec_line": 216,
            "functions_tested": ["Matrix_IsMatrix"],
            "status": "MATCH",
            "observed": "1,0,0",
        }
        cite = nvse_harness.format_citation(entry, meta, is_persistence=False)
        self.assertIn("Fallout: New Vegas 1.4.0.525", cite)
        self.assertIn("xNVSE 6.4.9", cite)
        self.assertIn("10 official base masters", cite)
        self.assertIn("no binary, DLL, or source consulted", cite)

        p_entry = {
            "id": "p2_combat_disabled",
            "extender": "xNVSE 6.4.9 core",
            "spec_file": "docs/spec-notes/ext-last-core.md",
            "spec_line": 175,
            "functions_tested": ["SetCombatDisabled", "GetCombatDisabled"],
            "before": "1",
            "samerun": "0",
            "cold": "0",
            "observed_tier": "CLEARED_ON_LOAD",
            "verdict": "MATCH",
        }
        p_cite = nvse_harness.format_citation(p_entry, meta, is_persistence=True)
        self.assertIn("CLEARED_ON_LOAD", p_cite)
        self.assertIn("BEFORE=1, SAMERUN=0, COLD=0", p_cite)

        # CLI cite rejects leaky observations
        bad_entry = dict(entry, observed=r"C:\Users\alice\leak.txt")
        with self.assertRaises(nvse_harness.PrivacyLeakError):
            nvse_harness.format_citation(bad_entry, meta, is_persistence=False)

        with tempfile.TemporaryDirectory() as td:
            sb_path = Path(td) / "sb.json"
            sb_path.write_text(
                json.dumps({"metadata": meta, "probes": [entry], "persistence": [p_entry]}),
                encoding="utf-8",
            )
            buf = io.StringIO()
            with redirect_stdout(buf):
                rc = nvse_harness.main(["cite", "--scoreboard", str(sb_path)])
            self.assertEqual(rc, 0)
            out = buf.getvalue()
            self.assertIn("so_01_mat_is_stringmap", out)
            self.assertIn("p2_combat_disabled", out)


if __name__ == "__main__":
    unittest.main()
