# The volumetric medium (#3032)

[design] An addition of NVForever's own, an engineering choice for the enhanced look: not a
fact about the original game (which draws its weather's clouds as flat layers on the sky
dome), never a parity item, and never to be used to explain a
difference from the original's frames. The clouds are switched by the row "Volumetric clouds
(experimental)" (`--graphics-only volumetric-clouds`), the sunlight in the air by "Volumetric light
(experimental)" (`--graphics-only volumetric-light`), each alone, off by default and in every preset.
Code: `engine/nvforever/src/medium.rs`, `medium_trace.wesl`, `medium_resolve.wesl`,
`medium_upsample.wesl`; the cloud source in `volumetric_clouds.rs` and
`volumetric_clouds_march.wesl` (graphics-enhanced.md §3.23); the light in the air in `medium_air*`.

## What it is, and why one system

Clouds, sunlight in the air, wind-blown dust and the weather's haze are all light scattered
and absorbed along the view ray. Built as separate effects, each would march the same air,
count the same light twice where they overlap, and disagree at their edges (a shaft of light
that ignores the cloud it passes, haze that is drawn over a cloud it should be behind). One
medium gathers the light of every source along each ray once, and the picture is composed
from it once. **Built so far are the clouds and the sunlight in the air near the camera**, each on its
own; dust and haze are later changes. The published technique is Bauer's "Creating the Atmospheric World of Red Dead
Redemption 2: A Complete and Integrated Solution" (SIGGRAPH 2019, Advances in Real-Time
Rendering in Games) [documented: Bauer 2019, slides 42-56]; the one-ray-in-several update is
also in Schneider's "The Real-Time Volumetric Cloudscapes of Horizon Zero Dawn" (SIGGRAPH
2015). Everything here is written from the talks' descriptions; no code from any project.

The talk has two representations [documented: Bauer 2019, slides 42-49]: a froxel grid near
the camera (lit and integrated slice by slice) and a ray march beyond it. The far representation
is the clouds'; a first grid is built for the light in the air alone ("The near grid and the light in
the air", below).

## The far buffer as built

**Where in the frame.** Two passes between the depth prepass and the main pass (where the
ambient occlusion's lighting pass sits), so the main pass reads a finished buffer; the picture
is composed in the main pass by the sky proxy (the dome's own mesh drawn again, depth test
Equal, in the sky's blended band, premultiplied over the dome, the stars and the moon). So
the clouds are composed in the linear HDR picture before tonemapping and before TAA, FSR and
DLSS, which treat them as sky (FSR's reactive reference draws the same proxy from the same
buffer). The row keeps the main view HDR (graphics.rs). Stage 1 is sky only: the rays are not
cut by the scene's depth, so the buffer has no depth discontinuities and the reconstruction
has no depth rejection yet; the weighted distance is kept for the stages that apply the
medium to the world.

**Sizes and formats.** The published image is a share of the main pass's render size in each
direction (the row's level: a third at Normal, two fifths at High, a quarter at Performance, "The
levels" below; half until 2026-11-23), rounded up (the render size is the upscalers' when FSR or DLSS draws:
`MainPassResolutionOverride`), `Rgba16Float`: rgb the light scattered toward the camera,
already weighted by what it hides (premultiplied, linear, the weather's colours times the sky
level), a the transmittance (1: nothing in the way). The divisor is a parameter, the scale
(`max(1, ceil(render / scale))` per axis, the level's 3, 2.5 or 4;
`NVF_PLAY_DEBUG_MEDIUM_SCALE=<1..4>` whatever the level) [design, #3032 stage 2a]. The
trace target is a third of that again
(one texel per 3x3 tile; `ceil(published / side)` for a tile side of 1 to 3): light `Rgba16Float` and the transmittance-weighted distance
`R32Float` (the cloud source's range, hundreds of thousands of units, is beyond half floats). Two history sets (light
`Rgba16Float`, distance and the cast count with the age `Rg32Float`) alternate. At scale 2 and side 2, 2560x1440: 37.8 MiB; 1920x1080:
21.3 MiB; at High 23.3 and 13.1 MiB [design: the sizes follow from the formats]. The published image is a stable
`Handle<Image>` the proxy's material binds, made again (a new handle) when the size changes,
since a material's bind group keeps the texture view it was made with.

**The levels** [design, 2026-11-23]. The row is a list: Off, Performance,
Normal, High (`volumetric_clouds.rs` `CloudLevel`; `--graphics-only volumetric-clouds` and an INI
written while it was a switch are Normal). High is the look judged by eye on 2026-11-23; Normal and
Performance differ in two things only:

| Level | Buffer scale | Probe spacing |
|---|---|---|
| High | 2.5 | as designed (800 or 1,600 units with the domain) |
| Normal | 3 | twice that |
| Performance | 4 | twice that |

The tile side stays 3 and the fine step's rule the same (a longer one pixelated in motion
[design: judged by eye in a play test, 2026-11-23]); what follows the cast ray's footprint
(`ray_spread`: the noise levels, the fine step far off) follows the buffer's texel, as with `NVF_PLAY_DEBUG_MEDIUM_SCALE`. The spacing's
factor is in the uniform (`profile.z`, `coarse_spacing` times `VolumeInputs::probe`); the march's text
does not know the levels. The block cap (16 cells) changes nothing near the camera: the near block is 8
cells of 120 units at High and exactly 16 at twice the spacing (4 and 8 in thin weather), and four
halvings bracket 16. Where the place width widens the spacing, the cap is met sooner than at High, so
a far block may be less than twice as long: more probes, never fewer, and it is the measured
prototype's own rule, which doubled the spacing in the shader under the same cap. The misses are the
refined march's own transect figures at twice the spacing (graphics-enhanced.md §3.23: 1.55 % of the
mass chords at amount 0.05, 1.66 % at 0.3; the baseline, High's spacing, 0.41 % and 0.45 %
[hypothesis: counted on a numerical mirror of the placement before the banks, not committed, and not
repeated on the bank field]).

A live change of the level is a change of the size the render world wants for the same epoch: one
hand-back (`a new far buffer is being made`), a new image and a new history, as for a render size
change; the old image goes when the material takes the new one. Log: `volumetric clouds: level <name>
(far buffer scale <s>, probe spacing x<f>)`.

Cost of the trace on an RTX 4080 SUPER at 1080p, fair at 60:20 / Zion at 150:18, its worst view [observed,
2026-11-23: the set-up of graphics-enhanced.md §3.23 ("The set-up of the cost figures"), the flat march with
the ceiling clip as edited shaders on c5a6c2993, the scales by `NVF_PLAY_DEBUG_MEDIUM_SCALE`, the spacing by
an edit, one run each]: High, the baseline of the others, 1.25 / 2.52 ms; scale 3 alone
1.07 / 1.97; scale 4 alone 0.99 / 1.32; twice the spacing alone 1.17 / 2.32. The levels as built
(99c5df603; `--graphics-only` `anisotropic,volumetric-clouds-high`, `anisotropic,volumetric-clouds` and
`anisotropic,volumetric-clouds-performance`): High, the baseline, 1.28 / 2.59; Normal 1.07 / 1.90;
Performance 1.01 / 1.32 [observed, 2026-11-23: the same set-up, one run each].

**The cast pattern** [documented: Bauer 2019, slides 51-54; our own form], as built at side 2 (the
default until 2026-11-23; side 3, the default now, casts each of a tile's nine texels once in nine
frames). Each frame one ray per 2x2 tile is cast: the tile's corners in the order (0,0), (1,0), (1,1), (0,1), the index the
frame's phase plus one on every other tile in a checkerboard, so neighbouring tiles never cast
the same corner in the same frame and every texel is cast once in four frames; a tile cut by
the buffer's edge cycles through the corners it has (a 1x1 buffer casts its one texel every
frame). The phase advances only on a whole reconstruction. Every tile is traced: the talk's
placement by the depth's minimum and maximum (slide 55) is not needed while the buffer is sky
only. The march's start offset is the R2 low-discrepancy sequence over the texel (Roberts,
"The Unreasonable Effectiveness of Quasirandom Sequences", 2018), turned by the golden ratio
at each of the texel's casts, so the history averages different offsets. No noise texture is
shipped.

**The tile side** is a parameter (3 by default; `NVF_PLAY_DEBUG_MEDIUM_TILE=<1|2|3>`) [design,
#3032 stage 2a]. At side 1 every texel is cast every frame and still goes through the history
(the running mean, the floor share and the clamp in force, as above): its nearest cast ray is
always its own, its age 0, and its start offset turns at every cast, modulo 64 (the phase's
upload modulo 256 holds that period too). The shaders take the side from the uniforms (the trace's
`tiles.z`, the reconstruction's `pattern.x`), so every side runs the same pipelines; side 2 is the
pattern above to the bit (a test compares it with stage 1's corners and offsets, edges and the
phase's wrap included). The 3x3 neighbourhood of the clamp is 3x3 tiles at either side (3x3
texels at side 1), and the motion ramp is in texels of the buffer, so another scale or side
changes the clamp's footprint and the ramp's angular meaning: judge those settings on their own.

**The ray.** Origin the camera, a unit direction through the texel's centre from the view's
projection **without** the upscalers' jitter (camera-relative, so nothing is lost far from
the world's origin), and the stretch `[t_min, t_max]` the medium owns (0 to the source's range
in this stage).

**The reconstruction** [documented: Bauer 2019, slide 52, adapted]. For each texel of the
published image:
- the box of the light and transmittance of the fresh rays of the 3x3 tiles around it;
- the cast ray nearest it among the 3x3 tiles (its own tile's on a tie): its distance seeds the
  history's lookup;
- its history: the point on its view ray at the distance the nearest cast ray found (by
  direction alone when that ray met nothing) is moved into last frame's view (last frame's
  unjittered projection and camera, camera-relative with the translation between the frames
  added) and last frame's buffer is read there, bilinear; behind last frame's camera or off
  its screen there is none; the history is held to this frame's rays by the clamp in force
  (below);
- a texel cast this frame takes its ray, blended with its history: **where we differ from the
  talk** [engineering choice]: the talk dithers and leans on TAA after the reconstruction
  (slide 48), but our default anti-aliasing is multisampling with no temporal pass after it, so
  a fresh ray takes a share of 0.1, still and moving (`NVF_PLAY_DEBUG_MEDIUM_FRESH`,
  `NVF_PLAY_DEBUG_MEDIUM_FRESH_MOVING`), as a floor under a running mean of the texel's own
  casts: share = max(floor, 1 / (max(count, 1) + 1)), the count being the texel's own casts since its
  history became valid (0 for a texel filled from the rays around or with none), carried with the
  history like the distance (nearest) and held at 255; so the first own cast after a cut
  averages with the fill (below), the two then weigh as one cast, and from the ninth the floor
  decides [engineering choice]. Where the step (share x (ray - history)) is smaller
  than the half float's spacing at the history's value or equal to it (2^-11 of it, the lower
  bound within a binade; 2^-24 below the normal range; a blend on the midpoint between two
  half floats rounds back to the even one) the blend may store the history again, so that
  channel alone moves toward the ray by the gap or twice that spacing, whichever is smaller
  (one or two stored steps), never past it. The threshold is conservative: it also catches
  some blends that would have stored one step away, and those then move two (history 0.5 under
  a ray of 0.49755859375 blends to exactly one step below and is nudged two); the cast is taken whole
  (light, transmittance and distance) only when the history is empty, the ray sees cloud and
  the blend would still store as empty (a thin cloud with stored transmittance 0.998046875 over
  empty history would otherwise blend to 0.9998, store as empty sky and never appear); the
  empty rule then applies as before. A first version took the whole ray whenever the
  transmittance stalled, which is the ordinary case where the transmittance is steady from ray
  to ray and the light is not, and brought the shimmer back (the figures below);
- a skipped texel with history takes the clamped history, its cast count, its age plus one
  (held at 15), and its distance
  carried into this frame's camera: the point on the history texel's own ray (last frame's
  camera) at its distance, moved by the camera's displacement, measured from here; a history
  clamped to empty sky gets the empty distance back;
- with no valid history, the fresh rays of the 3x3 tiles weighed by distance (next).

**The fill and the clip weigh the rays** [engineering choice: blocks where new cloud
enters the screen in a turn, and at a cut, judged by eye in a play test, 2026-11-23]. A texel with no valid history
was filled from the nearest of the nine fresh rays, so neighbouring texels held one value. It now
takes their mean under the weight exp(-d² / (2 x 0.36 x side²)), d the distance in texels from the
texel to the ray's own (at the buffer's edge a tile the neighbourhood is held to counts once, where it
lies: weight 0 for the copy). Premultiplied light and transmittance are both linear in what is composed, so
the mean composes to the mean of the rays. The variance clip's mean and spread use the same
weights, so the band is not one over a tile; the box does not. At side 1 no texel is ever filled
and the clip keeps its nine equal weights. The fill stands in for one cast: the texel's first own
cast averages with it, so the soft fill does not break into single raw rays in the frames after;
the count itself stays the texel's own casts. After n casts the fill's share is 1 / (2n) to the
tenth, then a tenth less a cast: 1/18 after nine (81 frames at side 3) [design: computed from the
share]. With a cast of its own behind it a texel of a still view is computed as before. The
distance stays the nearest ray's, so a mixed fill can hold cloud with an empty distance, as a
texel whose last cast met nothing already could; only the distance view reads it.
Stills of a view turning at 60 degrees a second: blocks at the entering edge with the nearest fill, none
with the weighted one [observed, 2026-11-23, judged by eye: the cost set-up of graphics-enhanced.md §3.23
in fair weather with `NVF_PLAY_DEBUG_MEDIUM_TURN=60` from the view 60:20, four stills 3.7 s apart from 60 s;
the shaders of c5a6c2993 edited, without and with the rule, and the rule as built, 99c5df603]. The
cost, one `exp` per ray per texel (nine), is not measured.

**The share and the clamp, as chosen** [engineering choice; a play test of the
experiment aids, judged by eye, 2026-11-19]. The first defaults (a share of 0.5 rising to 1
in motion, the history clamped to the box of the fresh rays) shimmered, still and moving: each
texel's new ray, cast every fourth frame with a turned start offset, let half of its 28-step
grain through, and in motion all of it. Measured on one machine, a still view (a played run
standing at Goodsprings looking up at clear noon, the cloud drift frozen), eight shots 70 ms apart, over
sky free of the HUD, the per-pixel temporal standard deviation in 8-bit levels and the share of pixels
stepping more than 2 levels between shots: the reference and the prototype 0.09 / 0.3 % (the
floor); a share of 0.5 0.36 / 2.4 %; 0.2 0.19 / 0.9 %; 0.1 with the box, the variance clip or
no clamp 0.14 / 0.4 to 0.5 % [observed, 2026-11-19, the development build 992ec21f5 and the aids below; the
heading and the size were not kept]. Shares from 0.05 to 0.2 kept low in motion were all acceptable, with
some shimmer left on tiny clouds; fixed or blue-noise offsets brought shimmer back; and with no clamp
specks held steady, while a loose variance clip brought their shimmer back [design: judged by eye in the
play test, 2026-11-19]. Mid-turn stills at 30°/s showed no trail even with no clamp [observed, 2026-11-19,
judged by eye: the same build and place, `NVF_PLAY_DEBUG_MEDIUM_TURN=30`]. So the share is 0.1 still and moving,
the offsets keep turning, and the clamp is `moving`: none while the view is still or slow, the
variance clip (the rays' weighted mean plus or minus 1.25 standard deviations per channel, the spread
taken from the deviations) fading in between 1 and 4 texels a frame of history motion. Why:
stage 1 is sky only, so there is no disocclusion for a clamp to catch; standing still the
camera's reprojection is exact (it lands on texel centres; the cloud content it carries can
still be stale, see the known limits) and a clamp can only harm a feature
smaller than a tile, which is hit one frame in four while on the other frames a band built only
from this frame's rays, which missed it, pulls the remembered speck toward empty sky however
loose the band (nine rays that all miss give a band of no width); in motion the history is
resampled every frame, and the clip bounds the blur and any trail.

