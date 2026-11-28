# Spec note: JIP event handlers (`SetOn…EventHandler`, `SetGameMainLoopCallback`)

JIP LN NVSE's own event registrations: one function per event, each binding a UDF
script to that event, with filters as arguments. Task J2 of
[jip-tasks.md](../handoff/jip-tasks.md); input for J6. They sit beside xNVSE's
`SetEventHandler` (ext-events.md, task E1). JIP's events reachable through
`SetEventHandler` by name are in ext-events.md, not here.

Wiki statements are `[documented: <page>]` (private export `C:\fnv-private\geckwiki\`,
in our own words). Results from the original game (JIP 57.30, xNVSE 6.4.9) are
`[observed: jip-events/<id>]`: handlers registered as xNVSE lambdas by runtime-compiled
queries (`tools/capture/jip_probes.py --set jip-events`, 2026-10-26, 17 rows, the 12
documented values all match; golden private). See §4.

## 1. The common shape

`SetOn<Event>EventHandler <handler UDF> <1 set | 0 remove> [filters…]` [documented: every
page below].
- The handler is a UDF script form: `Begin Function {args}`. Its arguments are the
  event's values, in the order given in §2. Where an actor or a projectile is
  involved, that reference is the calling reference (`this`) [documented].
- Removing takes the same filters as setting: `… 0 SomeActor` removes the `… 1
  SomeActor` registration [documented: the examples].
- **Actor filters** (`actorOrList`) take one reference or a form list of references.
  For AnimAction and PlayGroup, an **empty** form list means any actor [documented:
  SetOnAnimActionEventHandler, SetOnPlayGroupEventHandler]. For FireWeapon the filter
  is optional; left out, it means any actor [documented: SetOnFireWeaponEventHandler].
  `XMarker` (`0x0000003B`) is a wildcard only for `SetOnCriticalHitEventHandler` (where
  `0` is refused) and does **not** wildcard `SetOnHealthDamageEventHandler`,
  `SetOnUseAidItemEventHandler`, `SetOnAnimActionEventHandler`, or
  `SetOnCrippledLimbEventHandler` [observed: 2026-11-24, Fallout: New Vegas 1.4.0.525,
  xNVSE 6.4.9, JIP LN 57.30, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base
  masters, `tools/capture/nvse/catalogs/wave6d1_jip_xmarker_wildcard_scope.json` probes
  `w6d1_21_jip_health_damage_filters_dups_rem` (`dup=1,fl=0,xm=0,emp=0,amt=-5.0`),
  `w6d1_22_jip_use_aid_item_filters` (`b=1,fl=0,xm=0,emp=0|sP=1,a1S=1`),
  `w6d1_24_jip_anim_action_and_play_group` (`draw(1,0,0)|jam(1,1,0)|pg=1(v=24)`), and
  `w6d1_25_jip_crippled_limb_and_note_added` (`crip(n=1,xm=0,limb=2,sP=1)|note(n=1,s0=1,sP=0,ok=1)`);
  no binary, DLL, PDB, or third-party source consulted].
- SetOnQuestStageEventHandler returns success 0/1. The others return nothing
  [documented: their pages].
- **Lifetime.** SetOnRagdollEventHandler's registrations are flushed on every game
  load, so they must be made again after a load [documented: SetOnRagdollEventHandler].
  For the other events the pages say nothing [hypothesis: the same, as JIP keeps them in
  memory only; NVForever clears every JIP registration on load and on return to the main
  menu, matching the one documented case].

## 2. The events

| Function | Filters after set/remove | Handler gets | Fires when |
|---|---|---|---|
| SetOnHitEventHandler | target actor or form list (optional) | this = struck actor; no arguments | an actor is hit. Runs **before** the damage and before the Verbose message; once per shotgun pellet; up to three times for an explosion: GetHitProjectile gives 0 the first time, then the projectile or the explosion, then the explosion [documented] |
| SetOnHealthDamageEventHandler | actor or form list | this = actor; damage (**negative**), source actor or 0 | an actor loses Health, including through DamageAV. Poison: the source is the victim. Collision: 0 [documented] |
| SetOnCriticalHitEventHandler | target, source, weapon (each optional; `XMarker` = no filter, since 0 is refused) | this = struck actor; source, weapon | a critical hit [documented] |
| SetOnCrippledLimbEventHandler | actor or form list | this = actor; limb 0 head, 1 torso, 2 left arm, 3 right arm, 4 left leg, 5 right leg, 6 brain | a limb is crippled [documented] |
| SetOnFireWeaponEventHandler | actor or form list (optional) | this = actor; weapon base form | a ranged weapon fires a round. Not when it jams; **not for the player in god mode** (demigod works) [documented] |
| SetOnReloadWeaponEventHandler | — | this = actor; weapon base form | any actor reloads [documented] |
| SetOnProjectileImpactEventHandler | projectile form or form list | this = projectile reference; no arguments. Details through GetProjectileRefImpactRef, …Weapon, …Source and GetPos | a projectile of that type hits anything, terrain included [documented; JIP 54.91] |
| SetOnRagdollEventHandler | — | this = actor | any actor ragdolls: death, paralysis, knockdown, low fatigue, unconsciousness [documented; JIP 55.26] |
| SetOnAnimActionEventHandler | actor or list, anim action 0–13 (GetAnimAction) | this = actor; action | the actor starts that action [documented] |
| SetOnPlayGroupEventHandler | actor or list, anim group keyword | this = actor; group index | the actor starts playing that group [documented] |
| SetOnUseAidItemEventHandler | ingestible or form list | this = actor; item | an actor consumes it [documented] |
| SetOnDialogTopicEventHandler | topic or info (an info by hex form ID) | this = speaker; the topic or info | it is spoken. With JohnnyGuitar 5.24 the topic may be left out [documented] |
| SetOnQuestStageEventHandler | quest, stage (exact: `0` matches only stage `0`, not wildcard), skipScript (optional) | quest, stage (`GetSelfAlt == 0`; `GetStage` already reads the new stage inside the callback); returns `1` on new add, `0` on duplicate add, `1` on removal, `0` on removal miss [observed: 2026-11-24, Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, JIP LN 57.30, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `tools/capture/nvse/catalogs/wave6d1_jip_quest_stage_exact_and_dup.json` probe `w6d1_23_jip_quest_stage_and_skip_script`: `ret=1,0,1,0\|n1=1,n2=0\|s0=1,qOk=1,arg=10,in=10,post=10`; no DLL, PDB, or third-party source consulted] | `SetStage quest stage` runs from any script. With skipScript non-zero, the stage's **result script does not run** [documented] |
| SetOnNoteAddedEventHandler | — | the note | a note is added to the player [documented] |
| SetOnLocationDiscoverEventHandler | — | the map marker reference | the player discovers a location [documented] |
| SetOnFastTravelEventHandler | — | the marker reference travelled to (not always a MapMarker-based reference, form 0x10; the page shows following linked children to the real marker) | the player fast-travels [documented] |
| SetOnCraftingEventHandler | — | item base form, quantity | an item is made in the Recipe menu [documented] |
| SetOnPCTargetChangeEventHandler | — | the new target actor, or null | the actor under the reticle changes (range: fAutoAimMaxDistance) [documented] |
| SetOnKeyDownEventHandler / …KeyUp… | DirectX scancode | the key | a key is pressed / released. Fires even when the key is disabled with DisableKey [documented] |
| SetOnControlDownEventHandler / …ControlUp… | control code | the control | a control is pressed / released [documented] |
| SetOnTriggerDownEventHandler / …TriggerUp… | 0 left, 1 right | the trigger | a controller trigger is pressed / released [documented; Controller.xml] |
| SetOnMenuOpenEventHandler / …MenuClose… | menu ID (optional: any menu) | the menu ID | a menu opens / closes. It must be a **specific** menu ID: a menu *type* such as 1 errors or crashes [documented] |
| SetOnMouseoverChangeEventHandler | menu ID (optional) | the menu ID | the highlighted item in that menu changes [documented] |
| SetOnMenuClickEventHandler | tile path: `"Menu#<id>"`, a full path, a prefix, or `*` / `:<index>` segments | menu ID, tile id (-1 if none), tile name **lower-case**; repeated same-named sibling tiles get `:<index>` (`Ui::indexed_name_of` / `Ui::indexed_path_of`) | a matching tile is clicked [documented; hypothesis: 0-based index among same-named siblings; `*` wildcard and slash/case-insensitive filter deduplication/removal] |

Observed with SetOnHealthDamageEventHandler on the player and `DamageAV Health 1`:
- The handler gets -1 as the amount, and the actor is the calling reference (GetSelf is
  the player). The source passed for DamageAV is null: storing it with AuxVarSetRef
  leaves no element [observed: ev.amount, ev.this, ev.source.type]. All as documented.
- **Several handlers for one event run newest first**: the second registration ran
  before the first (two runs) [observed: ev.order.size = 2, ev.order.first = 2]. The
  pages do not say. NVForever follows the game for JIP's events. This is the reverse
  of SetEventHandler's tie order (ext-events.md §7).
- **A handler that declares fewer parameters than the event passes never runs**:
  handlers with no parameters and with one (the event passes two) were registered
  (the call succeeded) but did not run [observed: ev.noparams.ran, ev.oneparam.ran = 0].
  NVForever registers them and skips them, with an extender log line saying why
  [decision: the game gives no message].
- Not yet observed: whether one handler can be registered twice (a lambda is a new
  script each time, so it needs a GECK-compiled UDF). A second identical registration
  is kept once [provisional decision; ext-events.md leaves the same open for
  SetEventHandler, while SetEventHandlerAlt runs duplicates twice].

## 3. SetGameMainLoopCallback (alias SGMLC)

`[ref.]SetGameMainLoopCallback <UDF> <1|0> [delay frames, default 1] [mode flags,
default 3]` [documented: SetGameMainLoopCallback].
- The UDF takes no arguments. It runs every `delay` frames. Flags: 1 in game mode, 2 in
  menu mode, 8 removed on load or on return to the main menu.
- Called on a reference, the UDF runs with that reference as `this`; otherwise on the
  player.
- Example on the page: delay 30 is described as half a second, which assumes 60 frames
  per second. NVForever counts frames, as documented, not time [decision].
- Observed: a lambda registered with delay 1 and flag 1 ran, with GetSelf the player
  [observed: loop.ran, loop.this]. How many times per frame was not measured: a capture
  query does not run on every game frame.
- Zero/null callable registration (`XVal::Ref(0)` or any `XVal::Num(n)` that converts
  to form 0 via `n as u32` — `0.0`, negative numbers, fractions below `1.0`, and `NaN`):
  NVForever refuses to enqueue a form-zero main-loop callback and emits one `ScriptError`
  plus structured warning via `handler_failed_with_registrar` (carrying registrar provenance
  captured by `Exec::event_call` for `EventCall::MainLoop { add: true, .. }`) at
  registration time rather than dispatching form zero every frame (#2603) [design;
  native handling of null `SetGameMainLoopCallback` callable tokens is unobserved].
  Active main-loop registrations also retain registrar provenance (including across
  `take_session`/`put_session` handoff) for runtime dispatch errors; direct
  `Game::event_call` registrations without an executing script unit report
  `registrar_kind = "unavailable"`.

## 4. Evidence still to get

Runtime queries reach the events through **lambdas** (§2, done for health damage and
the main-loop callback). Rules for writing those probes [observed: jip_ev_trial2,
jip_ev_trial3]:
- type the parameters in the braces, `Begin Function {float fD, ref rS}`; parameters
  declared before `Begin` do not compile;
- use `GetSelf`, not `this` (`this` does not compile there);
- the steps share the session: register, cause the event, then read.

Still open:
- **GECK-compiled probes** (when the owner is back): a quest registers handlers on
  GetGameLoaded, causes the event (SetStage, DamageAV, a forced reload), and prints
  `EXTPROBE|…` lines, in the same pattern as `tools/capture/ext-probes/udf`. Cases to
  settle: the order of two handlers, double registration, and whether a registration
  survives a load (the §1 hypothesis).
- More events by lambda, each with a harmless trigger inside the session: quest stage
  (SetStage with skipScript), use aid item (a stimpak added and used), fire and reload
  (an NPC's AI makes these slow to trigger by script).

## 5. Where NVForever fires them

Registration is one rule for every `SetOn…EventHandler` (`fnv_script::vm::jip`); an event
runs only where the game fires it (`Game::jip_event`):

| Event | Fired | Since |
|---|---|---|
| hit, healthdamage, crippledlimb | the hit and its damage (`combat.rs`); DamageAV for health damage, and for a limb condition it brings to 0 [observed: jip_events2, `DamageAV LeftAttackCondition 200` on a placed NPC fired with limb 2] | J6, #2134 |
| ragdoll | an actor dying (`Game::die`) or going down knocked out, knocked down, out of fatigue or paralysed (`Game::knock`), unless its body lies already [documented: SetOnRagdollEventHandler; hypothesis: a body already lying is no new ragdoll] | #2134 |
| useaiditem | an actor consumes an ingestible (`aid.rs`) | J6 |
| noteadded, queststage, mouseoverchange, menuclick | the note, SetStage, the menus' tiles | J6, T8 |
| projectileimpact | a shot in flight hits anything, terrain included: `Game::projectile_impact`, on the shot's temporary reference; the filter is matched against its PROJ base | #1408 |
| menuopen, menuclose | `Game::open_menu`, `Game::close_menu`, the menu under the tutorial menu closing, and a Pip-Boy page left for another [hypothesis: each page is a menu of its own code]. The dialogue menu (1009) is not reported | #1408 |
| keydown, keyup | each frame, the keys and mouse buttons (256 + n) held by the player that changed since the last (`Game::jip_keys`), disabled keys included [documented]; named `OnKeyDown`/`OnKeyUp` (plain and `:code`) and gamepad `OnButtonDown`/`OnButtonUp` (plain and `:bitmask`) share the same frame entry point ([ext-events.md](ext-events.md)) | #1408 |
| controldown, controlup | the existing key-event frame entry point, comparing the selected-device 28-control snapshot; both JIP and colon-name handlers ([contract](control-input-events.md)) | authored engine controls, unrun |
| crafting | the recipe menu made an item: once per output, with all of it (`Game::item_crafted`) [hypothesis: the page does not say how a recipe made several times is reported] | #1408 |
| locationdiscover | the engine found a map marker, with the marker reference (`Game::challenge_marker_found`) | #1408 |
| dialogtopic | a dialogue line is spoken (`dialogue.rs` `speak` and `game.rs` `say_line`), with `this = speaker` and `arg0 = info` when matched by an INFO filter or `topic` when matched by a topic/wildcard filter [documented; hypothesis: unfiltered/wildcard passes topic] | authored controls |
| criticalhit | a critical hit lands on a living, non-immune actor (`combat.rs` `Game::hit`), after `last_hit` is recorded so `GetHitLocation` reads the hit; `this = target`, `arg0 = source`, `arg1 = weapon`, matching filters `[target, source, weapon]` with `XMarker` wildcard [documented: SetOnCriticalHitEventHandler] | #2272 |
| fireweapon | an actor fires a ranged weapon (`combat.rs` `Game::weapon_fired` for the player and `nvse_proj.rs` `Game::projectile_fired` for NPCs/scripted actor shots, deduplicated per `(frame, actor, weapon)` so player shots and multi-pellet bursts fire once per round); `this = actor`, `arg0 = weapon`; excluded for non-actors, melee animation types 0–2, and the player in god mode (`ToggleGodMode`, while demigod / `ToggleImmortalMode` still fires) [documented: SetOnFireWeaponEventHandler; observed: 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 + ShowOffNVSE 184, 10 official base masters, committed probe `w6t2p3_04_fire_god_melee` (`tools/capture/nvse/catalogs/wave6_t2_observed_matches.json`): `PROBE\|w6t2p3_04_fire_god_melee\|norm(so=1,jp=1)\|god(so=1,jp=0)\|melee(so=1,jp=1)` confirming normal firing and god-mode suppression; zero DLL, PDB, or third-party source consulted; hypothesis: per-frame deduplication across `weapon_fired` and pellets, and non-actor exclusion] | #2272 |
| reloadweapon | an actor enters a reload action (`anim_state.rs` `Game::set_anim_state` on the rising edge into `Reload` or `ReloadOpen`) or weapon jam (`AnimAction::Jam`, e.g. `ForceWeaponJamAnim`), via `Game::weapon_reloaded`, with `this = actor`, `arg0 = weapon` (equipped or held); `ReloadOpen` → `Reload` fires once per reload, `AnimAction::Jam` fires on jam entry (with `GetSequenceAnimGroup 4 == 200` `JamA` inside the callback while `IsAnimPlayingEx 4 == 0`), and `ForceScriptAnim` does not fire [documented: SetOnReloadWeaponEventHandler; observed 2026-11-24: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 + JohnnyGuitarNVSE 528 + ShowOffNVSE 184, 10 official base masters, `GSDocMitchellHouse` (`0x00103DF9`), isolated clean-room staging copy, committed probe `w6t2_14_jip_on_reload_and_seq_anim_group` (`tools/capture/nvse/catalogs/wave6c_jip_on_reload_and_seq_group.json`): `PROBE\|w6t2_14_jip_on_reload_and_seq_anim_group\|fj=1,relHit=1,s0=0,s4(pre=24,t0=200,t1=200,cb=200)`; no DLL or source consulted; hypothesis: `ReloadOpen` → `Reload` counts as one reload] | #2272 |
| pctargetchange | the actor under the player's reticle changes (`crosshair_events.rs` `Game::set_crosshair_ref`, `jip_events.rs` `Game::sync_pc_target` / `Game::set_pc_target`, using `crosshair_far` within `fAutoAimMaxDistance`, default 1800, else `crosshair_ref`), with no calling reference and `arg0` the new actor reference or `0` (null) when cleared or aimed at a non-actor [documented: SetOnPCTargetChangeEventHandler, Auto Aim Settings] | #2272 |
| triggerdown, triggerup | the existing key-event frame entry point (`jip_events.rs` `Game::jip_keys`, fed each frame by `nvf-play::script_input::sync_pad_state` -> `Game::publish_pad_triggers` in `PreUpdate` and `Update` before `PlaySet::Scripts` across all connected `ActionPads` with finite `0.0..=1.0` clamping and digital-only trigger fallback, plus the immediate API `Game::set_pad_triggers`), comparing left (`0`) and right (`1`) controller trigger state (`pad.triggers >= 30/255` or `pad.held_triggers`) on press/release edges, with exact filter matching (`0` is left trigger, not a wildcard) and no calling reference [documented: SetOnTriggerDownEventHandler, SetOnTriggerUpEventHandler, XInput `XINPUT_GAMEPAD_TRIGGER_THRESHOLD`; hypothesis: analog threshold 30/255; design: multi-pad aggregation and frame-synced dispatch inside `Game::tick` / `Game::page_tick` under `with_world` and pause/loading guards] | #2272 |
| fasttravel | the player fast-travels (`nvse_jip12.rs` `Game::fast_travel`, `Game::note_fast_travel_to`, and `Game::note_fast_travel` called by `nvf_play::pipboy::data::travel::depart`), with no calling reference and `arg0` the arrival marker reference (the map marker's linked reference when present, else the map marker itself); blocked/refused travel does not fire [documented: SetOnFastTravelEventHandler; hypothesis: `note_fast_travel` resolves the arrival marker from the player's landing position] | #2272 |
| animaction | an actor enters a new `AnimAction` in `Game::set_anim_state` (`anim_state.rs`, bridged from `nvf-play`'s `sync_player_anim_events` / `bridge_player_anim_state` in `PlaySet::Activation` and `nvforever`'s `AnimStatePlugin` after `PlaySet::Scripts`), with `this = actor` and `arg0 = action.code()` [documented: SetOnAnimActionEventHandler; design: edge-triggered on `AnimState::action` transition after storing the live state; in `nvforever`, if a player weapon action ends on the same frame a renderer-driven non-weapon action is still active, `sync_player_anim_events` clears `action` to `None` before `PlaySet::Scripts` and `AnimStatePlugin::tell` restores the non-weapon action after `PlaySet::Scripts`] | #2272 |
| playgroup | a script plays an animation group (`PlayGroup` / `PlayGroupAlt` via `Game::ask_anim`, skipping flag-0 repeats of the group already playing and internal sentinel codes) or an actor enters a new named locomotion group in `Game::set_anim_state`, with `this = actor` and `arg0 = group` [documented: SetOnPlayGroupEventHandler; design: bare known `anim_group` keywords compile to numeric codes, quoted/runtime strings normalize via `Game::anim_group_code` in `Game::jip_set`, and `Slot::ExactArg(0)` preserves literal 0 (`Idle`)] | #2272 |

All events listed above are now wired to fire.
