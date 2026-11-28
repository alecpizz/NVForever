# Spec notes: combat rules (step C2) — damage, spread, attack cycle, projectiles

The rules of `crates/fnv-combat` for docs/design/combat.md step C2. Combat is not a
parity target (owner, 2026-10-09). Each rule is either **[documented]** (a GECK wiki
formula from the owner's exports, summarised in combat.md §4.2) or **[design]**
(NVForever's own, tunable). Record fields are as docs/spec-notes/combat-data.md describes
them.

## 1. Inputs (`tuning.rs`)

- `Rules`: the game settings the documented formulas name, read from the load order
  (`Rules::from_forms`). A setting the masters lack takes the wiki page's default
  [documented default; that an absent setting means its default is a hypothesis,
  combat-data.md §3]. `Rules::defaulted` lists which came from defaults.
  `fCombatDistance` has no documented default; 107 units is fitted to the original's
  NPC swings (§10, Reach) [observed fit; hypothesis that the setting is that value].
- `CombatTuning`: every [design] number below, in one place:

| Value | Default | Shapes |
|---|---|---|
| `dt_floor` | 0.2 | Armour never removes more than 80 % of a hit [observed: damage-armour.md §2] |
| `headshot_unaware_crit` | true | a headshot on an unaware target is a critical |
| `bloom_per_shot`, `bloom_per_min_spread`, `bloom_auto_mult` | 0.08 + 0.04 × Min Spread, × 0.5 for automatics | spread added per shot (spread units) |
| `bloom_max`, `bloom_recovery` | 1.0 (about 6° of deviation, 3° of cone), 2.0 /s | bloom cap; how fast it shrinks once settling |
| `settle_delay` | 0.3 s | rest beyond the weapon's own time between shots before bloom shrinks; closer shots build on each other |
| `jump_spread` | 0.3 | actor-spread penalty while airborne |
| `recoil_base_deg`, `recoil_per_damage_deg`, `recoil_auto_mult` | 0.6°, 0.04°/damage, × 0.5 for automatics | view kick up per shot |
| `recoil_jitter`, `recoil_side_share`, `recoil_lean_flip` | ±25 %, up to 0.4 × the kick up, 30 % per shot | the climb's random pattern |
| `melee_window`, `melee_rays`, `melee_arc_deg` | 0.1 s, 5, 35° | melee swing |
| `explosion_edge` | 0.25 | blast damage at the edge of the radius |
| `default_bounce` | 0.3 | lobbers whose PROJ has no bounce value |
| `auto_reload`, `fallback_reload` | true, 2 s | empty-trigger reload; reload without a clip length |
| `stagger_cooldown` | 3 s | after a hit reaction ends, how long hits on a crippled part start no other (§12) |
| `guard_break` | 1 s | how long the player cannot block after a power attack broke its block (§12) |

The bloom values were lowered after the first `combat sim` runs: at 0.15 × Min Spread
with a cap of 1.0, the 10mm SMG hit 2 of 60 shots at 1000 units from the hip [observed
with `fnvtool combat sim`]. That went too far: with 0.03 × Min Spread recovering at
1.0 /s from the first frame, a pistol's bloom was gone before its next shot and the
crosshair never opened, and the 1° kick was back in under 0.1 s (owner, 2026-10-24).
The values above (2026-10-24) let a run of shots build, shrink only after a rest, and
keep automatics at half the per-shot growth and kick, so the SMG's long-range hip fire
stays poor but not hopeless.

## 2. Damage (`damage.rs`)

For one projectile (or one swing) that hits [documented unless marked]:

1. **Base.**
   - Guns: weapon damage × `fDamageWeaponMult` × condition × skill bonus ÷ projectiles
     per shot. The skill bonus is `fDamageSkillBase` + `fDamageSkillMult` × skill / 100.
   - Melee: weapon damage × multiplier × condition × skill bonus + MeleeDamage
     (Strength × `fAVDMeleeDamageStrengthMult` + offset) + the crippled-arm term.
   - Unarmed: weapon damage × multiplier × condition + UnarmedDamage
     (`fAVDUnarmedDamageBase` + `fAVDUnarmedDamageMult` × Unarmed) + the crippled-arm
     term.
   - The crippled-arm term is weapon damage × (`fDamageArmConditionBase` − 1 +
     `fDamageArmConditionMult` × arm condition) / 2.
   - The condition multiplier is the original's hard-coded one: full damage at and above
     75 %, below it `1 − (0.75 − condition) × 0.67` [documented: GECK wiki
     SetConditionDamagePenalty; observed: §17]. The page's `GunConditionPenalty`
     settings are unused in New Vegas [documented: Weapon Damage Formula].
2. **Miss.** A base of 0 is a miss: no critical, no damage.
3. **Modifiers.** Ammo Damage Mod effects (add, multiply, subtract, in list order), then
   the "Calculate Weapon Damage" perk hook, then × (1 + Companion Nerve) when the
   attacker is one of the player's teammates: 0.05 per point of the player's Charisma
   [documented: Fallout Wiki, Charisma (Fallout: New Vegas), "+5% to their damage and
   armor"; hypothesis: the place in the order] (`Attacker::nerve`,
   `Game::companion_nerve`).
4. **Sneak attacks.** × `fCombatDamageBonusSneakingMult` for guns,
   × `fCombatDamageBonusMeleeSneakingMult` for melee and unarmed.
   - Only the player makes sneak attacks [documented: GECK wiki Weapon Damage Formula,
     "only the player in … vanilla New Vegas"].
   - A hit is a sneak attack when the player was sneaking as it was made (a swing as
     it lands, a shot or throw as it left) and the target does not have the player in
     its Detected band when it lands (`Game::detection`; a pair not measured counts as
     unseen) [owner statement 2026-10-25: "all unseen hits from a sneaking player are
     criticals"; hypothesis: the band is judged as the hit lands, and the sneak taken
     from when the shot left].
   - Explosions are not sneak attacks [hypothesis].
5. **Body part.** × the part's damage multiplier [design: placed after the documented
   steps].
6. **Critical chance.**
   - CritChance = `fAVDCritLuckBase` + `fAVDCritLuckMult` × Luck + bonuses.
   - Times the weapon's crit % multiplier, divided by the fire rate for automatic
     weapons.
   - Through the "Calculate My Critical Hit Chance" hook, + `fVATSCriticalChanceBonus`
     in VATS, then the target's "Modify Enemy Critical Hit Chance" hook.
   - The hit is a critical if a roll in [0, 100) falls under the chance, if it is a
     sneak attack (§4) [owner statement 2026-10-25], or if it is a headshot on an
     unaware target [design]. A weapon with crit % multiplier 0 never crits, sneak
     attacks included [documented: GECK wiki Weapons, Crit % Mult: "will not cause any
     critical hits, even for sneak attacks"].
   - Critical damage (through "Calculate My Critical Hit Damage") is added before
     armour and before the ammunition's Damage Mod, which multiplies it with the rest;
     each projectile of a shotgun adds the whole of it [observed 2026-11-21:
     damage-armour.md §3].
7. **Armour.**
   - DT comes from the target, then ammo DT Mod effects, then the attacker's and
     defender's DT perk hooks. A weapon with "Ignores Normal Weapon Resistance" skips
     DT and DR [design: the page says the flag did not work in the original].
   - DR (after ammo DR Mod effects, capped at `fMaxArmorRating`) takes its percentage
     of what DT leaves, for every damage type; a weapon's own resistance (§ the damage
     type, weapon-data-fields.md §3) cuts it as well.
   - The hit never deals less than `dt_floor` of its damage and critical bonus before
     the ammunition's Damage Mod, whatever DT and DR took.
   - The body part's multiplier comes last, on what armour let through.
   - These four in this order are the original's [observed 2026-11-21:
     damage-armour.md §1, §2 and §4].
   - A teammate's DT and DR are × (1 + Companion Nerve) before the ammo effects and
     perks [documented: as step 3, "armor"; hypothesis: that armour means both, and
     the order] (`Target::nerve`). Its energy and fire resistances are not changed.
8. **Child flag.** A target whose race has the Child flag takes nothing.

Also in `damage.rs`:
- Weapon wear per attack moved to `condition.rs` and is now the masters' setting (§17).
- Knockdown chance for explosions [documented: Explosion Knockdown Formula, with both
  health thresholds].
- Melee knockback force [documented: Melee Knockback Formula].
- Blast damage falloff [design].

Perks: the `PerkHooks` trait has one method per combat entry point (weapon damage,
critical chance and damage, enemy critical chance, DT attacker/defender, gun spread),
each the identity by default. Reading PERK records is a later step.

## 3. Spread, bloom, recoil (`spread.rs`)

- **Actor spread** [documented: Gun Spread Formula (New Vegas)]:
  - Sum the penalties: not aiming, walking or running (÷ the "Modify Aiming Move
    Speed" divisor), standing rather than crouching, crippled arms by one- or
    two-handed weapon, and unmet Strength and skill requirements. While airborne,
    add `jump_spread` [design].
  - Multiply by 1 − `fWobbleToSkillConversion` × skill / 100, and keep at least
    `fMinGunSpreadValue`.
  - The page reports the original applies the Strength setting to the skill shortfall,
    a bug. We use the skill setting.
- **Degrees** [documented as the page authors' in-game measurement]:
  (actor + 0.0125·W² + 0.125·W) × 5.955, where W is Min Spread. Ammo Spread Mod effects
  multiply the result.
- **Cone**:
  - The degrees are the full width of the circle the shots fall in: the authors measured
    "the diameter of the sway circle" for the actor spread and fitted "the diameters of
    those circles" of shot impacts for the weapon spread [documented: the page's Testing
    Methods]. So the cone's half-angle is half the degrees (`spread::half_angle`; until
    2026-10-24 the whole figure was used as the half-angle, which made every gun's
    spread twice as wide as measured).
  - The shot leaves inside a cone of that half-angle around the view direction, uniform
    over the cone's disc, so the crosshair is always the centre [design].
  - Bloom adds to the actor spread per shot, up to its cap. It recovers only after the
    weapon has rested for its own time between shots plus `settle_delay`, so fire as
    fast as the weapon allows keeps widening the cone, whatever its rate, and a short
    pause lets it close again. (A bare 0.35 s delay reset the 10mm Pistol, about 0.5 s
    between shots, after every shot [observed: smoke run 2026-10-24].)
  - Recoil kicks the view up per shot (more for harder-hitting weapons, ±`recoil_jitter`)
    and sideways by up to `recoil_side_share` of that. The sideways part leans to one
    side for a few shots and swaps with `recoil_lean_flip` per shot, so each burst
    climbs in a different pattern. Nothing brings the view back: pulling it down is the
    player's (owner, 2026-10-24; a 70 % automatic return was tried first).
  - VATS mode's shots, throws and the `NVF_PLAY_TEST_NO_SPREAD` test aid's shots kick
    nothing and grow no bloom. The recoil's rolls have their own random stream.

## 4. Attack cycle (`attack.rs`)

