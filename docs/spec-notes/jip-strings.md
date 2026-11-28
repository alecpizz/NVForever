# Spec note: JIP string, name and regex functions

The JIP LN NVSE functions in the wiki's string categories: regular expressions, the
load-order-proof reference keys, form names and text fields, and string game settings.
Task J2 of [jip-tasks.md](../handoff/jip-tasks.md); input for J5. Math, mod and file
functions are in [jip-math-utility.md](jip-math-utility.md).

Wiki statements are `[documented: <page>]` (private export `C:\fnv-private\geckwiki\`, in
our own words). Results from the original game (JIP 57.30, xNVSE 6.4.9) are
`[observed: jip-forms/<id>]`: runtime-compiled probes, `tools/capture/jip_probes.py
--set jip-forms`, 2026-10-26, 24 rows, golden private. The forms used are vanilla
FalloutNV.esm records fetched with GetFormFromMod. Game text longer than a word is
recorded by its length only.

## 1. Regular expressions

| Function | Origin | Arguments | Returns | Behaviour |
|---|---|---|---|---|
| sv_RegexMatch | JIP | target (string_var); pattern (formatted string, up to 20 values) | 0/1 | whether the target matches the pattern [documented: Sv RegexMatch] |
| sv_RegexSearch | JIP | target; pattern (formatted) | array_var | every match in the target, in order [documented: Sv RegexSearch] |
| sv_RegexReplace | JIP | target; `"replacement\|pattern"` as **one** formatted string | string_var | a copy with every match replaced [documented: Sv RegexReplace] |
| ValidateRegex | NVSE 6.2.9 | pattern (string); error text (string_var, optional) | int | 0 when valid, else an error code: -1 arguments, 1 collate, 2 ctype, 3 escape, 4 backref, 5 brackets, 6 parentheses, 7 braces, 8 bad brace, 9 range, 10 space, 11 bad repeat, 12 complexity, 13 stack, 14 unknown [documented: ValidateRegex] |

Observed:
- `sv_RegexMatch` matches the **whole** target. "Whatchamacallit" against "(What)(.*)"
  gives 1, against "What" gives 0, and against "x(.*)" gives 0 [observed: regex.match,
  regex.partial, regex.nomatch].
- The wiki's search example finds 3 matches, the first "subject"; its replace example
  gives "This blip! has a blip! as a blip!" [observed: regex.search, regex.search0,
  regex.replace]. A `\b` inside a script string reaches the regex as written: the script
  compiler does not treat it as an escape.
- `ValidateRegex "["` gives 5 [observed: regex.validate], as documented.
- The target must be a string_var. A literal string there does not compile; use
  `(sv_Construct "…")` or a string_var variable [observed: jip_rx_trial, see
  xnvse-capture.md §5].

The pages warn that an invalid pattern **crashes the game** [documented: all three].
NVForever does not copy a crash: an invalid pattern is an extender run-time error (the
statement fails, the script goes on), the same decision as for Sv_Replace with an empty
old text.

**Dialect.** The wiki does not name it. ValidateRegex's error names are the error kinds
of the C++ standard library's regex (`std::regex` ECMAScript dialect: `\d+`, lookahead
`(?=...)`, lazy quantifier `+?`, backreference `\1`, and `ValidateRegex "[unclosed"`
returning `5`) [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30,
2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, read-only
`PrintToConsole` probe `w4c_b1_24_jip_regex`
(`tools/capture/nvse/catalogs/wave4c_extenders.json`, #3679, testing `sv_RegexMatch`,
`sv_RegexReplace`, `sv_RegexSearch`, and `ValidateRegex`):
`PROBE|w4c_b1_24_jip_regex|dig=1,look=1,lazy=1,back=1,rep=[item X cost X],srchN=3(s0=[1]),valBad=5`;
no DLL or source consulted]. Behaviour to keep: whole-string match for RegexMatch, `\b`, groups,
`[^ ]`, `.*`. A Rust implementation needs back-references (ECMAScript has them), which
the `regex` crate lacks. Candidate: `fancy-regex` (dependencies.md), anchored with
`^(?:…)$` for RegexMatch. Owner review is not needed: the observed cases decide it.

## 2. Reference keys

| Function | Origin | Behaviour | Observed |
|---|---|---|---|
| RefToString | JIP | a load-order-independent key for a form: `<plugin file>:<form ID without the mod index, hex, no leading zeros>`. Called on a reference it describes the reference; given a reference as an argument it describes the **base form** [documented: RefToString] | Doc Mitchell's base form = "FalloutNV.esm:104C0C" (as the wiki's example); the player reference = "FalloutNV.esm:14" [observed: reftostring.base, reftostring.player] |
| StringToRef | JIP | the form back from RefToString's text, or from GetFormIDString's eight hex digits [documented: StringToRef] | "FalloutNV.esm:104C0C" → 00104C0C [observed: stringtoref.modkey]; "00000014" → the player [observed: jip-getters/stringtoref.player] |

The upper-case hex and the dropped leading zeros come from the observed keys.

## 3. Names and text fields

| Function | Origin | Arguments | Returns | Behaviour | Observed |
|---|---|---|---|---|---|
| LNGetName | LU | form (optional; else the calling reference) | string | the full name of a reference or base form, any named type; unlike GetName it sees names set with SetRefName [documented: LNGetName] | Doc Mitchell = "Doc Mitchell"; the Prospector Saloon cell = "Prospector Saloon" [observed: lngetname.*] |
| GetFormDescription | JIP | base form | string_var | the Description text of a note, perk, message, terminal, weapon mod, challenge, loading screen … [documented: GetFormDescription] | perk Action Boy: 84 characters; an NPC (no description): "" [observed: formdesc.*] |
| GetObjectiveText | JIP | quest, objective index | string_var | the objective's display text [documented: GetObjectiveText] | VCG02 objective 30: 28 characters; objective 9999: "" [observed: objective.*] |
| GetBodyPartName | JIP | body-part data, part 0–14 | string_var | the name of a part. Parts: 0 torso, 1–2 head, 3–4 left arm, 5–6 right arm, 7–9 left leg, 10–12 right leg, 13 brain, 14 weapon [documented: GetBodyPartName] | NVGecko part 0 = "Torso"; part 14 = "" [observed: bodypart.*] |
| GetNthPerkEntryString | LU | perk, index | string | the string of a perk entry (the activate prompt of type 4) [documented: GetNthPerkEntryString] | Action Boy entry 0 = "" (not that type) [observed: perkentry.0] |
| GetLoadScreenTexture | LU | loading screen | string | its texture path [documented] | GeneralLoadScreen01 = `interface\loading\loading_billboard05.dds`, as documented [observed: loadscreen.texture] |
| GetProjectileMuzzleFlash | JIP | projectile | string | the muzzle-flash model path [documented] | 9mmBulletProjectile = `effects\MuzzleFlashes\handgunmuzzleflash01.nif` (case kept) [observed: muzzleflash] |
| GetFontFile | JIP | font type 1–8, 10–89 | string_var | the font's .fnt path [documented: GetFontFile] | 2 = `Textures\Fonts\Monofonto_Large.fnt`; 9 (outside the range) = "" [observed: fontfile.*] |
| GetStringSetting | LU | game setting name | string | a string game setting [documented: GetStringSetting] | sTraitMenuTitleText = its value, with `%d` left in [observed: stringsetting] |
| GetMapMarkerName | LU | — (map marker reference) | string | the marker's name [documented] | not probed |
| GetUIString | JIP | "Menu/element/…/trait" | string_var | a UI trait's string (J8, UI) [documented] | not probed |

A missing entry, part or objective gives an empty string, not an error [observed]. Text
comes from the loaded records (with later plugins overriding), so NVForever reads the
same fields fnv-esm already decodes: FULL, DESC, objective NNAM, BPTD part names, LSCR
ICON, PROJ muzzle-flash model.

**Setters** (not probed; they change the game). LNSetName sets the **base form's** name,
even when called on a reference [documented: LNSetName]. SetFullNameAlt (alias
SetActorFullNameAlt) also changes the base form, and the change is **permanent and kept
in the save**, unlike SetName and LNSetName [documented: SetFullNameAlt]. SetRefName
names one reference (read back by LNGetName). SetFormDescription, SetObjectiveText,
SetBodyPartName, SetMapMarkerName, SetStringSetting, SetLoadScreenTexture and
SetProjectileMuzzleFlash replace the field the matching getter reads; `SetMapMarkerName`
(`Game::map_marker_name_override`) also reaches the Pip-Boy World and Local Maps,
discovery notices, and HUD marker names, and `SetMapMarkerRep`
(`Game::map_marker_reputation_override`) reaches the World Map reputation line
(clearing it when set to `0` or a non-`REPU` form) [design]. The texts of
SetFormDescription and SetObjectiveText are formatted strings (up to 20 values)
[documented: their pages]. Whether the others last past a reload is not documented
[hypothesis: no, like base-game form changes without a save record]. Probing this needs
a save and a reload, so it waits for the J5 implementation.

## 4. Not covered

GetStringHash has a page with no content (JIP 57.30) [documented: GetStringHash]. It
returns 0 by decision (jip-world-12.md §2): the hash the original gives is unknown. AuxiliaryVariable…String and
RefMapArray…String belong to JIP's variable stores and go in the arrays note.
