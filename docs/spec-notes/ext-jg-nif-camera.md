# JohnnyGuitar NVSE: the NIF, particle and camera functions (2026-11-09, #1007)

21 JohnnyGuitar functions, a slice the coordinator gave this builder beside the
JohnnyGuitar batches (GetNiLODLevel, SetNiLODLevel, SetCameraShake, the viewmodel clip
distance and StopVATSCam were built already in batch 7; GetPlayerCamFOV is batch 9's, #1930). Code:
`crates/fnv-game/src/nvse_jg_nif.rs`, the scene mirror's node info and particle edits in
`crates/fnv-game/src/nif_blocks.rs`, the camera's state in
`crates/fnv-game/src/functions/camera.rs`; the engine's side in
`engine/nvforever/src/nif_mirror.rs` (bounds, particle systems, alpha and stencil
properties), `engine/nvforever/src/particles.rs` (the live systems' values),
`engine/nvf-play/src/script_camera.rs` (the camera's overrides and place) and
`engine/nvf-play/src/combat.rs` (the hit points). Tests: `crates/fnv-game/tests/jg_nif.rs`
and the unit test of `nvse_jg_nif.rs`.

Source: the GECK wiki export [documented: the page named], in our own words. Five pages
are templates with nothing filled in (Get/SetAlphaPropertyValue,
Get/SetStencilPropertyValue, IsNiSequenceActive, UpdateScenegraph): their arguments are
the pages' and their meaning a [hypothesis]. Not probed yet: set `jgnif` of
`tools/capture/jip_probes.py` (lab `L5.ext.jgnif`), questions in §4.

## 1. Nodes and particles

| Function | NVForever |
|---|---|
| Get3DDistanceBetweenNiNodes `ref1 ref2 node1 node2` | the distance between the two nodes in the world, the references' places and the nodes above included (`nif_blocks::node_world`, as GetNifBlockTranslationAlt's world place) [documented]; 0 without either [hypothesis] |
| Get3DDistanceToNiNode `node x y z` | from the node to the point [documented] |
| Get3DDistanceFromHitToNiNode `node` | from where the last hit on the calling reference struck: the engine tells each shot's and blow's point (`Game::note_hit_point`) [documented: in OnHit or the hit's event]; 0 before any [hypothesis] |
| GetNiBound `[block] [pc]` | [x, y, z, radius] in the world, the root's without a block [documented]: the sphere round the boxes of the shapes under the node, the node's own place with radius 0 when it has none [hypothesis: the engine's boxes for the file's bounds]. A missing block name returns the exact 4-element zero bound `[0.0, 0.0, 0.0, 0.0]` (not the root's bound) [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `w4c_b3_11b_jgnif_nodes_bounds` (`tools/capture/nvse/catalogs/wave4c_extenders.json`, #3679: `dHead=636.6,dRef=665.1\|bndRoot=4(r=72.1),bndMiss=4(r=0.0)\|grp(0,-1,-1)\|sq=0`) and `w4d_35_jg_camera_overlay_nif_bound` (`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3679: `head(n=4,x=2284.2,r=12.04)\|miss(n=4,0.0,0.0,0.0,r=0.00)\|camRef(preIS=FF001314,preUI=FF001318)`), and `w6t2_23_jg_dds_texture_format_and_bounds` (`tools/capture/nvse/catalogs/wave6_t2_observed_matches.json`: `PROBE\|w6t2_23_jg_dds_texture_format_and_bounds\|dds(w=64,h=64,m=0,fmt=5,miss=0)\|bndMiss(sz=4,0.0,0.0,0.0,0.0)`), no DLL or source consulted; hypothesis: empty array before the engine told the tree] |
| GetNiPSysModifierValue, SetNiPSysModifierValue `block id [value] [pc]` | the particle system of that name, or emitting from a node of that name, of a placed model: its values by the page's IDs 0-19 [documented: the table]; 0 order (not kept), 1 active, 16 the particles' base scale [hypothesis]; the setter changes the engine's live system while it lives, 1 when it found one [documented: "success:bool"] |
| GetParticleEmitterSpawnRate, SetParticleEmitterSpawnRate `block [value] [pc]` | the system's birth rate per second; set, it replaces any keyed rate [hypothesis] |
| Get/SetAlphaPropertyValue `block item [value] [pc]` | the shape's NiAlphaProperty from the model file: item 0 the flags, 1 the threshold [hypothesis: the page is a template; the fields' order, nif.xml]; set for the session's reads and queued via `SceneEdit::AlphaProperty` so `nvforever::nif_mirror` updates the drawn shape's private `NifMaterial::base.alpha_mode` live (`nvf_assets::nif_spawn::alpha_mode`) [design] |
| Get/SetStencilPropertyValue `block item [value] [pc]` | NiStencilProperty: 0 the flags, 1 the reference, 2 the mask [hypothesis]; set for the session's reads and queued via `SceneEdit::StencilProperty` so `nvforever::nif_mirror` updates the drawn shape's private `NifMaterial` `double_sided`/`cull_mode` from flags bits 10–11 via `nvf_assets::nif_spawn::stencil_cull` / `stencil_draw_mode_cull` (shared with load-time `NiStencilProperty` material creation so spawn and live updates produce identical face culling for draw modes `0..=3`) [documented: nif.xml `StencilDrawMode`; design: hardware stencil ref/mask test state is not exposed by Bevy's `StandardMaterial`, so items 1–2 remain mirrored only] |
| SetSwitchNodeIndex `block index [pc]` | the node's child of that index shown alone, the others hidden through the NIF block flags [documented: "Sets the active index"]; an index past them hides all [hypothesis] |
| IsNiSequenceActive `sequence [block] [pc]` | 1 when the reference plays a sequence of that name: its body's clip or one a script played (by file name), or its movement group [hypothesis: the page is a template] |
| GetSequenceAnimGroup `slot` | the anim group code in that slot, 0 idle … 7 special idle [documented]: Idle (0) in slot 0 standing [observed: §4], the body's other movement groups in slot 1; slot 4 (weapon/hand action) reads `24` (`Equip`) when a weapon is drawn or equipping and `200` (`JamA`, offset by `DNAM/reload_animation` `0..=22` [hypothesis], with `201 = JamB` [documented: `encodings.json`]) during `AnimAction::Jam`, `25` (`Unequip`) / `223` (`Stagger`) [hypothesis], and `-1` standing holstered [observed: §4; observed 2026-11-24: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 + JohnnyGuitarNVSE 528 + ShowOffNVSE 184, 10 official base masters, `GSDocMitchellHouse` (`0x00103DF9`), isolated clean-room staging copy, committed probe `w6t2_14_jip_on_reload_and_seq_anim_group` (`tools/capture/nvse/catalogs/wave6c_jip_on_reload_and_seq_group.json`): `PROBE\|w6t2_14_jip_on_reload_and_seq_anim_group\|fj=1,relHit=1,s0=0,s4(pre=24,t0=200,t1=200,cb=200)`; no DLL or source consulted] |
| UpdateScenegraph `type [time] [controllers] [block] [pc]` | nothing to do [decision: the engine updates every drawn node each frame]; 1 on a reference [observed: §4 refresh] |
| Update3DAlt `flags` | 1 model, 2 skin, 4 head, 8 face: the actor drawn again through the appearance changes (`crate::appearance`); 16 scale, 32 lights, 64 position need no request, the engine takes them each frame [documented: the flag table; decision]; 1 [observed: §4 refresh] |

The first-person model (pc) is not read: the mirror has the drawn model only, as for the
JIP block functions.

## 2. The camera

| Function | NVForever |
|---|---|
| GetCameraTranslation `x y z worldOrLocal` | the VM writes the three: the camera's place in the world (`0`), or its local translation (`1`): `(0, 0, 0)` in first person [observed: 2026-11-24, Fallout: New Vegas 1.4.0.525 (`040020D0`) + xNVSE 6.4.9 + JIP LN NVSE 57.30 + JohnnyGuitarNVSE 528, 10 official base masters, isolated clean-room staging copy (read-only `PrintToConsole` probe; zero writes to game install or Saves), `GSDocMitchellHouse` (`0x00103DF9`), committed probe `w6_t1_10a_jg_cam_trans` (`tools/capture/nvse/catalogs/wave6_t1_jg_camera_w2s.json`): `PROBE\|w6_t1_10a_jg_cam_trans\|N_w=(0.0,-3.7,118.0)\|N_l=(0.0,0.0,0.0)\|E_w=(-3.7,0.0,118.0)\|E_l=(0.0,0.0,0.0)`; no DLL, PDB, or third-party source consulted], and relative to the player in third person [hypothesis: third-person local frame]; 0 0 0 without an engine |
| SetCameraTranslate `override x y z` | the third-person camera put at that place relative to the player (GetCameraTranslation 1's frame), or the override cleared [documented]; the engine applies it after placing the camera (`nvf_play::script_camera`), not to the fly camera |
| SetCameraRotate `override axis angle [ref]` | the third-person camera's angle about 1 X (down positive, as the game's angles), 2 Y, 3 Z in degrees, or cleared; with a reference it keeps looking at it [documented]; clearing an axis clears the tracking [hypothesis] |
| WorldToScreen `xOut yOut zOut x y z handleType [ref]` | the VM writes X and Y (`0..1`, left to right, top to bottom) and D3D9 NDC depth `(far / (far - near)) * (1 - near / z)` (`< 1` in front of the camera, `> 1` behind it, matching `near = 5.0`, `far = 5000.0` to 5 decimal places across `d = 50, 100, 200, 1000, -100, -200`); returns `1` when on screen, `0` otherwise; off screen, handle type `0` clamps to the square edge, `1` to the circle, `2` leaves the unclamped projection (behind-camera points negate depth magnitude so right/up remain right/up); with a reference the point is an offset from its place [documented: the table; observed: 2026-11-24, same FNV 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 staging run, `GSDocMitchellHouse` (`0x00103DF9`), committed probes `w6_t1_10b_jg_w2s_paren` and `w6_t1_10d_jg_w2s_depth` (`tools/capture/nvse/catalogs/wave6_t1_jg_camera_w2s.json`): `PROBE\|w6_t1_10b_jg_w2s_paren\|ah0=1(0.500,0.500,0.9760)\|r0=0(1.000,0.500,0.9760)\|r1=0(1.000,0.500,0.9760)\|r2=0(1.722,0.500,0.9760)\|b0=0(0.000,0.000,1.0260)\|b2=0(0.500,0.500,1.0260)\|ref=1(0.500,0.534,0.9764)` and `PROBE\|w6_t1_10d_jg_w2s_depth\|z=(0.90090,0.95095,0.97598,0.99600,1.05105,1.02603)\|od0=0(0.781,0.000)\|od1=0(0.745,0.064)\|od2=0(1.233,-0.803)\|ba0=0(1.000,0.056)\|ba1=0(0.874,0.168)\|ba2=0(0.744,0.283)`; no DLL, PDB, or third-party source consulted; hypothesis: exterior cells with larger far planes use the camera's far plane] |

The engine tells the camera each frame (`CameraRequests::view`): place, heading, pitch,
roll, vertical field of view and the width over the height (GetPlayerCamFOV, JohnnyGuitar batch 9, may read it).

**Camera overlay references (`JG_CameraOverlayRef_PreIS`, `JG_CameraOverlayRef_PreUI`).**
JohnnyGuitar NVSE 5.26 introduced a 3D camera overlay system rendering overlays at
various render stages [documented: JohnnyGuitar NVSE 5.26 release notes,
`https://github.com/carxt/JohnnyGuitarNVSE/releases/tag/5.26`, "Add camera overlay
system; allows for 3D overlays at various render stages"; 5.28 release notes,
`https://github.com/carxt/JohnnyGuitarNVSE/releases/tag/5.28`, "Fix saving issues
related to overlay references" / "Don't render camera overlays during loading screen"].
Those release notes do not print the literal EditorID strings; the two EditorIDs
`JG_CameraOverlayRef_PreIS` (before image-space, selected when `bImageSpaceEffects` is
enabled) and `JG_CameraOverlayRef_PreUI` (before UI), looked up via `EditorIDToFormID`
and passed to `IsFormValid`, `ModelHasBlock`, `AttachModel`, `SetNifTrans`,
`SetMatrPropVal`, and `RemoveNifBlock`, are observed in an installed mod script
[observed: Radiation Visuals - ESPless (`https://www.nexusmods.com/newvegas/mods/84917`,
`MCM/RadiationVisuals.json` `minModVersion: 1.00`, requiring `JohnnyGuitarNVSE` 528),
`NVSE/user_defined_functions/RadiationVisuals/AttachCamera.gek`; JohnnyGuitar NVSE's
DLL/source code was not consulted]. NVForever exposes both EditorIDs as engine-owned
runtime references at `0xFE7F_0001` and `0xFE7F_0002` inside a dedicated
`[CAMERA_OVERLAY_BASE, LAMBDA_BASE)` (`0xFE7F_0000..0xFE80_0000`) window carved out
between `TEMP_REF_BASE` (`0xFE40_0000..0xFE7F_0000`, enforced in `Game::temp_ref` and
`Game::temp_item`) and `LAMBDA_BASE` (`0xFE80_0000..0xFF00_0000`), disjoint from
`CLONE_BASE` (`0xFE00_0000..0xFE20_0000`), `COMPILED_BASE` (`0xFE20_0000..0xFE40_0000`),
`FormId::FIRST_DYNAMIC` (`0xFF00_0000..`), and plugin indices `0x00..=0xFD` [design]:
`EditorIDToFormID`, `GetEditorID`, and `Host::form_sig` all let an explicit `SetEditorID`
or a loaded plugin record at the same FormID win first, falling back to `REFR` for the
built-in overlay references so `IsFormValid` and `IsReference` report `1` and
NIF/particle script calls (including `ModelHasBlock`) accept them without raising
`NoReference`. In original JohnnyGuitarNVSE 528, `EditorIDToFormID` resolves both
`JG_CameraOverlayRef_PreIS` and `JG_CameraOverlayRef_PreUI` as live runtime `0xFF`
references [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528,
2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters,
`w4d_35_jg_camera_overlay_nif_bound`
(`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3679):
`camRef(preIS=FF001314,preUI=FF001318)`; no DLL or source consulted]; NVForever's fixed
`0xFE7F_0001`/`0xFE7F_0002` window is `[design]` and renderer-side overlay pass behavior
remains `[hypothesis]`.

**Renderer-side 3D camera overlay consumer (`engine/nvforever/src/camera_overlay.rs`).**
`camera_overlay::sync` runs in `PostUpdate` ahead of `nif_attachments::produce` and
`nif_mirror::mirror` with an O(1) vanilla empty fast-path (`SceneMirror::is_watched`):
when neither overlay reference is watched, zero overlay roots, cameras, render targets,
or UI nodes are spawned [design]. Once watched, each stage spawns a `RefVisual` holder
rotated by `world::game_to_bevy()` (so NIF `+Y` projects ahead along Bevy `-Z`, `+X`
right, `+Z` up) with a `"Scene Root"` child so `AttachModel`, `InsertNode`,
`SetNifBlockTranslation`/`Rotation`/`Scale`, `SetNifBlockFlag`, `RemoveNifBlock`,
`SetMaterialPropertyValue`, `SetAlphaPropertyValue`, `SetStencilPropertyValue`, and
attached `NiParticleSystem` overrides work through the existing attachment and mirror
pipelines [design; hypothesis: overlay camera-local axes follow game-space NIF conventions]:
- `JG_CameraOverlayRef_PreIS` spawns under the main `WorldCamera` on dedicated
  `OVERLAY_PRE_IS_LAYER` (18), rendered by the main camera before `ImageSpaceGrade` and
  excluded from sun shadows, water reflection, scope lens, and extra PiP cameras [design].
- `JG_CameraOverlayRef_PreUI` spawns under a lazily created offscreen `Camera3d` (`order: -5`,
  transparent clear, `near: 0.1`, `far: 10_000.0`, FOV/aspect synced from the main camera)
  on dedicated `OVERLAY_PRE_UI_LAYER` (19), composited full-screen via `ImageNode` at
  `GlobalZIndex(-2)` (after `ImageSpaceGrade`, strictly beneath HUD/UI tiles and loading
  screens); the offscreen camera remains inactive until geometry or particles are attached
  and resizes its target texture in-place on window resize [design].
- Both stages hide their roots (and deactivate the `PreUI` offscreen camera and composite
  node) while `Loading::active()` is true [documented: JohnnyGuitar NVSE 5.28 release
  notes, "Don't render camera overlays during loading screen"], and tear down cleanly on
  session reset [design].
- Attached NIF models with `PreparedModel::particles` spawn a per-instance
  `ModelEmitter::attached` (using the overlay holder as `space_root` and its render layer
  for camera overlays), isolated from the host reference's placed particle systems via
  `particles::instance_systems` so `Get/SetNiPSysModifierValue` and
  `Get/SetParticleEmitterSpawnRate` read and mutate only that attached instance [design].
No JohnnyGuitar NVSE or third-party mod DLL or C++ source code was consulted.

## 3. Engine follow-ups

The first-person model's nodes (pc); the other sequence slots of GetSequenceAnimGroup;
hardware stencil reference/mask test state for `SetStencilPropertyValue` items 1–2 (Bevy
`StandardMaterial` exposes face culling only); original-game probe verification of exact
camera-overlay near-plane / depth-clear behavior under JohnnyGuitar NVSE 5.28.

## 4. Probe answers (set jgnif, run 2026-11-09)

On the player at Doc Mitchell's, in the original game with the extenders [observed], and
what NVForever does with each:

| Probe | Original | NVForever |
|---|---|---|
| `between`: head to pelvis above 0, a node to itself | `1 0` | the engine's node places; the records-only lab candidate has no 3D and gives `0 0` |
| `tonode`: the head 80-160 from the player's place, a missing block | `1 0` | as `between` |
| `fromhit`: no hit yet | `0` | `0` |
| `bound`: the root's size and radius above 0, a missing block's size | `4 1 4` | a missing block gives a 4-element zero-radius bound `[0, 0, 0, 0]` (`w4c_b3_11b_jgnif_nodes_bounds`: `bndRoot=4(r=72.1),bndMiss=4(r=0.0)`); in the records-only candidate empty |
| `missing`: a missing block's particle and property values | `0 0 0 0` | `0 0 0 0` |
| `camera`: the camera within 200 of the player, the local place's height above 50 | `1 0` | the engine's camera; the candidate has none |
| `screen`: WorldToScreen into variables | did not compile | not asked any more |
| `sequences`: slot 0, slot 4, `mtidle` standing | `0 -1 0` | Idle (0) in slot 0 of an actor standing or told nothing of, as here |
| `refresh`: UpdateScenegraph 0, Update3DAlt 32 | `1 1` | both 1, as here |

The local camera height not above 50 leaves the local frame open: the place may be the
camera's against the player's eyes, not the feet [hypothesis].
