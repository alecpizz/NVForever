# Spec note: the HUD

Tags as in `nif.md`: `[documented]` cites a page or file, `[observed]` says what was
looked at, `[hypothesis]` is a reading that nothing has confirmed yet. `[design]` marks
what is NVForever's own. Implementation: `engine/nvf-play/src/hud.rs` (the HUD),
`interface.rs` (what it shares with the message box and the corner messages: units,
colour, fonts, textures),
`combat.rs` `Combat::hud_weapon` (condition and ammo), `hud_target.rs` (whose health the
enemy bar shows), `crates/fnv-font` (`fonts.md`).

## 1. Sources

- **The game's interface files** in the base archive `Fallout - Misc.bsa`:
  `menus\main\hud_main_menu.xml`, the shared pieces `menus\prefabs\hudtemplates.xml`,
  `compassicon.xml`, `meter.xml`, `brightness.xml`, and `menus\globals.xml`. Described
  here in plain words; nothing of them is shipped.
- **The textures those files name**, in `Fallout - Textures2.bsa` under
  `textures\interface\hud\`. Looked at as pictures; read at run time from the owner's
  archives.
- **`Fallout_default.ini`**, sections `[Interface]` and `[Fonts]`.
- **The owner's screenshot** of the original's HUD at Goodsprings (1280 x 720, third
  person, with the parts labelled: Health, Compass, Action Points, Weap Condition,
  Ammo). **It was described to this session in words but not available as a file**, so
  nothing in this note is measured on it yet; every place it would settle is marked
  `[hypothesis]` in §4 and listed in §7.
- **The GECK wiki's "DisablePlayerControls" page**, from the owner's export.
- **The owner's screenshots for playtest list 5** (2026-10-14/15, private, in
  `C:\fnv-private`): the original's HUD at 1920 x 1080 in a fight with radscorpions
  (`Target healthbar`), a side-by-side of NVForever's and the original's bars at the
  same size (`HPAPMisaligned`), the four sneak words at 1280 x 720 (`Sneak detection`),
  and NVForever in a conversation with Trudy while Joe Cobb talks to her (`DialogueUI`).
  The measurements of §4, §8 and §9 are taken on them.
- [observed: private jobs hud_hardcore2, hc0-hc11; hud_radiation2,
  r_in_000-r_in_990; hud_enemy2, e5_back; hud_enemy3, n2_away, g2_gecko_half;
  hud_xp_limbs, l1_leg, l2_head; hud_xp_limbs2, x9_level_up_long; hud_prompt2,
  p4_container_empty, p5_container_empty_opened] T12b's 2026-10-29 trait/time reads,
  described in the private `hud-capture/t12b-findings.md`. These jobs' screenshots
  failed; the corrections below use numeric/string reads, not new picture measurements.

## 2. What the files say

[documented: `menus\main\hud_main_menu.xml`] The HUD is one menu, `HUDMainMenu`, drawn
behind every other menu (depth -50), with a quarter of a second of fading. It is made
of named, empty rectangles, each in the HUD's main colour, with only a width and a
height (interface units, 960 to the view's height):

| Piece | Width | Height |
| --- | --- | --- |
| `HitPoints` (health and compass) | 369 | 127 |
| `ActionPoints` (action points, condition, ammo) | 386 | 127 |
| `SneakMeter` | 356 | 34 |
| `ReticleCenter` (crosshair) | 63 | 75 |
| `EnemyHealth` | 350 | 52 |
| `Messages` | 460 | 90 |
| `Subtitles` | 800 | 110 |
| `Info` (the prompt under the crosshair) | 320 | 80 |
| `QuestReminder`, `Hokeys`, `Region_Location`, `XPMeter`, `RadiationMeter` | | |

The file gives **no position** for any of them and no contents for the first five: a
comment in it says that most of the HUD's parameters are set in the game's code. It
also holds the breath meter (a model), the crippled-limb picture, the armour-piercing
icons, the ammo type's label (font 7, right-justified, "set in code"), and the Hardcore
mode's four words.

[documented: `menus\prefabs\hudtemplates.xml`] The contents are templates the code
makes copies of:

- **Brackets.** `left_bracket` and `right_bracket`: a picture 512 x 256 each
  (`glow_hud_left_seperatorglow.dds`, `glow_hud_right_seperatorglow.dds`), behind the
  rest (depth -1).
- **A bar.** `meter`: the picture `hud_tick_mark.dds`, 8 wide and 20 high, repeated
  (`tile`), left-justified, with a "total width" of 300 and an "image width" of 8.
- **A bar's ground.** `MeterBackground`: the plain picture `solid.dds`, 60 wide and 20
  high, at x 90, behind (depth -1).
- **Texts.** Three templates, left-, right- and centre-justified, all in font 7.
- **The compass.** `compass_window`: the picture `glow_hud_comp_direction_strip.dds`
  in a window 345 wide and 64 high at (20, 65), drawn at its own size (zoom 100).
  Icons on it (`compass_icon` and three kinds: quest `glow_hud_compass_objective_marker`,
  the player's own marker `glow_hud_compass_pc_marker`, a place
  `glow_hud_compass_landmark`), each 40 x 50 with an x that the code sets as a
  "heading, in compass pixels … based on compass width in pixels", and a distance.
  Actors nearby are small ticks (`glow_hud_tick_mark.dds`, 32 x 32).
- Others not used here: the crosshair (`glow_crosshair.dds` and a filled one, 64 x
  64 at (2, 9)), the enemy's health bracket, the sneak bracket, the message bracket and
  icon, a separator with a divider (`glow_hud_bottom_info_seperator_divider.dds`,
  hidden), the hit direction indicator, the explosive indicator.

[documented: `Fallout_default.ini`] `sFontFile_7` is
`Textures\Fonts\Baked-in_Monofonto_Large.fnt`. `[Interface]` has `iSafeZoneX` and
`iSafeZoneY`, both 15, and a main HUD colour of red 26, green 255, blue 128 (the green
of the game before; the owner's game shows amber, as the message box's note says).

[documented: `menus\prefabs\brightness.xml`] A list of brightnesses names 175 for "line
brightness in HUD" and for "meter and compass in HUD", and the text's 255. The HUD's
templates do not include this file, so whether the HUD's lines are dimmer than its
texts is not known from the files.

[documented: GECK wiki, DisablePlayerControls] With the movement flag the "HUD is
partially disabled: Action Points, Health, Crosshair".

## 3. What the textures show

[observed: the pictures, decoded and looked at]

- **Left bracket** (content 372 x 137 of the 512 x 256): an upright line at x 17 from
  y 10 to 125; a long horizontal line at y 66–70 from the upright to x 360; under its
  middle a small triangle pointing down (x 165); on top of it near its right end a
  small corner mark (x 320–332).
- **Right bracket** (content 381 x 137): the mirror: the upright at x 369, the long
  line from x 15, the corner mark on top near its left end (x 58–70); and a second,
  shorter upright going down from the long line at x 207: the divider between
  condition and ammo.
- **Tick** (`hud_tick_mark.dds`, 8 x 32, DXT3, white): one upright block 6 wide and 22
  high at the top left; texels 6-7 and rows 22-31 are clear. Its alpha rises down the
  block in 4-bit steps: 34 in rows 0-1, then 51, 68 … 187 (rows 2-10, 17 a row), 204 in
  rows 11-12, 221, 238 in rows 14-16 and 255 in rows 17-21 (decoded 2026-10-29, §14.9).
  A 20-high box at a texel a unit shows rows 0-19: the fade over the top 17 rows, then 3
  full rows.
- **Compass strip** (1024 x 64, the drawing in rows 15–48): a horizontal line with
  small ticks and the letters N, NE, E, SE, S, SW, W, NW from left to right, 128
  apart, the N split over the two ends: 1024 texels are one full turn.
- `hud_compass_alphamap.dds` (64 x 16): white for the first 20 texels, then black.
  Nothing in the files says how it is used [hypothesis: it fades the compass's ends].
  Its step is not the shape of the original's fade, which runs over about 70 units at
  the window's right end (§4, §14.10) [observed].
- **The atlas pieces are other art** than these own files (menu-xml.md §3.1.3): the
  brackets' pieces in `interfaceshared.tai` have stepped end caps by the bars, cut
  corners, a doubled long line and the health underline's hard end; the strip's piece a
  doubled line with thick bars between the letters [observed: decoded, T12c]. The
  original draws the pieces [observed: §14.10]; both NVForever drawings do since T12e.

## 4. How NVForever sets it out

What the files leave to the game's code is set here. The places of the pieces in the
corners are `[hypothesis]`; the bars, labels and the condition bar were measured on the
original at 1920 x 1080 (`Target healthbar`, 1.125 pixels to the unit) `[observed]`:

- The two pieces stand in the bottom corners, 40 from the side and 30 from the bottom
  edge. Each bracket picture is drawn texel for unit at its piece's top left corner.
- The bars stand on the long line, about 4 above it. A full bar is 37 ticks, 296 long
  (the file's "total width" is 300), from the upright's inner edge to just short of the
  bracket's corner mark: x 20–316 on the left, 68–364 on the right [observed: on the
  `Target healthbar` screenshot the first tick starts at the upright's inner edge and
  the last ends about 2 pixels before the mark; on the right the first starts about 2
  pixels after the mark and the last meets the upright; owner's playtest 2026-10-16,
  after x 24–320 / 70–366 ran the last ticks into the mark and the upright; measured
  again 2026-10-16: left tick edges at x 69, 78 … 384 on both, right at 1520 … 1844 on
  the original against 1518 … 1842 with 67–363]. "The last meets the upright" was an
  observation by eye at 1080 lines, not the owner's request (the owner's was the
  opposite: no tick into the mark or the upright), and the 720-line shots contradict it:
  every tick's block stands at the start of its step, the picture's 2 clear texels after
  it, on both bars, so the action points' last tick ends that strip short of the upright
  [observed: hud_compare needs_off at 1280 x 720, the original's last AP tick at x
  1229-1232, a pixel clear of the upright at 1234; its first health tick at 45, the last
  ending at 265, a pixel before the step cap at 267; the health bar's tile read at x 20,
  §14]. Until T12e the action points' row put the strip before its first tick and its
  last met the upright, and the health bar stood at 21 [design, corrected T12e].
- The ticks are all as wide (whole pixels), and a full bar spans its room at any view
  size: the first tick at the bar's start, the last ending the tick's clear strip
  (`round(8 × scale) − round(6 × scale)`) short of its end, on both bars since T12e (the
  first that far in on the right before); the ticks between stand on whole pixels evenly
  spread, so the gaps differ by a pixel at most [design: a whole-pixel step `round(8 × scale)` left a
  full bar up to 25 pixels short of the mark or the upright at sizes where 8 × scale is
  not whole, 1280 × 800 or a maximised window; owner's playtest 2026-10-16, "a gap at
  the end of each"]. At 720, 1080 and 1440 lines the gaps are all equal, as before.
  The hand-built HUD keeps this; the tile-drawn HUD draws the original's even, soft
  ticks instead (§14.9) [design, T12d].
- "HP" and "AP" stand **above** their bars, near the uprights: "HP" starting at x 23,
  "AP" ending at x 355, on the line y 33 (their capitals 23 high) [observed: measured
  again on the same screenshot 2026-10-16; 33 and 362 before stood 11 and 8 pixels
  right of the original's]. The words
  are written as on the screenshots [hypothesis: the game takes them from its settings].
- A tick is the picture `hud_tick_mark.dds`: a block 6 texels wide (2 clear after it),
  its opacity rising from about an eighth at the top to full at the bottom [observed:
  the texture, §3], so the ticks look darker than the lines over the world; drawn at
  brightness 175 since T12d [observed: o5_00-01, 2026-10-31, the original's full-row meters
  `HitPoints/*:2` and `ActionPoints/*:2` (296 wide) read `brightness` 175, the bars' other
  visible children 255; §14.9]. Each tick is set on whole pixels — the
  step `round(8 × scale)`, the width `round(6 × scale)` — so all ticks are as wide as
  each other at any view size [design: the owner's "clean straight lines, not some
  bigger than others"; at 720 lines a 6-of-8 tick is 4.5 pixels and rounded unevenly
  before]. The tile-drawn HUD draws its bars as one stretched image instead (§14.9).
- A bar lights `floor(value × 37)` ticks [observed: §14.3]. The health bar empties from
  the right, the action points bar from the left (towards its label).
- The compass window is where the file puts it. The heading is in its middle: the
  strip is shifted by heading / 360 x 1024 [observed: §14.5]. The heading stands at the
  bracket's triangle (x 148 of the window, 168 of `HitPoints`), not the window's middle
  (x 192 of `HitPoints`) [observed: §14.5]: the marks' heading point. The strip's own
  N stands 6 further right at heading 0, x 154 of the window (`hud::STRIP_HEADING`)
  [observed: §14.10; design until T12e: the strip at 148 too, its N 4.5 pixels left of
  the original's at 720 lines].
  The left end is cut off sharp; the right end fades out from x 273 of the window to
  nothing at 341, linearly (`hud::STRIP_FADE`), the marks over it not faded [observed:
  §14.10; hypothesis: linear]. Until T12e both ends were sharp. Both drawings bake the
  window's strip into one image the window's width in texels (345 x the strip's height):
  the strip's texels from the one at the window's left end (`hud::strip_left` in texels,
  wrapping at its 1024 units), each column's alpha times the ramp, drawn stretched with
  linear sampling and baked again in place only when the heading moves the strip a texel
  (`crate::compass_strip`) [design]. Without the strip's texels, the doubled strip moved
  by the heading, unfaded. The first T12e drawing cut the window into 2-unit clipped
  slices, each with its own copy of the strip; a slice (1.5 pixels at 720 lines) and its
  copy rounded to whole pixels apart, so neighbouring slices showed the strip a pixel
  apart: "NE" striped like "ME", the ticks broken in steps [observed: the user's
  screenshot of the fade].
- Under the right bracket's long line: "CND", then the condition's bar over a ground 60
  long and 20 high [documented: the size], the bar at brightness 175 over an opaque
  ground at 100 [observed: §14.7, the original's 175/255 and 100/255 of the HUD's colour;
  T12e]; until T12e the bar at 255 over the bar's colour at a quarter's alpha
  [hypothesis: as `meter.xml`'s ground]. A player report (#3309) says the original's
  bar has transparent parts. The shots of §14.7 read it as above [observed]; the
  ground's template `MeterBackground` has no `alpha` trait [documented:
  hudtemplates.xml] and `vats_bar.dds`'s block is opaque [documented: the texture],
  which is static evidence, not proof that the original never shows it translucent
  (a translucent fill over an opaque ground would read the same on those shots)
  [hypothesis]. It stays opaque until observer check 264 says otherwise [design].
  The bar ends 17 before the divider (x 130–190)
  [observed: §14, read through xNVSE and on its screenshot; 10 before stood 7 too far
  right],
  "CND" ending 11 before it, and its bottom stands 2 above the line "CND" stands on
  (y 106), about as high as the letters [observed: on the `Target healthbar` screenshot
  the bar starts about 8 pixels after the "D"; owner's playtest 2026-10-16, after x
  132–192 with the bottom on the line left the bar 2 pixels from the "D" and low].
  Right of the divider the ammo, right-justified near the
  upright, as "rounds in the weapon/rounds carried besides".
- Left of the counter, the ammo type's label (`AmmoTypeLabel`: font 7, right-justified,
  its place and text set in code [documented: hud_main_menu.xml]): the AMMO record's
  `QNAM` abbreviation ("HP", "JHP", "AP", "Slg", "Bag", "MAG", "CS"), nothing for
  standard rounds, which have no `QNAM` [observed: FalloutNV.esm, 48 of 92 AMMO records
  have one; `Ammo9mm`, `Ammo10mm`, `Ammo12Ga` have none]. It starts at x 214, just
  right of the divider, whatever the counter's length [observed: the owner's screenshot
  2026-10-25 of the original, a 12 gauge with "1/16", marks the label's room at
  1680-1765 of 1920 pixels, the divider at 1672: 214-290 units; owner 2026-10-25:
  "anchor it to the left separator not against the ammo count"; commit 5887ee0e "at the
  owner's request"]. A thrown weapon and a weapon that uses no ammo show no label.
  **The maintainer's choice, kept (T12e).** The original at 1280 x 720 draws it **left**
  of the divider, right-justified at `AmmoTypeLabel`'s captured x W - 236 ("HP" at x
  1085-1104 pixels, the divider at 1114), the counter alone on the right, and moves
  "CND", the condition bar and its arrows left by the abbreviation's width + 20 (50 for
  "HP", 30 wide) [observed: hud_compare hud-ammo-label; hypothesis: the rule, one
  abbreviation seen]. The tile-drawn HUD does that (§14.7).
- Over the condition bar, the jamming threshold: `CNDArrows`, the picture
  `interface\shared\arrow\repair_meter_pips.dds` [documented: hud_main_menu.xml, its
  place "set in code"], two black arrows at the picture's top and bottom edges pointing
  at each other. The arrows' bases are the picture's first and last rows [observed: the
  texture]; drawn as high as the bar the filtered edge rows left a strip of the bar
  above and below them (owner's playtest 2026-10-16), so the picture is drawn 1 taller
  at the top and the bottom (22 × 22) and cut to the bar, the arrows'
  middle at three quarters of the bar [owner: "the 75% threshold on condition … when
  jamming occurs"; observed: at about 0.75–0.79 of the bar on the screenshot].
- Everything is in the interface's amber at full brightness, but the bars' ticks (175,
  §14.9) and the condition row (175 and 100, above).
- The brackets and the compass strip are their atlas pieces, stretched over their boxes,
  the right bracket's 4 units right of its piece's corner (`hud::RIGHT_BRACKET_X`)
  [observed: §3, §14.10; design until T12e: the own files at the corners].

## 5. Where the numbers come from

- **Health**: the player's current Health over its full value (`fnv_game`:
  `actor_health`, `max_health`).
- **Action points**: the Action Points actor value, what is left of base and permanent
  modifier after damage. Nothing in NVForever spends action points yet (no V.A.T.S.,
  no sprinting), so the bar stays full unless a script changes the value.
- **Compass**: the player's heading (`Player::heading`).
- **Condition**: the equipped weapon's condition, 0 to 1 (`combat-rules.md` §17). With
  bare hands the label and bar are not shown [hypothesis].
- **Ammo**: the magazine and what is carried besides, of the ammo type in use
  (`combat.rs`). A thrown weapon shows its own count and 0. A regenerating weapon (the
  Recharger Rifle) shows its charge and what is carried of its ammo type, with nothing
  taken off [hypothesis: observer check 106]. A weapon that uses no ammo
  shows nothing [hypothesis: the screenshot shows "0/0"; with which weapon is not
  known].

[design] The HUD is on the screen while the player is in the world (`PlayState::Playing`)
and off in every menu, in dialogue and behind loading screens (§10 for dialogue). The interface file does
not say which menus hide it. [documented, §2] While a script holds the player's
movement (the opening at Doc Mitchell's) both corner pieces are hidden, the compass,
condition and ammo with them, being parts of the same two rectangles [hypothesis].

Without the game's archives the brackets are plain lines, the ticks plain blocks and
the compass the eight directions' letters in Bevy's font.

## 6. Checking

- Unit tests in `hud.rs`: a bar's length, the compass's shift, the ammo's text, the
  pieces within their brackets.
- By eye, at the screenshot's size, in third person, a pistol at 60 % with 50 rounds,
  facing west-south-west (with `NVF_PLAY_TEST_SCRIPT_MENUS=0.2`,
  `NVF_PLAY_TEST_EQUIP=000E3778`, `NVF_PLAY_TEST_GIVE=0008ED03:50`,
  `NVF_PLAY_TEST_LOOK=20:247:-5`, `NVF_PLAY_TEST_CONTROL=3:view,21:condition=60` and
  `NVF_PLAY_TEST_SHOTS=24:<path.png>` set):
  `nvforever --data <Data> --cell Goodsprings --play --size 1280x720 --no-overlay --no-voice --run-seconds 26`
  (expect "HP" and a full bar, "SW" and "W" on the compass, "AP" and a full bar,
  "CND" with the bar at six tenths, "13/37").

- Smoke scenarios `hud-sneak-target` (the sneak word and a bighorner's health on the
  enemy bar) and `hud-dialogue` (Trudy's lines while Sunny talks to the player are logged
  `(not drawn: in dialogue)`), `tools/smoke/scenarios.toml`.

## 7. Not built, and open

- **Not measured**: the pieces' places on the screen, the compass's middle, the ammo's
  place, the brightness of lines against text (§4 measured the bars, labels and the
  condition bar).
- Compass markers: quest targets, places, the player's own marker, actors nearby.
- The compass's faded ends: the right end's fade is built (§4, T12e); how the original
  makes it is not known.
- [observed: private job hud_xp_limbs2, x9_level_up_long] XP meter: about 2 s fade in,
  2 s hold/glide (pointer starts at full alpha), 2 s fade out; LEVEL UP alone afterwards,
  up within 0.34 s, held about 1.3 s. [hypothesis] `xp_meter.rs` fits the notice at
  7.2 s, with a 1.2 s gap and symmetric 0.34 s fades (experience.md §6). History:
  0.25 s fade, 3 s hold, immediate eased 0.6 s slide and LEVEL UP replacing +n XP.
  [owner 2026-10-24; observed: private job hud_xp_limbs2, x9_level_up_long; design: #3438]
  The filled-bar sound stays at 4.0 s and `LEVEL_UP_DELAY` is 5.35 s so the menu opens at
  the observed 9.35 s after the `LEVEL UP` fade (9.18 s); the original's sound moment is
  still uncaptured.
- The radiation meter is built: radiation.md §6 (`radiation.rs`); its place is a guess.
- The breath meter is built: drowning.md §3 (`hud_breath.rs`); from its `nif` tile with
  `[UI] TileMenus=HUDMainMenu` (§14.12).
- The messages at the top left with their icons, subtitles,
  the armour-piercing icons. The ammo type's label is built (§4). The crippled-limb
  picture is built (§12), and Hardcore mode's words (§13).
  (The crosshair and the prompt under it are drawn as the original's: `reticle.md`.)
  The developer's stand-ins for some of these (`activate.rs`, `script_hud.rs`) are
  unchanged.
- [design: owner 2026-10-27, reticle.rs; maintainer's choice T12b B/M5; needs an issue]
  Keep `[Empty]`. [observed: private job hud_prompt2, p4_container_empty,
  p5_container_empty_opened] The original uses `[EMPTY]` before and after opening an
  empty container. Draft issue: decide whether the owner's casing should match the capture.
- The HUD's fading in and out, its opacity setting, and the glow the game adds over it
  (`[InterfaceFX]` `fBlurIntensityHUD`).
- What the condition bar looks like for a weapon being repaired or broken.
- Weapon jamming itself (playtest list 5, item 9) is another workstream's; the arrows
  only mark the threshold.

## 8. The enemy's health

[documented: hud_main_menu.xml, hudtemplates.xml] `EnemyHealth` is a piece 350 x 52;
its bracket `glow_enemy_health_seperator.dds` (500 x 64) stands at (-7, 3) in it
[observed: private jobs hud_enemy2, e5_back; hud_enemy3, g2_gecko_half]. The former
512-wide drawing confused texture size with template width [design: history]; the
hand-built marks' texture columns 56 and 307 now scale by 500/512 [design].
[observed: the texture] A long line (solid in rows 21–23 under a glow, from x 36 to
327) with a corner mark rising at each end; their inner edges at x 56 and 307.

[observed: the `Target healthbar` screenshot] The piece is centred across the view; its
long line is level with the upper of the corner pieces' two solid lines; the ticks (the
same tick picture as the player's bars) stand right on it, with no gap (the player's
bars stand 4 units over theirs), between the corner marks, about 32 of them (31 here, as
many as fit); the actor's name is centred under the line in font 7, standing on y 53 of
the piece. [design: #1826] The hand-built HUD centres the name, and the Living Anatomy
line under it, on the ticks' middle (halfway between the corner marks, x 170.3 of the
piece), measuring the letters' pictures only, not the room after the last letter;
centred on the piece's width (x 175) and its set width, the name stood right of the bar
on the owner's screenshot (2026-11-01). On that hostile screenshot it is red: about (248, 68, 45),
`iSystemColorHUDAlt*` 255, 67, 42 (`reticle.md`).

[observed: private job hud_enemy3, n2_away, g2_gecko_half] The whole piece's colour
follows the actor shown: systemcolor 1 (HUD main) for a non-hostile, 2 (alternative)
for a hostile. [design] The hand-built bracket, ticks, name, Living Anatomy line and
fallback blocks follow that flag, including a hostility change without a name change.
History: §8 previously generalised the hostile screenshot to always red [hypothesis].
[design: #2696] The hand-built ticks are drawn at the meters' brightness 175 (§14.8's
enemy meter read), in the main or the alternative colour, with the tick picture's own
fade (its alpha rising from 34 at the top to 255 by row 17 [observed: the texture]), as
the health and action points ticks. Until #2696 they stood solid at full brightness:
the VATS blink's alpha was also set on each tick's transparent node background, which
made it an opaque block behind the picture.

[owner, playtest list 5, item 32] The bar empties from both ends towards its middle:
the lit ticks are the middle `ceil(value × 31)`, an odd one gone from the right first.

[design: maintainer's choice, T12b B/M1; needs an issue] Keep the 31 ticks and the
owner's `ceil` above (`hud::lit_middle`; the health and action points bars floor, §14.3).
[observed: private jobs
hud_enemy2, e5_back; hud_enemy3, n2_away, g2_gecko_half] The original has at most 30,
always even; `2 × floor(share × 15)` fits the captured shares. Draft issue: decide
whether to replace the owner's 31-tick rule with the measured even-tick rule.

**When it shows** [observed: private jobs hud_enemy4 (run 1793322469) and hud_enemy5
(run 1793323493), with hud_enemy2/3, in the unmodded original through xNVSE and JIP;
"shown" means `EnemyHealth`'s three code-made children (the name, the meter, the
bracket) read visible, which they always did together. "Under the crosshair" is the
crosshair's ray on the actor; "the activation target" is the reference GetCrosshairRef
returns, the one the prompt is for, within activation reach only [documented: GECK wiki
GetCrosshairRef]]:

1. **Never at full health.** No bar for an actor whose Health is at its base value,
   neutral or hostile, aimed at, the activation target or in combat (hud_enemy4 s1, x1,
   g1, g3, g5; hud_enemy5 a0, b1, c1; hud_enemy3 h1). Health below base was necessary
   in every sample of every run.
2. **Out of combat, a hurt actor under the crosshair** gets the bar, hostile or not, the
   weapon need not be drawn: a hurt neutral settler at 350 units and at 150 aimed at the
   chest, neither the activation target nor with a prompt up (hud_enemy5 a1, a2, a3). It
   appears 0.1-0.25 s after the crosshair comes on it. Every shown case had IsWeaponOut
   0; none was captured with the weapon drawn.
3. **Out of combat, not while it is the activation target, nor while any activation
   prompt is up**: the hurt settler as the target with his Talk prompt (hud_enemy4 s2,
   s3; hud_enemy5 a3a), a hurt restrained gecko as the target with no prompt (hud_enemy5
   b2), the same gecko under the crosshair while a prompt for something else was up
   (hud_enemy5 b2). Prompt up and bar shown together: no sample in hud_enemy4 or 5. When
   the target dropped with the crosshair still on the actor, the bar showed at once
   (hud_enemy4 s2h, s2a, s3; hud_enemy5 a3a, b2).
4. **In combat with the player** (its combat target is the player), a hurt actor keeps
   the bar aimed at or not, the activation target or not, the meter following its
   health, until it dies (hud_enemy4 g4; hud_enemy3 g2; hud_enemy5 b4). A dead actor has
   none (hud_enemy5 b4).
5. **After the crosshair leaves it**, out of combat, the bar stays on that actor for a
   while: a restrained gecko's about 5.5 s (hud_enemy5 b2), a neutral settler's at least
   16-18 s (hud_enemy5 a1, a2, a3, a3a: the watch ended first). An actor that was the
   activation target as the crosshair left had its bar for one frame only (hud_enemy4
   s2, s2h, s2a, s3), which fits the linger applying only to a bar that was shown
   [hypothesis].

[design] NVForever (`hud_target::why_shown`, a pure function of the case, tested point
by point): hurt is Health below its full value (the meter not full); "in combat" is the
actor's combat target being the player; the activation target is
`crate::activate::Target`, and a prompt is up whenever there is one (unless a script has
the rollover text off). The linger is `LINGER_SECONDS` = 5.5 s for every actor [observed
for the hostile; a known divergence for a neutral, whose bar the original kept at least
16-18 s (point 5): what makes the two differ is not known], and the activation target or
a prompt coming up ends it. The player's own hit keeps the earlier rule [hypothesis]:
for 5 seconds after the player hits an actor it has the bar, aimed at or not, prompt or
not, while it is hurt. No actor that is no longer here has it: disabled, deleted or
outside the loaded cells (`Case::gone`, as after a script's Disable or a load door), and
behind a loading screen the bar forgets the actor it held and the player's hits
(`Memory::enter`). One actor at a time (`hud_target::candidates`, tested in that order):
the one under the crosshair (a ray along the crosshair's line through the collision
world, up to 8192 units, to the first actor hit volume or capsule), else the one the
player hit last, else a hurt actor fighting the player (the one the bar holds, else the
nearest), else the one it lingers on. Logged as `hud: enemy health <name> 0x<ref>
<percent> %`, `hud: enemy health <name> 0x<ref> held by the linger (…)` when only the
linger holds it, and `hud: enemy health off`. The developer's stand-in (a line of `|`
characters over the crosshair, `combat.rs`) is gone.

Earlier runs agree [observed: private job hud_enemy3, n0_neutral_dwell, n1-n1c,
h1_hostile_combat, g2_gecko_half; hud_enemy2, e4-e6]: full-health actors showed no bar
(including a hostile in combat aimed at for 0.94 s); a hurt hostile in combat showed
without aim or a player's shot; a hurt non-hostile on target showed none for 5.6 s. The
hostility flag (`TargetHealth::hostile`, in combat with the player or
`Game::would_attack`) only selects the colour above [design: T12b B9]; it is no input to
whether the bar shows.

Still open (docs/handoff/observer-checks.md): the player's own hit (no capture had the
weapon out, so no shot hit); aiming down the sights; why the bar comes about 1 s after a
hurt in combat (hud_enemy4 g4, hud_enemy3 g2) but within 0.25 s out of it (not built:
NVForever shows it at once); how long a neutral's bar lingers, and what ends it; what
`EnemyHealth`'s own visible value 8 means (it read 1, then 8 after a held Ready or right
mouse button, and stayed 8 with nothing pressed).

## 9. The sneak word

[documented: hud_main_menu.xml] `SneakMeter` is a piece 356 x 34 with a model in it
(`sneak_nif`); the template `sneak_bracket` (`glow_stealth_metre_seperator.dds`) is a
bracket like the enemy's. [observed: the owner's four screenshots] The original shows
neither: only a word in brackets, centred at the top of the view, its line at y 51 from
the top, in font 7: one of four words, the two calm ones in the HUD's colour, the two
warnings in its alternative red. The words are not in `FalloutNV.esm`'s settings
(they would be the program's defaults), so NVForever writes its own [design, #2984]:
`[UNSEEN]`, `[SPOTTED]`, `[ALERT]` and `[COMBAT]`.

Shown while the player sneaks (`PlayerControl::sneak`), with the rest of the HUD. Which
word [hypothesis: the meaning players give the words; not documented]: `[COMBAT]` when
an actor is in Combat with the player as its detected target (an enemy left in an unloaded
cell, or out of reach, detects the player no more: alert.md §2, #3390); else `[ALERT]` when an actor in
Alert or Lost has the player among its threats or an alarm the player caused
(`fnv_game::awareness`); else `[SPOTTED]` when some actor's latest detection of the
player is in the detected band (`Game::detection`, value above 0); else `[UNSEEN]`.
Teammates and actors whose current package is a Follow or Accompany package on the
player (`Game::with_player`) count for neither `[ALERT]` nor `[SPOTTED]`; their
detection is still measured and kept for their own AI [observed: owner's playtest
2026-10-31, #1159: a following companion alone set `[SPOTTED]`, which the owner
reports the original does not; hypothesis: the original leaves companions out by the
same rule, observer-checks.md P3 16]. Other actors, hostile or not, still count
[hypothesis: a neutral seeing the player is what theft and pickpocketing care about;
not checked in the original, observer-checks.md P3 16]. An actor with the player that
is in Combat with the player still gives `[COMBAT]` [design].
Logged as `hud: sneak [WORD]` on each change. (`fnv_game::detection::Band::meter`
names the bands HIDDEN, CAUTION, DANGER for the detection log; that older mapping is not
what the HUD shows.)

## 10. In dialogue

[owner, playtest list 5, item 74] While the player is in a conversation every HUD
element goes: the corner pieces, the enemy bar and the sneak word (`PlayState::Dialogue`
is not `Playing`), the crosshair (`reticle.rs`), and the messages at the top left — the
corner messages and the quest messages (`corner_messages.rs`, `quest_messages.rs`)
wait, their time held, and come back when the conversation ends [hypothesis: whether the
original holds them or shows them over the dialogue screen is not checked]. Other
actors' lines (Joe Cobb's to Trudy while the player talks to her) and the radio's are
not drawn as subtitles; they stay in the log's `subtitle:` lines with
`(not drawn: in dialogue)` after them. The conversation's own line is the dialogue
menu's (`dialogue_menu.rs`).

## 11. Compass marks and discovered places (owner's playtest list 5, items 12 and 57)

- [documented: `menus\prefabs\hudtemplates.xml`, `compassicon.xml`] The compass has
  groups of icons: `compass_npc_icon` (`glow_hud_tick_mark.dds`, 32 × 32, y −10 in a
  group at y 5), the map marker icon (`glow_hud_compass_landmark.dds`, 40 × 50 at y 5),
  the quest target and the player's marker. An icon's x is `_Heading`, "heading, in
  compass pixels of this object, based on compass width in pixels", and `_Distance` the
  distance to it; both are set by the code.
- [observed: the owner's screenshots in `C:nv-private\Creature Markers` and
  `Map Markers`] An actor is a tick over the strip, red when hostile and amber (the HUD's
  colour) otherwise; a map marker is a small triangle under the letters, hollow before
  the player has been there (`glow_hud_compass_landmark.dds`) and filled after
  (`…_landmark_discovered.dds`). The triangle's size and place are fitted to those
  screenshots: drawn at 0.6 of its texture with its middle 46.7 down the window. So were
  the tick's until #3310 (26 units high, its top 9 below the window's).
- **The actor ticks' box** (#3310; the hand-built HUD, `compass::mark_box`; the tile HUD
  draws the file's box).
  - The template's actor group stands at y 5 [documented: `hudtemplates.xml`,
    `template_compass_npc_icon_group`], but the copy the original's code makes reads y 10
    at run time [observed: §14.4, read from the original], which NVForever writes
    (`ui_hud::copies`). Each tick is at y -10 in the group, 32 x 32 [documented:
    `hudtemplates.xml`, `compass_npc_icon`], so the box's top is the window's top.
  - The picture's bar is whole in texel columns 6-11 and rows 4-26, its alpha and colour
    rising downwards to full at rows 22-26; row 27 has alpha 184 of 255 and a colour of
    129, row 28 125 and 68 [observed: `glow_hud_tick_mark.dds`, 32 x 32, from `Fallout -
    Textures2.bsa`, decoded]. Drawn a texel a unit it is 6.2 units wide and ends 27 down
    [both worked out from the texels, at half contrast over a mid-grey ground].
  - [observed 2026-11-24: the original, base game + xNVSE 6.4.9 + JIP LN, save hudtest,
    Goodsprings at night looking up 60 degrees into a black sky, 1920 x 1080 windowed,
    1.125 pixels a unit, private job `compass_310`, grabbed at native size]
    - The visible tick (`compass_window/*:0/*:8`) reads x 177.88, y -10, width and height
      32, zoom -1, depth -10, alpha and brightness 255; its group y 10, depth 2; the window
      y 65, height 64, depth 1.
    - With the strip and the bracket set to alpha 0 the tick's column of pixels is the
      picture's colour times its alpha, drawn 37 pixels high (36 rounded up) from 976.5
      pixels down the screen, to 0.004 of full brightness (rms over 38 rows); the same
      picture 26 units high does not fit. Its bar ends at about 1007.6, the strip's upper
      line starts at 1009: the strip's line is at rows 29 and 30 of its 64, so the window's
      top is at 976.4 and the two agree.
    - The strip and the bracket are drawn over the tick (its depth is -10): their dark
      halos (alpha over half within four rows of a line) dim the bar's top and its last
      five or so pixels, which is why its end reads softer than the picture's.
  - [design] The hand-built tick has that box: 32 x 32 at the window's top, its bar's
    middle on the bearing, under the bracket and the strip (`hud::CompassTicks`, #3777). At
    1920 x 1080 it is 36 pixels, so its bar ends a pixel above the original's [observed: a
    run beside the frame above]; why the original's is 37 is not known. Before #3310 it was 26 units high with its top 9 down, the bar
    ending 30.9 down, on the strip's line (a player: slightly low). #3310's first change
    put its top at 4.125, taking the bar for the top 22 of 32 rows, which the decoded
    picture does not bear out: it ended 26.1 down and was too thin (the same player: now
    too high).
- [hypothesis] Which actors: the living NPCs and creatures of the loaded cells within
  Perception × 300 units of the player; no game setting for this range is in the
  masters. Hostile is the enemy bar's rule (§8). Each mark stands at its bearing from the
  player's heading at 1024 units for 360 degrees, the strip's scale.
- Which markers: those of the worldspace within `iMapMarkerVisibleDistance` of the player
  (measured across the ground), found and unfound alike and whatever the Perception,
  while outdoors. The setting is not in the masters; its default in the executable is
  10000, and `iMapMarkerRevealDistance`'s 1000 [documented: GECK wiki "Complete List of
  GameSettings"; the wiki does not describe either]. NVForever reads the load order's
  setting and falls back on 10000 (`engine/nvf-play/src/compass.rs`, `marker_range`).
  That this setting is the compass's cut-off for markers is [hypothesis]: its name, and
  Oblivion's setting of the same name; nothing on the wiki ties the marker range to
  Perception (its SPECIAL page links Perception only to hostile marks on the compass).
  Observer check 227 asks the original (#2348). Until #2348 the range was Perception ×
  2048, built for the owner's playtest list 5 item 12 ("range by Perception"); the
  change back to the original's fixed range is in docs/owner-review.md for the owner.
- [observed: the owner, 2026-10-23] An actor that sneaks gets no tick. NVForever's
  "sneaks" is IsSneaking's (`fnv_game::Game::is_sneaking`: the engine's measure, else
  SetForceSneak, an Always Sneak package, a crouch to fire or reload, a teammate
  sneaking with the player); whether the original also hides the crouch-to-fire case
  is not known [hypothesis].
- [observed: the owner's screenshot "Discovered"] Finding a place shows "You have
  discovered" over its name in large capitals, where and as "Quest added" is drawn
  (quest-messages.md), and "+10 XP" (`iXPRewardDiscoverMapMarker` is 10 in
  FalloutNV.esm). A place is found when the player comes within its marker's radius
  (`XRDS`), as the Pip-Boy's map counts it (pipboy.md §5.3); a marker without `XRDS` (65
  of WastelandNV's 186 named ones, Coyote Den among them) within `iMapMarkerRevealDistance`,
  1000 by default [documented: GECK wiki "Complete List of GameSettings"; that it is this
  distance: hypothesis, #2338, observer check 128]; saves keep the places found.
  The sound is `UIPopUpMapMarkerAdded` [hypothesis: by its name].
- [documented: `menus/prefabs/hudtemplates.xml`, `template_compass_icon_quest`] The
  quest target's icon is `glow_hud_compass_objective_marker.dds`, 40 x 50 at y 5 as the
  map marker's, at depth 5 over the map marker's 3. Built 2026-10-23 (playtest list 7
  item 44): the active quest's marked targets (the Pip-Boy's, conditions passing,
  quests-dialogue.md §2) at any distance, drawn in the map marker's box over the other
  marks [hypothesis: the box, and no distance limit]; a target in another place (an
  interior, another worldspace) at the door of the player's place that starts the
  shortest way to it through load doors, up to 8, avoiding locked doors unless the target
  "Ignores Locks" (`fnv_game::quest_targets::DoorGraph`, 2026-10-25) [hypothesis: the
  shortest way; one step of doors before]. No distance figure: the icon file sets
  `_Distance` "set by code, distance to this object in game" and no trait reads it, so the
  original's compass shows none [documented: `menus/prefabs/compassicon.xml`, Fallout -
  Misc.bsa].
- [observed: hud_compare hud-bars-partial, hud-ammo-label, perk-meltdown-anatomy,
  xp-meter at 1280 x 720] A quest target off the compass's band is still drawn, held
  under the line's end: in all four shots at the left end, its middle at x 53.5-54
  pixels (the band's end, `_Heading` 0 with the icon's 40-unit box); in range it stands
  at its bearing (needs_limbs, at SE, full brightness where the strip beside it fades).
  [hypothesis] A target to the right is held at the right end. Built T12e
  (`compass::pinned`, both drawings through `CompassView`); the log line counts them:
  `… <m> on the compass, <k> held at an end`. Until T12e an off-band target had no mark
  [hypothesis: the actor ticks' band, §14.5]. None of NVForever's comparison scenes had a
  quest target (`compass: 0 quest target(s) placed`): no quest is started in them; the
  original's save has one.
- [design, T12d] Who and what is near is looked at ten times a second; where each mark
  stands is worked out every frame from the camera's heading, the player's place and the
  actors' moving bodies, unrounded (`compass::place`), before the tile HUD's writes, so
  both drawings move the marks with this frame's strip. Until T12d the places were whole
  units and only ten a second: the marks stepped while turning (§14.9).
- [documented: the HUD file's `compass_icon` kinds (§2) and `template_compass_icon_player`
  in `hudtemplates.xml`] The player's marker is the custom map marker (the Pip-Boy's
  Place Marker, SetCustomMapMarker; pipboy.md §5.3), drawn as
  `glow_hud_compass_pc_marker.dds`. [hypothesis] NVForever marks it only while it is in
  the player's interior or worldspace (no door is followed to it), lays it out and holds
  it at the band's ends as a quest target, and draws it over the quest marks, as the
  Pip-Boy's maps do (`compass::Mark::Player`). Which the original does is open: a
  capture with a marker placed behind the player, and one placed in another cell, says.
- Log: `compass: <n> actors (<h> hostile), <m> map markers (<f> found) on the compass,
  Perception <p>, <s> sneaking left off` when the counts change; `compass: discovered <name> (<FormID>), +<xp> XP`;
  `compass: <n> quest target(s) placed, <m> on the compass, <k> held at an end` when
  those change.

## 12. The crippled-limb picture (owner 2026-10-17)

`engine/nvf-play/src/hud_limbs.rs`; smoke `hud-crippled-limbs`.

- [documented: `hud_main_menu.xml`, `crippled_limb_indicator`] A hidden rect of seven
  images: `Interface\Stats\head.dds`, `face_00.dds`, `torso.dds`, `left_arm.dds`,
  `right_arm.dds`, `left_leg.dds`, `right_leg.dds`, all at alpha 0; places and showing are
  left to the code. The `_broken` pictures (dashed outlines) are in the same folder and in
  `interfaceshared.tai`.
- [observed: the owner's 1920 × 1080 screenshot of the original] The Vault Boy at the top
  right, his pieces as on the Pip-Boy's CND page (pipboy.md) at 0.74 of their unzoomed
  size (0.735 by the width, 0.744 by the height); his ink 100.5 units from the view's
  right edge and 170.7 from its top. The crippled right leg is drawn dashed in a dim red
  (about 105, 30, 30), the other pieces solid in the HUD's amber. Drawn over the
  screenshot, the pieces at that size and place fall on the original's lines.
- [design: owner 2026-10-17] The picture shows for 5 s after a limb is crippled; the limb
  just crippled flashes red (dashed), limbs crippled before are dashed in amber, whole
  limbs solid. A second crippling while it shows starts the 5 s again and only the new
  limb flashes. Healing shows nothing. The time stands while the HUD is off (menus,
  scripts holding the player).
- [observed: private job hud_xp_limbs, l1_leg, l2_head; one run] Whole pictures fade
  0.85 s in, stay full 3.3 s, fade 0.85 s out within the owner's 5 s; the face shares
  this envelope [design]. History: instant show/hide [design].
- [observed: private job hud_xp_limbs, l1_leg, l2_head; one run] Broken pictures flash
  in alpha 0-255 for about 4.3 s, then negative alpha. [hypothesis] A rising sawtooth
  of about 0.54 s fits the samples. [design] Apply the measured flash to the owner's
  newest limb only, under the whole pictures' envelope (it fades in with them), and hold
  that limb red at the envelope after 4.3 s, so it fades out with the picture and leaves
  no hole in it (maintainer's review of #902; what the original draws at negative alpha
  is not known). History: negative alpha clamped to hidden [design]; brightness cosine,
  low 0.35, period 0.6 s [design].
- [design: owner 2026-10-17; maintainer's choice T12b B/M3; needs an issue] Keep only
  the newest limb flashing and earlier broken limbs amber. [observed: private job
  hud_xp_limbs, l2_head; one run] Earlier broken limbs flashed red in step with the
  newest. Draft issue: decide whether all crippled limbs should flash together.
- [observed: the pictures] The faces grow worse from `face_00` (the smile) through 01
  (worried), 02 (grimace), 03 (crying) to 04 (eyes crossed out); `face_10` has the tongue
  out. The screenshot shows `face_02` at about 15 % health (6 of 37 ticks).
  [hypothesis: owner 2026-10-17, health picks the face] Above 80 % 00, above 60 % 01,
  above 15 % 02, above 0 03, else 04; `face_10` is not used. The Pip-Boy's CND page still
  shows `face_00` always.
- The brain (condition 31) has no piece.
- Log: `hud: crippled limbs shown, flashing <pieces> (crippled: <limbs>)`,
  `hud: crippled limbs hidden`. The stand-in line of words ("crippled: …", `combat.rs`)
  is gone.

## 13. Hardcore's need words, the radiation word and the limb word (owner 2026-10-23)

`engine/nvf-play/src/hud_needs.rs`; smoke `hud-needs-words`.

- [documented: `hud_main_menu.xml`, `HardcoreMode`] A rect 386 x 20 with no place (set in
  code), holding five texts, each font 7, right-justified, in the HUD's colour, visible,
  "Set it code!": `Dehydration` (`sStatsH20Abbrev`), `Hunger` (`sStatsFODAbbrev`), `Sleep`
  (`sStatsSLPAbbrev`), `Rads` (`sStatsRADAbbrev`) and `LMBs` (`sStatsLMBAbbrev`). So the
  original's file already has a limb word in the Hardcore row.
- [observed: FalloutNV.esm and the five add-on masters] None of the five settings is in a
  master; the game's own defaults are not readable here. The words drawn are a plugin's
  setting if one sets it, else `H20`, `FOD`, `SLP`, `RAD`, `LMB` [observed: §14.2].
  `RAD` stands in the file's order, between SLP and LMB [owner 2026-10-23: RAD with the other indicators,
  whether or not the original drew it].
- [observed: private job hud_hardcore2, hc0-hc11] The piece stays at view (190, 803),
  with words at x 0, 55, 109, 162, 218 and y 0, zoom 75 in every mode, including
  hidden words. Each x is the preceding x + its width + 10. [documented:
  hud_main_menu.xml, HardcoreMode] Right-justified. [hypothesis] The hand-built drawing
  treats x as the right end. History: two font spaces after HP, one between, on
  UPPER_BASE [hypothesis].
- [observed: hud_compare needs_limbs at 1280 x 720; the font] **The zoom 75 does not
  shrink the words.** The original's are 17 pixels tall (y 603-619), as tall as its
  "HP", their right ends at x 145, 186, 226, 266, 308; the widths the capture read (45,
  44, 43, 46, 43) are font 7's at 100 ("H20" 45, "LMB" 43, "HP" 30). T12b drew them at
  0.75 (13 pixels, their starts right of the original's) [design, corrected T12e]: both
  drawings draw them at the font's size, the first line font 7's 24 (§14.10) under the
  piece's top (the capitals' top a pixel under it, as the original's) [hypothesis].
- **Looks** [design: owner 2026-10-23]: faded (dim), normal amber, red, flashing red.
  [observed: private job hud_hardcore2, hc2, hc6; hud_radiation2, r_in_200, r_in_300]
  Faded alpha is 124/255. [observed: private job hud_hardcore2, hc5] Flashing words
  keep the alternative red and vary alpha from 128/255 to 1. [hypothesis] The word
  period remains unresolved: retain the 0.6 s cosine separately from the limb flash.
  History: faded 0.3; flashing shared §12's brightness pulse [design].
  A word with nothing to say is not drawn [design: owner 2026-10-24].
- **A need's look** follows the stage the game holds for it (aid-items.md §11: the
  DEHY/HUNG/SLPD records' thresholds 200 … 1000, Minor, Advanced, Critical, Deadly,
  Fatal) [design: owner 2026-10-23, "none/lowest faded, then normal, red, flashing for the
  last ones"; owner 2026-10-24: "faded, not faded, red, red flashing starting at the 200
  mark"]: no stage (under 200) hidden, Minor faded, Advanced normal, Critical red, Deadly
  and Fatal flashing. Counted from the worst stage, so a plugin's shorter list loses the
  faded stage first.
- **The radiation word** follows the radiation sickness stage the game holds (the RADS
  records' thresholds 200 … 1000, Minor … Fatal; held in every mode, aid-items.md §11)
  by the needs' rule [design: owner 2026-10-23, "same rules"; owner 2026-10-24]: under
  200 rads hidden, Minor faded, Advanced normal, Critical red, Deadly and Fatal flashing.
- **The limb word** follows how many of the six limbs (conditions 25–30) are crippled
  [design: owner 2026-10-23; owner 2026-10-24: hidden with none]: 0 hidden, 1 normal,
  2 red, 3 or more flashing. The brain (31) is not counted [design: not a limb; the
  picture of §12 has no piece for it].
- [design: owner 2026-10-23/24; maintainer's choice T12b B/M2; needs an issue] Keep the
  limb word's looks above. [observed: private jobs hud_hardcore2, hc9-hc10;
  hud_xp_limbs, l1_leg, l2_head] One limb has alpha 124, systemcolor 1; two alpha 255
  (two-limb colour and 3+ not read). Draft issue: decide whether LMB should use the
  observed faded/normal looks for one/two crippled limbs. [observed: hud_compare
  needs_limbs and needs_off, an arm and both legs crippled] The original's LMB is the
  alternative red at full alpha with three: the hand-built "flashing" at three is the
  owner's look, kept; the tile HUD draws three or more red (§14.10).
- **When they show**: H20, FOD and SLP only in Hardcore [design: owner]. RAD in every mode
  [design: radiation sickness is not a Hardcore rule]. LMB in every mode
  [observed: private job hud_hardcore2, hc9-hc10]. RAD and LMB keep their x 162 and
  218 outside Hardcore [observed: private job hud_hardcore2, hc0-hc11]. History:
  compacting them into H20/FOD's places was NVForever's choice [design, no owner request].
- Log: `hud needs: <word> <hidden|faded|normal|red|flashing> (<why>)` when a word's look
  changes, the why being `stage <n> of <count>, <stage effect>`, `no stage`, `Hardcore
  off` or `<n> limbs crippled`.

## 14. Read from the running original through xNVSE (2026-10-28)

[observed: `tools/capture/hud_traits.py` job, run 1793221057 on the owner's test install
(game 1.4.0.525, xNVSE 6.4.9, no JIP or SUP), save outside Doc Mitchell's house, health
200/200, AP 80, 20 samples, 1920 x 1080 windowed (the plugin scales its screenshots to
1280 wide [documented: nvf_capture README]); then the same with a 9mm pistol equipped by
script at 1920 x 1080, 1024 x 768 and 1280 x 800 (`FalloutPrefs.ini` `iSize` changed for
the run and put back). Results private, in `C:\fnv-private`.] Places are
in interface units: the view is 960 high at every size, so 1280 wide at 4:3, 1536 at
16:10, 1706.7 at 16:9.

- **The reads work.** A known trait reads its value and a missing one -999 with GetUIFloat
  and GetUIFloatAlt; GetUIFloatInherited gives the wiki page's own example exactly (-50,
  and 0 with `ExcludeMenuRoot`).
- **The corner pieces.** `HitPoints` stands at x 40, y 803 at all three sizes: with its
  height 127 its bottom is 30 above the view's, as §4 guessed. `ActionPoints` stands at
  y 803 with its right edge on the view's (x 894 at 4:3, 1150 at 16:10, 1320 at 16:9: the
  view's width less 386, in whole units), not 40 in from it as §4 guessed. The brackets'
  pictures stand inside their pieces, so on the screen the current HUD's right corner
  still lands where the original's does (below).
- **The middle.** `ReticleCenter`'s child (64 x 64) is centred less half a unit (607.5 at
  4:3, 735.5 at 16:10, 820.5 at 16:9); `SneakMeter`'s child stands at the middle (640,
  768, 853); `EnemyHealth` is centred (its 500-wide child at the middle less 182).
- **What the code makes under them** (`*:N` children; which is which is
  [hypothesis] by size and place): under `HitPoints` four tiles, one 296 x 20 at x 20,
  y 41 (the bar) and one 512 x 256 at the piece's corner (the bracket picture); under
  `ActionPoints` seven, among them 296 x 20 at x 30, y 41 (the bar) and 60 x 20 at x 90,
  y 83 (the condition bar's ground, the file's size).
- **Against the current HUD** (§4), on the view: the HP bar starts 1 unit further left
  (60 against 61), the AP bar 2 units further right (1350.7 against 1348.7), the
  condition ground 7 units further left (1410.7 against 1417.7). The first two are within
  the screenshot measurements; the third is not, and is fixed: a second run with a 9mm
  pistol equipped by script shows the ground (60 x 20, `visible` 1, behind) and its fill
  (59 wide, in front) at x 90 of `ActionPoints`, and on its screenshot the bar spans
  1058-1102 pixels and "CND" ends at about 1053, the divider at 1114-1116: the bar ends
  17 units before the divider, not 10 (§4).
- What the code changed against the file alone, among 31 traits: the prompt box
  (`Info/justify_center_hotrect`) sized to its text (176 x 52), `SneakMeter/sneak_nif`
  14 x 36 (the file says 128 x 128), `CNDArrows` 20 high (16), the Hardcore words'
  `alpha` 0 (not in Hardcore mode), and the lines' thickness 1.78 at this view.
- **Every piece's place.** [observed: a third run reading `x`, `y`, `width`, `height`,
  `visible` on every tile, set in the file or not, with the pistol, at 4:3, 16:10 and
  16:9.] The code sets each piece's `x` and `y` (the file has none); `y` is the same at
  every size; `x` follows one of three rules, W being the view's width (1280, 1536,
  1706.7) and the result in whole units where W is not whole (1320 at 16:9):

  | Piece | Size | x | y |
  |---|---|---|---|
  | `HitPoints` | 369 x 127 | 40 | 803 |
  | `ActionPoints` | 386 x 127 | W - 386 | 803 |
  | `HardcoreMode` | 386 x 20 | 190 | 803 |
  | `SneakMeter` | 356 x 34 | W/2 | 30 |
  | `ReticleCenter` | 63 x 75 | W/2 - 34.5 | 439.5 |
  | `EnemyHealth` | 350 x 52 | W/2 - 175 | 843 |
  | `QuestReminder` | 400 x 200 | 45 | 200 |
  | `Messages` | 460 x 90 | 25 | 55 |
  | `Subtitles` | 800 x 110 | W/2 - 400 | 660 |
  | `Info` | 320 x 80 | W/2 - 160 | 820 |
  | `Hokeys` | 300 x 300 | W - 371 | 483 |
  | `Region_Location` | 100 x 100 | 45 | 728 |
  | `XPMeter` | none | W - 420 | 650 |
  | `RadiationMeter` | 367 x 60 | W - 417 | 30 |
  | `BreathMeter` | 64 x 64 | W/2 | 110 |
  | `Explosive_positioning_rect` | none | W/2 | 480 |
  | `crippled_limb_indicator` | none | W - 230 | 130 |
  | `DDTIcon` | 32 x 32 | 414 | 852 |
  | `DDTIconEnemy`, `DDTIconEnemyAP` | 32 x 32 | W/2 - 207 | 846 |
  | `AmmoTypeLabel` | 13 x 42 | W - 236 | 883 |
  | `CNDArrows` | 16 x 20 | W - 253 | 886 |

  `DDTIconAP` has no place (not shown in this scene). The sizes are the file's where it
  gives one; the code changed `CNDArrows`' height (16 in the file).
- **`visible` holds more than 0 and 1.** The pieces' `visible` read 1, 2, 4, … 65536, a
  different power of two for most (`ActionPoints` 1, `HitPoints` 2, `RadiationMeter` 4,
  `QuestReminder` 16, `Region_Location` 32, `ReticleCenter` 64, `SneakMeter` 128,
  `Messages` 256, `Info` 512, `Subtitles` 1024, `Hokeys` 2048, `XPMeter` 4096,
  `BreathMeter` 8192, `Explosive_positioning_rect` 16384, `crippled_limb_indicator`
  32768, `HardcoreMode` 65536); `EnemyHealth`, `AmmoTypeLabel` and `CNDArrows` 1, the
  four armour-piercing icons 0. What the number means beyond "not 0" is not known
  [hypothesis: one bit per piece, as a mask the code tests].
- **Constants' numbers.** `&true;` reads 1 and `&false;` 0 (`HUDMainMenu/locus`,
  `_CrosshairVisible`), `&hudmainmenu;` 1004 (as the wiki's menu class IDs),
  `&does_not_stack;` 6008 (menu-xml.md §8 item 1).
- The root's `visible` (`&false;` in the file) reads 1 while playing; `_CrosshairVisible`
  and `_SneakVisible` read 0, `_SneakLevel`, `_RadiationLevel`, `_TargetType` and
  `_ActionType` 0, `_SpreadPercentage` 1 standing still with a pistol.
- [design] T4a (`fnv_game::ui_hud`, `nvf-play/src/tiles.rs`) writes these places and
  the pieces' visibility before scripts, beside the current HUD drawing. Pieces not
  drawn yet keep the file's visibility. The root shows in play, outside menus,
  dialogue, loading and pause/main menus; with it hidden the pieces' `visible` is 0
  [observed: §14.3, a menu open]. Each piece keeps its value whether it shows or not, and
  what shows is written on the children observed in §14.1. The engine mirrors
  `hud::gather`/`draw`/`texts`, `reticle::draw`, the needs, limb, breath, radiation and
  XP drawing, and the message queues, for those children and the ones still to capture;
  subtitles share the drawing's selection. Discovery notices map to `Region_Location`,
  other quest notices to `QuestReminder` [hypothesis: NVForever draws both in one queue].
  Since §14.11 the tile HUD writes a discovery's copies under both `Region_Location` and
  `QuestReminder` and draws `QuestReminder`'s, where the owner's screenshot shows them;
  `Shown::location` still only tells which kind of line shows.
- [hypothesis] T4a truncates W and W/2 before subtracting offsets: at 16:9,
  `1706.67 - 386` truncates to 1320 (nearest would give 1321); the truncated middle
  853 keeps the reticle's half-unit offset and reproduces its child's x 820.5.
  This fits the three captured sizes; the original's arithmetic at other sizes is open.
- Open: a resize while playing; the children's traits other than places (strings,
  filenames: they need GetUIString, JIP); an enemy targeted (the root's values in
  aiming, sneaking, radiation and with an NPC under the crosshair are §14.1).
  Whether the original overwrites script-set places/visibility each frame is not
  observed; nor what the pieces' `visible` read while the original's console is open
  (NVForever's console is a `PlayState::Menu`, so its HUD and these values are hidden). T4a writes them every frame, so SetUIFloat holds until the next frame's
  write [hypothesis: menu-xml.md §6 / ui-tile-tree.md §4.2]; traits left to XML keep
  their existing expression-change behaviour.
- A script's string on a need word stays [observed: the jip-probes set gaps1 (golden gaps1.json):
  `SetUIString "HUDMainMenu/HardcoreMode/Dehydration/string" "NVF one"`, then
  `SetUIStringAlt … "NVF %g" 2` read "NVF 2" in a later step (frames later), and "H20" only
  after the probe wrote it back], out of Hardcore at least. So the original does not write
  the need words every frame, or writes them only when they change [hypothesis]. Since
  #1026 `ui_hud::write` writes a need word only when the HUD's own word changes
  (`Ui::write_changed`), so a script's string lasts until then. Open: whether a stage
  change or a mode change rewrites it in the original (not probed yet).

### 14.1 States: drawn, aiming, moving, sneaking, a prompt, radiation (2026-10-29)

[observed: a private `hud_states` job built from §14's full read, run 1793257932 on the
owner's test install (game 1.4.0.525, xNVSE 6.4.9, no JIP or SUP), save `hudtest` with the
9mm pistol, 1920 x 1080 windowed. Each state read every tile and trait of §14 four to
eight times, with screenshots; each also read IsWeaponOut, IsSneaking, IsControlPressed 6,
GetRadiationLevel, the player's position and GetCrosshairRef, so a state that did not
take shows. Results private, in `C:\fnv-private`.] The states, in order: the pistol
holstered; drawn (TapControl 7); aiming (the key of control 6, Block, held: the right
mouse button, scan code 257 from GetControl 6 1); walking forward about 1,000 units and
stopping; sneaking with the pistol drawn and holstered (TapControl 8; holding the Ready
key holsters, tapping it reloads); a Goodsprings settler 120 units ahead, under the
crosshair (prompt "E) Talk"), holstered, drawn and aiming; standing in the nuclear test
site's shack (`coc SLNuclearTestShackINT`).

- **The root's own traits never changed.** `_CrosshairVisible`, `_SneakVisible`,
  `_SneakLevel`, `_RadiationLevel`, `_TargetType`, `_ActionType`, `_SpreadPercentage`,
  `user7` read the same (0, and `_SpreadPercentage` 1) in every state, with both
  GetUIFloat and GetUIFloatAlt. The code shows these states through the pieces and their
  children; it writes none of these root traits in them [observed]. Whether it writes
  them anywhere else (VATS, other weapons) is open.
- **A piece's `visible` keeps its value whether it shows or not.** `SneakMeter` read 128
  standing and sneaking, `Info` 512 with and without the prompt, `RadiationMeter` 4 in
  and out of radiation (and §14's `BreathMeter`, `XPMeter` and limb indicator read their
  values while not shown). Only aiming changed a piece: `ReticleCenter` 64 to 0 (the
  crosshair hidden, the gun up to the eye) and `EnemyHealth` 1 to 8, both back on
  release, with or without the settler under the crosshair.
- **What shows is the children.** Sneaking: `SneakMeter/sneak_nif` alpha 0 to 255 while
  sneaking, back to 0 on standing; its width 14 (the file's), 106 with the word
  `[HIDDEN]` on the screen, then 136, and it keeps the last width after standing (the
  word's width [hypothesis: the width of the drawn text]). Radiation (+9 RAD/SEC on the
  screen): `radiation_text`, `radiation_text_value`, `radiation_pointer` and
  `radiation_bracket` alpha 0 to 255; `radiation_pointer` x moves from 15 to 36 and on
  to 42.9 over 5 samples as the rads add up; `radiation_text_value` 50 wide, then 49.
- **The prompt** (the settler under the crosshair): `Info/justify_center_hotrect` moves
  from x 72 to 123 and from 176 to 74 wide (box, top and bottom with it; `button_text` x
  84 to 33, 156 to 54 wide); `visible` 0 to 2 on the hotrect; the code-made children
  `Info/*:1`-`*:4` change width (13 to 45, 14, 32, 14), `*:5` and `*:6` go visible 1 with
  alpha 0, `*:7` (y 2, 269 wide) visible 1. The prompt's sizes stay after it goes;
  `PCShortcutLabel` showed (visible 1, x -18 to -28) in the shack with nothing under the
  crosshair. Which child is which text is [hypothesis] until the strings are read (JIP).
- **Subtitles move up 50 when the prompt shows**: `Subtitles` y 660 to 610, and it stayed
  610 for the rest of the run, the shack included [observed]. When it goes back is open.
- **Drawn, walking and stopping changed nothing read.** With the pistol drawn, walking
  about 160 units a sample and stopping, no trait of the reticle's tiles or the root
  moved; the crosshair's spread is not in `x`, `y`, `width`, `height`, `alpha`,
  `visible` or `depth` of the tiles read [observed]. Its other traits (`zoom`, `user`
  slots of the children) are open.
- [design] T4a writes the pieces' values as above and the alpha of `sneak_nif` and the
  four radiation tiles (`Shown::sneak`, `Shown::radiation`); the reticle 0 and the enemy
  bar 8 while `AimZoom::aimed`. The prompt's and the sneak word's text widths wait
  for measurement; the provisional pointer and subtitle rules are now in §14.6.

### 14.2 The strings, through JIP (2026-10-29)

[observed: the private `hud_jip` job, run 1793261986: §14.1's states job again on the same
install with JIP LN NVSE 57.3 added (GetPluginVersion "JIP NVSE Plugin"), reading
`string`, `filename` and `texatlas` with GetUIString on every file tile and on the 12
code-made children of the nine pieces (501 reads a sample).]

- **JIP changed none of xNVSE's reads.** Every state's 2,864 numeric reads matched
  §14.1's xNVSE-only run, except values that follow the moment: one message's text width
  (289 against 281), the sneak word's width (the word then shown) and the radiation
  pointer (42.9 against 42.0, the rads taken by then). By the JIP audit's rule
  (observer-jip-audit-2026-10-25.md) the strings below can be used.
- **What the code writes where the file leaves it empty** (the file's `string` traits
  are `&-s…;` game settings, empty in `fnvtool ui` until T10): `HitPoints/*:1` "HP",
  `ActionPoints/*:4` "AP", `*:2` "CND", `*:3` the ammo counter ("13/60"),
  `XPMeter/XPLabel` "XP", `XPLevelUp` "LEVEL UP", `RadiationMeter/radiation_text`
  "RAD/SEC" (already before any radiation) and `radiation_text_value` "+0", then "+9" at
  9 rads a second; the Hardcore words `HardcoreMode/Dehydration` "H20" (a zero, not an
  O), `Hunger` "FOD", `Sleep` "SLP", `Rads` "RAD", `LMBs` "LMB"; `Subtitles/*:0` " ".
- **The pictures the code sets** (`filename`, with `texatlas`
  `Interface\InterfaceShared.tai` on the glow pictures): the corners' separators, tick
  marks and the compass strip (`HitPoints/*:0`
  `Interface\HUD\glow_hud_comp_direction_strip.dds`), the AP bar
  (`ActionPoints/*:1` `Interface\VATS\vats_bar.dds`), the crosshair
  (`ReticleCenter/*:0` `Interface\HUD\glow_crosshair.dds`, **`glow_crosshair_filled.dds`
  while something that can be activated is under it**), the message icon
  (`Messages/*:2`, the Vault Boy's mood picture).
- **The prompt** (§14.1's settler): `PCShortcutLabel` "E)", the hotrect and its
  `button_text` "Talk" (the file's "Button Text" until then), `Info/*:8` "Talk",
  `Info/*:7` the name "Goodsprings Settler", `*:1`-`*:4` "VAL", "--", "WG", "--" (value
  and weight; dashes for an actor), and waiting in `*:5` and `*:6` "[EMPTY]" and
  "Locked". The texts stay in the tree after the prompt goes (the shack, nothing under
  the crosshair, still read "Goodsprings Settler").
- **The sneak word**: `SneakMeter/sneak_nif` (and its `*:0`) "[HIDDEN]" while sneaking
  unseen; it keeps the word after standing, with alpha 0 (§14.1).
- `Messages/*:1` holds the corner message's text (the capture's own "NVF capture:
  running …").
- [design] T4b uses the observed "H20" for the dehydration fallback (`hud_needs.rs`,
  `sStatsH20Abbrev`), shared with the tile tree. The file tiles' strings above are
  written before scripts; absent sneak and prompt strings keep their last value.
  The code-made children are now built in the headless tree (§14.6).

### 14.3 The corners' bars, the ammo counter, the compass (2026-10-29)

[observed: the private `hud_corners`, `hud_corners2` and `hud_enemy` jobs (runs
1793264008, 1793264234, 1793264383) on the same install with JIP 57.3: §14.2's reads in
each state, plus `*:k/*:i` one level under HitPoints, ActionPoints, EnemyHealth and
ReticleCenter. Health set with DamageAV/RestoreAV, action points with DamageAV, the
weapon's condition with SetWeaponHealthPerc, one shot with TapControl 4, the heading with
SetAngle Z.]

- **The bars fill in whole ticks, rounded down.** The health bar (`HitPoints/*:2`, x 20)
  read 296 wide at 200 of 200, 144 at 100, 24 at 20; the action points bar
  (`ActionPoints/*:5`) 160 at 43.68 of 80, 192 at 52.15, 256 at 69.7, 296 full. Every
  width is `floor(share x 37) x 8` (a tick 8 units, 37 of them); rounding up or to the
  nearest misses (20 of 200 is 3.7 ticks and showed 3). The action points bar's x is
  `30 + 296 - width`: it stays on the right end and empties from the left; the health
  bar's x stays 20 and empties from the right.
- **The condition bar** (`ActionPoints/*:1`) is 59 wide at 99.9 % (the ground 60,
  §14), 30 at 50 %, 29 at 49.87 %, 6 at 10 %: `round(59 x condition)`. At 10 % its alpha
  moved between samples (260, 40, 35, 180, 115, 15, 75, 140): it flashes, by a value read
  above 255 at times.
- **The ammo counter** (`ActionPoints/*:3`) went from "13/60" to "12/60" after one shot.
- **The compass did not move in what was read.** At headings 0, 90, 180 and 270 no `x`,
  `y`, `width`, `height`, `alpha` or `visible` of the pieces or their children changed,
  the strip `HitPoints/*:0` included; one level deeper (`HitPoints/*:0/*:0`) reads -999,
  so xNVSE's `*:N` paths do not reach there or nothing is there. How the strip scrolls
  (a texture offset trait, a deeper tile) is open.
- **With a menu open the pieces' `visible` read 0.** The Item Condition tutorial box
  (opened when the condition went to 10 %) hid the HUD, and every piece's `visible` read 0
  behind it: T4a's "hidden root, pieces 0" [hypothesis] is observed.
- **Subtitles move up by the prompt's height + 8.** With a two-line prompt name (80 high)
  `Subtitles` y went to 572 (88 up); with a one-line name (42 high, §14.1) to 610 (50 up)
  [hypothesis: the rule from these two].
- **Health low**: at 20 of 200 a corner message "Your health is low. You may want to use
  a Stimpak or find some water." came up (`Messages/*:1`), the Vault Boy's picture
  changing with it.
- Not reached: an enemy under the crosshair. The settler was hostile (IsInCombat 1) but
  never under the crosshair (the observe step's drive script, turning the player with
  GetHeadingAngle, did not turn it); aiming gave only §14.1's ReticleCenter 0 and
  EnemyHealth 8. A console SetPos inside the shack did not move the player either.
- [design] NVForever lights `floor(share x 37)` ticks (`hud.rs` `lit`) and fills the
  condition bar to `round(59 x condition)` from the 60-wide ground's x, matching the
  above [observed]. The enemy bar still shares `lit` [design: maintainer's choice,
  owner playtest list 5 item 32; §8 records the captured contradiction and draft issue].

### 14.4 The compass and the crosshair by name (2026-10-29)

[observed: the private `hud_named` and `hud_marks` jobs (runs 1793265044, 1793265121),
xNVSE + JIP 57.3, headings 0, 45, 90, 180 and 270 set with SetAngle Z outdoors, a walk
with the pistol drawn and three quick shots. Names from `hudtemplates.xml` and
`compassicon.xml` [documented: the files].]

- **Named paths reach where `*:N` alone does not.** `HitPoints/compass_window` reads by
  name (x 20, y 65, 345 x 64, zoom 100, depth 1, the same as `HitPoints/*:0`), and through
  it `compass_window/*:k/*:m` reaches the marks; `HitPoints/*:0/*:0` read -999 (§14.3).
- **The marks.** Under `compass_window/*:0` (y 10) ten actor ticks
  (`glow_hud_tick_mark.dds`, y -10); under `*:1` and `*:2` (y 30) the player's marker
  (`glow_hud_compass_pc_marker.dds`), the objective marker and the places. Each mark's
  `x` equals its `_Heading` exactly, as `compassicon.xml` copies it; `_Distance` read 1
  for the ticks. A tick was visible at x 30.1 (heading 70), 112.4 (45), 234.6 (0); at 90
  another slot showed at 230.3. Which actor each slot shows and the heading-to-x mapping
  are open: one actor's x per degree of turn would be 3.3 then 2.7, so either the slots
  change actors or the mapping is not linear [hypothesis]. A finer sweep with an actor's
  bearing (GetHeadingAngle) settles it.
- **The strip does not scroll through any trait.** At every heading the strip's `x`,
  `y`, `width`, `height`, `alpha`, `visible`, `depth` and `zoom` stayed the same;
  `cropx`, `cropy`, `tile` and `repeathorizontal` (image traits the vanilla files use;
  Cropx: "crops the image in the x direction … applied after the zoom trait"
  [documented: GECK wiki Cropx]) read -999 on it; `filewidth`/`fileheight` 1024 (the
  texture's size). No documented xNVSE or JIP function lists a tile's traits or its
  children's names (DumpTileInfo is SUP's) [documented: the function catalogue, the wiki's
  UI function pages, searched 2026-10-29]. [hypothesis] The code moves the picture's
  texture area itself; ITR's SetUITexOffset page describes compass scrolling that way
  ("scrolling tiled UI textures, such as a compass strip, without changing the tile's
  screen position or crop window") [documented: GECK wiki SetUITexOffset, a separate
  plugin, not evidence about vanilla].
- **Measuring the strip instead**: screenshots of a fine heading sweep outdoors (indoors
  the compass follows the cell's NorthMarker [documented: GECK wiki GetAngle]), the
  heading read back with GetAngle Z, the letters' place against the window's edges and
  the bracket's triangle; that gives the scale, the centre and the wrap the current
  `heading / 360 x 1024` only assumes.
- **The crosshair copy** is `ReticleCenter/reticle_center` (x 2, y 9, 64 x 64, depth 1,
  `glow_crosshair.dds`; `user0`/`user1` read 0 as numbers, the file's two pictures as
  strings). Nothing read changed while walking or after three quick shots, and on the
  screenshots the two arrows stood as far apart standing, walking and firing: **the
  original's crosshair does not spread** with the 9mm pistol [observed]. NVForever's
  spreading crosshair is the owner's design, on by default, with the original's look
  when "Dynamic crosshair" is off (reticle.md §6).


### 14.5 The compass's scale and centre (2026-10-29)

[observed: the private `hud_sweep` job (run 1793266387), outdoors at the save's Goodsprings
spot, the settler `GoodspringsSettler01Ref` moved 300 units due north and held with
SetRestrained; the player turned with SetAngle Z through 0-355 in 5 degree steps, each
with a screenshot (1920 x 1080 scaled to 1280 x 720), GetHeadingAngle to the settler and
every mark's `x` and `visible` under `compass_window/*:k/*:m`.]

- **The marks**: the settler's tick stood at `x = 139.0 + 2.844 x bearing` (compass_window
  units, bearing -180..180 from where the player faces), 14.2 units each 5 degrees at every
  heading; 2.844 x 360 = 1024. It showed for bearings from about -49 to +48 (x from about 0
  to 276; outside, `visible` 0). A place mark stayed inside the same band (tile x 3.1 to
  272.7); the band fits a mark showing while it is within 139 units (148 - 9) of the
  heading point.
- **The strip** moved 85 pixels each 40 degrees on the screenshots (13 of 16 spans; the
  others spoilt by the scene behind it): 85 / 0.75 / 40 = 2.83 units a degree, 1024 to the
  turn as the marks. `heading / 360 x 1024` is right for both.
- **The centre**: the tick picture's bar is at texel 9 of 32 (`compass.rs` `TICK_MIDDLE`),
  so a bearing-0 tick's bar stands at 139 + 9 = **148** of the 345-wide window (x 168 of
  `HitPoints`). On the heading-0 screenshot N stands under the bracket's downward triangle
  (its tip about x 167 of `HitPoints`, N's middle about 172), which settles §4's open
  question: the heading is at the triangle, not the window's middle (172.5 of the window,
  192.5 of `HitPoints`).
- [design] NVForever uses `hud.rs`'s `COMPASS_HEADING`, x 148 of the window, for the
  marks (`compass.rs`), keeping 1024 units per turn; a mark shows while its offset from
  the heading point is within 139 units either way [observed: actor ticks and one place
  mark in the sweep]; a quest mark off the band is held at its end (§11, T12e). The
  strip's N stands at 154 (`STRIP_HEADING`, T12e, §14.10): until T12e at 148 too.

### 14.6 Headless copies and provisional rules (T5)

- [design] `crates/fnv-game/src/ui_hud/copies.rs` creates the copies after the HUD menu
  loads, including save-load rebuilds, through one helper awaiting the parallel `Code`
  tile origin. Missing templates/parents are skipped. Frames write before scripts
  (ui-tile-tree.md §4.1); T12b adds enemy children (§14.8). The file text tiles' widths
  (`sneak_nif`, `button_text`, `radiation_text_value`) are measured with the game's
  fonts and written before scripts (T5 part C, menu-xml.md §3.1.2).
- [documented: `menus/prefabs/hudtemplates.xml`] Template identifiers use
  `template_justify_left_text`, `template_justify_right_text`,
  `template_justify_center_text`, `template_meter`, `template_meter_background`,
  `template_left_bracket`, `template_right_bracket`, `template_compass_window`,
  `template_compass_npc_icon_group`, `compass_npc_icon`, `template_compass_icon_group`,
  `template_compass_icon_player`, `template_compass_icon_quest`,
  `template_compass_icon_marker`, `template_reticle_center`. Their tile names are the
  identifiers without `template_`, except `MeterBackground` and the three icon variants
  all named `compass_icon`. The reticle's `user0`/`user1` are picture paths; retaining
  these strings also gives the numeric 0 observed in §14.4.
- [hypothesis] Except for `compass_window` and `reticle_center` (§14.4), the captures
  record indices but do not confirm copy names. Retain the template names. Under
  `HitPoints`, use compass, left text, meter, left bracket in indices 0–3 (§14).
  Under `ActionPoints`, use background, condition meter, three right texts (CND,
  ammo, AP), AP meter, right bracket in indices 0–6 (§14–§14.3). The condition
  copy uses `template_meter` with the captured VATS picture (§14.2).
- [observed: private jobs hud_prompt, hud_prompt2 p0-p8] Info's actual copy set,
  places and showing rules supersede T5's provisional six-left/two-centre set: §14.8.
  [hypothesis] HP's left-text copy uses visible 1/alpha 255, as in T5.
- [hypothesis] Compass group 0 holds actor ticks, group 1 player/quest icons, group 2
  places. §14.4 settles the first group and the other two groups' y, but not the split
  between the latter. Start with ten hidden actor slots and one hidden player icon;
  reuse slots and hide surplus ones, growing pools as needed [design]. `_Heading`
  uses the drawing's existing offset plus 139 (§14.5 [observed]); actor `_Distance`
  stays 1 (§14.4 [observed]). Other distances default to 1 [hypothesis: the drawing's
  `CompassView` stores kind/offset only; the quest template defaults to 1
  (documented: `hudtemplates.xml`)]. The NVForever-owned `_NVFHudTemplate` trait distinguishes
  pools even when a mod omits a template [design].
- [observed: private job hud_radiation2 r_in_000-r_in_990; hypothesis: the rule between
  75 paired samples over 2.25..998.4 rads] The pointer uses `15 + 0.3 × rads`, replacing
  the unit slope and 42.9 cap. Totals clamp to 0..1000; holding 315 above 1000 is
  [hypothesis: Fatal not sampled]. Only write x while shown; hidden x persists.
  [design: T12b part B] The hand-built pointer uses the same scale; radiation.md §6
  keeps its previous notch-fit history.
- [observed: private job hud_prompt2 p0-p8] Subtitles start at y 660, then hold the
  raised y with no prompt for at least 19.9 s. [hypothesis] Each later prompt sets
  `660 - (height + 8)`, fitting the two captured heights 42 and 80 (§14.3); changing
  prompt heights in one run is untested. [design] Hold until another prompt or rebuild.
  One explicit name line uses 42, more use 80 [hypothesis]; automatic wrapping and
  further lines await font layout [design].
- [design] `engine/nvf-play/src/tiles.rs` supplies the latest `HudView` shares and
  `CompassView` marks, current weapon/ammo, accumulated radiation actor value and
  existing activation prompt. Drawing systems are unchanged; snapshots gathered
  after scripts become available to the next frame's before-script writes.

### 14.7 The HUD drawn from its tiles: which hand-built piece goes (T12a)

[design] With `[UI] TileMenus=HUDMainMenu` (`engine/nvf-play/src/tiles/vanilla.rs`; the
test aid `NVF_PLAY_TEST_TILE_MENUS`) the tile renderer draws the whole `HUDMainMenu`
tree: the file's tiles, the code-made copies (§14.6) and the mods' (menu-xml.md §3.1.3).
Each hand-built piece is mapped to the HUD file's piece by this note's sections; it is
hidden only where the tree's tiles are filled enough to draw it. Off, nothing changes.

| Hand-built (nvf-play) | HUDMainMenu piece | With the switch on |
|---|---|---|
| `hud.rs` left corner: bracket, "HP", health bar, compass strip and marks (`compass.rs`) | `HitPoints` (`left_bracket`, `justify_left_text`, `meter`, `compass_window` and its icon groups, §14.4-§14.6) | tiles; hand-built hidden |
| `hud.rs` right corner: bracket, AP bar, "AP", "CND", condition ground and fill, jam arrows, ammo counter and its abbreviation | `ActionPoints` (§14.3, §14.6), `CNDArrows`, `AmmoTypeLabel` | tiles; hand-built hidden |
| `hud.rs` sneak word | `SneakMeter/sneak_nif` (§14.1-§14.2) | tiles; hand-built hidden |
| `reticle.rs` crosshair (plain/filled, spread, gun cross) | `ReticleCenter/reticle_center` (§14.4) | tiles; hand-built hidden |
| `radiation.rs` meter | `RadiationMeter` (§14.1-§14.2) | tiles; hand-built hidden |
| `hud.rs` enemy bar (`hud_target.rs`) | `EnemyHealth` | tiles (§14.8); eligibility still provisional |
| `reticle.rs` prompt (key, verb, name, value and weight) | `Info` | tiles (§14.8) |
| `hud_needs.rs` words | `HardcoreMode` | tiles (§14.8), fixed word places |
| `xp_meter.rs` | `XPMeter` | tiles (§14.8), independent visual clock |
| `hud_limbs.rs` | `crippled_limb_indicator` | tiles (§14.8) |
| `hud_breath.rs` | `BreathMeter` | tiles (§14.12): its `nif` tile drawn with the hand-built meter's model reader; the showing rule and the breath stay the hand-built HUD's |
| `corner_messages.rs` | `Messages` | tiles (§14.11): the bracket, text and icon copies; the queue stays hand-built |
| `quest_messages.rs` | `QuestReminder`, discoveries also `Region_Location` | tiles (§14.11): banner and objective copies (a discovery's banner also under `Region_Location`, not drawn there); the queue and sounds stay hand-built |
| `script_hud.rs` subtitles | `Subtitles` | tiles (§14.11): its text copy; the selection stays hand-built |
| `sneak_gauges.rs` (NVForever's own, off by default) | none | hand-built |

- [design] The kept pieces' file and code tiles are not drawn (`KEPT_PIECES`), a mod's
  tiles under them still are.
- [design] What the tile-drawn corners need that T4 does not write is written only while
  the switch is on (`vanilla::write_corners`): the corner labels' copies at the
  hand-built HUD's places (`hud::label_places`; their places were not captured), the
  right piece's moved by its 40 units (`MARGIN_X`, the hand-built piece stands 40 in from
  the view's edge, the tile piece at it); the condition ground's alpha a quarter
  (`hud::GROUND`'s [hypothesis]); "CND", the ground and `CNDArrows` shown by alpha only
  with a weapon's condition.
- [hypothesis] `ActionPoints/right_bracket` at x -40: where the hand-built bracket stands
  on the screen, which §14 found where the original's corner lands. [observed: T12e,
  §14.10] Drawn there the atlas piece's divider, upright and step cap stood 3 pixels (4
  units) left of the original's at 720 lines: x -36 since T12e
  (`vanilla::RIGHT_BRACKET_X`).
- [hypothesis] `AmmoTypeLabel` shows the ammo's abbreviation (by its name and place; its
  string was not read, §14.2). [observed: T12e, hud_compare hud-ammo-label] It stays at
  §14's captured W - 236, 883 (right-justified: its right end just left of the divider),
  and `write_corners` moves the condition row ("CND", the ground and fill, `CNDArrows`)
  left by the abbreviation's width + 20 (`vanilla::condition_shift`, `LABEL_ROOM`)
  [hypothesis: the rule, one abbreviation seen; the row follows the weapon also while the
  HUD is hidden, design]. History [design, T12c]: the label at the hand-built HUD's
  place right of the divider, because at W - 236 it stood over the unmoved condition
  bar.
- [design, T12c] While on, the condition fill (`ActionPoints/meter:0`,
  `vats_bar.dds`) gets `zoom` 100 × 20/11 (`vanilla::CONDITION_FILL_ZOOM`): the picture's
  drawn block is 74 x 11 of its 128 x 16 texels [observed: the texture], so tiled at a
  texel a unit it drew two bands, where the original's fill is solid over its 20 units
  [observed: private shots `hud_corners2` c0, c4, rows 665-679 at 720 lines]. How the
  original fills it is not traced (its `zoom`, `cropx`, `cropy` were not read).
- [observed, T12c] The original's condition fill is 175/255 of the HUD's colour and its
  ground an opaque 100/255 (same shots); the hand-built row drew 255 and a quarter-alpha
  ground, the tile row 255 and alpha 64. Both draw the original's since T12e (the fill's
  and the ground's `brightness` 175 and 100, the ground at alpha 255).
- [design] Differences still to compare: the crosshair does not spread (the original's
  look, §14.4); the warning sneak words are
  the HUD's colour; the corners show whenever the root does (the hand-built HUD also
  hides them under a movement lock, a portrait, a held camera and the weapon wheel).

### 14.8 T12b part A: captured rules for the tile HUD (2026-10-29)

[observed: private jobs below; summaries `C:\fnv-private\hud-capture\t12b-findings.md`
and `t12b-fixes.md`, part A] Game 1.4.0.525, xNVSE 6.4.9, JIP 57.3, save `hudtest`,
1920 × 1080. These are trait reads: every screenshot in both rounds failed with
`StretchBlt failed`. No capture, game XML or dump is shipped.

| Private jobs (run IDs) | What they read / reached |
|---|---|
| `hud_enemy2` (1793279665), `hud_enemy3` (1793281613) | Reticle pictures/colours, compass tick colours, EnemyHealth children, health/base/combat; neutral/hurt settler, full hostile settler, halved gecko in combat. Gecko never aimed at or shot. [observed: enemy2 e0-e6, g0-g2; enemy3 n0-n3, h1, g0-g2] |
| `hud_prompt` (1793279735), `hud_prompt2` (1793281701) | Info copies by name/index, hotrect geometry and subtitle y; pistol, full/empty/locked containers, actor, nothing. Stimpak missed twice. [observed: prompt p0-p8; prompt2 p0-p8] |
| `hud_hardcore` (1793279831), `hud_hardcore2` (1793281465) | Word places, alpha, systemcolor and indices. Second run set needs 250/450/650/850, mixed needs, rads 450, one cripple, Hardcore off. hc8/hc9 paused in MenuMode; Fatal untested. [observed: hardcore hc0-hc11; hardcore2 hc0-hc11] |
| `hud_xp_limbs` (1793279919), `hud_xp_limbs2` (1793281789) | XP alpha/visible, pointer, amount/level strings; leg/head cripples/healing in round 1 only. Second run gain 190, LEVEL UP, menu. [observed: xp_limbs x1-x9, l1-l3; xp_limbs2 x9_level_up_long] |
| `hud_radiation` (1793280006), `hud_radiation2` (1793281551) | Pointer x paired with GetAV RadiationRads inside/outside the shack; second run set 14 totals 0..990. 75 settled pairs over 2.25..998.4 rads, excluding nine first samples still showing the previous total. [observed: radiation r_out_0, r_in_000-r_in_990; radiation2 r_out_0, r_out_300, r_in_000-r_in_990] |

- [observed: private job hud_jip file_0032; hud_hardcore2 hc2-hc5] Numeric systemcolor
  1 = hudmain, 2 = hudalt. Hostile reticle/ticks use 2 and the plain crosshair;
  neutral reticle/ticks use 1, filled on an activatable target [observed: private jobs
  hud_enemy2 e0-e2, g0-g2; hud_enemy3 h1]. [design] Map numeric and named colours
  alike in the renderer and numeric symbol reads.
- [observed: private jobs hud_hardcore hc0; hud_xp_limbs all; hud_prompt2 p0-p8]
  `*:N` and `name:N` count from the **last matching child**, zero-based. LMBs,
  Right_Leg and XPLevelUp are each their file piece's `*:0`; Info's first file child
  (the hotrect) is `*:8`. [design] Reverse code-copy creation to keep the observed
  indices: HitPoints 0..3 compass, HP, meter, bracket; ActionPoints 0..6 condition
  ground, condition fill, CND, ammo, AP, AP meter, right bracket. Compass groups are
  created places, player/quest, NPC, so NPC remains `*:0`; indexed pools reuse slots
  from the last child. Unindexed lookup and draw traversal keep file order [design;
  hypothesis: unindexed lookup]. No contrary vanilla menu/script dependency was
  established by these captures; other menus and deprecated xNVSE GetUIFloat parity
  remain untested [hypothesis].
- [observed: private job hud_radiation2 r_in_000-r_in_990; hypothesis: rule between
  the 75 paired samples over 2.25..998.4 rads] Pointer x = `15 + 0.3 × rads`, within
  0.7 x units of the reads, without saturation. Hidden x stayed 15 even at 300 rads
  [observed: same job r_out_0, r_out_300]. [design] Retain x while hidden. Clamp rads
  to 0..1000, holding 315 above 1000 [hypothesis: Fatal not captured].
- [observed: private job hud_hardcore2 hc0-hc11; hud_xp_limbs l1-l3] The words stay
  at x 0/55/109/162/218, y 0, zoom 75 within HardcoreMode (190,803), in both modes.
  H20/FOD/SLP only in Hardcore, RAD/LMB in both. Stage looks: absent alpha 0;
  Minor 124/main; Advanced 255/main; Critical 255/alt; Deadly flashes 128..255/alt.
  One cripple gives LMB 124/main; two give 255 (colour unread). [hypothesis] Fatal
  follows Deadly, RAD follows need colours above 600, LMB two retains main colour/255.
  [observed: T12e, hud_compare needs_limbs and needs_off] Three give LMB 255 in the alt
  red (steady in both shots); until T12e the tile HUD kept main colour there (amber). [design] Use a synchronized 0.6 s cosine for word
  alpha until the period is measured. These tile rules supersede §13's provisional
  placement/opacity/count rules only for the tile HUD.
- [observed: private jobs hud_prompt, hud_prompt2 p0-p8] Info indices 0..8 are
  separator, VAL, value, WG, weight, [EMPTY], lock, name, file hotrect. Two left
  texts: VAL (165,60), WG (10,60); two right texts: value (290,60), weight (135,60);
  three centred texts: [EMPTY]/lock (160,65), name (160,2). Separator y 32,
  500 × 136 (`template_info_seperator`); its x was unset. [design] Use x 0.
  Item only: separator/figures visible 1; otherwise hidden, figures `--`.
  During any prompt name/lock visible 1, [EMPTY] visible 1 except when locked;
  empty alpha 255 only for empty containers (before or after opening), lock alpha
  255 only for locked containers, otherwise alpha 0. No prompt hides all children
  and retains strings/alpha. Verb belongs to hotrect/button_text, never a copy;
  hotrect visible 2 prompted, 0 absent. Locked box verb `Open`, lock string
  `[Locked - Average]`; hidden lock normally `Locked`. Shortcut stays visible after
  the first prompt. [hypothesis] The name copy retains its template name
  (`justify_center_text:2`, not read by name); other lock levels use existing labels.
- [observed: private job hud_prompt2 p0-p8] Subtitles start at 660 and retain the
  raised y after the prompt goes, for 19.9 s; §14.6 describes the retained-offset
  implementation and the untested later-height rule.
- [observed: private jobs hud_enemy2 e5; hud_enemy3 n2, h1, g2] Enemy copies in
  creation order: bracket (-7,3), 500 × 64, brightness 175/depth 1; meter y 3,
  height 20, brightness 175/depth 1; centred name (170,35). Piece colour follows
  shown actor's hostility (1 neutral, 2 hostile); children inherit it. [observed:
  those states; hypothesis: rule between shares 0.156/0.178/0.5/1.0] Ticks =
  `2 × floor(share × 15)`, width = ticks × 8, x = 171 - width/2. Widths are
  32/32/112/240; floor(share × 31) rounded down to even fits equally well: near-full
  shares remain open. [design] Keep existing target selection, require health below
  base, show/hide all children together; keep piece visible 1 (8 aiming). Hurt
  actors off target and the player's own shot still need captures.
- [observed: private jobs hud_xp_limbs x1-x9; hud_xp_limbs2 x9_level_up_long;
  hypothesis: linear fades/glide and approximate breakpoints over 0..9.35 s]
  XP ordinary children fade in 2 s, hold 2 s, fade out 2 s, then become invisible.
  Pointer waits for full alpha then glides 2 s. [observed: xp_limbs x1-x2b;
  xp_limbs2 x9_level_up_long; hypothesis: formula between level-1 XP 10..200 points]
  Pointer x = `55 + 215 × (XP - last) / (next - last)`; 10/20/50/200 read
  65.75/76.5/108.8/270. First amount stays while shown [hypothesis: one +25 then
  +5 example]; further gains update the endpoint. LEVEL UP alone appears around
  7.2 s, rises within 0.34 s, holds about 1.3 s, falls by 9.35 s. [design] Tile-only
  `ui_hud::Animation` clocks pause without the HUD and reset on tree rebuild or XP/
  level reset. They do not schedule sounds/menus: `xp_meter` keeps `UILevelUpText` at the
  filled bar (4.0 s, owner 2026-10-24) and opens the menu `LEVEL_UP_DELAY` (5.35 s) later
  at 9.35 s, matching the observed menu opening after the `LEVEL UP` fade finishes at
  9.18 s [documented: nvf-play/src/xp_meter.rs, LEVEL_UP_DELAY; #3438].
- [observed: private job hud_xp_limbs l1-l3] Limb pictures at zoom 75:

  | Picture | x,y | width,height |
  |---|---|---|
  | Head | 0,40 | 63.75,69 |
  | Face | 13.88,62.5 | 36,48 |
  | Torso | -10.5,104.5 | 76.5,96 |
  | Left_Arm | 57,104.5 | 75,39 |
  | Right_Arm | -76.5,100.75 | 72,40.5 |
  | Left_Leg | 24.75,169 | 54,84 |
  | Right_Leg | -38.25,166.75 | 63,84 |

  [observed: same job l1-l3; hypothesis: linear fades/sawtooth between samples, one
  run only, 0..5 s] Whole pieces fade up 0.85 s, hold 3.3 s, fade out 0.85 s.
  All crippled pictures flash together in `_broken`/systemcolor 2 for about 4.3 s;
  roughly 470 alpha/s (~0.54 s period), then below zero (-15.3/-16.3 read).
  [design] Use a sawtooth then -16; new crippling restarts all broken pieces;
  healing cancels without a new display and retains broken pictures. Face selection
  stays the file's: the health-to-face mapping is unmeasured.
- [design] Five pieces leave KEPT_PIECES: HardcoreMode, Info, EnemyHealth, XPMeter,
  crippled_limb_indicator. Keep BreathMeter (NIF), Messages (text/icon copies absent),
  QuestReminder/Region_Location (texts absent), Subtitles (text absent) (as of T12b;
  the texts leave it in §14.11, BreathMeter in §14.12, and it is empty since). Only the five
  visibility marker declarations move from the hand-built files into tiles/vanilla.rs;
  their drawing/timing logic and optional sneak gauges stay untouched. Prompt/enemy
  copies and XP readout widths are measured with the game fonts. Text zoom scales
  glyphs about the top left [hypothesis: anchor unobserved]; image zoom remains
  menu-xml.md §3.1.3's interpretation. [observed: T12e] A text's zoom 75 does not shrink
  the original's words (§13): the tile drawing no longer scales text by `zoom`.

### 14.9 T12d: the ticks' picture, the bars as one image, smooth compass marks (2026-10-29)

- [observed: `hud_tick_mark.dds` from `Fallout - Textures2.bsa`, decoded] 8 x 32 DXT3,
  white; the block is texels 0-5 of rows 0-21, its alpha 34 at the top rising to 255 at
  row 17 (§3); texels 6-7 and rows 22-31 clear. Both drawings already read the right
  rows (the box's top 20, a texel a unit) and drew the alpha: the cause of the "flat"
  look is not the picture's region.
- [observed: private shot `hud_corners2.007` c0_full, 1280 x 720, 0.75 pixels a unit]
  The original's health and action points ticks stand every 6 pixels (8 units), the
  block 4-5 pixels with a 1-2 pixel gap, their top rows fading into the world. At their
  full rows they read (189, 132, 47) and (184, 129, 47) where the bracket's line reads
  (255, 188, 68): about 0.7 of the line's colour in every channel, the colour of
  brightness 175 (175, 129, 47) rather than of an alpha of 0.7 over the ground under
  them (about (199, 147, 55)). NVForever's ticks read 255 there and their fading rows
  stay bright (the T12c shots at 1280 x 800): "flat".
- [hypothesis] The corner bars' ticks are drawn at brightness 175: `brightness.xml`'s
  "meter and compass in HUD" [documented: `menus\prefabs\brightness.xml`,
  `_brightness_4` 175], the enemy bar's meter's read brightness (§14.8), and T12c's
  condition fill at 175/255 of the colour. Their own `brightness` trait was not read
  (an observer check). [design] The hand-built bars draw `amber(175)`
  (`hud::METER_BRIGHTNESS`); the tile HUD's `vanilla::write_corners` writes `brightness`
  175 to `HitPoints/meter` and `ActionPoints/meter:1`. The hand-built enemy bar keeps the
  full brightness `hud::enemy_colours` gives it.
- [hypothesis] The original blends in the screen's sRGB values and NVForever (Bevy) in
  linear light, which shows a part-transparent amber brighter over a dark ground; at
  brightness 175 the shot's fading rows and NVForever's differ by about 5 levels of 255.
  Not changed.
- [observed: the owner, T12d] The ticks stood unevenly: at 1280 x 800 a unit is 0.833
  pixels and a tick period 6.67, and each tick set on whole pixels (the hand-built
  `tick_lefts`) or each repeat of Bevy's `NodeImageMode::Tiled` (the tile HUD) took 6 or
  7 pixels by turns. [design] The hand-built HUD (the default) keeps the maintainer's
  crisp whole-pixel ticks (§4: the owner's "clean straight lines, not some bigger than
  others" and "a gap at the end of each"), the gaps differing by a pixel at most. The
  tile-drawn HUD, which follows the original, draws a bar as one image: the picture's
  texels repeated into one RGBA image the box's size in texels (`crate::repeat`, decoded
  by `fnv_dds`, kept per picture and size), stretched over the box and sampled linearly,
  so every tick stands `8 x scale` pixels from the next at any size, its edges slightly
  soft (test `tiles::draw::a_tiled_meter_is_one_image_and_its_ticks_stand_evenly`, at
  1280 x 800, 1920 x 1080 and 1280 x 720) [hypothesis: the original's even, soft ticks
  are one repeated texture filtered across the bar]. Every tiled picture of the tile HUD
  goes through it (the corner bars, the enemy bar, the condition fill and its ground);
  the XP meter has no tiled picture (its ticks are drawn in `glow_XP_bar_over.dds`). A
  box over 4M texels keeps Bevy's tiling.
- [observed: the owner, T12d] The compass marks stepped while turning and as actors
  walked, where the original's glide. Causes: the places were looked at ten times a
  second (`compass::gather`'s poll) and kept in whole units (`CompassView`), and the tile
  HUD spawned all its nodes again whenever a place changed. [design] The places are
  worked out every frame, unrounded, through `CompassView`, the tile tree's `_Heading`
  and `x`, and the tiles' boxes (§11); the tile renderer moves the nodes of pieces that
  only moved (`tiles::draw::change`), and the hand-built marks' nodes move in place
  while their kinds stay. Nodes still land on whole pixels (Bevy's layout rounding), as
  the strip's does.

### 14.10 T12e: the HUD's remaining differences from the original (2026-10-29)

[observed: the three-way comparison at 1280 x 720 of 2026-10-29, the original's frames
`C:\fnv-private\capture\shots\hud_compare_*` (1920 x 1080 scaled to 1280 x 720, 0.75
pixels a unit) against NVForever's hand-built and tile-drawn rows, measured on the raw
frames; one run each. Private, not shipped.] What changed, with the maintainer's choices
the hand-built HUD (the default) keeps:

| Difference | Hand-built HUD | Tile HUD |
|---|---|---|
| Ammo abbreviation left of the divider, the condition row 50 left for "HP" (§4) | kept: the owner's place right of the divider (commit 5887ee0e) | fixed: `AmmoTypeLabel` at W - 236, `vanilla::condition_shift` |
| Need words 13 pixels tall, starting right of the original's (§13) | fixed: at the font's size | fixed: text `zoom` not applied |
| The AP bar's last tick against the upright (§4) | fixed: the clear strip after every tick; the health bar at x 20 | fixed: the right bracket at x -36 |
| LMB with three limbs (§13) | kept: the owner's flashing look | fixed: alt red, 255 |
| Quest marker under the compass (§11) | no quest in the scenes; off-band targets now held at the end | the same, through `CompassView` |
| The strip's right end fades (§4) | fixed: one baked window image (`compass_strip`) | fixed: the same image |
| Condition bar 175, ground 100 (§4) | fixed | fixed |
| LEVEL UP 16 units low | fixed: 16 above the amount's line (`xp_meter::LEVEL_UP_BASE`) | fixed (#979): font 8's line 30 under the text's top (29.5 until every font was measured), a line per font (below) |
| Compass pattern, end caps, health underline (§3) | fixed: the atlas pieces | already the pieces |
| Radiation ruler 40 units long, ticks 16.7 pixels | fixed: the bracket at its texels' size, the owner's place and texts kept on the screen (radiation.md §6) | already |
| The strip 4 pixels left (§4) | fixed: `STRIP_HEADING` 154 | fixed: shared `strip_left` |

- **The right bracket's place** [observed: needs_off, ammo-label, limbs_hidden,
  xp_meter]: drawn at the right piece's corner the atlas piece's divider stood at x
  1111-1113, the upright from 1231-1232, the step cap from 998, the original's at
  1114-1116, 1234 and 1001-1002: 3 pixels, 4 units, the same in every shot.
- **The strip's N** [observed: compass-north, heading 0]: the original's centred at x
  159.5 pixels, the pointer's tip at about 153.5, both NVForever rows' N at 155 with the
  strip at 148: 4.5 pixels, 6 units.
- **The fade** [observed: needs_off, xp_meter; the strip's line over the ground]: about
  0.68 of the colour at x 266 pixels, 0.38 at 280, 0.21 at 290, 0.05 at 298: from full at
  about 250 to nothing at about 300.5 (window x 273 to 341). The quest marker at SE
  (x 255-264) is drawn full where the line beside it is at about 0.8.
- **LEVEL UP on the tile HUD (5 pixels high): a line per font (#979).** Until #979
  every font's first line stood 24 under a text tile's top, which drew the font-8 texts
  about 5 pixels high. [observed: hud_textbase capture 2026-10-31, 1280 x 720, 0.75
  pixels a unit; the text tiles' traits read in every state, no `y` or `height` moved;
  private, not shipped] `radiation_text` (font 7, height 42) and `radiation_text_value`
  (font 8, height 64) both stand at y 65; `XPAmount` (font 8, 64) at y 90, `XPLabel`
  (font 7, 42) at 80, `XPLevelUp` (font 8, 64) at 70, `XPLastLevel` and `XPNextLevel`
  (no `font`, 36) at 30. A text tile's `height` is its font's tallest glyph picture
  (fonts.md §3). Where the capitals' ink ends under the tile's `y`, and where it starts:
  "RAD/SEC" 25.0 (1.0), "XP" about 24 (about 1.7), "+9" 31.7 (-5.7), "+54" about 31.7,
  "LEVEL UP" 32.0 (-6.7), the level digits "1" and "2" (font 1) about 21 (about 1.3).
  So "+9" and "RAD/SEC" do not stand on one line: the 29.3 this note gave
  `radiation_text` before, like its value's, was a mis-measurement (menu-xml.md §3.1.2's
  23.7 agrees with the other font-7 readings), and a line per font fits every reading.
  [observed: the font textures, fonts.md §3] NVForever sets a glyph's picture with its
  top `ascent` over the line (fonts.md §2); the glows are as deep under the letters as
  over them, so the middle of a text's ink stands where it does whatever edge the eye
  takes: 12 units over the line for font 7's capitals, 18 for "LEVEL UP", 17.5 for "9",
  10 for font 1's digits. Measured so, the capture's lines stand 24.9 (font 7), 30.5
  (font 8) and 21.0 (font 1) under the tops. `tiles::draw::text_base` keeps font 7's 24
  (this capture's 24.9 is within its pixel of it; the earlier readings 23.7 and the
  prompt's 24.5-25.5, menu-xml.md §3.1.2) and stands fonts 8 and 1 as far from it as
  this capture shows: font 8 29.5, font 1 20. [observed: hud_menus capture 2026-10-31,
  fonts.md §3.1] Every font measured since, at three window sizes, one font-7 reading
  for all: `text_base` is font 1 20, fonts 2-5 16, font 6 22, font 7 24, font 8 30 (5.6
  under font 7 here, 6.0 there; 30, fonts.md §3.1), and the lines stand one pitch of
  their font apart (`draw::line_pitch`: 31, 35, 32, 36, 25, 38, 38, 52 for fonts 1-8),
  not the font file's size. No single font measure gives the
  three (the size less the deepest glyph's reach under the line: 21, 24, 37, fonts.md
  §3) [hypothesis: the rule behind them is not known]. The tile texts take it
  (`draw::text_lines`), the quest notices' tops (§14.11: the name's top 30 over the
  hand-built line), the need words (`hud_needs::WORD_BASE`, font 7) and the corner
  labels (font 7). Tests: `tiles::draw::{fonts_7_and_8_at_one_y_stand_on_their_own_lines,
  the_default_font_stands_higher_and_font_9_takes_font_7s_line,
  every_font_has_its_own_line_and_pitch, a_texts_height_is_its_tallest_glyph_and_a_pitch_a_line,
  a_text_tiles_font_trait_sets_the_line_it_is_drawn_on}`.
- Tests: `hud::{the_last_action_point_tick_stands_clear_of_the_upright,
  the_compass_keeps_the_heading_at_the_triangle, the_pieces_stand_inside_their_brackets}`,
  `compass_strip::{the_window_fades_out_at_its_right_end,
  the_offset_is_the_strip_left_in_texels, the_window_wraps_at_the_strips_end,
  the_window_is_baked_again_only_when_the_strip_moves_a_texel}`, `hud_needs::the_words_are_drawn_unscaled_and_end_at_their_x`,
  `compass::a_quest_target_off_the_compass_is_held_at_its_end`,
  `xp_meter::level_up_stands_above_the_amounts_line`,
  `radiation::the_bracket_stands_unstretched_where_it_stood`,
  `tiles::vanilla::the_corners_take_the_hand_built_places_and_the_condition_row_its_rule`,
  `tiles::draw::tiles_numeric_colours_match_symbols_and_flow_to_enemy_children` (text
  zoom), `crates/fnv-game/tests/ui_hud.rs`
  `need_words_keep_places_and_use_stage_alpha_and_colour_in_both_modes` (LMB). Smoke
  `tiles-hud-ammo-label` (new), `hud-compass-north` (the strip's log line).

### 14.11 The messages, quest notices and subtitles from tiles (T12 leftovers, 2026-10-29)

[observed: private run hud_traits run1, `*:N` children of each piece, sample 0 (x, y,
width, height, alpha, visible, depth); hud_jip for the strings, §14.2]

- **`Messages`** has three code-made children from the start, before any message:
  `*:0` 470 x 192 at y -20, alpha 0, depth 1 (the template `message_bracket`'s size and
  y); `*:1` a text at x 130, y 5, 281 x 80, alpha 0, depth 2 (the corner message's text,
  §14.2); `*:2` 160 x 160 at x 0, y -25, alpha 0, depth 2 (`message_icon`'s). All visible
  1. Which template the text comes from is not read [hypothesis: the left-justified
  text's, by its x where the hand-built HUD's text starts, `corner_messages::TEXT_X`].
- **`QuestReminder`**: its `*:0` and `*:1` were the file's `QuestAdded` and
  `QuestStages` (no code copy under it with no notice showing). **`Region_Location`**
  had no child. **`Subtitles`**: `*:0` a text at x 400 (the middle of the 800-wide
  piece), 13 x 42, alpha 255, visible 0, string " ".
- [design] With `[UI] TileMenus=HUDMainMenu`, `engine/nvf-play/src/tiles/notices.rs`
  writes these copies before scripts, only while the switch is on (off, nothing is made:
  the tree stays T4's). What shows, when and how strongly stays the hand-built HUD's:
  `CornerMessages` (one at a time, the record's Display Time, 0.25 s fades),
  `QuestMessages` (its order, times, fades, AutoDisplayObjectives hold and sounds), the
  subtitle selection of `script_hud.rs`, and the hand-built showing rules (hidden and
  held behind a message box, in dialogue, while an idle holds the camera). The hand-built
  areas are hidden (`vanilla::{MessagesArea, QuestArea, SubtitlesArea}`), their queues
  still run; the subtitles' area only while the tile renderer draws something (below,
  "Differences"). `KEPT_PIECES` is empty: `BreathMeter`, the last kept piece, is drawn
  from its tile since §14.12; `Region_Location` gets a discovery's copies (below).
- **Messages** [design, places observed]: the icon, text and bracket are made in that
  order, so `*:0`-`*:2` are as read; the text at its read x 130, y 5, depth 2. Alpha:
  icon and text 255 × the queue's strength, the bracket 0.65 of that (the hand-built
  HUD's `BRACKET_STRENGTH`, corner-messages.md §4); with no message all three alpha 0 and
  the last text held [observed: alpha 0 with a text]. The icon's `filename` is the
  record's Menu Icon, else the default Vault Boy's [documented: GECK wiki, Message].
  The text wraps at 330 (`wrapwidth`) [hypothesis: the hand-built HUD's break,
  corner-messages.md §4; the copy's own `wrapwidth` was not read].
- **Quest notices** [design]: made at the first notice (none were there before one),
  directly under `QuestReminder` [hypothesis: whether the original puts them under
  `QuestStages` (y 40) or `QuestAdded` (y 80) is not read; the first objective's top,
  39, is next to `QuestStages`' 40]. Places are the hand-built HUD's (quest-messages.md
  §4, measured on the owner's screenshots) less the piece's place (45, 200) and a text's
  line in its font (`tiles::draw::text_base`, §14.10: 24 in font 7, 30 in font 8):
  the label at (0, 23), the name in font 8 at (-1.5, 81), the first
  objective at (0, 39), each next one under the lines before it (the font's size + 2 a
  line: 38 in font 7, the same as font 7's observed line pitch, fonts.md §3.1, which
  sets the lines inside one wrapped objective), wrapped at 570. Each objective has a box copy of its kind (`template_checkbox`,
  `template_filled_checkbox`) at x -16, 3 under its text's top, 26 x 26 (the screenshots'
  square; the file's templates say x -20 and 16 x 16 for the open one) [observed:
  quest-messages.md §4-§5]. Surplus copies are hidden, not removed. A discovery ("You
  have discovered" over the name) is a banner here, where the owner's screenshot shows it
  [observed: quest-messages.md, "Discovered"].
- **Discoveries under `Region_Location`** [hypothesis: hud.md §14 maps discoveries to
  Region_Location; the capture saw no copies there, so their layout is the quest
  banner's]: at the first discovery a label and a name copy (`template_justify_left_text`,
  no bracket: the hand-built HUD draws a discovery as the label and the name only) are
  made under `Region_Location` (45, 728), at the quest banner's places moved to its top
  left, so they stand where the hand-built HUD draws a discovery: the label at (0, -505),
  the name in font 8 at (-1.5, -447). Their strings and alpha follow the queue as
  `QuestReminder`'s banner (alpha 0 with the last discovery held while another notice or
  none shows); their `visible` stays 0 [design]: the same notice is drawn once, from
  `QuestReminder`'s copies, because the hand-built drawing's place (the label's top 223,
  the name's 281) lies inside `QuestReminder`'s rect (45, 200, 400 x 200) and far outside
  `Region_Location`'s (45, 728, 100 x 100). What the original draws under
  `Region_Location` on entering a named place (a region or location name) is still not
  known.
- **Differences from the hand-built drawing** [design]: the quest's name fades as one
  tile (the hand-built letters come one after another over 0.6 s); a wrapped line inside
  one tile stands the font's size under the one before (the hand-built 2 more); the
  subtitles stand under `Subtitles`' y (660, or raised by a prompt, §14.6), the
  hand-built ones on a fixed base 633; the tile renderer draws nothing in a menu (the
  root's `visible` 0, written from `Shown::root`), and while it draws nothing the
  hand-built subtitles are not hidden (`vanilla::{hide, root_drawn}`, the root live,
  visible and with an alpha above 0 as the layout passes them down), so a line said
  while a message box is up shows, drawn by hand.
- **Subtitles** [design, observed place]: one centre-justified text at x 400, made at the
  first write with " ", hidden; while lines are drawn its string is the lines (one text a
  line), `visible` 1, wrapped at 800 [hypothesis: the hand-built HUD's width, its own
  `wrapwidth` not read]; after them hidden with the last text [hypothesis].
- **Widths** [design: ui-tile-tree.md §4.1]: each text copy's `width` is written from
  its string just after the copies, before scripts, in its font at its `wrapwidth`, only
  with the game's own fonts (`notices::write_widths`) [hypothesis: a wrapped text is as
  wide as its widest line; `Messages/*:1` was read 281 wide under the 330 wrap, and no
  copy holding a wrapped string was read]. The hand-built subtitles' area is given back
  (`Inherited`) every frame the tiles do not draw them, the HUD menu gone included
  (`vanilla::subtitles_visibility`).
- **Needs a capture**: the copies' traits while a message, a quest notice, a discovery
  and a subtitle show (alpha over time, `wrapwidth`, which template, the parents under
  `QuestReminder`, whether `Region_Location` gets children on entering a named place),
  read with GetUIFloat/GetUIString as §14.2; the original's fades and times
  (quest-messages.md §6, corner-messages.md §7).
- Tests: `tiles::notices::{the_message_copies_are_made_in_the_observed_order_and_fade_with_the_queue,
  the_renderer_draws_the_message_where_the_hand_built_hud_does,
  a_quest_banner_and_its_objectives_take_the_hand_built_places,
  subtitles_are_one_centred_text_there_from_the_start,
  a_rebuilt_tree_gets_its_copies_again_and_a_tree_without_the_pieces_none,
  a_discovery_is_written_under_region_location_too_and_drawn_once,
  the_log_names_what_each_area_shows,
  every_text_copy_gets_its_width_from_its_string_font_and_wrap}`,
  `tiles::vanilla::the_hand_built_subtitles_stay_while_the_tiles_draw_nothing`. Smoke
  `tiles-hud-notices`, `tiles-hud-discovered`, `tiles-hud-subtitles`; every `tiles-*`
  scenario expects the start line to end `hand-built HUD hidden` (no piece kept, since
  §14.12).

### 14.12 The breath meter from its `nif` tile (T12 leftovers, 2026-10-29)

- **The tile** [documented: `hud_main_menu.xml`, read with `fnvtool ui tree --at
  HUDMainMenu/BreathMeter`]: `<nif name="BreathMeter">`, `filename`
  `Interface\HUD\AirTimer01.NIF`, `animation` `Forward`, `visible` `&false;`, `alpha` 0,
  64 x 64, `systemcolor` `&hudmain;`, no children and no place. [observed: §14] Its place
  W/2, 110 and `visible` 8192 while playing (T4 writes both). No trait of the file says
  how far into `Forward` the model stands or when it shows: the code drives both
  [hypothesis]; neither was read in the original under water.
- **What the hand-built meter does** (`hud_breath.rs`, drowning.md §3): it reads the
  model itself (no 3D rendering): each textured square of the model's x–z plane is a
  picture turned to its angle, the model's bounds fitted in its own 64 box centred at
  W/2 and 104 down; `Forward`'s vis keys switch `Tick01`…`Tick16` off at the share of the
  breath used [observed: the keys; hypothesis: the share].
- [design] **The tile renderer draws `nif` tiles** with the whole menu drawn
  (`engine/nvf-play/src/tiles/model.rs`): the tile's `filename` under `meshes\`
  (`model::model_path`) read once by the same reader (`MeterModel::read_sequence`, with
  the tile's `animation`), its pieces spawned by the hand-built meter's own code
  (`hud_breath::spawn_pieces`), in the tile's colour and alpha. It is not the design
  note's "drawn to a texture" (hud-xml.md §5): a model with no flat pieces draws nothing
  and says so once. The mods' `nif` tiles with the switch off are still not drawn (T5).
- [hypothesis] **Where**: the model's origin at the tile's `x`, `y`, one model unit an
  interface unit, its z up the screen: the meter centred across at W/2 with its centre
  about 110 down (the 16 ticks reach about 30 units round the origin, filling the 64 x
  64 the file gives). The read x W/2, a left edge for every other piece, would put a box
  drawn from the tile's top left right of the middle, where the original's meter is not
  [the owner: "center high", drowning.md §3]. The hand-built meter (the default) keeps
  the owner's place, centred 104 down, and its bounds-fitted size (about 7% larger).
- [design] **When and how far** (`vanilla::write_breath`, only while the switch is on):
  `BreathMeter`'s `alpha` 255 while the hand-built meter would show (`hud_breath::shown`:
  the head under water, or the breath not yet whole, and the corners shown), 0 otherwise
  [hypothesis: as the sneak word's and the radiation meter's children, §14.1]; and
  `_NVFSequenceFraction`, NVForever's own trait, the share of the breath used (0 to 1),
  which `model::animate` turns into `Forward`'s time each frame and shows the pieces by,
  without spawning them again. The hand-built meter's root is hidden
  (`vanilla::BreathRoot`), its model still read. `KEPT_PIECES` is empty; the start line
  says `hand-built HUD hidden`.
- **Needs a capture**: `BreathMeter`'s `alpha`, `visible`, `x`, `y` and any new trait
  read under water (GetUIFloat every half second through a dive, as §14.1), and a
  screenshot of the meter at 1280 x 720 with about half the breath used: its centre and
  size against the tile's place, which tick goes first and whether the ticks go one by
  one or fade.
- Tests: `tiles::model::{a_nif_tile_names_its_model_under_meshes_and_its_sequence,
  the_breath_meter_draws_from_its_tile_only_while_written_shown_with_the_switch_on,
  the_model_stands_on_the_tiles_place_a_unit_a_unit,
  the_ticks_go_out_as_the_fraction_written_grows}`,
  `tiles::vanilla::{the_breath_meter_shows_by_alpha_and_stands_in_its_sequence_by_the_breath_used,
  the_start_line_names_the_kept_pieces_only_when_there_are_any}`, the `hud_breath` tests.
  Smoke `tiles-hud-breath` (new, shot `tiles_hud_breath.png`); the six other `tiles-*`
  scenarios that quote the start line now expect `hand-built HUD hidden` with nothing
  after it.

## 15. The grenade indicator (2026-11-13, #1891)

- **The files** [documented: `menus\main\hud_main_menu.xml`, `menus\prefabs\HUDTemplates.xml`]:
  `Explosive_positioning_rect` is an empty rect with `locus` true; the template
  `template_explosive_indicator` holds one image, `Interface\HUD\explosive_indicator.dds`,
  `&hudalt;`, 151 x 187 at x -75, y -93 from the rect, zoom 120, alpha 0. Nothing in the
  files places the rect or raises the alpha: the game's code does. A commented-out text
  under it would have said `sExplosive`.
- **The picture** [observed: the texture, 128 x 128]: the top half of a ring with a small
  point at its top, white on clear.
- **NVForever** (`engine/nvf-play/src/grenade_indicator.rs`): the picture at zoom 120 from
  the box's top left, cut to the box, as the tile renderer reads `zoom`; one mark per live
  grenade whose explosion radius + 64 reaches the player's feet (the rule actors use to
  get away, combat-ai.md §17.7), the nearest 8; on the grenade upright while it is in
  view, else on the view's edge 90 pixels in, turned so the point faces it
  [hypothesis: all of the placing; observer check 190]. Not the player's own grenade in
  its first 0.5 s. The Tweaks tab's three grenade-indicator switches (tweaks.md).


### Hover text consumers of the ammo copy's alpha

[documented] [GetUIFloatAlt](https://geckwiki.com/index.php?title=GetUIFloatAlt)
returns -999 for a missing trait. It reads the named local trait; this contract does
not substitute an inherited opacity or a new generic default.

[design] The engine-created `ActionPoints/justify_right_text:1` ammo copy now
materializes its local alpha only when the loaded template omits that trait. The
value is the existing renderer's omitted-alpha fallback, 255. Explicit template
values or expressions, including zero, remain authoritative. This happens only at
copy creation, including a menu rebuild, and does not overwrite subsequent script
writes. Parent alpha and visibility continue to control actual rendered pieces.
No foreign tile or missing UI path acquires a fabricated trait.

[hypothesis] A hover controller which copies that ammo alpha into its own title and
row alpha can otherwise forward the missing sentinel into an explicit negative
alpha: actual layout clamps it to zero while an independent backdrop remains
visible. This isolates one consumer boundary; earlier blank-bar screenshots do not
identify the current build or prove every missing label has this cause. Ordinary
inventory, crosshair events and the original installed controller's full execution
remain separate acceptance work. Native default/property publication parity has
not been observed.

[design] Three authored `nvf-play` controls run real HUD copy creation, compiled
GetUIFloatAlt/SetUIFloatAlt forwarding and actual tile layout with nonempty title
and row strings. They cover omitted alpha, explicit zero/expression and later
foreign hiding, parent opacity/visibility and the missing-path sentinel. They are
unexecuted at authoring; actual results are qualified by their exact executed head.
