# Spec note: the Pip-Boy

Tags as in `nif.md`: `[documented]` cites a page or file, `[observed]` says what was
looked at, `[hypothesis]` is a reading that nothing has confirmed yet. `[design]` marks
what is NVForever's own. Implementation: `engine/nvf-play/src/pipboy.rs` (state, keys,
pointer, the image the menus are drawn into), `pipboy/frame.rs` (what every page shares),
`pipboy/items.rs`, `stats/`, `data.rs` and `data/` (the pages), `pipboy/test_aid.rs`;
`engine/nvforever/src/pipboy_arm.rs` (the model's screen, glows and buttons),
`player_body.rs` (the clip and the arms' field of view), `imagespace.rs` (the blur behind
it), `engine/nvf-assets/src/texture.rs` (`pc_texture`).

## 1. Sources

- **Fifteen screenshots from the owner**, 2026-10-11, 1280 × 720 (kept privately in
  `C:\fnv-private\references\pipboy\original\`): every tab of STATS (Status with CND,
  RAD and EFF, S.P.E.C.I.A.L., Skills), ITEMS (Weapons, Apparel, Aid, Misc, Ammo) and DATA
  (Local Map, World Map, Quests, Misc, Radio), in Goodsprings in daylight.
- **The game's interface files** in `Fallout - Misc.bsa`: `menus\main\inventory_menu.xml`,
  `stats_menu.xml`, `map_menu.xml`, `menus\globals.xml` and the pieces they include
  (`card_info.xml`, `tabline.xml`, `tabline_template.xml`, `super_text_box.xml`,
  `text_box.xml`, `box.xml`, `line.xml`, `vertical_fade_line.xml`, `list_box.xml`).
  Described here in plain words; nothing of them is shipped.
- **The models and clips**: `meshes\pipboy3000\pipboyarm.nif` (and the female and NPC
  forms), `meshes\characters\_1stperson\locomotion\male\pipboy.kf` (and
  `female\pipboyfemale.kf`), textures under `textures\pipboy3000\`.
- **`Fallout_default.ini`** in the game folder, and the owner's `Fallout.ini` (the same
  values): `fPipboy1stPersonFOV=47.0`, `fDefault1stPersonFOV=55.0000`; `[Pipboy]`
  `fBlurRadiusPipboy=3.5`, `fBlurIntensityPipboy=0.25`, `fScanlineScalePipboy=50`,
  `bEnableFlickerPipboy=1`; `[InterfaceFX]` `fBrightenPipboy=1.3000`,
  `bEnableScanlinesPipboy=1`, `fVertHoldChance=0.08`, `fShudderChance=0.20` (under
  ";pipboy menu transitions"). NVForever never reads the INIs at run time; the values
  are built in where used.
- The menus' own words (`&-sInventoryItems;` and the like) are game settings that the
  masters do not hold (only 130 string settings are in `FalloutNV.esm`, none of these)
  [observed]; the labels are taken from the screenshots.

## 2. The device

### First-person script animations while the device is open

[reported in NVForever: owner's 060d playtest, 2026-11-17] The device can be invisible with
holstered hands or no weapon while an opening with a drawn weapon works. The
engine log records a first-person script animation above the built-in Pip-Boy
layer during affected openings; a successful menu phase does not establish a
visible model.

[design] While a Pip-Boy menu holds game time, the player's first-person animation
clock advances by the script frame's menu time. Explicit sequence samples,
GetGlobalAnimTime/GetAnimCurrentTime and their setters share that clock. Its
activation, deactivation, resume and automatic completion use the same clock.
World time, NPC animations and the player's third-person clock stay held. A
loading screen and other menus retain their existing clock behavior. The
first-person sequence's blend also advances under the device menu when the
virtual world clock is paused; ordinary weapon and locomotion layers keep the
world clock. The final first-person pose's seam smoothing uses that admitted
menu time too, so smoothing cannot replace the new sample with its last held
pose. The preparation consumer takes actual menu-clock progress once; retained
frame configuration during a skipped runtime tick supplies no new time.
Closing the device returns to the world clock without a jump.

[documented: GECK wiki GetGlobalAnimTime] The argument chooses first-person
animation time; the page does not specify menu pause behavior. The menu clock
mapping above is NVForever behavior, not established native pause parity.
Authored core and CPU pose regressions exercise this mapping; their execution
and an installed-profile replay are recorded separately. No model visibility
override or suppressed readiness warning is part of this change.

[design: #2862, `player_body.rs`, `pipboy_arm.rs`, `anim_blend.rs`] When the
built-in Pip-Boy arm clip is active (`pip.on_arm() && pip.warming.is_none()`,
outside the Esc pause menu) while the virtual world clock is held
(`--menu-pause original`), first-person seam smoothing also advances by the
menu frame time (`PlayClock::frame_secs`) without ticking ordinary world/weapon
clips or skipped script-sequence fades. Otherwise a frame without a new script
tick (including `--no-scripts`, render frames between script ticks, and
lowering transitions) leaves `Animator::last_dt == 0`, causing `Seams::smooth`
to copy the pre-menu arms-down pose over the raised Pip-Boy when the player's
hands were empty or holstered. Hidden warm-up poses evaluate unsmoothed
(`Animator::pose_unsmoothed`) so they neither freeze on `last_dt == 0` nor
overwrite live seam history, and retain `WarmUnculled`/`NoFrustumCulling` across
consecutive warm frames until warming finishes or aborts (preventing a
same-frame deferred-remove race between `restore_warm_pose` and `draw_warm_pose`
that culled the rig behind the camera on alternating warm frames and triggered
the zero-submission readiness cap). `requeue_raise` also refreshes retained
mesh bins once on entering `Phase::Up`, and `show_rigs` keeps the first-person
root's ECS `Visibility` synchronized with `Rig::shown`.

- **The clip** [observed]: `pipboy.kf` is one sequence `Pipboy`, 0.7333 s, clamped, with
  text keys `start` 0, `Blend: 8` 0.033, `Hit` 0.333, `end` 0.733. It drives
  `Camera1st` (a small move, no turn) and both arms from the spine out. A replacement
  clip of another length is played on the Pip-Boy's own (vanilla) clock: the raise maps
  onto its start..`Hit`, the lowering onto `Hit`..its end, and with no `Hit` key the
  vanilla share of it [design, #4492: a 0.067 s replacement, `Hit` at 0.033, held at
  0.333 s sat past its end with the arm put away, observed in FadedLight's Nuclear Sunset
  fork]. Measured on the
  arms rig, the screen's place before the eye through the clip:

  | seconds | screen against the eye |
  |---|---|
  | 0.03 | 115° down (the arm hangs) |
  | 0.23 | 30° down, turning to face the eye |
  | 0.31–0.40 | straight ahead, 11 to 13 units off, facing the eye (0.96–0.99) |
  | 0.73 | 32° down, edge on, going away |

  So the clip both brings the Pip-Boy up (to `Hit`) and puts it away (after `Hit`).
  NVForever holds it at `Hit` while the menus are open and plays on to the end when they
  close [hypothesis: the original does the same; the owner saw it come up and go
  straight back down when it was played to the end]. It fades in over its first 8
  frames and out over its last 8 [hypothesis: frames at 30 a second].
- **Field of view** [documented: the INI]: the arms are drawn as through a view of 47°
  (`fPipboy1stPersonFOV`) while the Pip-Boy is up. NVForever draws them bigger about the
  eye across the view, by the ratio of the two views' half-widths (1.76 at the default
  75°), eased with the clip, instead of with a camera of their own [design]. On the
  owner's ITEMS screenshot and ours the device then stands in the same place to a few
  pixels (buttons, dial, screen corners) [observed]. The normal first-person field of
  view (`fDefault1stPersonFOV=55`) is not applied: the arms and weapons are drawn with
  the world's (open question, §8).
- **Texture** [observed]: the models name `textures\pipboy3000\xbox\english\
  PipBoyArm01.dds`, whose buttons carry the console's LT and RT; `pc\`, `ps3\` and
  `ps3o\` forms lie beside it, and the PC screenshots show no such labels. A texture a
  model names under `\xbox\` is read from `\pc\` when that file exists [hypothesis].
- **Parts** [observed: shape names]: the screen `pipboyscreen:0`; buttons
  `PipBoyButton01`–`03` at the glows `StatsGlow`, `ItemsGlow`, `DataGlow` (STATS, ITEMS,
  DATA from the elbow out); the glass: `ScreenLit:8` (a lit, glossy copy of the screen in
  the same place, `screen.dds` with a normal map) and `glare:0` (`ScreenGlare.dds`, added
  over it, unlit, 0.47 units in front of the screen; NiAlphaProperty flags 4109, blend on,
  SRC_ALPHA / ONE; both shapes vertex colour 0.81; `fnvtool nif decode`). Also `RadNeedle`, `TabKnob`, `ScrollKnob`, `PipboyLightEffect`.
- **The page's button** glows alone while the Pip-Boy is up [observed: the screenshots];
  down, all three glow (owner, 2026-10-10). The pointer on a button lights nothing: a
  player comparing the two reports that the original's buttons do not light under it
  (#1516; not observed again here).
- **The needle and knobs** (nvforever `pipboy_controls.rs`, all the numbers there). The
  needle, the wheel and the knob turn through their own origins, their nodes' in the model
  [observed: `fnvtool nif decode`; the model has no controllers on them and `pipboy.kf` does
  not name them]:
  - `RadNeedle` (two shapes), about its local Z, out of the dial: total rads, not the rate,
    linear, 18° per 100, the bind pose at 500: 0 straight down, 500 to the right, 1000 straight
    up [observed: private job pipboy_special_device_motion: shots at 0 to 990 within a few
    degrees; 58 rads with a rate of 9 reads 58]. It moves slowly, the same speed for a long
    way as a short one: about 0.6° a game loop, a drop of 690 rads still on its way two
    seconds on [observed]; the capture has neither a fine clock nor the loops' rate, so 30° a
    second is [hypothesis], within about 20 to 40. No start-up delay can be told from the
    shots' own stalls. Held at the dial's ends above 1000 [hypothesis]. While the Pip-Boy is
    up it quivers a little about where it rests, a small shiver [observed: playtest 2026-11-05,
    against the original]; two sines of unrelated rates, about 2° in all at 3.7 and 6 a second,
    are [hypothesis], judged by eye. The travel and the log are without the quiver.
  - `TabKnob`, about the Z of its node `TabKnob`, through the origin, the whole knob leaned.
    In that node's frame the knob is straight: its base flat (within 0.02), its two fins 180°
    apart and the dial's disc centred on the origin [observed: `fnvtool nif decode` of both
    arms, the plate's faces and UVs onto `PipBoyArm01.dds`, 2026-11-05]. The shape's own local Z
    leans 28° off that shaft. Stood square to the plate it was seen tilted too far against the
    original (playtest 2026-11-05), so it leans 17.6° with its top towards the top of the
    device, which is what dropping the node's own tilt would give (`pipboy_controls.rs`
    `KNOB_SHAFT`, `KNOB_LEAN_AXIS`). That the original does so is [hypothesis]: silhouettes
    against the private job's five tabs do not settle it (0.145 against 0.147 for square),
    though a free fit of the lean landed within 3° of it. The turn itself: the page's tab, not
    the page (tab 0 of STATS, ITEMS and DATA look the same, and the wrap comes back), five
    detents at the dial's five marks. Bringing the fin's tip onto each mark on the screen, by a
    camera fitted to the private job, takes 67°, 37°, 10°, −16°, −49° [observed]; a constant 28°
    a tab, 66° at tab 0 and −46° at tab 4, is within 3° of those, and that the step is constant
    is [hypothesis]. A step is over in about 8 game loops [observed]; 180° a second, and the wrap swinging back at that speed,
    are [hypothesis].
  - `ScrollKnob`, about its local X, its axle: the list's selected row, one ridge a row (the
    wheel has 26 ridges, its outermost vertices every 13.85° [observed: `fnvtool nif decode`]),
    200° a second. It comes back to the same place at the top of a list [observed: private job
    pipboy_special_device_motion: its ridges to within 0.3 pixels]; the rest is [hypothesis]:
    that it follows the selected row and not the list's scroll, which the capture does not
    separate; the ridges' 17 pixels hide whole ridges moved between shots, so the step is not
    in the capture (a first reading of 2° a row was far too little against the original,
    playtest 2026-11-05), and the sign is not resolved. The mouse wheel at the top of
    the list moved neither. Following the row means the pointer on a row turns it too [design].
  - Absolute `bind × rotation` on the shape's own transform, after the arm is posed; a rebuilt
    rig's shapes take a bind of their own. Only rigid shapes of the size of the game's own
    male and female arm turn: a replacement model can share the names with other pivots,
    axes or skinning, so others are left still, said once ("pipboy: needle left still: …").
    Only the arms rig: the third-person model's controls stay still and NPCs' arms are not
    touched [design]. Off the arm a control finishes travelling to the last reading, then
    holds [hypothesis]; the first reading is taken at once. Logged "pipboy: needle settled at …",
    "pipboy: tab knob settled at …", "pipboy: scroll wheel settled at …".
- **The view** is first person while it is up and goes back to what it was [hypothesis].
- **The world behind** is blurred by `fBlurRadiusPipboy` (3.5) in the image space
  modifiers' blur units [hypothesis: the same units], eased with the clip; what is
  nearer than 40 units is the player's arms and is neither blurred nor blurred into the
  world [design]. `fBlurIntensityPipboy` is not used (what it scales is not known).

## 3. The screen

- **Canvas** [documented: globals.xml `_pipboy_width` 1024, `_pipboy_height` 768]: the
  menu files lay out the Pip-Boy menus in 1024 × 768.
- **Mapping** [observed]: the screen's texture coordinates run from 0 to about 0.75
  (u −0.010 to 0.753, v 0.002 to 0.762) over a 4:3 shape. Fitted to the owner's ITEMS
  screenshot, the canvas fills 0.8 of the texture each way (the menus were 1.07 times as
  wide and 1.10 times as high as a fill of 0.75 gave): a 1024 × 768 canvas in the top
  left of a 1280 × 960 image. So the screen shows the canvas to about 964 across and 731
  down. NVForever draws the menus into such an image (interface lengths are its pixels)
  and puts it on the screen as it is.
- **Colour** [observed: private job pipboy_special_device_base]: a filled bar in the
  original at noon in Goodsprings, level view, is about (177, 133, 47), dimmer than the
  playtest shots below, about (206, 163, 57) indoors and (201, 161, 54) at dusk. Whether the
  hour or the capture's own settings make the difference is not established. `SCREEN_TINT`
  is fitted to the playtest shots (the static treatment below). `fBrightenPipboy` is not
  applied as such.
- **The glass** [observed: private job pipboy_special_device_base, 11 × 11 means at 1280 ×
  720]: at eight empty places the original runs from (7, 6, 2) at the far left edge, (15,
  11, 3) at the far right and about (20, 15, 4) beside the figure on the left, to (30, 25,
  11) lower right and (79, 77, 49) at the top middle above the figure: a strong falloff to
  the edges, warm, and brighter across the top. Through the engine's display-value add
  (nif.md §6) the glare lay over the menus as a pale grey-blue haze, so it was hidden.
- **The original's screen in playtest shots** [observed: 1920 × 1080 shots of the original,
  2026-11-05]: a filled bar reads about (206, 163, 57) indoors and (201, 161, 54) at dusk, the
  same screen in both. Across a scanline red stays almost flat (201 to 211) while green runs
  145 to 183 and blue 48 to 66, so the stripes read yellow and orange, not grey. Below the
  bars the ink is yellower than amber's own ratio: green about 0.74 of red and blue about 0.3
  at every mid brightness. The empty screen is about (35, 27, 15) beside the figure and (22,
  17, 7) at the left, the glare at the top about (80, 73, 50). These shots are brighter than the
  private job's above; the numbers below are fitted to them.
- **The static treatment** (nvf-play `pipboy/device_screen.rs`, all its numbers in that
  file): image × `SCREEN_TINT` × the shapes' vertex colour (0.624 in linear light), plus the
  glare, with red stopped at a ceiling, through Bevy's TonyMcMapface table. The scene's
  image-space grade leaves the screen alone: the shape marks its pixels in alpha
  (`NifSurface::pipboy_screen`, `imagespace.wesl` `SCREEN_MARK`), because the grade at noon
  brightens by 1.3 and the original's screen does not follow it; the fade and VATS still cover
  it [design]. In this engine at 1920 × 1080 a filled bar reads (207, 169, 64) indoors, red
  206 to 208, green 154 to 189, blue 58 to 73, and (200, 163, 62) at noon in Goodsprings; the ink
  has green at 0.73 and blue at 0.28 of red at mid brightness, the empty screen is (35, 28, 13)
  beside the figure and (25, 19, 8) at the left, the glare (79, 72, 49) [observed: this engine's
  shots, 2026-11-05]. Custom colours and other hours are not measured.
  The ceiling is [hypothesis] for how the original comes by its flat red; it is fitted, and
  the original's own path is not established:
  - The falloff is the background picture's vignette: `pipboy.dds` runs from about 9 at its
    corners to 93 near the top of its middle [observed: the file]. Drawn over a large
    undercoat, as the first fit had it, the vignette was lost. The picture is at brightness
    120 (the menu file's 90 at the first tint [documented: inventory_menu.xml]), its red in
    linear light at 0.7 of amber's as the original's empty screen is less red, covering
    the undercoat at alpha 0.4 and adding what it would over black.
  - The undercoat, the screen camera's clear colour, is next to nothing: (0.00047, 0.00055,
    0.00054) of light in the view.
  - The glow across the top and the patch on the right are `glare:0`'s: its texture's
    brightest band and patch lie there [observed: the file]. It is drawn over the menus on a
    copy of its material, unlit, added in linear light as the terminal's glare is (hacking.md
    §7), at a linear scale of (3.2, 1.4, 0.75) on the texture, warmer than the texture.
    The copy keeps the shape's squeezed first-person depth, which Bevy's plain additive
    material would not, and the sheet's soft intersection is off.
  - `ScreenLit:8` stays hidden: `screen.dds` is black, `Screen_n.dds` is flat and its
    specular colour black [observed: the files; `fnvtool nif decode`], so it carries no
    falloff. Both shapes used are unlit, so nothing here is lit by the world [design]; the
    hour does not move it here, the grade being left off.
  - One baked column of scanlines over the canvas, a value per row (no tiling): the band
    of `pipboyscanlines.dds`, rescaled to dim display values by 20% at its dark end, every
    9.5 of the canvas: about 22% on the empty screen (the original 16 to 22%) and about 18% in
    green on a filled bar (the original 21%). The undercoat and the picture are raised by what the lines take
    away on average, as the terminal's are; the ink is not, its bright rows keeping their
    value.
  - One owner around the shared image: the layers are not part of the pages' tree, so
    any painter of the pages gets them, once. The owner retints the picture itself when
    the Pip-Boy colour changes, and the generic recolouring leaves its nodes alone; the
    undercoat is the glass and keeps its colour [hypothesis]. Not in the screen menu (no background, no
    scanlines, no glass, docs/design/screen-menu.md §2). The device's image shown flat
    because the model is missing keeps the layers and has no glare.
- **Scanlines** [observed]: `pipboyscanlines.dds` is 64 × 64 DXT1, one soft dark band
  (white at the top to about 0.8 at the bottom). On the screenshot the solid bar's rows
  are bright and dim by about 15% every 6 pixels, which is 9.5 of the canvas. Flicker is not
  built; the page change's transitions are below.
- **The beam**: a wave of light travels down the screen, its leading edge hard at the foot and
  a long fade behind it [observed: private job pipboy_special_device_motion, the same empty
  stretch of the screen frame by frame; playtest 2026-11-05]. The private job pipboy_fx_still
  (2026-11-10, 1280 x 720, 300 frames on consecutive game loops of an empty stretch of the
  Perks page, then 60 eight loops apart) measured it again [observed]:
  - Its leading edge passed the same row 5.12 s apart by the wall clock (the earlier 5 s was a
    whole-second clock's), going 231 and 239 rows a second down the shot in two jobs (156 on one
    sweep). The screen there is about 416 rows for about 731 canvas units, about 1.8 a row
    [hypothesis], so about 415 canvas units a second.
  - On the mean of the frames holding the whole wave, aligned at the edge, the light rises about
    straight over 72 rows (55 from 10 to 90 %) to +13.6 in display red on a base of 22, then
    falls to nothing over 24 rows (14 from 90 to 10 %). What it adds is about (1, 0.63, 0.12) in
    linear light on one job and (1, 0.50, 0.11) on the other (the earlier reading (1, 0.58,
    0.17)).
  - It is not banded: with about 14 display levels over some 70 rows a quantised ramp would step
    every five rows, but frame by frame a pixel's level along the rise changes every row at the
    median (runs of 1.6 rows on average), and a quarter of its steps go down while the wave
    rises. An empty stretch varies by 0.61 levels from frame to frame (standard deviation) without
    the beam. Noise or a dither breaks the steps up [hypothesis: which, and where, is not known].
  NVForever (`device_screen.rs`): a rise of 130 canvas units and a leading edge of 43, 173 in
  all (`BEAM_RISE`, `BEAM_EDGE`, `BEAM_HEIGHT`: the measured 72 and 24 rows at the 1.8 a row
  above, 129.6 and 43.2, rounded), the Pip-Boy's colour
  lightened 0.2 towards white at a fitted alpha of 0.016 at the peak, over the pages and under
  the scanlines, 2.3 s from above the top edge to off the foot every 5.12 s on the real clock,
  hidden between sweeps and held with the pause menu; not in the screen menu. The earlier 224
  and 24 units and 1.3 s every 5 s were fitted to single frames and a whole-second clock and are
  replaced. Its picture is 2048 x 173 with a triangular dither of up to a display level on the
  alpha (less near either end, so it never clips), read texel for texel from a new column every
  frame, so the 8-bit image holds the rise as a mix of neighbouring levels, not as bands
  [design]. The screen's noise (below) adds the frame-to-frame variation.
- **Page changes** [observed: private jobs pipboy_fx_switch_a and _b, 2026-11-10, 1280 x 720:
  30 switches in each of the six directions between STATS, ITEMS and DATA, 20 STATS and 10
  ITEMS tab steps, 10 bursts with no change; one shot of the old page, then 13 on consecutive
  game loops from a delay that differs from switch to switch, so the switches together cover
  the first 0.3 s densely; measured against the settled new page]:
  - Every page change is a transition of about a quarter of a second. At once the picture keeps
    only about an eighth of its detail on a lift of about 16 display levels (a gain of 0.12 to
    0.15 on the display values, fitted per band of eight rows on ink and background); from 70 to
    130 ms about a fifth on +10; it comes back from there (0.34 at 150 ms, 0.58 at 190, 0.74 at
    210) and is whole again at about 225 ms (the median settling; 196 to 294 over all of them).
    The new page shows through from 30 to 200 ms (about 100 at the median). The bursts with no
    change show none of this.
  - About a third also move the whole picture: 13 of 180 switches roll it (7 %), 35 jump it up and
    down by about 15 rows (19 %). With `fVertHoldChance` set to 1 and `fShudderChance` to 0 by
    SetNumericINISetting every one of 12 switches rolled; with them the other way round every one
    of 12 shuddered; with both at 0 none moved [observed: private job pipboy_fx_forced]. So the
    roll is the vertical hold and the jump the shudder, each rolled once a change at the INI's
    0.08 and 0.20 [hypothesis: once a change; the shares fit].
  - The vertical hold: the picture rolls up about 43 rows a game loop, wraps at its foot, and
    comes to rest from below at 250 to 290 ms: once through its whole height.
  - The shudder: the same steps every time within a row or two: 14 rows up at about 50 ms, 15
    down at 95, 10 down at 120, then 7 and 9 up, 5 up, 3 down twice, still by about 250 ms; never
    sideways. What fills the rows the jump uncovers at the top or the foot (the canvas's far
    edge wrapping in, the undercoat, or black) was not looked at.
  - A tab change is a small brighten and nothing more: a gain of about 1.05 and +3.5 levels at
    once, +2 at 100 ms, gone by about 200 ms, the new tab there from the first shot.
  NVForever (`pipboy/transition.rs`, all its numbers there): when the page on the arm changes, a
  transition of that kind runs on the real clock, held under the pause menu (its time stops
  and the picture stays): the wash by the table above, 225 ms; the shudder's steps, 245 ms; the
  vertical hold's roll from 40 to 255 ms, 270 ms (its wash stretched to the length); a tab's
  brighten, 200 ms. The hold is rolled first at 0.08, then the shudder at 0.20 [documented:
  `Fallout_default.ini` `[InterfaceFX]`]. The page itself is drawn at once as before, under the
  wash [design]. It is the screen material's (`ScreenLook` put on the menus' copy by
  `pipboy_arm.rs` `upload_look`, in the same frame as the change, after it is decided;
  `nif_material.wesl` `pipboy_picture`, which reads every tap inside the canvas, wrapped while
  it rolls and half a texel in from its edges), so it covers whichever painter drew the
  page. The shudder moves the picture through the same wrap as the hold, so the rows it
  uncovers at one edge come in from the canvas's other edge [hypothesis: not compared with
  the original]. The flat image where the model is missing and the screen menu have none [design].
  Logged `pipboy: page change transition <plain|shudder|vertical hold> (<ms> ms)` and `pipboy:
  tab change transition brighten (200 ms)`; the test aid's `glitch=` forces the kind as the
  forced job did.
- **Text edges** [observed: private jobs pipboy_fx_still at 1280 x 720 and pipboy_fx_text at
  1920 x 1080 scaled to 1280 x 720, twelve stills each of S.P.E.C.I.A.L., Status, Skills, ITEMS
  and DATA, the per-pixel median of the frames that did not sway]: the text's edges rise from
  10 to 90 % over about 1.6 pixels on most of it (a straight two-pixel ramp), 1.8 to 2.1 on the
  titles, the same along rows and columns, where the pointer's own hard edge measures 0.8. With
  `fBlurRadiusPipboy` and `fBlurIntensityPipboy` at 0 they measure the same, so that blur is not
  what softens them; what does is not known (`fBlurRadiusMenus=0.30` is a [hypothesis]).
  NVForever: the screen's material reads the menus' picture through a 3 x 3 tent of taps 0.4 of
  a view pixel apart (`device_screen.rs` `TEXT_SOFTEN`) [observed: the same measure on this
  engine's 1280 x 720 shots (scenario `pipboy-screen-text`): none 1.48 to 1.60 px, 0.75 1.69 to
  1.96, 0.4 S.P.E.C.I.A.L. 1.59, Skills 1.59, ITEMS 1.58, DATA 1.66, Status 1.62 (the
  original's Status, 2.06, is softer than this tent makes it)], and adds a triangular
  noise of 1.35 display levels' reach, new every frame (`SCREEN_NOISE`; about 0.6 levels once
  rounded) [design], on the model only.
- **Brightness of the drawn picture** [observed: S.P.E.C.I.A.L. and Status at 1280 x 720, the
  original's private job pipboy_fx_still against NVForever's scenario pipboy-screen-text]: the
  original's text peaks at 1.31 times its lines' brightness (heading line 128 red, names 168);
  drawn at the menu files' 255, NVForever's peaked at 1.57 (129 and 202). The same holds for
  the pictures the text's brightness tints: the S.P.E.C.I.A.L. Vault Boy reads 191 red at its
  99th percentile and 80.7 on average in the original, 212 and 96.2 at 255; the Status page's
  bright parts 114 at the 95th percentile, 169 at 255; the bars sit at the screen's red ceiling
  (about 205 to 212) either way. On the device NVForever draws the hand-built pages' text (and
  so their pictures) at 213 (`frame.rs` `DEVICE_TEXT`, chosen by `Kit::text`): names 167, Vault
  Boy 190 and 85.5, Status 144, lines unchanged [observed: the same scenario]. Why the
  original's drawn picture comes out dimmer is [hypothesis]; the dimmed text (128) is not
  measured. Only the device was measured, so the screen menu (Use Pip-Boy off) keeps the menu
  files' 255 (`frame.rs` `TEXT`). The flat image shown when the Pip-Boy model has no screen
  is the device's drawing and keeps its 213 [design].
  The pages drawn from their menu files (`[UI] TileMenus=StatsMenu`, `InventoryMenu`,
  `RepairMenu`, `ItemModMenu`, `MapMenu`; on by default when the list is absent [design, #2215]) take the same device
  brightness, by role as the hand-built pages do, not by a flat factor [design: #2212,
  `tiles::draw::device_brightness`, applied in the tile painter before it compares the
  pieces, for every Pip-Boy page and the DATA page's map picture]: while the Pip-Boy is on
  the device (`Frame::Device`, its flat fallback image included), a piece painted at the
  page's colour at the file's full level (resolved `brightness` 255 over `&pipboy;`) takes
  213 of 255: the texts, the Vault Boy and icons, the bars, the map's picture (its alpha
  128 kept). A piece the file already dims keeps the file's value: the boxes' lines at 200
  (the hand-built lines' 200), the dimmed texts and the highlight fills at 128, the
  background at 90; so do the maps' location icons (`MapMarker`, 220 and 255 under the
  highlight box, as the hand-built page keeps them in both frames) and any piece in another
  colour. The alpha is never changed. One difference the rule keeps: the file's heading
  lines (`MM_Headline_h*`/`_v*` and the like, `line.xml` with its brightness picked as
  255 by the heading) resolve at 255 [observed: fnvtool `ui tree` of map_menu.xml and
  item_mod_menu.xml at 1280 x 720, `_brightness` 2, `brightness` 255], so on the device they
  come out at 213 where the hand-built heading's lines are 200; the rule is kept whole
  rather than special-cased by picture [design; to be checked on shots]. In the screen menu
  the tile pages keep the file's values, as the hand-built pages keep 255 there. The
  hotkey panel over the pages and the radio's waveform are drawn by hand and keep their
  own colours. With every tile switch off nothing changes.
- **Background**: `Interface\Shared\Background\pipboy.dds` over the whole canvas at
  brightness 90 [documented: inventory_menu.xml]; `ScreenGlare.dds` and this picture are
  512 × 512 DXT1, alpha 1 throughout [observed].

## 4. What every page shares [documented unless marked]

- The menus' rectangle: 50 in and down, 855 × 650.
- Heading: a vertical line fading down at its left (60 long), a line of 50 to the title
  (font 4, 70 in), a line from 20 after the title to the first card; cards of figures
  (`card_info.xml`): a line along the top, one fading down the right side, the title 5
  in and the value right-aligned 10 from the right end, both 20 down in font 2. ITEMS:
  Wg at 215 (155 wide), HP 380 (155), DT 545 (110), Caps 665 (190). STATS: LVL 270
  (100), HP 380 (155), AP 545 (135), XP 690 (205).
- Tab line (`tabline.xml`): 650 down, as wide as the menus, a vertical line rising at
  each end; each tab is its name in font 2 with 35 across and 15 up and down
  (`tabline_template.xml`, `text_box.xml`); the tab shown is boxed (lines at brightness
  200) and filled (brightness 128, alpha 32); the lines between the tabs are as long as
  each other [hypothesis: the file leaves their length to code; on the screenshots they
  are even].
- Lines are 2 thick on a PC; lines at brightness 200, text 255.
- A text's letters stand 15/31 of the font's size under its top [hypothesis for every
  font but 2, where it was measured: message-menu.md §3].
- **The words** (#1443). Where a menu file names a game setting for a label, the label is
  that setting's text: the ITEMS heading `sInventoryWeight`, `sHitPointsShort`,
  `sInventoryDamageThreshold`, `sInventoryCaps` (`inventory_menu.xml`'s
  `IM_Headline_Player*Info`); the STATS heading `sStatsLVLAbbrev`, `sStatsHP`,
  `sStatsAP`, `sStatsXP`, its tabs `sStatsStatus` … `sStatsGeneral` and its
  `sStatsCNDAbbrev`, `sStatsRADAbbrev`, `sStatsEFFAbbrev` (`stats_menu.xml`); DATA's
  title `sData` and `sPlaceMarker`, `sShowLocation` (`map_menu.xml`); the item card's
  `sInventoryDamage`, `sInventoryWeightUpper`, `sInventoryValue`, `sInventoryStrReq`,
  `sInventoryDamagePerSecond`, `sInventoryCondition`, `sInventoryEffects`
  (`item_stats_display.xml`); the hints `sInventoryRepair`, `sInventoryMod`
  [documented: the menu files]. The STATS and ITEMS titles, the ITEMS and DATA tabs,
  Maintain and the "Level" under the player's name have settings no menu file names:
  `sStats`, `sInventoryItems`, `sInventoryWeapons`, `sInventoryApparel`,
  `sInventoryAid`, `sInventoryMisc`, `sInventoryAmmo`, `sLocalMapTabText`,
  `sWorldMapTabText`, `sQuestsTabText`, `sMiscTabText`, `sCommsTabText`,
  `sMaintainItem`, `sLevelAbbrev` [observed: a Russian install's language plugin
  defines each, and its text there is the word of that place, 2026-11-08]. The English
  masters define none of them [observed: FalloutNV.esm and the add-ons], so an English
  install shows NVForever's own words; a plugin that defines one is drawn with its
  text (`pipboy::labels`) [design].
- **The hints' words** (#1896, #1897). The ITEMS and DATA hints and the mod menu's read
  their settings the same way: `sInventoryRepair`, `sMaintainItem`, `sInventoryMod`,
  `sExit`, `sChallengeToggle` with the pages' own words beside them ("Repair",
  "Maintain", "Mod", "Exit", "Challenges"); `sPlaceMarker`, `sShowLocation`,
  `sShowActiveNotes`, `sShowAllNotes`, `sNextChallenge`, `sInventoryEquip`,
  `sInventoryDrop`, `sInventoryHotKey`, `sTravel`, `sMakeActiveQuest`, `sPlayAudio`,
  `sStopAudio`, `sTune` with NVForever's labels in the game strings table
  (`fnv_ui::strings`: "Place Marker", "Show Location", "Active Notes", "All Notes", "Next
  Challenge", "Equip", "Drop", "Hotkey", "Travel", "Set Active", "Play", "Stop", "Tune
  In"; `labels::built_in_label`) [documented: the setting names and the menus that use
  them, as that table cites them; design: the words]. "Use" (aid, books) and "Unequip"
  (what is worn) have no setting named and stay NVForever's own words [design].

## 5. The pages

- **ITEMS** (`items.rs`): the inventory model (`menu::model`, `Mode::Inventory`), the
  tab choosing the kind of item. Built: the heading (Wg, HP, DT and Caps), the list
  (name, count in brackets, the filled square of what is equipped, the selected row
  boxed), 9 rows at once (51 a row in a list 468 high). A row (its box) runs from the
  scroll bar to 5 short of the list's right edge, as the DATA lists' [design: it was the
  Repair page's 393, which ran over the item's figures; owner's screenshot 2026-11-01].
  - **Ammo the weapon cannot fire is dim** (#3442): a row of ammo the weapon in hand
    cannot fire is drawn at half brightness; the weapon's other kinds (the slug with the
    shotgun, the .38 with the .357) stay full, every ammo row is dim with the fists out,
    and a dim row can still be selected [observed: observer check 288, run 2026-11-22,
    text colour about half; results on PR #3466]. Ours: `Row::dim` from
    `FormStore::ammo_choices` of `Combat::player_weapon`, alpha 0.5 in the hand-built list
    and 128 in the list drawn from the menu file; only in the player's inventory, not in a
    container's or a trade's columns [design]. Not observed: a holstered weapon (ours
    treats it as equipped), dropping a dim row or putting it on a hotkey (ours allows both).
  - Added 2026-10-23 (owner's playtest list 7, items 5, 6, 15, 20, 33):
    - **HP** is the STATS page's health out of its whole; **DT** the threshold a hit on
      the player meets (worn armour at its condition plus the Damage Threshold actor
      value, `combat::target_armor`), one decimal [observed: "HP 205/205", "DT 8.0" on
      the owner's screenshots; hypothesis: the figure is combat's].
    - A name over two lines makes its row taller; the first row drawn moves on so the
      selection is always in the list, and the model keeps it as its scroll position (the
      last of many weapons could not be brought into view: the model counts nine rows a
      page) [design].
    - An aid item's EFFECTS: aid-items.md §14.
    - Skill books (BOOK with a skill to teach) are on the Aid tab, other books on Misc
      [owner: "Skill books ... should be showing in the Aid tab"; hypothesis: the
      others].
    - The Misc tab's folders [owner's wish; design]: the keys (KEYM) are one "Keyring"
      row and the Caravan cards one "Caravan Cards" row, at the top, each with its count;
      using one lists what it holds under a "Back" row (Backspace too; another tab closes
      the folder). The cards are those AddCardToPlayer gave (kept outside the inventory,
      caravan.md §2), and any CCRD item carried; they are neither used nor dropped.
  - The scroll bar at the list's left (32 wide), only while some rows do not show: the
    arrows at its ends and a line for the part shown [observed: the owner's scroll
    screenshot; the line's look as the DATA lists' (hypothesis)].
  - The selected item (the row under the pointer is selected) at the right
    [documented: places, `inventory_menu.xml`, `item_stats_display.xml`]: its picture
    (ICON) in `IM_ItemIcon` (450, 10, 380 square); its figures in `IM_ItemInfoRect`
    (385, 340, 446 × 150), cards a third of it wide and high, 10 apart: DAM (WEAP base
    damage) or DT (ARMO DNAM) at the left, WG and VAL; a weapon's STR (its Strength
    requirement) over VAL; the second row CND (a bar 62 in, 15 down, 20 high, 72 short
    of the card) and beside it, two cards and 10 wide, the ammunition ("20ga (2/30)":
    the AMMO short name, then what a clip holds of what is carried over what is
    carried, hypothesis) or armour's class ("Light", "Medium", "Heavy"); EFFECTS (a value too long for the room
    beside the title, as an aid item's four Hardcore effects, goes on in right-aligned
    lines under it, broken between effects: `frame::card_value_lines` [design, #2537; the
    file's `CI_ValueText` has a `wrapwidth`, documented, its look is not captured]) across
    the three cards on the first free row: an aid item's own effects, apparel's or a
    weapon's enchantment's (EITM), written as the STATS EFF list writes them ("HP +40",
    "AGL -1") [observed: the owner's ITEMS screenshots, all five tabs]. WG and VAL are
    the stack's (the Stimpak's VAL 675 is 9 × 75; WG "--" for nothing) [observed: VAL;
    hypothesis: WG].
  - Ammo effect card lines (#2502): see the formatting and shared-card rule below
    [design, #2502].
  - Added 2026-10-19 (`items.rs`):
    - **DPS** [documented: `item_stats_display.xml` `DPSInfo`, `sInventoryDamagePerSecond`;
      hypothesis: formula]: the damage a shot does at the item's condition
      (combat-rules.md §17) times its shots a second (an automatic weapon's fire rate,
      else its attack shots a second, else one), rounded [hypothesis: the GECK wiki's
      Weapons page names damage and fire rate as the DPS calculator's inputs]. Shown in
      the container and barter figures only: the original's ITEMS page has no DPS card
      (2026-10-24, below) [observed: the owner's ITEMS screenshots].
    - **The weapon's skill badge** (2026-10-24; replaces the ammunition's picture and
      the DPS card guessed on 2026-10-19) [documented: `IM_ItemIconBadge` at (390, 260),
      64 square, and the text `IM_StrReq` at (440, 300), font 7, left-justified;
      `textures\interface\icons\typeicons\weap_skill_icon_*.dds` (sm_arms, big_guns,
      energy, explosives, melee, unarmed), 64 square, a boxed picture in the lower left
      48; observed: the owner's screenshots show the bullets picture with 0, 25 and 50
      for the Single Shotgun, Service Rifle and Sawed-Off Shotgun (their Guns skill
      requirements, WEAP DNAM) and the knife picture with 25 for the Throwing Spear
      (Melee Weapons 25)]: the picture of the skill the weapon uses and, beside it, the
      skill it needs, "0" too. A weapon of another skill (binoculars: Barter) shows
      neither [hypothesis].
    - **The buttons' text** (Repair/Maintain R), Mod X), the repair and mod lists'
      Cancel/Exit) [documented: `text_box.xml`: the text sits half the box's 15 spare
      height plus 10 under the button's y; observed: "Maintain R)" clear of the heading's
      cards on the owner's screenshot]. Before 2026-10-24 it was drawn at the button's y
      and touched the Caps card (the owner's report).
    - **"Mod X)"** [documented: `IM_ModButton`, `sInventoryMod`, PC key X; observed: the
      last row of the hints on the original's Weapons tab (screenshots on issue #1896);
      hypothesis: shown for weapons with mod slots (WMI1–3), bright when the
      player carries a mod that fits a free slot, dim otherwise]. X on a bright hint opens
      the mod menu (2026-10-20, weapon-mods.md §4–§5); elsewhere X does nothing, a right
      click drops (§6) [design: owner 2026-11-02, #1393].
    - The repair hint ("Repair R)" / "Maintain R)") is built (repair.md §5), and the
      repair list takes the pointer (repair.md §5).
  - **Every tab's hints** (2026-11-08, #1896; `items.rs::page_hints`), by the input device
    (`crate::pad::labels`, the Controls page's Input Device; a change of device draws the
    page again):
    - **With a controller** the whole column, top to bottom in the original's order: the
      equip action ("Equip A)", "Unequip A)" on what is worn, "Use A)" on aid and books),
      "Drop X)", the repair action ("Repair Y)", "Maintain Y)" at full condition; dim on
      what nothing repairs: aid, misc, ammo), the Hotkey hint, and on Weapons "Mod LB)" (dim
      on a weapon without mod slots) [observed: the original's screenshots on issue #1896,
      taken with a controller connected, 1920 × 1080: Equip (Use on Aid), Drop, Repair,
      Hot Key, then Mod on Weapons only]. [design, #2312; `hotkey_panel.rs`]
      Hotkey now names RB (R1 with PlayStation labels): it opens the panel for the
      selected item, D-pad right/down advances and left/up goes back with wrapping,
      A assigns and B cancels. This selection method is provisional [hypothesis;
      observer check 211]; Ammo Swap refuses assignment. The word still comes from
      the load order's `sInventoryHotKey`, else our built-in label.
    - **With the keyboard and mouse** only the repair action ("Repair R)" / "Maintain R)")
      on every tab, dim where it does nothing, and "Mod X)" under it on Weapons [observed
      2026-11-09: the original's InventoryMenu button tiles read with keyboard and mouse
      on each ITEMS tab: `IM_EquipButton`, `IM_DropButton` and `IM_HotkeyButton` hidden
      on every tab; `IM_RepairButton` shown on every tab, its target off where it does
      nothing (Aid, Misc, Ammo, an item at full condition); `IM_ModButton` shown on
      Weapons only]. Enter, the right click and the number keys still equip or use, drop
      and assign hotkeys; only their hints are not shown. Until 2026-11-09 the keyboard
      had the whole column too, with "Enter)", "Right Mouse)" and "1-8)".

    Dim also: the equip action when Enter would do nothing,
    by the game's own checks (`menu::model::enter_acts`: `World::equip_check`, which
    `World::equip_at` runs, and `World::unequip_check`): misc and ammo, a book that
    teaches nothing, a broken weapon not worn, what a script locked; the hotkey on what a
    hotkey cannot hold (misc, ammo; weapon-wheel.md); drop on a quest item or a folder
    row; every hint on an empty tab [design]. [documented: `IM_DropButton`'s X; observed:
    the issue's report and screenshots of the original's sets per tab.] The hotkey panel
    still hides them all. Log: one `pipboy hints: items <tab>: <hint>, …` line of the
    hints drawn, "(dim)" after a dim hint, whenever the set changes (with the device too);
    the item's own `pipboy items: <name> Repair R)` and `… Mod X)` lines stay (with a
    controller `… Repair Y)`, `… Mod LB)`).
    - **Their places** (`frame::Ctx::hint_column`): the first row at the Repair button's
      place (855, 45) [documented: `IM_RepairButton`], its text `BUTTON_TEXT_Y` lower as
      every button's; the rows 46 apart; each word right-aligned at 855, the heading's
      right end, its key just after it [observed: the same screenshots: the first row's
      letters 40 px under the Caps card's, rows 42 to 43 px apart against the item list's
      47 to 48 px rows there (0.90), the words ending at the heading's right end with the
      button glyph beside them] [design: in our units 0.90 × our 51-high list rows = 46;
      40 px is about 43 units, our first row's letters stand 42.5 under the Caps card's].
      With the keyboard the same column holds Repair and, on Weapons, Mod under it. The
      keys may reach 45 past the menus' rectangle (the lit screen ends about 49 past the
      heading at the first row on NVForever's 1280 × 720 frame); when the widest would
      run further (a long key name, as the mod menu's "Enter)"), the whole column moves
      left until it ends there, the words still on one line [design]. The column lies inside the picture's documented box (450 to 830), as
      the original's words do; it is drawn after the picture, over it, so no picture
      hides a hint (before 2026-11-09 the 10mm Pistol's picture covered "Drop" on
      NVForever's own frame). Until
      2026-11-09 the order was Repair, Mod, Equip, Drop, Hotkey, 47 apart, each whole hint
      right-aligned 10 short of 855.
  - The mod menu (`item_mod.rs`): weapon-mods.md §5. Its two hints are a column as the
    page's, from (855, 75): "Mod Enter)" first, "Exit E)" 46 under it [observed: "Mod"
    over "Exit", 43 px apart, on the original's screenshot on issue #1896; documented: the
    file lists the Mod button first]; until 2026-11-09 Exit stood over Mod, 40 apart. The
    same two rows with the keyboard and mouse [hypothesis: no reading of the original's mod
    menu with the keyboard yet].
  - The flat image (no model's screen) shows only once the Pip-Boy is all the way up:
    before, it flashed for a frame or more on every Tab while the body took the screen
    (the owner's list 5, item 2).
- **STATS** (`stats/`): §5.1.
- **DATA** (`data.rs`, `data/`): §5.3.

### 5.1 STATS

Implementation: `pipboy/stats/mod.rs` (layout, keys, pointer, test aid), `model.rs` (the
words and rules), `read.rs` (the records and the player's figures). Sources:
`menus\main\stats_menu.xml` and the prefabs it includes (`list_box.xml`,
`list_box_template.xml`, `super_text_box.xml`, `text_box.xml`, `meter.xml`,
`scrollbar_vert.xml`, `box.xml`) [documented]; the owner's screenshots 1–5 (Status CND,
RAD, EFF, S.P.E.C.I.A.L., Skills); none of Perks and General, which are laid out from the
file alone. Compared side by side at 1280 × 720 with a new run (default stats): every
tab with a screenshot stands within a few pixels of the original [observed].

**Images in the file** [hypothesis, supported by the file's comment "filewidth/fileheight
already adjusted for zoom"]: an image tile shows its file at the tile's zoom from the
top left, cut off at its width and height, not stretched. The Vault Boy's pieces then
fill exactly the parts of their files the file gives as their sizes (the head's outline
is the top left 85 × 92 of `head.dds`) [observed].

- **Heading** [documented: places, `stats_*_info`]: LVL (the player's level), HP and AP
  as current/maximum (actor values 16 and 12: base and permanent modifier, less damage),
  XP as experience/what the next level needs, (n − 1) × ((n − 2) / 2 × `iXPBumpBase` +
  `iXPBase`) [documented: GECK wiki Settings; 200 at level 1, as the screenshots].
  Without the scripts' game the figures are "--".
- **Status** (`stats_status_container`, 50, 110, 855 × 482):
  - CND, RAD, EFF down the left, 84 wide, 51 high (font 2 and 20), boxed when pointed
    at, boxed and filled when chosen [documented]. The hardcore buttons (H2O, FOD, SLP)
    under them are not built.
  - Hints at the top right, right-aligned at 900, a slot 51 high each: "(9) Stimpak S)",
    the limb-switch hint, "(3) Doctor's Bag E)"; on RAD "RadAway
    A)", "Rad-X X)" [documented: places, `_PCButton_*`; observed: the words; hypothesis:
    the count is left out at 0 (the RAD screenshot has none), dim unless pointed at].
    The four items are the DOBJ list's Stimpak (place 0), Doctor's Bag (21), RadAway (3)
    and RadX (2) (default-objects.md §1), else the Editor IDs `Stimpak`, `NVDoctorBag`,
    `RadAway`, `RadX` [observed: FalloutNV.esm's list names those four, so nothing changes
    with the shipped data; ESM full audit 2026-10-24].
    **Using them on CND** (built 2026-10-23, owner's request; updated for Hardcore limb
    selection, #3431) [design]: S or a click on the Stimpak hint uses one on the body, or on
    the limb chosen (keys 1-6: head, torso, left arm, right arm, left leg, right leg; the
    same key again the body; the pointer on a limb's bar chooses it while there, and a click
    on the bar, word or picture uses a Stimpak on that limb outside Hardcore, or a Doctor's
    Bag on that limb in Hardcore); E or the Doctor's Bag hint uses one on every limb outside
    Hardcore, and in Hardcore on the limb hovered or chosen (or every limb when none is
    aimed). An item is not used when it would not heal: a Stimpak on the body at full health,
    on a limb that is whole, a Stimpak on a limb in Hardcore mode (it mends no limb there
    [observed: FalloutNV.esm help text, "In Hardcore Mode, a Doctor's Bag or an actual
    doctor is required"]), a Doctor's Bag on a limb that is whole, or on the body with no
    limb hurt, or none carried (`model::refusal`). The chosen or pointed-at limb's bracket
    is boxed (on both hand-built and tile-drawn pages). The item is taken, its consume sound
    plays and the runtime uses it as the inventory does (`stats::use_hints`); what it does
    to the limbs: aid-items.md §3.
    [design: `engine/nvf-play/src/pipboy/stats/mod.rs`, `stats/read.rs`, `pad.rs`;
    #1898] The middle hint uses `sSwitchLimbs` through `ui_loading::game_string`,
    with the repository's string table as fallback (NVForever's own label) [design:
    `crates/fnv-ui/src/strings.rs`, entry `sSwitchLimbs`]. Its labels follow the other
    hints: keyboard X, Xbox X, PlayStation Square. Pressing it or clicking its hint
    cycles body → head → torso → left arm → right arm → left leg → right leg → body.
    A pointer still on a limb is cleared when cycling so the chosen limb takes effect,
    and a limb's bar redrawn under the unmoved pointer right after does not take it
    back; other redraws leave a limb under the unmoved pointer hovered.
    [design: `engine/nvf-play/src/pipboy.rs::keys`, `stats/mod.rs::hint_pressed`,
    `stats/mod.rs::use_hints`; #1899] On RAD, keyboard A and controller A/Cross both
    belong to RadAway while its hint is shown: use one carried item through the inventory's
    `ItemUsed` path and stay on Status > RAD. With none carried the hint is dim and
    the press uses nothing and still stays on RAD. Rad-X remains unimplemented.
    A press that opened the page or changed its tab or frame is spent: the view's
    keys do not act on that frame, so A stepping from S.P.E.C.I.A.L. back to a Status
    left on RAD uses no RadAway [design: `stats/mod.rs::prepare`]. A box over the
    Pip-Boy that holds its keys (the tutorial; with the tile pages, any menu over the
    page) holds the view's keys and the wheel too, on every tab, by the rule the
    Pip-Boy's own keys follow, and for the frame it closes in: A closing a tutorial over
    Status > RAD uses no RadAway [design: `pipboy.rs::box_holds_keys`,
    `stats/mod.rs::prepare`].
  - CND: the Vault Boy at zoom 145, head 40 down and centred, face, torso, arms and legs
    placed from it as the file gives; a bracket (`vats_*_divider.dds`, 38 high) with a
    solid bar (77 × 14 at 25 or 51, 24) at each of six body parts, the bar as long as the
    part's condition (actor values 25–30, current over permanent) [documented]. A part at
    0 shows "Crippled" in place of its bracket [documented: the text's visibility;
    hypothesis: the word] and its `*_broken.dds` piece [hypothesis: the name]. The face
    is `face_00.dds` always [hypothesis; when 01–04 and 10 show is not known]. Under him
    "<name> - Level <n>" centred on the area's foot [observed: "Loot - Level 1"].
    Limb damage comes from combat's actor value damage; a new run's parts are whole.
    The bars read the same actor values combat lowers and cripples by
    (`Game::damage_limb`, `PlayerLimbs`) [observed: smoke `pipboy-stats-heal`].
  - RAD (zoom 130): "RAD RESIST" and the value (actor value 20, "8%") in a window
    showing the right end of `rad_effects_bracket.dds`, 360 down; the meter
    (`rad_meter_bracket.dds`) beside it, its 0 at 186 and full at 482 of the picture,
    "500" and "1000" over it, the arrow (`pointer_tail.dds`) at the player's rads (actor
    value 54) [documented: places; observed: a maximum of 1000, and no "0" written at
    the zero mark or at the arrow, so a 0 is not written (hypothesis)]. The radiation
    effects list above (`stats_rad_effects_*`, hidden by the file) is not built.
  - EFF: one line per source, 56 high, the name 20 in and the changes 300 to its right,
    lines between [documented]; the list centred in the area [hypothesis: fitted, one
    line at the middle on the screenshot]. Sources: the enchantment (ARMO `EITM`, ENCH
    of type Apparel) of each worn piece of apparel, named by the enchantment's FULL
    ("Vault Jumpsuit"), its Value Modifier effects written "Speech +2, Melee Weap. +2"
    with the actor values' short names (AVIF `ANAM`) [observed: the screenshot and the
    records], read from the game's active effects (aid-items.md §13): an effect whose
    conditions fail or that a Dispel took off is not listed [design: #2495]; then aid
    items (ALCH) with active effects. A detrimental effect (MGEF flag)
    is written with a minus [hypothesis]. "No Effects" when there is none [hypothesis:
    the words].
- **Lists** (S.P.E.C.I.A.L., Skills, Perks): 35, 110, 325 / 350 / 420 wide (25 / 50 more
  when a row is marked), nine rows of 51; the row 52 in (scroll bar 32 and 20), its name
  15 in and 20 down, its number right-aligned 10 before a right buffer of 20 (55 when
  marked), the mark 10 after [documented]. Skills are in name order [observed: Guns
  between Explosives and Lockpick], Big Guns left out [observed: thirteen; its AVIF is
  "Big Guns - OBSOLETE"]. A value shown is the game's actor value (base, permanent and
  temporary modifiers, less damage), which holds the worn apparel's enchantments in the
  temporary pool [documented: GECK wiki ActorValue; aid-items.md §13; #2495], skills kept to
  0–100; "(+)" or "(-)" when it differs from the permanent value [observed: "34 (+)";
  hypothesis: "(-)"]. Perks: the player's perks that are not Hidden by PERK DATA
  [hypothesis: its bytes as trait, minimum level, ranks, playable, hidden; documented: GECK
  wiki Category:Perk, Hidden "does not show up in the user interface", Playable only that
  the level-up menu offers it]. Perks that are not Playable (challenge perks such as Lord
  Death, implants, companion and quest perks: 62 of FalloutNV.esm's 176 [observed: #3118])
  are listed [hypothesis: as the original does; capture a non-Playable perk's row], traits
  included [hypothesis], in name order, the rank as the number when the perk has more
  than one [hypothesis].
  - Names, descriptions and pictures: AVIF FULL, DESC, ICON (the record of code c is
    FormID 0x514 + c, 0x3E8 + c − 5, 0x44C + c − 12, 0x4B0 + c − 32 or 0x5DC + c − 46 by
    range [observed in FalloutNV.esm]); PERK and REPU the same.
  - The selected row is boxed and filled (alpha 40) after a key moved the selection or
    while the pointer is on a row [hypothesis: neither screenshot shows a box, the
    pointer on the tab line or off the screen].
  - Scroll bar when the rows do not all show: arrows at the list's top and foot, the
    marker 6 wide, as high as the track over (hidden rows + 2) [hypothesis: fitted to
    the Skills screenshot; the file leaves these to code].
  - Beside the list (`stats_display_rect`, from the list's right side to 905): the
    picture at zoom 150 in 340 × 384, centred, 60 above; a line 22 in and 319 down with a
    fading line down from its end; the description (font 2, lines 35 apart) 22 in, 333
    down, 175 high, scrolled 20 a step with arrows at its right [documented]. It scrolls
    when its lines take more than 175 [observed: five lines fit on the S.P.E.C.I.A.L.
    screenshot, six do not on the Skills one].
- **General** (`stats_genrep_container`, 20, 50): the reputations (the file's first mode)
  or the statistics; R or the button "General R)" / "Reputation R)" at the top right
  switches [documented: `_PCButton_R`; hypothesis: the button names the other list].
  The list 500 wide from 40, 100, nine rows [hypothesis: the file's height, 542 from
  100, would reach the tab line]. Reputations: those with fame or infamy [hypothesis],
  the selected one's picture (REPU ICON, zoom 150) and its title by the levels of fame
  and infamy, in NVForever's own words (`crate::reputation::TITLES`) [design, #2982;
  the data holds no title setting], and
  "Fame n, Infamy n" [design]. Statistics: the 43 New Vegas names [documented: GECK wiki
  GetPCMiscStat] in the order of their codes [observed: the owner's screenshot of the
  original's list, 2026-10-14] with their counters (docs/spec-notes/misc-stats.md; "--"
  without the script runtime). Beside them the karma:
  "Karma: <alignment>", the picture, the karma title and the karma's number
  [hypothesis: alignments by the globals `KarmaVeryEvil`, `KarmaEvil`, `KarmaGood`,
  `KarmaVeryGood`; pictures `karma_evil`, `_bad`, `_neutral`, `_good`, `_saintly`].
  The karma title is the game setting `sKarmicTitle<Good|Neutral|Evil><level>` (levels
  1–30, 30 for 30 and over; levels below 10 spelled with two digits,
  `sKarmicTitleNeutral01`) [observed: `fnvtool esm inventory`: FalloutNV.esm holds 53 of
  the 90, levels 1, 2, 9, 11, 13, 15 and 20–30 in each column, 14 for Good and Evil, 28
  for Neutral and Evil, 8 for Evil; the DLC masters hold none]. The original shows a
  title at the other levels too, from text that is not in the data, which NVForever
  does not copy (LEGAL.md; #2983): a level without a setting takes the nearest level
  below with one in the same column, else the nearest above, and a load order with no
  title in the column shows the alignment word [design: `fnv_game::karma_titles`]. A
  `TTW.ini` `[Karma Titles]` entry comes first (ext-small.md). Good and Very Good take
  the good column, Evil and Very Evil the evil one [hypothesis]. The reputation list (the other mode) is the hook for
  the reputation workstream.
- **Keys** [design: `engine/nvf-play/src/pipboy.rs::keys`, `stats/mod.rs::keys`]:
  Up/Down choose CND, RAD, EFF; Left/Right step tabs on every view. A/D also step
  tabs except A on RAD, where the shown action owns the key even when unavailable.
  The audit of CND, EFF, S.P.E.C.I.A.L., Skills, Perks and both General modes found
  no other action using A/D or Left/Right. Up/Down and W/S select in a list, Page
  Up/Down scroll the description; the wheel scrolls the list, or the description when
  the pointer is on it; R switches General. The pointer selects the row it moves onto
  (not a row drawn under a pointer that has not moved) and clicks the buttons and arrows.
- **Checking it**: `NVF_PLAY_TEST_PIPBOY_STATS=<seconds>:<step>,...` with `sub=<0-2>`,
  `down`, `up`, `mode`, `desc=down|up`, `scroll=<n>` (the first row a list shows), and
  `perk=<FormID>` / `fame=<FormID>=<n>` to give the player something to show; each step and each selection is logged ("pipboy stats:
  Skills: Science 15 selected"), and a summary when a tab is drawn ("pipboy stats: LVL 1,
  HP 200/200, AP 80/80, XP 0/200; 7 S.P.E.C.I.A.L., 13 skills, …").
  [design: `tools/smoke/scenarios.toml`, `engine/nvf-play/src/test_aid.rs`]
  `pipboy-stats-rad-radaway` presses real keyboard A on RAD with RadAway carried,
  requires its use and forbids a General-tab log. `pipboy-stats-cnd-limb-pad`
  uses `NVF_PLAY_TEST_KEYS=24:pad-X,25:pad-X`: a synthetic controller's West button
  reaches `pad::menu_keys` and chooses the head, then torso on CND.
- **Open**: the game settings the page's words come from (`sStatsCrippled`,
  `sStatsNoEffects`, `sPCMenuHint*`) are not in the masters; when each face shows;
  whether the hints count 0 and when they are bright; the
  highlight box's rule; General's list height, order and right buffer, the reputation
  list's rule and second line, the karma titles. (The misc statistics' codes: misc-stats.md §1.)

### 5.1.1 STATS: S.P.E.C.I.A.L. from its file: what the code writes

The contract for the first slice of issue #1513 (`[UI] TileMenus=StatsMenu`,
`tiles/stats_menu.rs`): what code writes into the tree of `menus\main\stats_menu.xml` for page 1
to show. **Implemented, not verified**: every write below is our design and no finding about the
original, and no comparison with the original has closed it. Where runtime expressions place or show something, this note says only "the file works
this out at run time"; numbers appear only where they explain an incompatibility. Sources, as
tagged: the tree by `fnvtool ui tree` (values, tile names, template names); the file and its
includes by `fnvtool bsa extract`, read privately; and a private copy of those files laid over
Data with `--overlay` and read back with `fnvtool ui tree`, called "the overlay". What the overlay
shows is that authored overlay under **NVForever's evaluator**: it is no reading of the original
and does not check the evaluator. `NOGLOW_BRANCH` is empty.

**What the original was seen to do** [observed: private job pipboy_special_a, the original
running at 1280 x 720, the Pip-Boy open on this page; one character, one window size]:

- `screen()` is 1706.67 x 960, its converter 1.33 and the line thickness 2.67, which agrees at this
  one size with what the shared tree holds here (960 / window height). One size does not establish
  that rule in general; hud_menus' three sizes [observed: `tiles::resolution_converter`] are the HUD's.
- The list stands at x 35.33 and is 325 x 468. A row of 52 is derived from that height and nine
  visible rows [design: the arithmetic; one resolution], not read. The display stands at x 360.33, 544.67
  wide. The LVL card is at x 270, the XP card at x 690, and the tail line at y 652.
- The tabs stand at x 84.50, 220.50, 423, 591.50 and 746, y 626, 116, 179, 109, 107 and 133 wide,
  their parent line at y 652; their `_selected` read 0, 1, 0, 0, 0.
- The root's `user0` reads 0 on opening, then 1. A click on id 1 sets it by `ClickMenuButton`.
- The root's `user5` to `user7` read as text. `user8` reads empty on a level 1 character, while the
  LVL card shows on screen. **How `user8` came to be empty is open**. This slice's write of the
  model's level there is an explicit provisional choice behind the switch [design], so the card
  shows the level; it is not blanked to imitate that read. Parity on it stays open.
- A row's tile carries its AVIF name and id -1, read with `GetActiveUIComponentName` under the
  pointer. This slice gives its rows both, and knows them itself by actor value [design].

This is what was read, not yet compared with what this slice draws; other sizes are not covered.

Three different `user0`s are in play: the root's picks the page, the list's says a row is
marked, a row's shows its mark.

**The root** [observed: the vanilla files, by fnvtool bsa extract; the file's comments say
what each trait holds]:

| Trait | Code writes | From [design] |
|---|---|---|
| `StatsMenu/user0` | the tab, 0 to 4; here 1 | `PipBoy::tab` (`pipboy.rs`), the order of `Page::tabs` |
| `StatsMenu/user8` | the level, as text | `Snapshot::level` (`stats/model.rs`, filled in `stats/read.rs`) |
| `StatsMenu/user5` | HP, as text | `Snapshot::hp`, formatted as the hand-built page's card |
| `StatsMenu/user6` | AP, as text | `Snapshot::ap` |
| `StatsMenu/user7` | XP, as text | `Snapshot::xp` |

The numbering is the file's: the level is `user8`. The root's other code traits are Status's and
stay unwritten here. The root's `_PCButton_*` traits name buttons of Status and General, hidden on
this page, so they should not fire [hypothesis: menu-xml.md §3.2, the tile "must be shown and a
target"]. The page's keys stay code's: Up/Down, W/S, Page Up/Down in `stats::prepare`; A/D and
Left/Right in `pipboy::keys` [design].

**The list**, `stats_special_container`, a `list_box.xml` [observed: by fnvtool bsa extract: the
prefab's comments give `_highlight_y`, `_selected_height`, `_scroll_delta`, `_num_filtered` and
the scroll bar's `_number_of_items` to code, and say that with `_number_of_visible_items` set
code overwrites `height` and `_scroll_delta`; what a list longer than it shows gets: §5.1.2]:

| Trait | Code writes | From [design] |
|---|---|---|
| `height` | the file's visible rows times a row's measured height. **Required**: the tree reads it empty, and the list has `clipwindow` on | the rows' `height`, below. Not the hand-built list's height: `list_rect` multiplies by a caller's row height, the hand-built caller's being the font's size + 20 (`Ctx::line_height`), and a tile's text measures its tallest glyph (`tiles::draw::measure`), a different figure |
| `user0` | true when any row has a mark | any `Row::mark` |
| `_highlight_y`, `_selected_height` | the selected row's `y` and height while it is boxed, else -1 and 0 | `StatsView::selected[1]`, `StatsView::highlight` |

The scroll bar's `_number_of_items`, `_scroll_delta` and `_num_filtered` stay the file's: nothing
scrolls with seven rows of the file's visible count. That a list which fits keeps its bar hidden
is a [hypothesis]: what the original writes to these on this page is not known, and what a
container list does says nothing certain about it. A row count that does not fit (a replacement's
taller rows, fewer visible rows) scrolls as the other lists do (§5.1.2) [design].

**The rows**: template `stats_list_template` [observed: the vanilla files, by fnvtool ui tree,
`templates`], one per row under `stats_special_container`. Its hotrect `stats_list_template_item`
has the children `ListItemText`, `stats_list_template_number` and `stats_list_template_extra`
[observed: by fnvtool bsa extract]. Seven rows, `SPECIALS` in order: `Snapshot::special`
(`stats/read.rs`).

| Row trait | Code writes | From [design] |
|---|---|---|
| `listindex` | 0 to 6 | the row's place |
| `string` | the name | `Row::name` (AVIF FULL) |
| `user1` | the value as text, or the template's -1 for none | `Row::value` (`model::whole`). "--" without the scripts' game: a text value keeps it (`fnv_ui::Value::text`) though it compares as 0 (`Value::num`), so a numeric write would lose it |
| `user0`, `user2` | whether the mark shows, and the mark | `Row::mark` (`model::modified_mark`) |
| `height` | a height the template gives is kept; otherwise the text's measured height + the row's `_VerticalSpacing`, or 20 where that reads 0 [hypothesis: the prefab's comment; the derived 52 above agrees at one size] | the row's text as measured |
| `_y` | the height of the rows above | the same |

Everything else about a row, the list and the highlight box: the file works this out at run time.
The hand-built constants are not transplanted. The template gives a row no `id` and no `xup` or
`xdown` [observed: by fnvtool ui tree]; the running original's rows have the id and name above, and
how its code tells a row's click is not known. A tab's prefab has its
own hover sound and `pipboy/sounds`' `TabHovered` is ours: one must stay silent [design].

**The display**, `stats_display_rect`: place and size, the file works this out at run time.

| Tile and trait | Code writes | From [design] |
|---|---|---|
| `stats_icon/filename` | the picture | `Row::icon` (AVIF ICON) |
| `stats_icon/visible` | true while a row is selected. **Trap**: `stats_icon_separator` copies it and `stats_description_rect` copies that [observed: by fnvtool bsa extract], so a row without a picture must keep it true with an empty `filename`; the hand-built page draws line and text without a picture (`listed`) | `listed` in `stats/mod.rs` |
| `stats_description/string` | the text | `Row::description` (AVIF DESC) |
| `stats_description_scrollbar/_number_of_items` | one more than the scroll steps the tile's own text overflows by (font, wrap and window as the tile has them, not the hand-built page's pitch). **Required**: the prefab's value shows the bar on every row [observed: `visible` 1 in the tree and under the overlay]. The +1 is our adaptation, no capture of the original | a bound the tile view supplies through `StatsView::set_desc_bound`, as `listed` does its own |
| the same tile, `_current_value`, `_SetInCode` | `StatsView::desc`, then a poke of `_SetInCode`, which the prefab asks for when code sets the value | `StatsView::select` zeroes it on a new row |

`stats_icon_badge` stays hidden: what it shows on this page is not known; capture. The
description's value has one owner: the file adds its own arrow clicks and `wheelmoved` to
`_current_value`, and the model moves it too. So tile controls become `StatsView::scroll_desc`
commands, the value is written back after input on every tiled frame, and `wheelmoved` is not
written [design].

**A tab's inner rect**, `stb_text_box`, repeats the tab's own place, the tab's being read from it,
and has `alpha` 0 over words and lines of their own `alpha` [observed: by fnvtool ui tree; a tab has
no `locus`, the rect has]. So the two places are not added and the rect's `alpha` is not passed down
in the original, going by where its tabs stand [hypothesis]. `tiles/layout.rs` and the tile input
read no `locus` and multiply a rect's `alpha`, for every menu, so unchanged they lost the tab line
here, words and box. Writing either place collapses the other, standing every tab at the line's
start [observed: fnvtool's tree with a test write]. So this slice alone writes marks, the file's
places untouched: the tab gives its children no place (`_NVFNoOrigin`), and what is under the rect
keeps its own `alpha` (`_NVFOwnAlpha`) (`write_tabs`) [design]. Whether to read `locus` in general is
a separate decision. The selected tab's fill stays undrawn: a hotrect, whose `alpha` the file
changes there, draws nothing in this renderer [design].

**Ids** [observed: the vanilla files, by fnvtool ui tree, `id`]: `stats_tailline_status` 0,
`stats_tailline_SPECIAL` 1, `stats_tailline_skills` 2, `stats_tailline_perks` 3,
`stats_tailline_general` 4. Each tab's `_selected` asks whether the root's `user0` equals the same
number, and nothing in the file writes `user0`: a click on id 1 setting it was seen, above; the same
for the other ids is a [hypothesis]. The hand-built `PipBoyInput::Tab(i)` uses the same numbers (`tab_line` in
`frame.rs`). Also `stats_special_container` 6, `stats_icon` 25, `stats_description_rect` 26,
`stats_icon_badge` 52, `stats_title` 39, the cards 40 to 43; the scroll pieces are all `&generic;`.

**The title**: the `string` of `stats_title` is an entity without its closing `;`, which the
reader keeps as plain text (menu-xml.md §2.4) [observed: the vanilla files, by fnvtool ui tree].
The tile would show those characters, and neither the masters nor `BUILT_IN` have a setting of that
name. Code replaces them with `Page::title`, the hand-built page's own word, only while the string
still is such an unresolved name, and leaves a replacement's or a script's text alone [design: the
string rule, LEGAL.md: a word NVForever writes, none taken from the original's screen].
What the original does with it: not known; capture.

**What the page needs of `screen()`**, the question menu-xml.md §4.2 leaves open for the Pip-Boy.
Neither `stats_menu.xml` nor any file it includes reads `screen()`; only `globals.xml` does, for
`_line_thickness`: 2 on a PC, times `resolutionconverter`. The canvas comes from that file's
`_pipboy_width` and `_pipboy_height` [observed: by fnvtool bsa extract, every include searched].
With `--screen` at five sizes none of the tree's traits changes [observed: by fnvtool ui tree], but
the tool writes no converter, so that run cannot say which converter is right: it reads thickness
0 and lines without height. The renderer's target stays its own matter, and a 1024 x 768 canvas is
not written into `screen()`, because other menus share the tree [design].

- `tiles::interface_screen` is true with `StatsMenu` [design]: the file's lines need a converter,
  and the same gate lets a hotrect's `alpha` 0 pass (`Context` in `tiles/layout.rs`), so the list's
  rows draw. The gate is global, not Stats's own. With only the tile HUD on, adding `StatsMenu`
  changes the shared screen traits and the HUD's hotrect-alpha handling, as the other T15 menus do;
  with another T15 switch already on, adding it changes no gate. This effect is tested and stated,
  not isolated.
- 960 / window height (`resolution_converter`) is what the shared tree holds, and the original
  reads the same with the Pip-Boy open, above. So lines differ from the hand-built `THICK` of 2
  behind the switch.

**Hand-built value, tile target, what must stay** [design: values come from `Snapshot` and
`StatsView`; a writer works no figure out again]:

| Hand-built | Tile target | Stays? |
|---|---|---|
| A value with worn apparel's changes, kept in range (`shown_and_permanent`, `av::capped`) | row `user1` | yes (§5.1) |
| "(+)" and "(-)" (`modified_mark`) | row `user0`, `user2`; list `user0` | yes |
| A row always selected, boxed only after a key or under the pointer (`StatsView::highlight`, fitted to the owner's screenshots) | `_highlight_y`, `_selected_height`; the display always filled | yes |
| Screen menu: no title, the line running on from the lead line, the page names as a header [design: the owner, 2026-10-31, `heading_title` and `header` in `frame.rs`] | hide `stats_title`; write `x` and `_length` of `stats_headline_h2` to close the gap; the header stays the shell's, clickable, no tile | yes `[owner 2026-10-31]` |
| The page's background and scanlines, on the device; neither in the screen menu (`device_screen.rs`, screen-menu.md) | none: `background_layout` stays hidden in both frames, so the tiles are page content only; the file has no scanline tile, so they stay the shell's, with the scanlines over the tiles | yes |
| Keys, wheel; A/D [design: `pipboy::keys`]; the controller's D-pad [design: owner 2026-11-02] | none: `stats`, `pipboy.rs` | yes `[owner 2026-11-02]` |
| The words "S.P.E.C.I.A.L." and "STATS" (`Page::tabs`, `Page::title`) | the tab reads `sStatsSpecial` (`fnv_ui::strings::BUILT_IN`'s "S.P.E.C.I.A.L." unless a plugin sets it, the name as FalloutNV.esm's own text spells it), left alone: code does not overwrite a plugin's or the built-in label | yes `[owner 2026-11-06, #1740]`: both pages read "S.P.E.C.I.A.L." |
| Tabs, cards, row pitch (`tab_boxes`, `heading`, `card`, `list`) | the file works these out at run time | not a choice; may differ behind the switch. The places read above are the original's; the comparison at 1280 x 720 lists the differences |
| The log lines and `NVF_PLAY_TEST_PIPBOY_STATS` | none | yes: the smoke scenarios read them; `stats::prepare` keeps them whichever view draws |

**Not known; capture**: how `user8` came to be empty, and where the LVL card's value then comes
from; a row's `_selected`; the list's `_scroll_delta` and scroll bar count; `stats_icon_badge`; the
description scroll bar's count; whether the other tabs' ids act as id 1's does; whether a tab's
words and the title show at all before code writes them; any of the above at other window sizes.
None of these stops a first working slice with a complete fallback; they cannot be called verified
when parity is declared.

### 5.1.2 STATS: the other list tabs from their file: what the code writes

The contract for the second part of issue #1513, under the same switch and the same module: tabs
2 Skills, 3 Perks and 4 General (`TILED_TABS`, `tiles/stats_menu.rs`). Every write below is our
design; what the original reads is tagged, from two private runs of it at 1280 x 720, one character
(`run07_stats_tabs`, `run07b_stats`, "the runs" here). What §5.1.1 says of the root, a row, the
display and a tab's inner rect holds here too; the root's `user0` is the tab.

A tab is found alone (`paths(ui, tab, cut)`): a file short of one tab's list leaves that tab
hand-built, with its reason once, and the others tiled. Going to another list unloads the rows of
the last, as a new tree would; they are known by what they stand for (`model::RowKey`, filled in
`stats/read.rs`), never by their words. The rows stand in the model's order.

**A list longer than it shows** [design]: the model's scroll stays `StatsView::top`, whole rows,
and the view gives it its count of rows (`StatsView::set_rows_shown`), so the keys, the wheel and
the selection's keeping in view work out for the file's list. A list with
`_number_of_visible_items` shows that many, its height written as §5.1.1; one without keeps the
height the file gives and shows the whole rows that fit, ten of 52 in General's 542 [observed: the
runs, 0 visible items, 500 x 542]. The original's counts agree with `list::counts` on three lists
[observed: the runs: Skills 13 rows 5 items and a step of 52, Perks 14 rows 6 and 52, General 43
rows 34 and 51.33]. A list that has never scrolled keeps the file's counts, as §5.1.1; once it does,
it has the file's `_scroll_delta` back when its rows fit again.

