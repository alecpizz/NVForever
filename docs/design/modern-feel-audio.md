# Modern Feel: layered gunshots and sound occlusion

Two parts of the fork's Modern Feel experiment (`engine/nvf-play/src/modern_feel.rs`,
NVF Features › Modern Feel): **Layered gunshots** (`ModernFeel::gun_audio()`) and
**Sound occlusion** (`ModernFeel::sound_occlusion()`). Both are NVForever's own design,
none of it in the original; every constant below is `[design]`, an engineering choice
tuned by ear and by looking at rendered envelopes. No sound file is shipped: the shots are
the game's own sounds shaped by DSP.

## Layered gunshots (`engine/nvf-play/src/gun_audio.rs`)

**What.** Every weapon shot, the player's and other actors', plays with a tail by the place
it is heard in, and another actor's shot is shaped by its distance.

- **Outside, open air** (no environment, Mountains, Plain, Quarry, Forest …): a rolling
  echo, nine delayed copies of the shot between 0.18 s and 1.45 s (spaced closer early,
  jittered per shot), each quieter (falling over 0.55 s from 0.36 of the shot) and duller
  (one-pole low-pass from 3 kHz down to 700 Hz), alternating left and right, then two short
  all-passes per ear (11/27 ms and 14/31 ms, gain 0.55/0.5) smear them so they roll rather
  than click.