- **Interval.**
  - Automatic weapons: 1 ÷ `fire_rate`.
  - Semi-automatic weapons: the time of the attack clip's `a:` key ÷ (attack multiplier
    × animation multiplier) when the engine passes the clip (the clip's length when it
    has no `a:` key), else 1 ÷ "Attack Shots/Sec" [documented: GECK wiki Weapons].
    The `a:` key, not the clip's end [observed 2026-10-19, the base meshes archive and
    FalloutNV.esm]: the record's Attack Shots/Sec is exactly speed ÷ that key's time.

    | weapon | clip | `a:` key | end | speed | Attack Shots/Sec |
    |---|---|---|---|---|---|
    | 9mm pistol | `1hpattackright.kf` | 0.400 | 0.500 | 1.25 | 3.125 (1.25 ÷ 0.400) |
    | 10mm pistol | `1hpattackright.kf` | 0.400 | 0.500 | 1.1 | 2.75 (1.1 ÷ 0.400) |
    | varmint rifle | `2hrattack8.kf` | 1.267 | 1.333 | 1.4 × 1.1 | 1.216 (1.54 ÷ 1.267) |

    Timing shots by the clip's end made them 5–20 % slower than the original (owner
    playtest 2026-10-19). NPCs (and creatures' body guns) take the same rule from the
    standing attack clip in their own rig once it is known, 1 ÷ Attack Shots/Sec until
    then (`attack::npc_clip_timing`, §4.1, #2739). Where the clip has an `a:` key this
    gives the record's rate, as in the observed table above; a clip without one gives
    the slower rate of its end, and a creature's own clip can differ from the record
    (the sentry bot's launcher).
- **When the shot leaves.** At the attack clip's `Hit` key [documented:
  NiTextKeyExtraData]. Automatic weapons fire at once, except those that spin up.
- **Spin-up** [owner decision 2026-10-10]. An automatic weapon whose attack clip has
  an internal loop (`Fire` and `Loop` keys: a minigun's `AttackSpin`) fires its first
  shot at the clip's `Fire` key [documented: NiTextKeyExtraData, `Fire` works like
  `Hit`], `spin_up` seconds after the pull, then at its rate while the trigger stays
  held. Letting go during the spin-up fires nothing; every new pull, and firing on
  after a reload, spins up again [design]. `Event::SpinUp` marks the start, for the
  body's clip (docs/spec-notes/animation.md §9.8).
- **Reload.** Takes the reload clip's length; the animation multiplier does not speed
  it up [documented]. Without a clip length, `fallback_reload`.
  - **Reload speed** (`attack::reload_rate`, `Timing::with_reload_rate`). The player's
    reload clips play at 0.5 + 0.1 × Agility (1 at Agility 5, 0.6 at 1, 1.5 at 10)
    times the "Reload Speed" entry point of the player's perks with the weapon's tab
    (Rapid Reload multiplies by 1.25), and the reload, or each part of a round-by-round
    one, takes 1 ÷ that as long [documented: Fallout Wiki, Agility (Fallout: New Vegas):
    the weapon's reload time ÷ ((0.5 + Agility × 0.1) × Rapid Reload); observed: Rapid
    Reload's entry, Multiply Value 1.25, as its description's "25% faster"]. Agility is
    the current value within 1 to 10 [hypothesis: current, as the other derived
    statistics]. The body sets the reload layer's rate (`WeaponAnim::reload_rate`), so
    the clip and the cycle end together. Logged `combat: reloads at x<rate> (Agility
    <n>)` when the rate changes. The page also has Agility change draw and holster
    times; not built (Equip Speed, perk-entry-points.md). NPCs reload at their clips'
    own speed.
- **Round by round** (`Timing::rounds`, `RoundReload`). Some reloads load one round at
  a time and can be stopped:
  - The clips [observed: the base meshes archive, 2026-10-11]. Three reload
    animations have an opening clip beside the reload clip, in both the first-person and
    the third-person folders (with sneak forms): `ReloadX` (`1hp` and `2hr`), `ReloadY`
    and `ReloadZ` (`2hr`). No other reload has one.

    | clip | length | text keys (s) |
    |---|---|---|
    | `1hpreloadxstart.kf` | 1.067 | sound `WPN357RevolverReloadStart` 0, `WPN357RevolverReloadClick` 0.667 |
    | `1hpreloadx.kf` | 1.767 | sound `WPN357RevolverReloadLoop` 0, `hit` 0.633, `WPN357RevolverReloadClick` 0.917, `WPN357RevolverCock` 1.467 |
    | `2hrreloadxstart.kf` | 0.933 | sound `WPNLeverActShotgunReloadStart` 0.033 |
    | `2hrreloadx.kf` | 1.900 | sound `WPNLeverActShotgunReload` 0.033, `Hit` 0.5, `WPNLeverActShotgunReloadEnd` 0.667 |
    | `2hrreloadystart.kf` | 0.667 | sound `WPNHuntingSGReloadStart` 0.015 |
    | `2hrreloady.kf` | 1.533 | sound `WPNHuntingSGReload` 0.097, `Hit` 0.467, `WPNHuntingSGReloadEnd` 0.502 |
    | `2hrreloadzstart.kf` | 0.333 | sound `WPNCowboyReloadStart` 0.033 |
    | `2hrreloadz.kf` | 1.667 | sound `WPNCowboyReload` 0.033, `Hit` 0.5, `WPNCowboyReloadEnd` 0.6 |

    (First-person files; the third-person ones have the same lengths, `2hrreloadx.kf`
    its end sound at 0.533, and two openings a `Blend:` key.)
  - The weapons [observed: FalloutNV.esm WEAP reload animation, 2026-10-11]: `ReloadX`
    the .357 Magnum Revolver, Lucky and the Lever-Action Shotgun; `ReloadY` the Hunting
    Shotgun, Dinner Bell and the Grenade Launcher; `ReloadZ` the Cowboy Repeater, the
    Brush Gun and the Trail Carbine. The owner's list (the hunting shotgun, the
    lever-actions, the cowboy repeater, the brush gun, the trail carbine and the .357)
    agrees; the Grenade Launcher and the two uniques come with their animations. No
    list of weapons is kept: the clips decide.
  - The rule [hypothesis: the reload clip's `Hit` "also ends looping reloads",
    documented: GECK wiki NiTextKeyExtraData; the sounds after `Hit` are the
    `…ReloadEnd`, click and cock sounds]: the opening clip plays once; the reload
    clip from its start to `Hit` loads one round, and repeats while rounds are missing
    and carried; after the last round the clip plays on from `Hit` to its end, which
    closes the weapon. The close plays every time [owner decision 2026-10-11].
  - Stopping: a fresh pull of the trigger stops it after the round in hand and the
    weapon closes [owner decision 2026-10-11]. That pull fires nothing (owner,
    playtest of 2026-10-11, replacing "closes first, then fires" of the same morning).
    A later pull while it is stopped or closing is kept for 0.25 s
    (`fnv_combat::attack::SHOT_GRACE`) [design: the owner asked for a grace period; the
    length is chosen]: closed within that time of the pull, the shot follows without
    another pull; else the pull is dropped. A pull while it opens still lets the first
    round go in [design]. The pull that starts a reload on an empty magazine does not
    stop it.
  - Events: `ReloadStarted` (the opening clip), `ReloadRound` (the reload clip from its
    start), `RoundLoaded`, `ReloadClosing { stopped }`, `Reloaded { taken }`.
  - **Checked** [observed 2026-10-11, `nvforever --cell GSHouseInterior03 --play`, exit
    0, "run finished"]: the .357 revolver with 2 missing opened, loaded one round
    0.63 s after the other and closed (`reloading round by round (opens 1.07 s, 0.63 s
    a round, closes 1.13 s)`, `a round in, magazine 5`, `… 6`, `closing`, `reloaded 2`);
    with 3 missing and the trigger pulled with the second round in hand, that round
    went in, it closed (`closing (the trigger was pulled)`) and fired 1.2 s later,
    magazine 3 → 5 → 4. The Cowboy Repeater (0.33 / 0.50 / 1.17 s) and, in third
    person, the Hunting Shotgun (0.67 / 0.47 / 1.07 s) loaded the same way. Screenshots
    during the revolver's reload show the cylinder out, the left hand at it, then
    closed.
- **Trigger.** A semi-automatic weapon needs a fresh pull per shot; an automatic one
  fires while the trigger is held.
- **Empty magazine.** Pulling the trigger on an empty magazine reloads when ammo is
  carried (`auto_reload` [design]), else gives one dry click per pull. A magazine
  emptied after the pull, during the spin-up or wind-up, fires nothing: that pull is
  answered the same way, and its shot is not counted [design: `attack.rs`, #1613]. A
  free fourth shot (a "Regenerate Ammo (shots)" mod) needs no round and still leaves.
- **Ammo.** A shot takes `ammo_use` rounds.
- **Regenerating weapons** gain a round every `regen_rate` seconds and cannot be
  reloaded by hand [documented].
- **No-ammo weapons.** Melee and unarmed weapons cycle without ammo.

### 4.1 NPCs' shot timing and body guns' magazines (#2739)

The owner saw sentry bots fire missiles back to back in NVForever and with a pause
between them in the original [observed: owner playtest, #2739].

- **The records** [documented: fnvtool esm decode, FalloutNV.esm]:
  `SentryBotMissileLauncher2HL` (WEAP 0x00057E8F) is an Embedded Weapon, not automatic,
  Clip Size 1, ammo AmmoMissile, Attack Shots/Sec 1.5789, Fire Rate 1.0, attack and
  animation multipliers 1.0, Animation Type Launcher (2 Hand), Attack Animation
  AttackLeft, Reload Animation ReloadB, Semi-Automatic Fire Delay 0.1-0.5 s, Min/Max Range
  500/2000, Reload Time 2.3333. Its creatures (`CrSentryBotGLHard` 0x000E585D with the
  laser gatling, `CrSentryBotMGArmy` 0x000A5C6F with the minigun) fight with combat style
  `CSSentrybot` (Take Cover Chance 0, Wait-to-Fire Timer 10/10, Fire Timer 2/2,
  Semi-Auto Firing Delay Mult 1/1).
- **The clips** [documented: fnvtool bsa list / nif decode, Fallout - Meshes.bsa, main
  session 2026-11-16]: `meshes\creatures\sentrybot\` has 60 `.kf` files, among them
  `2hlattackleft.kf` (sequence AttackLeft, 0 to 2.0 s, clamped; keys `BlendIn:1` 0.033,
  `Hit` 0.067, `a:R` 0.967, end 2.0), its up and down forms, `2hlaim`, `2hlequip`,
  `2hlunequip`, `2hlholster` and `2hljamb.kf`, but **no reload clip**. Of all creature
  folders only `nvsecuritron`, `protectron` and `smspinebreaker` have a reload `.kf`.
- **What NVForever did** [design: `engine/nvforever/src/npc_combat.rs` on be6db0948]:
  an NPC's semi-automatic gun fired every 1 ÷ Attack Shots/Sec, its clip never asked
  (the launcher: 0.63 s, shorter than both its clip's `a:R` and its end); a creature's
  embedded gun had no magazine, so it never reloaded. The other pauses in the data were
  applied, or do not apply:
  - the weapon's Semi-Automatic Fire Delay (times the style's multipliers) follows every
    NPC shot, embedded guns included; the launcher's 0.1-0.5 s is shorter than its
    interval, so it adds nothing;
  - the style's Wait / Wait-to-Fire / Fire timers are cover timers ("how long to stand out
    of cover looking for targets to appear", "how long to stay out of cover firing at
    targets once they are seen" [documented: GECK wiki Combat Style]); NVForever applies
    them while a fighter holds cover, and CSSentrybot's Take Cover Chance 0 never takes
    cover;
  - the weapon choice (`fnv_combat::body_weapon`, combat-ai.md §19) switches by distance
    and crippled limbs, at most every 1.5 s, never on a timer: with the launcher the only
    gun that suits (the target past the other gun's reach and beyond the 1000-unit blast,
    or the other gun's limb crippled), it fires nothing else;
  - the WEAP's Reload Time is "Unused" [documented: GECK wiki Weapons]; it is no source.
- **Shot timing** (`fnv_combat::attack::npc_clip_timing`) [design, #2739]: an NPC's or a
  creature's semi-automatic gun is timed by the standing attack clip of its WEAP Attack
  Animation in its own rig (`attack_anims`, with the reload clips; one handoff,
  `AttackTimes::gun_timing`, for a fight's gunmen and for Use Weapon shooters, whose
  volley pauses come on top), by §4's rule for the player: the `a:` key's time (the clip's length without one) ÷ (attack multiplier ×
  animation multiplier). Until the clip is known, and without one, 1 ÷ Attack Shots/Sec as
  before. Automatic weapons keep their Fire Rate. The `Hit` key gives no wind-up: the
  NPC's clip starts with its shot. Sources: "Attack Shots/sec ... is calculated on the Art
  and Sound tab" of the weapon form [documented: GECK wiki FWeaponConditionRateOfFireXX],
  i.e. from an attack animation; the `a:` key "determines when the animation can be
  interrupted to play the next animation" [documented: GECK wiki NiTextKeyExtraData];
  "Attack Multiplier ... will also increase or decrease the projectile ejection rate"
  [documented: GECK wiki Weapons].
  - The sentry bot's launcher now fires every 0.967 s (its `a:R`), not 0.63 s. That is
    still no barrage-length pause: by the player's rule the clip's end (2.0 s) is not
    waited for. The original switches to its gun after each missile [observed
    2026-11-25, observer check 221]: the Hardened Sentry Bot fires one missile per
    launcher turn of about 4.6 s, then about 5.7 s of gatling bursts, a missile every
    9.9-10.4 s at 1,100-1,250 units; the Army one's minigun turns last 6-18 s; neither
    spends the missiles in its inventory.
  - The wiki also says Semi-Automatic Fire Delay Max "determines semi-auto weapon's fire
    rate when used by actors" [documented: GECK wiki Weapons]; NVForever waits that delay
    after each shot besides the cycle's interval (npc_combat module notes).
- **Magazine** (`fnv_combat::body_weapon::has_magazine`) [design, #2739]: an embedded gun
  has a magazine and reloads by the people's rule (as soon as it is empty, for as long as
  its reload clip runs) once its creature's rig is known to have the weapon's reload clip
  (`<stance prefix>reload<x>.kf` of its Reload Animation in the creature's folder or its
  `specialanims`). A creature without that clip fires on without a magazine as before:
  the 2.0 s fallback stands in for clips not yet known. This changes only the creatures
  whose folders have such a clip (Securitrons, protectrons, the spinebreaker skeleton's
  super mutants and nightkin, where the names match); the sentry bot has none, so its
  launcher keeps firing without reloads. Whether the original pauses a creature that has
  no reload animation is observer check 221 [hypothesis].
- **Ammunition**: rounds still come from nowhere [design: owner 2026-10-17, "they dont
  use ammo"]. The WEAP flag NPCs Use Ammo ("Actors will use up the associated ammo in
  their inventory like the player does. (This setting is disabled by default.)"
  [documented: GECK wiki Weapons]) is not read; the launcher's flags as decoded for #2739
  are Embedded Weapon, Can't Drop and Non-Playable [documented: fnvtool esm decode,
  FalloutNV.esm], so the original's sentry bot is taken not to spend its AmmoMissile
  either [hypothesis].

## 5. Projectiles, swings, blasts (`projectile.rs`, `shot.rs`)

- **The world.** `CombatWorld` is the engine's side: `ray(from, to, ignore)` and
  `actors_near(point, radius)`.
- **A shot.** `shot::fire` spawns the chain's projectiles per shot, each with its own
  direction in the cone.
- **Where an arcing round leaves** (issue #1471). The player's shots leave the eye along
  the view, except an arcing explosive missile (a PROJ of type Missile with gravity and
  an explosion: the grenade launchers' rounds), which leaves the weapon's muzzle along the
  view when nothing stands between the eye and the muzzle. The original's 40 mm round
  started 110 / 125 / 138 units above the feet and 51-55 units ahead at aim 0 / 15 / 30
  degrees up [observed: o26_gl, 2026-11-06, the first positions of its projectile
  reference]; our Grenade Launcher's muzzle is 110 / 126 / 142 above the feet and 60-64
  ahead [observed: smoke `grenade-launcher-arc`, debug log], while the eye stays about
  118 above. From the eye ours first hit the ground 840 / 1,841 / 2,763 units out after
  0.48 / 1.09 / 1.82 s, against the original's 793 / 1,944 / 2,815 after 0.45 / 1.16 /
  1.86 s: speed (1,750) and gravity (1.5 × 686.7) were the same, the launch point was not
  [observed]. From the muzzle ours lands 812 / 1,864 / 2,801 units out after 0.46 / 1.10 /
  1.85 s [observed: smoke `grenade-launcher-arc`, no spread]. The rest of the gap at 15
  degrees is the launch angle: the original's left 1.2 degrees above the aim there (0.3
  below at 0, 0.3 above at 30) [observed]; ours leaves along the aim [hypothesis: the
  original's small offsets are its aim spread]. Thrown weapons (lobbers) still leave the eye: the original's frag
  grenade started 118 above the feet and 52 ahead [observed: o26_gl frag0], as high as our
  eye.
- **Flight.**
  - Beams and hitscan projectiles trace at once. The hit is delivered after distance ÷
    speed; beams deliver at once.
  - Missiles and flames are stepped with swept rays under gravity, up to their range.
    A flame goes on through actors, hitting each once.
  - Projectiles flagged to pass small-transparent colliders go through them.
  - Gravity is the PROJ value × the world's 9.81 m/s² at 70 units per metre
    [observed for a missile: o26_gl below].
  - **Measured in the original (o26_gl, 2026-11-06, `nvfairport`, the runway, first
    person; the newest live projectile's position every game loop by `GetFirstRef 51`,
    times from GameHour at TimeScale 30; issue #1471).** The Grenade Launcher's
    `40mmGrenLaunchProjectile` (Missile, speed 1,750, gravity 1.5) flew at **1,750
    units/s** (1,750-1,753 in all four shots) under **1,030-1,039 units/s²**, that is 1.5
    × 686.7 [observed]: the PROJ speed and the gravity rule above, as built. The shots,
    from 110-140 units above the ground and 55 units ahead of the player:

    | Aim (GetAngle X) | Launch angle | First ground hit | Flight |
    |---|---|---|---|
    | 0 (attack key held 4 loops) | -0.3° | 793 units | 0.45 s |
    | -15 | 16.2° | 1,944 units | 1.16 s |
    | -30 | 30.3° | 2,815 units | 1.86 s |
    | 0, `player.FireWeapon` | 0.3° | 821 units | 0.47 s |

    The round went off at its first ground hit; it did not bounce or roll. The launch
    angle was 1.2° above the aim at -15 and 0.3° at -30 [observed]. So speed and gravity
    do not make NVForever's round fall short; the launch point did, and ours now leaves
    the muzzle (above, PR #1788).
  - A thrown frag grenade (`GrenadeFragProjectile`, Lobber, speed 1,200, gravity 1.0;
    the attack held 30 loops, aimed level; one run, o26_gl frag0): its figures are under
    "A thrown grenade rolls" and "Throw speed" below.
  - **A Lobber whose gravity is 0 falls under the full gravity** (#3441) [hypothesis;
    observer check 289]. The load order's Lobbers with gravity 0 [observed: PROJ DATA
    of the ten masters, 2026-11-23] are:
    - `NVDLC05GrenadeNukaProjectile`, `NVDLC05MFCGrenadeProjectile` and
      `NVDLC05MFCMineClusterProjectile2` (GunRunnersArsenal.esm);
    - `MineBottleCapProjectile`, `MinePlasmaProjectile` and `MinePulseProjectile`
      (FalloutNV.esm).

    The other 33 have 1.0, except 1.5 for `NVDLC0525mmGrenadeProjectileTimed`; the frag
    mine has 1.0. With the field taken only as a multiplier, a thrown Nuka-grenade flew
    dead straight and had unlimited reach (player report, #3441). So a Lobber of 0 now
    takes a multiplier of 1.0 (`ProjectileStats::fall`, the same rule as Bouncy Mult's
    0). Its flight, its reach in VATS, an NPC's throw choice and the aim of the low arc
    all use it. A Missile of 0 still flies straight.
- **Explosions.**
  - Without Alt. Trigger, the explosion goes off on impact.
  - With it, a lobber bounces (× its bouncy multiplier, else `default_bounce`) and
    settles; a thrown grenade (a fuse, no proximity, not Detonates) rolls instead, below. It explodes when its timer runs out, or, when it has a proximity radius,
    once armed (after the timer) when an actor other than the thrower comes within
    the radius.
  - **A thrown grenade rolls** (issue #1785; owner 2026-11-06: only thrown grenades
    roll, launcher rounds go off where they first land). At a contact the speed into the
    surface keeps `fGrenadeRestitution` of itself, out of it (0.01 in FalloutNV.esm
    [observed]; its meaning [hypothesis: from its name]), the speed along it keeps
    `grenade_roll_keep` (0.6) at the first landing, and the grenade goes on along the
    surface; on ground (a normal at least 0.7 up) it rolls, losing 140 units/s every
    second (`grenade_roll_friction`), until it is slower than 40 units/s or its fuse
    ends. Both are fitted to the original's frag grenade [observed: o32_throw,
    2026-11-25/26, nine throws on the runway, FireWeapon taps and holds of 5-120 loops,
    positions every game loop; observer follow-up 2026-11-28 on #1785]:
    - the share of the ground speed kept at touchdown differs by throw, 0.33-0.36 for the
      FireWeapon taps, about 0.6 for 5-10 loops, 0.48 and 0.71 for two 30-loop throws,
      0.8-1.0 at 60-120 loops (median 0.61); after it one hop of 13-69 units, then 1-3
      small ones [hypothesis: the share depends on how the grenade hits, not only on its
      speed];
    - the slow rolls come to a full stop (the taps at 502 and 588 units from the hand);
      one constant friction fits each roll within a few units, 117-178 units/s² below
      600 units/s of roll and 279-418 above 850;
    - every grenade goes off 2.46-2.48 s after it leaves the hand, where it has got to:
      5 loops 745 units out, 10 loops 969, 30 loops 1,267-1,313, 60 loops 1,844, 120
      loops 1,705, 30 with Heave Ho! 1,921.

    With 0.6 and 140 the distances at 1.0, 1.5 and 2.0 s and at the fuse come within
    about 50 units of the 5-, 10-, 30- and 120-loop throws and Heave Ho!'s [observed: a
    fit over the nine throws' figures, rms 124 units against 157 for the 0.9 and e^(−0.66 t)
    fitted before to o26 frag0, whose speeds were misread]; the taps, which kept only a
    third, roll about 150 units too far, and the 60-loop throw, which skimmed on at its
    full speed, about 400 short (smoke `frag-grenade-roll`, `frag-grenade-tap`). `fGrenadeFriction` (5.0 in FalloutNV.esm) is
    not used [hypothesis: a Havok friction; our grenade is no rigid body]. The grenade
    model's own body (`GrenadeFrag.NIF` bhkRigidBody: mass 5, friction 0.5, restitution
    0.4, linear damping 0.1, angular damping 0.05) is not used either [observed values].
  - **Throw speed: by how long attack is held** (issue #1785). In the original a thrown
    grenade leaves faster the longer attack is held, up to its PROJ speed [observed:
    o32_throw, 2026-11-25/26, the Searchlight Airport runway, aimed level, the game window
    focused, `HoldControl 4` for N game loops then `ReleaseControl 4`; frag grenade (PROJ
    1,200): 5 loops 627 units/s across the ground, 10 loops 715, 30 loops 912-952, 60 loops
    about 1,180-1,200, 120 loops about 1,170; a `FireWeapon` throw with no hold 554-601;
    the Nuka-grenade at 60 loops 1,239]. No throw slows in the air, and every one falls
    under the full gravity (690-700 units/s² where the fit is clean). Heave Ho! multiplies
    the speed by 1.5 on top (1,430 against 952 at 30 loops). o26 frag0's figures written
    here before (about 950 slowing to 650-770, a fall of about 570) were misread from the
    floor clock: the same 30-loop throw now holds 912-952 to touchdown at 513-514 units.
    Ours: the player's grenade (a `Grenade Throw (1 Hand)` weapon whose projectile
    explodes) leaves at its PROJ speed × (0.5 + 0.5 × the share of its wind-up, attack
    pressed to the clip's `Hit` key, that attack was held), the whole speed when attack is
    still held as it leaves, then times the throwing perks (`combat::throw_hold_share`)
    [hypothesis: the share over our clip's 0.87 s wind-up fits the figures (0.55, 0.60,
    0.79 and 1.0 at 5, 10, 30 and 60 loops, against 0.52, 0.60, 0.78 and 0.99) as well as
    a rise over 1 s; that mines, thrown weapons and NPCs' throws keep their PROJ speed; that
    VATS mode's throws do]. Smoke `frag-grenade-roll` (held 0.5 s: 940 units/s, down at
    553 units after 0.59 s) and `frag-grenade-tap` (0.1 s: 668 units/s).
  - Detonating projectiles wait for `detonate()` (C-4).
  - Anything else ends after 60 s [design].
- **Melee swing.** A fan of `melee_rays` rays over ±`melee_arc_deg` at the eye, as
  long as the reach. Each actor counts once [design]. The reach is `fCombatDistance` ×
  scale × weapon reach [documented formula; placeholder setting].
- **Blast.** Every actor within the radius takes damage, unless a ray from the centre
  is blocked first and the EXPL does not ignore line of sight [documented]. Only the
  world blocks that ray (walls, ground, doors); another body, living or dead, does not,
  or the front of a pack shielded all behind it (`CombatWorld::cover_ray`) [design;
  owner's playtest 2026-10-23, a grenade into a mantis swarm killed half]. The damage
  falls off [design]; the push is away from the centre with the EXPL force. In play the
  player counts, at the middle of its capsule (design/combat.md §5.8).

## 6. CLI contract

Both commands take `<Data> <plugins…> --schema <dir> --form <weapon>` and the shooter
and target options:
- `--ammo`, `--skill` (50), `--strength` (5), `--luck` (5), `--unarmed` (50),
  `--condition` (1), `--sneaking`;
- `--dt`, `--dr`, `--part-mult` (1), `--head`, `--unaware`.

- `fnvtool combat damage` → `fnv.combat.damage/1`:
  - `hit` (the `Damage` breakdown without a critical) and `critical_hit`;
  - `per_shot` (all projectiles hitting);
  - `explosion` (damage, radius, force);
  - `settings_defaulted`.
- `fnvtool combat sim` → `fnv.combat.sim/1`. It also takes `--distance` (1000),
  `--radius` (30), `--health` (100), `--seconds` (10), `--aim`, `--crouch`,
  `--carried` (1000) and `--seed` (1).
  - The setup: the shooter's eye is at 100 units, aiming at a sphere at the same
    height, on a floor. Semi-automatic weapons are pulled every other tick and
    automatic ones held; explosions use the blast rule. Runs with the same seed
    repeat exactly.
  - Output: `interval`, `actor_spread`, `cone_deg_first_shot`, `shots`, `projectiles`,
    `hits`, `hit_rate`, `criticals`, `reloads`, `damage`, `time_to_kill`.

Examples [observed, seed 1, skill 50, 100 HP target]:

| Weapon | Setup | Hits | Time to kill |
|---|---|---|---|
| 9mm pistol | hip fire at 1000 units | 8 of 9 | 2.7 s |
| 9mm pistol | aimed and crouched | 9 of 9 | 2.7 s |
| sawed-off shotgun | at 400 units | 17 of 28 pellets | 2.5 s (with a reload) |
| 10mm SMG | hip fire at 1000 | 19 % | 6.6 s |
| 10mm SMG | aimed at 600 | 8 of 8 | 0.85 s |
| laser rifle | aimed, DT 12 | 12 of 12 | 3.7 s |
| hunting rifle | skill 75, aimed at 3000 units, DT 6 | 3 of 3 | 2.2 s |

## 7. Hit volumes (step C3)

- **The capsule** (`fnv_phys::actors`, `HitVolume`): one vertical capsule per placed
  actor in the physics world, kinematic, owner `Owner::Actor { form_id, layer, part }`
  (part 255 since C8; per-part capsules are §12).
  - Its layer is BIPED while the actor lives and DEAD_BIP once dead. The layers are the
    documented Havok names; using them for actors is our choice.
  - It takes part in no contacts: it pushes no clutter, and the character controller
    walks through it [design].
  - Setting an unchanged volume does nothing; the shape is rebuilt only when its size
    changes.
  - Since 2026-11-03 the capsules are colliders of no body, placed beside the kinematic
    body (not attached to it), which avoids the forced re-insertion and full refit an
    attached write caused; `NVF_PHYS_HITVOL_ATTACHED=1`
    attaches them again (physics.md §5.1) [design].
- **In play** (`engine/nvf-play/src/hitvol.rs`): every drawn, present ACHR/ACRE gets a
  volume each frame from its render bounds [design]:
  - the union of its drawn parts' world boxes;
  - feet at the bottom centre, height the box height;
  - radius the smaller horizontal half-extent, clamped to 10–150 units.

  An actor that is hidden, removed or no longer drawn loses its volume.
- **Rays.**
  - The crosshair's collision ray targets an actor when it hits its volume.
  - The floor probe, the safe-spot search and the third-person camera skip actor
    volumes (`layers::actor_volumes()`).
- **Checked.**
  - `fnv-phys` tests: shots hit the capsule, follow it when moved and see its layer
    change on death; a filter can skip it; a character walks through it; moving it
    through a dynamic box does not push the box.
  - In the engine: `nvforever --data <Data> --cell Goodsprings --play --run-seconds
    25` exits 0 with "run finished" and reports 13 actor volumes at exit (10 NPCs and
    15 creatures drawn, 12 of them hidden by the simulation). A level probe ray from
    150 units at each capsule's middle hit its own actor for 12 of the 13, and
    something else first for one [observed 2026-10-09].

## 8. The player shooting (step C4)

- **Weapon and ammo** (`engine/nvf-play/src/combat.rs`):
  - The equipped weapon comes from the simulation, with the ammo from
    `CombatForms::ammo_for` (the first carried entry of its list).
  - The magazine is a view of the carried rounds. A shot removes `ammo_use` rounds
    from the inventory; a reload moves nothing.
  - The magazine never holds more than is carried, checked every frame, holstered or
    not: rounds dropped, sold or taken by a script are gone from the gun too, also a
    magazine a script set above that with SetPlayerCurrentAmmoRounds [design:
    `combat.rs`, #1612]; the original may keep such a count, nobody has seen it
    [hypothesis]. Regenerating weapons and god mode's full magazine are left out: their
    rounds are not in the inventory.
  - A weapon with no ammo but a projectile (grenades) spends itself.
  - A newly equipped weapon starts loaded with what is carried [design].
- **Controls.** Use is the trigger; the press that draws the weapon does not fire. A
  Ready Item tap reloads. Ammo Swap takes the next carried ammo and empties the
  magazine; the old type's rounds stay in the inventory, unfired, until a swap comes back
  to them. What follows depends on the cycle [design: `Cycle::swap_ammo`, #1613]:
  - Ready: the reload starts at once.
  - Spin-up or wind-up: the shot on its way is dropped and the reload starts at once.
  - Between shots: the reload starts when the wait ends, with no pull, so a swap never
    brings the next shot forward. Holstered first, the wait and the reload freeze; on
    the redraw the wait ends and the reload starts, with no pull [design: the cycle
    steps only with the weapon out].
  - During a reload: it goes on, on its own clock, and takes the new ammo. Round by
    round, the rounds already in are out again and the rest are of the new type.
  - Closing after a round-by-round reload, or jammed: the magazine stays empty and the
    next pull reloads.
  - God mode: the magazine stays full and nothing reloads.
- **Readiness and clips.** The cycle runs once the player body reports the weapon drawn
  and in hand (`WeaponAnim::ready`; without a body, as soon as it is wanted out).
  - The body gives the attack clip's length and `Hit` key (and whether a `Loop`
    key follows it: the spin-up time) and the reload clip's length, from the rig in
    view. The attack clip is `{prefix}{attack}{is}.kf` from the
    WEAP attack animation; the reload clip is `{prefix}reload{x}.kf`
    (`fnv_actor::player_clips`).
  - The body plays the clip once each time combat counts an attack or a reload; for a
    round-by-round reload the opening clip (`{prefix}reload{x}start.kf`) on the reload
    count and the reload clip from its start on each round's count (§4).
- **Shots.**
  - They leave the eye along the view, inside the cone from §3. The stance is taken
    from the controls and the character: aiming, sneaking, moving, airborne.
  - They are drawn from the muzzle of the weapon in hand (the model's
    `ProjectileNode`, animation.md §9.9; `WeaponAnim::muzzle` from the player body),
    closing on the view ray by the point aimed at. In third person a shot whose
    muzzle has something between it and that point leaves the muzzle instead
    (`fnv_combat::shot::leaving`; docs/design/combat.md §5.1) [design]. The debug log
    gives each shot's eye, aim point, muzzle and where it is drawn from, and each
    tracer's start; the summary at exit counts the shots drawn from the muzzle and
    those from behind cover.
  - The skill, Strength and Luck are the player's actor values
    (docs/spec-notes/actor-values.md): the skill from its attribute and Luck by the
    documented formula, plus anything a script changed.
  - Recoil turns the view's pitch.
- **The world.** Projectiles fly against the physics world (`PhysView`):
  - rays skip the player's capsule and the layers the character passes;
  - actor hit volumes (§7) report the actor;
  - small-transparent colliders are the layers TRANSPARENT_SMALL and
    TRANSPARENT_SMALL_ANIM.
  - **The plain Transparent layer (3) lets every shot through** (issue #2340): the
    original's Transparent is a layer where "nothing can pass through except
    projectiles", and Transparent Small is the same except that only projectiles with
    the Pass Through Small Transparent flag pass it [documented: GECK wiki "Collision
    layer", TRANSPARENT and TRANSPARENT SMALL]. So `layers::shot_ignores` holds
    TRANSPARENT (characters still stop on it), and the small-transparent layers stay
    under the projectile flag above. The same filter serves the aim ray, VATS mode's line
    of fire and aim assist, so those pass layer 3 too. A melee swing's rays do not: the
    exception is for projectiles, so the player's blows (the fan, the reach across the
    ground, VATS mode's straight blow and the look ahead for an impact) take
    `layers::melee_ignores`, the character's layers and invisible walls as before, and
    stop on layer 3 (`PhysView::for_melee`). NPC blows land by reach and facing, with no
    ray through the world, so they are unchanged. Blast cover lines already passed layer 3,
    and so does sight (`detection.rs` `see_through`; the same wiki line: actors see
    through it). Whether the original lets a 9mm through a layer-3 chain link is observer
    check 130 [hypothesis until then: the wiki holds for mesh bodies].
  - **Reach of the change** [documented: `fnvtool collide objects` over the eleven mesh
    archives of the base game and its add-ons, `fnvtool world refs` over every worldspace
    and the interior cells of FalloutNV.esm with its nine add-on masters, 2026-11-14]:
    600 collision bodies in 579 meshes are on layer 3, all static. 6,570 placed references
    (6,531 statics, 22 doors, 17 static collections) of 271 of those models carry one,
    5,739 of them in interiors. The commonest are utility-room walls, corners and halls
    whose layer-3 body is a flat ceiling grate at 183 units, catwalk rails (`Rails01`),
    `trusswork01.nif` (all of it on layer 3), the lower gap of
    `bathroomstalldivider01.nif`, the Divide's wall lights and `bedframe01static.nif`.
    The road gate `clfencegateroad01.nif` (door `CLFenceGateRoad01`, 21 placed) has its
    chain link `CLFenceGateRoad01Link` on layer 3; its add-on copy
    `dlcanchclfencegateroad01.nif` has it on Transparent Small (26).
- **Hits.** Hits and blasts are logged with the damage the rules give (target DT 0, no
  body part); applying it is step C5.
- **Feedback** [design, until impact effects and audio]:
  - developer markers, drawn only when switched on (`--hit-markers`, settings page
    Developer → Shot markers; off by default): an impact marker for 0.3 s (0.6 s for
    explosions) and a line from the muzzle along every hitscan shot for 0.08 s, to the
    hit or to the end of the range (for other projectiles at the PROJ tracer chance);
    the muzzle flash and the projectiles' models are §15;
  - an ammo counter (magazine / carried, the ammo's abbreviation).
- **Test aids.** `NVF_PLAY_TEST_CONTROL` presses `fire`, `reload`, `swap` and
  `aimactor` (look at the nearest actor's hit volume). `NVF_PLAY_TEST_TRACERS=<seconds>`
  gives every projectile a line and keeps the lines and a marker at the muzzle that
  long, so one screenshot shows where shots were drawn from (muzzle flashes and
  projectile models are not held).
- **Checked** [observed 2026-10-09, `nvforever --cell Goodsprings --play`, exit 0, "run
  finished"]:
  - **9mm pistol** (50 rounds given): magazine 13/13, one shot per trigger pull. Two
    shots hit a creature 1244 units away for 9.2 each (16 × the skill bonus at Guns
    15). A reload took one round. The screenshot shows the first-person pistol and the
    counter `12 / 37  9mm`.
  - **Missile launcher:** the missile struck the ground about 158 units from the
    creature it was aimed at. The explosion caught it for 176 and another creature at
    501 units for 125 (before armour).

## 9. Damage applied (step C5)

- **Health** (`crates/fnv-game/src/combat.rs`). Health's base (actor value 16) is the
  actor's maximum health whenever no script has set it, so `GetAV Health` and
  `GetHealthPercentage` see combat.
  - NPC: Base Health + (Endurance − 1) × `fAVDNPCHealthEnduranceMult` (5), plus
    (level − 1) × `fAVDNPCHealthLevelMult` (5) for an NPC with "Auto-calc stats"; a
    hand-set NPC's level adds nothing [observed: the AI observer's captures, fifteen
    NPCs at full Health, all fit: auto-calculated the NCR troopers (level 5, Base 10,
    Endurance 5: 50), Fiends 65–105, Joe Cobb 65, Sunny Smiles 90, Ringo 120, settlers
    45, the Strip crowd 45, Violet 170; hand-set Cook-Cook 215 (level 14, Base 180,
    Endurance 8), Doc Mitchell 65, Trudy 75, the Geckoman 70, Ramos 25;
    npc-behaviour-compare.md §9]. Until 2026-10-19 this was Endurance × 5 + level × 5
    for every NPC, 10 more for most and 75 more for Cook-Cook.
  - Creature: its DATA Health.
  - Player: `fAVDHealthEnduranceOffset` + Endurance × 20 + (level − 1) × 5 + Base
    Health [hypothesis until captured].
  - The settings' names and values are observed (combat-data.md §3).
  - Level comes from ACBS, or for "PC Level Mult" the player's level × the multiplier
    within the calculated bounds [documented: Stats Tab pages].
  - A template with the "Stats" flag supplies the stats.
- **A hit** (`Game::hit`):
  - Health's damage part grows (as `DamageAV`).
  - The last hit (location, critical, attacker, weapon) is kept for `GetHitLocation`,
    which is now implemented.
  - OnHit runs with the attacker, then OnHitWith with the weapon.
  - Child races and Invulnerable creatures take nothing.
  - Essential actors (base flag or `SetActorRefEssential`) fall unconscious at 1 health
    instead of dying [design]; they get up again (docs/spec-notes/unconscious.md).
  - At 0 health the actor dies as with `KillActor`: dead flag, killer, GetDeadCount,
    OnDeath. `KillActor` now uses the same `Game::kill`.
- **Kill XP** [documented: Experience Settings]. The target's level picks the class
  (the first `iXPLevelKill…` threshold at or above it), which gives the reward. There is
  none when the player did less than `iXPDeathRewardHealthThreshold` % of the damage.
- **Armour.** The target's armour is what it would wear of what it carries
  (`CombatForms::worn_armor`, the owner's wearing rule from actors.md §2.3). DT and DR
  are the sums over the pieces [hypothesis]. The simulation creates an actor's inventory
  on first touch, so the engine reads it through `World::inventory_mut`.
- **In play** (`engine/nvf-play/src/combat.rs`). Every projectile hit and blast goes
  through `Game::hit` inside the script runtime's world swap.
  - Blasts use location −1 and the same armour rule as bullets, with the damage and DT
    perk entries and none of the weapon formula (perk-entry-points.md §2, "Blasts").
  - An `ActorHit` message goes out with each hit.
  - Without the script runtime, hits are only logged.
- **On screen** (`engine/nvforever/src/deaths.rs`, `locomotion.rs`):
  - A living actor plays a hit reaction over its other clips when the hit cripples a
    body part or lands on a crippled one (the rule and the clips: §12).
  - A dead one whose skeleton has a ragdoll drops as one (ragdoll.md §4;
    `ragdoll.rs`): the Havok bodies and constraints of its skeleton as jointed rigid
    bodies, shoved by the hit that killed it and by later hits.
  - Until its ragdoll is in the world (its ground is not loaded, or it was dead when
    drawn), and for a skeleton without one: it plays `idleanims\deathpose1.kf` or
    `deathpose2.kf` (chosen by reference) and holds its last frame, or goes straight to
    that end pose when already dead when drawn; a skeleton without death poses freezes
    and is laid on its back [design].
  - Dead actors' hit volumes move to DEAD_BIP; a ragdoll's bodies take their place.
- **Looting.** The crosshair offers **Search** on a dead actor, and activation by hand
  opens its inventory in the container menu (the dead-actor rule of
  `Game::default_activation`, now also on the by-hand path).
- **HUD** [design]:
  - no hit marker at the crosshair (removed at the owner's request 2026-10-27);
  - the last target's name and health bar for 4 s;
  - the player's health (`HP now/max`);
  - a line per kill with its XP.
- **Test aid:** `aimnpc` (aim at the nearest NPC's hit volume).
- **Checked** [observed 2026-10-09, `nvforever --cell Goodsprings --play`, exit 0, "run
  finished"; screenshots kept privately]:
  - **Raven** (creature, 6 health): killed by one 9mm hit, its OnHit script ran, +1 XP
    (very easy creature). Later hits on the corpse did nothing.
  - **Goodsprings Settler** (55 health, Field Hand Outfit, DT 2): 7 hits (7.2 each, a
    23.2 critical) killed it, +10 XP.
  - **The body:** it lay in the death pose, the crosshair showed "Search", and
    activating it opened its inventory (5.56mm rounds, Field Hand Outfit, Varmint
    Rifle).
  - **HUD:** `HP 200/200` (Endurance 5 × 20 + Base Health 100).
- **Lab.** `fnvlab run --layer 5` gives 28/32, the same four known failures as main
  (`L5.script.decode` and three `delay_*`; layer5.md), so the new Health base changed
  no scenario.

## 10. Melee and unarmed (step C6)

- **Swings** (`engine/nvf-play/src/combat.rs`, `melee`). Melee and unarmed weapons
  don't use the gun cycle.
  - A tap of Use swings on release (`fnv_combat::melee`). Holding Use for `power_hold`
    (0.4 s) starts a power attack [design].
  - **Combos.** An attack clip's `a:` text key names the attack group to play next,
    and its time is when the clip may be interrupted for it [documented: GECK wiki
    NiTextKeyExtraData: `a:R` plays AttackRight, `a:7` Attack7, a blank key a random
    attack]. In the base meshes archive every `AttackRight` clip of `h2h`, `1hm` and
    `2hm` carries `a:L` and every `AttackLeft` clip `a:R`; `h2hattack3` names 4,
    `h2hattack4` 3, `h2hattack5` itself, `h2hattack6` and `7` R; power clips have no
    `a:` key [observed: both rigs, 2026-10-11]. Some times (third person, seconds):

    | clip | Hit | `a:` | end |
    |---|---|---|---|
    | `h2hattackright_a` | 0.30 | L 0.63 | 1.43 |
    | `h2hattackleft_a` | 0.33 | R 0.63 | 0.90 |
    | `1hmattackright_a` | 0.30 | L 0.43 | 1.13 |
    | `2hmattackright_a` | 0.57 | L 0.63 | 1.53 |

    A press during a swing starts the next one at the `a:` key (or at once, if the key
    has passed), playing the group the key names, so the fists alternate right and left.
    A press after the swing has ended starts again from the weapon's own attack group
    (`AttackRight` by default) [design: when the original resets a run is not known].
  - **Creature melee cadence (#2535).** **Creatures only** (the maintainer's decision
    of 2026-11-18 on #2971): a creature's next attack decision waits for its playing
    clip's `a:` key, even when later than the 1.2 s base timer; without a key it waits
    for the clip's end as well as the base timer. An earlier key still brings the
    decision forward for combos. A creature's power attack publishes its clip's timing
    the same way. Clip times use the existing playback clock
    (`attack_anims::CombatPose::layers`, `attack_anims::SwingClip::ready_in`) [design].
    The key, the combo window and the clip's end are divided by the blow's attack speed,
    in the manner of the semi-automatic guns' `a:` key (§4, §4.1; guns also divide by
    the Animation Mult, melee does not); the `Hit` key stays at the clip's own time
    [documented: GECK wiki Weapons page, Attack Multiplier "speeds up or slows down just
    the attack animation"; design, #2535 review]. In ordinary combat a creature's
    attack speed is 1, so its clip's own times hold [design: `npc_combat::fight`].
    **People and NPCs keep the base timing** until their pace is measured in the
    original (observer check 244): the next decision after 1.2 s ÷ the attack speed
    (Attack Mult × the teammate rate), brought forward to the clip's `a:` key at the
    clip's own time when that is sooner, never held for a later key or a keyless
    clip's end; the combo window is the clip's own key-to-end span; a power attack
    waits for its clip's length and lands at its `Hit` key [design; owner request
    2026-10-16; `attack_anims::SwingClip::person_ready_in`]. A creature here is a
    fighter striking with its own attack (`npc_combat::Arm::Creature`: a CREA base with
    no weapon drawn, `Game::creature_attack`), as for creature spacing; a fighter with
    a melee weapon or fists, a CREA with a weapon drawn included, keeps the base timing
    [design].
    Ordinary combat uses this boundary calculation. Scripted melee has the shared
    calculation available, but plays no attack clip yet: `attack_anims::start`
    requires a fighter pitch or existing attack pose, and `npc_combat::fight`
    clears poses and skips actors controlled by UseWeapon. Clip-bound scripted
    cadence remains open; without a published clip its base timer and volley
    pauses retain upstream main's behavior, and a person's scripted blows keep it
    with a clip too [design: `use_weapon::Swinger::advance`,
    docs/design/use-weapon.md §3.1, #2535].
    The key's interrupt meaning is [documented: GECK wiki NiTextKeyExtraData, above];
    the lower bound is [observed: capture run15b_b2, 2026-11-15] for radscorpions,
    and [hypothesis] for other creatures' ordinary and power swings. Attack-chance rolls, style
    hold timers and attacks on the run (#2163) are unchanged [design].

    [observed: FalloutNV.esm and Fallout - Meshes.bsa, fnvtool esm decode / nif decode,
    main session 2026-11-15] VCrTier2RadscorpionMed 0x0014F403 (Radscorpion,
    template CrRadscorpion1Small, BNAM 0.6) and VCrTier3GiantRadscorpionMed
    0x0014F401 (Giant Radscorpion, template CrRadscorpion2Large, BNAM 1.0) both
    use `Creatures\Radscorpion\Skeleton.nif`, RNAM 64, OBND x -70..70,
    y -125..125, z 0..60. CSRadscorpion / CSGiantRadscorpion have attack chance
    70 and power attack chance 50. Their clips (seconds):

    | clips | end | Hit | `a:` |
    |---|---|---|---|
    | `h2hattackleft_high/_low/_mid` | 1.500 | 0.700 | R 1.267 |
    | `h2hattackright_hight/_low/_mid` | 1.500 | 0.700 | L 1.300 |
    | `h2hattackpower` | 1.667 | 0.933 | R 1.533 |
    | `h2hattackforwardpower` (sting) | 1.267 | 1.100 | none |

    [observed: main session's measured Searchlight runway run on 63db0209c,
    2026-11-15] NVForever started each attack 1.20–1.21 s apart; 23 of 39 were
    power attacks. Every clip, including the 1.267 s sting, was cut by the next
    start. For creatures, the min-only key adjustment and ordinary-only timing
    publication are replaced by the clip boundary above; people keep the min-only
    adjustment [design: `npc_combat`, `attack_anims`].

    [observed: capture run15b_b2, 2026-11-15; xNVSE 6.4.9, JIP LN 57.30]
    On Searchlight Airport's runway against a standing god-mode player, each of
    this pair entered GetAnimAction 2 seven times in about 13 s. Start gaps were
    2.82, 1.47, 1.39, 2.20, 2.67, 1.28 and 2.05, 3.74, 1.93, 2.15, 1.37,
    2.00 s (median 2.0, minimum 1.28). Action 2 lasted 0.77–1.18 s, followed
    by action 4 for 0.44–1.10 s. No new attack began during action 2; the shortest
    gaps sit just past the ordinary clips' keys and the sting's end. The subsequent
    pause is not implemented: three of fourteen attacks followed action 4 at once,
    the others after 0.1–2.1 s. Against a player backing away for 10 s, one attacked
    three times (3.5 s apart), the other never. Extra pauses and backing-away parity
    remain open (observer check 244) [design].
  - **Variants.** The melee clips come as `_a` and `_b` (`h2hattackleft_a.kf`,
    `…_b.kf`; `h2hattack5` only `_a`, `h2hattack6`/`7` neither) [observed]; each swing
    picks one at random [hypothesis], falling back to the plain and `_a` forms.
  - **Power attacks** play by the movement keys held when they start: `AttackPower`
    standing, `AttackForwardPower`, `AttackBackPower`, `AttackLeftPower`,
    `AttackRightPower` (an axis past 0.3; forward and back win a tie) [observed: those
    clips for `h2h`, `1hm`, `2hm` in both rigs; owner's account of the original,
    2026-10-11; thresholds design]. Sneaking, only the standing one has a sneak form.
    Damage: base × `fDamagePowerAttackStandBonus` (3.0), `…ForwardBonus`,
    `…BackBonus` or `…SideBonus` (2.0 each) [observed: FalloutNV.esm values;
    hypothesis: that they multiply the damage, from their names]. `fDamagePowerAttackBonus`
    (2.0) is not used. A power attack cannot be cut by a combo; a press during it
    waits for its end.
  - The swing takes its own clip's length ÷ (attack × animation multiplier), or
    1 ÷ "Attack Shots/Sec" without a clip (× `power_time`, 1.3, for a power attack). It
    strikes once, at the clip's `Hit` key [documented: NiTextKeyExtraData], or at 40 %
    of the swing without one [design]. The body reports the timing of the clip it plays
    for the swing (`WeaponAnim::timed`) a frame after the swing starts.
  - The strike is a fan of `melee_rays` rays over ±`melee_arc_deg` from the eye along
    the view, as long as the reach; each actor is hit once (`projectile::swing`).
- **Reach** (`damage::melee_reach`) = `fCombatDistance` × scale × the WEAP Reach
  [documented formula]. The masters lack `fCombatDistance`; ours is **107**, fitted to
  the measurement below (until #1334 a placeholder of 150) [hypothesis, tunable]. A
  Reach of 0 (the Fists) counts as 1 [hypothesis]. The reach is counted from the
  attacker's body [hypothesis: the reading of the 45 below]: an NPC strikes within its
  reach plus the two bodies' radii centre to centre (`npc_combat::melee_spacing`; two
  people, 22 + 22: knife 97.5, sledgehammer 124, super sledge 151, bumper sword 172,
  lead pipe 178), and stops 6 units inside that (`MELEE_STOP_SHORT`, fitted to the knife
  troopers' 96 start and 92 closest), closing in again beyond it; the bodies, until known,
  are two people's (44). **Open:** ours mostly swings once stopped (a blow is rolled on
  the style's attack chance; a failed roll holds it for the hold timer), and the AI's
  walk ends `fnv_ai::alert::STOP_SHORT` (0.9) of its stop in, so its blows begin short of
  the original's: Violet with a lead pipe at 170 on open ground (151-158 among her camp's
  rocks) against 180 [observed: smoke melee-reach-lead-pipe, 2026-11-06]. Given its strike distance to stop at instead, she
  swung at 176, but with no band between stopping and closing in again she stopped and
  closed in again every frame as her body's radius moved with the swing (271 AI lines in
  30 s); a knife, whose walk ends only 10 units inside, could stop outside its reach. The
  fix belongs to the AI's walk (swing on the way in, or end the walk at the stop). The player's fan of rays
  runs from the eye out to the reach plus the player's capsule radius (22: a knife 75.5,
  as long as with the old 150 × 0.5).
  [design, #2535: `projectile::swing_reaching`, `nvf_play::combat::PhysView`]
  The player's ground-reach fallback adds a target margin of max(24, its published
  scaled skeleton BBX half-width), using the same width as actor blocking. Ordinary
  targets retain 24; widened low creatures can be hit with a short weapon at the
  blocking stop, looking level or down. Candidate lookup expands by the same margin;
  the arc, height limits and first-hit obstruction rays remain unchanged.
  Measured 2026-11-01 (campaign o14_reach, `nvfairport` runway, god-mode player standing
  still, NCR troopers of the leveled list given one weapon each, every game loop) [observed]:
  the centre-to-centre distance at which a swing starts (GetAnimAction entering 2 or 3;
  10-16 swings each, median, ignoring a few starts at 230-300 units while still closing)
  is knife (Reach 0.5) 95, sledgehammer (0.75) 120, super sledge (1.0) 150, bumper sword
  (1.2) 175, lead pipe (1.25) 180. That fits **about 45 + 107 × Reach** within 5 units.
  Read as a body gap, the 45 is about the two capsules' radii, which would make
  `fCombatDistance` about 105-110 rather than 150 [hypothesis: the gap reading; the
  capsules were not measured]. Ours takes 107 on that reading (#1334). Creatures' starts
  (same run, centre to centre, median): gecko 81, cazador (young) 80, night stalker
  (young) 102, radscorpion 97-129, feral ghoul 135-140, giant mantis 101-163 (drifting
  out over the fight), bighorner bull 232; and in o12_reach (same ground, 2026-11-01)
  coyotes 84-117 over 5 starts, a dog 172 and a deathclaw 88 (one start each, hits
  landing at 69 and 108). Ours fits each measured skeleton's reach beyond its RNAM
  (`npc_combat::creature_body_reach`, npc-behaviour-compare.md §3).
  Over 12-16 swings and about 13 landed hits each, the troopers' weapons stayed at
  GetWeaponHealthPerc 99.9 (§17).
- **Damage.** The melee and unarmed forms of §2, with the target's worn armour. The
  attacker's Melee Weapons and Unarmed skills are derived from Strength and Endurance
  (§8).
- **Unarmed.** With no weapon equipped the player fights with the `Fists` WEAP
  [hypothesis: the game's unarmed weapon form]. The Fists do 0 base damage, so a punch is
  the UnarmedDamage actor value (0.5 + 0.05 × Unarmed) [documented].
- **Knockback.** The force is computed with the documented Melee Knockback Formula and
  logged. A dead actor's ragdoll is shoved by every shot and blow that lands on it
  (ragdoll.md §4). A living actor (the player, an NPC, a creature) struck by a melee or
  unarmed blow that leaves it alive and on its feet (not knocked down or out) is pushed
  back (#1070, `nvf_play::combat::Knockback`):
  - **Force to motion** [design; the original's not measured, observer-checks.md #61]:
    the force is the speed it is pushed at, units per second, falling evenly to rest over
    fKnockbackTime (1 s in FalloutNV.esm [documented: GMST]), so it travels force × time
    / 2: a Sledgehammer blow of 14 damage on Agility 5 (force 114) 57 units, a punch of 3
    (force 48) 24, the cap (512) 256. A power attack deals more damage and so pushes
    further; nothing else multiplies it.
  - **Which way**: the blow's direction along the ground (the player's: the view's; an
    NPC's: from it to its target).
  - **How**: through the actor's character, as an attack clip's travel (actors.md §8.13):
    the player's character controller (`player::Lunge::knock`), an NPC's or creature's
    character in `residents` (one is made if it has none), so walls and other actors stop
    it; the pushed actor pushes nobody aside. A new blow's knockback replaces one still
    going [design]. It goes on through the stagger a blow starts (hit reactions hold the
    walk, not the push; they end an attack clip's carry, not a knockback, which
    `residents` keeps apart for that); a knockdown throws the ragdoll instead. On an NPC
    that is walking it adds to the walk, so a punched actor still closes in or gets away;
    an actor on furniture is not pushed (its body stays in the seat); the player's push
    counts down walking or swimming alike, and is dropped while seated or flying with no
    collision [design for each: review of #1219]. The player in god mode (`tgm`) is not
    pushed: pushed, a god-mode player bitten by dogs kept them chasing it (smoke
    `combat-dogs`, 93 chase lines).
  - **Blocked and 0-damage blows** push too: the formula's force with no damage (about 30,
    15 units) [documented formula; design to apply it to a blocked blow,
    owner-review item 37].
  - **A second blow** during a knockback replaces it [design]. The replaced one is logged
    as interrupted, and the new one is measured from where the feet are then, so a report
    never divides several blows' travel by the last one's request [design: #1386, which
    fixed the report keeping the first blow's start; checked by unit test, not in a run].
  - Logged, each line tied to its blow (`<tag>` = `blow=<n> attacker=<id> target=<id>
    power=<0|1>`; `<n>` counts the blows that knock someone back in this run; `SetVel`'s
    pushes have blow 0 and attacker 0): `knockback: <tag> knocked back at <v> units/s for
    <s> s (<d> units)`; at its end, `knockback: <tag> alive=<0|1> moved=<0|1> walked=<0|1>
    pushed back <n> units of <d> (<p> %) along=<a> lateral=<l> push=<word>, from … to …,
    frames=<n> secs=<s> travel=<t> steps=<n>`; replaced, `knockback: <tag> interrupted by
    blow=<n> …`, unless its time was already up, when it gets its end line (the player:
    `player: knocked back …`, `player: knockback done: …`); `knockback summary` at exit.
  - **What "pushed back" measures** [design: `residents/knock_run.rs`]: the net ground
    distance between the feet when the blow began and the feet when its time is up. It is
    not the push alone: a walk or MoveTo moves the feet too, and a death can stop it short.
    `moved` (a script moved the actor), `walked` (it walked, or an attack clip carried it)
    and `alive` (0 for dead or out cold) say when something else went into it. The size
    alone would score a push backwards as 100 %, so `along` and `lateral` split it against
    the way the blow asked, and `push` is `none` (under 5 % of the distance asked),
    `reversed`, `sideways` (over 25 % of it to the side) or `along` [design]. `frames`,
    `secs` and `travel` are what the frames asked for, `steps` the fixed physics steps
    taken meanwhile, to compare frame rates: the travel is integrated with the frame's
    time and consumed by later fixed steps (measured below). With the aid below, each frame
    also logs
    `knockback trace: … dt=<s> left=<s> owed=<d> consumed=<d> along=<a> lateral=<l>
    dir=(<x>, <y>) steps=<n>`.
  - Smoke `melee-knockback` isolates one player power blow
    (`NVF_PLAY_TEST_MELEE_KNOCKBACK`, nvf-play `test_aid/melee_knockback.rs`, nvforever
    `residents/melee_knockback.rs`): AI off on the target (it may be far away, with no
    state yet: SetActorsAI only writes the game's record of the reference [design]) and on
    the loaded area's actors, then one MoveTo onto the ground, a blow gated on readiness and
    watched up to impact, and the report tied to that blow. A standing actor has no
    character until its knockback makes one [design], so "settled" is its feet still, at
    the height of what is under them, with its hit volume there.
  - **Frame rate** [observed 2026-11-05, #1386: that scenario's blow, Display sync Off, a
    fresh home each run, 20 runs, all in the scenario's range]: the distance pushed as a
    share of the 17.25 units asked, by the Frame rate limit set.

    | Limit | Runs | Mean | Min to max | Frames in the 1 s push | Mean frame |
    |---|---|---|---|---|---|
    | 30 | 5 | 99.57 % | 98.11-102.32 | 30.0 | 32.3 ms |
    | 60 | 5 | 100.57 % | 99.86-101.86 | 59.2 | 16.8 ms |
    | 120 | 5 | 100.05 % | 98.99-101.25 | 57.0 | 18.0 ms |
    | 144 | 5 | 100.12 % | 98.30-101.29 | 60.0 | 16.5 ms |

    No dependence on the frame rate was found between 30 and about 60 frames a second: the
    spread within a limit is as big as any difference between limits, so the render-frame
    travel needs no fix for this range. That is all it shows. The machine never ran above
    about 65 frames a second, so limits 120 and 144 did not reach those rates and higher
    rates are untested; irregular frames were not tested separately.
- **Blocking.** `damage::blocked` takes `block` (60 %) off a melee hit taken while
  blocking [design]. Nothing attacks the player yet; NPC melee (Layer 6 AI) will use it.
  While the player holds Block with fists or a melee weapon (`MeleeInput::blocking`),
  tapping or holding Attack starts no swing, sound, or hit unless the opt-in
  `Power Attack From a Block` tweak (`bPowerAttackIfBlocking`, `instant_power`) is on
  [observed: player report in #3753; design: mid-block input suppression and post-block
  Scribe Counter timing pending an original-game capture].
- **The body.** A power attack plays `{prefix}attackpower.kf` (`1hmattackpower.kf`,
  `h2hattackpower.kf`). Normal swings use the weapon's attack animation, falling back to
  its `_a` variant (`1hmattackright_a.kf`).
- **Checked** [observed 2026-10-09, `nvforever --cell Goodsprings --play`, exit 0, "run
  finished"]:
  - **Sledgehammer** against the Goodsprings Settler (55 health, DT 2), standing on its
    open side: three normal hits for 14.3 (24 × the skill bonus at Melee Weapons 15 +
    MeleeDamage 2.5 − DT 2), then a power attack killed it (+10 XP). Knockback forces
    were 100 and 143. The screenshot shows the first-person power swing, the target bar
    at 0/55, the kill line and "Search".
  - **From the fenced side** the swings missed: a ray along the view hit the fence at
    71 units, and melee correctly doesn't reach through it.
  - **Fists:** three taps, one pressed during a swing, gave three hits of 0.2 (Unarmed
    damage 1.25 against DT 2, the 20 % floor).

### 10.1 People's power attacks (NPCs)

NPCs fighting with a melee weapon or their fists (`nvforever::npc_combat`, `Arm::Melee`).
Creatures' power attacks are older and unchanged (the radscorpion's sting; npc_combat
module notes).

- **Whether.** "If an actor decides to do an attack, the Melee Decision will also decide
  whether to do a Power Attack or a normal left or right attack" [documented: GECK wiki
  Combat Style, Melee Decision]. The chance, 0–100: the style's Power Attack % Chance,
  plus Recoil/Stagger Bonus to Power Attack while the opponent recoils or staggers, plus
  Unconscious Bonus to Power Attack while it is unconscious [documented: same page]
  (`fnv_game::combat_style::power_attack_chance`). The same page's Unconscious Bonus to
  Attack is added to the attack chance now too [documented]. The fields are CSTD's
  `power_attack_chance` (u8), `recoil_stagger_bonus_to_power`,
  `unconscious_bonus_to_power_attack` (f32) [documented: schemas/fnv/ai.json, xEdit
  layout]. Recoiling is the existing test (its blow was blocked); staggering is not told
  to the decision yet. Every actor gets a style (its own, else `DefaultCombatstyle`); with
  no style record at all the chance is the creatures' fallback 25 % [design: owner
  2026-10-16, random power attacks where the rule is not found].
- **Which.** With the flag "Choose Attack using % Chance" (CSTD flags bit 0) a roll of
  1–100 against the running sums of the Normal, Forward, Backward and Left shares picks
  the first it falls within, else Right [documented: GECK wiki Combat Style, the five
  steps; "if the sums total up to greater than 100, some of the later power attack types
  … may not ever be chosen"] (`combat_style::power_dir_by_chance`). Without the flag the
  movement decides: forward, back, left or right by the way the actor moves at the
  decision, standing ("normal") when still [documented: "Choose Attack Using Movement";
  hypothesis: that the flag cleared is that check box, as CSTD has one bit for the
  pair]. "Moves" is a horizontal speed above 20 units/s; the direction is its larger axis
  against the actor's facing, forward and back winning a tie, as the player's keys
  (`fnv_combat::melee::PowerDir::of`) [design].
- **Clips.** `{prefix}attackpower.kf`, `…forwardpower`, `…backpower`, `…leftpower`,
  `…rightpower` for `h2h`, `1hm` and `2hm` in `characters\_male`; crouched only the
  standing one (`sneak1hmattackpower.kf`, `sneak2hmattackpower.kf`,
  `sneakh2hattackpower.kf`), the others play standing [observed: Fallout - Meshes.bsa,
  2026-10-20]. Lengths (s): h2h 1.43 / 1.60 / 1.13 / 1.47 / 1.43 (standing, forward,
  back, left, right); 1hm 1.20 / 1.60 / 1.27 / 1.37 / 1.23; 2hm 1.67 / 1.60 / 1.50 /
  1.57 / 1.50 [observed: `fnvtool nif anim`]. The `attackcustomNpower` clips are not
  used (no style field names them). A direction the actor's rig has no clip for gives an
  ordinary blow [documented for the movement case: "left and right power attacks will
  not be performed"; design for the % case]. Both legs crippled: the standing one, as
  the player's (crippled-limbs.md §4) [design: the player's rule applied to people].
- **Damage.** Base × `fDamagePowerAttackStandBonus` (3.0), `…Forward…`, `…Back…` or
  `…SideBonus` (2.0) by direction, as the player's (above) [observed values;
  hypothesis: that they multiply, as above]. `ActorStrike::anim` carries the attack
  group; `nvf_play::combat` turns it into the multiplier for a person's blow, not a
  creature's.
- **Timing.** The blow lands at the power clip's own `Hit` key (fallback 40 %); the next
  melee decision comes no sooner than the clip's end: a power attack is not cut by a
  combo and starts a new run [design, as the player's].
- **Blocks and staggers.** A power attack breaks a block and the blocker staggers (§12,
  "Power attacks break blocks"), as the player's and creatures' do. The page's "staggers
  when the opponent hits them for a certain amount of damage, based also on the actor's
  Agility" has no formula sourced: not built; the hit-reaction rules (§12) stand.
- **Fatigue and AP: none spent** [design]. `fActionPointsPowerAttackMult` is 1.5 in
  FalloutNV.esm [observed]; the `fActionPointsAttack…` costs are VATS costs
  (docs/design/vats-mode.md), and NPCs do not use VATS. CSAD's Power Attack Fatigue Mod
  Base and Mult (5 and −10 in all 84 styles of FalloutNV.esm [observed]) have no
  documented meaning, and New Vegas's Fatigue is the knock-out meter
  (unconscious.md): nothing documents a power attack spending it.
- **Line.** The actor tries its PowerAttack combat line instead of Attack
  (combat-dialogue.md §1).
- **GetAnimAction**: 2 until the strike, 4 after, as any swing; the New Vegas table has
  no code of its own for a power attack [documented: GECK wiki GetAnimAction].
- **Log**: `npc combat: <name> power attack AttackLeftPower at <target> (30 %, by
  chance)` (or `by movement`, `fallback`), and `npc power attacks summary:` by group.
- **The styles in FalloutNV.esm** [observed, 2026-10-20]: 63 of 84 choose by chance.
  People's: DefaultCombatstyle, DefaultCombatMeleestyle, CSRaider(Melee), CSNCRTrooper,
  CSLegionary 25 % (20 each way, by chance); FollowersCombatStyleMelee 50 % (20 each);
  CSSupermutantMelee 50 % (10/30/0/30/30); CSFeralGhoul 10 % (20/40/0/20/20); Legate
  Lanius 45 % by movement (bonuses 15 and 25); FollowersCombatStyleRanged,
  MysteriousStrangerCombat, CSSupermutantGun 70 % by movement.

## 11. Explosives and destructible objects (step C7)

- **Thrown weapons and mines.** They are their own ammo (`self_ammo`): a shot spends one
  and throws its PROJ along the view. Each projectile type behaves as §5 describes:
  - grenades are lobbers on a fuse (Alt. Trigger timer);
  - mines land, arm after their timer and go off when an actor other than the thrower
    comes within their proximity radius;
  - C-4 (Explosion + Detonates, no Alt. Trigger) lands and waits. It no longer explodes
    on impact [documented: GECK wiki Projectile, Detonates]. At rest it leaves flight as
    a mine does: a reference of its PROJ, placed in the thrower's cell and owned by the
    thrower, so a save keeps it and it stays in its cell (#1986) [design]. The player's
    activation makes it the C-4 item again, as a disarmed mine.
  - A door travel drops what is still in flight, the hits on their way and the blasts
    not yet gone off: they stand at the old space's coordinates (#1986) [design].
- **OnFire and detonation.** Every shot runs the weapon's OnFire blocks
  (`Game::weapon_fired`, block 0x24).
  - The detonator's script, `DetonatorOnFire`, calls `player.DetonatePlacedExplosives`
    and has main-quest hooks: the Hoover Dam generators (`vHDGenerator0N.do 50000`) and
    VMQ03b [observed: its SCTX].
  - `DetonatePlacedExplosives` is implemented: it asks the engine
    (`Game::combat_requests`), which sets off that actor's detonating projectiles: those
    in flight, and the charges at rest it owns in the loaded cells (removed as they go
    off; one in a cell that is not loaded stays) [design].
  - Ranged weapons without a projectile (the detonator, binoculars) cycle without ammo.
- **Explosions** (`fly`). A queue of up to 64 per frame takes impacts, detonations and
  destruction stages [design]. Each explosion:
  - damages actors in its radius, with line of sight, armour and the cause's and target's
    damage and DT perks (perk-entry-points.md §2, "Blasts"; with no cause, the target's
    with no condition on the attacker or the weapon [hypothesis: as the original]);
  - reaches corpses too: one whose ragdoll took over from its hit volume is caught by its
    ragdoll, its centre the trunk's, its reach the bounds of its bodies, its bodies' centres
    seen and measured to as a living actor's part capsules are; once per actor, a volume not
    yet dropped standing for it; placed objects' ragdolls (traps) and gore pieces are not
    caught (#2346) [design: `CombatWorld::blast_targets_near`,
    `PhysWorld::ragdoll_bounds`]. Only blasts: proximity fuses and swings still go by the
    living actors' volumes. A corpse takes no damage (it is dead) but may lose the part facing
    the blast (dismemberment.md §4.2);
  - uses the impact point for effects, radius, damage falloff and push. For actor line
    of sight only, an impact with a finite, nonzero surface normal starts 0.5 units
    outward along its normalized normal, then derives the existing 16-unit raised
    origin from that cleared point. The ray to the raised origin must still be clear:
    a ceiling or floor above the explosion continues to shield actors. Timer, remote,
    mine, script and destruction-stage explosions without contact data keep their
    existing origins; bouncing and rolling projectiles already nudge their positions
    [design: `crates/fnv-combat/src/projectile.rs` `blast`, `Projectile::step`,
    `engine/nvf-play/src/combat.rs` `fly`]. This bounded clearance covers shallow
    surface penetration, not arbitrary embedding. Whether an embedded origin caused
    the intermittent missed player damage reported in #1581 remains a hypothesis
    until repeated runtime checks [hypothesis];
  - rolls knockdown: always with "Knock Down - Always", by the documented formula with
    "By Formula". A knockdown is counted and logged; it is not animated yet;
  - damages destructible objects whose collision overlaps its radius;
  - pushes dynamic bodies with its force (`PhysWorld::push_dynamic`: a speed change of
    the EXPL force in units/s at the centre, falling to 0 at the radius [design]).
  - the enchantment (EXPL `EITM`) applies to every actor damaged (hit-effects.md §1);
    radiation and the placed impact object are logged and not built.
  - is the hit of whoever set it off, with that weapon: the shooter of the projectile
    that exploded, the actor who detonated a placed charge, the attacker whose hit
    reached a destruction stage. A stage reached by burning down has no attacker
    [design]. Until 2026-10-19 every blast was the player's, with the player's weapon:
    an NPC's Incinerator or grenade on another NPC gave the player XP and a hit marker,
    ran OnHit with the player as the attacker, and was left out of the `npc hits
    summary` (§18).
- **Destructible objects** (`Game::damage_object`, now also behind `DamageObject` /
  `do`, so scripts and combat share it) [documented: GECK wiki Destructible Object
  Stage]:
  - damage adds up to the object's health; each stage applies once its health
    percentage is reached;
  - Cap Damage stops one hit at its stage;
  - Destroy marks the object destroyed, Disable disables it;
  - a stage's self-damage per second burns it down (`Game::destruction_tick`);
  - OnDestructionStageChange runs;
  - the stages' explosions go off 40 units above the object [design] and can damage
    other objects.

  [design] Numerical policy (`crates/fnv-game/src/combat.rs`,
  `destruction_damage_threshold` / `destruction_threshold_reached`;
  `crates/fnv-game/src/functions/objects.rs`, `destruction_stage`): compute the required
  damage as `health * (100 - percentage) / 100`. Both stage lookup and Cap Damage
  consider that threshold reached within `8 * f64::EPSILON * health` damage points.
  Cap only when the damage after the hit reaches it and the damage before does not;
  the stage index alone cannot establish a crossing, since stage 0 also represents
  damage above every threshold. Self-damage and subsequent hits pass an already
  reached cap. The tolerance covers floating-point rounding, including damage saved
  with the former `health * (1 - percentage / 100)` calculation, without a gameplay
  percentage margin (at health 20 it is about 3.6e-14 damage points). Reached caps are
  derived from saved `RefExtra.damage`; `burning` preserves self-damage on restore
  (`crates/fnv-game/src/game.rs`, `RefExtra`, `Game::snapshot` / `Game::restore`).

  Bullets damage the destructible they hit with the hit's damage (no armour). The
  stages' damaged models are not drawn yet (logged); of the `BSDamageStage` nodes only
  the add-on nodes below them follow the stage (smoke, fire: `destruction-stages.md`).
- **Test aids:** `equip=<FormID>` switches weapons during a run.
- **Checked** [observed 2026-10-09, `nvforever --play`, exit 0, "run finished"]:
  - **Truck02** (0x000E7F40, 450 health, 6 stages): one missile's blast took it to stage
    4 (destroyed). Its stage explosions went off (SmallNukeExplosion 0x1CA88 and
    GrenadeFragExplosion), and it then burned down to the last stage on its own. The
    first run also showed stages without an explosion (FormID 0) counted as explosions;
    they now read as none.
  - **Frag grenade** near the Goodsprings Settler: it exploded on its fuse.
  - **C-4:** thrown, it landed and waited. Switching to the detonator and firing ran
    `DetonatorOnFire`, and `DetonatePlacedExplosives` set off the one placed charge
    (C4Explosion). Both explosions went off away from actors: the grenade bounced far
    before its fuse ran out. Throw arcs and bounce are part of the planned projectile
    captures.

## 11a. Mines

Mines are the placed explosives of the plugins (PGRE, 297 across the masters) and the ones
the player puts down. Code: `fnv_combat::mine` (rules), `nvf_play::mines` (in play),
`fnv_sim::World::activate` (disarming), `fnv_game::functions::mine_spares` (who is spared).

- **The records** [observed: `fnvtool combat form`, `esm decode`, FalloutNV.esm]:
  - `MineFragProjectile` (0x43FA): Lobber; flags Explosion, Alt. Trigger, Can Be Disabled,
    Can Be Picked Up; proximity 100, timer 3; explosion `MineFragExplosion` (100 damage,
    radius 192, Knock Down By Formula); countdown sound `WPNMineTick`, disable sound
    `WPNMineDisarm`; Bouncy Mult 1.0; its weapon `WeapMineFrag` (0x433C, Land Mine).
  - It is destructible: DEST health 5, one stage at 0 % that disables and destroys it and
    sets off `MediumExplosion` (75 damage, radius 220).
  - Settings: `fMineExteriorRadiusMult` 1.6, `fMinesDelayMin` 0.2,
    `iProjectileMineShooterCanTrigger` 1, `iMineDisarmExperience` 5.
  - The model (`MineFrag.NIF`) has a `##LightOn` node (a glow and a lit lens) and a
    `##LightOff:0` lens beside the body `##FMine:0`, and a `BlastNode` 45 units up.
- **Trigger** [owner 2026-10-19: it starts beeping at about 2 m; 1.4 m quoted for the frag
  mine]: an actor within the proximity radius starts the fuse. Out of doors the radius is ×
  `fMineExteriorRadiusMult` [hypothesis from the setting's name; it makes the Fallout 3
  tutorial's 100 indoors and 160 outdoors, level-design.md]. Distance is measured to the
  nearest point of the actor's upright body [design]. Dead actors set nothing off.
- **Who sets it off** [owner 2026-10-19]: anyone who is not in the owning faction (XOWN) nor
  in a faction it counts as an ally or a friend. A mine without an owner spares nobody. The
  player's own mines never go off for the player. They spare the player's teammates too,
  which is a hypothesis: they are the player's side. `iProjectileMineShooterCanTrigger` is
  1 in the data, which the owner's rule overrides; what the setting does is open.
- **Fuse** [owner 2026-10-19: the player's Explosives skill sets how long you have, about
  3 s nominal]: the PROJ timer × (0.35 + 0.65 × the Explosives of whoever set it off ÷
  100), never below `fMinesDelayMin` [design, #1659]. For the frag mine (timer 3) that is
  1.05 s at Explosives 0, 1.44 s at 20, 2.03 s at 50 and 3 s at 100.
  - Why: the curve before, timer × Explosives ÷ 100 [hypothesis: linear], gave a starting
    character (Explosives 10–20) 0.3–0.6 s, about two blinks of the light: less than a
    human reaction and a step back, so a mine could not be escaped or disarmed once it
    beeped. A third of the timer at 0 gives about a second; the skill still adds the rest,
    up to the whole timer at 100. How the original scales it is not known.
  - The capture `cl_d_mines_live` (simulation.md §1), where a mine went off between 0.5
    and 1.5 s after the player stepped on it, still fits at Explosives up to 23. That
    save's Explosives was not recorded.
  - Once started, the fuse burns even if the actor leaves [design], and the light blinks
    and the countdown ticks for all of it.
- **Tick and light** [owner 2026-10-19: live mines show the red-orange light; it blinks
  once a mine is set off]:
  - A live mine shows `##LightOn`. A loose or disarmed mine item shows `##LightOff`
    [hypothesis].
  - While the fuse burns, the light is lit for 0.25 s and dark for 0.25 s, and the
    countdown sound plays each time it lights [design: the period].
- **Goes off**: the mine is disabled (as the capture saw), and its explosion goes off 40
  units above it. That is the height of the destruction stages' explosions; the model's
  BlastNode is at 45. The blast is the hits of the actor who put it down; nobody's for
  the plugins' mines [design].
- **Shot, or caught in a blast** [owner 2026-10-19: "you can shoot a mine to blow it up"]:
  through its destruction data, like any destructible (§11). A shot or a blast reaching 5
  damage sets off `MediumExplosion`. So mines set each other off, as the Fallout 3 tutorial
  says (level-design.md). A mine that goes off on its fuse is disabled first, so its own
  blast does not also set off its destruction stage.
- **Disarming** [owner 2026-10-19]: one activation by the player disarms a live mine, even
  while it beeps. It becomes its weapon, a loose item in its place, which is taken with the
  next activation like any other item. Every mine needs Explosives 1, so there is no
  check. XP and the "Mines Disarmed" statistic as before (experience.md). The disable sound
  plays. A faction's mine leaves an item that faction owns [hypothesis]. A scripted
  Activate still does nothing (simulation.md §1).
- **Putting one down** [owner 2026-10-19: armed "immediately after leaving the player
  hand"]: the thrown mine flies as a lobber. When it comes to rest it leaves flight
  (`ProjEvent::Settled`) and becomes a reference of its PROJ where it lies, owned by the
  thrower. It is then a mine like the placed ones.
  - **Lobber bounce, a fix that came with this:** a lobber keeps 0.3 × its Bouncy Mult of
    its speed at each bounce [hypothesis: Bouncy Mult multiplies]. It used to keep the
    Bouncy Mult itself, 1.0 for the frag grenade and the frag mine, so they bounced at
    full speed until the fuse ran out (§11 "Checked") or, a mine, for ever.
- **Drawn**: render streaming draws PGRE references (PROJ bases) with their projectile's
  model. The drawing code marks the light meshes (`MineLight`).
- **Checked** [observed 2026-10-19, smoke `mines-placed` and `mines-thrown`]:
  - The three frag mines on the bridge north of Primm (0x00172DB5, DDA, DDC) are drawn
    with their lights.
  - At Explosives 100, the player 60 units away along the bridge set one off with a 3 s
    fuse. Aimed at and activated while it ticked, it was disarmed into a Frag Mine lying
    in its place (0xFF000000), and the next activation took it.
  - The 10mm pistol's shot took 0x00172DDA to its destruction stage, and it blew up.
  - Stood on at Explosives 20, a mine went off after 0.6 s with `MineFragExplosion` (the
    linear fuse of the time; 1.44 s since #1659).
  - A thrown frag mine came to rest, was drawn lit and was not set off by the player
    walking over it.
- Not built: NPCs putting mines down, NPCs seeing and avoiding mines, the HUD's mine
  marker, mines owned by an actor base (XOWN naming an NPC_), `DisableAllMines`. Also the
  Light Step perk (mines do not go off for the player), and the Explosives chance of
  setting one off while disarming, which the owner's rule leaves out.

## 11b. Thrown weapons land (2026-10-24)

From the ESM audit (gap 5). Code: `fnv_combat::data::ProjectileStats::lands_as_weapon`,
`nvf_play::thrown`.

- **The records** [observed: FalloutNV.esm and the add-ons, PROJ `DATA` flags]: "Can Be
  Picked Up" without "Can Be Disabled" on `NVThrowingKnifeProjectile` (Missile; Pins
  Limbs, Rotation), `NVHatchetProjectile`, `NVSpearProjectile`,
  `NVDLC02WeapTomahawkProjectile` (and Honest Hearts' `NVdlc02tomahawkrProjectiletest`),
  `TrapBaseballProjectile` (Lobber) and Old World Blues' `NVDLC03MineMeat`. The
  mines and C-4 carry both flags.
- **What they mean** [documented: GECK wiki Projectile]: "Can be Picked Up: If checked,
  projectile can be picked up by the player character after it has been disabled, or
  regardless of it if Can be Disabled isn't enabled." "Default Weapon Source: Projectile
  references store whichever weapon they fired from/as, and if they can be picked up by
  an NPC or the player they will be added to inventory as that weapon." The same page's
  notes say the flag "doesn't appear to work with Missile type projectiles": the
  masters' thrown weapons are Missiles that set it; what the original does with them is
  an owner question.
- **As built** [hypothesis: where a landed weapon goes]: a projectile with the flag, no
  "Can Be Disabled" and no explosion becomes its weapon where its flight ends: in the
  inventory of the actor it struck (taken from the body), else as a loose reference at
  the point it struck, 4 units out from the surface, turned the way it flew (or where it
  ran out of range), in the thrower's cell; `crate::dropped` draws it and lets it settle.
  The weapon is the one thrown, else the PROJ's Default Weapon Source. Log lines
  `thrown: … landed at …` and `thrown summary` at the end.

## 12. Limb hits (step C8)

- **Which body part data** (`CombatForms::body_parts_for`):
  - a creature's own BPTD (CREA `PNAM`) [documented: xEdit];
  - otherwise the BPTD whose model is the actor's skeleton, which is
    `DefaultBodyPartData` (0x1D) for `characters\_male\skeleton.nif`. The player's
    `PlayerBodyPartData` (0x1C) is skipped [hypothesis, combat.md §5.9].
  - `DefaultBodyPartData` has six parts. The second number is the damage multiplier,
    the third the share of total health [observed: `fnvtool combat form`]:

    | Part | Bone | Damage | Health | Condition |
    |---|---|---|---|---|
    | Head | Bip01 Neck1 | ×2 | 20 % | Perception |
    | Torso | Bip01 | ×1 | 60 % | Endurance |
    | Left Arm | Bip01 L UpperArm | ×1 | 25 % | Left Attack |
    | Left Leg | Bip01 L Thigh | ×1 | 25 % | Left Mobility |
    | Right Leg | Bip01 R Thigh | ×1 | 25 % | Right Mobility |
    | Right Arm | Bip01 R UpperArm | ×1 | 25 % | Right Attack |
- **Part capsules** (`fnv_phys::actors`, `PhysWorld::set_actor_parts`): the actor's
  kinematic body carries one capsule per bone segment. Each collider's owner is
  `Owner::Actor { form_id, layer, part }`, where `part` is the index in the BPTD and 255
  (`actors::WHOLE`) means the single C3 capsule. Shapes are rebuilt only when a size
  changes, as in C3.
- **Bones** (`engine/nvforever/src/limbs.rs`) [design]:
  - A part runs from its bone to an end joint:
    - when its own bones lead on to other parts, the end is the farthest such
      connecting bone (the torso runs up to the neck);
    - otherwise the end is the farthest `Bip01` bone below the part (an arm reaches the
      fingers, the head the top of the head).
  - Chains skip the skeleton's root bone and `Bip01 NonAccum`. `Bip01`, the torso's
    bone, stands at the feet [observed: the posed segments, 2026-10-09]; a torso started
    there enclosed both legs.
  - Joints closer than 4 units to the last one kept are merged.
  - The renderer publishes the posed segments every frame, after transforms propagate
    (`nvf_play::hitvol::ActorParts`).
- **Radii** (`hitvol::limb_radius`) [design]: a share of the actor's size (the largest
  side of its render bounds):

  | Part | Share | At 128 units |
  |---|---|---|
  | Head | 0.065 | 8.3 |
  | Torso | 0.10 | 12.8 |
  | Arm | 0.042 | 5.4 |
  | Leg | 0.058 | 7.4 |

  Each part's chain is pulled in by its radius at both ends, so the caps end at the end
  joints. Without this, the torso's top cap covered the lower head and took head shots
  [observed]. The torso's lower end is not pulled in: pulled, it left a band at the hips
  that no capsule covered [observed].
- **The player's capsule** (`nvf_play::combat::player_hit_condition`, issue #1345)
  [design]: first and third person use the character controller's feet, height and
  radius, and the player's heading. A hit above 86 % of its height selects the head;
  below 50 % it selects a leg. Between them, more than 60 % of the radius sideways
  from the facing selects an arm; otherwise the torso. Left and right follow the
  player's facing. Height clamps to the capsule; a leg hit exactly on the centre line
  chooses the right leg. The named thresholds follow human skeleton proportions
  [design]; how the original chooses a player's part remains [hypothesis], observer
  check 3. The selected condition actor value finds the part in
  `body_parts_for(PLAYER_BASE, HUMAN_SKELETON)`, the same data the player's limbs
  already follow [design: `combat.rs`]. Missing data keeps location 0 and no limb.
- **Damage** (`nvf-play` combat):
  - The hit's part gives the damage multiplier (`BodyPart::hit_part`; head hits count
    for the unaware-headshot critical).
  - The part gives the `GetHitLocation` number (`BodyPart::location`, the BodyLocation
    enum).
  - The limb damage is the damage dealt without the part's damage multiplier × the WEAP
    Limb Dmg Mult (`Damage::limb_damage`). A multiplier of 0 deals none [hypothesis].
    The original's limb loss over its health damage was Limb Dmg Mult × 100 ÷ (max
    health × health % ÷ 100 × the part's damage mult) for every target and part, over
    3,254 hits [observed 2026-10-25: JIP GetHitLimbDamage and GetHitHealthDamage in the
    OnHit handler; docs/handoff/observer-jip-audit-2026-10-25.md §3.6]. With
    DefaultBodyPartData's head (20 %, ×2), torso (60 %) and limbs (25 %), a head hit
    costs the head the condition of the hit before its ×2; critical bonuses stay in.
    Until 2026-10-25 the multiplier stayed in, so head hits crippled twice as fast.
    Guns only: two machete hits did not fit (×4 on the head, ×2 on a leg), so melee is
    open.
  - Player shots missing a part use the capsule rule above, then `BodyPart::hit_part`,
    `BodyPart::location` and `limb_hit`, including the head's ×2 Health multiplier
    [design: `combat.rs`]. NPC blows on the player use the same rule for location and
    limb, with `HitPart::default()` for Health [hypothesis: melee is open]. Creature
    blows keep their weapon's Limb Dmg Mult, or the Fists' when that path supplies
    them [hypothesis: the unmeasured creature multiplier]. NPC-on-NPC blows keep
    location 0, no limb and their existing Health damage [design: `actor_attacks`].
    Blocked blows use the damage left after blocking; zero damage gives no limb
    loss. God mode stops Health and limb loss [design: `limb_hit`, `hit_player`].
  - Other single capsules without part data keep location 0. Explosions keep
    location −1 and their own limb shares; falls keep their own leg damage
    [design: `combat.rs`, crippled-limbs.md §5, physics.md §6.12].
- **Limb condition** (`Game::hit`, `Game::damage_limb`):
  - The part's condition actor value (25–31, starting at 100) loses
    `limb damage ÷ (max health × health % ÷ 100) × 100`, and stops at 0.
  - The limb is crippled when it reaches 0 [documented: GECK wiki Body Part Data,
    crippled after that share of total health; hypothesis: the count-down form].
  - `HitResult` reports the condition and whether this hit crippled the limb. The log
    and the HUD name it ("Goodsprings Settler: left leg crippled").
- **The player's crippled limbs** (`PlayerLimbs`, read every frame):
  - crippled arms add the documented crippled-arm spread terms (§3) and lower
    `Attacker::arm_condition` by the weapon (`damage::arm_condition`, for every
    attacker: crippled-limbs.md §3);
  - crippled legs slow walking and running to `fMoveOneCrippledLegSpeedMult` (0.85)
    with one leg and `fMoveTwoCrippledLegsSpeedMult` (0.75) with both, as for every
    actor (crippled-limbs.md §4; 0.7 and 0.5 [design] before 2026-10-18);
  - the HUD's health line lists crippled limbs.
  - What every crippled limb does, to the player and to NPCs: crippled-limbs.md.

  Ordinary NPC shots and blows now damage the player's limbs by default, as well as
  explosions, falls and scripts [design: `combat.rs`, issue #1345].
- **Hit reactions** (`damage::staggers`, `deaths.rs`) [design: the owner's rule,
  2026-10-11; before it every hit played one, and fast hits held an actor in it]:
  - The hit that cripples a body part always plays one.
  - A later hit on a part that is already crippled plays one when the actor's last
    reaction ended at least `stagger_cooldown` seconds ago (3 s; one clock per actor,
    whichever part was hit).
  - A power attack that breaks a block (below) always plays one.
  - Any other hit plays none: a hit on a healthy part, or a blast (no part).
  - The clip is the one for where the hit landed: `mt_hithead`, `mt_hittorso`,
    `mt_hitarmleft`/`right` or `mt_hitlegleft`/`right` (`idleanims`, the `_male`
    folder) [observed: the clips exist; design: which location plays which].
  - The log names each one ("hit reaction: … staggers").
- **Power attacks break blocks** (`damage::against_block`) [design: the owner's
  request, 2026-10-16; no page documents the original's rule]: a power attack (the
  player's, or a creature's such as the radscorpion's sting) that meets a blocker from
  its front is not reduced by the block. All of it is dealt, the attacker does not
  recoil, and the blocker does not play its block hit. A blocking NPC staggers instead
  (its torso hit reaction, whatever part was hit); the stagger ends its block and holds
  its attacks while it plays (combat-ai.md §14.3). The player has no stagger clip, so
  a broken block means it cannot block for `guard_break` seconds (1 s). The log says
  "breaks …'s block".
  - Checked [observed 2026-10-11, `nvforever --play`, exit 0, "run finished"; the
    Goodsprings Settler 0x00104F0A, 9mm pistol, `aimpart=0` and `aimnpc`]:
    - Four torso hits and an arm hit on healthy parts played nothing.
    - The fifth torso hit crippled the torso and played `mt_hittorso` (1.4 s).
    - The hit that crippled the head played `mt_hithead` (4.5 s); a second head hit
      1.2 s later played nothing.
    - A head hit 3.4 s after the crippling one played the reaction again. That run
      counted the cooldown from the reaction's start; counting from its end was not
      seen in play (the settler walks behind a wall before it runs out) and rests on
      the rule's test.
- **Test aids:**
  - `aimpart=<n>` aims at the nearest NPC's part `n`;
  - `aimnpc=<share>` aims at a share of its height;
  - `aimactor` / `aimnpc` now aim at the torso when the actor has parts. The exit
    probe does the same.
- **Not built:**
  - creature parts are not checked in play (no creature in range of the test);
  - a creature's torso may leave gaps (a tail);
  - crippled NPCs do not limp or drop weapons (no NPC movement or AI);
  - the head's screen effect is not shown;
  - perks (Adjust Limb Damage) are not read;
  - severing and exploding limbs (gore) are later.
- **Checked** [observed 2026-10-09, `nvforever --play`, exit 0, "run finished"]:
  - All 13 actor volumes had body parts. The exit probe at each aim point hit its own
    actor for 11–12 of 13 (12 of 13 with the single capsule in C3).
  - The Goodsprings Settler (55 health, DT 2), shot with the 9mm pistol through
    `aimpart`:
    - Head shots: 16.4 damage (×2), and one hit crippled the head (pool 11).
    - Leg shots: 7.2 damage; a second hit crippled the left leg (pool 13.75), and the
      leg hit reaction played.
    - Arms and torso were hit as aimed. The torso was crippled after five hits (pool
      33).
    - Some shots missed while the settler walked and flinched (a capsule about 15 units
      across, at 150 units with pistol spread).

## 13. Open points

- Every [design] number is a first guess for the owner to tune in play.
- Projectile gravity and speed: the Grenade Launcher's round matches the PROJ speed × the 686.7 rule (§5, o26_gl), and ours now leaves the muzzle (PR #1788). A thrown frag grenade went about 950 units/s across the ground, not its PROJ 1,200, and slowed in the air; ours still throws at the PROJ speed, and now rolls as the original's did (§5, PR #1788).
- `fCombatDistance`, the melee reach base: not in the masters and not documented; 107
  is fitted to the NPCs' swings (§10). The player's own reach in the original is not
  measured.
- Not here yet: weapon mods (Decrease Spread and the like), fatigue, reading perks,
  gore. All are later steps (combat.md §8).
- Limb radii, the crippled-leg speeds and the arm-condition share are [design] (§12).
  The owner tunes them in play.

## 14. Shots on objects (bottles, clutter)

What a shot does to a placed object that is not an actor. Found with the tutorial's
bottles (quest `VCG02`, "Back in the Saddle"), which a shot neither counted nor moved.

- **The bottles' script** [observed: `VCG02TargetSCRIPT` on MISC `VCG02Bottle`, seven
  references on the fence behind the Prospector Saloon, enabled with
  `VCG02BottleMarkerREF` by Sunny's line 0x0010A1E6]:
  - `Begin OnHitWith` without a parameter counts the hit when all of these hold:
    `VCG02` is running, this bottle was not counted before, `player.IsWeaponOut == 1`,
    `player.GetWeaponAnimType` is above 3 and below 9, and the player does not have the
    Codac R9000 camera equipped.
  - A counted hit makes Sunny say `VCG02SunnyShotReaction`, adds 1 to
    `VCG02.nTargetCount` and sets `CGTutorial` stage 62. The third completes objective
    10 and re-evaluates Sunny's package.
- **Hit blocks on objects.**
  - The target of OnHit and OnHitWith is "any reference" [documented: GECK wiki Event
    Handling, the owner's export], and they run before damage is dealt.
  - `Game::hit_object` runs the reference's OnHit blocks with the attacker and its
    OnHitWith blocks with the weapon; nothing else changes. The engine calls it for
    every shot that lands on a placed object.
  - [hypothesis] OnHit's subject is the attacking actor. That page gives a projectile
    reference for a ranged hit on a non-actor; we have no projectile references.
  - Melee blows only strike actors so far (§10), so they run no object's blocks.
- **`IsWeaponOut`** [documented: catalogue]: 1 while the actor has a weapon out; the
  value changes when the weapon is attached to the weapon node. The engine reports the
  player's (`WeaponAnim::ready`: drawn and in hand). No one reports other actors yet:
  they read 0.
- **`GetWeaponAnimType`.**
  - [documented: GECK wiki GetWeaponAnimType] it tells 11 animation types apart and has
    one catch-all value; several of the record's 13 (Fallout 3) or 14 (New Vegas) types
    share a value. The page's table is a template that is not in the owner's export,
    and the site refuses automated readers.
  - [hypothesis] the numbering in `functions::weapon_anim_code`:

    | WEAP DNAM animation type | value |
    |---|---|
    | 0 Hand to Hand | 1 |
    | 1 Melee (1 Hand) | 2 |
    | 2 Melee (2 Hand) | 3 |
    | 3 Pistol - Ballistic, 4 Pistol - Energy | 4 |
    | 5 Rifle - Ballistic, 7 Rifle - Energy | 5 |
    | 6 Rifle - Automatic | 6 |
    | 8 Handle | 7 |
    | 9 Launcher | 8 |
    | 10 Grenade Throw | 9 |
    | 11 Land Mine | 10 |
    | 12 Mine Drop | 11 |
    | 13 Thrown, anything else | 0 |

    No weapon equipped reads 1 (the actor fights with its hands).
  - What it rests on [observed: shipped scripts and records, `FalloutNV.esm`]:
    - `GetWeaponAnimType == 4` is tested together with `GetEquipped NVWeapMS22Camera`
      (SCPT 0x001136AE); that weapon's record type is 3. So the values are not the
      record's.
    - Four weapon-alarm scripts test `IsWeaponOut == 1 && GetWeaponAnimType != 1`:
      raised fists are no alarm, so Hand to Hand is 1.
    - Two scripts treat `>= 2` as armed and `<= 1` as unarmed.
    - The bottles' `> 3 && < 9` is then every gun (4 to 8) and nothing else.
    - 13 types in 11 values needs two pairs to share. The pistols are one (above). The
      rifles as the other is a guess.
  - The bottles count the Varmint Rifle (record type 5) and the 9 mm Pistol (3) whether
    or not the rifle guess holds: both land in 4 to 8 either way.
- **The push** [design]:
  - A shot strikes the loose (dynamic) body it lands on at the point of impact, along
    its flight: `PhysWorld::push_ref`.
  - Impulse: the projectile's Impact Force (PROJ DATA) times `OBJECT_PUSH` (1), read
    as mass × metres per second in the stored collision's units [hypothesis: the
    field's unit is not documented]. The masters' bullets carry 3 to 6.
  - The speed gained is the impulse over the body's mass, at most
    `OBJECT_PUSH_MAX_SPEED` (700 units/s). The turn it gives is bounded as a ragdoll's.
  - A bottle (mass 1) hit by the Varmint Rifle (Impact Force 3) leaves at 210 units/s.
  - Fixed and animated collision is not pushed.
- **Two faults found on the way.**
  - A sleeping body that was struck and then queried before the next physics step never
    moved (physics.md §5, rapier quirk). Explosions' pushes had the same fault.
  - Objects a script enables during play (`script_draw.rs`) were not redrawn where
    physics moved them; they are now (`script_draw::follow`).
- **Test aids:** `NVF_PLAY_TEST_CONTROL` `aimref=<FormID>` aims at a placed object's
  body; `NVF_PLAY_TEST_NO_SPREAD=1` makes shots leave exactly along the view.
- **Checked** [observed 2026-10-10, `nvforever --play`, exit 0, "run finished"]: with
  `VCG02` started and Sunny's line 0x0010A1E6 said, the player drew the Varmint Rifle
  and shot bottles 0x0010A202, 0x0010A203 and 0x0010A204 from 260 units.
  - Each shot ran the bottle's OnHitWith block and Sunny said a reaction line.
  - The first set `CGTutorial` stage 62; the third completed `VCG02` objective 10.
  - Each bottle left the fence at 210 units/s; one came to rest on the ground 94 units
    from where it stood.

  ```
  NVF_PLAY_TEST_NO_SPREAD=1 NVF_PLAY_TEST_GOTO=1:-68375:4800:8600
  NVF_PLAY_TEST_SCRIPT=3:start:VCG02,4:info:0x0010A1E6:SunnyRef
  NVF_PLAY_TEST_CONTROL=7:weapon,10:aimref=0010A202,10.5:fire,10.7:fire,13:aimref=0010A203,13.5:fire,13.7:fire,16:aimref=0010A204,16.5:fire,16.7:fire
  nvforever --data <Data> --cell Goodsprings --play --pos=-68375,4800,8700 --run-seconds 22
  ```
- **Open:** the `GetWeaponAnimType` table (owner-review.md); how hard the game's shots
  push clutter (the planned projectile captures); impact effects and sounds.

## 15. What is drawn of a shot: muzzle flash and projectiles

Owner request 2026-10-10: "actual bullet projectiles that aren't invisible along with
the muzzle flash effects". Where a shot is drawn from is docs/design/combat.md §5.1 and
animation.md §9.9.

- **The records** [documented: xEdit PROJ; GECK wiki Projectile, §4.2 of combat.md]:
  a PROJ has a model, a tracer chance ("how often the projectile is drawn"), a fade
  duration (beams), and with its Muzzle Flash flag a flash model (`NAM1`), a flash
  duration and a flash light (LIGH). Read with `fnvtool combat weapon` [observed]:

  | Weapon | PROJ | Type, flags | Speed | Tracer chance | Model | Flash model, duration |
  |---|---|---|---|---|---|---|
  | 10mm pistol | `10mmBulletProjectile` | Missile, Hitscan | 27,520 | 0 | `projectiles\10mmprojectile.nif` | `handgunmuzzleflash01.nif`, 0.04 s |
  | Minigun | `MinigunTracerProjectile` | Missile, Hitscan | 7,000 | 0.3 | `projectiles\762mmprojectile.nif` | `minigunmuzzleflash01.nif`, 0.04 s |
  | Laser rifle | `BeamLaserProjectile` | Beam | 10,000 | 0 | `projectiles\testlaserbeamsteady.nif` (fade 0.14 s) | `laserriflemuzzleflash.nif`, 0.14 s |
  | Missile launcher | `MissileProjectile` | Missile, Explosion | 1,550 | 0 | `projectiles\mlprojectile.nif` | `assaultriflemuzzleflash.nif`, 0 s |
  | Plasma rifle | `PlasmaProjectile` | Missile | 7,500 | 0 | `projectiles\plasmaprojectile01.nif` | `plasmariflemuzzleflash.nif`, 0.12 s |
  | Flamer | `FlameProjectile` | Flame, Hitscan | 12,000 (range 640) | 0 | `projectiles\flamerflame01.nif` | `flamermuzzleflash01.nif`, 0.2 s |
  | Frag grenade | `GrenadeFragProjectile` | Lobber, Explosion | 1,200 | 0 | the grenade's own model | none |

- **The models** [observed: `crates/fnv-nif/examples/effect_model_survey.rs` on
  `Fallout - Meshes.bsa` and `Update.bsa`; 63 files under `meshes\projectiles`, 16
  under `meshes\effects\muzzleflashes`]:
  - they are built along +y: the 10mm bullet fills y 0 to 1.5 (1 unit across), the
    missile y −55.7 to 12.7 with its flame behind, the laser beam y −2.7 to 2048 with
    a node `BeamEnd` at (0, 2048, 0), the handgun flash y −5 to 31.7 and the minigun's
    y −10.6 to 77;
  - `762mmprojectile.nif` holds a bullet (`Projectile`, 0.8 by 3 units) and a streak
    244 units long and 4 across under a `NiBillboardNode` named `Tracer`;
  - `10mmprojectile.nif` holds one shape, the bullet, under a node `10mm`, beside
    empty nodes named for other calibres;
  - `NiBillboardNode`: in 10 of the 63 projectile models and 13 of the 16 flashes, 31
    nodes in all, with billboard mode 1 (12), 4 (13) and 2 (6). The mode-1 nodes of the
    files looked at hold geometry that is long in y (the tracer streak, the laser
    beam), and the handgun flash's mode-1 node is turned a quarter turn about y in the
    file, the others not at all;
  - they also hold `BSValueNode`s named `AddOnNode<n>` (two in each bullet flash,
    three behind the missile, four in the plasma bolt), and the flash planes have a
    `NiTransformController` with no sequence in the file.
- **Readings** [hypothesis]: +y is the models' forward axis; the flash model is
  attached at the weapon's `ProjectileNode` with the node's rotation (whose y axis lies
  along the barrel, animation.md §9.9); `BeamEnd` marks the beam's far end so the model
  can be stretched to the beam's length; a billboard node shows its +z side and, in the
  "rotate about up" modes, turns about its own y [documented: the mode names, nif.xml
  BillboardMode]. How the game draws a hitscan bullet is not known.
- **Engine** (since 2026-10-10):
  - combat sends a `ShotFired` (the PROJ) for each shot and lists every projectile to
    draw in `ShotVisuals` (PROJ, place, direction; `nvf_play::combat`):
    - a projectile stepped in flight (missile, plasma, grenade): where it is seen
      (from the muzzle, closing on its path);
    - one whose hit is found at once (hitscan) [design]: a bullet flying from the
      muzzle to where it landed at the PROJ's speed, gone on arrival;
    - a beam [design]: from the muzzle to its end, for the PROJ's fade duration. Its
      start stays on the muzzle while it shows, and so does the start of the line
      along a hitscan shot: the attack clip moves the gun right after the shot
      [observed: the laser rifle's `ProjectileNode` is 10.3 units higher and 4.4 to
      the side one frame after it fires, and stays there], and a beam left where the
      muzzle was hung below the barrel (the owner: "that laser beam looks too low");
    - every projectile is listed, not the tracer chance of them
      (`DRAW_EVERY_PROJECTILE`) [design: the owner's request];
  - `nvforever::shot_fx` draws them:
    - the flash model under the `ProjectileNode` entity of the rig in view
      (`PlayerBody::muzzle_entity`), for the PROJ's flash duration and always for the
      frame it is made in, with a point light of the LIGH's colour, fade and radius
      (no shadows);
    - the PROJ's model at each listed projectile, +y along the flight; a beam's model
      scaled along y by its length ÷ `BeamEnd`'s y;
    - billboard nodes turned to the camera each frame (`ModelNode::billboard`);
    - the models of the weapon in use are loaded when it is equipped, so the first
      flash is not lost to loading;
  - a weapon without a `ProjectileNode` (or a run without a player body) shows no
    flash; its projectiles are still drawn.
  - another actor's flash (since 2026-10-20) goes under the muzzle marker of the
    weapon drawn on it (`weapons::Muzzle`: the same `ProjectileNode`, placed under the
    actor's `Weapon` node where it is in the model at rest), so it moves with the gun
    and its +y lies along the barrel as the player's does [same hypothesis]; an actor's
    weapon without the node puts it where the shot left, its +y turned along the shot
    [design]. The flash and projectile models of a weapon drawn on an actor are loaded
    when it is first drawn. Creatures fire no guns yet (their attacks are blows,
    docs/design/combat-ai.md), so turrets and robots show no flash.
- **Checked** [observed 2026-10-10, `--play` at Goodsprings, `--hour 12`, third person
  from the side (`orbit=75:5` at the screenshot's time), exit 0 and "run finished";
  screenshots kept privately]:
  - Minigun, trigger held: "46 muzzle flashes (46 with a light), 46 projectile models
    drawn, 0 projectiles without a model". During the burst one flash stands at the end
    of the barrel, pointing along it; 0.4 s after the trigger is let go there is none.
    A frame of another run shows a tracer streak in flight ahead of the barrel.
  - Laser rifle, one shot: "BeamLaserProjectile drawn at (−71672.1, 2091.8, 8437.5),
    a beam 628 units long"; the red beam runs along the shot from the muzzle, and
    0.1 s after the shot still starts at the muzzle, which has risen since.
  - The muzzle's place worked out from the pose (`Rig::muzzle_at`) and the drawn
    node's place a frame later agree to 0.1 unit in every frame logged, standing and
    through a shot.
  - Before the flash time was the record's alone it was held to at least 0.05 s, and
    the test aid held it for seconds: the owner saw the minigun's flash "sticking
    around way too long". Now 0.04 s is one frame at 25 frames a second.
- **Controllers of the effect models** (2026-10-19; the owner: the laser rifle's beam
  "looks static" beside the game's). Every beam PROJ of FalloutNV.esm and its models,
  base archives only [observed: `fnvtool combat form`, `fnvtool nif decode`; flags as
  nif.xml TimeControllerFlags: bit 0 APP_INIT, bits 1-2 cycle (0 loop, 2 clamp), bit 3
  Active, bit 5 manager controlled]:

  | PROJ (used by) | Fade Duration | Beam model | Its controllers |
  |---|---|---|---|
  | `BeamLaserProjectile` (laser rifle and others), `BeamLaserAutoProjectile` (0.10 s), `ARCHIMEDESIIProjectile` (10 s) | 0.14 s | `projectiles	estlaserbeamsteady.nif` | `LaserGeometry:1` (`FXEnerygTiler.dds`): `NiTextureTransformController` TT_TRANSLATE_V 0 → 1 over 3.67 s and TT_TRANSLATE_U 0 → −1 over 0.5 s, flags 72 (Active, loop). No alpha controller; material alpha 1 |
  | `BeamPewPewProjectile` | 0.14 s | `testlaserbeamsteadyyellow.nif` | the same two as the rifle's |
  | `ProtectronHeadLaserProjectile` | 0.10 s | `laserprotectronhead01.nif` | V 0 → 1 / 0.5 s on one shape, V 0 → 1 / 3.67 s and U 0 → −1 / 0.5 s on the other; flags 72 |
  | `DLC03TeslaChainProjectile` (×4) | 0.5 s | `dlc03teslachainbeam.nif` | the rifle's pair, and V 0 → 1 / 0.23 s on the glow; flags 72 |
  | `DLC03TeslaCannonProjectile` | 0.25 s | `dlc03teslamainbeam.nif` | V 0 → 1 / 0.5 s, V 0 → 1 / 0.1 s and U 0 → −1 / 2.83 s; flags 72 |
  | `ARCHIMEDESIProjectile` | 1.0 s | `fxhelios_laserbeam.nif` | the rifle's pair on two shapes; flags 72 |
  | `BeamLaserGreenProjectile` | 0.14 s | `nv\weaponeffects\lasergreenprojectile.nif` | V 0 → −1 / 0.67 s and `NiAlphaController` 1 (to 0.033 s) → 0 (0.2 s); flags 69 (APP_INIT, clamp, **not** Active) |
  | `PulseGunProjectile` | 0.75 s | `nv\weaponeffects\pulsegunprojectile.nif` | alpha 1 → 0 by 0.2 s on both beam shapes, V 0 → −1 / 0.47 s; flags 69 |
  | `DLC03ScatterBeam` | 0.14 s | `dlc03scatterbeam.nif` | none |

  The flash models have them too (played since the explosion-effects work):
  `laserriflemuzzleflash.nif` shrinks its glow's billboard node (scale 1 → 0.8 by
  0.13 s, an `NiTransformController`, flags 72) and scrolls two burst planes along U and
  V; `lasergreenmflash.nif` moves its root (flags 69). The Gauss rifle's, the plasma and the sonic PROJs are Missiles, not beams.
  The beam is scaled along y by its length ÷ 2048 (`BeamEnd`); the V of
  `LaserGeometry:1` runs along it (about 1.9 repeats of the texture over the 2048 units)
  and U across it (about 0.32 over the 9 units of each half).
- **Readings** [hypothesis]: an effect model plays all its controllers from the moment
  it is made, the inactive APP_INIT ones included (docs/spec-notes/explosion-effects.md
  §2; the green laser's and pulse gun's alpha fades are shaped as the beam's own short
  life), each material controller on its own cycle (`OwnCycle`), so the beams' looping
  scrolls keep scrolling while the beam shows; a texture offset moves the texture by its
  value (`uv + offset`, as for placed models, activators.md §2). The game stretches the
  beam's model as we do, so the texture and its scroll along the beam stretch with it (a
  500-unit beam shows the same 1.9 repeats as a 2048-unit one, and scrolls 0.24 times as
  fast in units per second); whether the game rescales the UVs to keep the texture's
  size instead is not known. **Fade Duration** is the time a beam stays after the shot,
  over which it fades from full to nothing in a straight line (alpha × left ÷ Fade
  Duration; the beams are additive, so all they add fades with it); a beam whose model
  fades itself (an alpha controller) is drawn at the model's alpha alone. The meaning of
  Fade Duration is from its name and the beam staying that long; the straight line is
  [design].
- **Engine** (2026-10-19, `nvforever::shot_fx`): a projectile's model is spawned as an
  effect (`spawn_effect`, its rig playing `Rig::effect` as flashes, impacts and
  explosions do), so the beams scroll. Combat gives each drawn shot a strength
  (`ShotVisual::strength`, `beam_strength`); for a beam whose clip has no alpha track,
  every shape of the model gets a material slot and `RigMaterials::fade` multiplies the
  strength into their alpha. A beam model with no controllers at all
  (`dlc03scatterbeam.nif`) has no rig and is drawn at full strength until it goes. The
  exit summary counts the beams played and faded (smoke `combat-beam`).
- **Not built:** the PROJ's own light, NPCs' shots. Add-on nodes are docs/spec-notes/particles.md; sounds, impacts and shell
  casings are §16. Not looked at in a run: the 10mm bullet, plasma bolt and grenade
  models (their PROJs go the same way).
- A model is not shown until it has flown the length it reaches behind its origin
  [design]: the minigun round's streak (220 units behind the bullet) ran back through
  the gun and the shooter in its first frame [observed].
- **Open:** whether the game draws a hitscan bullet at all outside its tracer chance,
  and at what speed; whether a billboard shows +z or −z (the flash glows and the beam
  show with +z); the flame's model has no shape of its own (add-on nodes only): its
  flame is particles (particles.md).

## 16. Sounds, impacts and shell casings

Owner request 2026-10-11: build what §15 left out. Particle effects are
docs/spec-notes/particles.md.

- **The records** [documented: xEdit; schemas/fnv `objects.json`, `combat.json`,
  `placed.json`], read by `fnv_combat::fx` (`fnvtool combat fx --form <weapon>` prints
  a weapon's):
  - WEAP: `SNAM` twice (Shoot 3D, Shoot Dist), `XNAM` Shoot 2D, `NAM7` Shoot 3D
    Looping, `TNAM` Melee Swing / Gun No Ammo, `NAM9` Equip, `NAM8` Unequip, `INAM`
    impact data set, `MOD2` shell casing model (read from the raw subrecord: the WEAP
    layout in the schema does not list it);
  - SOUN: `FNAM` a file or a folder under `sound\`, `SNDD` flags, minimum and maximum
    attenuation distance (stored ÷ 5 and ÷ 100), static attenuation in hundredths of a
    decibel, frequency adjustment in percent;
  - PROJ `DATA` sound; EXPL `DATA` sound 1, sound 2, impact data set;
  - IPDS: an IPCT for each of twelve materials (stone, dirt, grass, glass, metal, wood,
    organic, cloth, water, hollow metal, organic bug, organic glow);
  - IPCT: model, effect duration, effect orientation (Surface Normal, Projectile
    Vector, Projectile Reflection), texture set, sound 1, sound 2.
  - Read [observed, FalloutNV.esm]: 3,190 SOUN, 125 IPCT in 60 IPDS, 37 ADDN.
- **Examples** [observed]: the 10mm pistol's Shoot 2D is `WPN10mmFire2D`, the folder
  `fx\wpn\10mm_pistol\fire_2d\` (flag 2D, static attenuation 13.22 dB); its No Ammo
  `WPNPistol10mmFireDry` is one `.wav`; its casing `Projectiles\45CalCasing.NIF`; its
  impact data set gives `BallisticStoneImpact` (`Effects\ImpactBallisticConcrete01.NIF`,
  0.25 s, Projectile Reflection) for stone, `BalisticDirtImpact` for dirt and grass, and
  `BallisticOrganicImpact` (blood, 0.08 s, Surface Normal) for organic.
  `fGunShellEjectSpeed` is 160.
- **Sounds** (`nvf_play::sfx`):
  - a `PlaySound` names a SOUN and, for a sound with a place, a point. A folder stands
    for the `.wav` files directly in it, one picked at random [hypothesis];
  - WAV is decoded here (PCM of 8, 16 and 24 bits, 32-bit float) and played through
    Bevy's audio as a source of our own; another encoding is reported once;
  - loudness [design]: gain 10^(−static attenuation ÷ 20); with a place, full within
    the minimum distance of the player's eye, then minimum ÷ distance, fading to
    nothing at the maximum distance. Not panned. The record's attenuation curve is not
    used;
  - pitch: the frequency adjustment, and ±5% at random with Random Frequency Shift
    [design: the size];
  - combat plays: Shoot 2D (else Shoot 3D) for each shot of the player's [hypothesis:
    that the 2D sound is the shooter's own]; another actor's Shoot 3D at its muzzle, or
    Shoot Dist beyond the 3D sound's maximum distance; a silenced or suppressed weapon's
    mod sounds (`WMS1`, `WMS2`) instead (weapon-mods.md §7); the Shoot 3D Looping sound going round
    from a weapon's first shot until 0.25 s after its last [hypothesis: when; design:
    the 0.25 s]; No Ammo on an empty trigger; Equip and
    Unequip when the player starts to draw or put away [hypothesis: when]; an impact's
    sounds at the impact; an explosion's at the explosion;
  - `nvforever --no-sound` turns them off.
- **Impacts.** A shot that lands takes the IPCT its weapon's impact data set gives for
  the surface:
  - the collision material of the triangle hit [documented: nif.xml
    `Fallout3HavokMaterial`, 32 materials repeated as platforms and stairs] is mapped
    to one of the twelve by its name [hypothesis: `fnv_combat::fx::impact_material`];
    terrain, which has no code, counts as dirt [design]; an actor takes its base's
    Impact Material Type (NPC_ / CREA `NAM4`) [documented: xEdit], organic without one
    [design]; convex shapes' materials are read too since 2026-10-11 (bottles had
    counted as dirt; audio.md §11.1 has the table by surface);
  - `nvforever::shot_fx` shows the IPCT's model at the point for its effect duration,
    its +z along the orientation the IPCT names [hypothesis: +z, from the impact
    models' add-on nodes standing along +z]. The model's add-on nodes emit dust and
    debris (particles.md) while the model is there; the particles they gave birth to
    live out their own life spans after it goes (particles.md §2 "Master system").
    On an actor the organic IPCT (`BallisticOrganicImpact`, 0.08 s) gives
    `MPSBloodSplat01`'s drops, mist and splat: about 18 drops, 5 puffs and 7 splats a
    hit [observed: NVForever log, #1074], the puffs living about 1 s. They were born but
    not seen until their soft intersection was sized to them (nif.md §6 "Soft
    intersections of particles").
- **Shell casings.** The player body gives the weapon model's `ShellCasingNode` place
  and its +z axis to combat (`WeaponAnim::casing`). Each shot of a weapon with a casing
  model throws one along that axis at `fGunShellEjectSpeed`: at the shot, or, when the
  attack clip has an `Eject` text key, when the clip reaches it (the body counts it,
  `WeaponAnim::ejects`). The renderer starts it at the node as drawn
  (`PlayerBody::casing_entity`): in first person the arms are drawn through a narrower
  view (`place_lens`), so the posed node is not where the weapon is seen. It falls under
  the world's gravity, bounces (keeping 0.35 of its speed [design]) and is removed
  after `fGunShellLifetime`.
  - For +z as the way out [observed: `muzzle_node_survey --list`]: in the 59 models
    whose node is turned by no right angle, its z axis points to the side the node sits
    on and up (9mm (−0.38, 0.36, 0.85), service rifle (0.17, 0.59, 0.79)); the cowboy
    repeater's points up (lever actions throw up).
  - The `Eject` key [observed: `fnvtool nif decode` of the base archive's clips]: on
    the attack clips of the lever, bolt and pump actions (`2hrattack3/5/7/8/9` and
    their forms, 87 files), after `Hit`. `_1stpersonhrattack8.kf` (the varmint rifle,
    WEAP attack animation `Attack8`): `Hit` 0.033 s, `Sound:
    WPNRifleHuntingReloadChamber` 0.600 s, `Eject` 0.800 s, `a:8` 1.267 s, end 1.333 s;
    `2hrattack8is.kf` ejects at 0.767 s. In the original the varmint rifle's casing is
    first seen as the bolt comes back [observed: owner's screenshot, 2026-10-20]. Clips
    without the key throw at the shot [hypothesis].
  - Checked [observed 2026-10-21, smoke `varmint-casing-eject`]: the casing left 0.80 to
    0.82 s after the shot; at the eject the varmint rifle's node +z pointed (0.99 right,
    −0.03 up, 0.14 ahead) of the view (the clip pitches the rifle up and turns it left,
    it does not roll it), so the casing goes right off the receiver beside the bolt
    hand and falls, as in the owner's screenshot of the original. Ours shows lower on
    the screen because the arms rig holds the rifle lower in the view.
- **Checked** [observed 2026-10-11, `--play` at Goodsprings, exit 0, "run finished"]:
  - 10mm pistol: "sfx: 4 sounds played": `WPNPistol0Equip`, `WPN10mmFire2D` (gain
    0.218), `FXBulletImpactEarth` at 656 units (gain 0.276, `BalisticDirtImpact` on
    terrain), `WPNPistol0EquipUn`. No file failed to decode. Nobody listened: the check
    is that the files are found, decoded and handed to the audio output.
  - Minigun burst: "40 shell casings, 40 impact effects"; impacts named concrete, dirt
    and wood by surface; casings fall to the right of the gun and lie on the ground.
  - The minigun has neither a Shoot 2D nor a Shoot 3D sound [observed: `fnvtool combat
    fx --form WeapMinigun`]: its only shoot sound is the loop `WPNMinigunSpinLPM`
    (`fx\wpn\minigun\wpn_minigun_spin_lpm.wav`). Where its gunfire sound comes from is
    open [hypothesis: the attack clip's text keys].
- **Not built:** the Shoot Dist
  sound, panning and reverb, sounds named by animation text keys (reloads), the PROJ's
  own sound (a bullet passing), impact decals (the IPCT's texture set), the animations
  inside impact models (they grow and fade), explosion models, casings hitting the
  ground with a sound.
- **NPCs' shots** are not built and cannot be on their own: NPCs do not attack yet (no
  combat packages, detection or combat styles; docs/design/combat.md §8). What a shot
  shows and sounds like is ready to be used by them: the messages carry places, not
  the player.

## 17. Item condition: wear, armour, broken weapons

Owner's request, 2026-10-11. Code: `fnv_combat::condition`, `fnv_sim::inventory`,
`engine/nvf-play/src/combat.rs`.

- **What condition is.** A fraction of the item's full health, kept with the carried
  item (`Extra::condition`, simulation.md §3). Full health is the record's: WEAP and
  ARMO `DATA` health [documented: xEdit, combat-data.md §1].
- **Settings in FalloutNV.esm** [observed, `fnvtool esm decode`]:

  | Setting | Value | Used |
  |---|---|---|
  | `fDamageToWeaponValue` | 0.2 | health a weapon loses per attack |
  | `fDamageToArmorPercentage` | 0.35 | share of a hit's damage the armour loses |
  | `fDamageToWeaponGunMult`, `…EnergyMult`, `…LauncherMult`, `…MeleeMult` | 0.03, 0.04, 0.06, 0.05 | no |
  | `fVATSDamageToWeaponMult` | 1.0 | no |
  | `fDamageGunWeapCondBase`, `fDamageGunWeapCondMult` | 0.66, 0.34 | no |
  | `fWeaponConditionRateOfFire1`–`9` | 0.82 to 0.98 in steps of 0.02 | no |
  | `fWeaponConditionJam1`–`4`, `fWeaponConditionSpread1`–`9` | 0 | no |

  What each of them does in the original is not documented anywhere we can cite. The
  two we use are read by `Rules` like the other settings.
- **Weapon wear.** Each attack takes `fDamageToWeaponValue` health points, or the
  record's Damage to Weapon Mult in its place when its override flag is set, with the
  ammunition's Weapon Condition effects applied (`condition::weapon_wear`).
  - [documented: GECK wiki SetWeaponDegradationMult (ppNVSE): the record's value
    "overrides the fDamageToWeaponValue when calculating the per shot degradation", and
    its example's 0.5 "will degrade 0.5 per shot".] Until 2026-10-20 ours multiplied the
    two (0.2 × 0.005 for the Incinerator); the override now replaces the setting. The
    GECK wiki's Weapon page calls the field the multiplier "that determines the amount of
    damage the weapon will take", 0 for none [documented]; it does not say of what.
  - FalloutNV.esm sets the override on twelve weapons, all automatic or fast: the
    Flamer, Heavy Incinerator and Incinerator (health 200), the grenade machineguns,
    the chainsaw, the Rippers, the Thermic Lance and Euclid's C-Finder at 0.005, the
    binoculars and the BB gun at 0 [observed: `fnvtool esm decode`, every WEAP].
  - It applies to the player's weapon. An NPC's own shots and landed blows wear its
    weapon only when the weapon's record sets its own Damage to Weapon Mult (since
    2026-11-06, #1334; `nvf_play::combat::npc_weapon_wears`) [observed: the weapons
    without it did not wear (o14_reach, o15_round, below), Cook-Cook's Incinerator, which
    has it, did; hypothesis: that the override is what decides]. A shot from a scripted
    FireWeapon wears nothing [design].
  - [hypothesis: the setting's role.] It fits the published figure the owner quoted:
    "about 395 standard rounds, the equivalent of 33 reloads" for the 10mm Pistol from
    full condition. Its record has health 80 and 12 rounds [observed]: 80 ÷ 0.2 = 400
    shots, 33 full magazines.
  - The figure is 5 shots short of 400, which is exactly one health point
    (79 ÷ 0.2 = 395). [hypothesis, not built: the original may count a weapon with
    less than one point as broken.] Ours breaks at zero.
  - A shot wears the weapon whether it hits or not. A melee or unarmed swing wears it
    only when it lands, once however many it strikes [design]. Thrown weapons are
    spent, not worn. The Fists are not an item and do not wear.
  - The per-class multipliers (gun, energy, launcher, melee) and the weapon's damage
    play no part [hypothesis: with them the 10mm Pistol would not come out near 395].
- **Armour wear.** A hit takes `fDamageToArmorPercentage` of its damage before armour
  from the armour it lands on, as health points (`condition::armor_wear`)
  [hypothesis: the setting's role, from its name].
  - Where it lands [design: the owner's rule]: a hit on a head part (Head 1, Head 2,
    Brain) wears what is worn on the head, any other hit what is worn on the upper
    body, an explosion both (`condition::Zone`, `CombatForms::armor_struck`).
  - "On the head" is any of the biped slots Head, Hair, Headband, Hat, Eye Glasses and
    Mask; "on the upper body" is the Upper Body slot [documented: xEdit's slot names;
    observed: a power armour helmet sets exactly those six, body armour sets Upper
    Body].
  - Pieces struck together share the loss evenly. A head hit on a bare head, or a body
    hit on an actor with no body armour, wears nothing.
  - A target that takes no damage (dead, a child, invulnerable) does not wear.
  - **Only the player's armour wears.** In the original an NPC's armour kept its
    condition through a whole fight: Cook-Cook's stayed at health 85 and his Damage
    Threshold at 13 through 60 hits, the troopers' and Fiends' likewise [observed: AI
    observer campaign `fight_stats_round`, 2026-10-19, npc-behaviour-compare.md §9]. An
    NPC's weapon does wear (Cook-Cook's Incinerator lost 2.19 health a shot) [observed,
    same run]. NPC **melee** weapons did not visibly wear: knife (health 150),
    sledgehammer, super sledge, lead pipe (50) and bumper sword troopers swinging at the
    god-mode player 12-16 times, about 13 blows landing each, all stayed at
    GetWeaponHealthPerc 99.9 for the whole session [observed: o14_reach, 2026-11-01,
    §8]. At 0.2 health a landed blow the lead pipe would have shown 94.8, so in the
    original an NPC's melee blows on the player wear its weapon by much less than ours'
    rule, or not at all. Whether god mode on the target stops the wear was not
    separated. Against a mortal player (o15_round, 2026-11-01, healed by script) the
    weapons' GetEquippedCurrentHealth 5 did not move at all: lead pipe 49.95 over 10
    swings, sledgehammer 149.85 over 12, and an NPC 10mm Pistol 79.92 over 42 shots, a
    Hunting Rifle 299.7 over 15 [observed]. So these NPCs' weapons do not wear from
    their own attacks; ours wore them until #1334 and now wears only weapons with the
    override, as the Incinerator.
- **A weapon that is hit** [design: the owner's rule]. An actor holding its weapon
  drawn has a hit volume along the weapon (`hitvol::WEAPON_PART`: one capsule along the
  longest side of the model's bounds, `nvforever::limbs::weapon_segment`).
  - A shot or blow that lands there takes its damage before armour from the weapon's
    health. The actor is not hurt, and its hit blocks do not run [design].
  - A holstered weapon has no volume: a shot at the hip or the back hits the actor.
  - The weapon is the one the renderer put in the hand. The simulation rolls an
    actor's inventory on its own, so when it lacks that weapon one is added first
    [design; the two choices should become one].
- **What a worn item is worth** [documented: GECK wiki SetConditionDamagePenalty and
  SetArmorConditionPenalty (JIP), the hard-coded formulas those functions override]:
  - a weapon deals all its damage at and above 75 % condition; below,
    `1 − (0.75 − condition) × 0.67` of it (`damage::condition_mult`): 83 % at half
    condition, 49.75 % at zero. The original's troopers' Service Rifles at 30, 45 and
    50 % dealt 0.698, 0.799 and 0.832 of the full-condition hit [observed: §9 of
    npc-behaviour-compare.md]. Every actor's shots and blows use its weapon's real
    condition;
  - a piece of armour gives all its DT and DR at and above half condition; below,
    `1 − (0.5 − condition)` of them (`condition::armor_mult`): half at zero. Armour at
    zero stays worn.
  - An NPC's weapons and armour start at the condition its leveled lists give them:
    the COED after an LVLO (the CondNV… lists, e.g. CondNVServiceRiflelLoot: a Service
    Rifle at 30 to 60 %) [documented: GECK wiki Container, "Health %"; observed: the
    original's troopers drew them at 30 to 50 %].
- **A broken weapon** (zero condition) [design: the owner's rule]:
  - the player's is unequipped at once and stays in the inventory. `World::equip`
    refuses it (`SimError::Broken`) until it is repaired; a second, working one of the
    same kind is equipped instead. The player falls back to the Fists;
  - another actor's falls to the ground: its unit leaves the inventory as a dropped
    reference at the point of the hit, keeping condition 0
    (`World::drop_equipped`), and `WeaponBroken` makes the renderer take the model out
    of the hand and stop the weapon clips. Picked up, it cannot be equipped.
- **Which unit wears.** The equipped one. An NPC's weapon and armour are not marked
  equipped in the simulation until they first wear; then one unit is (plain ones
  first) and keeps the wear (`condition::wear_item`) [design].
- **Shown.** The ammo line ends with the weapon's condition ("CND 95%"); the inventory
  menu adds "(CND 46%)" or "(broken)" to a worn weapon or piece of armour; a message
  names a weapon that breaks. The percentage reads 0 only when broken.
- **Scripts.** `GetWeaponHealthPerc`, `SetWeaponHealthPerc`, `ModWeaponHealthPerc` and
  `AddItemHealthPercent` read and write the same value. A script that sets the
  player's weapon to 0 has it unequipped on the next frame; another actor's stays in
  its hand until it is hit [not built].
- **Test aids:** `condition=<percent>` (test_aid.rs) sets the condition of what the
  player has equipped; `aimpart=254` aims at the weapon the nearest NPC holds drawn.
- **Checked** [observed 2026-10-11, `nvforever --play` at Goodsprings, exit 0, "run
  finished"]:
  - 10mm Pistol set to 0.2 %: the first shot broke it ("the player's 10mm Pistol broke:
    unequipped"), the player went to the Fists, and `equip=` was refused ("is broken:
    it cannot be equipped until it is repaired").
  - 21 shots took the 10mm Pistol to "CND 95%" on the HUD (21 × 0.2 of 80).
  - `--actor-weapons drawn`, `aimpart=254`, the Goodsprings Settler 0x00104F0A's
    Varmint Rifle (health 120): ten hits of 12.6 took it 100 → 89.5 → … → 5.7 → 0 %;
    it "broke and fell" as reference 0xFF000000, which the dropped-item code drew.
    The settler's health did not change. Seen in the log only: the settler was too far
    off in the screenshot to see the empty hand.
  - Torso hits on a settler: its Field Hand Outfit lost 4.4 points per hit (0.35 of
    12.5), 100 → 95.6 → 91.3 → 86.9 → 82.6 %.
- **Not built:** repair (the owner: later); NPCs' own shots wearing their weapons and
  hits on the player's armour and weapon (NPCs do not attack yet; the code paths take
  any actor); jamming, slower fire and wider spread of a worn weapon; a worn item's
  lower value; armour worn to zero coming off; the per-class and VATS multipliers.

## 18. Damage between NPCs

How one NPC's hit on another becomes lost Health. The fixes are PR #475 (worn leveled
items, the documented condition penalties, no NPC armour wear, NPC Health) and the blast
attribution of §11. The original's fight they are checked against is in
npc-behaviour-compare.md §9.

- **The rule is the player's rule, §2.**
  - Weapon damage × `fDamageWeaponMult` × the condition multiplier × the skill bonus.
  - Plus the weapon's Crit Damage on a critical, before armour.
  - Then the target's DT. At least 20 % of the hit is left after DT.
  - Then its DR.
  - [documented: Weapon Damage Formula; observed: Cook-Cook's hits were exactly 0.2 ×
    10.9375, a critical added the full Crit Damage, npc-behaviour-compare.md §9.]
- **The attacker's values** are its own, read when it attacks:
  - its skill with the weapon (record and formula: troopers Guns 46, Fiend3 63,
    Cook-Cook 58; the player's level plays no part);
  - its Strength, Luck and Unarmed;
  - its weapon's real condition. It starts at the COED of the leveled list that gave it
    the weapon (CondNVServiceRiflelLoot: 30 to 60 %) [observed: §9].
- **The target's values.**
  - **DT** is the sum over the armour pieces it wears, each at its condition's share
    (`condition::armor_mult`), plus its own DT actor value. [observed: Cook-Cook's
    GetAV DamageThreshold was 13: Metal Armor DT 12 plus Raider Arclight Helmet DT 1,
    FalloutNV.esm `DNAM`.] Whether a helmet's DT counts against a body hit the same way
    is not settled: GetAV gives the total only [hypothesis: the sum for every hit].
  - **DR** is summed the same way. Every armour piece in this fight has DR 0.
- **No other multiplier** [observed].
  - FalloutNV.esm has no game setting that scales NPC damage. The damage settings in
    it are `fDamageWeaponMult`, `fDamageSkillBase`, `fDamageSkillMult` and the armour
    ones (`fnvtool esm inventory`).
  - The original's NPC hits fit the formula with nothing else (Cook-Cook's 2.1875).
  - Difficulty therefore plays no part between NPCs. [hypothesis: the capture save's
    difficulty was not recorded; that difficulty scales only hits by and on the
    player.] NVForever has no difficulty setting yet.
- **NPC armour does not wear; NPC weapons do** [observed: §17]. Ours wears an NPC's
  weapon by the documented per-shot rule of §17 (0.2 a Service Rifle shot, 0.005 an
  Incinerator glob). The original's Incinerator lost 2.19 health "a shot", far more,
  and the rule does not explain it. What is known (2026-10-20):
  - "A shot" there is `fight_stats.py`'s: GetAnimAction entering 2 or 3, so a full-auto
    burst counts once [observed: the script's definition]. Cook-Cook's bursts held the
    attack state 1.4–2.7 s, most 1.7–1.8 s, in campaign npc_battles (`ncr_fiends`, 20
    bursts) [observed]: at the record's 2 globs a second, some four globs a burst. That
    is about 0.55 a glob, 110 times the override; no ammunition effect explains it
    (Cook-Cook carries plain Flamer Fuel: LootAmmoFlamerFuel, FalloutNV.esm [observed];
    the homemade fuel's condition effect is × 3).
  - 2.19 is, to two places, the troopers' Service Rifle hit on Cook-Cook: 2.1875, the 20 %
    floor of 10.9375 (above). [hypothesis: the falls came from the troopers' shots
    landing on his weapon, or from every hit he took, not from his firing.] The capture's
    output with the per-sample weapon health is not on this machine, so it cannot be
    split again here.
  - To settle it: run campaign fight_stats_round again and read it with
    `tools/capture/fight_stats.py`, which now splits each fall of a fighter's weapon
    health into "hit" (an OnHit on the holder between the two samples), "firing" and
    "other", with the firing falls per shot and the hits' damage beside the hit falls.
    Ours is unchanged for hits on the holder until then: a hit on the weapon's own hit
    volume wears it by the hit's damage before armour (the owner's rule, above).
  - Ours, smoke `combat-npc-weapon-wear` (2026-10-20, one run) [observed]: the troopers'
    Service Rifles lost 0.200 a shot (18–47 shots, 3.6–9.4 health); Cook-Cook drew his
    Flamer, not the Incinerator the original's drew (both are in his inventory), and
    lost 0.005 a shot from firing (49 shots) but went from 80 % to 61 % condition from
    the troopers' shots on its hit volume.
- **Explosions** do the EXPL's damage with its falloff, then DT and DR (§11), with no
  skill or condition. They are their cause's hits (§11). The Incinerator does damage
  only this way: its WEAP damage is 1, its glob's NVIncineratorFireBurst does 20 in 212
  units, and its enchantment burns. The original's burn from it drained NPCs 1–2
  Health a second against the record's 4 (npc-behaviour-compare.md §9.1); ours drains 4.
- **Checked offline** (`fnvtool combat damage`, the records' values; condition and skill
  from §9's capture) [observed 2026-10-19]:

  | Attacker, weapon (condition) | Target (DT) | Ours per hit | The original |
  |---|---|---|---|
  | Trooper, Service Rifle 18 (30–50 %) | Cook-Cook (13) | 1.84–2.19 (floor) | median 2.0; 2.1875 at 50 % |
  | Trooper, Service Rifle, critical (50 %) | Cook-Cook (13) | 15.9 | crit damage added in full |
  | Trooper, Service Rifle (30–50 %) | Fiend (5) | 4.2–5.9 | medians 4.2–11.1 (with criticals) |
  | Fiend3, Hunting Rifle 52 (45 %) | trooper (Face Wrap 10 at 35–50 % + helmet 2: 10.5–12) | 21.9–23.4 | not isolated |
  | Cook-Cook, Incinerator glob | trooper (10.5–12) | 8.0–9.5 at the centre, less to the edge | medians 0.1–1.9, mostly burning |

  The Service Rifle figures are the original's. The task's "22–30 a Fiend's shot on a
  trooper against under 2" set rifle hits against a median of mostly burn damage. The
  original's troopers died chiefly of Cook-Cook's flames; its Fiends' rifles had little
  time (three Fiends dead within 8 s). By the same rule a Hunting Rifle body hit on a
  trooper is about 22 in both. Checking it needs the capture's hits split by attacker
  (`tools/capture/fight_stats.py` gives each actor's hits made and damage taken, not who
  hit whom).


## 19. Source queries for associated dropped weapons

[documented] [GetDroppedRefs](https://geckwiki.com/index.php?title=GetDroppedRefs)
returns world weapon references associated with a calling actor's death or targeted
weapon drop. These differ from inventory references and ordinary carried items.

[design] The named zero-argument query reads the existing `World::dropped_beside`
association, filters to weapon bases and returns those actual world references.
It does not create a drop, change inventory or infer association from proximity.
Actor qualification uses the current level-aware selected NPC/creature, so placed
LVLN/LVLC carriers qualify without being mistaken for ordinary actor base records.
A valid base-form or nonactor receiver yields the existing null/zero result.
An omitted receiver uses the calling actor. An explicitly supplied null/invalid
source receiver is refused by the existing VM before host dispatch, stops the script
and makes no getter mutation. The getter does not introduce a silent-zero continuation
or a special receiver adapter. Generic VM/host receiver policy is unchanged; no native
opcode adapter is added.

[design] Associated disabled/deleted world references remain excluded by the existing
world helper. Its deterministic FormID order is an engine mapping, not native order
parity. No associations yields the existing null/zero convention. Queries store no
new lifetime state: ordinary World/Game snapshot and restore retain association,
while a fresh world inherits none.

[observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9, 2026-11-24, `GSDocMitchellHouse`
(`0x00103DF9`), 10 official base masters, `w4d_42_vanilla_faction_class_kill_drop`
(`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3679): `dropSz=-1` (`ar_Size (GetDroppedRefs)`
called without a reference when no dropped weapons exist); no DLL or source consulted] When no
dropped weapons are associated, `GetDroppedRefs` returns `ar_Null` (`ar_Size == -1`), not an
empty array (`0`).

[hypothesis] Native ordering of multiple dropped refs and unusual unloaded/disabled-owner
timing remain probe questions. The getter relies on existing producer association; death/VATS
production and the full installed hover mod are not qualified by getter tests or a manually
seeded association.

[design] Four authored controls in the shared `fnv-game` integration binary use
compiled source, actual World placements and explicitly seeded existing associations.
They cover a hover list merging carried inventory references with dropped world
weapons and publishing its text, ordinary and leveled NPC/creature receivers,
disabled/deleted filtering, checked serialized restore and invalid-vs-omitted source
receivers. Array inspection stays inside the live compiled source and returns an
owned scalar representation; Rust does not inspect an expired returned array ID.
The original hover query's function-name collision is retained as a compiler
rejection control, with only that local renamed in its corrected consumer.

[validation: PR #2879, original `2c9574ec` composite `f2009ad5`]
All four original cases failed: a hover query did not compile, two helpers inspected
arrays after the query lifetime ended, and the explicit-invalid case received the
existing VM refusal rather than its incorrectly expected zero. Their downstream
world/UI/restore assertions were not reached and are not credited as passes. The
append-only corrected fixtures preserve those behavior oracles, assert the actual
error/no-continuation/no-world-mutation boundary, and are unexecuted at correction
authoring. Original and corrected baseline refs remain separate; predicted baseline
failures are not observations. These fixture corrections do not change compiler,
array lifetime, drop-producer or generic receiver policy.


[validation: PR #2879, `5a61bc59` composite `ddac1b90`]
The ordinary/leveled actor, filtering/serialized-restore and receiver cases passed
once. The original Label diagnostic control also passed, but the renamed hover
query still failed compilation before its count/UI/world assertions. Its quoted
semicolon is a source comment delimiter under the existing compiler contract
([extender bytecode §7](ext-bytecode.md#7-what-the-geck-refused-compile-messages)). The next fixture retains
that refused source as a `mismatched_quotes` control and generates the same delimiter
with `AsciiToChar 59`; exact title text, counts and references remain unchanged.
The production getter and compiler/array/receiver policies are unchanged. These
further corrected fixtures are unexecuted at correction authoring. The historical
three passes are scoped to `5a61bc59`, not transferred to its successor or baseline.
