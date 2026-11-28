# Spec notes: activators' animations and on/off looks

Clean-room description in plain language. Tags: **[documented]** cites a public source,
**[observed]** was measured on the owner's install (FNV 1.4.0.525), **[hypothesis]**
needs verification, **[design]** is our choice where the game's rule is not known.
Counts come from `fnvtool sim activator-models` (fnv.sim.activator_models/1) over
FalloutNV.esm alone, models from the default archive order; single models were read with
`fnvtool nif decode`. Sources: the owner's GECK wiki export (pages Activator, PlayGroup,
IsAnimPlaying, Radio Stations; 2026-10-09), niftools nif.xml (documentation only).

Code: `fnv_nif::anim` (`Track::controller_id`, `ValueTrack`), `nvf_assets::anim_play`
(`MaterialTrack`), `nvf_assets::material_anim`, nvforever `activators.rs`. Related:
radio.md (receivers on and off), doors.md (Open/Close on doors), nif.md §6 (emissive).

## 1. What the documents say

- "Non scripted activators can serve as drinkable water sources, radios that play radio
  stations or simple objects that play animations when activated" [documented:
  Activator]. Which animation group an activation plays is not said.
- "Random Anim Start: … this object kicks off its idle animation at a random start time"
  [documented: Activator]: activators play an idle animation by themselves.
- `PlayGroup <group> <flag>` plays an animation group on a reference; flags 0 normal,
  1 immediate start, 2 immediate loop [documented: PlayGroup]. `IsAnimPlaying [group]`
  is used on activators (`myActivator.isAnimPlaying forward`) [documented: IsAnimPlaying].
- Radios "will automatically play the radio station when turned on" [documented: Radio
  Stations]. Nothing documents how a radio or jukebox looks when on or off.

## 2. What the models hold [observed]

Activator sequences are NiControllerSequences embedded in the model. Besides transforms
they drive material members; a controlled block names the shape (`node_name`), the
property and controller types, and a controller ID saying which member:

- `NiMaterialColorController`, controller ID `SELF_ILLUM` (nif.xml TC_SELF_ILLUM
  [documented]), `NiPoint3Interpolator` with `NiPosData` keys: the emissive colour.
  Also `SPEC` on a few.
- `NiTextureTransformController`, controller ID `0-0-TT_TRANSLATE_U` / `_V` / `_SCALE_U` /
  `_SCALE_V` / `_ROTATE` (nif.xml TransformMember [documented]); every one in the
  activator models begins `0-0-`, and the controllers carry texture slot 0 (base) and
  shader map false: the ID reads `<shader map>-<slot>-<member>` [hypothesis].
- `BSMaterialEmittanceMultController` (`NiFloatInterpolator`): the emissive multiplier
  [hypothesis from the name].
- Also `NiAlphaController`, `NiGeomMorpherController`, `NiVisController`,
  `BSRefraction*Controller` and particle-system controllers.

**Radio receivers** (24 bases, ACTI with RNAM, radio.md §2) by their model's sequences:

| Sequences | Bases | Refs | Models |
|---|---|---|---|
| none | 12 | 107 | loudspeakers (`vloudspeakersr01.nif`), `vcontrolpanel02.nif`, `marker_audio.nif` |
| Open, Close | 9 | 107 | `vintageradio01.nif` (8 bases), `jukeboxdirty01.nif` |
| Idle | 3 | 6 | `jukeboxdirtynv.nif` (the saloon jukebox and one more), `jukeboxclean01.nif` |