**The measures a change to the reconstruction is held to** [observed, 2026-11-19, the
development builds named, one machine, the same steps each time]. First, standing still, as above: eight
shots 70 ms apart, the per-pixel temporal standard deviation in 8-bit levels and the share of pixels
stepping more than 2 levels between shots. Second, after a cut: the view snapped by 155°, shots
30 to 700 ms later (at about 300 frames a second), each one's mean difference from a shot
1.6 s later. At efe796057 (share 0.1, the `moving` clamp): clear noon 0.14 / 0.4 %, overcast
0.14 / 0.2 %; after the cut 1.31 1.18 1.03 0.92 0.79 0.53 0.25 0.16. At df4bbab7a (the running
mean and the first stall rule, which took the whole ray when the transmittance stalled): clear
0.27 / 1.1 %, overcast 0.25 / 0.7 %; after the cut 0.51 0.45 0.40 0.40 0.37 0.34 0.31 0.29, so
the running mean settles the first frames (0.51 against 1.31 at 30 ms) and the stall rule's
raw rays kept the picture from settling below about 0.3. The reference mode: 0.09 / 0.3 %.

A sample whose stored transmittance rounds to one adds no light in the composition (a very
thin edge would otherwise add its colour at zero alpha). The phase is uploaded modulo 256, the
pattern's period, so it stays an exact float.

**Cuts.** The history is dropped (one frame filled from the fresh rays around, no fade) on:
the camera's cut as the upscalers already test it (`UpscaleCamera::cut_from`, now applied to
the main view in every anti-aliasing mode: a frame skipped, a move of 400 units, a turn of
more than 45 degrees, a lens jump, a frame over 250 ms, a loading screen coming up (not its
leaving, "Under a loading screen" below), into or out of dialogue); another view, anti-aliasing in force, render extent, size or epoch (a history belongs to one
of each); the row switched on again or a trace asked for again (a gap since the last whole
reconstruction); and a jump of the sky, counted by the cloud source from the live sky: a
weather change with no fade (a script's ForceWeather), indoors and out (by the cell the camera
is in), another worldspace's dome, and the game clock (the hour, and the simulation's continuous
game time where there is one, so a jump of whole days to the same hour is seen and no day
boundary is invented) stepping back at all or ahead by more
than twice what the time scale and the frame's delta predict plus two of the sky clock's
half-minute steps (the wait and sleep menus, a console `set gamehour`, a jump of whole days;
a mod's high time scale alone does not cut). A running weather fade, its reversal or end, the
clock's own advance, midnight and walking over a cell's edge are not cuts: the clamp and the
four-frame refresh follow them.

**Under a loading screen** [design: a split second after the screen left, the clouds
were blocky, then resolved, judged by eye in a play test, 2026-11-23]. The screen is an interface node over a world
that is drawn all the same: the main camera stays active, the streaming, the sky, the cover and the
medium run, only `Time` stands still. So the history fills behind it, with the camera where play goes
on from; but the upscalers' cut test counts the change of play state out of `Loading` as a cut, and
that dropped the history in the first frame of play: nine frames of texels filled from the
casts around [design: upscaler/prep.rs `play_state_cut`, medium.rs `history_cut`]. The medium takes
`Loading` as `Playing` (`view_state`), so the screen's leaving keeps the history; its coming up still
cuts (the generation), and so do the travel's jump, the sky's jumps and indoors to out behind it. The
upscalers' own test is unchanged. A settled screen also waits for the clouds (`clouds_wait`): while the
row is on and a trace is asked for or to come (outdoors, above water, a cover composed or being
composed; a weather with no cloud layer has none, and the producer says so, `CloudCoverEmpty`), until the buffer is drawn and nine frames (the tile's texels) have been composed since the
last cut, seen so three frames in a row (`READY_FRAMES`: the render world's counts are a frame or two
old, and a cut starts the three again; a jump of the sky the history has not seen yet counts as one);
not with the row off, indoors, under water, under a clear or cloudless sky or a weather shown flat, or with
a pipeline that failed for good while nothing is drawn in its stead; at most 1 s of real time from when
all else was in, which fits the nine down to about nine frames a second and which the
settle time of 0.25 s is part of, so at 60 frames a second the nine are in before the screen would
leave anyway. After the nine, eight texels of nine have had their first own cast over a fill, half of it
each ("The fill" above): the picture is whole but still settling. The `loading screen phases` line names
`the clouds`; `loading screen: the volumetric clouds were drawn behind it, every texel cast` says that
the screen left with them ready, a debug line that the limit ended the wait.

Covered: the start in the world (`--play`, a save loaded from the menu, whose new run starts under
its own screen), load doors, fast travel, a script's or the console's move, all of which go through
`TravelRequest` and a screen. Not covered, the nine frames still seen: a New Game from the main
menu (it loads behind the menu and the intro film, without a screen), waiting and sleeping and a
console `set gamehour` (the sky's jump), dialogue's close-up in and out, a first frame of play over
250 ms, a script that changes the weather at once or the clock in the first frames of play, the row
or its level changed in play.

**The upsample.** The proxy's fragment reads the buffer at the screen position its own world
position has under the view's unjittered projection, bilinear, clamped to the edge. That
works for TAA, FSR and DLSS alike without knowing a backend's jitter sign: the fragment's
world position lies on its own jittered ray, and the unjittered projection maps it to where
the buffer holds that ray. The talk's dithered four-tap upsample (slide 56) leans on TAA and is
not used.

**What each mode sees.** MSAA: the proxy's edges against the world are the dome's own, per
sample; no jitter. TAA: the proxy is drawn jittered and resolved with the sky; the buffer is
not jittered. FSR and DLSS: the buffer is the level's share of the render size, composed before the upscaler.
FXAA and SMAA: after the composition, as for the sky.

**The handover.** The flat cloud layers give way only to a buffer of the right size written
whole by ready pipelines, the proxy's own draw pipeline among them: an epoch moves on every
change that could make the buffer wrong (the trace asked for or not, the view's size,
multisampling, HDR, upscaler or jitter, a new image), the render world echoes the epoch it
traced for, stamped with the render frame and the view, and the proxy is drawn held (adding
nothing) until a report of this epoch, this view and at most two frames old says the write was
whole and the proxy's own main-pass pipeline in that view (the id bevy_pbr specialised for
that entity, read every frame) is ready; then the hold is released and the flat layers leave
the main view in the same frame. A live change of the mesh pipeline's key (the shadow filter,
multisampling, HDR, the prepasses, the ambient occlusion in the lighting as its view bindings
are given, the sun visibility's bits that the Cloud shadows and Terrain shadows rows switch) is
part of the main world's view key, so the volume hands back before the frame in which bevy_pbr
would skip the proxy, until the new variant is ready. The rule: anything that enters the main
view's mesh pipeline key (bevy_pbr's `check_views_need_specialization`) and can change in play
must be in that key, otherwise the hand-back arrives up to two frames late; the inputs left out
are constant for the run (features the game does not use, the main camera's projection and
fog, the view transmission texture, which every 3D camera carries at its default quality and
nothing changes in play, and the tonemapping bits, which only a non-HDR view takes while the
row forces HDR). An absent entry in bevy_pbr's specialisation cache means
the proxy is not drawn that frame (bevy_pbr keeps an entry across frames and queues nothing
without one), so the verdict is not whole; a variant that fails, or an entry missing or not
ready after 10 s, is named in the log once. Each render frame publishes one verdict, once, at
the end of the frame (never a provisional one the main world could read meanwhile); a frame
whose pass did not run publishes "not written", and the image's extent is checked before
anything is written or the history moves. A verdict "not written" hands back at once; a whole
verdict more than two frames old keeps the volume as it is (a late verdict is not a bad one,
and the flat layers swapping in for a frame are a flash of their own) up to eight frames, and
hands back beyond. Every hand-back from a drawn volume logs `volumetric medium: hand-back to the
flat layers: <reason>` (the first at once, then at most one batch a second, the last reason with a
count of those not
shown) and the return `volumetric medium: the volume takes over again (<n> hand-backs so far)`;
the smoke scenarios forbid a hand-back in a run where nothing changes. A
loss the main world can see hands back in the same frame. A frame the render world alone
cannot draw leaves a buffer that had a whole write as it was (one frame of last frame's
clouds) and clears one that never had. The flat layers move to a render layer of their own while the volume draws, so
the water's mirror, the scope's lens and the extra cameras keep the weather's flat clouds. Under water the
request is taken back in `PostUpdate`, after that frame's camera was found under water (`underwater_fog`)
and before the handover (`trace_above_water`), so the flat layers are back in the frame it goes under.

**The cloud source's noise** (#3032 stage 2b). The trace binds, after the two uniforms, the
cloud source's generated base noise (3D), a repeating linear sampler (linear between levels too)
and the erosion noise (3D), in both entries; the source frame carries the two images' ids, and no
write is whole until both are on the GPU (`WriteCause::NoiseNotReady`; the source logs once
when the bake takes more than 10 s, and the medium once per activation when the generated images
are not both on the GPU after 10 s: `volumetric medium: the clouds' noise textures were generated
but are not both on the GPU after 10 s (base ..., erosion ...)`). The trace's bind group is kept while its buffers, both
views and the noise's sampler stay the same; the sampler is made with the first trace and
released with the activation, as the bind group is.

**The reference.** `NVF_PLAY_DEBUG_MEDIUM=reference`: the published image at the full render
size, every texel traced every frame, no history, the first prototype's per-pixel offset. It
traces the cloud source as it is now (since stage 2a the domain, the placement and the types),
so it is no longer the first prototype's picture; the measures quoted below for the prototype
and stage 1 are of the commits they name.

## The seam for sources

The medium's trace calls one function of each source: a ray in (`MediumRay`: origin, unit
direction, `[t_min, t_max]`, and its spread), the light it gathers out (`MediumSample`: the premultiplied colour,
the transmittance and the transmittance-weighted distance, -1 for an empty ray). Inside the
cloud source each sample asks three questions of its own functions: how much it takes
(extinction), whether the light reaches it (direct visibility) and what light arrives from all
around (incident ambient). A second source over the same stretch (haze beyond the near field,
later) must add its extinction and emission at the same samples of the same loop: adding a
second finished colour over the first would count the light through the shared stretch twice
[documented: Bauer 2019, slides 43-49]. The reconstruction depends only on the colour, the
transmittance and the distance, so a source changes nothing there. **The spread** (#3032 stage 2a, the
seam's one added value): the width per unit of distance of what one
cast ray answers for each frame, the cast texel's angular width times the tile side (so it
follows the buffer's scale, the tile side and the reference mode's full size), at the screen's
centre, from the view's real projection and the buffer's extent (medium.rs `ray_spread`, carried
in the trace uniform's `origin.w`; the wider axis on a texel the per-axis rounding leaves not
square). A source leaves out what is finer than `spread x distance`: the cloud source fades its
erosion's detail and its placement octaves by it (the placement through the footprint on the
bent shell); a later haze source will want it too. Direct visibility and the
ambient light are the two inputs later global illumination or ray tracing would replace.

## Weather mods

These are requirements of every stage:
- Every input comes from the load order's weather at run time through the live sky state (the
  shown weather, the one fading out, the fade, the hour, the sun): the cloud textures (the
  cover), the cloud colours and speeds, the sunlight colour, the sky level. Nothing is keyed to
  a weather's FormID, EditorID or plugin (the IDs are compared only to tell a fade from a
  snap); no constant fits only the base game's records.
- Scripted changes (SetWeather, ForceWeather, a mod's weather cycle) go through the same
  state: no per-weather result is kept past a change, a fade is blended, a snap is a cut.
- **When the medium cannot make sense of a weather, that weather is shown the flat way**,
  never a wrong sky: a weather with an active cloud layer whose texture cannot give plain 2D
  alpha gets no cover at all, whole, also while it is one side of a fade. Readable means: the
  sky's own loader would take the file (one set of shape rules: no cube map, sides and mip
  levels in range, its whole payload there), it is no volume texture whatever its depth, its
  format holds alpha, and a BC1 (DXT1) file uses its one-bit alpha somewhere (one whose blocks
  are all opaque carries none); a DXT5 or ARGB file at full alpha everywhere is a true full
  cover. A texture in no archive is unreadable too. The checks read the header and the sizes; only a BC1 file's top-level alpha is scanned
  for a transparent texel, and the producer decodes the top level's alpha (as Cloud shadows does)
  to compose the cover: no texture is decoded whole to RGBA to learn whether it has alpha. Each
  weather gets a cover of its own only from a composition made once every active layer was read;
  while any active layer of either weather of a fade is still being read nothing is published (the
  flat layers show), and the volume is drawn only while every live weather with a weight has its
  own cover (a live weather that lacks one is composed at once, not at the interval), with one
  exception: while a fade's incoming weather counts for at most 5 % and its first cover is being composed,
  the measured outgoing cover is taken whole, for at most one real second (cloud_shadows.rs `CoverWait`).
  The
  volume's cover recomposes on the real clock, so a paused game does not stretch that (with
  Cloud shadows on too, both keep the game's clock as before). While the volume wants the cover, a
  texture still being read after 10 s is named in the log once and its weather stays flat (with
  Cloud shadows alone none of this exists). A weather with a value the medium reads that
  is not a finite number at run time (a fog
  distance, a colour, the sky level, the cover's projection), on either side of a fade, is shown
  flat rather than repaired. A weather with no cloud texture, or an empty cover, is not drawn
  either. In an interior cell (by the cell the camera is in) and under a water surface (whether
  or not its water record could be read) the volume is not drawn.
- A mod changes the amount, the tint, the drift and the wind of the clouds; it cannot change
  their shapes or, since stage 2a, where they stand ([design]: the weather's composed cover gives the amount, its mean; the placement is the cloud
  source's own field, graphics-enhanced.md §3.23). A mod whose point is painted cloud forms
  loses them on the layers that become volume; the row is off by default.
- Finite extreme values (a sky level of 3e38, a fog distance of 0) give finite, bounded,
  non-negative uniforms (tested); no range beyond that is guessed. A weather record's non-finite
  field is the loader's 0.0 for the medium as for everything else (the fog, the flat sky and the
  lighting all run on that value): the medium agrees with what the rest of the frame shows and
  does not second-guess the loader from the raw record [design, 2026-11-19].
- Nothing is written anywhere a mod reads; the medium needs no record.

## The clouds' light volume (#3032 stage 2g, checkpoint B1)

An engineering choice for the enhanced look (graphics-enhanced.md §3.23 has the whole account). The
clouds' optical depth toward the light is no longer marched by eight taps from every lit sample but
kept on two nested lattices around the camera and read once, after the first two of the eight
intervals toward the light are marched [design; Kim and Neumann 2001, Bauer 2019 slides 33 and 35,
documented]. Near level 128 x 128 at 500 units, far level 128 x 128 at 4,000 units, both a layer every 1,000 units
(25 at the domain's 24,000), world-aligned in the wind's field coordinates, swept top down on the GPU before the medium's
trace, on its encoder (`nvf_cloud_light_far`, `nvf_cloud_light_near`, then `nvf_medium_trace`).
Since B2 each level has a front set the trace reads and a back set swept over 8 (near) or 32 (far)
frames from one frozen snapshot of the uniform; the sets swap when the back is finished and the
trace reads each front through its own window (graphics-enhanced.md §3.23). It follows the weather
through the march's own uniform (the same density as the view march, the same sun or moon
direction), at most 16 or 80 frames behind; a weather that jumps (a forced weather, a tuning file),
a clock jump, the moon taking over or a teleport drops the sets that no longer fit and the
eight taps stand in until a sweep finishes, so a weather change or a mod's weather needs nothing of its
own; with no light, or with
`NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_LIGHT_VOLUME=off`, nothing is held and the trace runs its legacy
variant, all eight taps. The figures that chose it, printed by trial tests on the CPU mirror
of the body of 2026-11-20 (removed with that body, so they cannot be printed
again here) [hypothesis]: |p95| against the fine depth to the top 0.013 to 0.028 for
the volume alone, 0.05 to 0.11 for the eight taps (the baseline); the largest crease 0.05 to 0.29 against
the taps' 0.30 to 1.75.
Since stage 2i M2 a shared reader of the completed fronts (`LightVolumeReader`,
`light_volume_reader.wesl`) gives the ground's cloud shadows their depth (graphics-enhanced.md §3.23,
"Ground shadows from the light volume") and the light in the air its K ("The visibility" below).
Not built: a cross-fade between generations.

