# Photo mode

A simple first photo mode, modelled on how Fallout 76's feels and is laid out (none of its
art) [design]. The code is `engine/nvf-play/src/photo_mode.rs`; its module notes are the full
description. This page is the summary.

## What it does

- **Open / close**: Photo Mode (a binding on the Controls tab under Actions, unbound by
  default: no key and no controller button until the player binds them) or the pause
  menu's Photo Mode entry, which is how it opens until then [design, #3634: the
  maintainer's decision]. Esc, the Photo Mode binding, or B on a controller close it. The view goes back to the player
  as it was (first or third person, the field of view).
- **The zoom is a layer on top** [design]. Without it the view stayed narrow after photo
  mode [observed: buckallobill, in game]. Each frame photo mode takes its zoom off the camera
  before the game's own writers of the field of view run (the sights' zoom, a
  conversation's zoom), and lays it on again after them. Those writers take any field of
  view they did not write for the unzoomed view, so before this they took photo mode's
  zoom as the gameplay view [observed: buckallobill, in game: a conversation's zoom eased
  out to vertical 22.7° under photo mode, and the close put back that zoomed angle]. The close
  leaves what the game's writers have on the camera: the view it opened on, or the
  gameplay view if a conversation ended meanwhile, or a field of view the settings set
  while open. The sights' zoom holds where it was while photo mode is open, so a view
  down the sights comes back down the sights. Photo mode never writes the field-of-view
  setting.
- **The view**: the HUD, the crosshair, quest lines and every other piece of the interface
  hide.
- **The camera** [design]: **Free** by default, not locked to the player. It looks with the mouse and flies anywhere up to 4000 units (about 57 m) from
  the player. That keeps it well inside the cells loaded round the player [design]. Walls
  stop it, and it slides along them. **Orbit** is the first version's camera: it circles
  a point that can move up to 200 units from the player's pivot, 30 to 320 units off.
