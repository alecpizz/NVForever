# `GetFormOverrideIndex` and `IsFormOverridden`

## Documented contract

- [documented: GECK wiki, [IsFormOverridden](https://geckwiki.com/index.php?title=IsFormOverridden)]
  `IsFormOverridden form` (JIP LN / Lutana NVSE) returns `1` if `form` is
  overridden by a plugin lower in the load order than the calling script,
  or `0` otherwise.
- [documented: GECK wiki, [GetFormOverrideIndex](https://geckwiki.com/index.php?title=GetFormOverrideIndex)]
  `GetFormOverrideIndex form` (JohnnyGuitar NVSE) returns the load-order mod
  index of the winning (last) plugin that overrides `form`.
- [observed: `plugins1` capture, `crates/fnv-game/src/nvse_plugins.rs`]
  For a non-overridden form defined in plugin index `0` (`Stimpak`), both
  `GetFormOverrideIndex` and `IsFormOverridden` return `0`.

## NVForever mapping and open questions

- [design] `GameData::form_overrides` already records the load-order plugin
  indices for every record (base form or placed reference) defined or overridden
  by more than one plugin (`LoadOrder::index`). `GameData::is_form_overridden_after`
  resolves the calling unit's plugin (`g.other.plugin`, matching `GetSelfModIndex`)
  against `GameData::load_order` and returns `true` only when `form` has an override
  (`form_overrides` length > 1) whose winning (last) plugin index is strictly greater
  than the calling plugin's index. When the caller is a runtime script not belonging
  to a loaded plugin (console, `CompileScript`, Script Runner), no loaded plugin sits
  lower in the load order, so `IsFormOverridden` returns `0` — matching the committed
  `jg_01_form_override_index_edge` observation below. `GameData::form_override_index`
  returns the winning (last) plugin index from `form_overrides`, or `(form.0 >> 24) as u8`
  for non-overridden plugin records (including placed references in `World::placed`).
  Both functions accept either an explicit argument or a calling reference
  (`form(args, 0).or(subject)`).
- [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 with the 10 official base masters, clean-room script probe `jg_01_form_override_index_edge` (`tools/capture/nvse/catalogs/wave4a_extenders.json`, `GetFormOverrideIndex`/`IsFormOverridden`/`TempCloneForm`): `5,0,255,0,0`; no DLL or source consulted]
  For `GetFormOverrideIndex`: an overridden form returns the winning plugin index (`5` for `Weap10mmPistol` overridden by `GunRunnersArsenal.esm`), a null form returns `0` (not `-1`), and a `TempCloneForm` (`0xFF...`) returns `255` with `IsFormOverridden` `0`. For `IsFormOverridden` called from that runtime `CompileScript` probe, `Weap10mmPistol` (`GetFormOverrideIndex = 5`) also returns `0` (`oW = 0`).
- [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `w4d_32_jg_override_index_unknown_dot` (`tools/capture/nvse/catalogs/wave4d_extenders.json`, steps: query `GetFormOverrideIndex` on `rDocBase` `0x00104C0C` and `rDoc` `0x00104C0F`, compile 0-arg dot-receiver helper `rDoc.GetFormOverrideIndex`, spawn a `Player.PlaceAtMe` dynamic `0xFF` ref and query `GetFormOverrideIndex`/`IsFormOverridden`, and build invalid ref `BuildRef 0 16777214` -> `00000000` and query `GetFormOverrideIndex`/`IsFormOverridden`): `base=0,docRef=0|dotCmp=0|dyn(idx=255,ov=0)|bad(00000000:idx=0,ov=0)`; no DLL or source consulted]
  In JohnnyGuitarNVSE 528, a runtime `PlaceAtMe` dynamic `0xFF` reference returns `GetFormOverrideIndex = 255` and `IsFormOverridden = 0`; an invalid `BuildRef` resolving to `00000000` returns `GetFormOverrideIndex = 0` and `IsFormOverridden = 0`; and 0-arg dot-receiver syntax (`rDoc.GetFormOverrideIndex`) fails to compile (`dotCmp=0`).
- [design] NVForever's own internal synthetic FormIDs (`form.0 >> 24 == 0xFE`) return `255` from `GetFormOverrideIndex` and `0` from `IsFormOverridden`, and NVForever keeps a permissive omitted-argument `0` fallback (or dot-receiver `subject` fallback) rather than refusing 0-arg compilation.
- [hypothesis] Whether an ESP script defined in plugin A and overridden by plugin B compares `IsFormOverridden` against its winning plugin B or its defining plugin A has not been separately probed in the original game; NVForever compares against the running unit's winning plugin (`g.other.plugin`, matching `GetSelfModIndex`) [design].
- [hypothesis] Original-game JohnnyGuitar `GetFormOverrideIndex` and `IsFormOverridden` behavior on an unresolvable non-zero FormID (not collapsed to `00000000` by `BuildRef`) has not been separately measured; NVForever returns `0` for unknown FormIDs [design].