## What is not done

What is open of the light in the air (stage 3: its own "Where we differ from the talk" and "Known limits" below; the white
land in a hazy morning, the lifted overcast and the clipped bright clouds are no longer, "The composite" 6 and 8 and
the deck's share in "The medium"), a second cloud layer, wind-blown
dust, haze and fog as one medium (stage 4), clouds in front of terrain (the medium applied to
world fragments), depth-aware ray placement (slide 55) and
depth rejection in the clamp (slide 52: not needed while the buffer is sky only), the
dithered four-tap upsample (slide 56), and a DLSS smoke scenario (DLSS needs files a player
supplies). A tile side of 4 (one ray in sixteen) is not built [design]:
a full tile needs 16 placements and an offset period of 1,024 phases (the upload's modulus
with it), and its 15 skipped frames fit the age's packing; what is missing is a fair schedule
for the tiles the buffer's edge cuts to 3, 6, 9 or 12 texels, tested across the phase's wrap with
both shaders agreeing, and a turn and cut run that accepts the clamp's footprint 12 texels wide
and a half-life of about 105 frames at the share of 0.1. The 3x3 search for the nearest cast ray
would still do (the farthest own corner is 3√2 texels away, any excluded tile at least five). The flat layers' visible shares order the layers by the dome's ranks
(`2 x layer + set`) while Bevy's transparent sort also takes each shape's distance, so at some
camera pitches two overlapping flat layers can be drawn the other way round than the shares
assume (a wrong tint share where two opaque layers overlap); fixing it belongs to the sky's own
sort keys and would change the flat sky with the row off, so it is left for an issue of its own.

## Known limits of stage 1

- **An opaque-only BC1 cloud texture shows its weather flat.** Of the base game's and the DLCs'
  sky textures, the one DXT1 file a weather names as a cloud layer is
  `textures\nvdlc04\sky\nvdlc04cloudovercast.dds` (Lonesome Road, 1024x1024; none of its
  65,536 blocks has a transparent texel), named in one cloud layer by 7 of LonesomeRoad.esm's 15
  weathers: NVDLC04HRWeatherFoggy, NVDLC04NukedWeather, NVDLC04LRInt02Weather,
  NVDLC04LRIntWeather, NVDLC04DivideVistaBWeather, NVDLC04HRWeatherClear and
  NVDLC04LRWeatherFoggy. Where that layer is active these weathers are shown flat (before this
  rule the volume would have made them a featureless full deck and lost the texture's painted
  pattern). No weather of FalloutNV.esm (63), DeadMoney.esm (8), HonestHearts.esm (9) or
  OldWorldBlues.esm (3) names a DXT1 cloud texture, so nothing there changes [documented: the
  plugins' WTHR records and the archives' DDS headers and blocks, read on
  2026-11-19; not yet looked at in a run]. The code keys nothing to these weathers or this
  file: the rule is the texture's alpha, as for any mod's texture.
- The cast count is not reset when a fast turn's clip replaces most of a texel's history (that
  would raise the share in motion, the shimmer the play test rejected), so after a fast turn stops a
  clipped texel settles at the floor share: about a fifth of its remaining error is left after
  a second at 60 frames a second (0.9^15) [design: computed from the share].
- The count is the nearest history texel's while the light is a bilinear mix of four, so near
  a junction with young or filled texels an established count can ride on partly young light
  and hold it at the floor share [hypothesis: inferred from the code, not seen in a run].
  The least count of the bilinear taps was tried (6047e2e33) and raised the grain in turns:
  the frames' spatial grain at 30°/s 1.13 against 0.97 to 0.99 in five earlier builds with the
  low share in motion, at 120°/s 1.24 against 1.09 to 1.12 [observed, 2026-11-19, one machine:
  the place of the still measure above with `NVF_PLAY_DEBUG_MEDIUM_TURN` at 30 and 120], since a minimum over the footprint each frame spreads low
  counts faster than casts rebuild them. If the case is ever seen, a count weighted through the
  bilinear weights (an effective sample count) is the thing to examine.
- A feature smaller than a tile can still flicker while the view turns fast: the clip is on
  then, and the specks come back as they did with a loose clamp.
- The camera's reprojection is exact, but the cloud content a texel remembers can be stale:
  when an established texel's clouds change under a still camera (cover drifting with the wind,
  a fade, the light moving within the time-scale allowance), it follows at the floor share,
  0.9^n of the old contrast left after n casts, one cast in four frames: half after about 26
  frames, a fifth after 60, a tenth after 88 [design: computed from the share, before storage
  rounding; not looked at in a run, not reported in the play test]. A young texel (fewer than
  nine own casts since a cut) follows faster by the running mean. Stage 2 is to examine moving
  the history with the cover's own drift (the wind) in the reprojection; no detector of a
  confirmed discrepancy is built.
- When the clouds come in front of terrain (the stage that applies the medium to the world), a
  history texel whose depth no longer matches the scene's must be rejected by a depth test (the
  talk's depth discrepancy, slide 52) rather than by any colour clamp.
- The flat layers' sort order (above) and the cloud look itself (stage 2).

## Cost

Indicative figures [observed: measured runs on one machine, 2026-11-19, at the development build
09d1abc43; Goodsprings at 13:00, `--no-actors`, sun shadows on, ambient occlusion, longer
shadows, light glow and sun shafts on; the median of the last five 4 s periods of two rounds;
whole frame in ms and the GPU spans of the `gpu time:` log line]:

| Case | Row off | First prototype | Reference | Default (trace / reconstruct) |
|---|---|---|---|---|
| 2560x1440, NVWastelandClear | 3.21 | 7.1 to 8.4 (its march 3.9 to 5.2) | 7.48 (trace 4.28) | 3.98 to 4.21 (0.61 to 0.93 / 0.04 to 0.09) |
| 2560x1440, turning at 60 deg/s | 3.63 | | | 4.23 to 4.44 (0.71 to 0.87) |
| 2560x1440, NVBlackMountainWeather | 3.30 | | | 4.32 to 4.47 (0.97 to 1.13 / 0.09 to 0.10) |
| 1920x1080, clear | 2.93 | 5.76 | | 3.19 to 3.28 (0.64 / 0.03) |
| FSR Quality, 2560x1440 output | 3.11 | | | 3.60 (0.50) |