The list bar's up arrow, down arrow and marker use `layout::OWN_ALPHA`, so each picture
keeps its own alpha under the bar's zero alpha [design: `tiles/stats_menu.rs::page`,
`list_bar_pictures`; the same mark as `tiles/container_menu.rs::mark_tiles`]. The bar's
`visible` still hides its pictures when the list fits. The scroll log reports which of the
three sized pictures are in the final paint set; this checks pieces, while shots check pixels
[design: `tiles/stats_menu.rs::page`; test `list_bar_pictures_keep_their_alpha_and_hide_when_the_rows_fit`,
using a fixture written for the test, covers Skills and General].

| Tile and trait | Code writes | From [design] |
|---|---|---|
| list `height` | the visible rows times the tallest row where the file counts them; otherwise left as the file gives it | `_number_of_visible_items`, else the file's `height`, else `stats::VISIBLE` |
| list `_scroll_delta` | what is left over by the positions after the first, when the rows do not all show | `list::counts`; so every row of such a list must have one height. Rows of different heights (a wrapped name) keep the hand-built page, with that reason |
| `lb_scrollbar/_number_of_items`, `_number_of_visible_items` | rows less those whole and 1, and 1 | the same [observed: the runs, the bar's visible items always 1] |
| `lb_scrollbar/_current_value`, `_SetInCode` | `StatsView::top[tab]`, on every pass after the input, then a poke | as the description's bar, §5.1.1 [observed: the runs, the value is the top row's index] |
| list `_highlight_y`, `_selected_height` | the selected row's place after the scroll and its height while boxed; -1 and 0 on a row out of sight, the selection staying | `StatsView::selected[tab]`, `highlight` [observed: the runs: a row's own `_selected` stays 0, and nothing is boxed at opening] |

