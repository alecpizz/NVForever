# Spec note: optional crouch height collision (`sneak-bounding-box`, #3845)

Covers the optional, off-by-default NVForever setting `sneak-bounding-box`
(`Gameplay › Game` — **Crouch height collision**), reducing character collider height while
crouching for the player and crouching human companions with an overhead headroom check
before standing back up, per the 2026-11-24 owner ruling on `#3607`
(`docs/design/extender-compat.md` §2 and §8: JIP LN `[GamePatches]` are not read from
`jip_nvse.ini`; wanted patches become individual named NVForever options, off by default).

## 1. Documented plugin behaviour

[documented: **JIP LN NVSE Plugin v57.30** (`https://www.nexusmods.com/newvegas/mods/58277`),
**JIP LN Settings INI v56.24** public `jip_nvse.ini` `[GamePatches]` section comment; no
plugin DLL, PDB, or third-party source code was consulted]

> `bSneakBoundingBoxFix=1`
> `; Fixes a longstanding Bethesda games' bug where the dimensions of the collision bounding box encapsulating the player`
> `; (as well as all NPCs) remained fixed and did not scale to correspond to body posture. This, effectively, had made it`
> `; impossible to crawl through breaches and spaces when crouched, despite being able to easily fit through them.`
> `; This patch fixes this issue for the player character and human companions.`

JIP LN ships `bSneakBoundingBoxFix=1` (enabled by default in `jip_nvse.ini`). NVForever
does **not** read `jip_nvse.ini` for this option; it exposes a named NVForever setting
that defaults to **Off** (`false`) so unmodded vanilla collision bounds (fixed standing
capsule height `128.9` game units while sneaking) are unchanged unless the player opts in.

## 2. NVForever design choices and hypotheses

All items below are NVForever's own engineering choices (`[design]`, not behaviour claimed
from JIP LN's plugin) or unobserved edge cases (`[hypothesis]`):

- **[design] Setting identity, placement, and default:** named NVForever switch
  `sneak-bounding-box` (**Crouch height collision**), registered in
  `engine/nvf-play/src/player.rs` (`SneakBoundingBox` resource, synced live into
  `PhysState::sneak_bounding_box_fix` by `sync_sneak_bounding_box`), placed under the
  existing `Gameplay › Game` category in `crates/nvf-settings-layout/src/lib.rs`,
  persisted in `NVForever.ini` `[Settings]`, and **off by default** (`false`).
- **[design] Crouched height ratio (`CROUCH_HEIGHT_SCALE = 0.62`):**
  `fnv_phys::character::CROUCH_HEIGHT_SCALE` scales standing capsule height (`HEIGHT = 128.9`
  to `~79.9` game units for the player, and `standing_height * 0.62` for crouching human
  companions) while keeping `feet` fixed and leaving swimming characters at standing height.
- **[design] Overhead headroom probe and sneak hold:** `PhysWorld::set_character_height`
  checks `Queries::character_overlap` at `feet.z + skin.max(0.5)` with the standing capsule
  before growing a character taller. If overhead geometry blocks standing up, resizing
  returns `false` and `sync_sneak_heights` keeps `MoveIntent::sneak` and
  `PlayerControl::sneak` active so camera height, movement speed, and `IsSneaking` remain
  aligned with the crouched collider until headroom clears.
- **[design] Companion scope & live sync:** `sync_sneak_bounding_box` identifies human
  companions via `Game::is_teammate(r) && Game::is_human_npc(r)` (resolving placed `LVLN`
  leveled actors via `Game::template_source(..., |_| false)` for `NPC_` base records,
  excluding creatures) and shrinks their capsule only while `Game::sneaks(r)` is true.
- **[hypothesis] Exact original-game crouch scale and companion crouch detection under JIP LN:**
  the public `jip_nvse.ini` comment does not state the exact crouch height ratio, headroom
  probe clearance, or how companion crouch posture is detected; these have not been
  separately measured in the original game with JIP LN loaded.
