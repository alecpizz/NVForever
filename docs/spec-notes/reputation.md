# Reputation in play: crimes, the title box, the Pip-Boy list

The script functions (AddReputation, GetReputationThreshold, …) and their captured rules
are in scripts.md §7.6 and layer5-captures.md §9.3 a. This note is what play adds
(owner's playtest list 5, item 13).

## 1. Sources

- FalloutNV.esm: `REPU` records (name, Pip-Boy picture `ICON`, scale `DATA`), `FACT WMI1`,
  the settings `sRepTitlePosTwoNegThreeDesc`, `sRepPositiveGain` … `sRepNegativeLoss`, the
  sounds `UIRepGood`, `UIRepBad`.
- The Fallout wiki page "Reputation (Fallout: New Vegas)" (owner's copy,
  `C:\fnv-private\fallout fandom`).
- The owner's screenshots in `C:\fnv-private\Faction Reputation`: the boxes for
  Goodsprings at fame level 2 and the Powder Gangers at infamy level 3, and the Pip-Boy's
  General page in its reputation mode.

## 2. What the records and the page say

- [documented: xEdit wbDefinitionsFNV.pas FACT "Reputation"; observed:
  GoodspringsFaction 00104C6E has WMI1 RepNVGoodsprings 00104C22] A faction names the
  reputation its members count towards.
- [documented: the wiki page] Fame and infamy each have four levels; together they give
  one of sixteen titles, each with a one-line description. The levels
  are at fractions of the reputation's scale (the page's per-faction numbers are 0.15,
  0.5 and 1.0 × the `DATA` scale, rounded up, as captured in §9.3 a).
- [observed: `fnvtool esm inventory`] The load order holds one of the descriptions as a
  setting, `sRepTitlePosTwoNegThreeDesc` (fame level 2 with infamy level 3), in
  FalloutNV.esm; no title and no other description is a setting, in FalloutNV.esm or the
  DLC masters. The original's other words are built in, not in the data, and NVForever
  does not copy them (LEGAL.md, #2982).
- [documented: the wiki page, "Negative reputation for all towns and factions"] Being
  caught stealing costs 2 and killing a faction's or town's people openly 30; killing
  from a hidden state with a silent weapon in one blow costs nothing.

## 3. What NVForever does

- **Crimes** [hypothesis on the reading]: a theft someone saw adds 2 infamy, and any kill
  of an actor while someone (the victim too) sees the player adds 30, to every
  reputation the victim counts towards (its factions' WMI1), each up to its scale
  (`fnv_game::functions::crime_infamy`, `Note::Infamy`). Self-defence counts: the
  owner's box for infamy level 3 came from killing Powder Gangers. Opening an owned lock,
  trespass and assault change no reputation.
- **Who counts towards what** [design: `functions/reputation.rs` `reputations_of`]: a
  faction that owns the goods counts towards its own reputation. A killed actor is a
  reference: the factions it is in now, run-time changes included. Goods owned by an NPC or
  creature name its base form (XOWN), which has no run-time changes: the factions of its
  record, or of its template's with Use Factions, at rank 0 or above. So a script taking a
  placed Chet out of his faction does not change what stealing Chet's goods counts
  towards. Each reputation counts once, however many of the factions name it.
- **The box** [observed: the screenshots]: when a reputation's title changes the game's
  message box opens (it stops the game as message boxes do, queued with the others),
  titled with the reputation's name; the text is the new title, then its description
  (the setting `sRepTitlePos<n>Neg<m>Desc` where the load order has it [hypothesis: the
  pattern of the one setting], else NVForever's own); a Vault Boy face 100 × 100 at the
  left (the message menu file's icon place); one button, the setting `sOk`. The titles
  and the other descriptions are NVForever's own words (`nvf-play` reputation.rs
  `TITLES`, `DESCRIPTIONS`) [design, #2982]. The face is
  `glow_message_vaultboy_very_happy` when fame's level is above infamy's and
  `glow_message_vaultboy_sad` when below [observed: fame 2 alone, infamy 3 alone];
  `glow_message_vaultboy_neutral` when equal [hypothesis]. `UIRepGood` plays when fame
  reached a new level, else `UIRepBad` [hypothesis].
- **Pip-Boy, Stats / General, reputation mode** [observed: the screenshots]: the
  reputations the player has fame or infamy with; beside the list the reputation's
  picture, its title and under it its name.
- Saves keep the titles last shown, so loading shows no box for them.
- Log: `reputation: <name> <old title> -> <new title> (fame <f>, infamy <i>)`;
  `crime: infamy with <name> <before> -> <after>`.

## 4. Not built, and open

- Disguises (faction armour resets fame and infamy to Neutral while worn), the
  NCR/Legion/Freeside resets and the reputation effects on factions' behaviour.
- The corner messages `sRepPositiveGain` … ("Fame Gained!") the game may show for a gain
  that does not change the title: not seen on a screenshot.
- Whether killing a hostile faction member counts in the original when nobody else is
  near (here the victim's own detection counts as seeing).