The file moves the rows by the bar's value and `_scroll_delta` [observed: the runs; documented:
`list_box_template.xml`]; nothing here moves them again. Only rows in sight are asked to fit and to
take the pointer. The bar's pieces are `StatsTile::ListBar` and become `StatsView::scroll`, a page
by the file's `_jump_size`, the marker by its `_step_height`; the wheel stays `stats::prepare`'s. A
row that comes under a still pointer as the list moves takes the selection, as the original's does
[observed: the runs]; a row drawn under one when the page comes up does not. That is the tile page's
rule: the hand-built page keeps its own. Only a row in sight selects under the pointer: General's window
shows a strip of the row after its last whole one, where selecting would scroll it in and bring the next
under the pointer, down the list [design]; a click there still selects. The marker's height and step are the file's, logged; the
original's general marker reads 32 with a step of 14.48 [observed: the runs].

**Skills**: `stats_skills_container`, rows as §5.1.1, thirteen in the model's name order.
`stats_icon_badge` shows on the five weapon skills by `items::skill_badge` [observed: the runs:
Energy Weapons, Explosives, Guns, Melee Weapons, Unarmed]; only its `visible` counts, its name is
left in place on a row with none, as the original's goes stale.

**Perks**: `stats_perks_container`; a rank of 2 and over is in the row's `string`, "<name> (<rank>)",
`user1` -1 [observed: the runs, three perks at `(2)`, one rank no suffix; hypothesis: none at a rank of
1 of a perk with more]. That list has no `user0` of its own in the tree, and none is written. A player
with no perks has a tile page with no rows and an empty display, not a fallback [design]: the
original's empty list was not seen (its character had traits, which are rows here too [observed:
the runs]). A row's tile keeps the name the others' do; the original's reads the template's with an
index there [observed: the runs]. Scripts reading it would see the difference.

