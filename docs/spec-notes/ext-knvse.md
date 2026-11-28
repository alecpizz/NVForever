# kNVSE's animation functions, second slice (2026-11-01, #1018)

The 16 kNVSE functions left after the first slice (docs/design/knvse-anim.md,
`crates/fnv-game/src/nvse_anim.rs`), built in `crates/fnv-game/src/nvse_knvse.rs`.
Sources: the GECK wiki export [documented: the page named], and the original game with
kNVSE installed [observed: probe set `knvse2` of `tools/capture/jip_probes.py`, lab
scenario `L5.ext.knvse2`; the first slice's `knvse-anim`]. No kNVSE source is read.

As in the first slice, the game keeps what scripts asked (`Game::knvse`) and answers
from it and from the clip the engine says each actor's body plays (`AnimState::clip`).
Paths compare without regard to case [decision: as the first slice, knvse-anim.md §2].

## 1. The clock and a sequence's time

| Function | NVForever |
|---|---|
| GetGlobalAnimTime `[first]` | what SetGlobalAnimTime gave plus the game seconds since; else the game seconds since the game first asked about the actor, that first frame counted [hypothesis: the clock starts with the actor's 3D; observed: knvse-anim and knvse2, `Ceil` of it reads 1 a few frames after placing an actor] |
| SetGlobalAnimTime `time [first]` | sets it, per view [documented]; the page's freeze example (`Set… (Get… − GetSecondsPassed)` each frame) holds the value |
| GetAnimCurrentTime `path` | the sequence's offset plus the actor's clock [documented: "the sum of the offset field … and … GetGlobalAnimTime"]; a sequence starting to play (PlayAnimationPath, ActivateAnim, the blends) gets the offset that makes its time 0 [observed: knvse2, under 0.1 the frame after PlayAnimationPath] |
| SetAnimCurrentTime `path time` | from the next frame, the offset that makes the current time `time` [documented; observed: knvse2, the same frame still reads the time from before, the next 1.5 and a frame] |

## 2. Weights, additive poses, text keys, attributes

[design: pipboy.md §2, first-person script animations] The player's first-person
clock additionally counts script frame time while a Pip-Boy menu holds game time.
The rendered sequence, clock getters/setters, deferred seeks and release/resume
share that per-view clock. Other actors and the player's third-person view keep
their existing game clock. This is NVForever's mapped menu animation behavior;
the default clock origins are admitted when animation time is first used even
if an explicitly set value for the other view supplies that call's result.
Admission inserts missing origins only; it preserves explicit per-view values,
their set times and the cumulative menu clock. A first-person activation must
therefore progress without requiring an earlier first-person getter.
This remains a mapped design;
the exact native pause policy is not established by the documented clock syntax
or the earlier non-menu captures. Authored regression selectors and remaining
validation are in [Pip-Boy animation validation](../handoff/pipboy-menu-animation-validation.md).

| Function | NVForever |
|---|---|
| SetAnimWeight `path weight` | kept by actor and path, read back as `seqWeight` [documented] |
| SetAnimAdditive `path [first] [refPose] [refTime]` | 0 for a missing `.kf` file [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + kNVSE 37, 2026-11-23/24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, read-only `PrintToConsole` probes `knv_03_add_anim_text_key_missing_ret` (`tools/capture/nvse/catalogs/wave4a_extenders.json`, #3679): `PROBE\|knv_03_add_anim_text_key_missing_ret\|0.00,0` and `w4c_b4_05_knvse_remaining` (`tools/capture/nvse/catalogs/wave4c_extenders.json`, #3679): `PROBE\|w4c_b4_05_knvse_remaining\|addMiss=0,blMiss=0\|seqSz(0=22,1=-1,7=-1)`; no DLL or source consulted]; 1 when the files exist, kept for the engine [documented: "Returns 1 if the additive animation is successfully applied"; hypothesis: success is the files existing] |
| AddAnimTextKey `path time key` | one key more for every actor; 0 for a missing `.kf` file [observed: `knv_03_add_anim_text_key_missing_ret` above (`0.00,0`)]; 1 when the file exists [documented: the page gives a Float return; hypothesis: return value on success]. The knvse2 probe cannot take its key off again (kNVSE 37 has no function for it); the change is in the probe game's memory only and the game quits after the set |
| SetAnimTextKeys `path times values` | the actor's own keys (no reference: every actor's); nothing while the actor plays the sequence [documented]; nothing when the lists differ in length [hypothesis] |
| GetAnimationAttributes `path` (GetAnimAttrs) | a string map of the 13 keys with no reference; on an actor, 22 keys (9 more, `lastTime` … `multiplier`) when it plays the sequence, else ar_Null [documented; observed: knvse2, 13 keys with no reference, 22 for a playing sequence, ar_Size −1 for one the actor never played] |
| GetPlayingAnimBySequenceType `type` (GetAnimByType) | the attributes of the sequence of that type the actor plays, else ar_Null [documented]. The body's clip is Idle (0) for the Idle group and Movement (1) for any other locomotion group [hypothesis] |

The attribute values the engine has not told read as defaults: the file's stem as
`sequenceName`, `frequency` 1, `beginKeyTime`/`endKeyTime` 0, `cycleType` 0 (loop),
`accumRootName` "Bip01", `animGroupId` −1, `keyType` 1 (0 for a clamped sequence)
[hypothesis]. The engine reads every sequence file it loads and can tell the real ones
through `Game::set_sequence_info` (follow-up: not wired yet). On a playing sequence
`state` is 1 (Animating), `offset` SetAnimCurrentTime's, the times the current time,
`multiplier` 1 [documented: the State list; hypothesis: the rest].

## 3. Blends and the rest

| Function | NVForever |
|---|---|
| CrossFadeAnims `source dest …` | 1 when the source plays and the destination does not (and exists): the source stops and the destination plays at `fWeight` (default 1); else 0 [documented: "a convenience function that calls DeactivateAnim on the source … and ActivateAnim on the destination"] |
| BlendToAnim `source dest [first] [duration] [destFrame] [sourceWeight] [destWeight] …` | 1 when the destination exists, also with a source that does not play [observed: knvse2; the page says the source "must be an actively playing animation" and that 0 means the blend could not start]; the destination plays at `destWeight` (default 1) from `destFrame`; a playing source keeps `sourceWeight`, stopping at 0 [hypothesis: the default source weight 0] |
| BlendToAnimFromPose `path [first] [duration] [destFrame] …` | the sequence plays from `destFrame`; returns 0 for a missing `.kf` file [observed: `w4c_b4_05_knvse_remaining` above (`blMiss=0`)], 1 when the file exists [hypothesis] |
| CopyAnimationsToForm `from to` | SetActorAnimationPath's and SetWeaponAnimationPath's overrides of `from` added to `to`, both views [documented; hypothesis: added to the target's own] |
| AllowAttack | the actor's anim action set to 0 [documented: "Sets the current Anim Action … to 0"]; the call waits for the engine (`Game::take_allow_attack`). Not in the installed kNVSE 37 (a runtime compile of it fails) [observed: knvse2's first run] |
| ForceFireWeapon | the equipped weapon fires as FireWeapon does (`CombatRequest::Fire`) [documented] |

## 4. Probe results and follow-ups

Probe set `knvse2` (31 rows, in the game with kNVSE 37 on a placed copy of Doc's NPC;
golden `jip/knvse2.json`, private), lab scenario `L5.ext.knvse2`: 29 of 31 rows agree
headless (`fnvtool ext queries`). The two that differ need the engine, which the headless
run does not have:

- `k2.attrs.cycle`: the smile idle's KF says clamp (2); headless the file is not read,
  the default 0 stands (the engine's `Game::set_sequence_info` would tell it).
- `k2.bytype.idle`: the actor's body plays its idle (22 keys); headless no body clip is
  told (`AnimState::clip`), so no sequence of type Idle plays (ar_Null). The first
  slice's body-clip rows (`kn.bygroup.idle` …) differ headless for the same reason.

A first run's `Floor (time × 10)` the frame after SetAnimCurrentTime 1.5 read 16 (the
game's frames were slow); the row keeps whole seconds.

Follow-ups (engine side, slice 2 of knvse-anim.md):
- **Wired [design]:** `AllowAttack` player attack-cycle cooldown, swing, and flinch
  release (`nvf-play` `combat.rs::fire` consumes `Game::take_allow_attack`); for player
  playback in `Animator`, script-sequence weights (`SetAnimWeight`), additive poses
  (`SetAnimAdditive`, both bind-pose delta and reference-clip delta), blend transition
  durations/views/priorities/`timeSync` clock phase alignment (`CrossFadeAnims`,
  `BlendToAnim`, `BlendToAnimFromPose`, aligning to the named playing sequence's
  normalized phase or clock time when active and still running the transition with
  `destFrame` / default clock when inactive), global/per-sequence clocks
  (`SetGlobalAnimTime`, `SetAnimCurrentTime`), and clip metadata publication
  (`Game::set_sequence_info` from `script_sequences.rs` and `anim_state.rs`); for drawn
  NPCs, layered `ActivateAnim` sequences on `BlendPlayer::script_layers` with sequence
  priority, weight, ease-in/ease-out, sequence clocks, additive poses (`SetAnimAdditive`),
  and `Game::set_sequence_info` clip metadata publication in `play_npc_sequences`.
- **Remaining:** runtime text-key triggers (`AddAnimTextKey` / `SetAnimTextKeys`).

## 5. Runtime `kNVSE.ini` (`[Anim Fixes] sLegacyAnimTimePaths`)

**Clean-room provenance:** File path, section name, key name, shipped default, and
inline comment come exclusively from **kNVSE Animation Plugin v37**
(`https://www.nexusmods.com/newvegas/mods/71336`), specifically the public `;` comments
and shipped defaults in `Data\NVSE\Plugins\kNVSE.ini`. No kNVSE DLL, PDB, disassembly,
or source code was inspected or consulted.

- **File read** [documented: kNVSE Animation Plugin v37 (`https://www.nexusmods.com/newvegas/mods/71336`), shipped `Data\NVSE\Plugins\kNVSE.ini`]:
  `Data\NVSE\Plugins\kNVSE.ini`, read through `fnv_game::mod_files::read_data_file`
  (case-insensitive loose read roots — NVForever's mods layer first, then game `Data` —
  followed by the VFS/archive reader) [design: NVForever's own VFS lookup order, not
  kNVSE's].
- **Parsed key** (`crates/fnv-game/src/knvse_ini.rs`, `Game::knvse_ini`):
  - `[Anim Fixes] sLegacyAnimTimePaths` [documented: kNVSE Animation Plugin v37
    (`https://www.nexusmods.com/newvegas/mods/71336`), `Data\NVSE\Plugins\kNVSE.ini`
    verbatim comment & default:
    `"; use legacy anim time algorithm for these paths (these mods rely on bugged behavior from previous versions of kNVSE)"` /
    `sLegacyAnimTimePaths = B42Inject,B42Interact,B42Loot`].
  - **Absent-INI default:** empty (`[]`) [design: NVForever's own fallback when
    `kNVSE.ini` is absent, leaving vanilla behavior unchanged].
- **Parser, matching, and startup reload** [design: NVForever's own behavior beyond
  what `kNVSE.ini` documents]:
  - Parsed directly on raw `&[u8]` with no `&str` byte-index slicing (UTF-8 BOM safe,
    `;` and `#` full-line/inline comment safe, ASCII case-insensitive section/key
    matching, tolerant of non-UTF-8 bytes in comments or other keys).
  - Entries are comma-split, trimmed, stripped of optional surrounding `"`, normalized to
    lower-case backslashes without a leading `meshes\`, deduplicated preserving order,
    and matched on whole path segments (`B42Inject` matches `B42Inject\stim.kf` and
    `Characters\_1stPerson\B42Inject\stim.kf` without matching `NotB42Inject\stim.kf` or
    `B42InjectExtra\stim.kf`).
  - `Game::new` loads `KnvseIni::load()` and records `mod_files::roots_generation()`;
    `nvf_play::script_input::mod_file_roots` calls `Game::reload_knvse_ini()` when VFS
    roots open, and `Game::tick()` calls `Game::sync_knvse_ini()` so root updates reload
    cleanly without overwriting manual overrides. Not serialized into save snapshots.
- **Runtime clock behavior (`w4e_08_knvse_legacy_anim_time`):**
  - `[observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + kNVSE 37 + JIP LN 57.30,
    2026-11-24, GSDocMitchellHouse (0x00103DF9), 10 official base masters, isolated
    clean-room staging copy with staged Data\NVSE\Plugins\kNVSE.ini
    (sLegacyAnimTimePaths = W4ELegacy,B42Inject) and read-only PrintToConsole probe
    w4e_08_knvse_legacy_anim_time (tools/capture/nvse/catalogs/wave4e_mods.json, #3803):
    `PROBE|w4e_08_knvse_legacy_anim_time|t0L=-3.40282e+38|t1(l=0.1080,n=0.1080)|dg=0.1650|leg(t=0.5205,off=-0.4560,last=0.2730)|nrm(t=0.5205,off=-0.4560,last=0.2730)`;
    zero DLL, PDB, or third-party source consulted]`
    1. Immediately after `PlayAnimationPath`, before the first animation tick,
       `GetAnimCurrentTime` reads `-FLT_MAX` (`t0L=-3.40282e+38`,
       `f64::from(-f32::MAX)`), not `0.0`.
    2. After `SetAnimSequenceFrequency 2.5` (`dg=0.1650`), **both** listed
       `W4ELegacy\test.kf` and unlisted `W4ENormal\test.kf` advance `GetAnimCurrentTime`
       identically by the frequency-scaled delta (`t1 + dg * freq = 0.1080 + 0.4125 =
       0.5205`), while `offset` remains fixed (`-0.4560`) and `GetAnimationAttributes["lastTime"]`
       tracks unscaled `global + offset` (`0.2730 = t1 + dg`).
  - `[hypothesis]` Immediately after `PlayAnimationPath`, before the first animation
    tick, `GetAnimationAttributes["offset"]` and `GetAnimationAttributes["lastTime"]`
    also return `-f32::MAX` (`-3.40282e+38`) (`w4e_08_knvse_legacy_anim_time` on `main`
    reads only `t0L` pre-first-tick and reads `offset`/`lastTime` after ticking).
  - `[hypothesis]` Any additional runtime conditions (beyond `SetAnimSequenceFrequency`
    tested in `w4e_08_knvse_legacy_anim_time`) under which kNVSE v37's
    `sLegacyAnimTimePaths` diverges from normal paths remain unestablished.
  - `[design]` NVForever parses, syncs, and exposes `Game::knvse_ini` /
    `KnvseIni::uses_legacy_anim_time(path)` cleanly, initializes `PlayAnimationPath`
    sequence clocks to `-f32::MAX` until the first tick, and integrates frequency-scaled
    deltas in `Game::script_sequence_time_for` while keeping `ScriptSequence::clock_offset`
    fixed and `lastTime` unscaled on both listed and unlisted paths (with no invented
    clamping or wrapping). Unmodded vanilla gameplay (without `PlayAnimationPath` /
    `SetAnimSequenceFrequency`) is unchanged.
- **Keys intentionally not wired (no dead struct fields):**
  - `[General] iConsoleLogLevel` (shipped `1`, `"; 0 = no log, 1 = kNVSE.log, 2 = console log"`),
    `iErrorLogLevel` (shipped `1`, `"; 0 = no log, 1 = kNVSE.log, 2 = error console log"`):
    native kNVSE DLL logging [documented: kNVSE v37 `kNVSE.ini`].
  - `[Blend Fixes]` (`bFixBlendSamePriority=1`, `bFixBlendAnimMultipliers=1`) and
    remaining `[Anim Fixes]` keys (`bFixEndKeyTimeShorterThanStopTime=1`,
    `bFixWrongAKeyInRespectEndKeyAnim=1`, `bFixWrongPrnKey=1`, `bFixWrongAnimName=1`,
    `bFixMissingPrnKey=1`, `bFixReloadStartAllowReloadTweak=1`): original-engine
    blending and malformed-KF export workarounds [documented: kNVSE v37 `kNVSE.ini`].
  - `[Engine Fixes] bFixLoopingReloads` (shipped `1`): original-exe looping-reload bug;
    already unconditionally satisfied by NVForever's round-by-round reload flow.
  - `[Engine Fixes] bFixIdleStrafing` (shipped `1`, `"; allow player to strafe/turn sideways mid idle animation"`):
    engine patch altering vanilla player idle locomotion and camera-hold behavior; out of
    scope here under `docs/design/extender-compat.md` §2 / §8.4 unless approved as a
    separate named NVForever option.