- **Outside among buildings** (the City, Alley, Parkinglot environments): a slap (eight
  reflections 18–120 ms, 6.5 → 2.8 kHz, their power summing to 0.18 of the shot's) and
  the echo at 0.22.
- **Inside** (an interior cell): the slap only, its power 0.2 of the shot's, its times
  scaled by the room reverb's decay (÷ 1.5 s, held to 0.6–1.6). The slap's taps overlap
  the shot, so they are scaled by power, not amplitude (eight taps at 0.5 each tripled a
  room's mix peak). The acoustic space's I3DL2 reverb (`crate::reverb`) still follows the
  whole.
- **Automatic fire**: a shot of the same sound within 0.25 s of the last keeps a quarter
  of its tail (the whole burst's tails overlapping drove a room's mix into the limiter:
  pre-limiter peak 2.2 against 1.4 without the part; now 1.6). The burst's end still
  rings with the last shot's.
- **Distance** (another actor's shot, `d` units off):
  - air absorption, a one-pole low-pass at 20 kHz ÷ (1 + (d − 300) / 1200), at least
    1.6 kHz (about 10 kHz at 1,600 units, 4 kHz at 5,000);
  - the tail louder against the direct sound with distance: × (1 + d / reach), at most × 2
    (reach: the 3D sound's maximum distance);
  - the **distant layer**: from 40 % of the 3D shoot sound's reach to the reach, the
    weapon's own distant shot (WEAP Shoot Dist) plays under the 3D one, rising by a
    smoothstep to 0.85 (its record's attenuation on top), dry; beyond the reach the
    distant sound alone, as before. So a mid-range shot sounds layered, not just quieter.
- **The player's own shot**: a low shelf (one-pole at 140 Hz, + 0.45 of the low band) and a
  trim of × 0.8, so it has weight without being louder overall (rendered 10mm pistol: the
  first 0.5 s within 1 dB of the dry RMS).
- **Silenced** shots (a Silence or Suppressor mod's sound set) keep 12 % of the tail.

**How.** `combat.rs` marks the sound a shot asks for (`gun_audio::mark_shot`: sound, place,
player or not, silenced, the 3D sound's reach); `sfx.rs` takes the mark when that sound
starts (`gun_audio::take`, in `one_shot_ready`, so a shot whose file is still being read or
that waits for a voice slot still gets it) and gives the playing's own `PcmSound` a
`ShotShape`; its decoder runs `ShotDsp` after the pan and before the reverb, and its tail
rings out after the file. Marks not taken within 0.6 s are dropped. Switched off, no mark
is made: new shots play as recorded, tails already ringing finish.

**Log.** `gun audio: player|npc shot <SOUN> …: open (rolling echo) | street (slap + echo) |
room (slap) | distant layer (dry), tail …, air … Hz[, punch][, silenced]` (at most one line
per 0.4 s), and `gun audio summary: …` at exit.

**Cost.** Per shot voice a mono delay line as long as the longest echo (≈ 1.5 s, 280 KB
at 48 kHz, allocated as it starts) and per sample about seventeen taps and filters; a
burst of 8 voices is well under 1 % of a core. Nothing on the main thread but a mark.

**Checked.** The render test `gun_audio::tests::render_shot` (ignored; `NVF_SHOT_IN` a
WAV, `NVF_SHOT_OUT` a path prefix, optional `NVF_SHOT_REVERB=<environment>` and
`NVF_SHOT_BURST=<shots>:<seconds apart>`) writes a shot through each shaping. On the
10mm pistol's 2D shot (100 ms RMS, dB): dry falls −7 … −45 by 1 s; open air holds a
rolling tail −16 … −30 from 0.5 to 1.5 s and dies by 2.2 s; at 3,000 units the tail is
about 4 dB higher; the room's slap lifts 0.3–0.6 s by about 4 dB and is gone by 0.9 s;
silenced stays within a few dB of dry. Single shots' peaks are unchanged (0.86 → 0.86–0.95).
In game (smoke, muted): `modern-feel-gun-audio-outdoors` (Violet at 1,500–2,700 units: tail
× 1.6–2.0, air 6.7–10 kHz, distant layers under her mid-range shots),
`modern-feel-gun-audio-room`, `modern-feel-gun-audio-off` (both rows switched off at run
time: no shot shaped, no ray cast); `auto-fire-burst-sound` and `audio-minigun-declick`
still pass with the part on (their mixes never reach the limiter).

**Rough.** Automatic fire inside still lifts the mix before the limiter (1.6 against 1.4)
more than the rendered bursts say it should (no rise offline, reverb and nine overlapping
shots included); not found why (voice-slot timing at a smoke run's low frame rate is the
suspect). The voice limit of 8 of one SOUN (`sfx::MAX_SAME`) fades out the oldest shot
with its tail. The space is the listener's,
not the shooter's. The distant layer uses the 3D sound's reach as the original's switch
to the distant sound does.

## Sound occlusion (`engine/nvf-play/src/occlusion.rs`)

**What.** A sound played at a place with walls, floors or hills between it and the listener
is muffled and quieter: per blocker −6.5 dB (at most −16 dB) and a low-pass falling from
open to 1 kHz at one blocker and 600 Hz at two or more.

- **Which sounds**: every sound with a `SoundAt` (sfx's placed one-shots: shots, impacts,
  footsteps, creatures, doors; radios) and ambient's placed loops (`Ambient::placed`). Not
  sounds at the listener, nor voices: the conversation is played unplaced, so the person
  you talk to is never muffled (barks are not occluded either).
- **The test**: one ray from the player's eye to the sound (`PhysWorld::queries().ray_all`),
  counting the distinct colliders it crosses: terrain, statics, animated statics (doors),
  ground; not actors, moving bodies, clutter, trees, glass, triggers. Hits within 24 units
  of either end are not counted (the sound's own object). Sounds within 160 units are not
  tested.
- **When**: a one-shot as it starts, in the frame it is spawned, before the audio output
  takes it up, so its first sample is already muffled. Loops and long sounds again every
  0.25 s, eased over 0.15 s on the main thread and over 60 ms per unit in the decoder.
  At most 12 starting sounds and 8 re-checks cast a ray in a frame; a start past the budget
  or near one tested in the last 0.5 s from about the same place (96-unit squares for the
  sound and the eye) takes the cached answer (a crowd's footsteps).
- **The filter** (`occlusion::Muffle`) runs in the placed sound's pan stage
  (`sfx::pan_frame`), shared by every placed decoder (sfx's `PcmDecoder`, `Panned<T>` for
  ambient loops and radios), on the occlusion carried by the sound's `Pan` (a third atomic).
  Clear, it passes the sample untouched.

**Log.** `occlusion: a sound N units off behind K blockers: low-pass … Hz, … dB` (at most
twice a second) and `occlusion summary: …` at exit.

**Cost.** At most 20 rays a frame (each a BVH walk, a few µs), usually far fewer; per
occluded sound one one-pole filter per sample.

**Rough.** One collider counts once however many of its walls the ray crosses (a building
of one mesh is one blocker). Moving ambient sources are tested where they are at each
re-check. No diffraction: a sound just round a corner is muffled at once.

## Shared-file edits (for merging)

- `engine/nvf-play/src/lib.rs`: two module lines.
- `engine/nvf-play/src/sfx.rs`: `PcmSound`/`PcmDecoder` carry the shot shaping; `Pan` holds
  the occlusion; `PanEase` runs the `Muffle`; `one_shot_ready` takes the mark; the plugin
  calls `gun_audio::build` and `occlusion::build`.
- `engine/nvf-play/src/combat.rs`: the marks at the player's and NPCs' shot sounds, the
  distant layer.
- `engine/nvf-play/src/ambient.rs`: placed loops keep their place (`Playing::at`,
  `Ambient::placed`).
- `tools/smoke/scenarios.toml`: two scenarios.
