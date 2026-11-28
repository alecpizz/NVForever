# Session projectile traits at initial shot creation

## Live position edits and source attribution

[documented: GECK wiki [SetPos](https://geckwiki.com/index.php/SetPos)] The
calling reference moves on the specified world axis. [documented: GECK wiki
[SetProjectileRefSource](https://geckwiki.com/index.php?title=SetProjectileRefSource)]
The new live-projectile source is the attacker considered by a hit target.
[observed: installed B42 Quickthrow3.1b original script demand, 2026-11-16] The
original quest polls its configured key, launches through a proxy creature, then
walks live projectile references. Its height correction uses SetPos; knives retain
that flight while an explosive may fire a replacement from the corrected pose.
It changes source before subsequent grenade/mine/knife outcomes. Original source
and assets remain private and are not copied into the tests.

[design] A successful canonical SetPos on an active engine shot queues its complete
position for the actual flight consumer. Consume the latest queued position once
before publishing the next physical position. Preserve velocity, age, travelled
distance, fuse, visual offset and launch data. Do not turn a reference's SetAngle
into an inferred trajectory edit. Pending resolved hits retain their existing
policy; position requests for them do not retarget those hits. Nonfinite positions
are not admitted to physical flight, without changing the canonical setter's
existing conversion/result behavior. Retirement clears queued position edits;
the per-live-shot queue is session state, not new save data.

[design] Before flight/detonation outcomes, read canonical Shot.source into the
existing flight attribution. Explosion causes, settled ownership and published
visual ownership then use the new source. Preserve the projectile's original
physical shooter/collision exclusion, remote-detonation admission and direct-hit
source recheck: attacker attribution alone does not establish those native policies.
Missing session rows do not become implicit source0 edits. Use source identity
accepted by the existing setter; add no new actor-only admission restriction.

[hypothesis] Native timing, live-projectile SetPos response and source changes on
collision exclusion/remote-detonation ownership still require permitted capture.
The mapped engine behavior above is not a native parity claim. Tests must retain
the old-consumer failure and corrected success through real source calls, emitted
flights, world/visual movement, actual blast/settled outputs, neighbor and retirement
controls, and the current pending-hit/save/unload boundaries. Original installed
initialization, skeleton/animation, physical input, inventory, cook/return features,
world rendering and complete Quickthrow lifecycle remain separate acceptance.

[design: authored, unrun] Six independent live-edit controls extend the existing
portable lifecycle fixture without changing its default records. They reach real
source SetPos, flight and visual movement with a live neighbor; real timed blast
effects and an ActorHit attacker; natural terrain settling and the existing mines
consumer's placed World owner; null-source/non-shot admission, deletion and travel;
retained resolved-hit point/due time; and nonfinite position rejection at flight.
The separate candidate-only queue control reaches successful source requests,
missing-runtime delivery, deletion and actual checked_snapshot/restore_save.
It inspects the new queue and is not identical-baseline instrumentation. For the
old-consumer comparison, run the identical six live-edit bodies and fixture
refactor on accepted main, omitting only the new internal-queue module declaration;
do not add a fake production API or weaken their assertions. No test execution is
claimed here.

[documented: public main da13f4f8a65cb56d9f4f38823d79a0126ff9467b, crates/fnv-game/src/nvse_jip7.rs num_field/set_field; crates/fnv-combat/src/data.rs CombatForms/projectiles; engine/nvf-play/src/combat.rs fire/actor_attacks] Existing Game setters retain session field overrides, while the original physical launch reads imported typed CombatForms records. Getter readback alone does not establish new-shot velocity or carried impact force. The existing trait mapping identifies speed as2 and impact force as9; portable schemas/catalogue define ammo/weapon projectile selection.

[observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24,
`GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated staging copy,
read-only, committed probe `w4g_11_ammo_clear_and_proj_delete` in
`tools/capture/nvse/catalogs/wave4g_f2_extenders.json` (companion catalog PR #3803);
no DLL or source consulted] `SetAmmoProjectile` clears an ammo's projectile only when
the optional 2nd argument is omitted (`SetAmmoProjectile rAmmo`, yielding `00000000`);
passing an explicit null ref (`0`) as the 2nd argument (`SetAmmoProjectile rAmmo rNull`)
is a no-op that leaves the current projectile unchanged (verified on both authored
`Ammo12GaSlug` `0x0013E447` and session-overridden `Ammo10mm` `0x00004241`). Verbatim
printed probe output:
`slug(p0=001572C5,null=001572C5,omitCmp=1,omit=00000000)|a10(p0=00000000,set=0002CD5F,clr=0002CD5F)`.

[hypothesis] Resolve the selected ammo/weapon projectile for each new shot. Clearing the ammo projectile (by omitting the 2nd argument of `SetAmmoProjectile`) falls back to the weapon's. Resolve a temporary PROJ form through the existing original-form relationship, keep the requested identity and its own overrides before inherited original overrides, and copy only supported speed/impact-force values into a per-shot record. Preserve finite signed values including zero; refuse nonfinite or nonrepresentable f32 overrides. Leave typed caches, unrelated forms and existing flights unchanged.

[hypothesis] Share the existing random spread/spawn operation between ordinary and edited shots. Preserve no-runtime/no-override typed behavior, vanilla weapon speed multipliers, normal player ammo/magazine/counters/animation, and the distinction between a selected missing typed record (effects/counters but no physical flight) versus no selected projectile (the existing silent path). This leaf adds no setter and broadens no generic World placement admission.

Process qualification (historical authored CPU controls): An owned coherent d9 source pair ran the same eight source-compatible controls: three baseline passes and five physical-consumer assertion failures, then all nine candidate controls passed. The ninth control uses the new factored spawn API and was candidate-only. An earlier E0463 host dependency-search setup failure ran zero tests and remains separately recorded. Later coherent c28 CPU contexts also passed the original nine bodies. These results belong to those exact source contexts, not any later public revision.

[documented: authored engine/nvf-play/src/combat/projectile_launch_tests.rs] Nine portable-record controls exercise actual Game setters and next-frame actor shots, physical velocity/carried force, restoration without editing existing flights, temporary-form trait inheritance, unrelated typed neighbors, missing/no selected projectile and normal equip_sync -> combat::fire -> fly with actual ammo, magazine, counters, animation and shot messages. The player input boundary is CPU PlayerControl.trigger, not OS input or complete animation scheduling. A fixed-random three-projectile control checks six draws and independent analytic direction sentinels.

Process qualification (pinned public hosted CI): [run 43666264301](https://github.com/LootJunkieAzza/NVForever/actions/runs/43666264301) and its [guard](https://github.com/LootJunkieAzza/NVForever/actions/runs/43666264187) completed all seven checks successfully for source head `cc789749b9b69c27dac9a1a7609bb2b2ee9c76df`. The actual checkout was merge `897883d59090670466e86a87458e3ee18d375e1d`, combining that head with main `78408bcba623baaabbab4bb77a0e19d3a5e4fd72`. All nine portable launch controls passed once in the engine job. Complete summaries reported 3019 core passes with four existing ignored cases, and 2917 engine passes with zero ignored cases; both had zero failures, measured or filtered cases. Both strict Clippy checks, both dependency audits, the six Python suites and the data guard passed. These are repository/process results in that tested merge context; they are not pristine-head, native, rendered or full B42 acceptance. Later source revisions require their own exact-head CI and review.

[hypothesis] The authored opt-in physical-consumer source and scenario below require current-source typechecking and review. Actual GAME execution remains optional and UNRUN. Getter-only JIP smoke is not edited-velocity proof; the aid observes actual physical insertions. No guessed asset identity or rewritten acceptance oracle is introduced.

[hypothesis] Initial launch support does not complete B42 startup, quickthrow input/animation, item/reference spawning or native parity. Temporary form trait resolution does not establish temporary-base World reference admission, visual model lookup, GetBaseObject/type51 scans, TTL deletion or save/session ownership. Existing clone/high-ID/callback/save boundaries remain untouched. No generic high-ID exemption, warning suppression, DLL-version spoofing, native implementation, installed script, asset or save is part of these authored controls.


## Authored physical-consumer scenario

[hypothesis: bounded diagnostic policy] The opt-in `NVF_PLAY_TEST_PROJECTILE_LAUNCH`
contains one bounded weapon EditorID, resolved uniquely from the loaded typed
records. The authored scenario uses `Weap10mmPistol`; missing, ambiguous, modified,
beam, multi-projectile or unsupported records refuse setup. No numeric FormID is
guessed. The aid requires the canonical `b42-launch-probe-home` directly under
the smoke output directory, the matching actual SaveConfig, and no preexisting
saves. A fresh output directory is required for each run.

[hypothesis: physical acceptance] Compile real script getters/setters and
`player.FireWeapon` with exact loaded forms passed through `StringToRef`. Observe
only the resulting actual `Flying` insertion, after the ordinary actor shot
consumer pushes it. The first shot must carry the staged finite speed and force;
a second shot must carry independently resolved original physical values after
restoration. Keep weapon speed multipliers and the ordinary random shot path.
Scripted FireWeapon does not debit ammo here; the normal player-attack debit is
covered separately by the unchanged portable CPU control. Getter values or a
queued request alone never satisfy the physical acceptance oracle.

[hypothesis: cleanup policy] Preserve exact presence and XVal contents of the two
session override entries, not just their numeric getter values. An opaque lease
binds restoration to its original Game incarnation and can be consumed once; a
wrong-owner attempt retains it for its actual owner. For restored physical values,
a nonnumeric prior override follows the existing typed launch fallback rather
than the numeric getter's zero. Unsupported original representations refuse the
fixture. Capture and restore weapon/ammo item counts on success, script error,
physical mismatch, timeout or observed teardown. Cleanup does not claim a general
rollback of inventory events, world state, arbitrary concurrent edits or a
retired/replaced Game. Those boundaries fail visibly without touching a replacement.
The run owns its isolated QA session and never loads or writes an original save.

[hypothesis: bounded scheduling] The aid is absent in normal runs. Its finite
waiting and active-stage deadlines, at most two matching requests, distinct actual
insertions, ordered stage markers and final exact-field/count cleanup are required
by `projectile-launch-physical-session-traits`. Raw failures remain visible. The
launcher tests use a real owned child only to verify the delivered EditorID, HOME
and inherited-aid filtering; synthetic judge rows test acceptance rules and are not
rendered/game results.

Process qualification: the pinned current2c source's seven lease controls,
nine initial-launch controls and seven physical/context controls passed all23
selected CPU cases in a coherent owned dependency graph. This result is specific
to that source; it is not whole current-public canonical CI or GAME acceptance.
The two Python launcher/judge boundary checks also passed against that source.
Original failed source-argument assumptions and setup errors remain retained.

[documented: authored engine/nvf-play/src/combat/projectile_launch_probe.rs::drive]
The frame guards use strict greater-than comparisons against7200 waiting frames
and300 active frames, checked before staging or restored-shot dispatch. Independent
real-time guards remain120 waiting seconds and15 active seconds.

[hypothesis: bounded diagnostic policy] Two additional direct boundary controls set
the actual Probe counters near each cap, with real elapsed time below both separate
timeouts. The wait control admits7200 without staging, makes the7201 update eligible at20 real seconds (below the waiting timeout), and requires refusal before preparation can acquire a capsule or fire.
The active control admits the first real physical insertion at300, aborts301 before
a restored second request, and requires exact absent-field restoration, original
inventory counts and no repeated dispatch or cleanup. They use no7200/300-frame
loops. All original23 control bodies are unchanged. Authored coverage specifies
the required oracles; compilation and execution results must be bound to the exact
source revision and reported separately. Actual GAME execution remains optional
under CONTRIBUTING; the meaningful consumer scenario must be source-reviewed and
typechecked before Ready.
Nothing here completes B42 startup, Quickthrow OS input/animation, temporary World
reference placement/model lookup, TTL deletion, save ownership, native or rendered
acceptance.

## Explicit removal of an associated projectile

Implementation context: on pinned public main
`6692d2da4612fa4aae9f0a9eea177452ba511c4e`, `functions/refs.rs::mark_for_delete`
sets the reference's deleted state. The engine's `combat::fly` owns separate
`Flying` and `Pending` entries; `ShotVisuals` also carries the remaining `Ghost`
visuals to `shot_fx::draw_flying`. The next flight step and due-hit delivery do not
consume that deleted state. `projectile_gone` retires the script session and fuse
state on natural completion. These are repository implementation facts, not a
native-game observation.

[documented: the structured `Disable` and `MarkForDelete` entries in
`schemas/script/functions.json`, sourced there to the GECK wiki] Disable and
deletion are distinct. Disable can be reversed; MarkForDelete queues removal.
The catalogue does not establish native projectile-specific cancellation timing.

[hypothesis: engineering cancellation policy] Before the next flight consumer
processes detonation requests, advances a projectile or delivers a pending hit,
retire an associated shot whose World reference is deleted. Retire only that
reference's flight, pending hit, remaining visual and session/fuse bookkeeping.
A missing session row must not allow the old delayed hit to fall back to the
weapon's default damage. A normal reference without the engine's `shot` flag is
not an owned projectile and is not retired by this bridge.

[hypothesis: visual lifetime policy] Natural completion can mark a reference
deleted while its beam visual continues fading. Preserve that fade: a Ghost with
no active flight or pending hit is cancelled only by an explicit MarkForDelete
request. Keep its original shot reference solely for this identity check, never
select recipients by model name, base form or position. The bridge scans the
existing flight/pending/visual collections and allocates a set only for removed
references; it adds no persistent cancellation history.

[hypothesis: compatibility boundaries] Disable alone, a fade followed by Enable,
missing script runtime and failed reference creation do not become permanent
physical cancellation. Keep an already queued FireWeapon request from a removed
reference: it uses the caller's stored pose and can create a distinct replacement.
Deletion before the next combat consumer wins over a queued remote-detonation
request for that removed flight. Already delivered effects and independently
queued blasts are not rolled back. Natural expiry, impact delivery, door travel,
settled charges and unrelated shots keep their existing ownership paths. No save
format, temporary-base persistence or restored-session policy changes here.

Authored consumer coverage uses portable records, compiled public script calls and
the actual player clock, actor-shot and flight systems. It includes live cleanup
with a moving neighbor, replacement ordering, reversible Disable/fade, actual
terrain-hit Pending/Ghost creation, the missing-session-row fallback, remote
detonation, natural beam fade, expiry and Travel. These bodies specify meaningful
baseline/candidate boundaries; they are not a statement that any revision has
compiled or passed. Exact execution and native/rendered/mod-profile qualification
must be reported separately. Disable-only visual suppression is not established
by this deletion-only change.


## Full non-flag projectile trait overrides

[documented: GECK wiki `GetProjectileTraitNumeric`, `SetProjectileTraitNumeric`,
`GetProjectileExplosion`, `SetProjectileExplosion`, `GetProjectileLight`,
`SetProjectileLight`, `GetProjectileMuzzleFlash`, `SetProjectileMuzzleFlash`,
`SetProjectileSound`; xEdit `wbDefinitionsFNV.pas` `PROJ`; observed: `jip7`, `jip7b`,
`gaps6`] Session overrides for all 15 non-flag numeric `PROJ` traits (`DATA/type`,
`gravity`, `speed`, `range`, `tracer_chance`, `explosion_alt_trigger_proximity`,
`explosion_alt_trigger_timer`, `muzzle_flash_duration`, `fade_duration`, `impact_force`,
`rotation.{x,y,z}`, `bouncy_mult`, and `VNAM` detection sound level), form traits
(`DATA/explosion`, `DATA/light`, `DATA/sound`, `DATA/sound_countdown`,
`DATA/sound_disable`), and `NAM1` muzzle-flash model path are exposed through the
canonical helpers `fnv_game::Game::projectile_traits`,
`Game::projectile_trait_overrides`, and `Game::projectile_sound` (`DATA/flags` stays
separate).

- **Validation** [design; observed: `gaps6` null explosion leaves existing explosion;
  `jip7`/`jip7b` type and VNAM ranges]: `SetProjectileExplosion` requires a `PROJ`
  target and an `EXPL` form (including `TempCloneForm` aliases) and leaves the explosion
  unchanged on null; `SetProjectileLight` clears on omitted/null and requires a `LIGH`
  (or cloned `LIGH`) otherwise; `SetProjectileSound` accepts `PROJ`/`SOUN` (including
  `TempCloneForm` aliases) for slots `1..=3`; `SetProjectileTraitNumeric` rejects
  non-integer/out-of-range trait IDs without mutating state.
- **`TempCloneForm` copy-at-creation snapshot isolation** [observed: Fallout: New Vegas
  1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`),
  10 official base masters, isolated staging copy, read-only, committed probe
  `w4g_02_tempcloneform_world_proj_csty` in
  `tools/capture/nvse/catalogs/wave4g_f2_extenders.json` (companion catalog PR #3803),
  verbatim printed output
  `proj(0002CD5F:t1=3.00,t2=27520.0,t4=0.0|cl1=1234.5/0.75|afterSrc=5678.5:cl2=1234.5/0.75|cl2sub=1234.5)`;
  no DLL or source consulted]: cloning a `PROJ` snapshots all active session overrides
  (numeric traits, `DATA/flags`, form slots, and model/muzzle-flash paths) from the
  cloned source onto the new clone (`Game::snapshot_projectile_traits`), so pre-clone
  edits carry onto clones and clone-of-clone chains, while post-clone edits on the
  source (or parent clone) never bleed into already-created clones. Untouched fields on
  a cloned `PROJ` read directly from the authored record instead of live-inheriting
  later overrides on the original.
- **Headless combat consumer adapter** [design]: `fnv_combat::projectile_overrides::ProjectileOverrides`
  applies sparse non-flag overrides onto per-shot / per-mine `ProjectileStats`,
  `ShotChain`, and `ProjectileFx` copies (feeding `Projectile::spawn`, `fire_projectile`,
  `MineRules::trigger_radius`, `MineRules::fuse`, `mine::is_mine`, and
  `ProjectileStats::explodes`) without mutating loaded `CombatForms`/`CombatFx` caches or
  touching `DATA/flags`. Engine call-site wiring into `nvf-play` remains a separate
  engine step.
