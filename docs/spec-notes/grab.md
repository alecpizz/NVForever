# Grab: picking up, carrying, dropping and throwing objects

The player holds a loose object or a corpse in front of the view, carries it, and drops
or throws it. Code: `crates/fnv-phys/src/grab.rs` (the hold in the physics world),
`engine/nvf-play/src/grab.rs` (controls, limits, scripts, log), `fnv_game`
`Game::set_player_grab` and IsPlayerGrabbedRef (`functions/senses.rs`). Queue item 24
(2026-10-21).

## 1. Sources

- The control map: the game's `Grab` control is `002CFF09` in `[Controls]` of the
  owner's `FalloutPrefs.ini`: key Z (DirectInput 0x2C), no mouse button, controller
  byte 09 [observed, docs/spec-notes/controls.md §2]. The extender's GetControl calls
  it control 27 [documented: GECK wiki GetControl example, "Returns the DirectX
  Scancode assigned to the "Grab" control: GetControl 27"].
- The script side: block types OnGrab (0x21, 4 scripts) and OnRelease (0x22, 3 scripts)
  [observed: script-functions.md §3.3]; the extender's event handlers OnPlayerGrab /
  OnPlayerRelease fire "when the player picks up (Control Code 27)" and "releases (drops
  the grabbed ref)" an object [documented: GECK wiki OnPlayerGrab, OnPlayerRelease];
  IsPlayerGrabbedRef "returns true if the player is currently grabbing the selected
  reference" [documented: GECK wiki IsPlayerGrabbedRef]. GetPlayerGrabbedRef exists in
  the wiki's GECK list but has no New Vegas opcode in the catalogue
  (`schemas/script/functions.json`): not built.
- The shipped uses [observed: decompiled scripts, `fnvtool script decompile`]:
  `VCG01BrahminSkullSCRIPT` (0x00104C04) on the brahmin skull activator in Doc
  Mitchell's house (REFR 0x00104C10, base `VCG01SkullBrahminActivator`, model
  `clutter\skeleton\brahminskull.nif`, one dynamic body of 5 kg, layer 10): its
  OnActivate block is empty and its OnGrab / OnRelease blocks set CGTutorial stages 22
  and 24 while VCG01's objective 20 is shown; an older copy (0x0002F562) without the
  objective test; 0x000C79DF with empty OnGrab / OnRelease; two trigger scripts that
  test IsPlayerGrabbedRef on corpses carried into them (VMS18CarlyleStClairREF into the
  dumpster, ProperBurialCorpseref into the grave).
- Game settings: the executable names `fGrabMaxWeightWalking`, `fGrabMaxWeightRunning`,
  `bGrabIK` and the `fZKey*` family (`fZKeyMaxForce`, `fZKeyMaxForceScaleHigh/Low`,
  `fZKeyMaxForceWeightHigh/Low`, `fZKeyObjectDamping`, `fZKeySpringDamping`,
  `fZKeySpringElasticity`, `fZKeyMaxContactDistance`, `fZKeyMaxContactMassRatio`,
  `fZKeyComplexHelper*`) [observed: setting names among the executable's strings; their
  values are not there in readable form]. None is a GMST record of the shipped plugins
  [observed: FalloutNV.esm inventory, 2026-10-21], so their values are the executable's
  defaults and not known. The wiki's Ragdoll page says a ragdoll's "Articulated" flag
  "applies velocity equally to all bones of a grabbed object" [documented: GECK wiki
  export, Misc.xml]; not built (only the held body is driven).

No capture of the original's grab was made (the owner's queue rule: no original runs for
this item). Everything below that is not tagged otherwise is [hypothesis].

## 2. Controls