**General**, `stats_genrep_container`: both lists hold their rows in both modes, the file showing
one and its side from the trait below [observed: the runs]. No display: its picture and text are
left empty.

| Tile and trait | Code writes | From [design] |
|---|---|---|
| `stats_genrep_container/user0` | 1 for the reputations, 0 for the statistics [observed: the runs, 1 on the first entry; it holds after a tab change and a reopening, not tested over a load] | `StatsView::reputation`, a resource kept for the session |
| `stats_general_container`, `stats_reputation_container` | each its rows from the shared template, kept across the switch | `Snapshot::general` (43, the code's place as key), `Snapshot::rows(4, true)` |
| `stats_karma_alignment/string`, `visible` | the alignment's word; no label of the hand-built page's | `model::karma_alignment`, "--" without the scripts' game |
| `stats_karma_image/filename`, `visible` | the karma's picture [observed: the runs, `karma_neutral`, `karma_good`] | `stats::karma_picture` |
| `stats_karma_title/string`, `visible` | the title **where a setting holds it**, else empty: `KARMA_TITLES` is no data file's and is not written behind the switch | `Snapshot::karma_setting_title`, "--" without the scripts' game |
| `stats_reputation_image/filename`, `stats_reputation_title/string`, `stats_reputation_rep_title/string`, each `visible` | the picture, title and name of a row pointed at; nothing while none is [observed: the runs, unlike the other lists' display; hypothesis: and none after the pointer leaves] | `Standing`, `StatsView::highlight` |
| `stats_genrep_button/_PCButtonText` | the switch's key: `sPCMenuHintR`, with a controller the button's name, as Take All's | `pad::game_hint` |
| `stats_genrep_button/button_text/string` | with the reputations up the setting `sStatsGeneral`, a word of a replacement's own standing; with the statistics NVForever's "Reputation" | below |

**The switch's words.** The file's use of `sStatsGeneral` on this button lacks the closing `;`
(menu-xml.md §2.4), so the reader keeps the characters as plain text where every closed name is
read from the settings at load; left alone they showed on the page. The other word is in no file
and has no setting [observed: FalloutNV.esm's GMST list; the original's code writes it, and it is
wider], so it is NVForever's own, the list's name, the hand-built page's since before any shot of
this page (§5.1). The button is never its key alone [observed: the runs]. A click is
`StatsTile::Switch` and `StatsView::switch`; R stays `stats::keys`, and R, E, X and the controller's
A, Y and X join `HAND_BUILT_KEYS`, since the root binds those keys [documented: its `_PCButton_*`] and
they would act twice.

Left on the hand-built page and not carried over: "Karma: " before the word, the karma's number (no
tile in the file holds it), the button's alternating names, the line of an empty list and
"Fame n, Infamy n" (§5.1).

**Words written** on these tabs, by source: record data (AVIF, PERK and REPU FULL, DESC, ICON); the
settings `sKarmicTitle…`, `sStatsGeneral`, `sPCMenuHintR` or the pad's own names; NVForever's own
"Reputation", the "(n)" of a rank; figures, and the "--" of §5.1.1. From the model and no data file,
as on the hand-built page: the 43 statistics' names (`misc_stats::NAMES`) [documented: GECK wiki
GetPCMiscStat, a script function's argument], reputation titles (`reputation::TITLES`) [documented:
GECK wiki GetReputationThreshold, and the Fallout wiki's table], the alignment's words spelled from
the globals' Editor IDs (`model::karma_alignment`) [hypothesis], "(+)" and "(-)"
(`model::modified_mark`, §5.1.1). They stay until the maintainer settles the words or a setting is
found for them.

### 5.1.3 STATS: Status from its file: what the code writes

Tab 0, the last of #1513, as §5.1.2. Which sub-page: `stats_status_container/user0`, 0 CND, 1 RAD,
2 EFF, 3 H2O, 4 FOD, 5 SLP; each button's `_selected` and each page's tiles follow it by the file
[observed: the runs]. The display is left empty. Also on the root: `user1` the meter's value, the
rads [observed: the runs, 0, then 450; hypothesis: a need's on its own page], `user2` 1000, the maximum;
`user3`, `user4`, `user9`, `user10`, `_meter_height` stay the file's.

| Tile and trait | Code writes | From [design] |
|---|---|---|
| `stats_H20_button`, `stats_FOD_button`, `stats_SLP_button` `visible` | whether Hardcore is on: the file leaves it to code; `_enabled` stays 1 [observed: the runs] | `Snapshot::hardcore`, `StatsView::set_status_pages` |
| `stats_player_<part>_pct_meter/_Value` | the limb's condition as a fraction [observed: the runs, 1, 0.40, 0] | `Snapshot::limbs`. The file shows the bar or the `_crippled` word from it [documented: the file; observed: the runs] |
| `stats_player_<part>/filename` | the limb's picture, "_broken" at 0 [observed: the runs, `left_leg_broken.dds`, `head.dds`] | `stats::boy_picture` |
| `stats_player_face/filename` | `face_00.dds` always [observed: at 1 and 0.40; when others show is not known] | `stats::BOY_FACE` |
| `stats_player_<part>_pct/target`, `_crippled/target` | true: the limb healing click on CND (Stimpak outside Hardcore [owner 2026-10-23], Doctor's Bag in Hardcore [#3431]); the file gives only the picture one, while the meter is under 1 | a click is `StatsTile::Limb`, `UseHint`, and so is one on the picture or, over the head, the face [observed: the runs, id 24] |
| `stats_player_name/string` | "<name> - <`sStatsLVLAbbrev`> <level>": the hand-built page's "Level" is off the original's screen, so the setting's word | `Snapshot` |
| hint `button_text/string` | a count in parentheses when some are carried, the item's name from its record | `model::hint_text`, `Hints` |
| hint `_PCButtonText` | the key: `sPCMenuHintS`, `E`, `A`, `X`, with a controller the button's | `pad::game_hint` |
| hint `target` | `Hints::lit` [owner 2026-10-27]: the file's alpha follows it, and a dim hint lets the pointer through, so behind the switch it cannot be clicked to be told why not; `brightness` is no signal [observed: the runs] | the hand-built rule of when; the original lights RadAway and Rad-X too, which are not used from here |
| hint `visible` | CND's two on CND, RAD's two on RAD, none else [observed: the runs] | `StatusShown` |
| `stats_rad_resist_level/string`, `stats_player_rads/string` | "<n>%"; the rads, empty at 0 [observed: the runs] | `Snapshot` |
| `stats_rads_mid/string`, `stats_rads_max/string` | 500 and 1000 [observed: the runs' shot] | `stats::RAD_MAX` |
| `stats_player_h20`, `_hunger`, `_sleep` `/string`; each `_mid`, `_max` | the need's figure, the same scale [observed: the runs, RAD's numbers on all three pages] | `Snapshot::needs` |
| `stats_effects_container` rows from `stats_effects_template` | `listindex`, `stats_effect_name/string`, `stats_effect_string/string`, `user2` true but on the last row (its divider [documented: the file]), in name order | `Snapshot::effects`; the hand-built page keeps the model's order |
| `stats_effects_container/height` | the rows' total, for the file to centre [observed: the runs, 168 at 3 rows], up to what its area holds; more are cut, not scrolled, as on the hand-built page | the rows' heights |
| `stats_no_effects/visible` | true on EFF with no rows [observed: the runs] | words the file's |

The arrow's place is the file's from the root's two values [observed: the runs: the zero-to-full span
over `user2`]. `stats_healing_mode` is the file's, a controller's only [documented: the file]; hidden, it
keeps a box's height over the bag's hint [observed: the runs, the bag's y 114], so with no words of its own
its text holds a space to be measured, or `sSwitchLimbs` when present so it displays and cycles limbs
(`StatsTile::SwitchLimbs`) when shown with a controller [design: #3431].

**Pictures' sizes**, on every tab: `filewidth` and `fileheight` of each picture under the menu, its file's
texels times its zoom, written for the file it shows (`write_file_sizes`), as the file works from them
[documented: its comment; observed: the runs, the RAD meter's x 189.4 = 855 - 512 x 1.3, the karma's picture
384 = 256 x 1.5]. Unwritten they read nothing: the meters stood off the canvas, and the limbs' brackets, the
karma's and the reputation's pictures had no size to draw or be clicked in. A picture of an atlas has
none [design].

**The six buttons** are the tabs' prefab, so they get `write_tabs`' marks (`mark_inner_rects`): unmarked each
inner rect stands twice as far down, its words hidden and over the next button.
Input: `StatsTile::Sub`, `Hint`, `SwitchLimbs`, `Limb`, and hover gives `StatsView::hover` for `aimed()`
(an unmoved pointer on a limb does not reclaim a limb chosen by keyboard/controller, matching the hand-built
page's `keep_limb`).

**Selection box and dim hints** [design: #3431]: when a limb is hovered or chosen on CND (`StatsView::aimed`),
the tile page frames that limb's bar (`_pct`) or `"Crippled"` word (`_crippled`) with a four-edge `Look::Box`
appended to `pieces` after `usable` (no extra tile, zero pieces added when no limb is aimed). **Lost behind
the switch**, for the maintainer to accept: a dim hint gives no click, so no refusal's sound or log line from it.

**Left out**: the rad effects' row (`stats_rad_effects_template`, `stats_rad_effects_bracket`), a label
and a column per effect at high rads and on the needs' pages [observed: the runs]: the hand-built page has
none and the model no such list. The left bracket on the needs' pages has no label or figure
[observed: the runs]; nothing is written there. Words the hand-built page's not carried over:
"Crippled", "RAD RESIST", "No Effects", "RADS" (the file's by setting).

**Words written** on Status, by source: record data (the items' FULL, the effects' names); the
settings `sPCMenuHint*`, `sStatsLVLAbbrev`; `model::changes_text` of AVIF short names; figures and
NVForever's punctuation.

**Not known; capture**, for these tabs: a lit Stimpak's first HP; a rank of 1 of a perk with more; an
empty Perks list; a limb's click and a hint's click in the original, what they use and ask; whether
General's mode survives a load; effects overflowing the page; the switch's and hints' words
(by the string rule, not read).

### 5.2.1 ITEMS from its file: what the code writes

The ITEMS page drawn from `menus\main\inventory_menu.xml`, in the Pip-Boy's image, behind
`[UI] TileMenus=InventoryMenu`, on by default when the list is absent [design, #2215;
ui-tile-tree.md section 6.2] (`tiles/inventory_menu.rs`; issues #1513, #1468).
Off, nothing of it runs. The rules stay `menu::model`'s and `pipboy::items`'; the module writes
values and says what a tile stands for [design]. Tile names and where they stand are
[documented: the file, by `fnvtool ui tree`]. "The capture" below is the original at 1280x720 in the
private jobs run07_inventory, run07_inventory_pipboy, run07_repair_menu, run07c_inventory and
run07c_inventory_b [observed].

**The one decision**: `InventoryTiles::ready`, as `StatsTiles::ready` (§5.1.1): the switch on,
ITEMS showing, the repair list and the mod menu closed, the file's tiles there, the tabs made and
a usable measured layout. A failure leaves the whole hand-built page, says why once, and is tried
again the next frame. While the Pip-Boy goes down the page stays as last written, the model being
gone [design].

**Usable** differs from §5.1.1 in two ways: a scrolled list stands partly outside its window by
design, so the check asks of the rows whose words lie whole on the canvas and inside the list's
window, and of at least one when there are rows; an empty tab is a page. A list that scrolls must
have a window. Every tab's words must show whole. The rows asked of must take the pointer at their
middles, by one walk of the menu however many rows there are. So must the tabs but the one showing,
but one that does not is warned of and the page stays, the keys and the device's knob turning the
tabs [design]. A reason is warned of on the third final pass in a row that fails, once by its kind
whatever the place, and a model not there yet with nothing written is no failure. From then on the page
is tried again when what it would show changes, not every frame [design].

**The tabs' places**: a tab's `x` follows its `stb_text_box`'s, which follows `_x`, and `tabline.xml`
says the tabs are added by code [documented: the prefabs]; unwritten, all five stood at one place,
which is what the first run met [observed: smoke 2026-11-07, `Tab(1) does not take the pointer`]. The
original's: a line of `_LeftLineLength`, a tab, a line, a tab, and the rest for the last line, with the
length floor((line - widths) / 6) [observed: the capture; hypothesis: the floor, from one set of words,
54 in 855]. Code writes `_x` and `_LeftLineLength` on every final pass where they differ; `_selected`
and the box follow `_CurrentTab` in the file, and a tab's word on its own `string` is copied down
[documented]. The log line `tiles: InventoryMenu tabs stand:` says where they are.

| Model | Tile, trait | Notes |
|---|---|---|
| The tab (`PipBoy::tab`) | `IM_Tabline`: `_ButtonCount` 5, `_CurrentTab`, `_LeftLineLength`; five tiles from `TabButtonTemplate` under it, each `listindex`, `string`, `_x` | Template rows, unlike STATS' fixed tabs [documented]. A tab that does not show is a fallback. As STATS' tabs, a tab with an `stb_text_box` gives its children no place [hypothesis]. |
| Wg, HP, DT, Caps (`items::hp_text`, `dt_text`) | `IM_Headline_Player{WG,HP,DT,Caps}Info/_Value` | The Wg value cuts whole units off where the hand-built page rounds [observed: the capture, one case]. The titles are the file's but where noted below. `IM_Headline_PlayerDRInfo` stays hidden, the same look as the original's `alpha` 0. |
| Title, heading line | `IM_Headline_Title` hidden in the screen menu, `IM_Headline_h2` run on [design: the owner, 2026-10-31]; both as the file on the device | The title's words are the file's, never written. |
| The rows (`MenuModel`'s player list) | `IM_InventoryListTemplate` rows under `IM_InventoryList`, by `item_menu`: `ListItemText/string` (`look::row_text`), `IM_Template_ItemMarker/visible` (equipped), `_y`, `height`, `_scroll_delta`, the bar's counts, `_highlight_y`, `_selected_height` | As the container's, one side. Folder rows and "Back" are rows like any other. `IM_EquipItemMarker`, the hollow square by the highlighted row, is the file's, from `_EquippableItem` [observed: the capture]. |
| The selected row's picture, badge, skill | `IM_ItemIcon/filename,visible,x,width,height`; `IM_ItemIconBadge/filename,visible`; `IM_StrReq/string,visible` (the number only) | Hidden with the panel. Ammunition's picture stands at x 410, 350 square, against 450 and 380 [observed: the capture, on ammunition no carried weapon takes; hypothesis: on all]. |
| The figures (`items::info_of`, no DPS on this page) | `item_card::write` under `IM_ItemInfoRect`, whose `visible` follows having a row [observed: the capture] | The same cards as the container's; `user6` 1 on weapons, 0 on armour; effects on the first free row, one row up on aid, at y 110 under two rows [observed]. `ModInfoOne` to `Three` unwritten (not in `Info`). |
| The buttons | `visible`: Repair on every tab but inside a Misc folder, `target` while R does something (a spare and something to repair), the file dimming it; Mod on the Weapons tab, `target` with a row; Cancel inside a Misc folder, in Repair's place, sending `Back` there, E or a click; Equip, Drop and Hotkey hidden with keyboard/mouse [observed: the capture]. Hotkey shows with Controller and names RB/R1, its target following `hotkeys::can_hold` [design, #2312] | The words are the file's, but Repair's follows the condition, the longer word can be lit: `sMaintainItem`'s text where the load order defines it, else "Upkeep", by `fnv_game::repair::hint`'s rule, on the button's `string`, the file's coming back. With no row it holds on Weapons and Apparel, and is the file's on the others [observed: the capture]. |
| Root | `_EquippableItem` (a weapon, armour or aid row, not broken [observed: 0 at condition 0]), `_KeyringOpen` (a Misc folder open) | Armour and aid [hypothesis]. |
| The hotkey panel (`HotkeyHold`, `Hotkeys`) | Nothing of the file's: `hotkey_panel::draw_over_tiles` draws the owner's panel over the page while a key is held or the controller modal is open [design, #2312], and `IM_ButtonRect` and the picture, badge and skill give way. `IM_HotKeyWheel` stays hidden | [the owner 2026-10-26, and 2026-10-27 for its place]. **A difference**: the capture never saw the file's wheel open, so it is not built here. |
| `IM_Background` | hidden in both frames | The shell's background and scanlines, as §5.1.1. |
| The prompt | nothing in the image: `QuantityMenu` on the window (`menus::quantity_tiled`) | As the container's. |
| The repair list, the mod menu | not eligible: the hand-built body draws them | Their own files are other work. |

**Every string written, and where it comes from** [design: LEGAL.md]:

| Written | Source |
|---|---|
| The tabs: `sInventoryWeapons` … `sInventoryAmmo` where the load order defines them (§4, through `pipboy::labels` as the hand-built page), else "Weapons", "Apparel", "Aid", "Misc", "Ammo" (`TAB_WORDS`) | "Apparel" is the data's word [observed: FalloutNV.esm MESG `HelpApparel`, repair.md §5; #1740]. The other four cite no record: generic words for the kinds of item, chosen as NVForever's own. The English masters' string settings hold none of the five [observed]; a localized install's language plugin defines them (§4). All five fallbacks are the same as `Page::tabs`', tagged as read off the owner's screenshots; that list is not read, and the likeness is for the maintainer's eye. |
| Headline and card titles where the label leaves no room: "Wt" (`IM_Headline_PlayerWGInfo`), "Str" (`StrengthReqInfo`), `sStatsCNDAbbrev` (`ConditionInfo`) | NVForever's own short words, and the strings table's, only while the title is still a built-in label: a plugin's setting stands (`Ui::built_in_label`). |
| Repair's word on an item that needs no repair: `sMaintainItem`'s text where the load order defines it (§4, as the hand-built hint), else "Upkeep" | "Upkeep" is NVForever's own: the English masters define no `sMaintainItem` (§4), and the data's help text has only "maintained". The hand-built hint keeps its "Maintain" (`fnv_game::repair::hint`) [observed, repair.md §5]. |
| Row names | Record data (`look::row_text`, the count in brackets). |
| Cards' values, the headline's values, `IM_StrReq` | Figures from the model and record data; "--" for what is unread, as the hand-built page. |
| `AmmoInfo`'s title | Record data, or armour's class by the model's word (`item_card`, `info_of`), as the container's. |
| The panel's line | An item's name, or the panel's own words "Empty", "Ammo Swap", "Key 2 is Ammo Swap", "Can't hotkey ..." (`hotkey_panel`), drawn by that module. |
| `_PCButtonText` | Repair `sPCMenuHintR`, Mod `sPCMenuHintX`, Cancel `sPCMenuHintE` (the strings table or the load order's); with a controller the button's name, and none on Cancel: B closes the Pip-Boy, as on the hand-built page. Equip and Drop: none. Hotkey: the current pad's RB/R1 with Controller, hidden with keyboard/mouse [design, #2312; `inventory_menu::hints_of`]. |

**Input** (`inventory_menu::feed`): the pointer is `PipBoyScreen::pointer`, read and never written;
nothing else here touches the hand-built pointer [design]. It takes nothing under the prompt, with
a menu over the page, or with another tile menu taking the frame. The commands are the hand-built
page's:

| What | Sends |
|---|---|
| The pointer moves onto a row, the wheel not holding the selection | `Hover(Player, i)`, behind a guard |
| A click on a row | `Select(Player, i)`, behind a guard: on the selected row the model acts, on another it only selects, as `items::on_row_click`. Not `item_menu::row_commands`, whose click acts at once on any row: the container's |
| A click on a row with a hotkey's key held | `hotkey_panel::assign_row` [owner 2026-10-26] |
| E inside a Misc folder | The file's Cancel: `Back`. Outside one E does nothing, as on the hand-built page |
| A right click on a row, no key held | `Hover` then `Drop`, with Shift `DropStack` [owner 2026-11-02, #1393] |
| A tab | `PipBoyInput::Tab(i)`; the pointer onto it, `TabHovered(i)` |
| Cancel | `Back` inside a folder; ignored outside one (the button is hidden and does not close the Pip-Boy, #3915) [design] |
| Equip, Drop (a script's click: not shown) | `One`, `Drop` |
| Repair, Mod | `RepairInput::Open`, `ModInput::Open` |
| The bar's arrows, page areas, marker | The list's view, no command of the model's |

The keys and the wheel stay `items::keys`', `repair::keys`', `item_mod::keys`' and `pipboy::keys`'
(`HAND_BUILT_KEYS`): a file's binding acts for a script's tap, not for a physical press, and never
for a controller's button, which only a real press can make (the X drop would act twice).

**Measuring**: `NVF_PLAY_TEST_TILE_COUNTS=1` adds a line a frame for each pass while the page is up,
`tile counts: InventoryMenu pass=<early|late> writes= changed= micros=`, the late one with `rows=
rows_in_clip= hit_tests= candidates= pieces=`.

**Unit coverage given up** (#1512): the test fixture is written for the tests. It cannot prove how
the file's expressions read what is written: the tab template's inside, `_CurrentTab`,
`_LeftLineLength`, a tab's `_selected`, `_EquippableItem` and the marker that follows it,
`_KeyringOpen`, a button's `target` and its dim look, the headline's `_Value`s, or the file's own places. The smoke scenarios
`tiles-items*` check structure and hits on the real file; their logs cannot prove pixels, only their
shots can.

**The hints' room**: the screen menu's image shows the canvas to x 955 (`pipboy::SCREEN_CROP`) and the
device's screen about the same [observed: smoke `tiles-items`, 2026-11-07, cut there in both]. No tile clips them. The
original's hints end near 934 [observed: the capture]; a longer one is cut at 955.

**Known differences behind the switch** [design]: the keyring and the Caravan cards are `Folder`s with a
"Back" row at the top of Misc, the model's; the original's keyring row stands last, has no Back row, and
Cancel takes you out, which is a change for the model, not this view. The hot key wheel is the owner's
panel. Ammunition's dim rows are not built [observed: only pictures].

**Not known**: the file's hot key wheel; a larger stack's drop prompt in the original; `_LeftLineLength`
with other words. None of these stops a working slice with a complete fallback; they cannot be called
verified when parity is declared.

### 5.3 DATA

`data.rs` (the page: state, input, what is shown and its drawing), `data/model.rs` (plain
rules, tested), `data/read.rs` (records). Sources: `menus\main\map_menu.xml` (places, sizes,
fonts) and the owner's five DATA screenshots (§1), compared at 1280 × 720 with
`NVF_PLAY_TEST_SHOTS` runs; px below are those screenshots' pixels, about 0.62 to a unit.

- **Heading** [documented: map_menu.xml]: the title 70 in after a line of 50; a line from
  180, 470 long; the place right-aligned at 630 and the date and time at 845, both 20 down
  in font 2; lines fading down at 650 and at the end, a line from 660 to the end.
  - The date: month, day, the year's last two digits, then the time, "10.25.81, 9:29"
    [observed: the screenshots, on 25 October 2281]; no leading zeros and a 24-hour clock
    as the wait menu's [hypothesis, wait-menu.md §4].
  - The place: indoors the cell's name [hypothesis]; outdoors the map name (`REGN RDMP`)
    of the regions the player's cell is in (`CELL XCLR`), the highest priority first, else
    the worldspace's name [hypothesis: the screenshots show "Goodsprings", the map name
    of `VMapGoodspringsRegion` (observed)].
  - Right-aligned texts of the original end about 5 px further right than ours (the time,
    "Show Location", "Place Marker") [observed]; not corrected (for the hints: before
    they became a column ending at 855 on 2026-11-09).
- **Lists** (Quests, Misc, Radio) [documented: map_menu.xml, list_box.xml]: 15 left of the
  menus, 75 down, 400 × 468, the scroll bar of 32 on the left; a row's square of 25 at 15,
  20 and its text 40 in and 20 down in font 2, broken at 310. A row is 54 high and a text's
  further lines 34 lower [observed: fitted; rows 33.5 px apart, two-line rows 54.5 px];
  the squares start at the same pixel as the original's [observed]. The selection box
  starts 9 left of the row and ends 5 short of the list's edge [observed: the Radio
  screenshot boxes from 448 px; documented: the 5]. The box shows while the pointer is
  on a row or after Up/Down (W/S); a new tab starts without it [design: the screenshots
  show it only under the pointer]. Hover selects, a click or Enter acts. The wheel scrolls
  the list. The scroll bar: the file's arrows (32 × 16) at the ends and a line for the part
  shown [hypothesis: the marker's look].
- **Hints at the right** (`MM_ButtonRect`, 2026-11-09, #1897; `data.rs::tab_buttons`,
  `button_boxes`). The file has three hint tiles, A (`MM_ButtonA`), X (`MM_ButtonX`) and Y
  (`MM_ButtonY`), right-justified, the first at the lists' top (75), each under the one
  above only where that one shows [documented: map_menu.xml `MM_ButtonA`/`MM_ButtonX`/
  `MM_ButtonY`]. When each shows, by the file:
  - **A**, with a controller only, never with the keyboard and mouse: the World Map
    always; Quests when the player has a quest; Misc when the selected note is an audio
    note; Radio when at least one station is known. Its title by tab: `sTravel`,
    `sMakeActiveQuest`, `sPlayAudio` (`sStopAudio` while the selected audio note plays),
    `sTune` [documented].
  - **X**: on both maps (`sPlaceMarker`): the file's title for it names a controller
    [documented], but with the keyboard and mouse the original shows it too, the word
    alone at the top right with no key, a button that takes a click [observed: the
    original with no controller, 2026-11-10: `MM_ButtonX` visible 1 and a target on the
    Local Map and the World Map; a 2026-11-07 shot of the World Map shows "Place Marker"
    there]; Quests with either
    device when the player has a quest (`sShowLocation`); Misc with either device when the
    player has a note: `sShowActiveNotes` while all notes show, `sShowAllNotes` while the
    filter is on, `sNextChallenge` while the challenges list is up [documented].
  - **Y**: Misc with either device, its word `sChallengeToggle` in both views, the PC key
    R (the file's PC hint for R names this tile) [documented]. The file also turns it on
    for the maps with a controller, but the original's controller screenshots show no Y
    hint on either map [observed: issue #1897, Local Map "Place Marker X)" alone, World
    Map "Travel A)" and "Place Marker X)"]: NVForever follows the screenshots there; the
    file's rule for the maps is not seen.

  So with the **keyboard and mouse** [documented, the rules above, and observed for the
  maps]: Local Map and World Map "Place Marker" alone (bright: it asks about the custom
  marker, #1954, below); Radio no hint; Quests "Show Location X)" (with a quest); Misc the notes' filter
  ("Active Notes X)", "All Notes X)", with a note; "Next Challenge X)" while the
  challenges show) and "Challenges R)". With a **controller**: Local Map "Place Marker
  X)"; World Map "Travel A)", "Place Marker X)"; Quests "Set Active A)", "Show Location
  X)" (none without a quest); Misc "Play A)" / "Stop A)" on an audio note, the notes'
  filter, "Challenges Y)"; Radio "Tune In A)" (none without a station) [observed: the
  original's controller screenshots on issue #1897 show these sets: Local Map Place
  Marker; World Map Travel, dim with no marker chosen, and Place Marker; Quests Make
  Active Quest and Show Location; Misc the notes' filter and Challenges; Radio Tune].
  The set follows the device as it changes (`crate::pad::labels`, the Controls page's
  Input Device; the page draws again). The keys still do what they did with either
  device (Enter on a row, X on the maps, R on Misc, clicks); only their hints follow the
  original. Until 2026-11-09 every tab showed its A and X hints with the keyboard too
  ("Travel Enter)", "Set Active Enter)", "Tune In Enter)", "Place Marker X)"), Misc "Notes
  R)" while the challenges showed, and the hints were whole boxes right-aligned 10 short
  of 855.
  - **Dim** [design]: what does nothing now: Travel with no Travel target (below), Next
    Challenge (not built; X logs `pipboy data: NextChallenge is not built`), the
    make-active hint on a quest done or with none selected. Place Marker was dim until
    it was built (#1954, 2026-11-10).
  - **Our counts** [design]: "has a quest" is a quest listed; "has a note" counts every
    note the player has, whatever the filter keeps (the file counts the notes list's
    entries; counting the filtered list would hide the filter's hint while it hides every
    note, so it could not be turned off); "known stations" are those the Pip-Boy receives
    here (`crate::radio`); an audio note is a Sound or Voice note, "playing" the one
    playing and not paused (`crate::holotape`) [hypothesis: the file's flag for a playing
    audio note]. Stop shows only on a Voice note: a Sound note's sound is played once and
    not followed (`crate::holotape`; no master's Sound note names a sound, holotapes.md
    §2), so its hint stays Play, which is what a second press does (it plays the sound
    again) [design; `data.rs::audio_hint`].
  - **Words**: the settings' texts, which the masters do not hold (§1), else NVForever's
    labels for them in the game strings table (`fnv_ui::strings`,
    `labels::built_in_label`): `sTravel` "Travel", `sMakeActiveQuest` "Set Active",
    `sPlayAudio` "Play", `sStopAudio` "Stop", `sTune` "Tune In", `sPlaceMarker` "Place
    Marker", `sShowLocation` "Show Location", `sShowActiveNotes` "Active Notes",
    `sShowAllNotes` "All Notes", `sNextChallenge` "Next Challenge". `sChallengeToggle` has
    no label in that table and keeps NVForever's own "Challenges"
    (`labels::label`) [design]. The words on the original's screenshots that no data file
    holds are not copied (LEGAL.md).
  - **Their places** (`frame::Ctx::hint_column`, as the ITEMS page's, §5): the first row
    at the file's 75, its text `BUTTON_TEXT_Y` lower as every button's; the rows 46 apart;
    each word right-aligned at 855, the heading's right end, its key just after it
    [observed: the issue's five 1920 × 1080 screenshots: the words end within 3 px of the
    heading's right end (at 1440 to 1448 px against the heading's 1442 to 1451); the rows'
    letters 43 px apart on Quests and Misc (tops 231 and 274 px), as the ITEMS column's
    42 to 43 px; the first row's letters 77 px under the heading's line, 0.155 of the way
    from the heading's line to the tab line, against 0.150 on NVForever's frame before
    this change, whose first row stood at the same 75]. A click box per row: its word and
    key widened by the file's box, 20 across and 15 up and down, so the boxes of rows 46
    apart meet [documented: `_horbuf`, `_verbuf`]. The hints are spawned after the page's
    body (the maps' window and its markers) and before the question box, so a click on
    a hint over the map's window is the hint's (before this, the map's window took the
    clicks on the World Map's Travel word) [design]. The hover box is not drawn.
  - **The data at the right** (Quests' objectives, a note's text or meter) starts under
    the hints' rect (75 + 120) when any hint shows, a button's height higher on the tabs
    up to Misc while the A hint does not show [documented: `MM_DataRect` y]: with a
    controller Quests' objectives stand one row lower than with the keyboard [observed:
    the issue's Quests screenshot, the objective under both hints].
  - **Keys** [design, #1897]: X is each page's X button (Place Marker, below; while the
    challenges show Next Challenge, which logs that it is not built; Show Location;
    the notes' filter), on the controller X (`crate::pad::menu_buttons`' map entry); R
    (the controller's Y) the challenges toggle. Enter on the World Map and the controller's
    A ask to travel to the Travel target; A presses Enter too, and the two ask once. A
    click on a hint does what its key does. Log: one `pipboy hints: data <tab>: <hint>, …`
    line, "(dim)" after a dim hint, "none" for a tab with no hint, whenever the set
    changes (with the device too).
  - **Travel target** [design, #1897; `data.rs::travel_target`, `travel_ask`]: the marker
    under the highlight box; while the mouse pointer is in use, the last marker the box
    was on, kept while the pointer goes on to the Travel hint, until a click on the map
    away from any marker (or another tab, or the page closing); only a marker still
    shown. Over the buttons at the right the pointer is off the map: the highlight box
    stays where it was, and a press there is the button's, never a click or drag of the
    map, so one click asks to travel once. A click on the map on a marker asks to
    travel there and makes it the Travel target. Since 2026-11-09 the Travel hint shows
    with a controller only, while the page ignores the mouse pointer, so of this only the
    map's own clicks and Enter remain in play with the mouse.
- **Quests**: the quests with a name and an objective shown or done (quest-messages.md §5);
  running quests before those done (completed or failed), each part newest first, quests
  first seen together by FormID from the highest [hypothesis: fits the screenshot's
  order, the add-ons' quests (01…–04…) on top, "Back in the Saddle" (0010A214) before
  "They Went That-a-Way" (000842DD), the completed "Ain't That a Kick in the Head" last].
  "Newest" is when NVForever first saw the quest listed (looked at four times a second;
  not saved). Done quests are drawn at half strength [hypothesis: greyed on the
  screenshot]. The active quest's square is filled; a click or Enter on a running quest
  makes it active [hypothesis: the PC's form of "Make Active Quest"]; until the player
  picks one, the newest running quest is active [design]. When the page comes up the
  active quest is selected [hypothesis: the screenshot shows its objectives with the
  pointer elsewhere].
  - Objectives at the right: 400 wide at the menus' right edge, under the buttons' place
    (75 + 120) less a button's height, the scroll bar on the right [documented]; each
    behind a square, filled and at half strength when done [hypothesis]; those to do
    first, each part from the latest objective down [hypothesis].
  - The right pane (the objectives here, a note's text on Misc) scrolls when longer than
    its window, by Page Up/Down a window (400 for the objectives, 385 for a note; a
    controller's LB/RB press those keys, `pad.rs`) [design], the wheel over it 20 a step
    [documented for a note's text; the objectives take the same step, design; over the
    objectives' 400 × 400 window only, elsewhere on Quests the wheel scrolls the list]
    and, on a
    controller, the right stick's up and down smoothly while held, a note window (385) a
    second at a full push [observed: the original scrolls both panes with the right
    stick, issue #1667; the speed is design, as far a second as one LB/RB page]. One
    scroll serves both panes (`pipboy/data.rs` `scroll_pane`): it stops at the top and at
    the end (the last line at the window's foot) and starts at the top again when
    another quest, note or tab is chosen [design] (Up on the first row or Down on the
    last keeps the place). The objectives' scroll bar shows the part shown and takes the
    pointer as the tile menus' bars do: a click on an arrow steps 20 (a wheel step), on
    the track above or below the marker a page, and the marker held follows the pointer
    [design]; a note's arrows step 20 a click [design].
  - "Show Location" shows the World Map [design: the quest's place is not centred].
- **Misc**: the player's notes (AddNote; `NOTE` records) by name [hypothesis]; "(6 of 6)"
  is part of the note's name [observed: `VMQ01CourierNote` FULL "Mojave Express Delivery
  Order (6 of 6)"]. A text note's text is `TNAM`, a NUL-ended string [observed; the schema
  does not read `TNAM` of `NOTE`]: font 1, broken at 400, in a window 17 left of the item
  data, 20 down, 400 × 385, scrolled 20 a wheel step [documented]; Page Up/Down and the
  right stick scroll it as the objectives (above) [design]; arrows at its right when it
  is longer [documented]. The window reaches
  10 over the first line so its letters are not cut [design]. Our lines break exactly as
  the screenshot's [observed]. An image note shows its picture (`XNAM`) 360 square at the
  right [documented]; sound and voice notes (holotapes) play when used (a click or
  Enter; again to pause, again to go on) and show the audio meter (time remaining and a
  marker moving along the tick meter)
  ([holotapes](holotapes.md) §3). The notes' filter (X, `sShowActiveNotes`) keeps the notes whose quest (`ONAM`) is the active one.
- **Radio**: the stations the Pip-Boy receives where the player is, by name (`crate::radio`,
  radio.md); the one playing has its square filled; a click or Enter tunes to a station,
  or turns the radio off on the one playing [hypothesis: "Tune"]. The waveform panel
  [documented]: 500, 125, 350 square, the tick meters along its foot and right side, the
  flat line (2 high, 5 short, 8 over the middle) while nothing plays [observed: the
  screenshot, the radio off]. While one plays the original's trace moves [observed: a
  60 fps capture of the Radio tab, 2026-10-27]: about 8 waves across the panel stand
  nearly still while their envelope runs to the left, some 7 panel widths a second;
  its height follows the sound (a steady tone draws even bursts with flat gaps, music
  peaks of mixed size, a quiet stretch small ripples), up to about a third of the panel
  either side of the line. Here the waves are two sines drawn under an envelope taken
  from the playing file's loudness (root mean square over 2048 samples) with a slow
  rise and fall of its own [design: whether the original draws the sound itself or a
  model is not known].
- **World Map**: follow a worldspace's `WNAM` parent while `PNAM` bit 2,
  "Use Map Data", is set; the last worldspace is the map root. The root supplies the
  picture (`ICON`) and cell span (`MNAM`) [documented: xEdit `wbDefinitionsFNV.pas`
  WRLD, `wbWorldMapData`; `schemas/fnv/world.json`]. A child without that bit uses its
  own picture and span, or has no map. The picture uses `MNAM`'s usable width and height
  (2048 square for the main map), drawn at alpha 128 [documented: `map_menu.xml`].
  - Each position uses its **own** worldspace's `ONAM` scale `s`, cell X offset `ox`
    and cell Y offset `oy`, and bounds `NAM0` / `NAM9` [documented: xEdit
    `wbWorldMapOffset`, `wbWorldObjectBounds`; `schemas/fnv/world.json`]. With
    `c = (NAM0 + NAM9) / 2` on each axis, its map position is
    `Xmap = s * (X - c.x) + c.x + ox`, `Ymap = s * (Y - c.y) + c.y + oy`.
    Without `ONAM`, use scale 1 and offsets 0; without both bounds, use centre 0.
    This applies to roots and children alike; ancestor transforms are not composed
    [observed: run08 captures of MapMenu traits, 2026-11-07].
  - From the root's `MNAM` north-west `(nwx, nwy)` and south-east `(sex, sey)` cells,
    let `cx = sex - nwx + 1`, `cy = nwy - sey + 1`. Texture coordinates are
    `u = 0.5 + F * ((Xmap / 4096 - nwx) / cx - 0.5)` and
    `v = 0.5 + F * (((nwy + 1) - Ymap / 4096) / cy - 0.5)`, with
    `F = 51/64 = 0.796875` on both axes, independent of picture and usable sizes
    [observed: run08 captures of MapMenu traits, 2026-11-07]. The arrow's and every
    marker's `_x`/`_y` fit exactly with no residual. This replaces the earlier
    fitted origin and span. The traits are texture coordinates 0..1 [documented:
    `map_menu.xml`].
  - Three main-map markers, as the original reported them [observed: run08, the
    `_x`/`_y` of MapMenu's marker tiles on the Mojave Wasteland map (WastelandNV),
    2026-11-07; X/Y are the marker references' positions in the load order's winning
    records (FalloutNV.esm and the four add-ons), read with fnvtool]. WastelandNV's
    `ONAM` is scale 1 with offsets 0, so `Xmap = X`. Texels are of the 1024 picture
    (`_x * 1024`); "old fit" is the earlier origin (0.108, 0.102) and span 0.7876.

    | Marker (reference) | X, Y | Original `_x`, `_y` | In texels | Old fit, texels (off by) |
    |---|---|---|---|---|
    | Goodsprings (0x000CDF51) | −70992, 2560 | 0.33236843, 0.5636847 | 340.3, 577.2 | 344.2, 572.2 (+3.8, −5.1) |
    | Hoover Dam (0x00135E5F) | 74712, 24848 | 0.79706764, 0.49260074 | 816.2, 504.4 | 814.5, 500.2 (−1.7, −4.2) |
    | Jacobstown (0x000CDF57) | −108427.35, 96469.77 | 0.21297449, 0.26417476 | 218.1, 270.5 | 223.4, 269.0 (+5.3, −1.5) |

    The rule above gives the original's `_x`/`_y` for all three to under 0.0001 texel,
    and for all 184 markers placed in WastelandNV itself that the capture read (the
    other five on the map are placed in child worldspaces); the old fit missed those
    184 by 5.0 texels on average and 9.8 at most (the larger axis of each). The old fit was tuned on places on
    the picture, not on the markers: Goodsprings' roads at (342, 575), the arc of Hoover
    Dam at (815, 496) and the ring of Jacobstown at (225, 270). Those lie 1.7/2.2,
    1.2/8.4 and 6.9/0.5 texels (x/y) from where the original draws the markers, which
    is how far the new rule moves away from them. These numbers replace the screenshot
    fit; the test `the_wasteland_map_puts_markers_where_the_original_reads_them`
    (`engine/nvf-play/src/pipboy/data/model.rs`) checks the three rows within 0.01
    texel, room for 32-bit rounding only.
  - The map shows every worldspace in this family, each marker projected with its
    own worldspace's numbers; inside a child it still shows the root's picture and
    the whole family's markers. The arrow uses the current worldspace's numbers;
    outdoor quest markers in any family worldspace use the same projection
    [observed: run08 captures of MapMenu traits, 2026-11-07].
  - The menu file keeps the picture's edges at most to the window's middle
    [documented: `map_menu.xml`]; NVForever opens centred on the player, pans when
    dragged, and zooms 1.25 a wheel step from 1 to 4 times
    [design: `engine/nvf-play/src/pipboy/data.rs`].
  - Map markers: references of the `MapMarker` STAT (0x10) with `XMRK`; name `FULL`, type
    `TNAM` (0 None … 14 Vault [documented: xEdit; observed: vaults 14, a cave 5, Camp
    McCarran 8, Hoover Dam 7]), flags `FNAM`, radius `XRDS`. Shown when the player has been
    within the radius (`XRDS`) in the marker's own worldspace with its type's icon,
    or when ShowMap or the Visible flag made it known, with the undiscovered icon
    [hypothesis]. `GetMapMarkerVisible` reads the authored flags before the player has
    found the marker: `FNAM` Visible (1) alone gives 1, Visible | Can Travel To (3)
    gives 2, Can Travel To alone (2) gives 0, as does no flag [observed: observer check
    252, 2026-11-22, campaign `o31_mapmarker_visible` with a probe plugin overriding
    Coyote Den 0x00154A69 to 1, Hoover Dam 0x00135E5F to 3 and Jacobstown 0x000CDF57 to
    2, and Honest Hearts' Southern Passage, authored 3, read as it ships]. JIP's
    `GetMapMarkerTravel` read 0 for the Can Travel To alone marker and 1 for the
    Visible | Can Travel To ones [observed: same runs], so Can Travel To without Visible
    gives no fast travel either [hypothesis as a rule]. Unflagged markers in the same
    save read 0 until shown or found; `ShowMap` 0 made one read 1 and `ShowMap` 1 made
    one read 2; `ShowMap` 0 on a marker already at 2 left it at 2; walking into a marker's
    radius after `player.MoveTo` made it read 2 and raised Locations Discovered by one
    [observed: same runs, control run without the probe plugin]. In the base game only
    Southern Passage sets any of these flags: every placed MapMarker of FalloutNV.esm (189),
    DeadMoney.esm, OldWorldBlues.esm and LonesomeRoad.esm has `FNAM` 0, and HonestHearts.esm
    has one at 3 among 55 [observed: a scan of every MapMarker reference, 2026-11-22].
    A marker without `XRDS` is found within `iMapMarkerRevealDistance`
    [documented: GECK wiki "Complete List of GameSettings", the Fallout: New Vegas default
    1000; no master sets it], that setting being this distance a [hypothesis] (#2338;
    observer check 128); a plugin's value is used as it is, and 0 finds no such marker by
    walking [design]; before #2338 such a marker was never found by walking up to it.
    Named markers (`XMRK` and `FULL`) in each master's own worldspaces, and how many have no
    `XRDS` [documented: fnvtool esm decode of each placed MapMarker reference]:
    FalloutNV.esm 189 / 66 (WastelandNV 186 / 65, Coyote Den 0x00154A69 and the four
    Powder Ganger Camps among them; FreesideWorld 1 / 1; WastelandNVmini 2 / 0; the
    others carry XRDS 200-15000, median 2500 in WastelandNV); DeadMoney.esm 11 / 4;
    HonestHearts.esm 55 / 26; OldWorldBlues.esm 35 / 16; LonesomeRoad.esm 28 / 18. None
    carries an `XRDS` of 0, so an absent radius and a non-positive one are read alike
    [design: `discovery_radius` in `engine/nvf-play/src/pipboy/data.rs`]. Icons `interface\icons\world map\icon_map_<type>.dds`, the inverted form
    under the highlight box [hypothesis: by the files' names]; 24 wide, 1.75 times and
    brightness 255 (else 220) under the box [documented: the template's 1.75 and
    brightness; observed: 24 fitted].
  - The player's arrow: `glow_cursor.dds` [documented], 40 wide, 1.28 times as high
    [observed: fitted, the screenshot's triangle 25 px], showing the part of the picture the
    file's `zoom` (its width × 3.5) shows: 1.4 times its texels from its top left, cut to the
    box [documented: `MM_WorldMapCursor`]. The picture's arrow is a pointer in its upper half,
    its tip at texel (9, 9) and its base from (24, 23) to (9, 29), its axis some 24 degrees
    left of up unturned [documented: the texels]; it is turned by the heading and that angle
    about the box's middle, so it points the way the player faces, up at heading 0 as the
    original's, a sharp symmetric triangle [observed: a capture of the original's World Map at
    1280x720, 2026-11, heading 0] (`data::arrow_turn`). Until 2026-11-12 the hand-built page
    stretched the whole picture over the box and turned it by the heading itself, its tip off
    the way the player faced; fixed on both drawings. The hand-built arrow's drawn size changed
    with it: the box is still 40 × 51.2, but its picture went from 1.25 × 0.8 times its texels
    (the whole 32 × 64 picture stretched) to 1.4 times both ways (the part the file's zoom
    shows), so the pointer is drawn about 12% wider and 75% taller than before [design: as the
    file draws it; how it compares with the original's size is to be checked on shots, not
    measured]. Drawn over the markers and under the highlighted one [documented:
    depths 1, 7, 11].
  - Quest markers: the targets (`QSTA`) of the active quest's objectives shown and not done,
    where they are now; a target indoors at the door outdoors into its interior
    [hypothesis: the screenshots' quest marker for Sunny Smiles stands by the saloon's
    door]; the picture is the HUD compass's objective marker
    `glow_hud_compass_objective_marker.dds` [hypothesis: the shape on the screenshots],
    56 wide [observed: fitted], 1.25 times as high, 15 right and 5 down [documented]. The
    marker is for the targets whose conditions pass [documented: Quest Objectives Tab],
    evaluated four times a second [hypothesis]; a
    target indoors is marked at a door into it that is not locked unless the target ignores
    locks [hypothesis] (quests-dialogue.md §2, target conditions).
  - The highlight box [documented]: 160 square, corners of 40 with 15-long fading tips, a
    dot of 8 (`waypoint_node.dds`) at its middle, the name in font 1 centred 180 down. It
    follows the pointer over the map and takes a marker's place and name within 40
    [hypothesis: the screenshots show it away from the middle, once round a door with
    the door's place name]; at first it is at the window's middle. A World Map marker
    with a reputation (`REFR WMI1`) carries its reputation in parentheses on a
    second line in the original [observed: run08 captures of MapMenu traits,
    2026-11-07]. The field is the reputation shown on hover [documented: GECK wiki
    Reference, Map Marker]. NVForever keeps its current reputation name and standing
    wording; that wording remains the maintainer's open review item
    [design: `engine/nvf-play/src/pipboy/data/model.rs::reputation_line`;
    `docs/owner-review.md`].
  - A click on a marker (the button pressed and let go without the pointer moving
    more than 6 units [design]) asks whether to travel there: fast travel,
    `fast-travel.md`. Enter, Space or the Travel hint (with a controller, its A) ask for
    the marker under the highlight box too (#1897) [design].
- **Place Marker and the custom marker** (#1954; `data/marker.rs`, `data.rs`
  `place_marker`, `answer_marker`, `waypoints`). From a capture of the original's
  MapMenu with the keyboard and mouse at 1280 × 720 (tile readings and screenshots,
  2026-11-10) [observed unless marked]:
  - Place Marker is the maps' `MM_ButtonX` (the word alone with the keyboard, the X
    button with a controller). Pressed (X, the controller's X, a click on the hint) it
    opens a question box. With no custom marker it asks whether to set it, two
    answers, yes first: yes places the marker at the highlight box's middle (the
    marker's texture coordinates equal the box's middle within 0.001), no does
    nothing. With a marker placed it asks with three answers, in this order: move (to
    the highlight box's spot), remove, leave (it stays).
  - **With the mouse** (#3324) [design]: a click on a map away from any marker (the left
    button pressed and let go on the map's window, the pointer moving no more than 6
    units between, as a travel click) does what X does there, of the place under the
    highlight box at the pointer: it asks the question above, or under the player's No
    Place Marker Popup tweak answers at once as X does (docs/design/tweaks.md). The
    click is read once the map is laid out for good on the frame the button is let go
    (its pan and zoom, the window the page drawn from the file published and any second
    layout of it, the highlight box as it stands then): on the World Map, with the
    player's Map Selectable Companions tweak on and companions under the box then, it
    summons them; else with a marker under the box it asks to travel there instead and
    places nothing. A click wins over Enter (A, the Travel hint) in the same frame: one
    question, the click's. The Local Map has no travel, so a click anywhere
    on it places (on a door or marker the box has taken, at its spot, as X there). A
    press that moved further is a drag and does neither; a press on a hint over the map
    is the hint's; with a question box up the box takes every click; a map that cannot
    be drawn (a worldspace without map data) takes none, nothing being under its box; a
    press held while DATA closes (another page, the Pip-Boy down) is let go with it
    (`data.rs::travel_ask`, `take_click`, `step`). The capture above placed the marker
    with X [observed]; how the original's mouse places it (a left click on an
    empty spot, whether it asks first) is [hypothesis] (docs/handoff/observer-checks.md
    #262). Log: `pipboy map: a click on the <world|local>
    map away from any marker: Place Marker`, then Place Marker's own lines.
  - The words are game settings by name: `sSetMarkerQuestion` (no marker yet),
    `sMoveMarkerQuestion` (one placed), `sYes`, `sNo`, `sMoveMarker`, `sRemoveMarker`,
    `sLeaveMarker` [documented: the GECK wiki's Complete List of GameSettings names
    them; which setting holds which answer is matched to the box's texts]. FalloutNV.esm
    and the add-ons define none of them [observed: fnvtool's GMST inventory of every
    master, 2026-11-10], so an English install shows NVForever's own words, written for
    this box (`marker::Words`; LEGAL.md) [design]; a plugin that defines a setting wins.
    NVForever's box is the fast-travel question box (`travel::Popup`, its keys and
    clicks: Up/Down, Enter, Y the first answer, N the last). An answer the pointer is on
    takes the choice only once the pointer moves more than 2 units after a key moved it:
    the box is drawn anew as its choice moves and the answer under a resting pointer was
    entered again, which undid every Up/Down at once. The answer under the pointer is
    followed from its pointer-over and pointer-out events, so a pointer moving on within
    an answer takes it once the hold lets go. Enter (or Space, E, the confirm button)
    takes the choice as it stands after the same frame's Up and Down (both boxes;
    `data.rs::box_frame`, `choose`) [design].
  - There is **one** custom marker, shared by both maps: placed or moved on the Local
    Map, the World Map shows it at the matching world position, and the other way
    round. It is the scripts' marker too: JohnnyGuitar's SetCustomMapMarker "functions
    the same as setting the marker within the map menu in the Pip-Boy" [documented: GECK
    wiki SetCustomMapMarker], so NVForever keeps it in the game
    (`fnv_game::Game::custom_map_marker`, the worldspace outdoors or the interior cell,
    and the point; docs/spec-notes/ext-jg8.md) and the Pip-Boy reads and writes that one
    [design]. On the World Map the spot is the position under the box's middle in the
    worldspace shown (the picture's projection undone, `model::WorldMap::position`); on
    the Local Map the point under it in the interior or worldspace the player is in
    (`marker::local_point`), so an interior's marker shows on that interior's Local Map
    only [design].
  - Drawn: a tile `glow_hud_compass_pc_marker.dds` at depth 5, over the quest markers
    (4), as wide as the quest markers on both maps (50.59 on the World Map, 75 on the
    Local Map in the capture). NVForever draws it as a quest marker is drawn (size, the
    1.25 height, the 15, 5 nudge, the Local Map's clamp to the window) one step above
    them [hypothesis: the quest markers' templates, `WorldMapQuestMarkerTemplate`,
    `LocalMapQuestMarkerTemplate`, whose width the capture's matches].
  - **The dotted lines** (World Map only: the capture's Local Map has no waypoint
    nodes): each a chain of `WaypointTemplate` nodes (`waypoint_node.dds`, 0.8 of the
    map markers' width, depth 3; 17.41 against the markers' 21.76) with a `PathLine`
    child (`waypoint_path.dds`, tiled, depth −1, a third of the node's height, from the
    node's middle, `(user0 − 4) × _Magnification` wide, turned about its left end by
    `rotateangle`) [documented: map_menu.xml]. `user0` is the path's length at a
    magnification of 1 (the capture: 27.87 for a target 0.0136 of the 2048 picture
    away) and `rotateangle` its direction (0.377 for a target up and to the right). With
    the active quest's target on the map one chain runs from the arrow to it; with a
    custom marker placed too, a second chain runs from the arrow to the marker (two
    nodes at the arrow, one at each target) [observed]. NVForever draws one line from the
    arrow to the custom marker and one to each quest target the World Map shows, a node
    at each end, the file repeated along the line at its 64 × 16 shape at the line's
    height [hypothesis: the tiling; one line per target where there are several]; a
    target nearer than 4 has none (the capture's −2.1 wide line).
  - Not captured: whether the original keeps the marker in its saves (the capture's save
    was made before any marker existed) [hypothesis: it does]. NVForever saves it
    (`custom_map_marker` in the engine's part of a save, the worldspace or cell moved to
    the current load order, dropped with a plugin no longer loaded) and puts it back into
    the restored game before the LoadGame and PostLoadGame handlers and the Script
    Runner's load files run, so a handler's SetCustomMapMarker or ClearCustomMapMarker
    wins (`data.rs::restore_custom_marker`, from `crate::script`) [design]; a new game
    starts without one (the game is made anew).
  - The HUD compass: the original has a compass tile with the pc-marker picture, but it
    stayed hidden at four headings with the marker about 300 units away (why is not
    known; perhaps it hides near the player) [observed]. Not built.
  - Log: `pipboy map: Place Marker asks "<text>" (<n> answers) for (x, y) in <space>`,
    `pipboy map: answered "<answer>"`, `pipboy map: custom marker placed at (x, y) in
    <space>`, `… moved to (x, y) in <space>`, `… removed`, `… left where it was`,
    `pipboy map: no custom marker placed`; `pipboy map: <world|local> map shows the custom
    marker at (x, y)` / `shows no custom marker` when that changes; `pipboy map: dotted
    line to <custom marker|quest target> (n segments)` when the lines' targets or the
    marker change; `pipboy map: custom marker back from the save at (x, y) in <space>`.
    Test aid: `placemarker` (§7).
- **Local Map**: the same window, markers, arrow, quest markers (kept inside the window
  [documented]) and highlight box, on an empty map at 28 game units to a unit [observed:
  fitted, the gas station's door and the saloon's door 168 px apart on the screenshot; the
  zoom then is not known]. Doors leading elsewhere (`XTEL`) in the 7 × 5 cells round the
  view (or the interior) show `interface\icons\local map\icon_local_door.dds`, named by the
  place their destination door stands in [observed: the screenshot's door is named
  "Goodsprings Gas Station", the cell `GSGasStation`]. Left out: a door reference flagged
  Hidden From Local Map (header bit 6, "The reference will not be displayed on the local
  map" [documented: GECK wiki Reference]; 4 teleport doors, all in Dead Money
  [observed]), a door whose base DOOR is flagged Hidden (`FNAM` bit 2 [documented: xEdit];
  3 in FalloutNV.esm, all secret doors, none a teleport door [observed]; what the flag does
  is a hypothesis, owner-review.md), and a door disabled now [hypothesis, as a disabled
  marker]. Under them an outline of the place
  (`data/outline.rs`) [design: the original renders the area from above, its screenshot
  dark save a few shapes; the owner asked for an outline]: a grid of rays straight down
  through the physics world's collision (not actors, triggers or loose clutter), 96
  units apart outdoors from 8000 over the feet, 28 apart indoors from 150 over the feet
  (under the ceiling), cast a few hundred a frame (6 ms) and made into one picture when
  done; outdoors the ground shaded by its slope lit from the north-west, indoors the
  floor (a ray landing within 120 below and 40 above the feet) filled faintly, what
  stands on it brighter, and the floor's edge to nothing (the walls) or a step of 64
  down to a neighbour drawn bright on the higher side; outdoors a step of 192 (walls,
  cliffs, not the hills' slopes at that spacing). The grid covers the window at a
  magnification of 1 and 15% more round its anchor, kept while a pan stays within a
  quarter of it, made again for another floor (128 up or down) or place. Indoors the
  map is 8 units to a canvas unit (outdoors 28) [design]. What the physics world has not
  loaded stays empty. Not built: which doors and markers the original shows (its
  screenshot shows only the gas station's door), fog of the unexplored.
- **Dragging** (both maps): the left button held down on the map and the pointer moved
  pans it [design]; the pointer's own moves are followed, not Bevy's drag events, which
  ended at once because the map's nodes are drawn again as the highlight box follows the
  pointer (the owner's list 5, item 52). Test aid: `drag=<dx>:<dy>`.
- **On a controller** [observed: the original] the right stick moves the highlight box
  and, pushed past an edge, pans the map; the left stick's up and down zooms in and out
  smoothly while held (the cursor and zoom speeds are design); see controller.md §3.
- Checking it: as §7 with `page=data` and `tab=<0-4>`; `key=down|up|use` steps move and
  act on the DATA lists too. A run with a note, quests and the radio on:
  `NVF_PLAY_TEST_SCRIPT=3:info:0x001057E8:DocMitchellREF,4:start:VCG02,4.5:objective:VCG02:3,5:objective_done:VCG02:3,5.1:objective:VCG02:5`
  (Doc Mitchell's line whose result script adds the courier's note) and
  `NVF_PLAY_TEST_RADIO=5:next`. Log: "pipboy data: N map markers", "map marker … found",
  "local map: N doors leading elsewhere", "active quest …", "radio tuned to …".
- Open: the quest list's order and the active quest's default; the objectives' order and
  how done ones look; the hollow square's size (ours draws smaller than the original's,
  about 9 px against 14); which doors and markers the local map shows and its zoom;
  where the World Map opens; how the original turns the arrow; the radio's waveform model;
  Challenges, the audio notes' panel, the buttons' hover boxes; the custom marker's
  compass mark and whether the original saves it (Place Marker above).

### 5.3.1 DATA from its file: what the code writes

The DATA page drawn from `menus\main\map_menu.xml`, in the Pip-Boy's image, behind
`[UI] TileMenus=MapMenu`, on by default when the list is absent [design, #2215;
ui-tile-tree.md section 6.2] (`tiles/map_menu.rs`; issue #1513). Off, nothing of it
runs and the hand-built page (`pipboy/data.rs`) draws as before. The rules stay `pipboy::data`'s:
the module writes values and says what a tile stands for; commands go where the hand-built page's
go [design]. Tile names, places and expressions are [documented: `menus\main\map_menu.xml` and the
prefabs it includes, `list_box.xml`, `list_box_template.xml`, `text_box.xml`, `tabline.xml`]. No
capture of the original's MapMenu traits was read for this slice but run08's map markers (§5.3);
what the original writes into the traits below is not known unless tagged.

**This slice draws every tab: the Local Map, the World Map, Quests, Misc and Radio** [design,
#1513]. Both maps are drawn from the file through one viewport the hand-built page shares
(`pipboy/data/view.rs`, `MapView`, below); their picking stays the model's. The Local Map's picture
is NVForever's own outline of the place (§5.3), given to the tile painter as a runtime picture.
Radio's waveform in the file is `MM_Waveform`, an image the code gives an animation ("set in code",
which one not known); it stays hidden, and the hand-built trace of the radio's loudness (§5.3) is
painted over the file's own frame of the panel (below). The page goes over to its tiles and back as
the tab changes, as STATS' tabs do; with the switch on every tab is prepared early (below), so the
question boxes (fast travel's, Place Marker's, #1954) and their hover reset work on either drawing.

**The one decision**: `MapTiles::ready`, as `InventoryTiles::ready` (§5.2.1): the switch on, DATA
showing on a tab drawn from the file, the file's tiles there, the tabs made, the model prepared for
this tab, and a usable measured layout. A question box up does not change it (below). A failure
leaves the whole
hand-built page, says why once (on the third final pass in a row that fails), and is tried again
when what it would show or the tree changes. While the Pip-Boy goes down the page stays as last
written [design].

**Preparing and painting** [design]: with the switch on the DATA page's preparation (its lists, the
player's keys, the pointer's acts, notes marked read, the Local Map's outline, the quest order and
the hints) runs in `pipboy::data::prepare`, before the tile decision and the frame, for the DATA page
as the Pip-Boy shows it, whatever draws it; the hand-built `draw` then only paints, and only while its
body is there. Off, `draw` prepares and paints as before. Exactly one presentation publishes what the
page's rules read of the drawing: how many rows fit, how far the right pane can scroll and its window
(the wheel's), each from the drawing that showed this frame.

**The file's structure this relies on** [documented: map_menu.xml]:

- The root `MapMenu` binds the controller's X to `MM_ButtonX`'s `clicked` and Y to `MM_ButtonY`'s,
  and names `MM_ButtonY` for the PC's R (`_PCButton_R`).
- `MM_Tabline` (`tabline.xml`) under `GLOW_BRANCH`, beside `MM_MainRect`: `_CurrentTab` is the tab,
  0 Local Map, 1 World Map, 2 Quests, 3 Misc, 4 Radio. The parts' `visible` read it: the local map's
  window on 0, the world map's on 1, the highlight box's window on 0 and 1, `MM_QuestsList` on 2,
  `MM_NotesList` and `MM_ChallengeList` on 3, `MM_RadioStationList` and `MM_WaveformRect` on 4. The
  tabs are made from `TabButtonTemplate`, as ITEMS' (§5.2.1).
- `MM_HeadlineRect`: `MM_Headline_Title` (`&-sData;`), the line `MM_Headline_h2` (x 180, 470 long),
  `MM_Headline_LocationInfo` and `MM_Headline_TimeDateInfo`, both `string` "Set in code".
- The lists (`list_box.xml`): `MM_QuestsList`, `MM_NotesList`, `MM_RadioStationList`,
  `MM_ChallengeList`, each at x -15, y 75, 400 × 468; `_enabled` is empty, "set in code", on the
  first three (the radio's follows its own `visible`). A row's `target` is its list's `_enabled` and
  its own; the highlight box shows only while `_enabled`.
- The rows' templates: `MM_ListMarkerTemplate` (`MM_ListItem`, 400 wide; `ListItemText` font 2 at 40,
  20, wrapped at 310, its `string` the row's and its `alpha` the row's `_TextAlpha` (255);
  `MM_Template_ItemMarker`, 25 square at 15, 20, shown when the row's `_ShowEmptyMarker` (false) or
  its `_selected` is, `square.dds` or `square_filled.dds` by `_selected`, at `_TextAlpha` too) and
  `MM_ListTemplate` (its text at 15, no marker).
- `MM_ButtonRect` at the main rect's right edge, at the Quests list's `y`, 120 high, holding
  `MM_ButtonA`, `MM_ButtonX` and `MM_ButtonY` (`text_box.xml`): right-justified, `_horbuf` 20,
  `_verbuf` 15, X under A and Y under X only where the one above shows (`_y`), the line's alpha
  255 while `target`, 127 otherwise (dim). Their `visible` and `_Title` are expressions of the tab,
  the lists' child counts, `_ItemType` and `&xbox;` (§5.3's hint rules); Y's `_PCButtonText` is
  `&-sPCMenuHintR;`.
- `MM_DataRect`, 400 × 400 at the main rect's right, under the buttons when one shows: `visible`
  false, set in code; its user traits `_ItemType` (the selected row's kind: its children show on 1
  the text, 2 the picture, 3 the audio meter, 4 the objectives), `_ItemStringInfo` (the audio's time
  left), `_AudioFileCompletion` (the share played) and `user0` (a playing audio note highlighted).
- `MM_QuestObjectivesList` under `MM_DataRect` (`list_box.xml`): 400 wide, 400 high on a PC, at the
  rect's right, `_enabled` false (no highlight box, no row a target), `target` true, its scroll bar on
  the right, shown on `_ItemType` 4.
- Misc's parts under `MM_DataRect`: `MM_DataTextRect` (a clip window 385 high on a PC, at -17, 20,
  shown on `_ItemType` 1) holding `MM_TextScrollbar` (`scrollbar_vert.xml`, its `_ScrollDelta` 20,
  one visible item) and `MM_DataText` (font 1, wrapped at the rect's width, its `y` the bar's
  `_current_value` times `_ScrollDelta`, negated; its `string` set in code); `MM_DataImage` (360
  square at the rect's right, its `filename` set in code, shown on `_ItemType` 2); `MM_DataAudioInfo`
  (shown on 3) with `MM_AudioTimeRemaining` reading the rect's `_ItemStringInfo` and the meter's
  marker placed by its `_AudioFileCompletion`. The rect's `user0` (a playing audio note) turns A's
  title to `sStopAudio` [documented: `MM_ButtonA`'s `_Title`].
- `MM_NotesList` and `MM_ChallengeList` both show on tab 3 (their `visible`), each with a
  `square_filled.dds` highlight (`MM_Notes_SelectedNoteHighlight`, `MM_Challenge_SelectedHighlight`)
  hidden until set in code; the challenges' `_enabled` is set in code too.
- `MM_RadioStationList` (`list_box.xml`, as the other lists) and `MM_WaveformRect` beside it (500,
  125, 350 square, shown on tab 4): `MM_WaveformMeterHoriz` (`tickmark_horiz.dds`, 32 high at its
  foot, `cropy` 2), `MM_WaveformMeterVert` (`tickmark_vert.dds`, 32 wide at its right, `cropx` 2),
  `MM_WaveformCenterLine` (`solid.dds`, 345 × 2 at y 166) and `MM_Waveform` (hidden, an `animation`
  "Forward", set in code).
- The World Map: `MM_WorldMap_ClipWindow` (a clip window 50 down, 855 × 500, shown on tab 1) holding
  `MM_WorldMap_ParentImage`, a **hotrect** with the picture's `filename`, `filewidth` and
  `fileheight` (2048), `_Magnification` (1), `user0`/`user1`/`user2` (the map markers', quest
  markers' and arrow's widths, "set in code"), its `width`/`height` the file's size times the
  magnification and its `x`/`y` its drag held between the window's middle less its size and the
  middle; its children are the markers, which the original's code removes and makes again every
  update (the file's comment). Beside it `MM_WorldMapCursor` (`glow_cursor.dds`, `_x`/`_y` in
  texture coordinates times the picture's size plus its place less half its own size, `width` the
  picture's `user2`, 1.28 as high, turned about its middle by `rotateaxisx`/`rotateaxisy`).
  `MM_Highlight_ClipWindow` (the same box, shown on tabs 0 and 1) holds `MM_MapHighlightBox` (160
  square, its `_Title` shown by `MM_HighlightLocationName`, font 1, centred 80 in and 180 down, a
  centre dot and the corner lines).
- The World Map's templates, each placed at `_x`/`_y` (texture coordinates 0..1) times its parent's
  size less half its own: `MapMarkerTemplate` (`MapMarker`: `_LocationName`, `_MarkerIndex`,
  `_selected` 1.75 times larger, brightness 255 not 220 and 10 deeper; `filename` set in code; width
  the parent's `user0`), `WorldMapQuestMarkerTemplate` (`WorldMapQuestMarker`, the parent's `user1`
  wide, 1.25 as high, 15 right and 5 down, depth 4), `WaypointTemplate` (`WaypointIcon`,
  `waypoint_node.dds`, 0.8 of `user0`, depth 3) and `PathLineTemplate` (`PathLine`,
  `waypoint_path.dds` repeated, from its parent's middle, a third of its parent's height, as long
  as its `user0` less 4 times its grandparent's `_Magnification`, turned about its left middle,
  depth -1). `LocalMapQuestMarkerTemplate` is the Local Map's.
- The Local Map: `MM_LocalMap_ClipWindow` as the World Map's window (shown on tab 0), holding
  `MM_LocalMap_ParentImage` (a hotrect as the World Map's, with no picture and `visible` false, both
  set in code) and `MM_LocalMapCursor` (as the World Map's arrow, hidden until code shows it);
  `LocalMapQuestMarkerTemplate` as the World Map's quest markers' but held inside the window (its
  `x` and `y` clamped by the picture's place and the window's size).
- Not read in this slice: `MM_Headline_ChallengeType`, the two square highlights above.

**What the code writes on Quests** [design]:

| Model (`pipboy::data`) | Tile, trait | Notes |
|---|---|---|
| The tab | `MM_Tabline`: `_ButtonCount` 5, `_CurrentTab`, `_LeftLineLength`; five tiles from `TabButtonTemplate`, each `listindex`, `string`, `_x` | As ITEMS' tabs (§5.2.1), the same rule for their places. The words are the hand-built page's (`Page::tabs`: `sLocalMapTabText` … `sCommsTabText` where the load order defines them, else "Local Map", "World Map", "Quests", "Misc", "Radio"). |
| The place, the date and time | `MM_Headline_LocationInfo/string`, `MM_Headline_TimeDateInfo/string` | `read::place_name`, `model::date_time`, as the hand-built heading. |
| Title, heading line | `MM_Headline_Title` hidden in the screen menu, `MM_Headline_h2` run on; both as the file on the device | As §5.1.1 [design: the owner, 2026-10-31]. |
| The quests (`model::sort_quests`'s order) | `MM_ListMarkerTemplate` rows under `MM_QuestsList`, kept by the quest's FormID; each row's `string`, `_selected` (the active quest: filled square), `_ShowEmptyMarker` 0, `_TextAlpha` 128 for a quest done, else 255; the list's `_enabled` 1 | A done quest at half strength and the active quest's square as the hand-built page's (§5.3) [hypothesis there]. Rows from `list::reconcile`, placed by `list::measure_rows`: a wrapped name makes a taller row. |
| The selection, boxed | the list's `_highlight_y`, `_selected_height` while boxed; -1 and 0 otherwise | As the hand-built box: under the pointer or after Up/Down, none when a tab comes up. |
| The list's scroll | `lb_scrollbar` counts by `list::counts`, `_current_value`, `_SetInCode` | The tile page's own scroll, as ITEMS' (§5.2.1): the keys bring the selected row into view, the pointer's pick does not; the wheel over the page (not over the objectives) moves it a notch a value; the bar's arrows, page areas and marker move it. |
| The selected quest's objectives (`objectives`) | `MM_DataRect`: `visible` 1 while a quest is selected, `_ItemType` 4; `MM_ListMarkerTemplate` rows under `MM_QuestObjectivesList`, by place: `string`, `_ShowEmptyMarker` 1, `_selected` (done), `_TextAlpha` 128 when done | The hand-built pane's squares and dimming. |
| The right pane's scroll (`text_first`, one scroll for both panes, §5.3) | `MM_QuestObjectivesList`: `_scroll_delta` 1; its `lb_scrollbar`'s `_number_of_items` (how far it can scroll, plus 1), `_number_of_visible_items` 1, `_current_value` the pane's scroll, `_SetInCode` | So the rows move by exactly the units the hand-built pane does: Page Up/Down a window, the wheel over it 20 a notch, the right stick, the bar's clicks (an arrow a notch, a page area a page of the window as measured, the marker held dragging by the file's own bar: its marker's place over its `_step_height` is the pane's scroll) [design]. |
| The hints (`tab_buttons`, #1897 / #1952) | `MM_ButtonA` (Set Active), `MM_ButtonX` (Show Location), `MM_ButtonY` (none on Quests): `visible`, `string` (the hint's word), `_PCButtonText` (its key, "X)", with a controller its button), `target` (lit: 1, dim: 0) | The hand-built page's rules, not the file's expressions: A with a controller only, X with either device when a quest is listed, no Y on Quests. Bound by what each hint does (`ButtonKind`), never by its place in the list. Put right on every pass: the file works `visible` and `string` out by expressions, again whenever what they read changes (the tab, a list's rows). |
| `MM_Background` | hidden in both frames | The shell's background, as §5.1.1. |
| `MM_Headline_ChallengeType` | hidden | Misc's, not on this tab. |

**What the code writes on Misc** [design]: the hand-built page's lists and panes (§5.3), as on
Quests where nothing is said:

| Model (`pipboy::data`) | Tile, trait | Notes |
|---|---|---|
| The notes (the notes' filter applied), or the challenges while Misc shows them | `MM_ListMarkerTemplate` rows under `MM_NotesList` or `MM_ChallengeList`, kept by the note's or the challenge's FormID: `string`, `_selected` 0, `_ShowEmptyMarker` 0, `_TextAlpha` 128 for a challenge completed, else 255; that list's `_enabled` 1 | The hand-built rows' words, place and dimming (no square). The list not shown has no rows, `_enabled` 0 and `visible` 0, put right on every pass (the file shows both on Misc). The quests list likewise on Misc, the notes' and challenges' on Quests. |
| The list's scroll | each list's own (`list::ListView`), as Quests' | The notes' filter is the list's filter: another one starts the list at its top. So does the toggle between the notes and the challenges, as the model starts the list it shows at its first row; back from another tab a list keeps its scroll. |
| The selected note's text, a challenge's description and state | `MM_DataRect` `_ItemType` 1; `MM_DataText/string` the text (`\r\n` as `\n`) | One scroll for the text and the objectives, as the hand-built pane's: `MM_TextScrollbar`'s `_ScrollDelta` 1, `_number_of_items` how far the text can scroll (its measured height past the window it is seen through: what clips it, the rect or a smaller clip above it) plus 1, `_number_of_visible_items` 1, `_current_value` the pane's scroll, `_SetInCode`. |
| An image note's picture | `_ItemType` 2; `MM_DataImage/filename` the note's `XNAM` | |
| A Sound or Voice note | `_ItemType` 3; the rect's `_ItemStringInfo` the time left as the hand-built page writes it ("00:30:9 remaining"), `_AudioFileCompletion` the share played (0 to 1) | `user0` is not written: the A hint's words are the model's. |
| No note or challenge | `MM_DataRect` `visible` 0, `_ItemType` 0 | The text and picture emptied while they do not show. |
| The hints | X the notes' filter (Active Notes / All Notes) or Next Challenge (dim), Y Challenges, A Play or Stop with a controller | By what each does (`Slot::of`); Y's key is the hand-built page's (R, with a controller Y). |

**What the code writes on Radio** [design]: as on Quests where nothing is said:

| Model (`pipboy::data`) | Tile, trait | Notes |
|---|---|---|
| The stations the Pip-Boy receives here (`crate::radio`'s list) | `MM_ListMarkerTemplate` rows under `MM_RadioStationList`, kept by the station's base FormID: `string`, `_selected` (the one playing: filled square), `_ShowEmptyMarker` 0, `_TextAlpha` 255; the list's `_enabled` 1 | The hand-built rows' words and square. Which template the original's code makes the stations from is not known [hypothesis: the marker template, as the playing station's square shows on the original's screenshots]. A click (`UseRecord`) or Enter tunes to the station, or turns the radio off on the one playing, the hand-built page's rule (`use_row`). |
| The list's scroll | its own (`list::ListView`), as Quests' | The wheel over the page moves it. |
| The waveform panel | `MM_WaveformRect/visible` 1 on Radio, 0 on the other tabs; `MM_Waveform/visible` 0 | Put right on every pass, as the lists' `visible`. The file's frame draws: its two tick meters and its centre line. `MM_DataRect` is hidden (`visible` 0, `_ItemType` 0). |
| The trace while the radio plays (`data::WaveScope`) | none: painted over the tiles (`data::trace_over_tiles`), inside `MM_WaveformRect`'s box as laid out, cut to it | The hand-built trace (§5.3), its proportions scaled from the 350 square to the box measured (at 350 exactly the hand-built numbers: the line 5 short and 8 over the middle, 110 high at the loudest), in the hand-built line's colour, above the page's tiles in the Pip-Boy's image, as the hotkey panel over ITEMS (§5.2.1). No flat line of its own: the file's centre line stands under it, and alone while the radio is off. |
| The hints | A Tune with a controller, none with the keyboard | `tab_buttons`. |

**What the code writes on the World Map** [design]: the model's (`pipboy::data::world_map`,
§5.3) through one viewport, `MapView` (`pipboy/data/view.rs`): the window the map is seen through
(the hand-built page's is the file's numbers; the tile page publishes the clip window as laid out,
`DataState::publish_map`, which the model reads for that tab while it stands), the picture's place
and size in it from the model's pan and zoom (centred on the player when the page comes up, a drag
moves the middle point, 1.25 a wheel step from 1 to 4, the file's limits on the picture's edges),
and the transform from texture coordinates to the window and back. The hand-built page and this one
paint from it, and the model's pointer is read in its window, so what is under the pointer is what
is drawn.

| Model (`pipboy::data`) | Tile, trait | Notes |
|---|---|---|
| The picture, its size at 1 (`MNAM`) and the magnification | `MM_WorldMap_ParentImage`: `filename`, `filewidth`, `fileheight`, `_Magnification`; `x`, `y`, `width`, `height` and `dragdeltax`/`dragdeltay` from `MapView`, put right on every pass | The place is written, not left to the file's expressions: they would read a drag NVForever does not feed. The children keep the file's hierarchy and texture coordinates, so a pan or a zoom moves the picture alone and the file's expressions place the rows on it. |
| The picture drawn | none in the tree: the page paints a piece of its own from the hotrect's traits as resolved (`map_menu::map_picture_piece`): its `filename` (the model's, or what a script wrote since), stretched over its place and size, at its `alpha` (128) and `brightness` in the colour its `systemcolor` above gives, at its depth (under its rows), cut to the map's window | The tile painter paints no hotrect's picture, and no hotrect with a `filename` is painted in general; the original's code draws this one in a way not known. Nothing is added to the game's tree. |
| The widths | the hotrect's `user0` 24, `user1` 56, `user2` 40 | The hand-built page's sizes (§5.3). |
| The markers the player knows (`known`), by reference | `MapMarkerTemplate` rows under the hotrect, kept by the marker's FormID: `_x`/`_y` (§5.3's projection, each marker by its own worldspace's numbers), `filename` (its icon, `model::marker_icon`), `_selected` (the marker the highlight box is on, the model's pick), `_LocationName`, `_MarkerIndex` | Made once and kept across frames and while the page goes down and up; a marker found or gone adds or unloads its row alone, and a frame whose markers did not change writes none of them (a pan or a zoom writes only the picture). The original makes them every update. |
| The active quest's targets, by reference; the custom marker | `WorldMapQuestMarkerTemplate` rows under the hotrect: `_x`/`_y`, `filename` (`glow_hud_compass_objective_marker.dds`; the custom marker's `glow_hud_compass_pc_marker.dds` at depth 5) | The custom marker from the quest markers' template [hypothesis: the capture shows it as wide as they are, §5.3]. |
| The dotted lines (#2054) | `WaypointTemplate` nodes under the hotrect at the arrow (named `WaypointStart`) and at each line's end (`WaypointIcon`); under the start node a `PathLineTemplate` line to each end: `user0` its length at a magnification of 1 (the window's units), `rotateangle` its direction (radians, from the window's x axis, y down); each line marked `OUTSIDE_CLIP`, so the map's window cuts it and its node's `clips` does not (#4685: the node's box had cut it to nothing outside the node) | As the hand-built lines (§5.3); one line per end from the arrow [hypothesis there]. The tile painter turns the line by `rotateangle` about the file's `rotateaxisx`/`rotateaxisy` (its left middle), clockwise with y down, as the hand-built line turns [hypothesis: the sense]. The nodes' and lines' size, `zoom`, `tile` and brightness are the file's (their brightness the arrow's, above). The original's line to the custom marker shows as small separate dots; its node read `width` 17.41, its line `height` 5.804, `rotateangle` 0.377 and -0.01 (radians) and `user0` the line's width plus 4 [observed: a capture of the original's World Map at 1280x720, 2026-11, heading 0]: radians and the length less 4 as here; its nodes are 0.8 of a marker width of 21.76 where NVForever gives 24 (`user0`, the hand-built page's), a known difference. |
| The player's arrow | `MM_WorldMapCursor`: `_x`/`_y`, `rotateangle` the heading and the picture's own angle (radians, clockwise; `data::arrow_turn`, §5.3), `visible` while the player is on this map; `brightness` 255 when it has none | Its place, size and `zoom` (its width × 3.5, which draws `glow_cursor.dds`, 32 × 64 texels with its arrow in the top half, at 1.4 times its texels cut to the box) are the file's; the tile painter turns it about its middle (the file's axis traits). The original's arrow is a sharp triangle [observed: a capture of the original's World Map at 1280x720, 2026-11, heading 0], as this crop draws it. The brightness: the waypoint nodes and the dotted lines copy the arrow's, which the file does not give; a trait a tile does not have reads 0 in the tree while the painter draws a tile without one at 255, so the code gives the arrow 255 while nothing else gave it one (else the copies draw black). |
| The highlight box, its name and a marker's reputation | `MM_MapHighlightBox`: `x`/`y` its middle on the model's highlight (in its own window's units: the map's window's offset from it added), `visible` 1, `_Title` the name and on a second line the reputation's (`model::reputation_line`) | The model's highlight: at the pointer over the map, else where it was left; on a marker within reach that marker's place [hypothesis there]. |
| The windows | `MM_WorldMap_ClipWindow/visible`, `MM_Highlight_ClipWindow/visible` 1; the clip windows turned on (`item_menu::mark_menu`) | Put right on every pass. |
| The place and the time | as on Quests | The shared headline writer. |
| The hints | X Place Marker (bright), A Travel with a controller (dim with no Travel target) | `tab_buttons`, by what each does. |

**What the code writes on the Local Map** [design]: as on the World Map where nothing is said,
into `MM_LocalMap_ClipWindow` (a clip window 50 down, 855 × 500, shown on tab 0),
`MM_LocalMap_ParentImage` (a hotrect with no picture in the file and hidden, "set in code"; its
children the markers) and `MM_LocalMapCursor` [documented: map_menu.xml]. `MapView`'s Local Map
projection is the hand-built page's (the player's place or where the map was dragged at the window's
middle, north up, 28 game units a unit outdoors and 8 indoors, magnified 1 to 4) with a frame: the
grid the outline covers (`outline::Outline`, its top left and size in game units), which the code
takes as the picture the rows' texture coordinates are of [design: the file has the rows placed by
`_x`/`_y` times the picture's size, the original's local picture's frame is not known].

| Model (`pipboy::data::local_map`) | Tile, trait | Notes |
|---|---|---|
| The frame, the magnification | `MM_LocalMap_ParentImage`: `x`, `y`, `width`, `height` from `MapView` (put right every pass), `filewidth`/`fileheight` the frame's size at 1, `_Magnification`, `visible` 1 (the file's 0: shown in code), `user0`-`user2` | The World Map's window hidden (`visible` 0) while the Local Map shows, and the other way round. |
| The outline's picture, once made | the hotrect's `filename` `nvf:local_map`, the runtime picture (`tiles::runtime_images`) given the outline's texture and its size in texels before the name is written; given again for each new outline (a new revision, so the painter repaints though the name stays); taken away when there is none; empty while the grid is being made | Painted from the hotrect's traits, outside the tree, as the World Map's picture. The outline is made anew when the map is dragged off it, the player goes elsewhere or in or out, and its picture once its grid is filled in (a few hundred rays a frame), so the picture comes as it fills in and is replaced as the place changes. |
| The doors leading elsewhere and the markers the player knows near (outdoors) | `MapMarkerTemplate` rows by reference: `_x`/`_y` (the frame's texture coordinates), `filename` (the door icon or the marker's), `_selected`, `_LocationName` | Every door of the cells read and every known marker of this worldspace, those outside the window too (its clip window cuts them), so a pan or a zoom keeps their rows; the hand-built page still draws only those inside the window and a marker's width more (`data::WorldParts::rows`). |
| The active quest's targets here, the custom marker in this space | `LocalMapQuestMarkerTemplate` rows by reference (the file's template keeps them inside the window), the custom marker's picture at depth 5 | No dotted line [observed: the original's Local Map has none, §5.3]. |
| The player's arrow | `MM_LocalMapCursor`: `_x`/`_y`, `rotateangle` the heading and the picture's own angle (`data::arrow_turn`), `visible` 1 (the file's 0) | Turned about its middle by the file's axis traits. |
| The highlight box | as on the World Map, its name a door's place name or a marker's | No reputation line. |

**The World Map's input** [design]: the tiles take the tabs and the hints (T8, as on the other
tabs); the map's drag, the wheel's zoom, the controller's sticks, the highlight box and picking
(the nearest marker within reach, a click a press and release with little movement), Enter and the
controller's A asking to travel stay the model's (`data::take_input`, `place_cursor`,
`travel_ask`), read in the window the tile page published, the hints' boxes as laid out excluded
(the pointer over a hint is the hint's, never the map's). So one owner starts travel.

**The question boxes** (fast travel's, Place Marker's) [design]: the model keeps the box
(`DataState::popup`) whatever draws the page; over the page drawn from the file the same box is
painted in the Pip-Boy's image above the page's tiles, in the middle of the map window as laid out
(`data::popup_over_tiles`), its answers taking the pointer as the hand-built box's do, spawned again
when its choice moves (which forgets the answer under the pointer, as the hand-built drawing does).
The page stays drawn from its tiles under it (no frame of the hand-built page). While it is up, and
after it is answered until the left button is let go, it owns the frame's tile input
(`MapTiles::popup_owns`, set by the tile input before it acts): the page's tiles take neither the
pointer nor the file's key bindings, so no click goes through it and no key acts twice. The
shell's keys are not held (`pipboy::takes_keys`): Tab and Esc close the Pip-Boy, the box going
with it, as on the hand-built page. Its own
keys and clicks are the model's (`popup_input`, which returns before anything else on the page
moves). A menu over the page is not used for it (`MapTiles::over_data` stops the model's input
altogether).

**Every string written, and where it comes from** [design: LEGAL.md]: the tabs as above; the place,
the quests', notes' and challenges' names and the challenges' descriptions (record data); a note's text (record data, `TNAM`) and picture's file (`XNAM`); the date and time and a
challenge's progress (figures); "Completed" and the audio's "remaining" (NVForever's own words, the
hand-built page's); the objectives' texts (record data, SetObjectiveText's); the hints' words (the
settings' texts, else NVForever's labels in the strings table, §5.3); their keys (`crate::pad::hint`).

**Input** (`map_menu::feed`): the pointer is `PipBoyScreen::pointer`, read and never written; with
Controller chosen as the Input Device it is ignored, as on the hand-built page. It takes nothing with
a menu over the page or another tile menu taking the frame. What a tile stands for goes where the
hand-built page's node sends it, through `DataState`'s acts. A row's act names its record (the
quest's, note's or challenge's FormID), and the page finds that record in its list as it is when it
takes the act, a frame of scripts later: a record gone meanwhile (a note filtered out, the list
toggled to the challenges) takes nothing, one moved is still the one clicked [design]:

| What | Sends |
|---|---|
| The pointer moves onto a quest's row | `HoverRecord(tab, form)`, taken as `Hover`: selected and boxed |
| The pointer leaves the selected row | `LeaveRecord(tab, form)`, taken as `Leave`: no longer boxed |
| A click on a row | `UseRecord(tab, form)`, taken as `Use`: selected and used (`use_row`: made the active quest) |
| A click on a hint | `Button(kind)`: the A hints do Enter's work; Show Location shows the World Map |
| A tab | `PipBoyInput::Tab(i)`; the pointer onto it, `TabHovered(i)` |
| The objectives' or the text's bar: arrow, page area | `Bar(Up/Down/PageUp/PageDown)`: the right pane's, a page the window as measured |
| That bar's marker held | `PaneTo(v)`: the pane's scroll where the file's bar puts it (`list::value_under`) |
| The tab's list's bar | the tile list's own scroll, no command of the model's |

The keys, the wheel and the sticks stay `pipboy::data`'s and `pipboy::keys`' (`HAND_BUILT_KEYS`): a
file's binding acts for a script's tap, not for a physical press, and never for a controller's
button (the file's X and Y would act twice with the page's own X and R).

**A menu over the page** (a message box, the tutorial menu; `MapTiles::over_data`, set before the
scripts, so it holds through the frame that menu closes in) holds the DATA page's own input on every
tab while the switch is on: its keys, the wheel, the test aid's steps and the pointer's acts that
frame are the menu's, so Enter answering a message box does not also make the selected quest active.
The page is still prepared [design]. Off, the hand-built page is as it was (it does not hold them; a
separate issue).

**Usable**: as §5.2.1: every tab's words shown whole, the tabs but the one showing taking the pointer
(one that does not is warned of and the page stays); the tab's list's rows whose words lie whole
inside the list's window, at least one when it has rows, taking the pointer at their middles; a list
that scrolls has a window; with a quest selected, the objectives' window (what their list clips to,
read off its scroll bar) has a size and lies wholly on the canvas, and a quest with objectives shows
at least one of them whole inside it among what is painted (a list or text hidden, by its own
`visible` or one above it, or placed outside the window, shows none); a text shown has its window
likewise (what clips `MM_DataText` itself: its rect and its bar are hotrects, which lay out no box,
and the bar shows only while the text scrolls [documented: scrollbar_vert.xml's `visible`]) and
some of its words painted inside it, and a text that scrolls shows its bar's marker; a picture is
painted whole on the canvas and inside what clips it; an audio note's time is painted whole; or the page falls back. The right
pane's window is published while it scrolls (the objectives, a text). A hint that shows lit and does
not take the pointer at its word is warned of and the page stays (its key still works); its key's
text (`PCShortcutLabel`) stands beside the hint's box [documented: text_box.xml] and is not asked.

**Falls back, whole page, with the reason** [design]:
`MapMenu` not in the tree; a tile or template above missing (a mod's restructured file), each tab
asking for its own (a file short of Misc's text still draws Quests from the file, and the other way
round); the tabs or a row not made; nothing prepared yet for this tab (no failure); the layout not
usable (on Radio also the waveform panel not shown, without a size or not wholly on the canvas: the
trace would have no box; on the World Map the map's window hidden, without a size or not wholly on
the canvas, its picture not painted inside it, or the highlight box not shown; a World Map without
a picture here; likewise on the Local Map with its window, a Local Map without a place). On the
World Map each of its four templates is asked for too, on the Local Map `MapMarkerTemplate` and
`LocalMapQuestMarkerTemplate`. The test aid
`NVF_PLAY_TEST_MAP_TILE_CUT=<name>[@<seconds>]` takes a tile or template as missing, and at its end
builds the menu again from its file, for the recovery (with the switch on only). After three failing
passes a missing name waits for the tree to change; a layout that is not usable is tried again
about every half second besides, as a place, `visible` or `target` can be put right without the
tree's shape changing.

**Log**: `tiles: MapMenu draws <tab> from tiles` and `tiles: MapMenu leaves <tab> hand-built: <reason>`
when that changes; `tiles: MapMenu page from tiles: tab quests, <n> rows, "<name>" selected, <m>
objectives, hints: <…>` or `… tab misc notes|challenges, <n> rows, "<name>" selected, text|picture|audio|no
pane, hints: <…>` or `… tab radio stations, <n> rows, "<name>" selected, waveform on|off, hints: <…>`
when that line changes (an audio note playing changes the time left every frame, not the line);
`pipboy data: waveform trace over the tiles at (<x>, <y>) <w> x <h>` when the trace is painted again
over the file's panel; `… tab world map, <n> markers, <m> quest markers, custom marker|no custom
marker, <k> dotted lines, "<name>"|"nothing" highlighted, magnification <x>, hints: <…>` when that
line changes; `pipboy data: question box over the tiles (<n> answers)` when the box is painted over
the page; `… tab local map, <n> markers, <m> quest markers, custom marker|no custom marker, picture
<w> x <h>|not made yet, "<name>" highlighted, magnification <x>, hints: <…>` when that line changes;
`tiles: MapMenu local map picture given, <w> x <h> texels|taken away` when the outline's runtime
picture changes; `tiles: MapMenu <tab> frames: <n> from tiles, <f> by hand after a failed pass, <u> by hand with
nothing prepared, <m> by hand as the Pip-Boy came up or went down` when a visit to a tab drawn from
the file ends (another tab, the page down), every frame of it counted by what drew it: a failed
pass (warned of only after three in a row), nothing to draw from with the Pip-Boy up (nothing
prepared, no menu files, no game: a fallback shown as surely), and the Pip-Boy rising or lowering
with nothing prepared yet (the preparation runs while it is open). A run shows a page that never
fell back with the Pip-Boy up by 0 in the second and third; the counts say nothing of pixels; `MapMenu drawn by hand: <reason>` (a warning)
for a failure; the painter's `tiles: MapMenu drawn from tiles (…)` and `set out for the Pip-Boy's
image: …` lines.

**Unit coverage given up** (#1512): the tests' fixture is written for them. It cannot prove how the
file's expressions read what is written: `_CurrentTab` and the lists' `visible`, the tabs' insides,
`_selected` and `_TextAlpha` in the row template, `_ItemType` and `MM_DataRect`'s `y`, the buttons'
`_y` and their dim look, the text's `y` by its bar, the audio's time and meter, or the file's own
places. The smoke scenarios `tiles-data*` check structure
and hits on the real file; their logs cannot prove pixels, only their shots can.

**Known differences behind the switch** [design]: the hints' hover box is the file's (`_box_visible`
on mouseover), which the hand-built page does not draw; the file's own hint rules are not used.
Radio's waveform: the original animates `MM_Waveform` (which animation, and whether its centre line
shows under it, is not known); here `MM_Waveform` stays hidden and the hand-built trace is painted
over the file's frame, its centre line standing under the trace while the radio plays (the
hand-built panel draws no flat line then). The World Map: the picture is a piece the page paints from
the file's hotrect's traits, outside the tree; the markers are kept, not made every update; the
arrow and the dotted lines turn by `rotateangle` as the tile painter reads it (radians, clockwise
with y down, about the file's `rotateaxisx`/`rotateaxisy` [hypothesis: the sense]); the question boxes are
NVForever's own box painted over the tiles (the original's box is another menu, not read here); the
reputation's line is the hand-built page's words; the Local Map's picture is NVForever's
outline of the collision (§5.3), not the original's picture of the place from above, and its frame
(the outline's grid) is NVForever's; the sizes `user0`/`user1`/`user2` are the
hand-built page's and do not change with the zoom (the file's comment says the arrow's does, by
what the code writes, not known).
**Not known**: what the original writes into `_TextAlpha`, `_ShowEmptyMarker` and the objectives'
scroll; whether the original's objectives scroll by rows, as a `list_box.xml` list's rows move by
`_current_value` × `_scroll_delta` [hypothesis: the file's expressions alone do not say what the
original's code writes there]; ours move by DATA's continuous pane.

## 6. Keys and pointer [hypothesis unless marked]

- Tab opens and closes; Esc closes. F1, F2, F3 show STATS, ITEMS, DATA; A/D and
  Left/Right the tabs [design]. On the controller LT/RT step the pages (round again)
  and the D-pad's left/right the tabs [design: owner 2026-11-02; controller.md §3]. ITEMS: Up/Down (W/S), Page Up/Down, the wheel, Enter or
  Space to equip, unequip or use [design]. A right click on a row drops one (a stack asks
  how many), Shift + right click the stack; it never equips or uses [design: owner
  2026-11-02, #1393]. X opens the mod menu on a weapon with mod slots and does nothing
  else (weapon-mods.md §4); the keyboard has no drop key. On the controller X drops one,
  the original's `IM_DropButton` button (`&xbuttonx;` [documented: inventory_menu.xml]):
  the PC's X for drop came from mixing up the controller's X with the keyboard's.
- The pointer is followed onto the model: on the screen, it is passed to the menus as
  a pointer of their own (Bevy's picking, as its `render_ui_to_texture` example does); a
  click on a button on the device shows its page. A tab and a row take clicks; the
  pointer on a row selects it, a click on the selected row acts on it.
- The pointer is found on the shapes as the arm's bones pose them, from CPU copies of
  the screen's and the buttons' meshes (nvforever `pipboy_arm.rs`, `Posed`) [design].
  Until 2026-10-11 Bevy's mesh ray cast was used, which sees neither the skinning nor a
  mesh kept on the GPU only, so no click on the model did anything (the owner's
  playtest). The pointer on a button does not light it (§2); a click still shows its
  page. `show_screen` logs "pipboy: buttons lit: <pages>" on each change.

## 6.1 Sounds [observed: the SOUN records, FalloutNV.esm; the owner's list 5]

At the listener: another tab `UIPipBoyTab` (`fx\ui\pipboy\ui_pipboy_tab.wav`); another
page (the device's buttons, F1–F3) `UIPipBoyMode` (`ui_pipboy_mode.wav`); the pointer
onto another tab of the tab line `UIMenuFocus` (`fx\ui\menu\ui_menu_focus.wav`), that
tab then boxed without a fill [hypothesis: the look]; the ITEMS rows sound as before
(`UIPipBoyScroll`, the same file); the Pip-Boy light (`crate::flashlight`)
`UIPipBoyLightOn` / `UIPipBoyLightOff`; while up, a static crackle `UIPipBoyStatic` (the
folder `fx\ui\pipboy\static\`, a file at random) 0.5–2 s after it is up and then every
4–10 s [hypothesis: how often the original plays them is not captured]. Coming up and
going down keep `UIPipBoyAccessUp` / `UIPipBoyAccessDown` (`crate::ui_sounds`). Each is
logged "pipboy sound: <EditorID>" (the focus sound at debug level).

## 7. Checking it

`NVF_PLAY_TEST_PIPBOY=<seconds>:<step>,...` with steps `open`, `close`,
`page=stats|items|data`, `tab=<0-4>`, `key=down|up|use|drop`, and for the pointer
`point=<x>:<y>` (the pointer on that canvas point, through the model's screen),
`point=off`, `click=<x>:<y>` and `click=stats|items|data` (a click on the device's
button; logged "pipboy: the STATS button pressed"), `placemarker` (the DATA maps' Place
Marker, as X; answer with `key=down` and `key=use`), `point=stats|items|data` (the pointer
on that button, no click; logged "test aid: the pointer on the STATS button"); screenshots with
`NVF_PLAY_TEST_SHOTS`. A new game's add-on message boxes come first: answer them with
`NVF_PLAY_TEST_SCRIPT_MENUS=0.05` and open after about 12 s:

```
NVF_PLAY_TEST_SCRIPT_MENUS=0.05 NVF_PLAY_TEST_PIPBOY="12:open,15:page=stats,17:close" NVF_PLAY_TEST_SHOTS="14,16.5" nvforever --data <Data> --cell Goodsprings --play --no-voice --hour 12 --size 1280x720 --run-seconds 19
```

Expect "pipboy: up, page ITEMS tab Weapons", "pipboy: the menus are on the model's
screen", "pipboy: the glare is over the menus", "pipboy: screen treatment on (…)",
"pipboy: STATS Status", "pipboy: down". Compare at 1280 × 720 with the owner's
pictures.

The mouse (2026-10-11): `NVF_PLAY_TEST_PIPBOY="12:open,14:click=320:650,16:click=stats,18:click=600:650"`
gives "pipboy: ITEMS Apparel" (the second tab), "pipboy: the STATS button pressed",
"pipboy: STATS Status" and "pipboy: STATS Perks"; `point=200:330` on ITEMS boxes the
fourth row, and a click on a row equips what it selects.

## 8. Open

[observed 2026-11-07, commit `4fbe5750d`, 8 alternating fresh launches, one machine] 4/4 clean first openings enabled and 4/4 blue or partial with `NVF_PLAY_TEST_NO_PIPBOY_WARM=1`. Screen-down medians 7.68 s enabled (7.38-7.98) and 6.81 s disabled (6.41-6.96): about +0.9 s in these launches. They support the combined fix.

[observed: 2026-11-07, commit `8fa3f8a04`, superseded by the set above] An earlier set of
eight alternating launches gave the same 4/4 and 4/4, with noisier screen-down medians
(8.84 s enabled, 8.27 s disabled).

[observed: earlier supplied `target/smoke/pipboy-first-open.log` and shots, 2026-11-07]
The failed first attempt reached its three-second startup cap and asked zero new
pipelines during the raise, yet its first three shots were blue or partial. Its
single enabled/disabled pair took 12.98/10.79 seconds on a busy machine; that pair
was not a repeatable startup-cost estimate.

[design: Bevy 0.20.1 registry `bevy_render/src/camera.rs:910-1028`,
`bevy_pbr/src/material.rs`, `prepass/mod.rs:1086,1376`] Pipeline compilation and
per-entity phase submission are different: a cached PSO can be ready while an entity
has no colour entry yet. Upstream's main pass skips an entity whose uploaded mesh or
allocator slabs are not there yet without keeping it for a retry. An earlier revision
of this fix patched both branches to retry and dropped pending entries no longer
visible; hidden and raising rig meshes also mark themselves changed so retained bins
are refreshed after visibility/material changes.

[observed: 2026-11-07, `pipboy-first-open`, `-no-warm`, `pipboy-screen-menu`,
`-unworn`, one machine, `bevy_pbr` rebuilt clean] With those retries and the visibility
filter reverted (the probe kept), all four scenarios pass, the first raise held 61 ms
for 0 pipelines and the first-open shots show the textured device with no blue or
world-filled hole, so the retry patch was dropped. The warm-up and the first raise's
own readiness wait are what the fix rests on.

[hypothesis] Depth without colour came from the rig's main entries being missing or
stale on the first raise. Shader depth rejection, incorrect GPU instance data, or a
draw-command skip remain possible if a report shows complete submission/bindings
while a shot is blue.

[design: `engine/nvforever/src/pipboy_arm.rs`, `pipboy_ready.rs`] The fix normally
waits for the observable submission conditions, with a bounded failure release;
those conditions are not proof of correct pixels.

[design: `engine/nvforever/src/player_body.rs:1441,2189`,
`engine/nvforever/src/pipboy_arm.rs::copy_of`, `::wear_materials`;
`engine/nvf-assets/src/nif_spawn.rs:720,779`, `texture.rs:312`, `model.rs:1260`]
Hiding the arms only changes visibility. Model construction requests its textures even
when hidden; worker textures are inserted independently of visibility. The first-person
material copies are made with the rig. Menu/glare copies are added on first substitution,
then `Swapped.menus`/`glass` retain strong source, image and copied-material handles
across teardown. An unchanged source and target image reuse the same asset IDs at the
real raise. A recreated screen image or rebuilt rig does make new copies. No extra
keep-alive cache is needed. Pip-Boy page pictures are a separate on-demand path
(`nvf-play/src/pipboy/frame.rs`, `device_screen.rs::draw`); drawing STATS warms that
path without opening an inventory.

[design: Bevy 0.20.1 registry `bevy_render/src/material_bind_groups.rs:2762-2840`,
`erased_render_asset.rs:372-451`; `engine/vendor/bevy_pbr/src/material.rs:598,1658`;
`engine/nvforever/src/world.rs:651-694`] Material preparation retries when an image is
not yet a GPU asset. NifSurface uses bindless allocation when supported; both passes use
the same prepared material and allocation, and the colour command additionally needs a
live bind group. Allocator creation itself has no multi-second timer. The upload budget
is 24 MiB/frame in play and unlimited behind the loading cover; materials have no byte
size budget, but their images and meshes do. Thus image preparation can delay a material,
without explaining this run's depth/colour asymmetry on its own. Readiness now checks
prepared material, actual allocation and main queue, rather than assuming a ready PSO
proves them.

[design: `engine/nvforever/src/pipboy_arm.rs`, `pipboy_ready.rs`, `player_body.rs`]
The start borrows the real rig and image camera for four render reports, with a 250 ms
wall-clock budget plus teardown. It does not wait for the global queue to go quiet.
The existing loading gate waits for the pipelines already requested. If the rig could
not be submitted in that budget it remains cold. The first real device raise verifies
submission again, behind the camera with CPU culling disabled, even following a start
warm-up. It releases only after its own main/prepass pipelines and material bindings
are ready, or releases at a three-second wall-clock cap with a warning that readiness
is incomplete. The cap allows cold preparation while bounding failure from a shader,
upload or lost rig; scheduling and pose restoration can add a frame. A released raise
does not hold again during that opening. Tab/Escape dispatch Close through the normal
key handler during the hold; cancellation skips the lowering clip. Later opens reuse
the prepared rig; a new rig/material resets readiness.
A missing rig keeps the existing flat-image fallback.
A screen-menu warm-up draws only its page/image camera; it neither borrows the arm nor
creates device treatment. Only the startup warm-up stays script-silent: a held real
opening reports its remembered
STATS/ITEMS/DATA page immediately. The device access sound waits until the borrowed
pose has been restored and the visible raise begins; ordinary openings, screen menus
and the disabled-warm path keep their opening-frame sound. A cancelled hidden opening
has no access sound.
The treatment's visible log is independent of when its layers were prepared.

[design: `tools/smoke/scenarios.toml`, `engine/nvforever/src/pipboy_ready.rs`]
Run `pipboy-first-open` and `pipboy-first-open-no-warm`, then the unchanged
`pipboy-screen-menu` and `pipboy-screen-menu-unworn`. `NVF_PLAY_TEST_PIPBOY_DRAW=1`
installs the logging system only for the aid. Its first visible render report is
marked only after pose restoration and gives
`shapes`, actual colour `queued`/`opaque`, `prepared`, `bound`, `ready`, `missing`, `gpu_meshes`, `slabs` and `specialized`.
`missing=0` with a nonzero shape count means every watched shape has a main entry,
an uploaded mesh and allocator slabs, a GPU material bind group and compiled
main/prepass PSOs. `queued < shapes` isolates
submission; `prepared < shapes` isolates material/image preparation; `bound < prepared`
is material allocation/binding. `gpu_meshes < shapes` or `slabs < shapes`
identifies mesh upload/allocation; `specialized < shapes` identifies a missing main
specialisation. With those prerequisites complete, `ready < bound` indicates compilation.
`missing=0` with a wrong shot rules out those prerequisites, and requires draw/GPU/depth
investigation. The log cannot certify pixels. The own-rig pipeline IDs come from its
actual queued main entries and prepass cache, excluding the world's. IDs are local to
each run. Both scenarios enable shader definitions and the submission report.

[design: `engine/nvforever/src/pipboy_ready.rs`, `engine/nvf-play/src/pipboy.rs`,
`ui_sounds.rs`, `tools/smoke/scenarios.toml`]
`pipboy-first-open` asserts that the first report after pose restoration has every
shape prepared, bound and uploaded (shapes still out of view aren't queued yet), that a
later report has at least 15 queued and all of those ready, and forbids the first-raise
cap warning. If the cap ever releases, a partial rig can show; that's the chosen
trade-off over a locked player.
Its shots should show a textured arm/device without blue, a world-filled hole or blur
on the arm; during a hold the world stays unobstructed. The disabled twin remains a
negative control. Unit checks cover Tab/Escape dispatch during a hold, bounded failure
release, remembered page reporting, pose cleanup, and access-sound deferral/cancellation.
Cold New Game, save load, screen menu, no device, interruption combinations and 16:9/4:3
still need runtime checks after this review revision.

- Whether the original holds the clip at `Hit` (the measurements say the screen faces
  the eye there).
- The keys of the PC game for the pages and tabs.
- The needle's and knobs' speeds in time (the capture counts game loops at no known rate),
  the wheel's step and sign, whether the mouse wheel turns it off the top of a list, how the
  knob swings back at the wrap, and whether anything moves down (§2).
- The glass: the static treatment (§3) is fitted at noon, level view, one page. Whether
  the original's screen moves with the hour, the pose and the page beyond what the grade
  does here is not fitted.
- `fDefault1stPersonFOV=55` for the arms outside the Pip-Boy.
- What `fBlurIntensityPipboy`, `fScanlineScalePipboy` and `fBrightenPipboy` scale.
- The mouse pointer: the screenshots show an amber arrow (`interface\icons\misc\
  glow_cursor.dds` is in the archives); NVForever now shows the game's own `cursor.dds`
  tinted amber everywhere in the window (`engine/nvf-play/src/cursor.rs`, 2026-10-16).

## Ammo effect card lines (#2502)

- [design, #2502] The shared item card reads AMMO RCIL in record order through the
  loaded `CombatForms.ammo_effects` (`engine/nvf-play/src/pipboy/items.rs`). Each
  resolved effect supplies one line below WG/VAL; no heading is added. Empty
  lists retain the previous WG/VAL-only card. Unresolved IDs and unsupported
  operations supply no line, rather than inventing a numeric meaning.
- [documented: fnvtool esm inventory, FalloutNV.esm, main session 2026-11-14,
  #2502 writer brief] All 54 base AMEF records have FULL. [design, #2502] Use that
  name verbatim when nonempty. For an absent or whitespace-only FULL, our own
  labels by kind are Damage, Damage resist, Damage threshold, Spread, Weapon wear,
  Fatigue; an unknown kind uses Ammo effect. These English keys use
  `fnv_util::words::tr` and `lang/{de,es,fr,it,ru}.toml`; FULL is never translated
  by this fallback table.
- [documented: offline GECK wiki Ammo Effect, revision 37444;
  `schemas/fnv/combat.json`; `crates/fnv-combat/src/damage.rs::apply_effects`]
  The operations are Add, Multiply, Subtract. [design, #2502, settled owner
  choice] Multiply displays `x0.80` (two decimals); Add displays its signed
  value, Subtract its negation (`+5`, `-5`, `-0.75`), with fractional digits
  only when needed. These lines describe the record, including fatigue effects;
  they do not alter combat.
- [design / file refs] `Info` supplies the same text to the hand-built inventory,
  barter and container cards and to `tiles/item_card.rs::write`, including the
  default tile ITEMS page. Hand-built cards get a row per line because their
  text helper draws one line. Tile cards use the multiline effect value and
  hide only its title text for ammo, saving its prior visibility on that title
  tile. Non-ammo and empty selections restore that value (including a hidden
  title), or remove the code-created visibility trait if the menu omitted it.
  Non-ammo selections before ammo leave the menu's title visibility untouched.
- [design / file refs] Synthetic tests `pipboy::items::tests::ammo_` cover all
  three operations, FULL/fallback labels, RCIL order, missing IDs and cards with
  and without effects. `tiles::item_card::tests::ammo_` checks the text, first
  free row, hidden heading and selection cleanup, including custom menus with
  initially hidden or absent title visibility.
- [documented: #2502 round-w2 writer brief, fnvtool esm decode, FalloutNV.esm,
  main session 2026-11-14] `Ammo357MagnumHollowPoint` (0x0013E43C) references
  AMEF 0x00157B54 ("Damage Bonus", Multiply 1.75) and 0x00157B56
  ("Dam. Thresh. Increase", Multiply 3.0); `Ammo357Magnum` (0x0008ED02)
  has no RCIL. [design, #2502] Smoke scenarios `pipboy-ammo-effects` and
  `tiles-items-ammo-effects` give both rounds and select each on ITEMS > Ammo,
  requiring `Damage Bonus x1.75` and `Dam. Thresh. Increase x3.00` in order,
  then no effects after returning to the ordinary round. The former uses the
  hand-built page; the latter uses the default tile menus without an override.
  [design / file refs] They read existing selected-card logs from
  `pipboy/items.rs::draw` and `tiles/inventory_menu.rs::describe`; screenshots
  accompany both selections for visual review. Log checks verify card text
  and selection cleanup; layout still needs the runtime screenshots.


### Replacement-art density and map-image dimensions

- [design] The hand-built STATS list, karma and reputation panels retain their
  existing 256-pixel logical image height and zoom. An actually selected own-file
  replacement with another pixel density uses the same density conversion already
  used by the fallback paperdoll: logical height divided by selected image height.
  The displayed geometry and normalized source region stay unchanged. Selected
  atlas pieces retain their existing texel contract; the size of a shadowed own
  file is not used. A missing atlas uses the density of its actual own-file fallback.
- [design] MapMenu publishes own-file image dimensions before evaluating image
  layout, using the same file-size writer as StatsMenu. This includes zero-size
  metadata children used by ratio-based map-marker templates. A replacement
  image's filename or zoom change refreshes its dimensions; cached decoded sizes
  do not substitute another image's dimensions. Atlas-backed and runtime images
  retain their existing metadata owners.
- [hypothesis] These repair two reported Windows QA boundaries: clipped high-
  resolution skill art when the Stats page falls back to the hand-built view, and
  ratio-based map markers whose child-image dimensions were never supplied. They
  do not establish original-game UI parity or repair a tile-page admission failure.
- [design] Authored CPU controls call the actual fallback page producer through
  GameData/VFS and inspect its ImageNode geometry/source rectangle, including TGA
  content under a DDS name, native atlas selection and missing-atlas fallback. A
  MapMenu page control reads authored DDS files through the same image provider
  and checks evaluated image dimensions, normalized crop, zoom and emitted pieces
  across a filename/density change. Execution results belong to their exact tested
  revision; source authoring alone supplies no runtime or screenshot acceptance.

- [design] The fallback-art controls explicitly choose General's karma or reputation
  mode instead of inheriting the default view. Positions and layout units stay exact;
  positive emitted dimensions may differ by at most one representable f32 step when
  an atlas subrectangle is multiplied back into canvas units. Normalized source
  regions and the selected atlas rectangle remain independent, unchanged checks.

### List-row `<clips>` under `<clipwindow>` (`StatsMenu`, `InventoryMenu`, `MapMenu`)

- [documented: GECK wiki `clipwindow`] In `list_box.xml` + `list_box_template.xml`,
  `<clipwindow> 1 </clipwindow>` on a list container defines the clipping window for
  descendants carrying `<clips> 1 </clips>`. On a list row (`stats_list_template_item`,
  `IM_InventoryListTemplateRect`, etc.), `<clips> 1 </clips>` opts that row into the
  list's `<clipwindow>` rather than cropping the row's own text children
  (`ListItemText`, value/modifier texts) to the row's `<hotrect>` height.
- [design] Because `fnv_ui::input::clips_children` retains self-clipping for legacy
  `<clips>` layouts, `tiles::list::unclip_row_children` marks live direct children of a
  `<clips>` row under a `<clipwindow>` list with `fnv_ui::input::OUTSIDE_CLIP` (the same
  mechanism `tiles::vats_menu` uses for right-justified queue rows). Both layout and
  pointer hit-testing then assign `parent.clip_above` — the enclosing list's
  `<clipwindow>` — to the row's text children. This prevents rows whose `ListItemText`
  vertical alignment offset exceeds `_VerticalSpacing` from failing `StatsMenu`
  readiness with `row 0 does not fit` (or `InventoryMenu` / `MapMenu` with `no row shows
  whole`) while keeping list-window clipping and off-canvas/narrow-window rejections
  intact.
- [hypothesis] Original-game clipping of nested grandchildren inside a `<clips>` list
  row and full installed-profile visual verification remain unqualified without an
  original-game capture.

### Restructured container & `<hotrect>` scrollbar fallbacks (`InventoryMenu`, `MapMenu`)

- [design] `InventoryMenu` and `MapMenu` resolve required and written tiles by exact
  vanilla path first (`Ui::find_tile`) so unmodded menu files behave identically, and
  fall back segment-by-segment to direct child or live descendant search under each
  enclosing container (`tiles::stats_menu::resolve_tile`) when a UI overhaul wraps a
  tile (such as `IM_ItemIcon`, `IM_ModButton`, or `MM_QuestObjectivesList`) inside an
  extra container `<rect>`. Scoping each segment to the previously resolved container
  keeps generic child names (`button_text`, `CI_TitleText`, scrollbar pieces) from
  matching sibling subtrees, and still rejects menus where a required tile is absent.
- [design] `InventoryMenu::built` checks `IM_InventoryListTemplate`'s
  `IM_Template_ItemMarker` as `<image>` first (vanilla), falling back to `<rect>` when
  a modded row template implements the equipped marker as a composite `<rect>` container
  (the adapter only writes `visible` on `IM_Template_ItemMarker`).
- [design] `tiles::layout::tile_box` evaluates a single live tile's laid-out rectangle,
  inherited clip window, and ancestor-propagated visibility using the same ancestor walk
  as `tiles::layout::layout`, including `<hotrect>` containers that `layout` does not
  emit as drawable pieces. `MapMenu::pane_window`, `MapMenu`'s scrolling text-scrollbar
  marker visibility check, and `tiles::list::marker_grab_on` read `tile_box` so modded
  `list_box.xml` / `scrollbar_vert.xml` layouts that declare `lb_scrollbar` or
  `scrollbar_vert_marker` as `<hotrect>` containers stay on the tile tree without
  falling back to hand-drawn pages.