- **The panel** (right side, in the HUD font and colour over a soft dark backdrop as dark as the game's menus over the world, 0.93) has
  three tabs with the options photo modes commonly have [design]:
  - **Camera**: Camera (Free or Orbit), Zoom (field of view, 10-120 degrees), Roll (-45 to
    45 degrees), and the depth of field: Focus (Auto, which focuses on what is under the
    screen's centre, or a distance up to 8000 units on a curve), Blur (Off to 100%).
  - **Look**: Brightness, Contrast, Saturation, Warmth (-100 cool to +100 warm), Vignette,
    Film Grain.
  - **Frame**: Letterbox (Off, 2.39:1, 1.85:1, 16:9, 4:3, 1:1; the bars are in the photo),
    Grid (rule of thirds; never in the photo), Hide Player (the body goes; its shadow
    stays).

  Everything is drawn by nvforever's image-space grade, so the photo is graded as the
  screen is. Brightness, contrast and saturation multiply the grade. Warmth, vignette,
  grain and the letterbox are a pass at the end of the grade shader. The depth of field
  is a lens blur of its own, bokeh [design; `engine/nvforever/src/bokeh.rs`]:
  each point is drawn as a disc as large as its circle of confusion through a thin lens
  focused at Focus and opened to Blur, near-white points spread as round bright discs,
  and a blurred background never spreads over a sharp subject in front of it. A blurred
  foreground spreads soft over what is behind it, as a layer laid over by how much of
  each point its discs cover, and in front of the focus a disc grows up to three times
  the largest one behind it, so a wall by the lens melts wide. It runs only while photo mode is open with
  Blur above Off; gameplay's depth of field (VATS, conversations, the modifiers) is the
  game's Gaussian one as before. Motion blur is off while photo mode is open.
  The panel can be hidden for a clean view. Every option resets when photo mode closes.
  Focus distances show in metres at 1.428 cm a unit [hypothesis: the Creation Kit wiki's
  figure for Bethesda's games of this engine family].
- **The photo**: the panel and every piece of the interface are hidden for one frame, and
  the window is captured at full size. The PNG goes to `<NVForever home>\Photos`,
  for example `NVForever 2026-11-10 01-23-45 Goodsprings.png`: the local date and time,
  then the place as the Pip-Boy names it (an interior's name, or outdoors the region's
  map name). Two photos in one second get " (2)". The file is written on its own thread.
  "Photo saved" (or "Photo not saved") is photo mode's own notice, low in the middle of
  the view for 2.5 s. The corner messages (items added, quests) wait, hidden and with
  their clocks stopped, until photo mode closes, as behind a menu.

## The game pauses

While photo mode is open the game pauses as under a menu (`PlayState::Menu`), and so does
the picture (the virtual clock, as `tfc 1` does).

## Controls [design]

Photo mode's keys and buttons are its own and fixed, but for opening it: Photo Mode is the
one binding, unbound by default on keyboard, mouse and controller (#3634). They are read
only while photo mode is open,
which holds the play controls, so they may be keys the play controls use too (Tab, H, R,
A on a controller).

| | Keyboard and mouse | Controller |
|---|---|---|
| Look (Orbit: circle) | mouse | right stick |
| Forward / back (Orbit: nearer / farther) | W / S | left stick up / down |
| Sideways | A / D | left stick left / right |
| Up / down | Space or E / Ctrl or Q | RT / LT |
| Zoom | mouse wheel | the Zoom slider |
| Change the tab | Tab / Shift+Tab | RB / LB |
| Choose / change an option | Up, Down / Left, Right (a slider held repeats) | D-pad |
| Take the photo | Enter or left click | A |
| Hide the panel | H | Y |
| Reset the options | R | X |
| Close | Esc or the Photo Mode binding | B (Start) |

Shift moves the camera three times faster.

## Lights [design]

The **Lights** tab places up to 4 coloured lights for the shot, as other games' photo mode
lights do [design: mechanics only; nothing of any game's art or assets is used]. The settings and state are in `nvf-play/src/photo_lights.rs`; nvforever draws them as
the engine's dynamic point and spot lights, with the game's attenuation
(`nvforever/src/photo_lights.rs`, `lights.rs`).

- **Rows** (they scroll, six at a time): Light (which one the rows set), Add Light, Move &
  Aim, On, Type (Point or Spot), Hue (0-360), Saturation, Brightness, Range, Width (the
  spot's full cone, 6° tight spot to 170° flood), Direction and Tilt (heading and pitch),
  Shadows, Pin to Camera, Remove Light. Left/Right change a row or press an action.
- **Colour**: the Hue slider goes round the colour circle and Saturation mixes it to white
  (HSV with the value at full). There is no colour wheel [design].
- **Placing**: Add Light drops a new light where the camera is, aimed along the view, and
  starts editing it. Nothing points a light at the camera on its own afterwards; the
  player sets its direction.
- **Editing** (Move & Aim, or a new light): the photo camera stands still, and its inputs
  drive the light instead. The mouse or right stick aims it, WASD or the left stick move
  it (level with the ground along the camera's heading), Space/E and Ctrl/Q or the
  triggers raise and lower it, and the wheel, [ ] or LB/RB narrow and widen it. Esc, B or
  the right mouse button (or Move & Aim again) end editing. The camera is then put back
  exactly where it stood when editing began, from a kept copy of its place and view.
- **Pin to Camera** is an option, off by default. A pinned light rides at the camera, and
  its direction is kept relative to the view. Unpinning sets it down at the camera,
  pointing where it pointed.
- **Markers**: a ball in the light's colour and an arrow along its beam for every light,
  and the cone's rim for the chosen spot. They are drawn through walls while the Lights tab
  is shown or a light is edited, and never in the photo.
- **Shadows** use Bevy's shadow maps (a cube map per point light, one map per spot), and
  carry the sun's render layers so the player's body casts its shadow.
- The lights exist only while photo mode is open. They are removed when it closes and are
  never saved.

## Not in this version

Frames, filters, poses, time of day.

## Tests

Unit tests in `photo_mode.rs` cover the options (defaults, range, steps, choices going
round, reset, text, tabs, what reaches the grade), the
FOV conversion, the file name and the place's safe form, unique names, the capture's pixel
order, PNG writing, the test aid, the pause and the fixed keys. The smoke scenario `photo-mode`
(`NVF_PLAY_TEST_PHOTO`) opens it alone, moves the camera, sets two sliders, saves a photo
and closes it; `photo-mode-options` flies the free camera and turns the new options on;
`photo-mode-lights` adds, colours, moves, aims and widens two lights with the camera held,
saves a photo and checks the lights go on close. Unit tests in `nvf-play/src/photo_lights.rs`
cover hue/saturation to colour, ranges, the light limit, input routing while editing, the
camera put back, pinning and clearing.
