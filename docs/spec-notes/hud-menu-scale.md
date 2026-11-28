# Independent HUD and menu scale (#2519)

[documented: NVForever issue #2519](https://github.com/LootJunkieAzza/NVForever/issues/2519)
requests separate in-game HUD and menu sizes. This is NVForever UI policy, not a
claim about a setting in the original game. The labels and translations are authored.

## Values and migration

[documented: existing NVForever interface design, `hud.md` §14 and issues #1631,
#1766, #1777] The former shared `ui-scale` slider has 21 choices from 50% to 150%.
HUD pieces keep their screen edges. Menus use the shared value capped at 100%:
their fixed layout reaches the view edges (the pause Back row is at 882 of 960
units). Retain that effective menu range, 50% to 100% in 5-point steps; enlarging
menus would need separate layout work. Review of this bound and row placement is
requested from the UI maintainer alongside the settings work in #2518.

[design: #2519] Keep `ui-scale` as the HUD key and label its row **HUD scale**.
Add **Menu scale**, `menu-scale`, immediately after it in Display > Screen. Both
default to 100% and take effect at the next start, including a save load. The old
key remains a normalized 0..1 slider part for 50..150%; the new key is a normalized
0..1 part for 50..100%. Thus legacy `ui-scale=0.25` means HUD 75% and initially
menus 75%, encoded as `menu-scale=0.5`, not `0.25`.

[design: migration] Before ordinary settings loading, synthesize only a missing
menu key from the fitted legacy HUD value and its old 100% menu cap. An explicit
menu key wins. Invalid legacy text leaves the new row at its default. Invalid
explicit menu text follows ordinary settings parsing and retains the default;
it does not silently resume following HUD. Loading this migration does not write
the INI. The existing settings store writes all registered rows on any edit, so
changing only HUD saves the initial menu size explicitly. It cannot follow that
HUD edit on the next restart. Unrelated INI sections remain intact.

## Consumers and validation boundary

[design] Keep the existing `ui_scale()` HUD route and `menu_scale()` menu route.
Only the source of the latter changes to the independent fixed value. Handbuilt
menus, tile menus, VATS menus and their inverse pointer transforms keep those
same routes. Texture-backed Pip-Boy and terminal pages retain `original_units()`;
their texture canvas is not resized by this change. No UI engine, reflow, dialogue
layout, native DLL or original-game compatibility behavior is introduced here.

[design: authored controls] CPU controls cover defaults, legacy/explicit precedence,
clamping, INI round trips after a HUD-only edit, independent edits in both
directions, and scoped drawing/pointer inverses at distinct HUD/menu sizes. The
sole existing global-setting test is extended rather than adding a parallel
writer of the HUD setting atomic. Registered smoke cases retain the old 75% menu
oracles and add HUD 75%/menus 100% restart, handbuilt and tile pointer, settings
and main-menu review. Source-bound execution results belong in the PR; the
registered installed-game cases require their actual logs and visual review.

## Expanded HUD and individual menu sizes [design]

The earlier `ui-scale` codec and migration above remain the compatibility rule for old saves/settings. The expanded HUD row uses a distinct `hud-scale-v2` part for 25–250% in 5% steps. A missing new value is synthesized from the old decoded HUD size, alongside the already independent menu value; explicit keys keep precedence. This avoids reinterpreting an old 75% size as a different value in the wider range.

Buying/selling, repair and workbench sizes are independently saved by `menu_readability`. Their existing painters and hit consumers own reflow and clipping; a full-canvas zoom must not move controls outside the usable view or desynchronize native hit tests. Source regression coverage includes bounds, the actual settings INI writer/reload and an isolated shared-test-binary startup that resets public HUD defaults before proving reload and the new drawing value. These controls and the menu consumer integration remain uncompiled/unrun in this source preparation; earlier narrow-range results do not qualify the expanded path.
