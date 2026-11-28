# Spec note: optional random weapon mods on NPC main weapons (`npc-weapon-mods`, #3876)

Tags as in `nif.md`: `[documented]` cites a public page or file, `[observed]` records a
measurement in the original game, `[hypothesis]` marks an unconfirmed reading, and
`[design]` marks NVForever's own engineering choices.

Per the 2026-11-24 owner ruling (`docs/design/extender-compat.md` §8.4, `#3607` / `#3794`),
JIP LN NVSE Plugin `[GamePatches]` entries are never read from `jip_nvse.ini`. Each wanted
patch becomes a named, **off-by-default** NVForever setting in its own issue and PR.

## 1. Source

- **[documented: JIP LN NVSE Plugin `57.30` ([Nexus Mods #58277](https://www.nexusmods.com/newvegas/mods/58277)), `jip_nvse.ini` `[GamePatches]`]** (no DLL or source code was consulted; verbatim INI block and comments):

  > `bNPCWeaponMods=1`
  > `uWMChancePerLevel=2`
  > `uWMChanceMin=10`
  > `uWMChanceMax=60`
  > `; When enabled, all NPCs will have a chance for their main weapon to include weapon mods. The chance is based on each NPC's level -`
  > `; the higher the level, the greater the chance. The type of mod(s) is selected randomly from the ones available for the particular`
  > `; weapon. Weapons have a chance to include multiple mods, though 2 mods is rare, and a fully-modded weapon is uncommon.`
  > `; uWMChancePerLevel controls chance % increase per NPC level.`
  > `; uWMChanceMin and uWMChanceMax set the minimum/maximum possible chance.`
  > `; Formula: Chance(%) = MinOf( uWMChanceMax , MaxOf( uWMChanceMin , Level * uWMChancePerLevel ) )`

- **Original unmodded game:** does not randomly attach weapon mods to NPC weapons based on
  NPC level.

## 2. NVForever option and behaviour

- **Setting (`[design]`):** `npc-weapon-mods` under **Gameplay › Game** (label `"NPC weapon
  mods"`, stored in `NVForever.ini` `[Settings]`, **off by default**). Uses JIP LN `57.30`'s
  default chance parameters (`uWMChancePerLevel=2`, `uWMChanceMin=10`, `uWMChanceMax=60`) as
  a single on/off row (`docs/design/settings-page.md` §0). `jip_nvse.ini` is never read.
- **Off (default):** unmodded vanilla NPC weapons are untouched — zero RNG calls, zero mod
  flags added, and zero `RefExtra` state written.
- **Chance formula (`[documented: JIP LN NVSE Plugin 57.30 jip_nvse.ini]`):**
  `chance_pct(level) = min(60, max(10, level * 2))`, where `level` is the NPC's resolved
  actor level (`Game::actor_level`, minimum 1).
- **Eligibility:**
  - Non-player NPCs only (`actor != FormId::PLAYER_REF`, resolved through
    `Game::template_source(base, actor, |_| false)` so placed/spawned leveled `LVLN` actors
    that resolve to `NPC_` are included, while `CREA` creatures and containers are excluded).
  - Applies only after the NPC is outfitted (`RefState::outfitted == true`) and has an
    equipped main weapon (`functions::items::equipped_weapon`).
  - Only weapons that define at least one valid weapon mod slot (`WMI1`, `WMI2`, `WMI3` ->
    bits `1`, `2`, `4`, honoring `TempCloneForm` clones and session `SetWeaponItemMod`
    overrides). Non-moddable weapons receive `0`.
  - Never overwrites an already-modded equipped weapon (`Extra::mods != 0`).
- **Single-roll-per-NPC tracking (`[design]`):** `RefExtra::npc_weapon_mods_rolled` records
  that an outfitted NPC has already been evaluated so repeated ticks never re-roll its main
  weapon. Respawns (`respawn::recreate`), inventory resets (`ResetInventory`,
  `ResurrectActorEx` with inventory reset), and leveled resurrections (`resurrect::roll_again`)
  clear the marker and re-evaluate the freshly outfitted weapon.
- **Deterministic per-NPC RNG stream (`[design]`):** NVForever mixes `(world_seed,
  actor_form_id, weapon_form_id, rolls)` with a SplitMix64 stream rather than mutating the
  shared simulation leveled-list RNG, so enabling the option does not perturb unrelated
  leveled-list rolls.
- **Multi-mod probability curve (`[design]`):** JIP LN's INI comment states qualitatively that
  *"Weapons have a chance to include multiple mods, though 2 mods is rare, and a fully-modded
  weapon is uncommon"* without publishing exact sub-probabilities. NVForever's own curve when
  the initial `chance_pct(level)` roll succeeds is:
  - 1 available slot: 1 mod (100%).
  - 2 available slots: 1 mod (80%), 2 mods (20%).
  - 3 available slots: 1 mod (75%), 2 mods (20%), 3 mods (5%).
  Available slots are shuffled via Fisher-Yates and the first `count` slot bits are ORed into
  the equipped weapon's `Extra::mods` via `Inventory::set_mods_one`.
- **Live consumers:** attached flags are stored directly on the equipped inventory entry
  (`Extra::mods`), so `GetEquippedWeaponModFlags` / `Inventory::equipped_mods` /
  `Inventory::mods_in_use`, third-person modded weapon models (`nvforever::outfits`),
  silenced/suppressor shot sounds and detection loudness (`nvf-play::combat`,
  `nvforever::detection`), projectile speed multipliers, and NPC ranged/melee combat weapon
  stats (`WeaponStats::modded`) all observe them live.
