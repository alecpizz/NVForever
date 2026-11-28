# Tonemapping presets (#4690)

Editable looks for the main view's tonemapping: a preset is a few values laid on the view's
tonemapper curve, picked on the settings page, edited live in a panel during play and shared
as one small file. An optional improvement over the original, not the original game's
behaviour [design, #4690]. Off by default: the built-in **Default** preset changes nothing, so
every rendering mode keeps its shipped look.

## The curve and where a preset acts

The main view's curve is one of two today (nvforever lights.rs `ToneCurve`):

- **Clipped**: the display-value look (the original's lighting), the light clipped at white
  (`GAME_CLIP`; `GAME_HDR` with HDR output, docs/design/hdr-output.md).
- **Tony McMapface**: Bevy's curve, under Physical lighting (graphics-enhanced.md §3.25).

A preset's values are applied in Bevy's own colour grading on the world camera
(`bevy::render::view::ColorGrading`), which the tonemapping pass runs before the curve
[documented: engine/vendor/bevy_core_pipeline/src/tonemapping.wesl `tone_mapping`]. So the
same values work on either curve, in SDR and in HDR output alike, and the eye adaptation
still applies: Bevy's auto exposure adds its own exposure to the grading's
[documented: engine/vendor/bevy_post_process/src/auto_exposure/auto_exposure.wesl, the last
line, `view.color_grading.exposure += exposure`]. Default is Bevy's default grading, which
the camera already carries, so the shipped look is unchanged to the bit.

Other views (the Pip-Boy, the scopes' lenses, the minigames) tonemap linearly and take no
preset.

## The values (stage 1)

| Value | Range | Default | What it does |
|---|---|---|---|
| `exposure` | -3 … +3 EV | 0 | Exposure compensation, before the curve: `ColorGrading.global.exposure` |
| `contrast` | 0.5 … 1.5 | 1 | Contrast of shadows, midtones and highlights alike (`ColorGradingSection::contrast`) |
| `highlights` | 0 … 1 | 0 | Highlight roll-off: the highlights' gain lowered by up to half (`highlights.gain = 1 - 0.5 x`), so bright light reaches the curve's shoulder later |

All three are `[design]`. A choice of curve (AgX, others) with its own controls comes later
(below); each mode keeps its own curve until then.

Display calibration is not part of a preset: HDR output's paper white and peak brightness
stay the display's own settings (hdr-output.md, Calibration), so a shared preset never
carries one screen's calibration to another.

## The file

One preset per file, in `<NVForever home>\Tonemapping\<name>.toml` (docs/design/install-layout.md;
never the game's folders), human-readable and editable by hand:

```toml
# NVForever tonemapping preset (docs/design/tonemapping-presets.md)
format = 1
name = "Warm dusk"
exposure = 0.3
contrast = 1.1
highlights = 0.2
```

- `format` is 1; a file of a newer format is listed as unreadable, not guessed at.
- A value out of its range is held to it; a missing value takes Default's.
- A file that does not parse is left out of the list, with a log line naming it and why.
- `Default` is built in, never a file; a file named Default is left out.

The active preset is the settings row `tonemap-preset` (Graphics › Lighting and Effects),
kept in `NVForever.ini` by its name, so the look comes back after a restart. A name no
longer on disk falls back to Default.

## Stages

1. **The model and the file** (this page's values): presets read from the folder, the
   settings row picking one, applied live without a restart.
2. **The panel**: opened from the settings page during play, over the live scene: pick,
   duplicate, edit with sliders, compare with the preset as last saved, cancel, save under
   a name, back to Default. Laid out as photo mode's panel is (docs/design/photo-mode.md).
3. **Import and export**: one preset file in, one out, through the system's file dialog. An
   import that fails or whose name is taken keeps the current look and saved presets as
   they are and says what happened.

Later: a choice of curve per preset (AgX under Physical lighting; Tony McMapface in the
display-value look) and the controls each curve offers.