- `vintageradio01.nif`: the shape `VintageRadioBody:0` stores emissive **black** with a
  glow map (`VintageRadio01_g.dds`). "Open" (0.67 s, clamp) keys SELF_ILLUM 0, 0, 1, 0,
  0, 1, 1 at 0, 0.13, 0.2, 0.27, 0.4, 0.5, 0.67 s: the dial flickers and lights. "Close"
  (0.37 s, clamp) keys 1, 0, 1, 1, 0: it flickers and goes dark. No sound keys.
  Decoded in full (issue #1080) [observed: `fnvtool nif decode`]: Open and Close have one
  track each, that SELF_ILLUM; no transform, texture-transform, morph or visibility track,
  and the needle `VintageRadioNeedle01` has no controller: the tuning is the flicker.
  The model has three shapes, `VintageRadioBody:0`, `VintageRadioNeedle01` and
  `VintageRadioGlassDial:0` (the round dial in the centre, alpha-blended, scale 0.6), and
  **all three list the same `NiMaterialProperty`** (emissive black × 1), the one the
  `NiMaterialColorController` targets, each with the same texture set (glow map
  `VintageRadio01_g.dds`). The glow map is black but for the dial face (the patch the
  glass dial's texture coordinates use); the body's panel does not glow. So the
  controller lights the dial, through the property it shares with the body [documented:
  nif.xml NiTimeController Target, NiAVObject Properties as references].
- `jukeboxdirty01.nif`: three shapes store black; "Open" and "Close" are the same
  2.67 s flicker that ends dark (0 at 2.67 s) — a broken jukebox that does not stay lit.
- `jukeboxdirtynv.nif` (`JukeBoxDirtyNV`, `JukeBoxDirtyNVCountry`; the Prospector
  Saloon's `00107D73`): one sequence, **Idle**, 8 s, loop (cycle type 0), six tracks:
  SELF_ILLUM on `Computer Screen` (stored 0.62; brief dips to 0.15–0.37), on
  `JukeBox01:0` (the body, stored 0.86 with glow map `JukeBoxClean_G.dds`; down to 0.35
  at 2 s and 6 s, flickers to 0.1 near 2.8 s and 4.07 s), on `JukeBox01:1` (the light
  tubes, unlit shader); TT_TRANSLATE_U on `Music` (notes, 2 → 0 in 8 s), `Bubbles`
  (0 → 2) and `JukeBox01:1` (0 → 1). The stored look is the lit one; there is no Off
  or Close sequence. `jukeboxclean01.nif` has the same Idle.
- No receiver model has a light node (NiLight); the "light" is emissive colour, glow map
  and unlit textures.

**Other activators** (1,119 ACTI bases that are not receivers; 274 models with
sequences among all ACTI and TACT, counted once each): sequence-name sets by bases
(placed refs): none 184 (2,370), no model 714 (1,234),
Backward+Forward 30 (1,627), Left+Right 53 (145), Close+Open 36 (107), Forward 15 (69),
SpecialIdle 12 (34), Idle 6 (12), Idle+Off 2 (18: the Gomorrah and Tops slot machines,
"Off" a 3.33 s loop), and 20 rarer sets. Cycle types: Open and Close always clamp (16
models each); Idle loop on 20 of 21; Forward/Backward/Left/Right both. Of the TACTs, 25
bases use `terminal01.nif`/`terminal02.nif` with Idle (0.07 s), Open and Close.
Time controllers outside any sequence (no ManagerControlled flag, nif.xml
TimeControllerFlags bit 5 [documented]): 44 in the activator models (29 transform, 7
texture transform, 6 material colour controllers).

The scripts: `VintageRadioStartOnSCRIPT` only activates its reference (radio.md §4).
PlayGroup is used 864 times in the masters' scripts (scripts.md §2); §5 surveys which
groups FalloutNV.esm's scripts play on which objects.

## 3. Behaviour implemented

- **Material tracks are played** [design, from §2]: a clip's SELF_ILLUM, emittance
  multiplier and base-texture TT_TRANSLATE_U/V tracks pose the shape's material: drawn
  emissive = colour × multiplier (× glow map), a track replacing the member it drives
  [hypothesis, as nif.md §6]; a texture offset moves the texture by its value (`uv +
  offset`) [hypothesis: the direction is not documented]. Each reference whose clip
  drives a material gets its own copy of it. A clip that drives only materials now
  counts as animating its model, so models with such an **Idle** (or SpecialIdle when
  there is no Idle) play it from load like transform idles (nif.md §6), whatever their
  record type: jukeboxes, `vmonitorbank.nif`, consoles, slot machines. A shape whose
  emissive is driven loads its glow map even when its stored colour is black.
- **Shared materials** (issue #1080): a sequence's track on a `NiMaterialProperty` member
  (SELF_ILLUM, emittance multiplier, alpha) names one shape, but every shape of the file
  that lists the same property block is driven with it (`model::material_sharers`, log
  `<model>: shapes sharing a driven material are driven with it: …`). Before, only
  `VintageRadioBody:0` was driven, whose texels have no glow, so the radios never
  changed: on, off and while switching, the dial crop of the general store's radio read
  the same mean (105.8, 68.4, 19.3) in all four frames of smoke `radio-dial-onoff`
  [observed in NVForever, 2026-10-31]. Now the dial face (crop 515-595 × 512-548) reads
  106.6, 72.4, 29.5 on; 88.7, 55.0, 15.1 off; the same dark value 0.3 s into Open
  (its flicker's 0 key at 0.27 s); and lit again at the end of Open. Off, the face keeps
  its painted cream under the room's light, without the glow. The same rule now also
  drives `terminal01.nif`'s `Text01:2`, `Text02:2` and `T01Screen01:1` with the shapes
  its sequences name.
- **Shared texturing properties** (issue #2450): the same holds for a base-texture
  offset, scale or rotation track (`NiTextureTransformController`): it names one shape,
  but every shape listing the same `NiTexturingProperty` block moves its texture with it
  (`model::texturing_sharers`, log `<model>: shapes sharing a driven texturing property
  move their texture with it: …`) [documented: nif.xml, as above; properties are shared by
  reference]. The Hidden Valley dust storm (`HVSandStormNight`, ten persistent references
  in WastelandNV's persistent cell, initially disabled, enabled by
  VHiddenValleyDustStormQuestScript; `effects\nv\nvsandstormnight.nif`) is 18 dust
  sheets `NVSandstorm01:0`..`NVSandstorm18:0` that all list one `NiTexturingProperty`
  and one `NiMaterialProperty`; its SpecialIdle (loop, 4 s) scrolls the base texture's U
  from 0 to 2 (quadratic keys) and holds alpha 1, naming `NVSandstorm01:0` only
  [observed: `fnvtool nif decode`]. Before, only that sheet scrolled and the other 17
  stood still, the storm a frozen picture (#2450; the alpha was already shared, the
  scroll was not). Then (#3919) all 18 stood still again in play: the quest enables the
  references with a fade, and the fade's see-through copy went on before the model bound
  its own material, so the model copied the fade's copy and the fade, at its end, gave
  back the material from before; the scroll went on in a material nothing drew. With the
  storm on, frames 0.5 s to 8 s apart at (-17863, -4814) at 22:00 did not differ above the
  horizon (mean 0.01 of 255) though disabling the ten references darkened it from 29 to
  8 [observed in NVForever, main d205d16fe, 2026-11-25]. Now the fade marks its copy
  (`nvf_assets::material_anim::HeldCopy`): a rig binding meanwhile copies the mesh's own
  and names its copy there, and the fade draws a see-through copy of the rig's own and
  gives it back; the same frames then differ (mean 0.7 to 1.4 of 255) [observed in
  NVForever, this change]. Smoke `hv-sandstorm-scroll` (its `fade: 0x000E18D7 in done; …
  meshes given their own material anew` line). The weather itself (HVDustStorm) has no
  model; its cloud layers are `sky\alpha.dds` but layer 3 (`NVCloudlight.dds`, ONAM
  speeds 179, 0, 0, 0) [observed: `fnvtool esm decode`], so its visible clouds stand
  still by the data.
- **Receivers' on and off looks** follow the runtime's switch (`fnv_game` `radio_on`,
  off when destroyed):
  - Open/Close models: switched on plays "Open" and holds its end, switched off "Close"
    [hypothesis: as doors; consistent with the radios' stored dark dial, an Open ending
    lit and a Close ending dark]; never switched: the stored look. Timing, holding and
    turning back halfway are the doors' (`DoorMotion`, doors.md §2).
  - Idle-only models (the jukeboxes): the idle plays while on; while off it stands
    still and the emissive tracks' shapes are drawn dark [hypothesis: by analogy with the
    radios; the model has no off look, and the owner expects the jukebox to light up
    when on]. The unlit shader ignores emissive (nif.md §6), so every unlit shape the
    idle drives (the jukebox's light tubes `JukeBox01:1`, opaque; the notes `Music` and
    `Bubbles`, additive: NiAlphaProperty 4097) is drawn black while off (alpha 0 if
    blended), and gets its stored colour back when on. [design, #2685:
    `material_anim::RigMaterials::apply`] Reactivation also restores the cached base
    alpha when no alpha track or fade writes it; otherwise the animated/faded alpha
    takes precedence, so the temporary dark alpha cannot keep a blended shape invisible.
    The original's tubes go dark when
    switched off [observed: owner 2026-10-30, #1079]; the notes and bubbles vanishing
    with them is [hypothesis]. Before, the tubes froze at full texture brightness.
    Tube crop from the `NVF_PLAY_TEST_NEAR` spot (smoke `jukebox-tubes-off`), mean
    (R, G, B): off 12.0, 15.9, 9.4 where it was 57.5, 107.3, 81.3; on 37–46, 65–85,
    49–64 with the flicker [observed in NVForever, 2026-10-31]. `JukeBoxDirtyNVCountry`
    and `jukeboxclean01.nif` go by the same rule.
  - Without `--play` (no runtime) nothing is driven: idles play, Open/Close models
    keep their stored look.
- **Other activators open and close like doors** (issues #2231, #2232): an activator
  that is not a radio receiver (no ACTI RNAM) changes its open state with SetOpenState
  and with each default activation (a script's `Activate`, or the player's when no
  OnActivate block decides), and the change asks for its **Open** or **Close** group
  as PlayGroup asks one (`fnv_game::Game::set_ref_open`): played once and held at its
  end, the collision following it, its sound keys heard, shown so when drawn again
  (§5.3). GetOpenState reads 1 or 3 by the state. A model without the sequence plays
  nothing [hypothesis: an activator with Open and Close works like a door; observed in
  the data: Fallout 3's Overseer terminal opens the tunnel with `SetOpenState 1` on the
  desk `VRmOverseerDesk01` (Open 10 s, sound key OBJOverseerDeskOpen; Close 6.6 s), and
  the vault door control pod `CG04Vault101ControlPodScript` activates its linked
  `VaultGearDoor` (Open 25 s), whose script runs the default activation for anyone but
  the player]. Log: `activator: <ref> opens|closes (its Open|Close sequence)`.
- Log: `activator: <ref> (<base>) on|off: plays Open (0.67 s) | plays its idle | holds
  its idle, lights dark, unlit shapes black | keeps its stored look`, and at exit `activator summary: N
  receivers drawn with an on/off look; switched on N times, off N times while drawn`.
- Smoke test (CLAUDE.md, the radio test): in the Prospector Saloon the jukebox is
  switched on by its OnLoad ("on: plays its idle"), off at 20 s ("holds its idle,
  lights dark"), on at 28 s; the broken radio `00109087` "keeps its stored look".
  Screenshot pair from the test spot (`NVF_PLAY_TEST_NEAR=00107D73`), crop of the
  jukebox body: mean (R, G, B) 29.4, 34.5, 20.3 on and 24.8, 31.4, 19.8 off; 42,363 of
  304,000 pixels differ by more than 10 [observed in NVForever, 2026-10-11].

## 4. Not built, and questions for captures

- Not built: activation of a non-receiver activator playing a group other than Open or
  Close (which group the "simple objects that play animations when activated" play is
  not documented; Open/Close: §3);
  `LoopGroup` (§5); the slot machines' "Off"; `NiAlphaController`, `NiVisController`,
  `SPEC` colour, texture scale and rotate, morph and particle controllers in sequences;
  controllers outside sequences; the Open/Close sound keys (none on the receivers).
- Capture questions for the owner: the saloon jukebox switched off in game — answered:
  its glow tube goes dark (owner 2026-10-30, #1079; §3). Do its notes and bubbles go
  too? A vintage radio switched on and off: switching plays the tuning (Open's flicker)
  and the centre lights only while on [observed: owner 2026-10-30, #1080]; that it
  flickers through Close as the keys say is [hypothesis]. Which way the jukebox's music
  notes scroll.

## 5. Scripts' animation groups: PlayGroup and IsAnimPlaying

Survey: `fnvtool sim playgroup-calls --schema schemas/fnv <Data> FalloutNV.esm`
(fnv.sim.playgroup_calls/1): every compiled script unit decoded with fnv-script, each
PlayGroup (0x1013) and IsAnimPlaying (0x1128) call in statements and expressions, the
object it runs on (the explicit reference's base, else every base that carries the
script), and whether that base's model has a sequence named like the group.

### 5.1 What the documents say

- `PlayGroup <AnimGroup> <InitFlag>` plays an animation group on the calling reference.
  InitFlag 0 "Normal": the current animation finishes its full cycle, then the new one
  starts from its beginning; 1 "Immediate Start": the current one stops at once and the
  new one starts from its beginning; 2 "Immediate Loop": the current one stops at once
  and the new one starts at the beginning of its loop cycle [documented: GECK wiki
  PlayGroup, owner export]. `PlayGroup Idle 1` returns an actor to normal [documented].
- `IsAnimPlaying [AnimGroup]` is 1 while the calling object plays an animation; with a
  group, only an animation of that group's "sequence type"; the page's example is an
  activator, `myActivator.isAnimPlaying forward` [documented: GECK wiki IsAnimPlaying].
- `LoopGroup <group> [flags]` loops a group a number of times on an actor, with the same
  flags [documented: GECK wiki LoopGroup]; no opcode is known for it and no shipped
  script calls it [observed: script-functions-catalogue.md has no opcode for it].
- The group is compiled as a bare code [observed: crates/fnv-script/data/encodings.json
  `anim_group`]; the names shipped source uses for the codes it uses: 0 Idle, 2
  SpecialIdle, 3 Forward, 4 Backward, 5 Left, 6 Right, 7 FastForward, 8 FastBackward,
  9 FastLeft, 10 FastRight, 15 TurnLeft, 17 Aim, 24 Equip, 25 Unequip [observed].

### 5.2 What FalloutNV.esm's scripts do [observed]

589 calls: PlayGroup 520, IsAnimPlaying 69 (17 without a group).

| Group | flag 0 | flag 1 | flag 2 | other |
|---|---|---|---|---|
| Forward | 63 | 168 | 1 | 1 (flag 5) |
| Backward | 32 | 44 | 10 | |
| Left | 21 | 51 | | 2 (flag 5) |
| Right | 23 | 17 | 20 | |
| SpecialIdle | 21 | 3 | 22 | |
| Idle | 2 | 1 | | |
| FastForward, FastBackward, FastLeft, FastRight | 8 | 4 | | |
| Aim, Equip, TurnLeft, Unequip | | 6 | | |

- On what (call × target base): ACTI 555, STAT 131, CONT 86 (82 are `IsAnimPlaying`
  without a group in the vendor containers' script), MSTT 27, TACT 7, FURN 1; 14 calls
  run on a reference variable, 75 sit in scripts no base carries, 1 in a package result.
  STATs and MSTTs are reached through explicit references (`0x000D7D2E.PlayGroup Forward
  1`: Helios One's 75 solar reflector rows; the CG04 klaxons; boulders).
- The model has a sequence of the group's name for 543 of the 555 ACTI targets and for
  every STAT, MSTT and named CONT target; 4 ACTI targets (a roulette table, a laser
  tripwire) and the FURN marker have no sequences. Names match only case-insensitively
  in places (`nvpinto_fruited.nif` has `backward`).
- No activator sequence has a loop-start text key: their keys are start, end, `Sound:`
  (251) and a few others; flag 2 has nothing else to start from.
- Typical uses: harvestable plants (`NevadaAgavePickable` 151 refs,
  `BarrelCactusPickable` 189, `MaizePickable` 220 …): OnLoad plays Backward (not picked)
  or Forward (picked); OnActivate gives the item, plays Forward with flag 1 and calls
  `SetDestroyed 1`. Neon and window lights: script `0x0011B039` (the Strip's signs, the
  Prospector Saloon's) every frame, at night, plays Left with flag 0 if `IsAnimPlaying
  Left` is not 1, by day the same with Right; script `0x001759FC` (house windows) Left
  after 19:30, Right after 6:30. Traps: the bear trap's Forward carries `Sound:
  TRPBearFire`; tripwires, pressure plates. One-off scenes: B-29s, vertibirds, the Hoover
  Dam guns, Repconn's dome.
- Script `0x0017B074` (The Tops' and Vault 21's sign lights) tests `IsAnimPlaying
  Forward == 1 && IsAnimPlaying Backward != 1` before playing Backward: its author
  expected IsAnimPlaying to tell Forward and Backward apart.

### 5.3 Behaviour implemented

- **Rules** (fnv-game `anims`): `PlayGroup` queues a request (reference, group code, its
  name, start flag) for the engine and traces `anim_asked`, once per reference until the
  group or the flag changes [design: scripts that repeat it every frame]. `IsAnimPlaying`
  answers from what the engine reports: with a group, whether that very group plays
  [hypothesis: the documented "sequence type" may be broader; script 0x0017B074 needs
  Forward and Backward apart]; without one, whether any group asked by PlayGroup plays,
  an object's own idle not counted [hypothesis]. With an engine attached, a group counts
  as playing from the call on, until the engine reports it ended or unplayable [design:
  scripts test it right after asking]. Headless (no engine) nothing plays [design, as
  IsIdlePlaying]. Flags other than 0–2 (the three flag-5 calls) are taken as 0 [design].
- **Engine** (nvforever `object_anims.rs`): every drawn object whose model has a named
  sequence that animates it gets its own node hierarchy and a `GroupRig` [design: a
  script may name any reference; the cost is a node hierarchy per such reference,
  counted as "with named sequences" in the cells-loaded line]. A request plays the
  sequence of the group's name [hypothesis: the names match; 543 of 555 activator
  targets have it]:
  - a clamped sequence (cycle type 2) plays once, holds its end and is then reported
    ended; a looping one loops until another group replaces it [hypothesis: as the NIF
    cycle types];
  - flags 1 and 2 start it at once from its beginning (2 as 1: no loop-start key, above);
    flag 0 waits for what plays: a group played once until it ends, a loop (the object's
    idle included) until its cycle ends [documented flag meanings; that an object's idle
    counts as "the current animation" is a hypothesis]; a loop asked for again while it
    plays goes on [design];
  - an object not drawn keeps the latest request and shows it when drawn: a group played
    once at its end, a loop from its start [design]; meanwhile it is reported not
    playing. An object a script enabled at run time (`nvf_play::script_draw` draws it a
    frame or more after the Enable, with its own node hierarchy when its model has named
    sequences) shows a group played once where it would be by now, from its start when
    it was asked for with the Enable, and the sound keys passed meanwhile play when it is
    drawn [design: VMS15's bomber, enabled by the detonator, plays its 9.9 s `Forward`
    rise; smoke `b29-rise`];
  - the group's material tracks (emissive, emittance, texture offset, alpha; §3, §6) and
    visibility tracks (§6) play with it; a track on the model's root node is not
    applied, the root carrying the placement [design]; particle, refraction and morph
    controllers are not played (§6);
  - `Sound: <SOUN>` keys the group passes play where the object is (anim_sounds.rs).
  - **A group the model lacks** (issue #2420): when the engine finds the model has no
    sequence of the group asked, that group is not what the object shows when drawn
    again; the group it replaced is the last asked again (`ObjectAnims::not_in_model`)
    [design]. A tripwire's script plays `Forward` and then activates itself, which asks
    "Open" (§5's activator rule); `tripwire01.nif` has no "Open", and the sprung wire was
    drawn armed after each cell build (Ranger Station Charlie).
  - **Drawn again and saved** (issue #1396): the rules keep the last group asked of each
    reference (`ObjectAnims::asked`, until `Reset3DState`) and save it with the game
    (`object_groups` in the snapshot). An object drawn again with no request waiting
    (after a load, or streamed back in) shows it: a group played once at its end, a
    loop from its start [design: a gate a script opened and left open is open after a
    load; whether the original keeps the pose is the capture question below]. Each rig
    is looked at once when the scripts run (not only in the frame it was drawn: a load
    draws the cells before the scripts start). Smoke test: `animated-collision-write`
    and `-load` (a Goodsprings plant played Forward, saved, loaded: `… drawn again,
    shows Forward at its end` and its collision following Forward).
  - **Collision follows the drawn model** (issue #1396; nvf-play `phys`): a reference that
    is not a door opening in place (doors: docs/spec-notes/doors.md) and whose model's
    sequences move the node of a collision body physics does not move itself (a gate, a
    lift, a drawbridge built as an activator or a static) keeps those bodies posed from the sequence its
    rig plays. The renderer publishes each rig's clip and time every frame
    (`nvf_play::phys::ObjectPoses`, from the clip drawn: one clock for both); an object
    not drawn is posed at the last group asked (a group played once at its end, a loop at
    its start), otherwise in its bind pose. A sequence moves a node when it has a track
    on that node or a parent whose pose leaves the node's stored transform somewhere in
    the sequence (sampled at its keys and at 16 even steps); a track holding the stored
    pose (an idle listing every node) does not count [engine choice]. The bodies stay solid while they move
    [engine choice: a lift or a drawbridge has to carry and stop actors; a door's leaf is
    not solid while it swings; hypothesis: the original pushes actors with them, as with
    doors]; nothing pushes an actor out of a body that moves into it beyond what the
    character controller does. Such a reference's drawn floors are not used (they are the
    file's unposed geometry). Tallied `animated_collision` per reference prepared and
    `animated_collision_refs` per reference added; log `phys: 0x<ref>: <n> animated
    collision bodies follow <sequence> at <t> s` the first time they move. The
    navigation tools (`fnvtool nav`, `phys`) pose doors only: other animated bodies stand
    in their bind pose there, which the static navmesh does not depend on.
- Log: `playgroup: <ref> plays <group> (once|loop, <s> s), flag <n>`; `… not drawn, shown
  when it is`; `… drawn, shows <group> as asked while it was not`; `… drawn (enabled at run time),
  plays <group> from <s> s as asked while it was not`; `… drawn again, shows <group> at its
  end|from its start`; `… its model has no
  sequence of that name`; at exit `playgroup summary: …`; the `anim sounds summary`
  counts "object group sounds".
- Smoke test (CLAUDE.md): Goodsprings at 21:00. On load the plants' OnLoad blocks play
  Backward or Forward, the house windows Left, the Prospector Saloon's lights
  (`0x0016B5E8`, Left: a 0.07 s loop of emittance and SELF_ILLUM) Left; at 12 s the test
  aid activates the Nevada agave `0x0016AF20`, whose OnActivate plays "Forward (once,
  0.47 s), flag 1" [observed in NVForever, 2026-10-11: 50 asked (Backward 22, Forward 11,
  Left 17), 0 without a sequence of that name].

### 5.4 Not built, and questions for captures

- Not built: `LoopGroup` (no opcode); particle, refraction and morph controllers in the
  groups (§6); IsAnimPlaying's "sequence type" grouping, if it is broader than the
  group. PlayGroup on actors is §7; alpha and visibility tracks §6; the bear trap's sound
  §8.
- Capture questions: does a picked agave keep its picked pose after a save and reload
  (we show Forward's end through OnLoad)? Does an activator gate a script opened keep
  its open pose, drawn and solid, after a save and reload, and does it push the player
  while it moves (issue #1396)? Does the saloon's sign flicker at night and
  stay steady by day? After a group played once, does an activator return to its idle,
  and does `IsAnimPlaying <that group>` stay 1 while its end is held (§6.1)? Do
  Goodsprings' house windows change at 19:30 and 6:30 (§6)? Does `PlayGroup Aim 1` on a
  creature or NPC hold the aim until it moves (§7.3)?

## 6. What else the sequences drive: alpha, visibility, particles, refraction

Survey: `fnvtool sim activator-models` (ACTI and TACT bases of FalloutNV.esm, 274 models
with sequences; `NVF_ARCHIVE_ORDER` set to the base archives) [observed]. Tracks by
controller type, with the models, bases and references whose sequences have one:

| Controller | Tracks | Models | Bases | Refs | Most in sequences |
|---|---|---|---|---|---|
| NiTransformController | 1,135 | 76 | 164 | 1,352 | Forward, Left, Backward, Right |
| NiPSysEmitterCtlr | 860 | 13 | 21 | 60 | Forward, Left, Idle |
| NiMaterialColorController | 484 | 76 | 147 | 901 | Left, Right, Forward |
| NiAlphaController | 384 | 69 | 98 | 1,330 | Left 50, Right 50 (models) |
| NiTextureTransformController | 360 | 37 | 52 | 161 | Forward, Right, Left |
| NiPSysModifierActiveCtlr | 315 | 9 | 16 | 21 | Forward, Left, Idle |
| BSMaterialEmittanceMultController | 279 | 46 | 50 | 654 | Left, Right |
| BSRefractionStrength / FirePeriodController | 50 each | 13 | 13 | 450 | Left, Right |
| NiVisController | 47 | 10 | 18 | 58 | Forward, Left, Backward |
| NiGeomMorpherController | 46 | 5 | 6 | 48 | Idle, Forward |
| NiPSysEmitterSpeedCtlr, …InitialRadiusCtlr | 5, 9 | 3, 1 | 4, 1 | 38, 1 | Forward |

- **The neon signs and lit windows** fade with alpha. Script `0x0011B039` (41 targets:
  the Strip's signs and window glows, Primm's `windowEm`, the Prospector Saloon's lights)
  plays Left when `GetCurrentTime` is above 20 or below 6 and Right between 6 and 20,
  each with flag 0 and only while `IsAnimPlaying` of that group is not 1. Across the 41
  models Left and Right drive alpha (`NiAlphaController`), emissive colour and
  multiplier, texture offsets and a few transforms; Gomorrah's lights also three
  `NiVisController` tracks [observed].
- `windowem01.nif` (`windowEm`, 18 refs in Primm): one quad `windowEm01:0` with an
  `NiAlphaProperty` (flags 4333) and an `NiMaterialProperty` that stores alpha 0; the
  controlled blocks name property type `NiMaterialProperty`, and the `NiAlphaController`
  targets that property. Left (loop, 0.03 s) keys alpha 1, Right (clamp, 0.07 s) alpha 0,
  SpecialIdle (the object's own idle, loop) alpha 1 [observed].
- Goodsprings' house windows (`NVCraftsmanWindowExt`, script `0x001759FC`: Left after
  19:30, Right after 6:30) key alpha 0 to 1 over Left's 0.27 s and 1 to 0 over Right's
  0.33 s, with constant emittance, SELF_ILLUM, SPEC and refraction tracks; their shape
  `Object01:0` has no `NiAlphaProperty` [observed], so the alpha track changes nothing we
  draw [hypothesis: material alpha without an alpha property is not blended]. What makes
  these windows look lit at night in the game is not known.
- Particle controllers (`NiPSysEmitterCtlr` and the others) sit in 13 activator models
  (the Hoover Dam sand storms `nvsandstormnight.nif`, the vertibirds' dust, sparks).

### 6.1 Behaviour implemented

- An `NiAlphaController` track replaces the shape's material alpha while its sequence
  plays (`nvf_assets::anim_play::MaterialTarget::Alpha`) [hypothesis: as the emissive
  tracks, the value replaces the stored one]; offset and alpha stay where a track last
  left them when the next clip has no track for them [design]. It shows only on shapes
  drawn blended or tested.
- An `NiVisController` track shows (`Visibility::Inherited`) or hides the node it names
  at the sequence time, keys stepping (`fnv_nif::anim::BoolTrack::at`) [hypothesis, as
  for weapons, animation.md §9.7]. A shape the file hides is built anyway when a
  visibility track of one of the rig's clips names it or an ancestor, and it starts
  hidden; it is drawn by the rig only [design].
- A model whose sequences only fade or show and hide its shapes now counts as animated:
  it gets a rig and a `GroupRig` like any other.
- Not built: particle-system controllers (no placed-object particle systems are drawn;
  `nvforever::particles` serves add-on nodes such as muzzle flashes, not cheap to reuse
  here), refraction (no refraction shader), `NiGeomMorpherController` (morph targets),
  the `SPEC` colour and texture scale and rotation tracks.
- Seen in a run [observed in NVForever, 2026-10-11]: Primm at 19:55, the player at
  (-51300, -49430) facing east. The windows `0x000E532A` and `0x000EC9A6` play Right
  (alpha 0, dark); when the game clock passes 20:00 their script asks for Left and they
  light ("plays Left (loop, 0.03 s), flag 0"). Screenshots at 11 s and 19 s: the two
  window regions' mean luminance 8.5 → 78.2 and 8.7 → 76.6, an unlit window beside them
  9.5 → 9.5, the wall 40.5 → 40.4.
- Seen on the way: by day the script asks for Right every frame in which it does not
  play; a clamped Right is reported ended at its end, so it is asked for again and
  replayed every 0.07 s (in a Primm run 2,002 Right asked in 18 s). The look does not
  change (Right holds alpha 0), but it suggests that in the game a group played once and
  held at its end still counts for `IsAnimPlaying` [hypothesis, against script
  `0x000AE070`, which waits for `IsAnimPlaying Forward` to become 0 before playing Left;
  a capture question, §5.4].

## 7. PlayGroup on actors

### 7.1 What FalloutNV.esm's scripts do [observed]

- `fnvtool sim playgroup-calls` now lists the calls whose object it cannot name from the
  unit (`unresolved`): 14 calls through reference variables in 6 scripts (`linkedRef`,
  `myLink`, `rVidScreen`, `rPointer`: activators and placed markers), 75 in 19 object
  scripts no base carries (trigger, button and sign scripts), 1 in a package's end
  script. No call names a placed NPC or creature, and no script that an NPC_ or CREA base
  carries calls PlayGroup.
- The one actor call: package `CRHDDogNCRBuddyBomberAlertPackage` (`0x0013C6F7`, a
  Follow package whose target is `VHDLegionAssassinBomberREF`) ends with `PlayGroup Aim
  1`, run by the actor whose package ends (a dog, by the Editor ID). Who is given the
  package was not traced.
- `PlayGroup Idle 1` returns an actor to normal [documented: GECK wiki PlayGroup]; three
  calls use Idle, all on objects.

### 7.2 Which clip a group names on an actor [observed]

`fnvtool nif anim` over `Fallout - Meshes.bsa` and `Update.bsa` (42 skeleton folders of
`characters\` and `creatures\`), the sequences named like a group:

- **Aim, Equip, Unequip**: per weapon stance prefix, `<prefix><group>.kf`; 35 folders
  have an Aim clip, 27 of them as `h2haim.kf` (the unarmed stance); the other 8 only
  under another prefix (the eyebot's `1hpaim.kf`, the sentry turret's `2haaim.kf` and
  `2hraim.kf`, …). Equip and Unequip likewise (26 of 35). An armed NPC's are its
  weapon's (`weapons.rs`).
- **Idle**: `mtidle.kf` in 37 folders, `h2hidle.kf` in 4, `idle.kf` in 1.
- **Movement and turning** (Forward … FastRight, TurnLeft, TurnRight): `mt<group>.kf` in
  the folder or under `locomotion\` (and `locomotion\male\`, `\female\` for people):
  found so in 30 of the 33 folders with a Forward clip and in all 32 with a TurnLeft
  clip.
- **SpecialIdle**: 1,287 clips under `idleanims\` whose sequence name begins with
  SpecialIdle, many per folder (the dog's 19); no one clip is "the" SpecialIdle.
- One misspelt file (`creatures\dog\mtbackrward.kf`, sequence Backward) is not found by
  name.

### 7.3 Behaviour implemented (nvforever `actor_groups.rs`)

- A request for a reference whose base is an NPC_ or CREA (or a drawn actor) goes to the
  actor side; one for an actor that is not drawn is dropped and reported not playing
  [design, as idles].
- The clip, in order: the actor's weapon stance clip of that name; its locomotion set's
  move of that name; else `h2h<group>.kf`, `specialanims\h2h<group>.kf`, `<group>.kf`,
  `mt<group>.kf`, `locomotion\mt<group>.kf` in its skeleton's folder, if the file's
  sequence has the group's name [hypothesis: the names match, §7.2]. SpecialIdle plays
  nothing [design]; `Idle` fades out what plays in the slot, so the actor's own
  animation shows again [documented meaning; how is ours].
- The clip plays in the actor's idle slot (idles.md §7, the top two blend layers) with
  the fades of its text keys: a clamped clip once, a looping one until replaced, until
  `PlayGroup Idle`, or until the actor walks [design]. A loop asked for again while it
  plays goes on [design, as for objects]. Flag 0 waits for what plays in the slot (a
  loop leaves its loop at the end of its pass); 1 and 2 start at once.
- `IsAnimPlaying` sees the group from the call until the clip ends or is replaced, or it
  cannot be played. A group is not an idle for `IsIdlePlaying`, and an actor's idles are
  not groups for `IsAnimPlaying` [hypothesis].
- Test aid: `NVF_PLAY_TEST_SCRIPT=<s>:play_group:<ref>:<group>[:<flag>]` calls the
  reference's `PlayGroup` (fnv-game's function, so it is traced as `anim_asked`).
- Log: `actor group: <actor> plays <group> from the <weapon stance|locomotion set|
  skeleton folder>, once|loop, <s> s, flag <n>`; `… flag 0: after what it plays ends`;
  `… its turn come`; `… plays Idle (flag n): back to its own animation`; `… no clip of
  that name for its skeleton <path>`; `… the actor is not drawn`; at exit `actor group
  summary: …` (the playgroup summary counts what it handed over).
- Seen in a run [observed in NVForever, 2026-10-11]: Goodsprings at 12:00, the smoke
  test of CLAUDE.md. `GoodspringsSettler01Ref` (armed) plays Equip (weapon stance, once,
  0.37 s), then Aim (loop, 2.47 s) with flag 0 after it, then Unequip (0.47 s); the
  big horner `0x0010706E` plays Aim from `creatures\nvbighorner\h2haim.kf` (loop, 6.67 s)
  until `PlayGroup Idle 1`; SpecialIdle finds no clip; Easy Pete, in the saloon, is not
  drawn.

## 8. Sound keys of groups: a trap heard

- The bear trap (`TrapBearTrap`, `beartrap01.nif`, 31 refs; script `0x0002CBA1`): on
  activation by the player it shows `TrapBearDetectMsg` with "Leave it alone." and
  "Disarm the Bear Trap."; with Repair at least `trapbearskillreq01` (23, set by stage 1
  of quest `0x0002012B`, which every trap's OnLoad sets) button 1 disarms it and plays
  Forward with flag 1. Forward (clamp, 0.1 s) carries `Sound: TRPBearFire` at 0.033 s;
  Backward `Sound: TRPBearOpen` [observed]. Tripwires (`TrapTripwire01`, 31 refs) carry
  `Sound: TRPTripwire` 0.00001 s into Forward; pressure plates `Sound: TRPPressurePlate`
  likewise [observed].
- Two faults found and fixed in `object_anims.rs`: the keys of a group's first frame
  were skipped (a group started had no time of its last frame; the tripwire's key would
  never sound), and a clamped group's keys were passed again on the frame it ended (its
  cycle count went up past the end; the bear trap's sound played twice).
- Seen in a run [observed in NVForever, 2026-10-11]: the player at (-68124, -17000) by the
  trap `0x00165495`, `trapbearskillreq01` set to 0 by the test aid, the trap activated
  at 15 s and button 1 picked at 15.1 s: "You have disarmed the trap.", "playgroup:
  0x00165495 plays Forward (once, 0.10 s), flag 1", "passes the sound key TRPBearFire of
  Forward at 0.033 s", and the sound `sound\fx\trp\bear\fire\trp_bear_fire_01.wav` played
  at 217 units; "1 object group sounds" in the anim sounds summary.
- A key's second word (`sound: OBJVaultGearConsoleUnlock SoundNode01`, the vertibirds'
  `SoundNodeProps`) names a node of the model [hypothesis]; the sound is played where the
  object is.

## 9. Drinking from activators (ESM audit group 7, 2026-10-23)

- "Non scripted activators can serve as drinkable water sources" [documented: GECK
  wiki Activator]; the field is ACTI WNAM "Water Type" [documented: xEdit]. In
  FalloutNV.esm 31 activators name one, all fixtures: toilets, sinks, urinals, water
  coolers, fire hydrants and water pumps (`DrinkSink01Avg`, `DrinkToiletClean03Purified`
  …), each with the water drunk (`d1PurifiedWater0r30h` … `d5TerribleWater20r4h`)
  [observed]. The ESM audit listed them as water drawn as plain models; they draw no
  water surface.
- As built: the crosshair offers "Drink" on such an activator [hypothesis: the
  original's word]; activating it by hand applies the water's actor effect (WATR XNAM,
  the healing and the rads of drinking it, radiation.md §3) to the player, after its
  script if it has one [hypothesis]. The log says `activate 0x… <name>: the player
  drinks from it`.
- The drinking idle (#2349): the player's activation asks the idle tree's
  `ActivateIdles` root, whose `ActWaterSource` branch (IsWaterObject) plays
  `1stPersonActWater` in first person and `3rdPersonActWater` in third, holding nothing;
  the effect still applies at the activation [design, #2349] (idles.md §16).
- Not built: a drinking sound (no record names one), NPCs drinking.



### Object water under the crosshair

[documented] [GetCrosshairWater](https://geckwiki.com/index.php?title=GetCrosshairWater)
returns the WaterType belonging to the object reference under the crosshair within
activation range. [observed: original installed mod data] The Contextual Prompts
callback checks this before its generic reference/icon branches to select an
object-water image. Its initial callback is queued before the runner's dependency
guards; persistent event registration and image injection occur after the guards.
This source demand does not establish original-game execution or native timing.

[design] Read the existing activation-range GetCrosshairRef publication, qualify
its base as ACTI, and read WNAM through the existing effective record accessor
used by GetActivatorWaterType. SetActivatorWaterType changes are visible to the
next query. Missing/unmapped targets, non-activators and absent WNAM return no
form. This adds no ray, surface-water detection, retained result or World mutation.
It follows the existing target publication lifetime rather than inventing separate
visibility, deletion or distance policies. Drinking, water remapping, HUD hiding,
provider/version reporting and event dispatch are unchanged. Full surface-water
and native timing behavior remain unqualified.

[validation plan] Independently authored file-backed CompileScript source chooses
the injected image using the real query; a real setter/getter pair checks effective
record authority and repeated target/invalid/nonwater/reentry cases preserve the
World snapshot. A separate engine control uses the actual activation collision ray
and then a source image branch and real draw Picture/title pieces, retaining
ordinary target/getter/accessor controls and collision eviction. The file-reader
fixture has its own process-scoped test target. Both tests and identical-fixture
baseline are unexecuted at authoring; the engine callback is direct compiled source,
not an assertion that original initialization or OnCrosshairOn has run. The separate
event-producer contribution requires its own composition/execution. Original
profile, native, rendered and full Contextual Prompts/yUI acceptance remain open.