- **Grab (Z)** held, with something that can be held under the crosshair: it is picked
  up and held until Z comes up [hypothesis: the settings are named after the Z key, as
  in the earlier games' hold-Z grab].
- **Activate (E)** held for 0.4 s on it picks it up too, and holds it until E comes up;
  a shorter press activates it as before, when E comes up [hypothesis: the tutorial
  skull's empty OnActivate block makes sense only if a press of E does nothing and a
  hold of E grabs; 0.4 s is a guess]. Presses of E on anything that cannot be held are
  unchanged (activated on the press). The test aid `NVF_PLAY_TEST_ACTIVATE` still
  activates at once.
- **Use** (attack) while holding throws instead of firing.
- On a controller: the right stick's press is Grab (controls.md §2); A held as E.

## 3. Holding

- **What can be held**: under the crosshair ray, within the activation reach (150), a
  reference whose collision became one dynamic body (placed clutter, a dropped item),
  or a corpse's ragdoll (the body nearest the hit). Not: living actors, static or
  keyframed collision, a reference with several dynamic bodies, and a body on the
  non-collidable layer (15): the ray passes through it. The pickable plants White Horse
  Nettle and Broc Flower have one dynamic box of 45 kg there [observed: `fnvtool nif
  decode`]; held, they came off the ground whole (#3389). They stay rooted [hypothesis: the
  players' report; observer check 278]. That body is placed fixed and touching nothing
  (`fnv_phys` world.rs `rooted`), so walking into one, a blast or a body falling on it
  leaves it be (#4917) [design: in the original nothing but the grab key moved them,
  and one let go fell through the ground, observer check 278].
- **Where**: the grip is the hit point on the body; it is wanted on the crosshair ray at
  the distance it was picked up at, no nearer than 50 units. The body hangs from that
  point and turns about it (its spin damped to 5 % a second).
- **How** (`fnv_phys::grab`): before each fixed step the held body's velocity is set to
  close 12 × the gap a second, at most 700 units/s, plus the step's fall; its change of
  velocity is bounded by 100 000 kg·units/s² over its mass. It stays a dynamic body, so
  walls, floors and other objects stop it. A ragdoll held at rest moves again. While held it is never put at rest again
  (the engine's calm and 3 s guards, ragdoll.md §4): its held body would stop following
  and the grab would drop it [design]. Smoke `grab-corpse`.
- **Let go**: when the key comes up (dropped, it falls); Use throws it along the view at
  2500 kg·units/s over its mass, at most 600 units/s; it is dropped when it is more than
  90 units from where it is wanted (caught on something), when play stops (a menu, VATS,
  furniture) or when its body is gone (its cell unloaded).
- **Weight**: a body heavier than `fGrabMaxWeightWalking` is not picked up; while one
  heavier than `fGrabMaxWeightRunning` is held the player walks. The plugins set
  neither, so 100 kg and 25 kg are used; a plugin's GMST of that name is read instead.
  The unit is taken to be the model's Havok mass (kg). For a corpse it is the held
  body's mass, so a corpse is dragged by a limb or its trunk.
- The player's own weapon: Use does not fire while something is held.
- Where it comes to rest is kept as any pushed object's (physics.md, `crate::phys`
  write-back): placed clutter keeps its new place in the save, a dropped item too.

## 4. Scripts

The engine tells `fnv_game` what is held (`Game::set_player_grab`) when a grab starts
and ends (manual release, throw, caught/out-of-range drop, play stopped by a menu/VATS/
furniture, body or reference gone from simulation/physics, or physics teardown [design]).
A change updates `senses.grabbed`, runs the OnRelease blocks and `OnPlayerRelease`
extender event handlers of the object let go, then the OnGrab blocks and `OnPlayerGrab`
extender event handlers of the one picked up (the player as the block action reference,
and the grabbed/released reference as `arg0` and calling reference for `OnPlayerGrab` /
`OnPlayerRelease`, plain and colon-qualified `:formType`) [documented: GECK wiki
OnPlayerGrab, OnPlayerRelease; hypothesis: handler-vs-block order, colon-qualified
`:formType` delivery, calling reference `this = grabbedRef`, and `IsPlayerGrabbedRef` /
`GetPlayerGrabbedRef` reading the updated grab state inside handlers]. Repeating the same
grab state is a no-op (zero double-fire), and when no `OnPlayerGrab` / `OnPlayerRelease`
handlers are registered the event path allocates nothing [design]. IsPlayerGrabbedRef
answers from it (0 at rest [observed: function probes, script-function-coverage.md]).
Not saved: a save made while holding loads with nothing held.

## 5. Log and smoke

`engine/nvf-play/src/grab.rs` logs `grab: picked up <loose object|corpse> <name> by
<GrabKey|Activate|Aid>: <mass> kg ...`, `grab: carrying ...` half a second in, `grab:
<dropped|threw> ... (<why>) at (...), N units from where it was picked up`, `grab: ...
came to rest at ...`, and `grab: the grabbed object's OnGrab ran` / `OnRelease ran`.

Test aid `NVF_PLAY_TEST_GRAB=<seconds>:<z|e>:<hold seconds>:<turn degrees>[:throw]`.
Smoke scenario `grab-skull`: in Doc Mitchell's house next to the brahmin skull (on a
wall shelf above the eye; the aid aims at its centre), Z held 2 s with the view still
(picked up, 5.0 kg, held within a few units of where it is wanted, put back, at rest on
the shelf), then E held 3 s while the view turns 90° (picked up after 0.4 s, carried
about 70 units, dropped, at rest on the floor some 220 units from the shelf); OnGrab and
OnRelease of `VCG01BrahminSkullSCRIPT` run each time. Passed 2026-10-21 (one launch).
The throw is covered by `fnv-phys` tests (`a_thrown_box_flies_off`), not the smoke.

## 6. Open

- The original's values of every constant above; whether E's hold time is a setting.
- `fZKeyMaxForce*` (a force limit falling with weight?), `fZKeyMaxContactMassRatio`
  (what a held object may push), `fZKeyComplexHelper*` (corpses?), `bGrabIK` (the arms
  reaching for the object; not built: no arm animation while holding).
- Whether a held object can be rotated, and whether the original throws on Use.