These are stage 1's figures: a slab of 28 view steps, one ray in four into a half-size buffer. Against
the stage's aim of 1 ms at 2560x1440 the whole frame grew, with the row off as the baseline, by +0.8 to
+1.0 ms in clear weather (about a fifth of the first prototype's +4.3 over the same baseline), met at the
edge, and by +1.0 to +1.17 ms under overcast, missed. The trace's span followed the march's step count (4
view steps 0.17 ms, 28 steps 0.61 to 0.93, 56 steps 1.57 at 2560x1440). They do not hold for the clouds
as they are now, whose trace is measured in "The levels" above and in graphics-enhanced.md §3.23, "The
view march" (each names its baseline). At stage 1, per ray, the small pass cost
more than the full-size reference (about 3 to 8 ns a ray against 1.2 to 2.1); the frame was
close to CPU-bound in the small cases, so the GPU may have been clocked down, and the default's
rays lie two texels apart on the half-size buffer, so neighbouring threads read the cover and
the noise less coherently and leave the march at more different steps [hypothesis; the tile
entry's own work per ray, the corner choice and the R2 offset, is a few integer operations
beside a 28-step march].

## Debug aids

Developer aids (engine/nvforever/src/dev_aids.rs): they exist only in tests, in debug builds and in a build
with the `dev-aids` feature (in engine/: `cargo build --release -p nvforever --features dev-aids`). A release
build without `dev-aids` reads none of these variables, has neither the tune file's follower nor the turn's system, and runs with
the defaults given here. Read once at the start, usable in a played run (they do not make a run unattended):
- `NVF_PLAY_DEBUG_MEDIUM=reference`: the reference above.
- `NVF_PLAY_DEBUG_MEDIUM_VIEW=fresh|age|history|clamped|distance`: the buffer shows, opaque
  over the sky: `fresh` green where a ray was cast this frame, grey where the texel was rebuilt
  from its history, red where it had none; `age` the frames since its last ray (white at 8,
  stored up to 15; the cast count shares its channel and is not shown);
  `history` red where no valid history was found, the picture elsewhere; `clamped` how much the
  clamp moved the history (x4); `distance` the weighted distance (white at the source's range), blue for
  an empty ray. Ignored in the reference.
- `NVF_PLAY_DEBUG_MEDIUM_FRESH=<0..1>`: the fresh share's floor (0.1).
- `NVF_PLAY_DEBUG_MEDIUM_FRESH_MOVING=<0..1>`: the share the motion ramp rises to (0.1): the share
  is max(floor, ramp(motion) x this), so with a low value a moving view keeps accumulating
  through the reprojected history (the clamp and the fallback for a texel with no history stay).
- `NVF_PLAY_DEBUG_MEDIUM_OFFSETS=turning|fixed|blue`: the default mode's ray start offsets:
  `turning` (the default) the R2 offset of the texel turned by the golden ratio at each cast;
  `fixed` the texel's R2 offset at every cast (no grain in time, a fixed grain in space); `blue`
  interleaved gradient noise of the texel (Jimenez 2014) turned by the golden ratio at each cast;
  `centre` 0.5 for every ray at every cast (#3032 B1b: a pattern still seen with it is not the
  offsets'). No effect in the reference mode.
- The log tallies the reconstruction's history cuts (#3032 B1b): after the first 120 frames,
  then after a later window of 120 with cuts only when its dominant cause (the most counted of
  a gap, a camera cut, a frame over 250 ms, a sky jump, a new history) differs from the last
  logged one's (a camera cut now and then is ordinary play and writes no line
  every two seconds), `volumetric medium: history dropped on <n> of the last <m> frames (a gap,
  a camera cut of which a frame over 250 ms, a sky jump, a new or missing history)`;
  `NVF_PLAY_DEBUG_MEDIUM_TALLY=1` logs every window that had a cut. A history dropped every
  frame leaves eight texels of nine (three of four at side 2) filled from the cast rays around: soft,
  unsteady edges whatever the fresh share or the offsets.
- `NVF_PLAY_DEBUG_MEDIUM_CLAMP=moving|box|variance|off`: how the history is held to this frame's
  rays: `moving` (the default, above); `box` the box of the fresh rays of the 3x3 tiles (the
  first default); `variance` their weighted mean plus or
  minus k standard deviations per channel (Salvi, "An Excursion in Temporal Supersampling", GDC
  2016), kept to what the stored format holds; `off` none (for diagnosis: the history as
  reprojected; the no-history fallback and the cuts stay). `NVF_PLAY_DEBUG_MEDIUM_CLAMP_K=<0.5..8>`
  sets k (1.25, also `moving`'s). The `clamped` view shows how far the clamp in force moved the
  history.
- `NVF_PLAY_DEBUG_MEDIUM_TURN=<degrees a second>`: the view turns at that rate (positive to the
  right; the player's heading in a played run, the free camera's yaw in the viewer), for
  pictures and cost runs mid-turn.
- `NVF_PLAY_DEBUG_MEDIUM_SCALE=<1..4>`: the default mode's published image is the render size
  divided by this, rounded up, whatever the row's level (a whole number held to 1..4, or 2.5; unset
  or unreadable: the level's own).
  No effect in the reference mode.
- `NVF_PLAY_DEBUG_MEDIUM_HISTORY_FILTER=linear|cubic|adaptive` (adaptive): how the history is read
  between its texels while it moves: `linear` bilinear; `cubic` with the Catmull-Rom correction, held to
  the range of the fresh rays and the bilinear history; `adaptive` cubic, with the cast count limited by
  the squared bilinear weights.
- `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_VIEW=sun|ambient|depth` (#3032 stage 2e, L1; graphics-enhanced.md
  §3.23): the cloud source lit by the direct light alone, by the ambient alone, or the optical
  depth toward the light it measured (grey on a log scale, magenta where the light was skipped;
  per ray the opacity-weighted mean, opaque). `VIEW=opacity` shows camera-ray opacity, `body-height`
  where the visible material sits between its local base and top; `cover`, `vigour`, `count` and
  `noise` are in §3.23. `VIEW=none` restores the picture.
- `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_TUNE=<file>` (#3032 stage 2e, L1): the cloud source's aids
  tuned live from a `NAME=value` file, looked at at most every 0.5 s of real time while the row is
  on (live: `SHAPE`, `EROSION`, `DENSITY`, `STEPS`, `TYPE`, `VIEW`, `WISP`, the light's weights,
  `SUNCOLOUR`, `NIGHTPULL`, `MOONFORWARD`, `MOONBACK` and `MOONCAP`; start-only: `HEIGHT`, `HORIZON`).
  Each change of the effective values counts as a sky jump for the history (dropped
  once), so paired pictures after an edit start from fresh rays. A key the file does not set (a
  deleted line included) takes its start value: its start variable, else its start default
  (graphics-enhanced.md §3.23, "Debug aids").
- `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_LIGHT_VOLUME=off|near|far|both` (#3032 stage 2g, B1; default
  both; off: the eight taps, nothing held), `..._LIGHT_VOLUME_CELLS=<near>,<far>` (the levels'
  spacings across, units; 500 and 4,000), `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_LIGHT_TAPS=0..8`
  (the near taps before the volume is read, 2; 8 is the legacy path; at least one is marched whatever
  is asked, and the logs state the count marched) and
  `..._LIGHT_VOLUME_SPREAD=<near>,<far>` (the frames a sweep takes, 1 to 256; default 8 and 32;
  `1,1` is B1's whole sweep every frame).
- `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_` `SHAPE`, `EROSION`, `STEPS`, `TYPE`, `HEIGHT`, `HORIZON`, `WISP`,
  `PUFF`, `AIR`, `REFINED_STRIDE`, `LIGHT_REUSE` and the light's `OCTAVE`, `BACKSCATTER`, `UPPER`, `LOWER`,
  `GROUND`, `FORWARD`, `SUN`, `LEVEL`, `OCCLUDE`, `POWDER`, `SUNCOLOUR` (alias `SUNCOLOR`),
  `NIGHTPULL`, `MOONFORWARD`, `MOONBACK`, `MOONCAP`, and
  `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS=<0..4>` (`DENSITY`): the cloud source's start values, ranges and meanings
  are graphics-enhanced.md §3.23, "Debug aids". The cloud source's light is the source's own business:
  the medium's seam (`MediumRay` in, `MediumSample` out) and its history do not see it. The wisps' phases
  are the base noise's, so they do not drift against the body and the reprojection is unaffected.
  Smoke: `volumetric-clouds` checks the defaults, `volumetric-clouds-no-aids` that a release build ignores the
  aids.
- `NVF_PLAY_DEBUG_CLOUD_SHADOWS_SOURCE=weather|volume|auto|share|depth` (#3032 stage 2i, M2; graphics-enhanced.md
  §3.23, "Ground shadows from the light volume"; default auto): where Cloud shadows come from while
  that row and Volumetric clouds are on (the layers whatever it says while Volumetric clouds is off). `auto`: the light volume's completed fronts where they have data, the weather's
  flat layers elsewhere, mixed through the windows' guard bands and while a level fades in (about a
  second); the layers whole unless the volumetric sky is what the player sees: this row on with the
  medium's buffer shown (its composing material ready, the flat sky hidden), its clouds asked of the
  medium, and a front to read (sun_visibility.rs `AutoGate`);
  `weather`: the layers only (the shadows before M2, for paired pictures); `volume`: the volume only,
  no shadow where it has no data; `share` and `depth`: debug views of the reader in place of K, its
  covered share (presence and guard bands included, 0 no data) and its optical depth over 8 (shown
  as grey with `NVF_PLAY_DEBUG_SUN_VISIBILITY=cloud`). Log line `cloud shadows: source <what and
  why>` whenever the source in use changes while Volumetric clouds is on, and when it goes off.
- `NVF_PLAY_DEBUG_MEDIUM_TILE=<1|2|3>`: one ray per tile of this side a frame (3); 1 casts every
  texel every frame through the history. Any other value is 3 (4 is not built). No effect in the
  reference mode.

Log lines: `volumetric medium: far buffer <w>x<h> (a third of the main pass's render size), one ray
in nine a frame into <w>x<h>, ...` (the scale in words: `the main pass's render size`, `half`,
`two fifths of`, `a third of`, `a quarter of`; the tile: `one ray in nine`, `one ray in four` or `one ray per texel`; or `(the main
pass's render size), every pixel traced each
frame (reference), ...`) when it is made or resized; `volumetric medium: trace pass drawn (...)`
once; `volumetric medium: far buffer <w>x<h> written whole and volumetric_clouds_pipeline ready,
the clouds drawn from it` when the flat layers give way; `volumetric medium: far buffer
released`. Smoke: `volumetric-clouds` (Normal), `volumetric-clouds-taa` (High),
`volumetric-clouds-fsr` (Performance), the same two turning with `NVF_SMOKE_DEV_AIDS` set (`-taa-turn`, `-fsr-turn`:
they turn the view by an aid),
`volumetric-clouds-level-change` (the three, in play).

## The near grid and the light in the air (#3032 stage 3)

[design] An addition of NVForever's own, an engineering choice for the enhanced look: sunlight
seen in the air, shafts where the air is lit and shade where a ridge, a building or a cloud
stands between it and the sun, or by night the moon. It is not a statement about the original's frames, never a
parity item, and never to be used to explain a difference from them. Off by default and in
every preset; a release build without `dev-aids` reads none of the aids below (dev_aids.rs), so there the row alone
switches it; started with it off, nothing is allocated, no pipeline is asked for, nothing is
logged and the picture is as on main (medium_air_grid.rs, "Off means off"; tested). What is left with
it off: the rows and their keys, the run conditions and a few checks a frame, the shaders' registered
sources, and at a candidate blended shape's build the name of its base texture ("Solid cores of blended
shapes"). Switching the
row off in play logs the transition (`no longer wanted by the row`, medium_air_demand.rs) and the
render side then releases its volumes and buffers; the pipelines asked for and a sampler stay for the next
time, as does which materials were found animated and which textures solid. A weather fade's cadence is
the clouds' alone (`sky::fade_cadence`): the light without them changes nothing of it. Code:
`engine/nvforever/src/medium_air.rs` (the pure maths, the aids, the tune file, the gate and the
log line; CPU mirrors and tests), `medium_air_demand.rs` (the switch), `medium_air_light.rs`
(the main world's side: the tune file, the sunlit share, the gate), `medium_air_grid.rs` with
`medium_air_grid.wesl` and `medium_air_filter.wesl` (the plugin, the volume, the grid and filter
passes), `medium_air_compose.rs` with `medium_air_compose.wesl` (the composite), and three narrow
edits in `graphics.rs`, `terrain_shadows.rs` and `sun_visibility.rs` (below). It works with the
Volumetric clouds row on or off.

**How it is switched.** By the row "Volumetric light (experimental)" (Graphics > Lighting and
Effects, after Volumetric clouds; `volumetric-light`, also `--graphics-only volumetric-light`)
with its slider "Volumetric light strength" (`volumetric-light-strength`, 0 to 200 % in steps of
10, 100 % by default, greyed while the row is off), or by the developer aid
`NVF_PLAY_DEBUG_MEDIUM_LIGHT=<strength>` (not in release builds, "Debug aids" above), read once at the start (a
finite number above 0 asks for the light). `AirWanted` (medium_air_demand.rs) is the row or the aid. The strength k is one
overall scale of the whole applied correction, 0 to 2 (1 the look as tuned; at 0 the gate stays
closed): the aid's value when the aid asks, else the slider's, which a slider move changes live
[design]. The
aid works in a played run (it does not make the run unattended). While the light is wanted the
main view stays HDR (graphics.rs `apply`, as the Volumetric clouds row keeps it): the light is
composed in the linear HDR picture. The screen-space Sun shafts yield in the render world, per
view and per frame: when the composite issues its draw for the light's views (`none`, `delta`,
`rays`) it records the view in `AirComposedViews` (medium_air_compose.rs; the list is emptied
each frame), and the shafts' pass, after tonemapping in the same frame, skips on such a view
(shafts.rs `shafts_skip`), logging `sun shafts: yielding to the light in the air` and `sun
shafts: the light in the air no longer draws, the shafts resume` once per change. The shafts'
own uniform is left alone, so a view the render side refuses, a pipeline still compiling or a
closed gate keeps the shafts, with no frame of lag [design]. The
game's settings page greys the Sun shafts row while the light is wanted at a strength above 0
(settings.rs `shafts_status`; at 0 % the shafts draw and the row is free; the launcher, whose shared
rules know no slider's value, does not grey it); the row's note ends "An addition
of NVForever's.".

**The three narrow edits** [design], each with a test beside it, each reading
`AirWanted` as absent-is-false: graphics.rs keeps the view HDR (and its cached change detection
follows the demand both ways); terrain_shadows.rs splits the terrain map's demand into the
producer (T0's heightfield, the jobs, the publication), which runs for the Terrain shadows row
or the light, and the surfaces' feed, which stays on the row alone, so the light never shades
surfaces by the map nor changes their pipelines (a heightfield already on by its own aid stays
on through any on/off cycle); sun_visibility.rs marks the outdoor sun light (`OutdoorSunLight`,
its GPU flag `DIRECTIONAL_LIGHT_FLAGS_NVF_OUTDOOR_SUN_BIT`) for the light too, with no view
bindings, shader defs or GPU parts of the surfaces' path, so the grid finds the sun and never the
first-person arms' shade light, which is a directional light too.

### The grid

[documented: Bauer 2019, slide 42 (a froxel grid of 160 x 88 x 64), slide 47 (each column marched
front to back, slice by slice); Wronski 2014, "Volumetric Fog", SIGGRAPH 2014 Advances, for
froxel grids with exponential slices; our own form and code.]
- **Size.** 160 x 88 x 64 at 16:9: rows 88, columns following the main pass's aspect (rounded to
  a multiple of 8, held to 64..320), 64 slices (medium_air.rs `AirGrid::for_view`). An asked grid
  (the aid or the tune key `GRID`) is held to 16..320 x 8..176 x 8..128 [design].
- **Slices in view depth.** Slice k spans `[z(k), z(k+1)]`, `z(k) = 50 (q^k - 1) / (q - 1)`: a
  first slice of 50 units growing by q each slice, q solved by bisection so that `z(64)` is the
  grid's depth; linear (depth / 64 a slice) where the depth is too short for the curve [design]. At 12,000 units q is 1.036066 and the last slice 466 units thick; at 48,000 q is
  1.067636 and the last slice 3,088 [design: computed from the curve, test
  `air_grid_curve_figures`]. A froxel 48,000 units out is about 460 units wide across 160 columns
  of a 1.39 rad view [hypothesis: computed]. The fog is taken by radial distance
  (view depth times the column's ray length), the cascades by view depth.
- **Why 48,000 by default** (`AIR_DEPTH`; the aid `NVF_PLAY_DEBUG_MEDIUM_LIGHT_DEPTH` and the
  tune key `DEPTH` take 1,000..200,000) [design: judged by eye from stills, 2026-11-20]: the Mojave's fog is front-loaded (below), but in the commonest weather three
  quarters of the fog share a far ridge shows lie beyond 12,000 units, and the flat clouds' plane
  is about 29,000 units up, so a 12,000 grid could not reach the shafts under broken cloud. In the
  lit-fraction view the 48,000 grid showed rays fanning from behind the ridge at 18:30 and cloud
  columns at noon, the 12,000 grid hardly any [hypothesis: judged by eye in stills of 2026-11-20 at
  Goodsprings, a development build of that day; the build, the view and the size were not kept]. The talk's 160 m grid carries local lights and
  fog volumes; ours carries one light whose occluders beyond the cascades are coarse (the terrain
  map's 256-unit texels, the cloud image), so a deep grid loses less here [hypothesis].
- **The visibility samples.** Each froxel takes S samples of the visibility (`SAMPLES`, 1..8,
  default 4): stratified in depth inside the slice, moved off each stratum's centre by the
  column's fixed dither value with alternating sign (the last of an odd count at its centre, so
  the samples' mean depth is the slice's middle), each on a ray moved sideways inside the
  column's own footprint by the R2 sequence (Roberts 2018) shifted by two more dither values
  (each axis within half a column, so no sample reads outside the frustum). The dither is
  interleaved gradient noise of the column index [documented: Jimenez 2014, "Next Generation
  Post Processing in Call of Duty: Advanced Warfare", SIGGRAPH 2014], fixed in time. **Each
  sample carries its own stratum's fog step** dF_s, the last stratum ending at the slice's
  boundary, so with V = 1 the steps add up to the slice's own step and a fog ramp narrower than a
  slice is weighted by the strata it falls in [design; tested].
  Each stratum's sample sits inside the part of the stratum where the fog share changes (the
  stratum's overlap with the fog's ramp, near to near + max(far - near, 1) along the ray, in view
  depth), so a ramp narrower than a stratum is read where it is (a
  shadow confined to a ramp at 514 to 515 units inside a stratum over 500 to 515.6 is now seen);
  a stratum the ramp covers whole keeps its dithered tap, and a stratum with no fog change does
  no visibility read (its weight is 0). The slice's stored V is the mean of the samples actually
  read (1 in a slice with none) [design; tested].
- **What a froxel stores** (`Rgba16Float`, cumulative to the slice's far boundary): r
  `N = sum of dF x V`, g `M = sum of dF`, b the slice's own V (the plain mean of its samples, for
  the debug view), a 1 [design]. Slice 0 starts from a share of 0, not F(0) (a fog
  whose near is below 0 gives the camera itself a share, which slice 0 owns), and each step is
  taken against the largest share so far, so M never falls, N never exceeds M and M telescopes
  to what a surface at the boundary was given. **No phase is stored**: the first version stored
  `sum of dF x V x p` per column and divided by the pixel's own phase, and between columns an
  all-lit grid then read as shadow near the sun (0.655 at the centre of a 16 x 8 grid with g 0.8
  and every sample lit) [design]; the composite applies the
  pixel's own phase once.
- **The walk: one invocation a column, or four chunks** [design; tested]. On a discrete NVIDIA card
  (wgpu's adapter vendor 0x10DE and type `DiscreteGpu`) a column's slices are shared between four
  invocations; on every other adapter (AMD, Intel, integrated, virtual, unknown) one invocation walks
  the column (the chunks were slower where measured, "Cost and memory"; the choice is
  `GridWalk::choose`, the aid `NVF_PLAY_DEBUG_MEDIUM_LIGHT_WALK` overrules it). The chunks (the workgroup is 8 x 8 x 4, the dispatch one group deep): chunk c takes slices
  `[c x p, (c + 1) x p)`, `p = ceil(nz / 4)`, the last chunks fewer or none where nz is no multiple
  of 4. Each walks its own slices from sums of 0 and keeps them; it leaves its N and M in workgroup
  memory, and after the barrier adds those of the chunks before it, in order, and stores its slices.
  Why: a walk is one chain of 64 x 4 dependent reads and a grid has only 220 groups of columns, so
  the pass may wait for reads more than it fills a wide GPU; four chains of 16 slices run at once
  [hypothesis: that latency is the reason for the time measured in "Cost and memory"]. The joined sums are
  the one walk's: all a later slice takes from the earlier ones besides N and M is the largest share
  so far, which follows the fog alone (a stratum without a step skips its visibility read, never
  that update) and, for a share that does not fall along the ray, is F at the chunk's start, the
  float the slice before it ended at. That is exact arithmetic; in f32 two things differ. The
  additions are grouped otherwise (rounding; the volume stores half floats). And WGSL gives `pow` an
  accuracy but no promise that it never falls [documented: WGSL, "Floating Point Accuracy"]: where it
  rounds a farther share below a nearer one, the one walk keeps the larger and a chunk that starts
  there the smaller, so a stratum may take a step of a few units in the last place, and a visibility
  read, the one walk would not. N and M weigh it by that step; b, the plain mean of the samples read,
  can move by a whole sample, and only the diagnostic view `visibility` shows b. N and M are joined by
  the same operations in the same order, so N = M still holds exactly where all is lit and M never
  falls across a chunk's start. Variants (`GridWalk`, specialised by shader def, chosen each frame
  from the adapter, its limits and the grid's slices): `AIR_CHUNKED` keeps at most 16 slices a chunk
  (grids to 64 slices, the default's 64), with `AIR_LONG_CHUNKS` 32 (to 128, the most a grid has;
  checked when the code is compiled); where the chunks are taken, a tune file's `GRID` that crosses 64
  slices asks for the other pipeline, and the light is not drawn until it is ready. The one walk is one
  pipeline throughout. The chunks need 256 invocations and 2,048
  bytes of workgroup memory: WebGPU's defaults are 256 and 16,384, but Bevy asks for the adapter's own
  limits and Vulkan 1.0 requires no more than 128 invocations, so one they do not fit takes the one
  walk too. That is the variant without `AIR_CHUNKED` (8 x 8 x 1): the same walk stored as it goes.
  Every invocation reaches the barrier: a column outside the grid
  (an asked grid's sides need not be multiples of 8) has no slices instead of returning. No subgroup
  operation is used, so nothing depends on a subgroup's size. The chunks' extra
  work, the set-up of a column (done four times), one fog share a chunk, the kept slices (an array
  read by index across the barrier) and the wait for the slowest chunk, is where they lose on a GPU
  that the one walk already fills [hypothesis].
- **The filter pass** (`FILTER`, 0 or 1, default 1) [design, committed after the first look]:
  a 3 x 3 tent `(1 2 1 / 2 4 2 / 1 2 1) / 16` in x and y within each slice, of N, M and V, into a
  second volume of the same size that the composite then reads; the weights of the neighbours
  that exist are renormalised at the grid's edges. A tent rather than a box: per axis its
  response is 0 at the grid's Nyquist frequency, where a column-to-column alternation lives (at
  interior texels and along the edges a checkerboard becomes its mean; the weights' sum is 16
  inside, 12 on an edge and 9 at a corner, and a corner keeps 5/9 of its own value and 4/9 of the
  other), while a box's leaves a third of it, inverted. Linear with the
  same weights in every slice, so M stays monotone in depth and N never exceeds M (tested). Why:
  interleaved gradient noise is made to be followed by a 3 x 3 filter [documented: Jimenez 2014],
  and without one the dither showed as a fixed screen-door pattern in the rays where the sun
  grazes, amplified by the ray contrast [hypothesis: judged by eye in a play test, 2026-11-20; stills of
  later builds of that day did not show it, with the filter off or on].
- **Pass order.** Three `Core3d` render systems, chained, after `Core3dSystems::MainPass` and
  before `Core3dSystems::EarlyPostProcess`: the grid pass (compute, span `nvf_medium_air`, one
  or four invocations per column, "The walk" above), the filter
  pass (compute, `nvf_medium_air_filter`, one invocation per texel) and the composite (a
  fullscreen draw, `nvf_medium_air_compose`) [design]. Why there: after both shadow
  passes have drawn the cascade maps and after the medium's cloud trace, on the finished HDR
  picture with the weather's fog in it (the light re-shades that fog), and before what takes the
  picture as given: FSR and DLSS, bloom, eye adaptation, tonemapping, the game's TAA and the
  screen-space Sun shafts. Three pipeline stages must be ready in the frame: the grid's, the
  filter's (with `FILTER` on; a filter pipeline still compiling or failed marks the frame not
  drawn) and the composite's. The composite checks that the volume it reads was written this
  frame before it flips the view's target, so a frame that cannot draw passes the picture through
  untouched.
- **Its own bind group.** The grid pass binds Bevy's public light resources in a group of its
  own: the lights uniform (`LightMeta`, with the view's dynamic offset), the directional shadow
  maps (a depth 2D array) and their comparison sampler, our uniform, the terrain map, the Cloud
  shadows image and its sampler, and the volume as a write-only storage texture [design]; the light volume's variant ("The visibility") adds its own uniform, the two levels and the
  mean's storage buffer (bindings 8-11) in a second layout. Not the mesh view group: Bevy lays that out for the fragment stage (vendored bevy_pbr
  `mesh_view_bindings.rs`, read from the code), so a compute pass cannot take it as it is, and a
  group of our own needs no patch of the vendored Bevy and adds no sampled texture to the mesh
  view group (two more arrays there could pass 16 sampled textures on small adapters
  [hypothesis]).

### The visibility

`V = K x min(C, T)` at each sample (`OCCLUDERS=all|cascades|terrain|none|clouds` picks the parts;
all by default) [design]:
- **C, the cascades as Bevy picks them** for a surface at that view depth (vendored bevy_pbr
  `shadows.wesl`): the light flagged as the outdoor sun with its shadows on (never the first
  light or a direction match); the first cascade whose far bound lies beyond the depth; within the
  overlap share of its far end mixed toward the next cascade as Bevy blends them; the point moved
  toward the light by the light's depth bias (air has no surface, so no normal bias); one
  hardware comparison; lit outside a cascade's box and beyond the last cascade's far bound.
- **T, the terrain map read as surfaces read it** (sun_visibility.wesl `nvf_terrain_visibility`,
  mirrored constant for constant): the four texels around the point, the point lifted by 16 plus
  twice the bilinear relief, four smooth comparisons combined bilinearly, lit fading in over the
  map's last four texels. The map counts only while its light is within a dot of 0.999 (about
  2.6 degrees) of the current one; otherwise, and while none is published, T is 1. Its producer
  runs for the light by its own demand (above), with or without the Terrain shadows row.
- **K, the clouds' shadow, from one source with the ground's** [design]: the light volume of the
  volumetric clouds where the surfaces' cloud shadows read it, else the Cloud shadows image. The air
  decides nothing: sun_visibility.rs `prepare_sun_visibility` leaves what the surfaces read this
  frame (`CloudVolumeFrame`: the source `AutoGate` chose, the strength, the reader's window with its
  presences, the two level views) and the air takes it as it is (medium_air_grid.rs
  `prepare_air_volume`, `air_cloud_source`). So the row, the aid
  `NVF_PLAY_DEBUG_CLOUD_SHADOWS_SOURCE`, the sky not shown, fronts missing, and an adapter without room for the surfaces' arrays send the air to the image when
  they send the ground there (under the aids `share` and `depth`, debug pictures of the reader on
  surfaces, the air reads the volume alone). Only the air's own pipelines not ready or failed, or an
  adapter with no storage buffer in compute, hold it to the image while the ground reads the volume.
- *The light volume* is read through the shared reader (light_volume_reader.wesl
  `lvr_optical_depth`; a test pins that the grid has no copy and that the mix after it is the
  surfaces' text), at the sample's world position, with the same light and the low-sun band of the
  image: `K = 1 - strength x (1 - exp(-depth))`, mixed over the image's K by the covered share
  (window guards, a level fading in), the image's alone where it has no data. The window is measured
  from the camera under this frame's drift and the reader subtracts that camera itself, so the moving
  origin needs nothing of the grid, which keeps no history. No march: for a sample under the slab
  floor one layer of one level (two in the near level's guard band), for one above it two layers of
  each. Toward a low sun a sample meets the floor beyond the near window, so its K is the far
  level's, of 4,000-unit nodes: softer than the near level's 500 [design].
- *The variant*: the shader def `AIR_LIGHT_VOLUME`, its pipelines asked for once the volume can become
  the surfaces' source (`volume_expected`): never while the Volumetric clouds
  row is off, with the default source only while the clouds are traced. The aid
  `NVF_PLAY_DEBUG_CLOUD_SHADOWS_SOURCE=volume`, `share` or `depth` asks for them, and for the volume's
  source, whatever is traced, with both rows on. Without the variant the layout, the uniform and the flat path are as before
  (tested).
- *The reference visibility* V_ref (below) must be the mean of the K that is read: the image is drawn
  relative to its own clear percentile, the volume's K is not, so with the image's mean an overcast
  would put all the air below the reference. While the volume is read a one-group dispatch (span
  `nvf_medium_air_cloud_mean`) takes the image's exact whole-pattern mean W (the CPU's `cloud_mode.y`;
  1 with the volume alone and from the weather plane up, where the image shades nothing) times the
  mean uncovered share, plus the mean of `share x the volume's K`, at 32 x 32 points 4,000
  units apart about the camera (two sums, so a dark mean is not lost against a bright W; a covered
  mean is held above 0, which the composite takes as no mean), and the result, with the clouds' own mean transmittance beside it ("The medium", the deck's share), is
  copied over the uniform's `cloud_mode.yz` before the
  grid and the composite [design; the extent and spacing are our choice]. The image is never sampled
  on that lattice, which would alias its pattern: with no coverage the reference is W exactly (no
  step when a front arrives), under full coverage the volume's mean alone (tested with the checker).
  The pass, the copy and the wait for its
  pipeline are left out where the composite reads neither number: both `REF` and `SHARE` fixed,
  the diagnostic views, `off`.
- *The Cloud shadows image*: the image while the Cloud shadows row publishes
  it, read as surfaces read it (sun_visibility.wesl `nvf_cloud_weather`): the ray from the
  point toward the light meets the cloud plane, K = 1 - strength x (1 - the image there), fading
  in between light elevation sines of 0.02 and 0.1; 1 at or above the plane, with the light lower,
  or with nothing published. Its **mean** over the pattern's repeat for the current light
  (from the image's texel mean: K is affine in the texel, so the mean of K is K of the mean)
  rides in the uniform as the reference visibility's default (below); 1 with the camera at or above
  the plane.
- **Not yet a source**: local lights (the light in the air is the sun's, and by night the moon's).

### The medium

The medium in stage 3 is the weather's own fog, and the light is a **re-shading of the fog's own
light, in the fog's own blending space** [design]. Along a view ray the fog gives a surface at
distance d the share `F(d) = x^power x strength`, `x = clamp((d - near) / max(far - near, 1), 0,
1)` (fog.rs `GAME_FOG`), read each frame from the camera's effective `DistanceFog` as the world
shaders get it (weather fades, fractional alpha and `--fog` included).

**The Mojave's fog is front-loaded.** The shares at four distances for the base game's day fogs
[hypothesis: read on 2026-11-20 with fnvtool from the WTHR records of an installed game, whose version and
the command were not kept: the near, far and
power columns are their FNAM day values; the F columns
are computed from them with fog.rs's curve and are not fields of the records]:

| Weather | near..far, power | F(3,000) | F(12,000) | F(50,000) | F(130,000) |
|---|---|---|---|---|---|
| NVWastelandClear (and 10 more with the same numbers) | -10..200,000, 0.6 | 0.08 | 0.19 | 0.44 | 0.77 |
| NVWastelandGS | 10..120,000, 0.5 | 0.16 | 0.32 | 0.65 | 1 |
| NVWastelandHazy | 1,800..30,000, 0.6 | 0.15 | 0.54 | 1 | 1 |
| NVTheStripWeather / NVUrbanClear | 2,000..200,000, 0.35 | 0.16 | 0.35 | 0.61 | 0.86 |
| NVHooverDamWeather | -1,000..100,000, 0.6 | 0.14 | 0.29 | 0.66 | 1 |
| NVColoradoRiverWeather | -100..75,000, 0.5 | 0.20 | 0.40 | 0.82 | 1 |

With powers of 0.35 to 0.6 the fog's density per unit of distance is highest at the camera and
falls with distance: a 12,000 grid holds a real share (0.19 to 0.54), so shafts round buildings,
rocks and the player's own surroundings can show, while a far ridge's share lies mostly beyond it
[design: what the table means for the grid].

**The blend is in display values**: `shown' = mix(surface', C', f)` with `x' = x^(1/2.2)` and
back with `^2.2` (fog.rs `GAME_FOG`, `bi.x > 0.5`, the outdoor variant). In linear light the fog
over a dark surface adds far less than `f x C` (a black surface at f = 0.19 shows `0.19^2.2 =
0.026 C`), so a re-shading in linear light (taking `s x f x C` out of shadowed air) would take
more light than the fog put there and clip dark ground to black. So the light is applied where
and how the fog was: in display values, with the fog colour C' as the budget near the camera,
bounded by the share the fog gave [design].

**The sunlit share** s: the share of the fog's light taken to come from the sun, `d / (d + a)`
from consistently scaled lights as sky.rs sets them this frame: d the Rec. 709 luma of the sun
light's displayed colour times `illuminance / (pi U)` (so the image space's sunlight dimmer, the
screen modifiers and the horizon fade are in it), a the luma of the ambient's colour times
`brightness / U`; the denominator taken as at least 0.001, so s falls with the direct light near
black (medium_air.rs `scaled_share`, its module doc names sky.rs's fields) [design].
`SHARE` overrides it; `a = clamp(s, 0, 1)` is the share alone. The strength k multiplies the
whole correction: the uniform's scalar is `k x a` (medium_air.rs `applied_scale`, `phase2.z`), applied
before the floor, so k 0 changes no pixel and the floor holds at any k.

**The moonlight** [design]: at night the sky's direct light is the moon's, from the opposite side in the weather's night
Sunlight colour, faded out at the horizon (sky.rs `aim_sun`), and the air is lit by it as by the sun. The source (medium_air.rs `AirSource`, `air_source`)
is the sun while the elevation that aimed the light is above 0
(`SkyState::sun_elevation_at(sun_light_hour())`, the hour sky.rs aims the light from, so no comparison
with the stepped hour's sun is needed), the moon at and under it where the climate has a moon
(`Climate::has_moon`), and nothing otherwise: the gate is then closed. At the horizon the light's
illuminance, so the share, is 0 on both sides, and the change of source cannot show (tested). The share is the same `d / (d + a)`, so
the moon's share of the sun's strength and MoonlightNVSE.ini's factor come with the illuminance, once. The moon's
constants are its own (`MoonTune`, `AirTune::lit_by`; [design], the contributor's pick from stills, 2026-11-25): the strength's factor
`MOON_STRENGTH` (1) times the aid's or the slider's strength, rays 8, lift 1.6, shade 0.4, phase g 0.7, cap 3, sky 0.3, sky phase 0.15
(`MOON_RAYS`, `MOON_LIFT`, `MOON_SHADE`, `MOON_G`, `MOON_CAP`, `MOON_SKY`, `MOON_SKYPHASE`); each `auto` is the day's. By day none of them is read (tested: the same uniform bit for bit). The clouds follow
their own clock, the sky's stepped hour (graphics-enhanced.md §3.23, "By moonlight"): the two are not joined.

**The deck's share** D: the weather's lights still name a direct part under a closed deck (an s of
0.62 in an overcast noon, 0.65 in a fair one [hypothesis: the figures this was worked for on 2026-11-24;
how they were read was not kept]), so the light lifted an overcast picture as a whole. The
shader's scalar is `k x a x D`, `D = AIR_DECK_LEAST 0.25 + 0.75 x smoothstep(AIR_DECK_FROM 0.40,
AIR_DECK_TO 0.85, T)`, once a
pixel (medium_air.rs `deck_share`, medium_air_compose.wesl `air_deck_share`): what is added and what
is taken are weakened alike, the sky's and the dome's too. T is the clouds' own mean transmittance for a
light from straight above, in `cloud_mode.z` (the reference visibility reads the mean of K in y, which
it is not: K carries the shadows' strength and their fade at a low sun, and the weather image is relative
to its own clear percentile, so a closed deck and a clear sky can be the same image):
- **The light volume's.** The mean pass sums `covered share x exp(-optical depth)` at its points beside K's parts,
  with neither the strength nor the fade; over the covered share that is the mean along the light's
  slanted way, and to the power of the light's height (held to 0.1..1) the one straight up: Beer's law
  in the path's length, for a slanted way through the same clouds is 1 / up as long. It is taken as at
  least 1e-4 (`AIR_DECK_THIN`), so an opaque deck stays closed at every height. **The power is of the points'
  mean**, not of each point's depth before the mean (`exp(-depth x up)`): of clouds about as tall as they are
  wide a lower light meets more, since their slanted shadows lengthen, and the gap fraction falls as that
  same power; a point's own power keeps an opaque cloud opaque, so those longer shadows would read as more
  cloud. In one clear weather the mean's power moved little with the hour (0.867, 0.879 and 0.893 at noon, 17:30
  and 18:36, below; the share 1 at each), where the point's would read about 0.87 at noon and 0.7 at 18:36 [hypothesis: the same
  stills; the second an estimate] [design, 2026-11-24].
- **The flat layers'.** `Π (1 - mean alpha x weight)` over the layers the dome draws, of both weathers of a fade with
  their weights on the dome (cloud_shadows.rs `flat_transmittance`): the absolute product's mean over every
  offset of the layers, so it does not move as they drift, and over a fade it goes from the one weather's
  to the other's with the dome's weights, frame by frame. While a new texture of a fade is read the last stays; a weather replaced at once and another
  worldspace start from 1 until theirs are read. 1 with
  Cloud shadows off, indoors, no layer's alpha read, and from the cloud plane up (known without the image); a 0
  is an opaque deck.
- **Between them** by the covered share, no step at the first sliver of coverage and nothing of either strength.
  Both describe the same deck: where a low light's ways leave the volume's window, and under the
  shadows' band where the volume is not read, the flat layers' stands for it (medium_air.rs
  `deck_transmittance`) [design, 2026-11-24; tested].

A first form took the smoothstep of the mean of K, which fell with the sun and took 8 to 20 % of a clear
evening: in one clear weather it was 0.918 at
noon (up 0.975), 0.869 at 17:30 (0.525) and 0.815 at 18:36 (0.306); the volume's T is 0.867, 0.879 and 0.893, D 1
[hypothesis: read back from pairs of stills of 2026-11-24 with a development build, K turned into T by
its strength of 0.6; the steps were not kept]. A closed deck keeps a quarter of the light for its breaks; with
none left an overcast noon had no light at all [hypothesis: the same stills]. The CPU's
share, the gate and the log's bounds are before D, so they are upper bounds (m may be known on
the GPU alone). `SHARE` set is an absolute share: D is then 1 (`rays2.w`) [design, 2026-11-24;
tested].

**Weather mods.** Every input comes each frame from the camera's fog and the live sky; nothing is
keyed to a weather. A fog the light cannot read (a non-finite field, far not beyond near, power
or strength not above 0) is shown the plain way, as is a fog not blended in display values
(tested with synthetic weathers: far 0 and 1e6, powers 0.05 and 8, NaN) [design].

### The composite

A fullscreen pass at the main pass's render size (its viewport and origin; outside it the picture
passes through) that reads the HDR picture, the depth prepass at its real sample count and the
volume (linear, clamped), and writes the alternate HDR target, alpha kept. **Per depth sample**,
as the shader applies it (medium_air_compose.wesl `air_pixel`, `air_result`; mirrored whole by
medium_air_compose.rs `pixel_mirror` over medium_air.rs's functions):
1. The sample's view depth z from reverse-Z depth (`near / depth`, the first-person shapes'
   squeezed depth undone first) and its radial distance `d = z x ray length`. Cleared depth (the
   sky where nothing wrote depth) takes the grid's end: z the grid's depth, d the grid's depth
   times the ray's length. The sky dome itself is opaque and in the depth prepass (sky.rs, at
   `SKY_DEPTH_FROM` and beyond), so most sky pixels are samples with depth, not cleared depth.
2. **The lit fraction** `Vbar = clamp(N / M, 0, 1)` from one trilinear lookup at the sample's
   slice coordinate, held to the grid's end (texel k holds boundary k + 1; the camera is a zero
   boundary, so inside the first slice the sums scale with the coordinate; beyond the end the
   end's value is carried: the near air in front of far ground keeps its light); 1 while M is at
   most 1e-9. Phase-free: the dF-weighted mean of V in front of the sample. *Follows from the fog.*
3. **The relative form** `Vbar_c = clamp(1 + RAYS x (Vbar - V_ref) / V_ref, SHADE, LIFT)`.
   V_ref, the reference visibility, is `REF` when set (0.05..1), else the clouds' mean K (0 or
   not finite taken as 1, held to 0.25..1). What the code does (medium_air_compose.wesl
   `air_relative_lit`): air as lit as the average gets `Vbar_c = 1` exactly, whatever RAYS is;
   RAYS (1 is physical) scales the departure from the average; the result is held between SHADE
   and LIFT, so shadowed air keeps at least `SHADE` of its relative light and lit air rises to at
   most `LIFT`. `Vbar_c = 1` does not leave such air unchanged by itself: the phase still applies
   (step 4), so toward the sun a phase above 1 brightens average air, and only with `p_eff = 1`
   (FLOOR 1, away from the sun) is it left as the weather drew it. Why relative: the weather's fog
   colour was drawn for the average light under that weather's cloud [design]. What stills
   showed [hypothesis: 2026-11-20 at Goodsprings, a development build of that day with the first,
   absolute form; the views, weathers and size were not kept]: that form darkened
   every picture as a whole (-4 levels at its defaults, -17 at RAYS 4) and no ray stood out, since
   the air under broken cloud is never fully lit; with no shade limit at RAYS 6 a slightly
   below-average lit fraction went to no light at all. Shadowed air is meant to be no darker than the
   open haze [design]. *Artistic choices: RAYS, LIFT, SHADE and the relative
   reference itself.*
4. **The phase** p of the pixel's own ray against the light: two Henyey-Greenstein lobes mixed by
   `BACKWEIGHT`, capped and normalised on the CPU so that the final function has a sphere mean of
   1 and its largest value is at most `CAP`: `CAP` is an upper limit, reached only when the
   normalised lobes rise above it (with `G = BACKWEIGHT = 0` the phase is 1 everywhere whatever
   `CAP` is; medium_air.rs `AirPhase::normed`) [documented: Henyey and Greenstein 1941 for the lobe; Bauer
   2019, slides 29-31, for mixed lobes; our cap]. Then `p_eff = max(p, FLOOR)`. *FLOOR is an
   artistic choice*: the mean-one phase is well under 1 away from the sun, so with FLOOR 0 (the
   energy-neutral phase) every view away from the sun darkened by 12 to 21 levels at g 0.6 to 0.8
   and cap 2.5 to 4 [hypothesis: stills of 2026-11-20 at Goodsprings, a development build of that day; the
   views and the size were not kept], while the fog colour was
   authored for lit open air; FLOOR 1 keeps lit air away from the sun as the weather drew it and
   lets only the forward side gain. The cap bounds the glare toward the sun (the sky dome and
   Light glow already draw the sun's aureole) [design]. g and CAP were chosen by eye: a first g 0.6 with
   cap 2.5 gave the hill toward a low sun a hazy glow with the near house clear [design: judged by eye, the
   same stills]; the defaults are lower since ("Debug aids, tune keys and log lines").
5. **F_near** = F at `min(d, the grid's depth x the ray's length)`, analytic: never above the
   sample's own share. *Follows from the fog.*
6. **The blend** `t = t_d + (1 - t_d) g` between the world's rule (0) and the sky's (1), the same t
   in the correction and in its budget (below). **The far blend** t_d: 1 for cleared depth, else
   `smoothstep(0.6 S, S, d)`, S sky.rs
   `SKY_DEPTH_FROM` (157,500 units): 0 near, 1 for the sky, continuous in value and slope between,
   so far terrain (which lies beyond S too) reaches the sky's form with no step on a hillside
   [design]. The sky dome's samples have depth beyond S, so they
   reach t = 1 by their distance, as far terrain does; only cleared depth takes t = 1 as such.
   **The fog blend** `g = smoothstep(AIR_FOG_BLEND_FROM, 1, F_near)`: the world's rule adds F_near of
   the fog colour's gain whatever the surface, while the sky's is weighted by SKY, so a plain or a
   pylon the fog nearly covered stood out white against the sky behind it. With g a surface the fog
   covers (F_near 1) takes exactly the dome's result, one the fog hardly covers keeps the world's
   rule, and the union of the two blends has no break in slope where they cross [design, 2026-11-24;
   tested]. The start, 0.35, is a start value to be set by stills: one near 1 only moves the white
   band nearer, for at F_near 0.8 the additive rule already adds 0.8 of the gain [design, 2026-11-24].
   F_near stops at the grid's end, so a surface beyond it in a fog that covers it only there has g
   below 1 and part of the world's rule [design; known limits]. *The start is an artistic choice.*
7. **One continuous form** in display values (`k a` with the deck's share D in it, "The medium"):
   `corr_t = max(k a F_near (Vbar_c x mix(p_eff, mix(1, p_eff, SKYPHASE), t) - 1) x mix(1, SKY, t), -min(F_near, 1))`,
   `delta = corr_t x mix(C', Y', t)` per channel, and for a sample with depth (not cleared)
   `delta = max(delta, -F_near x C')`; `Y'_new = Y' + delta`, `Y' = max(pixel, 0)^(1/2.2)`,
   `C' = max(fog colour, 0)^(1/2.2)`, the result `max(Y'_new, 0)^2.2` held to 0..60,000 per
   channel (medium_air_compose.wesl `air_result`). Near (t = 0) it is the additive world rule with
   the fog colour as the budget: what the fog put there is re-shaded. A sample with depth never
   loses more than `F_near x C'` per channel at any blend t [design; tested]. For a single-sampled fogged opaque fragment `Y' = (1 - f) S' + f C'` the
   result is therefore never below `(1 - f) S'` per channel, far terrain included. Under MSAA
   every sample starts from the pixel's one resolved colour, so the floor holds against that
   colour, not against each sample's own surface: three unfogged white samples and one fully
   fogged sample with `C' = 0.1`, fully shadowed at t = 0, give about 0.708 in linear light where
   0.750 of surface light truly survives [design]. For the sky
   (t = 1) the budget is the pixel's own display value: the sky's light is modulated by the lit
   fraction of the air in front of it [documented: Bauer 2019, slide 62], weighted by SKY (0..1)
   and taking SKYPHASE (0..1, default 0) of the phase, since a sky that took the phase whited out
   near the sun [hypothesis: judged by eye, the same stills]. Cleared depth keeps the scalar
   floor `-min(F_near, 1)` [design]. The sky dome, having depth, keeps
   the per-channel bound `-F_near x C'` like every sample with depth: shadowed air darkens the
   dome only within the fog colour's budget (with `C' = 0` not at all), while cleared sky would
   take the full modulation. The dome cannot be told from far terrain by depth: its mesh is the
   weather's own `atmosphere.nif` scaled to `DOME_RADIUS` (sky.rs), so its distance varies with
   the direction and with the data, and distant terrain reaches beyond `SKY_DEPTH_FROM` [design; known limits].
   *SKY and SKYPHASE are artistic weights; the budget, the floors and the display space follow
   from the fog.*
8. **The bright hold**: a gain (`corr_t` above 0) on a pixel that is already bright pushed a lit
   cloud past white. It becomes `corr_t (1 - t h)`: whole for the sky, none near, shadow untouched.
   With y the largest channel of Y' and `gain = corr_t x mix(c, y, t)` (c the largest of C'),
   `over = gain - max(AIR_BRIGHT_FROM - y, 0)`, `room = max(AIR_BRIGHT_TO - max(y,
   AIR_BRIGHT_FROM), 0)` and `h = over / (over + room) x over / gain` (0 while over is at most 1e-6):
   nothing is held back while the lifted value stays at or under `AIR_BRIGHT_FROM` 0.50, what is over it
   is fitted into the room left under `AIR_BRIGHT_TO` 0.76, and a sky pixel at or over 0.76 gains nothing
   (display values before tonemapping; medium_air.rs `bright_hold`, `hold_bright`). One scalar for the three channels, so a
   hue keeps; the brightness is the pixel's resolved one for every MSAA sample, t the sample's own;
   `delta` and `rays` show the held scalar. A brighter pixel never comes out darker under the sky's rule, and
   a grey one nowhere, at any strength (tested at the Natural look and a stronger one, LIFT 2, CAP 2.5, SKY 0.3, SKYPHASE
   0.2, at 200 %); between the two rules a strongly coloured pixel in a fog of another colour can lose a few
   thousandths of a display value as it brightens, since the two largest channels are not the same one.
   A fade of the gain over the input's own brightness between the two ends cannot hold that: the
   stronger look's sky gains up to 0.96 of itself, which lifts 0.50 to 0.98 while 0.76 stays [design,
   2026-11-24]. **Why the ends are so far under white**: the picture's grade comes after this pass and
   brightens, by about 1.2 in an evening scene (a shown 210 of 255 was 0.68 before it), so white is
   reached there at about 0.82; with the first ends, 0.70 and 0.95, 25 % of a bright cloud still
   clipped (16 % with the light off, 30 % with no hold), and with 0.55 and 0.80 20 % [hypothesis: read back from stills of
   2026-11-24 with a development build; the steps were not kept]. How much the grade brightens depends
   on the scene and this pass does not know it, so the ends are a fixed heuristic, an artistic choice
   [design, 2026-11-24].
9. **The clamp**: the picture is read held to 60,000 per channel (an infinity times a scalar of 0
   would be no number), and every colour the pass computes, the debug views too, is held to 0..60,000 per
   channel (`AIR_HDR_MAX`, inside `Rgba16Float`'s largest finite value 65,504 [documented: IEEE
   754 binary16]), so bloom and eye adaptation never meet an infinity, also under the extreme
   keys (tested) [design]. Every pixel inside the main pass's viewport
   goes through it, so an input above 60,000 (up to 65,504) is cut there even where the
   correction is 0; the pass-through outside the viewport is not clamped.

**Under MSAA** the pixel takes the mean of its samples' results (colours in linear light, as
resolved). When their view depths, held to the grid's end, lie within a quarter slice of each
other, the samples share one volume lookup at the farthest of those depths (free of the samples'
order, and its sums hold all the fog in front of the nearer ones); otherwise each sample looks up
its own. Each sample still takes its own F_near and blend from its own distance [design]. Each sample's
correction is taken on the resolved colour, an edge approximation under a display-space blend,
exact for none. Nothing needs multisampling on or off.

**The bounds in the log** (medium_air.rs `ViewBounds`, `view_bounds`, `on_line`), in display
values: `near pixels at most +<g1> x the fog colour, far pixels at most +<g2> x mix(fog colour,
pixel, t), sky at most x<1 + gs> (display values; a snapshot for this weather, hour and view)`,
with `g1 = (LIFT x most - 1) x k a x F`, g2 the largest over t of `(LIFT x mix(most, mix(1, most,
SKYPHASE), t) - 1) x mix(1, SKY, t) x k a x F` (the far form's phase and weight at that t; terrain
in fog has a t above 0 too), and
`gs = (LIFT x mix(1, most, SKYPHASE) - 1) x k a x F x SKY`; F is the share at the grid's end on
the view's longest ray (the corner ray), so no pixel's F_near is above it: a conservative bound
for the whole view [design]. The
line is written again on a tune change and when a figure moved by more than 0.05, looked at
every 5 s (`bounds_moved`, `BOUND_RELOG_SECONDS`); the "at most" figures are printed rounded up
to a thousandth, and the phase they need is solved once per change of its four inputs and
cached. Before the calibrated defaults, at 17:30, the
older form of the line read `world at most +1.200 x the fog colour, sky at most x1.267`, while
the measured pictures stayed far under it: the picture's mean moved by -7 to +16 levels of 255
against the light off at 07:00, 12:00, 17:30 and 18:30, with no view clipped, also facing the sun
[hypothesis: stills of 2026-11-20 at Goodsprings with the development build b757be01f; the views, the
weather and the size were not kept].

### Solid cores of blended shapes

An engineering choice of the enhanced look, never a parity claim (solid_cores.rs,
nif_spawn.rs `solid_core_rules`, `solid_core`).

**Why.** The composite tells each pixel's distance from the depth prepass, which holds no blended
material. A shape whose NiAlphaProperty blends but which is solid to the eye was painted with the light
of what lies behind it: the water tank by Goodsprings (`architecture\goodsprings\nv_watertank.nif`,
reference 00106B5E, flags 4333: blend on, no test) [observed, 2026-11-24: the light seen on it, judged
by eye in a play test by the tank with the Volumetric light row on, a development build of that day; the
flags read from the model's NiAlphaProperty]. About 875 lit blended shapes, drawn by
about 8,300 placed references, are of its plain class [hypothesis: counted on 2026-11-24 with a parser
that is not committed].

**What.** While the light in the air is drawn (and only then: from its gate's "on" line to its "off" line,
`AirDraws`, so not at night without a moon, indoors, under water or at a strength of 0; hidden from the first frame the
composite stops, `AirOpen`, gone half a second later) such a shape is drawn once more as its
solid core: the same mesh with a copy of its material as a cut-out at 0.85
(`SOLID_CORE_CUT`), so its solid texels are in the depth prepass. The blended draw stays and is drawn
over the core at its depth (the main pass's test is greater or equal; a hair model's layer relies on
the same), so the soft edges are the blend's. The core's own edge is hard: under multisampling a soft
one would widen the solid part.

**Two cuts.** 0.85 where the texture is known to be mostly solid: of its top level's texels at 0.85 and
above, three quarters or more are at 250 of 255 or above, so the rest are the edges of what is solid. 0.98
otherwise (`SOLID_CORE_CUT_THIN`): a texture with thin parts in that range too (an atlas with a pane), and
BC7, whose alpha is not read here (no CPU decoder) [engineering choice; texture.rs `SolidTexels`]. A BC7
replacement of the tank's texture may so show the line along its rim again.

**The colour is close, not the same.** Where the texture's alpha is between 0.85 and 1 the
blend let up to 15 % of the background through; that part is now the core's colour (the first cut, 0.98,
left the border texels of the tank's texture out, 0.89 to 0.96, and the light behind drew a bright line
along its rim [design: judged by eye in a play test, 2026-11-24]). And the core is not
shaded exactly as the blend: ambient occlusion is compiled only into pipelines that write depth (vendored
bevy_pbr mesh.rs, `NVF_AMBIENT_OCCLUSION`), so the core takes it and the blend does not. What shows of that
is the same up to 15 %: the blended draw covers the rest.

**Which shapes.** By the file (once, at the material's build): blended SRC_ALPHA / INV_SRC_ALPHA, lit,
the shader flag ZBuffer_Write set (BSShaderFlags2 bit 0 [documented: nif.xml]), a base texture, and
nothing but that texture sets its alpha: material alpha 1, no vertex alpha, no vertex-alpha cut, no
alpha animation, no view-angle falloff, no decal, hair, skin, refraction, window reflection
(drawn premultiplied) or object LOD block. By the texture: some texel at 250 of 255 or more, the top
level read once per file on a background task and only while cores are wanted (a wholly opaque
texture's shape is drawn opaque already, `blend_is_opaque`). That answer is the file's, not the
shape's: BC7 counts as having solid texels (no CPU decoder), and one solid texel of a shared atlas
qualifies a shape that never samples it. The cut decides per pixel: at 0.98 such a shape's thin parts
stay out; at 0.85 a part between 0.85 and 0.98 would be in, which is why that cut asks for a mostly solid
texture ("Two cuts"). By the material as it stands now: still that blend, alpha 1, the
texture it was built with, no alpha cut, holes, height map or other special draw. By the mesh: a placed
reference's, not skinned, not morphing, under no material override, not under an actor's
rig. Where the mesh terrain blend draws a clone of its material, the core is the plain material's while the clone has
the same alpha outline, and none while it has another's (its replacement is not ready). The corn rows (`blended_cutout`) qualify while they are drawn blended; under a temporal
upscaler they are alpha-tested and have no core. Authored material sets are followed (they change
the normal, roughness and metal of both draws); height maps are drawn on no blended shape.

**Following.** The core is a loose entity, nobody's child, linked to its shape by a map kept in solid_cores.rs.
The shape gains no component and no child, so nothing that walks or queries a reference's entities (the
scripts' NIF nodes, the reference's root, the flat-drawn items) can tell whether the light is on. Every frame,
between the propagation and the visibility check, the core gets its shape's world matrix and inherited
visibility, bit for bit: it has no `Transform` or `Visibility` of its own, since a matrix taken apart and put
together again is not the same bits and the two draws would not lie at equal depth. Motion vectors come
from the world matrix of the frame before and stay right. Its mesh, layers, range and shadow receiving are
copied, removals included (room culling shows a mesh again by taking its layers off); it ends with its
shape's mesh (cell unload, travel, load) in the same frame. Every fade, script alpha, stealth field, effect-shader hole and
texture swap gives the mesh another material or edits its own. The one system runs in `PostUpdate` after
those, the mesh terrain blend's conversion and the asset events, and before the visibility check, so a
core goes from the frame its shape's change is drawn in. (In `Last` it would not: the renderer keeps its draw
bins, and takes an entity out when it left the frame's visible list or the specialization check listed it,
both of `PostUpdate`.) There is one core material per source material. Its write reaches the renderer a frame
after the source's edit, so an edit of the alpha outline (texture transform, base texture, alpha, cut, holes)
ends the core at once and the material has none again for as long as it is in the store; any other edit
(emissive, gloss) is followed a frame late, under the blended draw. A core made anew under a moving
shape (another mesh or material, or its first or last visibility range) has no motion in its motion vectors for
one frame: the GPU's instance history is kept by entity.

**What changes besides.** A core is in everything that reads the depth prepass or the opaque
picture: ambient occlusion round the shape, soft intersections, motion vectors, the copy the refracting and
additive shapes read. A blended shape behind a core fails the depth test where the blend would
have covered it anyway, and a concave shape's own far faces no longer show through its near ones
where they were drawn out of order. All of it only while the light is drawn.

**What stays wrong.** Glass and everything partly transparent (no solid texels), particles, water,
windows, unlit effects, the soft edges themselves, skinned shapes, first-person and menu models: they
take the light of what is behind them. **Shadows are unchanged**: a core casts none, and the blended
shape casts none, as before.

**Cost.** Off: two run conditions, and one optional texture name kept per candidate material. On: one extra
draw in the prepass and one in the main pass per loaded candidate (batched as other cut-outs are), a pass
over the placed meshes' change ticks, one lookup and comparison a core, the material events and the removals each frame, one texture read per file.
Measured (NVIDIA GeForce RTX 5070 Ti, four alternated pairs a view, one exe, cores on against
off): the tank view, 34 cores in the loaded cells, frame +0.04 to +0.08 ms (median +0.07 of 3.2), GPU +0.07 to +0.14
(median +0.09); the view over the town from the east, frame -0.02 to +0.05 (median +0.01 of 3.6), GPU -0.03 to +0.01
[observed, 2026-11-24: a development build of that day whose commit was not kept; `--play --cell Goodsprings
--hour 7 --weather NVWastelandHazy --still-sky --no-actors --no-messages --no-voice --sun-shadows
--graphics-only volumetric-light --msaa 4 --size 1920x1080 --log-frame-times 5 --run-seconds 75` with
`NVF_PLAY_DEBUG_CLOUD_PHASE=100`, `--pos=-68354,2129,8372` looking to heading 176 (the tank) or
`--pos=-66000,-2000,9200` to 270 (the town), the median of the last eight periods of a run, the pairs'
differences; off is `NVF_PLAY_DEBUG_SOLID_CORES=off`].

The developer aid `NVF_PLAY_DEBUG_SOLID_CORES` (not in release builds) `=off` draws none (the pair for pictures); `=ref:<FormID hex>` logs once, when
the count has rested and a core of that reference is in view (or after 20 s without one), `solid cores:
reference <FormID>: <n> cores (<m> of them in view)`, and once, when some are drawn and the mesh terrain blend draws their shapes,
`solid cores: reference <FormID>: <k> cores in view under a terrain blend clone`. In view: some view's visibility check passed it. Log: `solid cores: <n> (draws of
<m> materials, for the light in the air)` once the count has rested a second, again when it is notably
another; `solid cores: removed (...)` when the light is no longer drawn; nothing with the light off.

### Where we differ from the talk

[documented: Bauer 2019, slides 42-48, 62, 64; engineering choices]
- **No temporal filter and no TAA to lean on.** The talk jitters its samples over frames and
  resolves them in time, with TAA after (slide 48). Our default anti-aliasing is multisampling
  with no temporal pass after it, so the grid's samples are fixed in time, spread in space
  (several dithered samples per froxel) and filtered in space (the 3 x 3 tent); a temporal filter
  of V is later work.
- **One light.** The talk's grid gathers local lights and fog volumes; ours the sun, or the moon, alone.
- **A fullscreen, opaque-world approximation** instead of applying the volume in the deferred
  lighting and to forward-drawn surfaces (slide 64): the composite re-shades the finished
  picture by its depth [design]. The production route, if the look is kept: sample the
  volume inside the fog function of the main pass (exact per fragment and per MSAA sample,
  blended surfaces included). Not built.
- **The sky modulated by the lit fraction** (slide 62) rather than a scattering volume of its own
  for the sky: the sky dome is drawn without fog, so "the fog's own light" is not defined there,
  and the dome already draws its own glow round the sun.

### Known limits

- The fullscreen composite cannot know which fragments were fogged: the scope's image, some
  particles, the water's mix and emissive overlays take the correction of what the depth shows.
- The deck's share knows the deck only while Cloud shadows is on (1 without). The flat layers' T leaves out the
  dome's own fade towards the horizon and takes the layers as independent, and how well it agrees with the
  volume's T for the same weather is not measured: where they differ, D moves as the light sinks and the
  ways from the mean pass's points leave the volume's window (under a light height of about 0.1 [hypothesis:
  from the window's size and the planet's curve; at 0.06 about half is left in the test's fixture]), and under
  0.1 the volume's power is held, so a thin sheet reads somewhat thicker there. The volume's power of the
  light's height fits an even sheet and clouds about as tall as wide. A broken deck without height, whose gaps no
  slant closes, reads clearer at a low sun: half clear and half opaque, D is 0.35 with the light overhead and
  1 at a height of 0.25 (a test states it). A closed deck's quarter is for its breaks and
  is there without any [design, 2026-11-24].
- The bright hold's ends are fixed while the grade after it depends on the scene: one that brightens more
  may still clip a lit cloud, one that darkens is held sooner than it need be. It judges a pixel by its
  resolved colour: at a silhouette under MSAA the mix of both
  sides decides for each.
- The fog blend reads the fog share at the grid's end at most (F_near): terrain beyond the end in a fog
  that covers it only there keeps part of the world's additive rule.
- Blended surfaces (water, glass, particles) get the light of what lies behind them; the solid texels
  of placed blended shapes do not ("Solid cores of blended shapes").
- With the Mesh terrain blend row on, a converted shape's core has the plain material's colours, not the landscape's
  near the ground; the clone is drawn over it, so they show through at most 15 % of the colour there.
- Desert haze: its veil has two colours; the light changes only the weather fog's `f x C'` term
  near the camera and leaves the haze's term alone; far away (t toward 1) the budget is the
  pixel's own colour, so the haze is modulated only through it. Neither it nor aerial perspective doubles
  the light: with both on it adds to a picture what it adds without them, to 0.03 of 255 in the mean and 0.5
  in any 20 x 20 block [observed, 2026-11-25: a development build of that day, Goodsprings, 1920x1080, a stronger look (LIFT 2, SHADE 0.85, RAYS 7, G 0.7, CAP 2.5, SKY 0.3, SKYPHASE 0.2), two views at
  07:00, 12:21, 17:30 and 18:36 under NVWastelandClear, NVWastelandHazy (where both are off by their own rule) and
  NVBlackMountainWeather; still sky, clouds' phase held]. Not looked at in a wide vista.
- The dither is fixed: it stays with the screen while the view pans (the filter softens it; a
  temporal filter is its cure).
- FSR's reactive mask compares the picture after the air with its opaque reference captured
  before, so it reads the air's change as transparency on opaque terrain and sky [design; deferred].
- The composite's ray uses the raster pixel without the upscalers' temporal jitter offset (the
  depth texel itself is the right one) [design; deferred].
- Night without a moon is off ("The moonlight", under "The medium").
- The moonlight's shafts come from what its light is hidden by: the one near shadow cascade of the night, the terrain map (which
  follows the light's own direction, the moon's too) and the clouds. They are faint: by 22:00 and 04:00 in clear weather at
  Goodsprings a cloud's shadow streak and a pole's shadow show by a few levels of 255. The whole picture's mean moves by -0.6 to +1.9 of 255 against the same clouds with the air off
  in clear weather (most facing the low moon), by -1.8 in the hazy weather facing the moon, and by under 0.3 in overcast and Zion's weather [observed:
  NVForever runs, 2026-11-25, 1920x1080]. The bright hold, the moon's disc, the Strip's lamps, the Sun shafts' yield in
  the first degrees after sunset (they end at 2 degrees under the horizon, and give way to the moonlit air from
  its first frame) and FSR are not looked at by night.
- In a frame in which the light's share is exactly 0 at the horizon the air draws nothing, so Sun shafts may draw in it, though
  their own fade is above 0 down to 2 degrees under the horizon [design; not seen].
- A halo round the moon in the sky (`MOON_SKYPHASE`) multiplies the sky's own light (the composite's far
  form), so a black zenith stays black: what it can add there is bounded by the sky's own value.
- Interiors and under water are off; so are a fog not blended in display values, an unreadable
  fog, a strength of 0 and a sunlit share of 0 (the gate's reasons, below). While the gate is
  closed the grid and filter passes are not dispatched for the light's views (`none`, `delta`,
  `rays`) or the `off` baseline; the volumes stay allocated (medium_air_grid.rs
  `grid_dispatched`). The diagnostic views (`visibility`, `lit`, `slice`, `share`) run the grid
  whatever the gate.
- One grid, for the main view only: the scope's, the reflections', the map's and a mod's extra cameras'
  views have none.
- **Hazy weather shows little.** In NVWastelandHazy at 17:30 the calibration measured 0 to +2
  levels of 255 against the light off, where the clear weather gave +2.6 to +5 [hypothesis: stills of
  2026-11-20 at Goodsprings, two development builds of that day; the views and the size were not kept]. Why is not established; a fog dense near the camera and a
  rule measured against the clouds' average may leave hazy air little contrast to show
  [hypothesis].
- The sky dome takes the per-channel bound of a sample with depth, so the air's shade shows on
  it only within the fog colour's budget (above, step 7). A coverage mask drawn with the dome's
  own mesh would tell dome from far terrain and lift the limit; not built.
- The cloud mean that sets the reference visibility is forgotten while the light is not wanted
  and worked out again on the first frame back, so it never describes another weather whose
  texels were rewritten under the same image handle meanwhile [design].

### Cost and memory

GPU spans at 2560x1440 with four samples a froxel, each pass's own time (absolute: not compared with a
frame without the light): the grid pass 0.21 to 0.31 ms, the composite
0.14 to 0.15 ms [hypothesis: a run of 2026-11-20 with a development build before the per-stratum fog
steps and the filter pass; the start point, the steps and the card were not kept, and another build ran
on the machine].
The three passes have their own spans; the filter pass's (`nvf_medium_air_filter`) is not
measured on its own yet, and whole-frame times are not measured yet in a quiet window.
The four chunks a column ("The walk") against one invocation a column, the baseline, at Goodsprings
with the default grid (64 slices, so 16 kept a chunk), the grid pass's span in two views (heading and
pitch 60:20 and 13:56), mean of two alternated runs of each build (the chunked build 6a85a673a against
the development branch before it, 49a98dca2, on the same machine), the stills equal to the same build's own
repeats within noise. The first machine's rows [hypothesis: 2026-11-24, at the hours named (12.35, 17.5,
18.6); which rows were on and the overcast weather's name were not kept]. The laptop's [observed, 2026-11-24:
`--play --cell Goodsprings --pos=-71680,2048,8372 --weather
NVWastelandClear --still-sky --no-actors --no-messages --no-voice --graphics-only
anisotropic,volumetric-clouds,cloud-shadows,volumetric-light --log-frame-times 5 --run-seconds 123` with
`NVF_PLAY_DEBUG_CLOUD_PHASE=100`, the first view held to 60 s and 30 s more, the second 30 s, the median of
the `nvf_medium_air` span from 45 s after the first view's turn and 12 s after the second's]:

| Machine | Sky, time | One walk | Four chunks |
|---|---|---|---|
| RTX 4080 SUPER (in a virtual machine), 1920x1080 | fair 12:21 | 0.36, 0.55 ms | 0.18, 0.28 ms |
| the same | fair 17:30 | 0.39, 0.62 ms | 0.20, 0.31 ms |
| the same | fair 18:36 | 0.36, 0.58 ms | 0.18, 0.28 ms |
| the same | overcast 12:21 | 0.43, 0.56 ms | 0.22, 0.28 ms |
| AMD Radeon 780M (a laptop's integrated GPU), 1280x800 | fair 12:21 | 1.00, 0.99 ms | 1.75, 1.40 ms |
| the same | fair 17:30 | 1.63, 1.00 ms | 1.85, 1.46 ms |
| the same, 1920x1080 | fair 12:21 | 1.02, 1.00 ms | 1.79, 1.39 ms |
| the same | fair 17:30 | 1.55, 0.95 ms | 1.94, 1.50 ms |

The composite is 0.09 to 0.17 ms on the first machine; on the 780M about 0.5 ms at 1280x800 and 1.05 ms
at 1920x1080. So the chunks halve the grid pass on the one and cost 13 to 79 % more on the other, and
only discrete NVIDIA cards take them. No discrete AMD or Intel card is measured, and no NVIDIA card but
this one.
Memory [design: from the formats], without the HDR picture, which the light keeps the main view in where
nothing else does: one volume of 160 x 88 x 64 `Rgba16Float` texels is 6.9 MiB,
13.8 MiB with the filter's second volume (the log's grid line states it); the columns follow the
aspect. Besides, one uniform buffer and two 1 x 1 stand-ins for the terrain map and the cloud image; its terrain map, made for it alone with the Terrain shadows row off, is the
heightfield's texture (2 MiB) and two targets of 4 MiB; while
the light volume is read, a 96-byte uniform and a 16-byte storage buffer more (its textures are the
clouds'). The light volume's reads and the mean pass are not measured on their own yet.

### Debug aids, tune keys and log lines

Developer aids, as the clouds' ("Debug aids" above): only in tests, in debug builds and in a build with the
`dev-aids` feature (in engine/: `cargo build --release -p nvforever --features dev-aids`). A release build without `dev-aids` reads
none of them, has no tune file follower and no reference line of the solid cores, and draws the light with the
defaults given here. Read once at the start, usable in a played run:
- `NVF_PLAY_DEBUG_MEDIUM_LIGHT=<strength>`: the switch (a finite number above 0) and the
  strength k (held to 0..2).
- `NVF_PLAY_DEBUG_MEDIUM_LIGHT_VIEW=visibility|slice|share|delta|off|lit|rays`: `visibility` the
  slice's own V as grey over the picture, `slice` the slice index banded (these two show nothing
  on the sky and beyond the grid's end), `share` F at the pixel, drawn everywhere, sky and beyond
  the grid included (all three from depth sample 0); `lit` the lit
  fraction Vbar as opaque grey `0.02 + 0.5 Vbar^2`, world and sky, not gated (the shafts with no
  look constants in the way); `delta` the applied scalar as mid grey plus 4 x it in display
  values; `rays` the applied scalar as grey `0.2 + 2 x` it; `off` the grid made while the light
  draws and the composite not run (with HDR still held: the baseline for paired pictures).
  `delta`, `rays` and `off` are gated as the light is.
- `NVF_PLAY_DEBUG_MEDIUM_LIGHT_GRID=<x>,<y>,<z>`, `..._DEPTH=<units>`,
  `..._OCCLUDERS=all|cascades|terrain|none|clouds`.
- `NVF_PLAY_DEBUG_SOLID_CORES=off|ref:<FormID hex>`: no solid cores, or one reference's logged
  ("Solid cores of blended shapes").
- `NVF_PLAY_DEBUG_MEDIUM_LIGHT_WALK=auto|serial|chunks`: how the grid walks a column ("The walk");
  `auto` (and anything else) by the adapter, `chunks` on any adapter their workgroup fits.
- `NVF_PLAY_DEBUG_MEDIUM_LIGHT_TUNE=<file>`: a `NAME=value` file looked at every 0.5 s of real
  time, each text parsed over the start values (a removed line returns its key to its start
  value), unknown names and unreadable values ignored, each value held to its range. The keys,
  with their ranges and defaults (medium_air.rs `AirTune::parse`, `AirTune::default`):

| Key | Range | Default | What |
|---|---|---|---|
| `STRENGTH` | 0..2 | the aid's value, else the slider's (1 at 100 %) | the overall scale k of the applied correction |
| `SHARE` | 0..1, `auto` | `auto` (s from the lights) | the sunlit share s; set, the deck's share is 1 |
| `G` | 0..0.95 | 0.5 | the forward lobe |
| `BACK`, `BACKWEIGHT` | -0.95..0.95, 0..1 | -0.3, 0 | the back lobe and its weight |
| `CAP` | 1..8 | 1.5 | the upper limit of the phase's largest value |
| `DEPTH` | 1,000..200,000, `auto` | `auto` (48,000, or the aid) | the grid's depth |
| `GRID` | `<x>,<y>,<z>`, `auto` | `auto` (160 x 88 x 64 at 16:9) | the grid's size |
| `SKY` | 0..1 | 0.25 | the far form's weight |
| `SKYPHASE` | 0..1 | 0 | the phase's share on the sky |
| `RAYS` | 0..8 | 3 | the ray contrast (1 physical) |
| `FLOOR` | 0..1 | 1 | the phase floor (0 energy-neutral) |
| `REF` | 0.05..1, `auto` | `auto` (the clouds' mean K) | the reference visibility |
| `LIFT` | 1..3 | 1.4 | the most a lit ray rises above the average |
| `SHADE` | 0..1 | 0.9 | the least shadowed air keeps |
| `SAMPLES` | 1..8 | 4 | visibility samples a froxel |
| `FILTER` | 0, 1 | 1 | the 3 x 3 filter pass |
| `VIEW`, `OCCLUDERS` | as the aids | none, all | |
| `MOON_STRENGTH` | 0..2 | 1 | by moonlight: the factor on the strength (the product held to 2) |
| `MOON_RAYS`, `MOON_LIFT`, `MOON_SHADE`, `MOON_G`, `MOON_CAP`, `MOON_SKY`, `MOON_SKYPHASE` | as the day's, `auto` | 8, 1.6, 0.4, 0.7, 3, 0.3, 0.15 | by moonlight, in place of the day's (`auto`: the day's) |

The defaults are the Natural look, set by stills [design, 2026-11-24]. The earlier values (G 0.6, CAP 2.5,
RAYS 6, LIFT 1.5, SKY 0.5, the same SHADE) drew the far plain and the pylons on the horizon of a hazy morning
as white cut-outs (the fog blend, "The composite" 6, takes those away at their cause) and lifted noon and
overcast air (the deck's share, "The medium", is for the overcast whatever the values) [hypothesis: stills of 2026-11-24 at Goodsprings, a
development build of that day; the steps were not kept]. The first values (RAYS 4, LIFT 1.75, SHADE 0.5,
SKY 1) took shadowed air darker than the open haze by up to 24 levels of 255 and washed out the sky beside a
low sun [hypothesis: about 50 runs of 2026-11-20 at Goodsprings with development builds of that day;
the views and the steps were not kept].

Log lines (prefix `volumetric medium: light in the air`): ` wanted (debug aid
NVF_PLAY_DEBUG_MEDIUM_LIGHT)` once at the start (never in a release build), ` wanted (the Volumetric light row)` and ` no
longer wanted by the row (the Volumetric light row)` as the row changes; ` on: grid <x>x<y>x<z> to <d> units, lit by the sun | lit by the moon, phase g <g>,
cap <c>, sunlit share | moonlit share <s>, strength <k>, near pixels at most +<g1> x the fog colour, far pixels
at most +<g2> x mix(fog colour, pixel, t), sky at most x<1 + gs> (display values; a snapshot for
this weather, hour and view), sky <w>, sky phase <q>, rays <r>, floor <f>, ref auto | ref <v>
(set), lift <l>, shade <h>` (by night the moon's values) when the gate opens or the source changes, again on a tune change and when a bound moved by
more than 0.05 (looked at every 5 s); ` off: <reason>` once a reason has
held 0.5 s of real time (at the start the fog shader is patched a moment after the first frames),
the reasons in the gate's order: `indoors`, `the camera is under water`, `no sun light`, `the
weather's fog cannot be read (...)`, `the fog does not blend in display values`, `night, and the
climate has no moon`, `the strength is 0`, `the sunlit share is 0`; ` tune: <changed keys>` (or ` tune: the file
cannot be read (...): the start values`); `: grid <x>x<y>x<z> to <d> units (<m> MiB in <n>
volume(s), filter 3x3|off), view <v>` when the volume is made; `: terrain map in use (<side> texels
of <texel> units)` or ` not in use (...)`; `: cloud shadow in use (plane <h> units up, strength
<s>, mean transmittance <m>, from the image)` or ` not in use (...)` (the image's state, K's fallback while the light volume
is read); `: cloud shadow source <the light volume (...) | the weather's layers (...) | off>` when
the air's source changes (the words of the surfaces' `cloud shadows: source`); `: grid drawn from the
clouds' light volume (its mean transmittance copied into the reference | no mean: ...)` once per
activation, after the mean's copy and the grid's dispatch; `: grid walk <one walk a column | four chunks of at most 16 | 32 slices a column> (<why>)` when
the walk is chosen and when it changes (why: the adapter, its limits or the debug aid);
`: grid pass drawn` and
`: composite drawn (view <v>, multisampled depth yes|no)` once each per activation; and the
warning ` not drawn: <reason>` once per reason (a projection or fog the shaders cannot take, a
grid over the adapter's 3D limit, a failed pipeline, missing bindings). Pipelines:
`medium_air_grid_pipeline`, `medium_air_filter_pipeline`, `medium_air_compose_pipeline`.

**Tests**: unit tests of the pure maths beside it (the slice curve and its figures, froxel
geometry round trips, the fog share against fog.rs and under synthetic extreme weathers, the
phase's sphere mean and cap, the sums and the lookup, the lit fraction, the relative form, the
floor and the bounds, the far and fog blends' continuity, the deck's share, the bright hold, the gate and its log, the source and the moonlight's values,
the share over the horizon, the tune file); CPU
mirrors of the shaders' loops (`column_mirror_at`, `lookup_mirror`, `pixel_mirror`, the filter's;
the grid's walks against a reference of the column written apart from them, for every slice count)
with numerical fixtures (an unobstructed sun fully lit between columns, MSAA samples with their
own fog, the extreme tune finite and in range); naga validation of the three shaders in every
variant (the composite with either depth type) and checks that each declares what its layout
binds and the uniform as the CPU side has it; the plumbing (off holds nothing, an inactive camera
then the light off holds nothing, a non-perspective camera unmarked, unsupported projections and
fogs refused, the format support cached apart from the grid); and the three narrow edits' tests. Smoke: `volumetric-light` (the row at Goodsprings at 17:30 under NVWastelandClear
with the Terrain shadows and Cloud shadows rows, 4x MSAA), `volumetric-light-taa` (single-sampled
depth), `volumetric-light-fsr`, `volumetric-light-night` (01:00: lit by the moon;
a climate without one is a unit test's), `volumetric-light-off` (the row off: no line of the light at all), `volumetric-light-no-aids` (a release build
ignores the aids), `volumetric-light-row` (the row alone, without the shadows' rows), `volumetric-light-switch` (off, on, off, on in play),
`volumetric-light-clouds` (with the clouds and their shadows), `volumetric-light-solid-cores`, and with
`NVF_SMOKE_DEV_AIDS` set (an exe built with the feature) `-aid` (the light by its aid, the row off), `-picture` (the picture with and
without it, measured), `-solid-cores-tank` and `-solid-cores-blend`.

## Wind-blown dust: the rules (#3032 stage 4)

[design] An addition of NVForever's own, an engineering choice for the enhanced look: not a fact
about the original, never a parity item, and never to be used to explain a difference from its
frames. **Only the rules are built** (`engine/nvforever/src/wind_dust.rs`, pure functions and a
small state with unit tests): nothing is drawn, no system runs them, there is no row, aid or log
line. The grid, the light on it and the row are later changes. Every number below is a start value
[design] until stills with the drawing judge it.

**What it follows: weather values alone.** No form IDs, editor IDs or file names decide anything, so a
mod's weather gets what its own values say. A number that is not finite counts as 0 (none), but a
wind direction that is not leaves the dust the velocity it had.

- **How much.** The weather's wind speed byte, blended through a weather fade as the trees' wind
  is, through straight lines: 0 up to byte 20, 1 at 50, 3 at 127, 3 above. Nearly every master
  weather stores 50, HVDustStorm 127, NVHooverFinalBattle 130 [observed: the records,
  `fnv_world::sky::Weather::wind_speed`]. So an ordinary day is 1 and strong wind 3: in a throwaway
  prototype's stills 1 was barely seen, 3 clearly dust and 8 heavy [observed: NVForever's own
  frames, 2026-11-25].
- **None while drops fall.** A weather with the Rainy flag, the class Rainy or Snow (DATA
  [documented: xEdit; GECK wiki GetWeatherTraitNumeric]) or a precipitation model has no dust. Through a
  fade the dry share is a product of linear ramps of the fade's progress, by weather_fx.rs's own rule on
  the fade bytes [hypothesis there]: the blend of the two weathers' shares; into a model, from 1 at
  the fade's start to 0 where its precipitation begins; out of one, from 0 where it stops to 1 at the
  fade's end. With a model on one side the product is a curve, not a straight line. A wet weather shown
  with no fade left (ForceWeather) has none at once.
- **Until the last drop.** None either while a precipitation model is placed or any drop of
  precipitation is alive: one flag and one count from the caller, no timers. **Required of the next
  slice:** tag in particles.rs every birth that comes from a weather's precipitation (the model's own
  systems and its add-on nodes, whichever model gave it, also after it is taken down) and expose how
  many tagged ones are alive after the simulation. A caller without the count passes 0 and no model, and
  gets the weather's own rule only.
- **No jump back.** The amount rises by at most 0.1 a second, whatever moved its target: the end of
  rain, its last drop, a forced weather, a turned-round fade, a hitch (for the amount a frame counts
  as 0.25 s at most). Ordinary dust is back ten seconds after the rain, a strong wind's after thirty. The wind's strength
  glides at the same rate both ways; the amount falls by at most 3 a second except where drops cut
  it to 0 at once. In an interior it is 0 and the rules go on with the weather outside, so a door
  is no way round the glide. Only a load or a new game (`WindDust::arrive`) shows its target in the
  first frame; a door, a cell change or a trip must not call it.
- **Where it goes.** The trees' direction (fixed, or a script's SetWindDirection [documented: GECK wiki
  SetWindDirection]) at 600 units a second for byte 255 (118 at 50); the velocity follows the wind's
  with a time constant of two seconds, and the place is its exact integral in f64 (the closed form, over
  the whole frame however long), so a turn, a cut or a stop changes the way from then on and never the
  place, and the frame rate changes neither for a wind held through the frame (one that changes within it, as in a weather fade, is
  taken once a frame). It wraps after 96,000 units, 48 cells of 2,000 and 160 of
  600. **Required of the next slice:** its shader must hash both octaves' lattices periodically (every
  48 and every 160 cells on each axis); only then is the wrap seamless.
- **Colour.** The fog colour now, tinted by the hue of the light on it (ambient plus sunlight
  over their luminance, 0 to 2 per channel), never a colour of ours. Under a black fog it is black.

**What it does not cover.** Honest Hearts' rain is drawn by a script's activators while IsRaining
(docs/spec-notes/world.md §6): the weather's Rainy flag cuts the dust there; those activators'
drops are not counted. A fade redirected away from a wet weather that has no model may lower the
amount within a second with no rain to see.
