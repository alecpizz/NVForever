# Enhanced graphics: engine-level improvements (workstream L)

Status: first set built 2026-10-12 (branch `claude/graphics-engine`, PR #236); follow-ups
(temporal anti-aliasing, sun shafts, screen-space water reflections; Bevy's volumetric fog tried
and dropped) the same day (branch `claude/graphics-2`). Owner's request (playtest
list 2, docs/handoff/archive/playtest-2026-10-12.md): "Add any modern engine level graphics rendering
functionality to improve the game visual without modifying the base assets or implementing new
ones."

Everything here is an **engineering choice**, not a fact about the original game: none of it
is tagged `[documented]`/`[observed]` about Fallout: New Vegas, and none of it may be used to
explain a difference against the game's own frames. The parity checks compare the `vanilla`
look only.

## 1. What the renderer does now (Bevy 0.19.1, `engine/nvforever`)

| Part | State before this work | Where |
|---|---|---|
| Main view | Not HDR: forward PBR with tonemapping (Tony McMapface) in the shaders into an 8-bit sRGB target; MSAA 4x (`--msaa`, the owner's game runs 8); depth prepass (water and the image-space blur read it) | `main.rs` `setup` |
| Alpha-tested edges | Smoothed with MSAA (soft edges standing in for transparency multisampling) | `nvf_assets::nif_material`, grass, trees |
| Sun | One directional light; shadows on by default in the game since 2026-10-26 (owner, once the first-person flicker was fixed: player-body.md; off 2026-10-13 to 10-26 for frame cost and that flicker; `--no-sun-shadows`, off without `--play`; `--sun-shadows` or the settings page): 3 shadow cascades out to `--shadow-distance` (8,000 since #254; §5.3), maps 2048² (Bevy's default), Gaussian PCF (Bevy's default); a second "shade" light for the first-person arms casts within 256 units of the camera (`ARMS_SHADE_REACH`) | `sky.rs`, `player_body.rs` |
| Local lights | Point/spot lights from LIGH records, flashlight with shadows | `lights.rs`, `flashlight.rs` |
| Sky | The game's sky meshes, colours mapped through the inverse of the tonemapper so the grade sees the weather's colours | `sky.rs`, `sky.wesl` |
| Fog | Bevy distance fog, linear, from the weather's near/far | `sky.rs` |
| Water | Planar reflection camera, refraction from the view transmission copy, depth-based shore fade | `water.rs`, `reflection.rs` |
| Image space | The game's grade (saturation, tint, brightness, contrast) and the modifiers' blur, double vision, radial blur, fade, as passes after tonemapping | `imagespace.rs` |
| Texture filtering | Trilinear, 8x anisotropic for the game's DDS textures; trilinear (no anisotropy) for the default sampler | `nvf_assets::texture` |
| Bloom, AO, eye adaptation | None (image-modifiers.md §5: the HDR and bloom parameters are not drawn) | |

Constraints that decide what fits:

- The custom prepass fragments (NIF, grass, tree, water) write **depth only**; Bevy's SSAO, SSR,
  TAA and motion blur need normal or motion-vector prepasses from every material.
- The view is **multisampled**, and the alpha-tested foliage depends on it. Bevy's SSAO and TAA
  require MSAA off.
- The owner's PC has a **GeForce GTX 1050 (4 GB)**, shared with other sessions' game runs.
- `vanilla` must stay the picture the parity lab and the smoke checks were made with.

## 2. Candidates

| Candidate | Value | Cost / risk | Decision |
|---|---|---|---|
| Ambient occlusion (own pass from the depth prepass) | High: grounds objects, adds depth to interiors | ~1-2 ms; first a post-process darkening direct light too, since #2897 stage b inside the lighting on the ambient light and unshadowed lamps | **Built** (§3.1) |
| Bevy SSAO (GTAO-like) | High | Needs MSAA off + normal prepass in every custom prepass shader | Rejected |
| Longer sun shadows (reach ×1.5; 4096² maps until #1367) | Medium: shadows further out | About 0.2-0.3 ms at Novac for the longer reach; the 4096² maps cost about 2 ms more and went | **Built** (§3.2) |
| Contact shadows (Bevy, screen-space ray march) | Low-medium | Tried: speckled self-shadowing on near surfaces and per-frame noise without TAA | Rejected (§4) |
| Bloom (own LDR pass, thresholded) | Medium: lamps, windows, the sun glow as in the game's own frames (world.md §6 notes the halos the renderer lacks) | ~0.5 ms at quarter size | **Built** (§3.3) |
| Bevy bloom | Medium | Needs an HDR main view: Rgba16Float targets (twice the traffic with MSAA 4x on the 1050) and every pass reading the picture runs on another format | **Built** 2026-10-23 as the light glow, on in vanilla (§3.11; the owner's PC now has an RTX 4070) |
| Sharpening (own contrast-adaptive pass) | Medium: the 2010 textures look crisper | ~0.2 ms; can add rims if overdone | **Built** (§3.4) |
| Bevy CAS | Medium | Tried: garbled the non-HDR view (yellow/white noise) | Rejected |
| Anisotropic filtering 16x | Low-medium: ground and roads at a slant | Nearly free on the GPU | **Built** (§3.5) |
| Sun in-scattering in the fog (Bevy `DistanceFog` directional light) | Low: NVWastelandClear's fog reaches 200,000 units, so nothing showed | A shadow-map fetch per fragment | Tried, rejected |
| Bevy TAA | Medium | Needs motion vectors from every material's prepass (the custom NIF, grass, tree and water prepasses write depth only; wind-animated grass and trees would need last frame's wind) and MSAA off | Not used: own pass instead |
| TAA from depth and camera alone (own pass) | Medium: smooth edges and plant cut-outs without multisampling, cheaper than MSAA 4x | Softer textures while the view moves; what moves by itself is not followed | **Built**, as the other choice to MSAA, not in `enhanced` (§3.6) |
| Bevy FXAA, Bevy SMAA | Low-medium: smooth edges without multisampling, cheaper than MSAA; on top of MSAA they add little | Blur fine texture detail (FXAA more); plants' cut-outs are not smoothed (no multisampling) | **Offered** in the anti-aliasing choice instead of MSAA, not in `enhanced` (§3.6, 2026-10-16) |
| Shadow filtering method (Bevy `ShadowFilteringMethod`) | Low: harder or softer sun-shadow edges | Temporal is grainy without TAA | **Offered** as a choice, Bevy's default (Gaussian) kept (§3.9) |
| Sun shafts (own screen-space pass), tied to the weather's fog | Medium: dusk and dawn, the sun behind roofs and poles | ~0.03 ms here | **Built** (§3.7) |
| Bevy volumetric fog | Would replace the game's fog look | Tried: darkens the whole picture on the non-HDR view, box edges | Rejected (§4) |
| Desert haze: exponential height fog in closed form, in the world shaders' fog path, on top of the weather's fog | Medium: far ridges fade in layers, basins haze over, warmer toward the sun | A few arithmetic operations per fragment where the fog runs; no pass | **Built**, off in every preset (§3.16) |
| Volumetric clouds: a bent domain ray-marched in the sky's band, its amount and colours the weather's own (Cloud shadows' composition), its placement and shapes ours | Medium-high: clouds with thickness, bright rims and dark bases with the sun low | The medium's trace, 1.25 ms at High in fair weather (RTX 4080 SUPER, 1080p, the trace alone; the set-up is in §3.23) and the levels below it (volumetric-medium.md); the light volume's sweep and the memory beside it | **Experimental**, off in every preset (§3.23) |
| Aerial perspective: the weather's fog per colour channel and toward the sky's colours in the ray's direction (Hoffman and Preetham), in the world shaders' fog path | Medium: successive ridges separate, bluer at noon, warmer at dusk, brighter toward the sun | A few arithmetic operations per fragment where the fog runs; no pass; the fog uniform grows by 48 bytes | **Built**, off in every preset (§3.18) |
| SSR on water | Low: the planar reflection is better where it reaches | Own march in the water shader from the depth prepass | **Built** for where the planar reflection does not reach (§3.8) |
| Bevy atmosphere | Would replace the game's sky look | | Rejected |
| Auto exposure, depth of field, motion blur | Change the look; the game's own HDR/eye-adapt values (IMGS DNAM) are not decoded | | Not now (§6) |
| PCSS soft shadows | Medium | Needs the `experimental_pbr_pcss` feature, noisy without TAA | Not now |
| Placed meshes blended into the landscape where they meet it (#2661) | Medium: road shoulders, rock feet and cliff bases lose their hard seam with the ground | Immutable front/back colour atlases (round 10b: about 48 MiB together at the default 512 texels per cell with their per-cell mip chains, against 144 MiB at 1024 without mips before; intermediates, the bake's single-level target while a replacement bakes and retiring handles aside); copies for unchanged cells and rasterization for entering/dirty cells; a lookup in eligible fragments; a height atlas per publication of 0.22 MiB with smoothing off and 3.4 MiB at the default 4 x 4 smoothing (#2149/#2661: it holds the drawn surface's squares), round 10a: about 0.45 MiB more of it (detail rows), 1.6 KB more index table per enabled clone, and near contact fragments read the landscape's own layer textures (up to 13 diffuse and 13 normal reads) | **Built**, off by default and not in `enhanced` (§3.14) |

## 3. What was built

`--graphics vanilla|enhanced` picks the look; each improvement is also a switch on the pause
menu's settings page (category "Graphics") and can be compared alone with
`--graphics-only <keys>`. The settings page's **Enhanced effects** row (Graphics > Picture, #1966) sets
them all as `enhanced` (On) or `vanilla` (Off) does and reads Custom when they are mixed;
the Quality row's presets never touch them (docs/spec-notes/quality-presets.md). The log says what is on: `graphics: ambient occlusion, longer
shadows, bloom, sharpening, 16x anisotropic filtering, sun shafts, screen-space water
reflections` (or `graphics: vanilla (no engine-level improvements)`), again whenever a switch
changes. Anti-aliasing (`anti-aliasing`: multisampling off/2x/4x/8x, FXAA, SMAA or TAA) and
shadow filtering (`shadow-filter`) are choices of their own, not part of `enhanced` (§3.6,
§3.9); the field of view, the display sync and a frame-rate limit are on the Display page (§3.10).
"Blend meshes into ground" (`mesh-terrain-blend`, §3.14) is a switch off in both looks and
left out of `enhanced` until the maintainer decides.
Code: `graphics.rs` (preset, switches, applying them), `ssao.rs`/`ssao.wesl`,
`post.rs`/`post.wesl`, `taa.rs`/`taa.wesl`, `shafts.rs`/`shafts.wesl`, `water.wesl` (`ssr`),
`sky.rs` (`shadow_reach`), `nvf_assets::texture::set_anisotropy`, `frame_pace.rs`, the rows in
`settings.rs`.

[design: `engine/nvf-play/src/graphics_resume.rs`, `settings.rs`, `pause.rs`;
`engine/nvforever/src/settings.rs`, `pipeline_gate.rs`] **Resuming after graphics edits
(pipeline portion of #1820).** Effective edits to the registered renderer rows latch for
the pause session, including Defaults. Continue, Esc and the timed resume aid use one
request. An unchanged session resumes immediately. A changed session keeps
`PlayState::Menu`, `Paused(true)` and the pause picture while rendering and pipeline
compilation continue. The wait counts all `PipelineGate::busy()` pipelines, including
sun shadows even with the travel-only shadow test aid enabled.

[design: the same files; `engine/nvf-play/src/loading.rs`] After the resume request and
after any further edit, fresh completed render frames must report no pending pipelines
and no new requests for at least 0.25 real seconds and three render frames. A stale zero
count cannot release the menu; a late request restarts settling even if it has already
finished. The wait ends at the loading screen's `PIPELINE_CAP_SECS` (15 real seconds)
if readiness never arrives. Repeated resume requests keep the first deadline. Loading a
save, quitting or closing the menu clears the session. Choosing anything else on the
column while the wait runs (Save, Load, Settings, Mods, Quit), or any page or the quit
question opening by another path, cancels the wait but keeps the latch: the release
would otherwise close what the player just opened, and the next Continue waits again
with a new deadline. The timed resume aid acts as Esc back to the column, then Continue. Completion is logged once as
`pause menu: resume after graphics change: pipelines waited <seconds> s, pending <n>, capped <true|false>`;
the existing `pause menu: resume` follows only when release is requested.

[design: `engine/nvforever/src/world.rs`, `loading_gate.rs`] Draw-distance streaming
stays outside this wait. Loading screens have cell/model readiness signals, but no
contract identifies which newly visible assets a distance edit must await. That part
of #1820 remains open; this change does not fully close the issue. Runtime compilation
timing and the two smoke setups remain unverified until run. State tests use synthetic
settings and controlled gate samples; smoke scenarios are `settings-resume-pipelines`
and `settings-resume-unchanged` in `tools/smoke/scenarios.toml`.

The ambient occlusion runs between the depth prepass and the main pass, which reads it inside the lighting (§3.1.2; its picture placement, `NVF_PLAY_DEBUG_AO=post`, comes after tonemapping, or before FSR or DLSS under them, §3.1).
Pass order after the main pass: tonemapping → (the ambient occlusion's picture placement) → temporal anti-aliasing, FXAA or
SMAA → sun shafts → heat haze (§3.15) → bloom and sharpening → the game's image-space blur and grade (imagespace.rs) → UI. So the game's grade and its modifiers
(fades, blur, double vision) work on top of the improvements exactly as on the vanilla picture,
and nothing repeats them.

### 3.1 Ambient occlusion (`ambient-occlusion`)

**Since #2897 (stage a) the estimate is ground-truth ambient occlusion** (§3.1.1), and **since
stage b it goes into the lighting** (§3.1.2): it scales the ambient light and the lamps that cast
no shadow, never the sun, the moon or highlights. The placement described below and in §3.1.1,
a factor multiplied into the picture, is kept as the fallback (`NVF_PLAY_DEBUG_AO=post`, and
what runs while the lighting hook is not in place). The older estimate described next stays in
the code as a fallback too (`NVF_PLAY_DEBUG_AO=old`, on the picture, §6). Both share everything
but the estimate and the denoise: the inputs, half resolution, the picture's two placements
(after tonemapping, or before a temporal upscaler), the radius, the screen-radius cap, the far
fade and the output.

The old estimate: from the depth prepass alone (multisampled or not: sample 0 is read): the view-space position
from the reverse-infinite depth (distance = near / depth), the normal from the neighbouring
depths on the side with the smaller step, 12 samples on a 3-turn spiral within 64 game units
(about 0.9 m; at most 80 pixels of 1080 on the screen, a share of the picture's height since 2026-11-24: it was 80 pixels at any size, half as far at 4K), turned per pixel by a 4 × 4 ordered pattern;
each sample occludes by its cosine to the normal above 0.12, fading out between half and the
whole radius. Strength 0.55. Since 2026-11-02 (#1370) this is worked out at half resolution,
for the top-left pixel of each 2 × 2 block, into a two-channel texture that keeps the pixel's
distance beside its openness (a quarter of the work; the pattern turns per half-resolution
texel). The result is blurred over the 4 × 4 texels of the pattern around each pixel, keeping
to texels within about 5 % of the pixel's own distance (so nothing bleeds across a depth edge),
and multiplied into the tonemapped picture. Those 4 × 4 texels span about 8 × 8 pixels of the
picture against 4 × 4 before, so contact shadows are a little softer. A pixel no nearby texel
shares a distance with (a pole or a grass blade a pixel wide) takes the texel nearest its
distance. Distances are held to 60,000 units (half floats end at 65,504; the sky dome is at
175,000 or more, far past the fade). It fades out between 4,000 and 7,000 units (where the radius is a pixel or two and the
fog takes over). The sky is left alone: the dome is opaque and in the depth prepass, but at
175,000 units or more, far beyond the fade (corrected 2026-10-12: not depth 0 as first written).

Known limits of the picture placement (stage b's lighting placement removes the first two,
§3.1.2): it darkens sunlit creases too (a post-process cannot tell ambient from direct
light); water is not in the depth prepass, so the lake bed's occlusion shows faintly through
the water surface; the first-person arms take a little occlusion from their own folds.

While FSR 3.1 upscales (since 2026-11-08) the same pass runs before FSR instead, at FSR's
render size, multiplied into the HDR picture FSR then accumulates
([fsr-upscaling](fsr-upscaling.md), "Ambient occlusion"); `NVF_AO_BEFORE_FSR=0` keeps it
after tonemapping.

#### 3.1.1 Ground-truth estimate (#2897 stage a)

An engineering choice for the enhanced look, like the rest of this page: the original draws
no ambient occlusion, and this is never a parity item nor an explanation of a difference
from the original's frames.

**Why.** The old estimate counts buried samples instead of integrating the visible part of
the hemisphere, so flat areas darken unevenly and depth edges get halos (#2897).

**Technique** [documented: Jimenez, Wu, Pesce, Jarabo, "Practical Real-Time Strategies for
Accurate Indirect Occlusion", SIGGRAPH 2016 course; written from the paper's maths, no code
copied from any implementation] (`ssao.wesl` `gtao_visibility`): per half-resolution texel
(the top-left pixel of its 2 × 2 block, as before), `SLICES` = 2 screen directions through
the pixel, φ = (k + turn) π / 2. In each slice (the plane through the view vector V and that
direction) the normal N (from the depth, `normal_at` as before: on each axis the
neighbouring texel with the smaller depth step, so an edge does not bend it) is projected:
its angle γ from V, atan2(N·t, N·V) with t the slice's tangent, and its length |n_p|. The
depth is marched `STEPS` = 4 times on each side, from one pixel out to the screen radius
(64 units, at most 80 pixels of 1080 as before), the steps closer together near the pixel (squared
spacing) where the contact shadows are. Each side keeps the highest horizon as its cosine
from V, starting at the edge of the normal's hemisphere (γ ± π/2); the horizons are
clamped to that hemisphere and the cosine-weighted visible arc between them is integrated
in closed form, ¼(−cos(2h − γ) + cos γ + 2h sin γ) for each horizon h.

- **Normalisation** [engineering choice, own maths]: the slices' sum Σ|n_p|(â₁ + â₂) is
  divided by the same slices' integral with nothing in the way, Σ|n_p|(cos γ + γ sin γ),
  not by the slice count. The paper's mean over slices is 1 on open ground only on average
  over the directions (one slice of a surface tilted by α from the view gives between
  cos α and cos α + α sin α), which two slices and a 4 × 4 pattern would show as a grain on
  open ground; the ratio is exactly 1 there with any slice count, never divides by zero
  (cos γ + γ sin γ ≥ 1 for |γ| < π/2) and tends to the same integral as the slices increase.
- **Thickness** [engineering choice]: no occluder counts beyond the radius; a sample's
  horizon is blended towards the hemisphere's edge as its distance from the pixel goes from
  half the radius (`FALLOFF_START`, a quarter of the squared radius) to the radius, the
  same falloff curve as the old estimate's. So a thin post far in front of a wall does not
  hide the wall's sky. A sample off the screen or on the sky raises nothing.
- **Bias**: a sample's cosine is lowered by 0.03 (`HORIZON_BIAS`) before it may raise a
  horizon, so a sample on the pixel's own plane (depth rounding, the normal from
  neighbouring depths) does not occlude it.
- **Pattern**: the slice turn from the 4 × 4 ordered pattern per half-resolution texel (as
  before), the steps' offset from the same cell's sixteen values in another order
  (i → 7i + 5 mod 16), so the denoise's 4 × 4 texels hold every turn and every offset
  once. Fixed from frame to frame: under TAA, FSR or DLSS a per-frame turn could add crawl
  [engineering choice]. On planes the denoise is expected to average all sixteen turns away;
  the occlusion-alone pictures showed flat walls even and white, but a grain over bushes and
  other alpha-tested plants, whose depth has no plane [observed, 2026-11-18, main session's
  runs, 1920 × 1080, MSAA], hence the denoise's fallback below (its effect on plants still to
  be confirmed in pictures).
- **Denoise** (`openness_at`): the same 4 × 4 texels around each full-size pixel, all of
  equal spatial weight (so the sixteen turns cancel), each weighted by how far its distance
  lies from the distance this pixel's plane has at that texel: under the reverse-infinite
  projection a plane's depth values (near / distance) are affine in the screen position, so
  the plane is predicted exactly from the depth gradient (on each axis the neighbouring
  drawn texel with the smaller step, as for the normal), a slope seen at a grazing angle
  included. Texels within 3 % of that distance plus one unit (`DENOISE_TOLERANCE`) count,
  falling linearly to nothing; a texel across a depth edge is left out, which is what keeps
  occlusion from leaking across edges (the halos). Where the plane-aware weights of the 16
  texels add up to little (`PLANE_WEIGHT_FEW` = 1, depth with no plane: plants, grass,
  leaves), the weights blend smoothly (over `PLANE_WEIGHT_BLEND` = 2 more) into the old blur's:
  the gap to the pixel's own distance, within 5 % plus one unit (`FALLBACK_TOLERANCE`), so
  foliage averages over most of the sixteen turns again while planes keep the sharp
  plane-aware result; no hard switch, which would draw an edge of its own. The same 16 texel
  reads serve both. A pixel no texel takes at all takes the texel nearest its own distance,
  as before. The old estimate keeps its own blur (within 5 % of the pixel's distance), so
  `NVF_PLAY_DEBUG_AO=old` draws the old picture to within rounding [observed, 2026-11-18, main
  session's runs: against a build of main with the switch on, Doc Mitchell's house at 1920 x 1080,
  2 % of the pixels differ by one level of 255 and 57 pixels by more, where two runs of main
  differ in none; the Goodsprings street differs as much as two runs of main do]; its cost is to be measured (both estimates
  sit in one pipeline behind a uniform branch).
- **Positions under an upscaler's expanded depth** (`NVF_AO_BEFORE_FSR=0` with FSR or DLSS):
  the centre and the horizon samples are taken at the centres of the drawn texels their
  pixels show, as the normal is, not at display-pixel centres with a texel's depth (which lie
  off the surface and occluded themselves); a step that lands in the pixel's own texel
  raises no horizon. Without upscaling the two are the same.
- **Strength** (`ssao.rs` `GTAO_STRENGTH` = 0.46 since 2026-11-25, 0.4 before, the picture
  placement's; the lighting's is 0.92, §3.1.2): factor = 1 − 0.46 (1 − visibility). It still multiplies all light, so it is kept
  low as the old 0.55 was. Open flat ground has
  visibility 1 and stays as bright as with the switch off (the old estimate did not
  guarantee that); a floor point at the foot of a wall has 0.5 (the wall hides half the
  cosine-weighted hemisphere) and becomes 0.8, about what the old estimate gave in such a
  crease [engineering choice from that reckoning; to be checked in pictures].
  `NVF_PLAY_DEBUG_AO_STRENGTH` (§6) replaces it for comparisons by eye: open ground is now
  exactly 1, so a higher strength darkens creases and contact without greying open areas.
- **Cost**: 2 slices × 4 steps a side = 16 depth reads per texel against the old 12, plus
  per-slice trigonometry; the denoise reads 4 more depths per full-size pixel for the
  gradient. Aimed at the old pass's cost (about 0.3 ms at half resolution at 1920 × 1080,
  #1370); `SLICES` and `STEPS` are constants (`ssao.wesl`, mirrored in `ssao.rs` for the log)
  to trade once measured. No extra full-size pass.
- **What does not change**: the switch (`ambient-occlusion`, no new row; #2897 asked for it
  "in `enhanced`": it stays in the `--graphics enhanced` preset as before; only the settings
  page's Enhanced effects row is gone since the settings menu overhaul), the two placements, MSAA, TAA, FSR and
  DLSS paths, the far fade 4,000–7,000 units, the 60,000-unit distance cap, the main camera
  only (the scope lens and Pip-Boy views carry no `AmbientOcclusion`).
- **Log**: `ambient occlusion: ground-truth estimate (GTAO), 2 slices x 4 steps a side at
  half resolution, depth-aware denoise` each time the occlusion turns on on the main camera;
  since stage b the line goes on with what runs (§3.1.2), on the picture `, on the picture
  (NVF_PLAY_DEBUG_AO=post)` (`old estimate (12-sample spiral, NVF_PLAY_DEBUG_AO)` with the
  old estimate; smoke scenarios `ambient-occlusion-gtao`, `ambient-occlusion-gtao-fsr`, both
  with `NVF_PLAY_DEBUG_AO=post` since stage b).

**Not done** (in this stage; stage b, §3.1.2, did the first):

- Applying the occlusion to the ambient term inside the lighting instead of to the finished
  picture (#2897 stage b): done in stage b (§3.1.2).
- The paper's multi-bounce fit: it needs the surface colour, which a pass on the finished
  (tonemapped, or HDR before the upscaler) picture does not have.
- A bent normal for the ambient lookup, a per-frame turn of the pattern accumulated by TAA or
  the upscalers (left fixed, see above), the paper's thickness heuristic of a slowly sinking
  horizon (the distance falloff stands in for it; stage b's `thin` remedy lets the horizon
  sink, §3.1.2).

#### 3.1.2 In the lighting (#2897 stage b)

An engineering choice for the enhanced look, like the rest of this page, and the switch's
default placement since stage b. Code: `ssao.rs` (the producer, the extraction, the debug
switch), `ssao_lighting.rs` / `ssao_lighting.wesl` (the uniform, the upsample, the lamps'
factor), `ssao_lighting/hook.rs` (the text hook in `pbr_functions`), the vendored mesh view
bindings (engine/vendor/README.md, "NVForever ambient occlusion").

**Why.** On the picture the factor darkens everything a crease receives, the sun included:
sunlit creases darkened, and the lake bed's occlusion showed through the water. Inside the
lighting it can be kept to the light that a crease really loses.

**Frame order.** `ao_lighting_pass` runs in `Core3d` after the depth prepass is complete and
before the main pass (where Bevy's own SSAO runs): the occlusion pass as before, then a
half-resolution `denoise` pass (`ssao.wesl` `denoise`) that writes, per texel q, the factor of
full pixel 2q through the apply pass's own filter (`openness_at`, the old estimate and the
remedies included) with that pixel's distance, into a second two-channel texture. The main
pass reads it. Under FSR or DLSS the passes run at the render size, the size the main pass
draws at (`upscaler/prep.rs`), whatever `NVF_AO_BEFORE_FSR` says. Whenever the passes cannot run
(a pipeline still compiling, the depth's sample count changing for a frame, no uniform) the
texture is cleared to "open", so the main pass never reads stale or undefined data. Texel q
stands for the depth prepass's pixel 2q, the same pixel coordinates as a fragment's position
(no viewport term).

**Bindings.** bevy_pbr's mesh view bind group gains bindings 43 (a uniform) and 44 (the
denoised texture) through a view component, `NvfAmbientOcclusionBindings`, built as the sun
visibility's (engine/vendor/README.md). The extraction puts it on the main camera's view while
the switch is on, the placement asked for is the lighting and the hook is in (`lights.rs`;
`ssao_lighting.rs` `HookState`), and takes it away otherwise; the view's pipelines then get the
shader def `NVF_AMBIENT_OCCLUSION`, and `prepare` points the binding at this frame's texture.
Switching the row rebuilds the world's material pipelines, which the pause menu's resume wait
covers [observed 2026-11-18, the main session's run: off and on again in play, `pipelines
waited 0.62 s, capped false`, no white or black frame in 14 screenshots].

**The hook** (`ssao_lighting/hook.rs`, applied after lights.rs's display-value patch and the
sun visibility's hook): every statement it adds is under `@if(NVF_AMBIENT_OCCLUSION)` and the
lines it replaces are kept word for word under the def's negation, so without the def the text
compiled is the one before (unit tests). Bevy's own ambient call and `diffuse_occlusion` are
left alone: the NIF and terrain shaders subtract exactly that ambient highlight with the
un-occluded value.

**The upsample** (`nvf_ambient_visibility`): the 2 × 2 half-resolution texels around the
fragment, each weighted bilinearly and by how near its distance is to the fragment's own (from
its world position, so the first-person arms' squeezed depth does not matter), within 5 % plus
one unit; 1 where no texel matches, beyond the fade, for a texel never written. Under
multisampling each sample's fragment reads with its own distance, so a plant's outline does
not take the ground's factor nor the ground the plant's, where the picture placement multiplied
the resolved pixel by one factor [observed 2026-11-18, the player's play test: no halos at plant
outlines; the main session's `view-lighting` pictures at the Goodsprings bushes: smooth, no rim
at plant outlines].

**Who reads it.** By bevy_pbr's rule, the pipelines that write depth (Bevy's own SSAO rule):
NIF objects opaque and alpha-tested, the terrain, grass, trees, skinned actors. The
first-person shapes (the arms, the weapon, the Pip-Boy: `FIRST_PERSON_DEPTH`) take a factor of
1 on the ambient and the lamps, so only the world is occluded [design, #3075: "exclude them
from the calculation so only the environment would be affected"]; their glow in the lamps'
colour with lamp shadows on was the shade light's relight (vendored bevy_pbr "no directional
shadows", engine/vendor/README.md). Opted in: the landscape's blended texture layers (they lie on the prepass's surface;
`nvf_assets` `NifSurfaceKey::terrain_layer`). Opted out: water and refracting NIF shapes (in no
prepass, the texels under them are the bed's or the wall's behind). Grass and leaf cards read
it unless their constants say otherwise (`grass.rs` `GRASS_READS_AMBIENT_OCCLUSION`, `tree.rs`
`LEAVES_READ_AMBIENT_OCCLUSION`, both on). Blended shapes, decals and every other view (the
reflection camera, the scope lens, the Pip-Boy) do not. A compile test composes the materials
that read it with the def through Bevy's shader cache (`nvf-shader-tests`).

**What it scales.** The display-value sum (lights.rs `DISPLAY_HELPERS`): the ambient term, and
the light of the point and spot lights that cast no shadow at the surface (where bevy_pbr leaves
their shadow unread: every placed lamp; the Pip-Boy light casts one). Never the directional
lights (the sun, the moon, an interior's directional light), never a highlight. #2897 asked for
the ambient term only; it was widened to the lamps because interiors are lit almost wholly by
them, and on the ambient alone the occlusion all but vanished indoors: at strength 0.8 and
exponent 1, Doc Mitchell's house changed 0.7 % of its pixels by more than 2 levels of 255
against stage one's 9.7 %, with the lamps as well 5.1 %; the Prospector Saloon 5.9 % against
7.4 % [observed 2026-11-18, the main session's pictures]. A lamp that casts no shadow lights a
crease as fully as open floor, so nothing else keeps its light out. The lamps' weight is
`ssao_lighting.rs` `LAMPS` = 1 (`NVF_PLAY_DEBUG_AO_LAMPS`, 0 for the ambient term alone).

**Exponent** (`ssao_lighting.rs` `EXPONENT` = 1): the factor scales the display-value sum, which
is shown as about itself, so the shown value falls by the factor, as the picture placement
multiplies the shown picture by it. The first value, 1/2.2, scaled the linear light, so the
shown value fell by only about f^0.45: the main reason the placement first looked weak. At 1
the Goodsprings street at noon changes as many pixels as stage one (20.3 % against 20.9 % by
more than 2 levels at strength 0.8, mean darkening 1.85 against 2.34 levels) [observed
2026-11-18, the main session's pictures].

**Strength** (`ssao.rs` `LIGHTING_STRENGTH` = 0.8): the reason for keeping the picture
placement's low (it darkened the sun's light too) is gone. Indoors 0.4 and 0.8 looked about the
same and neither too dark; outdoors 0.8 with the remedies below "feels about right" [observed
2026-11-18, the player's play test at exponent 1 and lamps 1] [engineering choice: the
player's]. 0.92 since 2026-11-25 [design, set by eye in Primm's casino once the display sum
showed its dark values by the sRGB curve (world.md §8 "Dark values of the sum"): the creases
at 0.8 read as too light; the picture placement's went up by the same 15%, 0.4 to 0.46].

**Remedies on plants** (`ssao.rs` `LIGHTING_VARIANT`, `thin+soft`; `NVF_PLAY_DEBUG_AO_VARIANT`).
The ground-truth estimate shimmers on swaying alpha-tested plants, more than the old one. The
candidates were measured on the occlusion alone, on the picture, at the Goodsprings street at
1920 × 1080 with 4x multisampling: 24 screenshots 0.12 s apart, the median over consecutive pairs
of the mean absolute change per pixel in levels of 255, for two bush crops and the bare road
[observed 2026-11-18, the main session's pictures]:

| Estimate / remedy | Bush 1 | Bush 2 | Road |
|---|---|---|---|
| Ground-truth, plain | 1.96 | 2.87 | 0.23 |
| Old estimate | 0.67 | 0.92 | 0.24 |
| `thin` | 1.26 | 1.70 | |
| `soft` | 1.43 | 1.83 | |
| `thin+soft` | 0.87 | 1.11 | |
| `thin+wide+soft` | 0.74 | 0.86 | |
| `slices4` | 1.82 | 2.48 | |
| `steps8` | 2.07 | 3.06 | |
| `nearest` | 1.46 | 2.15 | |
| `wide` | 1.54 | 2.07 | |

At strength 0.8 with no remedy the player still saw the shimmer; with `thin+soft` neither
shimmer nor halos [observed 2026-11-18, the player's play test]. `thin+soft` is the lighting's
default [engineering choice: the player's; `thin+wide+soft` measured a little steadier and was
not played]; the picture placement keeps none, and `NVF_PLAY_DEBUG_AO_VARIANT=none` gives the
plain estimate in either. The other remedies stay in the aid for comparisons.

**Debug and log.** `NVF_PLAY_DEBUG_AO=view-lighting` draws the factor each fragment's ambient
took in place of its colour, at the very end of the material's colour (after the fog); the
image-space grade and the light glow's eye adaptation still follow (`--no-grade`, light glow
off, for a plain picture). The start's line says what runs: `ambient occlusion: ground-truth
estimate (GTAO), 2 slices x 4 steps a side at half resolution, depth-aware denoise, variant
soft+thin, strength 0.92, in the lighting: on the ambient light and on lamps without a shadow by
1, exponent 1` (an aid's value is named after it); then `lights: ambient occlusion hook
installed`, `ambient occlusion: on the ambient light in the lighting (main camera)` when the
view takes the bindings and `ambient occlusion: off the lighting` when it gives them up; a
failed hook warns once and names the picture placement that runs instead. Smoke scenarios
`ambient-occlusion-lighting`, `-interior`, `-taa`, `-fsr` and `-switch`.

**Cost** [observed 2026-11-18, GPU spans in ms, the occlusion's passes / the main opaque pass; Sun shadows on, no actors;
4x MSAA unless FSR is named; the 2560x1440 figures are single 40 s runs on the final build, the others medians of five
4 s periods, two rounds, on the build before the lamp factor shared the ambient's read]: Goodsprings at 2560x1440: main's old
pass 0.24 / 0.86, the first stage 0.40 / 0.87, this placement 0.30 / 0.91 (0.25 / 0.87 with the lamp weight at 0); Doc
Mitchell's house at 2560x1440: 0.34 / 0.46, 0.45 / 0.40, 0.41 / 0.48; Goodsprings at 1920x1080: 0.13 / 0.66, 0.23 / 0.66,
0.18 / 0.70; Doc Mitchell's house at 1920x1080: 0.18 / 0.28, 0.29 / 0.28, 0.24 / 0.31; Goodsprings under FSR Quality to
2560x1440: 0.10 / 0.52, 0.20 / 0.52, 0.08 / 0.51-0.54. The main pass reads four texels per shaded fragment (the lamp factor
is formed from the ambient's read). Not measured: a slower GPU.

**Multi-bounce** (2026-11-18): the ambient's factor and the lamps' (those that cast no shadow
here) take the paper's multi-bounce fit per colour channel from the surface's diffuse colour
(ssao_lighting.wesl `nvf_ao_multibounce`, ssao_lighting.rs `MULTIBOUNCE`) [documented: Jimenez,
Wu, Pesce, Jarabo, "Practical Real-Time Strategies for Accurate Indirect Occlusion", SIGGRAPH
2016 course: the cubic fit of the visibility after several bounces against the one-bounce
visibility, per albedo]: never below the plain factor, exactly 1 on open surfaces, a light
surface's crease lifted more than a dark one's. Its effect on the game's dark textures is small
[observed 2026-11-18, enhanced at 1600 x 900, multi-bounce on against
`NVF_PLAY_DEBUG_AO_MULTIBOUNCE=0`: Doc Mitchell's house at 12:00, 0.03 % of the pixels lighter
by more than 2 levels of 255; the Goodsprings street at 12:30, 0.4 % lighter and 0.46 % darker,
the swaying plants' and actors' level of change; the house's factor is below 0.9 on 6.2 % of
the pixels and below 0.7 on 2 %, its edges and corners]. Kept on: it only lifts.

**Shimmer** [observed 2026-11-18, the Goodsprings street at 12:30, standing still, enhanced,
four pictures 0.05 s apart, pixels changing by more than 8 levels between consecutive pictures]:
the factor alone (`NVF_PLAY_DEBUG_AO=view-lighting`) 1.65 % with 4x MSAA and 1.70 % with DLSS
Quality (the camera jittering), the lit picture 1.33 % and 0.84 %; by more than 24 levels
0.00-0.04 %. The same with and without the jitter: the change is the plants' sway, not the
occlusion's. Not measured yet: the view moving; the Freeside alley and the gas station
exterior pictures #2897 asks for.

**Off, Performance, Quality** (2026-11-19, [design, #3075]: AO's row to match Sun shadows' and
Lamp shadows'). The row `ambient-occlusion` is a list: Off; Quality, the estimate above
unchanged (2 slices x 4 steps a side, half resolution, the depth-aware denoise, `thin+soft`,
strength 0.8, multi-bounce, first-person shapes excluded: the uniform is the same value for
value); Performance, the same pass with one slice direction a pixel (`ssao.rs`
`PERFORMANCE_SLICES`, through the uniform's slice count, the one the aid's `slices3` uses; the
aid's own count still wins). The `enhanced` preset is Quality; `--ambient-occlusion[=performance|
quality]` turns it on at a level whatever the look; an INI written while the row was a switch
(`1`/`0`, true/false, on/off, yes/no) loads as Quality or Off, and the launcher shows that value
as the list's (`catalog::list_value`). The start's line ends with the level (`, level
Performance`), and is said again when the level changes.

Reads a half-resolution texel in the lighting placement, without multisampling (as with
DLSS) [engineering count from `ssao.wesl`, not a measurement]:

| | Quality | Performance |
|---|---|---|
| `occlusion`: the pixel's depth, the normal's 4 neighbours | 5 | 5 |
| `occlusion`: the horizon march, slices x steps x 2 sides (scattered, up to 80 px of 1080 away) | 16 | 8 |
| `denoise`: the pixel's depth, the gradient's 4 neighbours | 5 | 5 |
| `denoise`: the 4 x 4 occlusion texels (`Rg16Float`) | 16 | 16 |
| all | 42 | 34 |

The march is the costly part (its reads miss the cache, and each slice adds two `acos`, an
`atan2` and the arc integral), so Performance takes about half the occlusion pass and none of
the denoise, whose 4 x 4 texels the one slice's sixteen turns need to cancel (a 2 x 2 denoise
would show the pattern; the turns are fixed per pixel, not jittered, so DLSS would not average
them). Steps stay at 4: the contact shadows come from the near steps. Not measured yet: the
cost and the pictures (the main session's in-game run).

**The depth resolved once under multisampling** (2026-11-19; frame-rate-audit §9 "What is
left": 0.3 of the pass's 0.6 ms at 1440p 4x was reading the multisampled depth). With a
multisampled prepass, `ssao.rs` `resolve_depth` first writes sample 0 (the sample every pass
read; with the aid's `nearest`, the nearest sample) into a single-sampled `Depth32Float`
texture at the prepass's size (`ssao.wesl` `resolve_depth`, a depth-writing fullscreen pass),
and the occlusion, denoise and picture passes read that with their single-sampled pipelines:
one coherent read a pixel instead of 26 reads a half-resolution texel from the multisampled
texture. The values are the same bit for bit, so the picture is too. Inside the
`nvf_ambient_occlusion` GPU span; `NVF_AO_RESOLVE_DEPTH=0` reads the multisampled depth as
before, for the comparison. The start says `ambient occlusion: the multisampled depth resolved
once to one sample for the passes`. Without multisampling (DLSS, FSR, TAA) nothing changes.
Not done: replacing Bevy's 0.23 ms copy of the multisampled depth (frame-rate-audit §9) with
this resolve. That copy is in `bevy_core_pipeline` (`prepass/node.rs`, not vendored), and the
copy it makes is read per sample by the water's edge test (`water.wesl`, every sample against
the fragment) and bound as a multisampled texture in every material's view bindings (bevy_pbr
`mesh_view_bindings`), so it cannot become a single sample without changing those readers.

**Shadowed lamps, highlights and a moving pattern** (2026-11-24, play tests in Doc
Mitchell's house with the ALC shadows row on). Lamps whose shadow is looked up (the
flashlight, the Pip-Boy light, the ALC shadows row's lamps) took none of the factor, so with
that row on a room's occlusion was left on the ambient light alone (0.7% of pixels changed
by the occlusion in Doc Mitchell's house against 2.5% with the lamps casting none). They now
take half of the lamps' factor (`ssao_lighting.rs` `SHADOWED_LAMPS`; a shadow map's texels
are several units wide and its edges filtered wider, so the creases the occlusion finds are
below what it resolves; at 1 a crease the shadow already darkens is darkened twice): 1.8% of
that room's pixels change by 4 levels or more against none before, 4.6% at 1. The factor
reaches the highlights of point and spot lamps (`ssao_lighting.wesl`
`nvf_specular_visibility`, the specular occlusion of Lagarde and de Rousiers from the
ambient's factor, the view's angle and the roughness, `HIGHLIGHTS` 1; in a world view Bevy's
`direct_light` holds only the lamps' highlights, and the hook takes back what its line added
without the factor): a bottle, a metal shelf or a wet floor no longer glints in a crease as
in the open. Not the sun's or the moon's, nor the ambient highlight. Under a temporal method
(TAA, FSR, DLSS) the 4 x 4 sample pattern moves a cell each frame (`ssao.wesl`
`frame_shift`): the denoise's 4 x 4 texels still hold every turn once, so a plane's factor is
unchanged (0.4% of pixels differ in a still), and the grain left on leaves and outlines
becomes noise the temporal method averages. Test aids: `NVF_PLAY_DEBUG_AO_SHADOWED`,
`NVF_PLAY_DEBUG_AO_HIGHLIGHTS`, `NVF_PLAY_DEBUG_AO_STILL`. An Ultra level (the pass at full
resolution, 0.41 ms against 0.13 at 4K with DLSS Quality) was built and dropped after a play
test: "basically not noticeable even at 4k native".

**Not done:** a bent normal (the ambient is a flat colour with no direction), occlusion on
decals and blended shapes (not in the prepass), and on the ambient highlight (no environment
reflections exist yet for a specular occlusion to act on, §3 gaps).

### 3.2 Longer sun shadows (`fine-shadows`)

**Since 2026-11-03 (#1367) the maps stay 2048²**; the switch keeps only the longer reach,
and the settings page calls it "Longer sun shadows" (the INI key stays `fine-shadows`).
The owner chose the frame rate: "we're going for the smaller one i want as much frames we can
get". [observed 2026-11-03, main 650911ee, i9-14900K, RTX 4070, 1920 × 1080,
`--graphics enhanced`, `--play` standing still, `NVFOREVER_HOME` without stored settings;
one run per place cycling the three sets every 10 s (`--graphics-cycle`), the first loop left
out, three or four periods each]

| Place | 4096², reach 12,000 (before) | 2048², reach 12,000 (now) | 2048², reach 8,000 (switch off) |
|---|---|---|---|
| Novac 07:00 | 11.30 ms | 9.02 ms | 8.70 ms |
| Novac 12:00 | 10.88 ms | 8.75 ms | 8.52 ms |
| Goodsprings 12:00 | 7.60 ms | 6.94 ms | 6.77 ms |
| Strip 22:00 (moon shadows, always 2048²) | 6.14 ms | 6.10 ms | 6.11 ms |

The shadow passes' GPU time at Novac 12:00 went from 1.82 to 0.87 ms with the smaller maps;
the rest of the difference was the render thread waiting for the GPU. Screenshots from the
free camera close to the ground (Goodsprings 12:00, Novac 07:00, two headings each, pitch
−35°) show the same shadows with slightly softer edges at 2048².

Before #1367: `DirectionalLightShadowMap` 4096² instead of 2048², and the last cascade at 1.5 ×
`--shadow-distance` (12,000 units by default since #254), the first cascade bound unchanged (a twelfth of
`--shadow-distance`): the nearest shadows get twice the texels per unit, the far cascades about
1.3 times, and shadows reach half as far again. Before #254 the arms' shade light copied the
cascades, so it also got 4096² maps: about 512 MB of shadow maps in all (vanilla: 128 MB).
Since #254 it has one cascade of its own (256 units); the total has not been measured again.

**By night and at the horizon** (#1369, owner 2026-11-02: cheaper moon shadows;
`graphics::sun_shadow_quality`): below the horizon the sun's light is the moon's (`sky.rs`),
and it casts one cascade out to 3,000 units (`MOON_SHADOW_DISTANCE`) in 2048² maps whatever
this switch says; while the light is under a tenth of its strength in its fade at the
horizon (`SHADOW_FADED`), no shadow maps are drawn. [observed 2026-11-02, i9-14900K, RTX 4070,
1920 × 1080, 8x MSAA, enhanced] With the day's 3 cascades at 4096² (before #1367) the moon's shadows cost
about 2.5 ms on the Strip at 22:00 (9.2 ms against 6.7 ms without sun shadows).
In interiors the sun is the cell's directional light (XCLL) and the clock stands still, so
it always casts the day's shadows there. The map size (`DirectionalLightShadowMap`) is one
setting for every directional light, so the first-person arms' shade light (`player_body.rs`
`shade_light`, its own 256-unit cascade) has 2048² maps too.
The phase is applied again when a new sun is spawned (a worldspace switch) and when
something else puts cascades on the sun (`sky::set_shadows`, the settings page).

### 3.3 Bloom (`bloom`)

On the tonemapped picture (linear values 0..1): the brightest channel over 0.82 with a soft knee
of 0.12 is kept at a quarter of the size (four bilinear taps over 4 × 4 pixels), blurred across
and down with a 9-tap Gaussian two texels a step (about 34 quarter-size texels wide, ~136 pixels
of the full picture), and added back at 0.9. This is how bloom was done in the original's era,
on display values; it does not touch the HDR parameters of the game's image spaces, which are
still not decoded (image-modifiers.md §5).

### 3.4 Sharpening (`sharpen`)

Contrast-adaptive against the four neighbours: the centre is pushed away from their sum by
0.5 × ¼, less where the neighbourhood already has contrast (none above 0.6), and held within
85 %-115 % of the neighbourhood's range so no bright or dark rims appear.

### 3.5 Anisotropic filtering 16x (`anisotropic`)

The game's textures (`nvf_assets::texture::sampler`) get 16x instead of 8x, and the default
sampler (generated LOD and other engine-made images) 16x instead of none. The switch applies to
textures loaded after it changes (cells loaded next); the start's textures follow the command
line.

### 3.6 Anti-aliasing (`anti-aliasing`; not part of `enhanced`)

One choice on the settings page (since 2026-10-16; before, temporal anti-aliasing was a switch
`taa` of its own and the multisampling only `--msaa`): **Off, 2x, 4x, 8x** (multisampling, the
start being `--msaa`, default 4x), **FXAA**, **SMAA** or **TAA**. The last three draw the view
without multisampling and smooth the picture after tonemapping, before the sun shafts, bloom and
the game's image space (the same place as TAA, so the grade is not smoothed). Choosing one of them
keeps the multisampling level for when a level is chosen again. Stored as a part in the INI like
any slider (`anti-aliasing=0.33` is 4x; the seven choices are the steps 0, 1/6, … 1). Unattended
runs keep `--msaa`, so `--graphics vanilla` draws as before.

- **FXAA**: Bevy's (`bevy::anti_alias::fxaa::Fxaa`) at its default thresholds (High, High)
  [engineering choice, not tuned by eye]. Cheapest; softens fine texture detail and does not
  smooth the plants' cut-outs (tested per pixel without multisampling).
- **SMAA**: Bevy's (`bevy::anti_alias::smaa::Smaa`) at its default preset (High) [engineering
  choice]. Sharper than FXAA, a little dearer; the same limit on the cut-outs.
- **TAA**: ours, as follows.

What the multisampling levels cost (#1379; the settings page's note gives the first two rows'
figures in its own words) [observed 2026-11-02: plain release build, i9-14900K, RTX 4070
(driver 616.56, Vulkan), 1920 × 1080, `--graphics enhanced`, vsync off (`--log-frame-times
10`), `--play` standing still, `NVFOREVER_HOME` without stored settings, `--msaa N`, 45 s runs]:

| Place | Off | 2x | 4x | 8x |
|---|---|---|---|---|
| Novac 12:00 | 10.36 ms | 11.19 | 11.63 | 12.76 |
| Goodsprings 12:00 | 7.15 ms | 7.92 | 8.45 | 9.37 |
| Strip 22:00 (the CPU sets the frame) | 6.26 ms | 6.23 | 6.30 | 6.31 |

So 2x adds about 0.8 ms, 4x about 1.3 ms and 8x about 2.3 ms where the GPU sets the frame.
The steps and the 4x default stay (#1379, a note only): the note tells a player turning
everything up what 8x costs and names FXAA, SMAA and TAA as the cheaper choices.

#### Temporal anti-aliasing

The other choice to multisampling, not on top of it: while it is on the main view is drawn
without multisampling (`Msaa::Off`); switching it off puts back `--msaa`. The alpha-tested
cut-outs (plants, fences, hair, grass) are then tested per pixel and smoothed by the jitter
over the frames instead of by alpha to coverage.

- The projection is shifted every frame by a sub-pixel amount (Halton 2,3, eight steps;
  Bevy's `TemporalJitter`), textures sampled at a mip bias of -0.5.
- After tonemapping and the ambient occlusion's picture placement (§3.1), before the sun shafts, bloom, sharpening and the
  game's image space (so the grade, fades and blurs are not smeared over the frames): each
  pixel's point is carried from this frame's camera to the last frame's (the depth and both
  cameras' matrices; the reprojection is worked out on the CPU in double precision, the world
  being hundreds of thousands of units across), taking the nearest depth of the 3 x 3
  neighbourhood so edges keep to the object in front. The history is sampled with a
  Catmull-Rom filter and clipped to the colours around the pixel this frame (variance clipping
  in YCoCg, 1.25 sigma, within the neighbourhood's min/max), then blended: 10 % of this frame
  standing still, up to 50 % when the picture moves 16 pixels a frame or more, weighted by
  inverse brightness. The history is a pair of Rgba16Float textures; it starts over on the
  first frame, after a resize, when the camera jumps more than 400 units in a frame (doors,
  MoveTo) and whenever the pass did not run the frame before.
- Bevy's own TAA needs motion vectors from every material's prepass; ours write depth only,
  so what moves by itself (actors, plants in the wind, water) is looked for where it was not;
  the clipping keeps that from smearing, at the cost of less smoothing on it while it moves.
- The first-person arms and weapon move with the camera: points nearer than 48 units are taken
  as not moving on the screen, and the neighbourhood's nearest depth is taken only among
  pixels on the same side of that distance (otherwise the world next to the hand took the
  hand's motion and left a dark rim while turning [observed 2026-10-12, fixed]). A wall nearer
  than 48 units counts as camera-locked too (it then keeps less smoothing while turning).
- Switching the multisampling while the game runs: in the frame it changes, the view's `Msaa`
  and its depth prepass texture can disagree; the passes that bind the depth (ambient
  occlusion, sun shafts, TAA, the image-space blur) now go by the texture's own sample count
  and skip that frame (before, a validation error ended the run).
- Switching TAA off while the game runs (another anti-aliasing choice, or a temporal upscaler
  taking over, which turns native TAA off) [design, #2955]: the camera keeps its render-world
  view, and Bevy's sync takes only the TAA marker off it, so the pass's own state on that view
  (its pipeline, history pair and uniform) is dropped in the same frame and the history with
  it, and the pass runs only on a view that carries the marker this frame (log: `taa: off for
  a view: its pass state and history dropped`). Before, the pass went on blending the history
  frozen at the switch into every frame (a faint still picture over the view under FSR or DLSS
  until a load). Turned on again, the history starts over on its first pass that runs
  (`taa: on for a view: its history starts over` is logged when the view's state is made,
  which can be a frame or more before, while the pipeline compiles).
- Looks [observed 2026-10-12, Goodsprings noon, standing, and walking while turning 40 degrees a
  second]: edges as smooth as MSAA 4x standing still, plants' cut-outs smoother; textures a
  little softer standing still and clearly softer while the view moves (the usual TAA trade);
  the arms stay sharp. **Not in `enhanced`** for that softness and because the owner plays with
  8x multisampling; the settings page offers it (`anti-aliasing` TAA).
- Cost: see §5.1; the pass itself ~0.05 ms here, and the main pass and the prepass are cheaper
  without multisampling, so TAA costs less than MSAA 4x.

### 3.7 Sun shafts (`light-shafts`)

Screen-space light scattering, at a quarter of the size, on the tonemapped picture after the
temporal anti-aliasing and before bloom: the sky around the sun (where the depth prepass has only
the sky dome, beyond 157,500 units; with multisampling, in every sample of the pixel, so a
silhouette's edge such as the gun's against the sun is not taken as sky) within 0.3 of the
picture's height of the sun, its brighter part (linear luminance 0.04 to 0.4), is blurred along the lines towards the sun (24 taps over
1/24 of the length, then 24 taps over 85 % of the way to the sun, each weighted 0.97 of the one
before: about 576 taps in all) and added to the picture, kept below white without a hard clip.

Tied to the original's look: the light is the game's own sky and sun as drawn (their colours,
the clouds in front of the sun), and how strongly it scatters follows the weather's fog:
strength `0.6 + 0.9 x haze`, `haze = min(1, 20,000 / fog far) x fog strength` (NVWastelandClear,
fog far 200,000: haze 0.1, strength 0.69; a weather with its fog far at 20,000 or less: 1.5).
None indoors, none once the sun is down (the sun disc's own fade, -2 to 3 degrees of elevation),
fading out as the sun leaves the screen (from the edge to 1.6 x the half-width beyond it).

Looks [observed 2026-10-12, Goodsprings 19:00 facing the sunset]: light streams past the
mountains and over the town, the sun's glow is wider; at noon looking away from the sun nothing
changes. Every constant is a choice by eye on that one view; the first try (strength 1.5, source
radius 0.45) washed the whole view out.

### 3.8 Screen-space water reflections (`water-reflections`)

The planar reflection (reflection.rs) shows one water plane at a time, the nearest below the
camera; water at another height in view reflects only the sky gradient. There, with this switch,
the water shader (water.wesl `ssr`) marches the reflected ray through the depth prepass (28 steps
from 24 units, each 1.16 times the one before, about 9,000 units in all, then four halvings) and
takes the opaque picture (the transmission copy) where the ray passes behind something by less
than a step; trust fades near the picture's edges and towards the end of the march. Mixed in
after fog and tonemapping, in the reflected share's place (an approximation: the copy is already
fogged and tonemapped).

Where it applies: exterior water at a second height in view, e.g. Lake Las Vegas (water 3,000)
next to Lake Mead's cells (2,600), Camp Forlorn Hope (5,360), the Devil's Throat (2,290)
[observed: fnvtool world cells, the cells' water heights in WastelandNV]. Interior cells draw no
WATR planes in this engine (the Goodsprings Cave pool is a model with its own material), so the
switch does nothing indoors. Checked only by forcing it in place of the planar reflection at Lake
Las Vegas (`--pos=28500,59000,3500 --heading 200 --pitch -12`, a temporary shader change): the
far shore's cliffs show in the water as with the planar reflection, without artefacts; that water
reflects weakly, so the difference is small. It runs only on the water pixels it applies to, so
its cost was not measured.

### 3.8.1 Mirrored water (`water-mirror`; on in both presets)

The planar reflection itself (reflection.rs): a second camera draws the world mirrored in the
nearest water plane while that water is on screen. It is on in vanilla too, since the original's
water reflects the world; `--graphics-only no-water-mirror` or the settings switch turns it off
(#1380), and water then reflects the sky gradient, and the picture where `water-reflections` is
on. Cost at Hoover Dam by day, Lake Mead in view, where the CPU sets the frame: 7.6 ms a frame
with it, 7.1 without [observed 2026-11-02, RTX 4070, 1920x1080, `enhanced`]. The camera keeps
its own set of the sun's cascades (Bevy's renderer needs them for every view the sun lights);
emptying their caster lists was tried and saved 0.12 ms of shadow-pass GPU time and nothing of
the frame, so the reflected world keeps its shadows.

### 3.8.2 Enhanced water effects (`water-effects`)

[design] A nonexplosive missile crossing the surface can also disturb the reflected and
refracted image. Each crossing starts a short pair of expanding crests at its timed
entry point. The water material keeps up to eight recent crossings. The weapon's impact
set still supplies its splash model and sound. Explosive projectiles make a heavier
entry. A qualifying near-surface blast keeps the authored water column and drives a
broader surface wave scaled by its radius and depth. A light crest and dark trough
help the leading wave remain visible as the plume falls and the bright splash
clears. The existing blast rules resolve damage, force and sound.
Smoke scenarios: `water-shot-rings` and `water-blast-waves`.

[design, #2988] Shoreline wash uses the undistorted bed depth to
soften wind waves and vary sparse patches of wash over time. The wash uses world
coordinates for breakup and fades on steep receivers and toward distant water.
Its shallow-depth gate leaves deep water and placed pools out of this effect. The vanilla preset keeps
the switch off.

[design, #2674] Graphics > Water offers this switch on its own. The enhanced look enables
it. From above the surface, the water shader reconstructs the position of the opaque
object visible through refraction. Sunlight projected through the surface draws a moving
network of bright focused lines there, attenuated by the water body's opacity. The surface
is modelled as eight travelling sine waves (wavelengths 41 to 239 game units, crests
moving 33 to 81 units/s). The light at a receiver is the inverse of the Jacobian
determinant of the refraction displacement, evaluated analytically. Shallow water shows
a fine sharp web and deeper water a broader, softer one. The bright lines add up to
about 1.1 times the receiver's colour, and the gaps between them lose at most 0.15, so
the bed's mean brightness barely changes. Waves narrower than a few pixels on the
receiver fade out in both views. A submerged
outdoor camera uses its depth prepass to project the same pattern on receivers in nearby
loaded cells with water at the same level. First-person geometry and the sky are excluded.
The pattern fades with water depth, the sun's elevation and the weather's sunlight.
The submerged pass attenuates the added light by the camera's remaining visibility.
Local shadowing is approximate. Projection through a water surface and attenuation with depth
are described in [GPU Gems, chapter 2, section 2.2](https://developer.nvidia.com/gpugems/gpugems/part-i-natural-effects/chapter-2-rendering-water-caustics).
The wave set, the focusing model's tuning and the band limit are NVForever's.

[design] From shore, the water scatters more light as the bed drops through its WATR
above-fog range. The shallow end, deep colour and placed-water opacity cap remain
authored by the water type. Underwater, the existing distance fog thickens as the camera
descends. At one above-fog depth range below the surface its visibility distance is
halved. The density increase is capped at 2.5 times the surface density. The effect
works at night and uses the water type's own colour in both views.

[design] The shallows: by the WATR ramp alone a wading depth of NVCleanWater is under a
fifth opaque, and at Boulder Beach the near water could not be told from the dry shore.
With the switch on, the bed seen from above takes the water's shallow hue and darkens to
0.62 of its brightness over the first 90 units of ray in the water, and the body gathers
along the same ramp twice as fast at its thin end (the far end is unchanged). `vanilla`
keeps the ramp as it is.

[design] Wave normals come from three height taps round each point, one texel apart
near the camera and a pixel's footprint apart beyond, which limits shimmer as the waves
travel (in every look). A span of two texels each side was tried first: it removed
4-texel waves and kept 0.64 of 8-texel ones, flattening the surface, and took twelve
taps a pixel for nine. The water mirror is given the main camera's cascades every frame
(`shadow_rate.rs`): the renderer numbers each view's shadow-map layers from zero, so a
held layer holds the main camera's map, and the mirror reading it with matrices of its
own made whole patches of reflected riverbank flash at Cottonwood Cove. With the same
matrices in both views the staggered schedule keeps holding near water. The mirror also takes the main camera's caster lists after each view's own cuts (`sky::sun_casters_over_slice`, `player_body::near_shade_casters`): with the matrices alone the two views drew different casters into the same layers. Drawing every
cascade of both views every frame was the first fix and doubled the cascade passes near
water for every player. Reflected shadows reach as far as the main camera's cascades;
past that the reflection is sunlit. The submerged pass is ordered after ambient
occlusion and before temporal anti-aliasing: each of the three swaps the view's two
pictures, and unordered they could record on two threads, one reading a stale picture
for a frame [hypothesis for the flicker reported under water, from the code].

[design] Under a temporal upscaler (FSR, DLSS) the submerged pass runs before it instead,
as the ambient occlusion's picture placement does (§3.1): at the render size, from the
depth prepass as the main pass drew it, and multiplied into the HDR picture the upscaler
reads (and, under FSR, into FSR's opaque copy, so its reactive masks compare the two as
before). The upscaler then accumulates the pattern with the bed it lights. After the
upscaler the pattern would be added unsmoothed every frame, from a depth drawn jittered
at the render size and expanded to the display size by nearest texels, and the pass would
cost the display's pixels. The band limit stays the displayed pixel's, as the textures'
mip bias keeps their detail at the display's. The gain then brightens the HDR light before
tonemapping, so on a bright bed it shows a little less than when it multiplied the
tonemapped picture [engineering choice; compared by eye with the old placement under
FSR/DLSS at Boulder Beach in daylight, #3089: looks fine]. `NVF_CAUSTICS_BEFORE_UPSCALER=0`
keeps the pass after tonemapping under an upscaler too (§6). The log says the placement
once a view takes it ("underwater caustics: on the HDR picture at the render size, before
the upscaler").

[design] River currents animate the noise layers on selected exterior reaches: the
Colorado below Hoover Dam through Cottonwood Cove, and the western channel in Zion
Canyon. No record names a flow, so the downstream directions and the speed (110 units a
second, each noise layer at 1.0, 0.75 and 1.3 of it so the surface changes shape as it
travels; at 40 the river moved slower than the wind moves Lake Mead's first layer, 65)
are this project's choices.

[observed, 2026-11-18: a private script of the change's author over their own
FalloutNV.esm and HonestHearts.esm, read-only; its output is not in the repository] The
table (`river_flow.rs`) selects cells at the worldspaces' default water heights (WRLD
DNAM: WastelandNV -2300, NVDLC02ZionCanyon 5000). Which cells: for each exterior cell
with the "has water" flag, the script read the LAND heights (VHGT) along the row a
quarter of the way up the cell and listed the stretches lying more than 12 units under
the cell's water height. On the Colorado they run between cells x 17 and 24 from row y 6
down to -31 (north of that the water is Lake Mead's, at 2600); in Zion the western stream
runs from cell (1, 8) to (-3, -9). The table's rectangles cover the cells those
stretches cross.

[design] A locally placed surface at the same elevation can share the cell's current. Each noise layer retains a share of its
water type's wind, bounded so its crests travel downstream. Actor ripples keep their
world position. The current changes surface appearance only.

[observed, 2026-11-18, the first pattern (two noise octaves) and a current of 40 units a
second; not repeated since] Fixed-camera captures show the light pattern on the Goodsprings
Source trough floor and the Boulder Beach bed above and below water at noon. At midnight
the submerged pass stayed off. At Cottonwood Cove the surface texture travelled south
in all 40 successive frame pairs measured on a patch projected back onto the water plane.

[observed, 2026-11-18, the first shadow fix (every cascade drawn every frame) and the
first wave normals; not repeated with the shared cascades] Two fixed 1920×1080 play views at Cottonwood Cove were
sampled 32 times each, 80 ms apart. In the view facing the opposite bank, the
largest frame-to-frame brightness change in the water after spatial smoothing
(Gaussian σ = 16 pixels) fell from 24.3 to 2.6 on a 0–255 scale. The buildings
at the cove also showed less fine shimmer. Forcing the original build to redraw
its shadow maps each frame stabilized the bank while retaining sun shadows.

[observed, 2026-11-18, the first shadow fix, since replaced] Four 1920×1080 release runs at the opposite-bank view on an
RTX 5070 Ti used Quality sun shadows, mirrored water and Water effects. The original
and revised binaries ran in A/B/B/A order. Median GPU time reported for the shadow
pass across warm four-second windows was 0.08–0.09 ms on the original and
0.22–0.26 ms on the revision. Whole-frame times varied enough that the two pairs
gave opposite signs for the difference.

[observed, 2026-11-18, the mirror sharing the main camera's cascades] Release explorer,
RTX 5070 Ti, 1920x1080, `--graphics enhanced --sun-shadows`, Cottonwood Cove looking
across the river, four 32-second runs in A/B/B/A order with `--log-frame-times 5`; B set
`NVF_PLAY_TEST_SHADOW_STAGGER=0`, which draws every cascade of both views every frame as
the first fix did. Warm five-second windows: 5.93 to 6.28 ms a frame with the schedule
holding (A), 6.78 to 7.05 ms without (B; two of B's first windows read 5.18 and 5.41);
the shadow pass's GPU time 0.10 to 0.20 ms against 0.20 to 0.30 ms. At Boulder Beach
(`--play`, 09:00, twelve frames 0.2 s apart, 1280x720) the largest frame-to-frame
brightness change after the same smoothing was the same with the schedule holding and
without (5 to 7 of 255 once the start's fades were over): no flash of the reflected shore.

[observed, 2026-11-18, the mirror sharing the main camera's cascades and caster lists]
Cottonwood Cove, the player in the river facing the opposite bank (`--play --pos=69300,-90750,-1700`,
09:00, 1920x1080, `--graphics enhanced`), eleven frames between 10 s and 12.3 s of two runs,
one with the schedule holding and one with `NVF_PLAY_TEST_SHADOW_STAGGER=0`: frame for
frame the two runs differ by at most 4 of 255 in any pixel. A held shadow map shows the
reflected bank what a fresh one does.

[observed, 2026-11-18, the pattern of eight waves and the shallows' tint] Release explorer,
RTX 5070 Ti, 1920x1080, `--sun-shadows --log-frame-times 5 --graphics-cycle
"vanilla;water-effects;water-effects;vanilla"`, 86 seconds a view, the first window left
out. Mean frame time with the switch on against off, over eight five-second windows
each: 2.90 against 2.86 ms at the Boulder Beach shallows, 2.50 against 2.47 ms submerged
there, 2.95 against 2.94 ms at Cottonwood Cove. The windows of either state range over
0.2 ms, so the cost is within the run's noise, under 0.05 ms.

[observed, 2026-11-18, initial water effect before the depth and pattern revision] Release explorer, RTX 5070 Ti, Vulkan, 1280x720, desktop streaming
active. At each fixed camera `--log-frame-times 5 --graphics-cycle
"vanilla;water-effects;water-effects;vanilla"` alternated five-second periods. After the
first block, the median paired changes in mean frame time over four complete blocks were
0.00 ms at the Boulder Beach shallows, +0.02 ms submerged and +0.02 ms at Cottonwood
Cove (the four river differences ranged from -0.02 to +0.11 ms). The submerged pass
itself measured 0.02 ms GPU. At Cottonwood the transmissive pass measured 0.03 ms off
and 0.04 ms on.

### 3.9 Shadow filtering (`shadow-filter`; not part of `enhanced`)

How the sun's shadow maps are sampled at a shadow's edge, Bevy's `ShadowFilteringMethod` on the
main camera: **Fast** (`Hardware2x2`: one hardware-filtered tap, harder and more stair-stepped
edges), **Smooth** (`Gaussian`, Bevy's default and what vanilla has always drawn: the camera then
carries no component at all) or **Temporal** (a rotated 8-tap pattern that changes every frame:
soft but grainy unless TAA is the anti-aliasing, which it is meant for). Matters only with sun
shadows on. The names are engineering choices; Bevy's PCSS is not offered (it needs the
`experimental_pbr_pcss` feature).

### 3.10 Field of view, vertical sync, frame-rate limit (Display page)

Settings rows that are not improvements and do not change the vanilla picture unless chosen:

- **Field of view** (`field-of-view`): the game's FOV setting (the horizontal angle at 4:3, as
  `--fov`, default 75) from 50 to 110 in steps of 5, shown in degrees. It sets the player
  camera's projection at once; aiming down sights zooms from it (aim_zoom.rs takes a changed
  angle as its new base).
- **Viewmodel field of view** (`viewmodel-field-of-view`, under Field of view): the angle the
  first-person arms and weapon are drawn with (as `--viewmodel-fov`, degrees as `--fov` counts
  them, default 55, the INI's `fDefault1stPersonFOV` [documented: `Fallout_default.ini`]) from
  30 to 90 in steps of 5. `player_body` reads it each frame for the arms' magnification
  (`arms_magnification`), so it takes effect at once; the world's view is left alone, the
  Pip-Boy keeps its own 47, and the console's `fov` with a first-person angle still wins
  [design]. Scripts read both rows' angles, `settings::script_fov` telling them to the game
  state before the scripts run (`CameraRequests::settings_fov`): GetCameraFOV and
  GetPlayerCamFOV answer a SetCameraFOV (console `fov`) value first, then these, then the
  INI's; moving Field of view drops a console world angle, which the camera no longer shows
  [design].
- **Display sync** (`display-sync`, a dropdown; #1082): Vertical sync, on as Bevy's default
  (`PresentMode::AutoVsync`); Off is `AutoNoVsync`; Variable refresh (G-Sync, FreeSync) is
  vsync with the frame rate capped a few frames under the monitor's refresh rate (winit's
  current video mode; 144 Hz → 138). Kept as `display-sync=off|vsync|variable`; an INI with
  only the old `vsync=1`/`0` switch still sets it. Not on the page while `--log-frame-times`
  measures (which turns vsync off). What vendors document about VRR in borderless and
  exclusive full screen: spec-notes/display-sync.md.
- **Frame rate limit** (`frame-cap`): Off, 30, 60, 90, 120 or 144 frames a second
  (`frame_pace.rs`): each frame waits until 1/limit s has passed since the last (sleep, then
  spin the last millisecond), right after it is handed to the render thread and before the
  next frame's input is read, so the wait is not between the input and the picture (until
  2026-11-10 it waited at the end of the main world's frame, after the input; measured in
  [frame-generation](frame-generation.md), "Measurements"). A late frame is made up for a
  hundredth of a frame's time at a time. Bevy has no limiter of its own (`bevy_framepace` is a
  separate crate); this plain one is enough to keep a fast GPU from drawing hundreds of
  frames a second in menus [engineering choice].

### 3.11 Light glow (`glow`; on in vanilla too)

The original as the owner plays it (`bDoHighDynamicRange=1`) glows: neon, bulbs, lit
signs, fire and beams bleed soft halos into the night (the fx audit frames of 2026-10-23,
`C:\fnv-private\references\fx_audit\game`). The owner asked for that look, and for a
natural one rather than a copy, so this is on by default in `vanilla` and in `enhanced`;
`--graphics-only no-glow` or the settings switch "Light glow" turns it off, and the
enhanced "Bloom" (§3.3), when on, stands in for it.

- The main camera gets an HDR view (`bevy::camera::Hdr`: values over 1 survive until the
  tonemapper, which then runs as its own pass) and Bevy's bloom with its NATURAL preset
  (energy-conserving, no threshold: every pixel scatters a share of its light, so only
  what is far brighter than its surroundings shows a halo) (`post.rs` `glow_bloom`).
- Its strength is 0.15 at the image space's Bright Scale 2 (NVDefaultExterior) and
  follows Bright Scale / 2 (held to half and twice) as the place's image space and the
  weather's adapters change (`post.rs` `glow_intensity`, world.md §6 "HDR and bloom").
- What emits light is brighter than white only in this view: unlit shapes are drawn at
  their emissive colour × multiplier (nif.md §6 "Emissive multiplier of unlit shapes"),
  so Gomorrah's neon at multiplier 25 burns; lit shapes' emissive light already is.
- Every pass after the tonemapper (the ambient occlusion's picture placement, anti-aliasing, sun shafts, Bloom and
  sharpening, the image-space grade) sees the same tonemapped values as before; the
  passes key their pipelines on the view's format.

Engineering choices made by eye against the original's frames; not used to explain
differences in the parity frames.

### 3.12 Terrain height blending (`terrain-height-blend`; off in both presets)

[design, #2550] An addition of NVForever's own, not a fact about the original game. The
landscape's layer opacities live on the 33 × 33 vertex grid, 128 units apart, so where a
layer is painted at full strength on one vertex and not at all on the next the change is
a straight 128-unit ramp that follows the triangle grid. With this switch the folded
terrain shader (terrain.wesl, `TERRAIN_HEIGHT_BLEND`) shapes that ramp by the textures:
the incoming texture's brighter texels (stones, clumps, cracked tops) come through first
and the gaps between them fill last. Prior art for this engine family: the New Vegas
Reloaded terrain shader's `HeightBlend` and Skyrim's Community Shaders; cited only as
prior art, nothing of either was used.

- **Height source.** Height is `clamp(gain * (luma(raw_texel) - luma(reference)),
  -0.25, 0.25)`, Rec. 709 luma on linear, sRGB-decoded colour before material or vertex
  tint. Diffuse alpha has no part in the blend (#3789). `global`
  (default) reads the existing coarsest mip at UV (0.5, 0.5), a near texture mean for a
  full mip chain. A bright texture does not win just for being bright; broad shading in
  the texture still contributes. A short mip chain gives an imperfect reference.
  `local` reads a coarser mip at the texel's own UV: `LOD = min(last_mip,
  max(4, estimated_current_lod + 2))`. Current LOD is estimated from UV derivatives scaled
  by each texture's dimensions, including the view's mip bias. Derivatives are captured
  before divergent control flow or discard; reference samples use explicit levels in
  both binding forms. This local reference removes some broader spatial shading too.
  A base without a diffuse texture has height zero. The base diffuse is read again for
  raw colour, at the standard material's coordinate and bias.
- **Weights.** For painted opacity `a[k]`, ordered shares are
  `w[k+1] = a[k] * product(1 - a[j], j > k)` and `w[0] = product(1 - a[j])`.
  They sum to one. Compute `score[i] = w[i] + clamp(height[i], -w[i], w[i])`,
  `highest = max(score)`, and `q[i] = w[i] * max(0, depth - (highest - score[i]))`;
  final weights are `q[i] / sum(q)`. Scores outside the depth window drop out.
  The share cap preserves continuity when a layer appears or disappears. A zero share
  stays zero and a sole contributor stays one. Equal heights now sharpen unequal shares;
  there is no equal-height gate or exponential boost. Equal shares and equal heights
  remain equal. Increasing depth converges towards the ordered blend. With shares
  (0.5, 0.5), heights (-0.05, +0.05), depth 0.10 gives weights (0, 1).
  Colour and normals use the same weights; the resulting normal is made unit length once.
  terrain_weights.wesl is plain WGSL, with naga validation and a CPU mirror in terrain.rs.
- **Default parameters** [design, #2550]: depth 0.10, gain 8, global reference, the user's choice from in-game comparisons at a painted gravel/cracked-mud transition (2026-11-15, the matrix below); height
  limit remains 0.25. Test aid `NVF_PLAY_TEST_HEIGHT_BLEND=depth:gain[:source]` is read
  once per process and logged at start. Source is `global` (also when omitted) or `local`.
  Depth must be finite and at least 0.001 (also the shader's floor); gain finite and non-negative.
  Invalid input warns and falls back to the complete default `0.10:8:global`.
  This configures the effect; enable it with the graphics switch. Only the on variant
  reads the appended uniform `(depth, gain, source_id, reserved)`, source IDs 0/1.
  The material pipeline key remains its on/off bit.
- **Picture comparison matrix** [design, #2550], compared at a road-side transition near Goodsprings, 1920x1080, 13:30. Keep camera,
  lighting, textures and graphics settings fixed; compare roads/clumps, flat textures,
  oblique distant ground, quadrant/cell edges and the fold limit. Restart between values.

  | Comparison | Switch | `NVF_PLAY_TEST_HEIGHT_BLEND` |
  |---|---|---|
  | Ordered baseline | off | `0.10:4:global` |
  | First candidate | on | `0.10:4:global` |
  | Wider transition | on | `0.20:4:global` |
  | Narrower transition | on | `0.05:4:global` |
  | More height contrast (chosen default) | on | `0.10:8:global` (or unset) |
  | Spatial reference | on | `0.10:4:local` |

  Off, the gravel fades into the cracked mud as a broad gradient; on, the mud breaks into
  islands along its cracks with gravel between. Depth 0.05 starts to show straight edges
  along the triangles; gain 0 (no height, sharpening only) gives a hard band without shape;
  the local source looked like the global one there, so global (no extra read) is kept.
  Retextures with higher-resolution or higher-contrast diffuses may want another gain;
  neither source is claimed to suit every terrain texture.
- **What it reaches.** Only the folded draw (terrain.rs "One draw for the base and its
  first layers"): up to six layers where the repeat divides the cell, four otherwise. A
  quadrant's later layers that keep their blended draws, and every layer on a GPU without
  bindless textures, blend as before: their coverage over the folded result is unchanged,
  but height does not compete across that draw boundary, so moving a layer across the
  fold limit can change the outline there.
- **Quadrant and cell edges.** The two sides of an edge give the same result only where
  they have the same contributing textures, the same effective (ordered) shares and the
  same fold partition. Stitching (#2096, terrain.rs `stitch_layers`) makes the painted
  opacities agree along an edge, but each quadrant keeps its own base and layer order, so
  equal opacities can still give different ordered shares, and a layer folded on one side
  can be a separate draw on the other. Whether that shows is checked by pictures (the
  edge-stitch location of `terrain-edge-stitch`, quadrants at the fold limit), not
  claimed here.
- **Switching.** The settings row Terrain height blending (Graphics > Picture), or
  `--graphics-only terrain-height-blend`. It is a pipeline key of the folded material
  (`TerrainLayersKey`); switching flips every folded material in place
  (`terrain::sync_height_blend`, logged as `terrain: height blending on|off (<n> folded
  quadrant materials, <m> changed; …)`), and the first switch compiles the new pipeline
  (the pause menu waits for it). With the switch off the shader's path is the one there
  always was, operation for operation.
- **Cost.** Global uses one coarsest-mip reference sample per contributing texture,
  generally a small cached working set. Local uses one explicit-level spatial sample per
  contributor at the same UV; it has a larger working set and derivative/LOD arithmetic.
  Both also reread the base's raw diffuse, usually cached. Cost needs GPU measurements at
  layer-heavy Goodsprings roads and the Primm junction, off/global/local. Zero-share
  layers still have the ordinary diffuse/normal reads; with Terrain anti-tiling on as
  well (§3.13), zero-share reads are skipped and heights use its raw samples with a
  global reference.
- Whether `enhanced` turns it on is the owner's call; until then it is off in both presets
  and is not an effect of the Enhanced effects row. Smoke: `terrain-height-blend` (switched
  on, off and on while running), `terrain-height-blend-start` (global from the start),
  and `terrain-height-blend-local-start` (local from the start); all
  want the variant's `terrain height blending pipeline <n> ready` line
  (`NVF_PLAY_TEST_PIPELINE_LOG`, pipeline_gate.rs). The look itself is checked by pictures.

### 3.13 Terrain anti-tiling (`terrain-anti-tiling`; off in both presets)

[design, #2551] An addition of NVForever's own, not a fact about the original game, which
repeats every landscape texture plainly every 256 world units, 16 times across a cell
(docs/spec-notes/world.md §3 "Texture repeat" [observed]). From a hill or over the
Mojave flats the same marks recur on that grid. With this switch the terrain shaders read
each landscape texture at a few offset places and blend them, so the repeat no longer
lines up into a grid. NVForever keeps Hex3 [design, #2551], its own small version of
Morten S. Mikkelsen's published technique, ["Practical Real-Time Hex-Tiling"](https://jcgt.org/published/0011/03/05/)
(JCGT 11(3), 2022) [documented: cited publication]. It uses no histogram transform.
Heitz & Neyret's histogram-preserving by-example noise (HPG 2018), Quilez's "Texture
repetition", Skyrim Community Shaders' "Terrain Variation" and New Vegas Reloaded's
LOD noise are prior art only; no code was taken from any of them.

- **Lattice and hash.** The texture coordinate (u = game x / repeat, v = game y / repeat,
  as the meshes carry it) is laid on a lattice of equilateral triangles four repeats on a
  side (1,024 units at the game's repeat). Each lattice vertex is named by two signed whole
  numbers (floor, never truncation) and hashed (a 32-bit integer mixer, no `sin`) into an
  offset of the texture and one of six 60° rotations. The coordinate hangs on
  the world, so neighbouring quadrants and cells read the same place the same way: no seam
  at quadrant or cell edges from the reads. Each read's coordinate is taken relative to its
  corner, so it stays small far from the origin; the tests compare it with an f64
  reference near ±200,000 units (f32 positions there are 1/64 unit apart, about 0.03 texel
  of a 512-texel texture over 256 units). No promise is made past that range.
- **Hex3.** All three corners as hex cells, offsets and rotations, the barycentric
  weights to the fourth power made to sum to 1. Continuous (a corner's weight is 0 on
  the edge across from it). The curve narrows the blend; it is not a histogram transform
  and does not promise kept contrast. Hex3 is the sole method; the Triangle2 generator
  and temporary comparison selector have been removed [design, #2551].
- **Picture decision.** At noon on the flats of cell (-24,-9), 1280x720, anti-tiling off
  shows the 256-unit repeat as regular stripes receding into the distance. Hex3 removes
  them; Triangle2 leaves faint stripes [observed: main-session picture comparison,
  2026-11-15]. This is why NVForever keeps Hex3 [design, #2551].
- **Mip selection.** Every read takes explicit gradients: the plain coordinate's change
  across a pixel (taken before any branch or discard), turned with the read's rotation and
  scaled by 2^mip bias, since a gradient read takes no bias of its own (the upscalers set
  it). Offsets and lattice choices are not differentiated, so a hash border causes no mip
  jump.
- **Normal maps** take the same reads: each texel is decoded (two-component maps rebuild
  z), its green flipped as the material says, then turned back by the read's rotation; the
  blend is made unit length and put through the surface's frame. The decoded result does
  not go through Bevy's `apply_normal_mapping` again. Folded layers take the base
  material's decoding flags, as the ordinary path does (an image's format is not known
  when the material is made: textures decode on worker threads).
- **Faint reads.** After Hex3's weights, each weight less 1/256 (held at 0) is
  made to sum to 1 again, so a read fades to nothing before it is skipped (no jump at the
  cut); at least one read always stays.
- **Skipping.** The folded layers are walked last to first with the ordered blend's own
  products (`terrain_ordered_shares`): a layer of opacity 0 is not read, and a fully
  covering later layer leaves everything under it, the base too, unread. Normal maps are read only for textures whose
  final weight is above 0; height references only for textures whose ordered share is
  above 0. Exact zeros, no epsilon: faint painted layers count.
- **Height blending together.** With §3.12 on as well, the heights come from the very
  anti-tiled raw decoded diffuse reads before tint (no second read of the base), via
  `terrain_height(raw, reference, gain)`. The reference is always the coarsest mip at
  (0.5, 0.5), independent of tile offsets, including when `source=local`: a local reference
  would need the same offset taps. `terrain_height_weights(w, h, depth)` applies §3.12's
  depth competition (score = share + clamp(height, -share, share), retain within depth of
  the highest, multiply by share and normalise), using depth and gain from `height_params()`.
  Shared diffuse/normal taps and exact zero-share skips are kept [design, #2551].
- **The surface set-up.** Instead of Bevy's `pbr_input_from_standard_material` (which
  would read the base texture and normal map once more at the plain place, and take the
  SSAO multibounce from that plain colour), nvf_assets terrain_pbr.wesl starts from
  `pbr_input_from_vertex_output`, copies the landscape material's values and multiplies
  vertex colour and material colour in once; after the colour is final it applies Bevy's
  SSAO multibounce and specular occlusion formulas to it. Its contract is the landscape
  material's (colour, diffuse texture, normal map, scalar values; terrain.rs
  `surface_material`, checked by a test); a wider material needs this changed too. With
  the switch off, both earlier paths (height blending off and on) are compiled as before:
  their `@if` conditions only gain `!TERRAIN_ANTI_TILING`.
- **What it reaches.** The folded draw (base and folded layers), and the landscape's own
  NIF materials: a quadrant's base without folded layers, the layers past the fold limit
  drawn blended over the folded result, and every layer where the GPU has no bindless
  textures (nif_material.wesl with `TERRAIN_ANTI_TILING` and `TERRAIN_SEPARATE`; the
  material carries its repeat in `NifSurface::terrain_repeat`). Their blending, draw order
  and depth bias are unchanged; a blended layer whose coverage is exactly 0 reads nothing.
  The same world lattice lines a separate layer up with the folded draw under it, but
  height blending still does not compete across that boundary (§3.12), and a separately
  lit layer is still lit on its own. Not anti-tiled: the depth prepass and shadows (the
  same depth either way), the normal prepass's normals (screen-space passes that read them
  keep the plain reads), and the distant LOD terrain's baked textures.
- **Read budget.** Per read texture, up to three diffuse reads instead of one, the
  same for each contributing normal map, plus a little arithmetic; skipping brings the
  common case (a base and one or two layers) near the ordinary count. With D diffuse-read
  textures, N contributing normal maps and M contributing height references: at most `3*D + 3*N + M`
  reads before faint reads drop [design, #2551].
- **Measured GPU frame cost.** At 2560x1440, unpaced (`--log-frame-times 8`,
  `--graphics-cycle`), two passes each, one game on the machine, RTX 4070. These
  measurements predate the final depth-based height blend; its combined cost needs
  remeasurement [design, #2551]:

  | View | Off | Hex3 added | Triangle2 added | Height blending (#2550) added | Hex3 with height blending added |
  |---|---|---|---|---|---|
  | Goodsprings hillside, several layers (-63488, 6144) | 4.37-4.40 ms | 0.12-0.13 ms | 1.2 ms | 0.28-0.31 ms | 0.23 ms |
  | Flats | 2.01-2.03 ms | 0.11 ms | - | 0.00 ms | - |

  Deltas are against off [observed: main-session GPU frame measurements, 2026-11-15].
  In that earlier revision, the combined hillside cost was below height blending alone. The anti-tiled path skips
  zero-share layers and does not read the base twice [design, #2551;
  terrain.wesl anti-tiled branch, terrain_pbr.wesl surface set-up]. These measurements
  support keeping Hex3 and removing Triangle2 [design, #2551]; they describe these views
  and this GPU, not a general performance guarantee.
- **Switching.** The settings row Terrain anti-tiling (Graphics > Picture), or
  `--graphics-only terrain-anti-tiling`. A pipeline key of the folded material
  (`TerrainLayersKey`); switching flips every folded material in place
  (`terrain::sync_anti_tiling`, logged as `terrain: anti-tiling on|off (<n> folded quadrant
  materials, <m> changed) and <s> separate landscape materials (<c> changed)`), new
  materials take the setting, and the first switch compiles
  the new pipeline (the pause menu waits for it). A texture repeat that is not above 0
  keeps the ordinary reads (a warning when it is switched on).
- **Chosen values** (NVForever's own [design, #2551]): lattice side 4
  repeats, faint-read cut 1/256, Hex3's fourth-power curve, salts `0x25510001..3`.
- Whether `enhanced` turns it on is the owner's call; until then it is off in both presets
  and is not an effect of the Enhanced effects row. Smoke: `terrain-anti-tiling` (switched
  on, off and on while running), `terrain-anti-tiling-start` (on from the start) and
  `terrain-anti-tiling-height-blend` (one folded pipeline with both switches),
  `terrain-anti-tiling-separate` (every layer drawn on its own,
  `NVF_PLAY_TEST_TERRAIN_LAYERS=blended`, switched on and off) and
  `terrain-anti-tiling-overflow` (`--terrain-tile 300`: four folded layers and the rest
  drawn on their own; it wants `terrain: <n> layers drawn blended over folded quadrants`,
  logged by the first cell that has such layers, and the separate pipeline with
  `blend=on`); they want
  `terrain anti-tiling pipeline <n> ready: path=folded|separate height=on|off
  blend=on|off`
  (`NVF_PLAY_TEST_PIPELINE_LOG`, pipeline_gate.rs). Picture and frame-cost evidence is
  recorded above; future visual checks should cover both landscape draw paths.

### 3.14 Blend meshes into ground (`mesh-terrain-blend`; off in both looks, not part of `enhanced`)

Where a placed road, rock or cliff meets the landscape, the mesh's lower edge takes the
landscape's own colour and shading over a short height band, so a broken asphalt shoulder
reads as ground-lit dirt instead of a hard seam (#2661). Off by default and in `vanilla`,
left out of `enhanced` (false in both looks, so `quality.rs::enhanced_keys` leaves it out;
a test says so) until the maintainer decides; the settings row is under Graphics > Detail
and Distance after the terrain rows, kept in the INI like them, `--graphics-only
mesh-terrain-blend` on the command line. Skyrim Community Shaders has a Terrain Blending
feature; the design is our own. Every band, angle, resolution and selection rule below is
NVForever's choice [design], not a fact about the original game or another renderer.

**The look the row draws** [design: `mesh_terrain_blend.rs::setting_config`]: the `height`
shape with an 8-unit band, 512 colour texels per cell (round 10b; 1024 before), full shading
(`8:surface:height:512:full` in the test aid's terms). [observed: the main session's
round-7 comparison pictures at a WastelandNV road shoulder] `height` turned the broken
asphalt edge into ground-lit dirt and left the road top alone; the hybrid shapes either
removed the effect or striped the road's lip. Rocks stay subtle with it; that was accepted.

**Method** [design: `terrain_surface.rs`, `nvf-assets/src/nif_surface_blend.rs`,
`nif_surface_bindings.wesl`, `nif_material.wesl`]:

- *A world-space cache of the landscape around the camera.* A dedicated orthographic camera
  on its own render layer (16, `BAKE_LAYER`) draws flat, unlit copies of the 3 by 3 cells
  around the selected centre cell into an RGBA16F target of `3 x resolution` texels a side
  (round 10b: a single-level bake target; the published atlas below has a mip chain).
  Each quadrant's copies are its whole near-terrain stack in drawing order: the base
  (a folded base with its own `TerrainMaterial` in an unlit bake variant, any other base
  with the single-texture `SurfaceCaptureMaterial`) and every blended layer over it with
  its texture, vertex alpha and sort bias, composited by the GPU's own blend state. Nothing
  is lit, fogged or scorched; weather and light changes need no rebake. The selected centre
  is held separately from the committed front publication, with 256 game units of
  hysteresis per axis: retain coordinate `c` throughout the inclusive interval
  `[c*4096 - 256, (c+1)*4096 + 256]`, otherwise select `floor(position/4096)`.
  Backfill and cache assembly use this same selection, with southwest origin centre
  minus `(1,1)`; epoch/off transitions reset it. At `y=-8192`, a northward recentre needs
  `y > -7936`, and a southward return needs `y < -8448`. Teleports select directly.
- *Exact heights.* Beside the colour, an RGBA8 atlas holds the f32 coefficients of both
  triangles of every square of the drawn landscape surface (`height_planes`), packed
  losslessly into bytes and read with `textureLoad`, never filtered; the shader picks the
  triangle with the surface's own diagonal (`grid_triangles`) and evaluates its plane, so the
  band follows the drawn landscape exactly. #2149/#2661: the landscape is drawn and collided
  as a `Surface` with 1 (smoothing off), 2 or 4 squares per LAND square and side, fixed for
  the run before the first cell (nvf_play `smooth_terrain::spec`; the Smooth terrain row
  applies at the next start), and up to its bound off the stored LAND triangles (8 units
  by default; the test aid allows up to 16), so the atlas
  holds that surface's squares, `32 x sub` a cell side (nif_surface_blend.rs
  `HeightLayout`): each cell's `SurfaceCell.heights` are the surface's own (`side()`
  squared, terrain.rs `surface_cell`), and a cell whose heights are not the run's surface's
  is excluded (`invalid-prepared-heights`). The atlas is 576 by 99 texels with smoothing off
  (0.22 MiB, the layout before #2149), 1152 by 195 at 2 x 2 (0.86 MiB) and 2304 by 387 at
  4 x 4 (3.4 MiB), each held twice while a replacement is pending (front and back) and once
  more on the CPU (the image keeps its main-world bytes for the probes). The subdivision
  reaches the shader as definitions (`MESH_TERRAIN_SUB`, the tile and atlas dimensions;
  `MESH_TERRAIN_SUBDIVIDED` only above 1, so with smoothing off the lookup is the one before
  #2149): each publication records its layout and every enabled clone carries it in its
  key (`NifSurfaceBlendKey`), so a run specializes the blend for one layout; the ordinary
  `NifSurface` is unchanged. The lookup finds the ground's square on the surface's grid
  (`terrain_square`, terrain_surface.rs `ground_square`), its normal from the plane's
  slopes over the surface's spacing; the quadrant, the region's edge fade and the
  landscape detail stay on the LAND grid. Each cell's tile ends, after its squares' rows, in
  a metadata row: a nine-bit mask of the resident neighbours and a four-bit mask of the
  quadrants whose colour was captured. Every prepared cell gives its heights even where a
  quadrant's colour cannot be captured. The bake's copies of a quadrant's draws are taken
  at the LAND grid's resolution (terrain.rs `land_resolution`, 17 by 17 vertices whatever
  the surface's subdivision): the bake is flat and unlit, and what it reads (vertex
  colour, every opacity, the packed channels of a folded draw, the texture coordinate as
  the world place over the repeat, anti-tiled or not) is linear on the LAND triangles, so
  the picture is the same as from the drawn mesh, up to float rounding, with about a fifteenth of
  the vertices kept in the main world at 4 x 4. Switching the blend on while the game runs
  prepares the region's up to 9 loaded cells again on the main thread
  (`terrain_surface.rs::backfill_cells`, `terrain.rs::capture_surface`), at the surface's
  resolution: a one-time cost, larger at 4 x 4 than with smoothing off.
- *4 x 4 near (known approximation).* Under the Smooth terrain row's 4 x 4 near (#2676)
  every quadrant starts drawn with every other surface vertex (`QuadrantDraw::HALF`,
  terrain_lod.rs `first_draw`), the one under the camera too, until terrain_lod redraws it
  in full near the camera; a full quadrant beside a half one draws that side coarse (its
  vertices between the neighbour's put on the neighbour's straight segments, one surface
  square into the full quadrant). The atlas holds the full 4 x 4 surface regardless and is
  not rebuilt when a quadrant's drawing changes, so wherever a quadrant is drawn half or
  with a coarse side the band measures from the 4 x 4 surface, not from the triangles
  drawn. [design] What bounds the difference: the drawn coarse squares' corners are 4 x 4
  surface vertices and their diagonals lie on the LAND squares' diagonals, so each drawn
  triangle lies within one stored LAND triangle, and its heights (and a coarse side's
  moved vertices, averages of two surface heights along a LAND square's side) are averages
  of surface heights; the surface lies within its bound (8 units by default) of the stored
  triangles, so the drawn ground and the atlas's are at most twice that apart there. Most
  of it lies beyond the near distance (2048 units by default); near the camera only until
  the first redraw and along a full quadrant's coarse sides.
- *Residency.* Before sampling, the shader needs the fragment's own cell's height and live
  coverage and its quadrant's colour bit, and (round 10b) every own-cell texel the colour
  read takes at each contributing mip level in a captured quadrant of that cell (below); a
  128-unit fade at the region's edges remains. Round 10b review: the read crosses into
  neighbouring cells, and a neighbouring texel counts only when its cell is live-covered
  and the texel lies in a captured quadrant; one that does not (a missing diagonal cell of
  an L-shaped region included) gets no weight instead of rejecting the read. The metadata
  row's residency mask is still written but no longer read by the shader (live coverage
  and the colour masks decide). A failed lookup leaves the fragment exactly as it was.
  #2661 review: the contact weight (below) needs only the height plane, so the lookup
  computes it right after the height check and, in the surface view, stops where it is
  exactly 0, before the colour and detail masks, the neighbour checks and the colour
  footprint; the diagnostic views always run the whole lookup, so their classification is
  unchanged.
- *Publication and continuity.* Each immutable front/back publication owns its images,
  southwest origin and cell identities. A camera crossing retains completed, resident,
  unchanged cells inside the new 3 by 3 region at the **old origin** until replacement
  commits. An enabled-only nine-bit coverage mask intersects baked height and quadrant
  validity in the initial lookup, in every tap of the colour read (round 10b review: own
  and neighbouring cells' alike) and in each border-fade neighbour check. Departing or dirty cells lose coverage without a colour-image upload.
  Continuity covers retained interior footprints; new cells, genuinely invalidated cells,
  teleports without overlap and changing missing-neighbour border fades have no guarantee.
- *Incremental assembly and receipts.* Unchanged cells are GPU rectangle copies into a
  fresh back atlas (round 10b: at every mip level); only entering/dirty cells have flat draw
  copies. Fully populated axial
  and diagonal crossings copy/bake 6/3 and 4/5 cells; one changed cell copies/bakes 8/1.
  Removal alone changes coverage; unchanged frames allocate no atlas and do no copy/bake.
  The small height/metadata image is rebuilt for a replacement. Copies use the bake's
  reversed Y and run in render cleanup **after** `render_system` has submitted all final
  camera-target writes (`engine/vendor/bevy_render/src/lib.rs`), in a following queue
  submission. Exact dirty-draw receipts reset every frame; partial frames cannot accumulate.
  Copy-only work also needs a transfer receipt and both GPU images. *Round 10b's assembly*
  [design: `terrain_surface.rs::assemble_atlas`, `MipPipeline`, `terrain_surface_mips.wgsl`]:
  a render attachment has one level, so the bake camera draws into a single-level bake target
  of the atlas's size, held by the job alone, and the published atlas (every level, no CPU
  copy, never a render target) is written only by the assembly. In the frame every bake draw
  drew, one submission copies the retained cells at every level from the previous
  publication, the baked cells' level 0 from the bake target, then runs one compute pass per
  lower level over the baked cells only: each texel of level `k` is the linear mean of the 2
  by 2 texels of level `k - 1` under it. The resolutions are powers of two and the chain
  stops at one texel per cell (10, 11 or 12 levels), so every level is a 3 by 3 grid of whole
  cells, every 2 by 2 block lies inside its cell and no level ever mixes two cells. The view
  is complete, and the receipt can go, only after that submission is queued, so the receipt
  covers the mip passes; a frame where a draw is missing, or where the mip pipeline does not
  exist yet, assembles nothing and the bake camera draws again. A copy-only replacement (its
  entering cells have heights but no colour capture) has no bake target, an inactive camera
  (no target, no passes) and no mip pass. The pipeline is created when the first bake view
  reaches the render world and dropped when the gate closes (below). Completed views leave
  the sorted cameras before another clear, and publication retains the extra main-frame
  delay. Receipts name epoch, request, destination and source revision; the current cell
  identities and source revision are revalidated before commit. No pixel readback or GPU wait.
- *The blend.* With `d` the fragment's height above the ground triangle, `B` the band, `S`
  the steepness fade and `F` the residency fade, the `height` shape's weight is
  `w = S F (1 - smoothstep(0, B, max(d, 0)))`. *Steepness* (#2661 review: the user's
  play-test pictures showed vertical streaks on some road lips): every texel the blend
  paints is placed by world xy, a top-down projection, so on a near-vertical face one row
  of texels stretches down the face. `S = smoothstep(cos 80°, cos 60°, u)`, with `u` the
  absolute cosine between the mesh face's geometric normal and up: whole up to 60 degrees
  from horizontal, 0 from 80 degrees. The geometric normal is the cross product of the
  world position's screen derivatives, taken at the top of the fragment before any
  discard, not the interpolated vertex normal, which smoothing tilts towards up on a lip;
  where the derivatives are parallel (a face seen edge-on) the vertex normal stands in.
  `S` multiplies every contact shape's weight, and the weight view shows it
  (`nif_surface_blend.rs::face_up`, `steepness_fade`; `nif_surface_bindings.wesl`
  `mesh_terrain_face_up`, `mesh_terrain_steepness`). Faces steeper than 80 degrees get no
  blend, rock and cliff faces included.
  Where `w > 0` the forward fragment, after the existing coverage tests and normal mapping
  and before lighting, mixes the base colour towards the cached colour by `w`; full shading
  also bends the normal towards the ground triangle's geometric normal and moves roughness
  towards the landscape's 0.95, reflectance towards 0.2 and metallic towards 0, and fades
  emission, clear coat and anisotropy by `1 - w`. Alpha, discard, coverage, positions, depth,
  the prepass and the geometric normal used for shadow bias are untouched; `w <= 0` leaves
  every input exactly as it was.
- *The landscape's own layers near the camera (round 10a)* [design: `terrain.rs`
  `terrain_draws`, `terrain_surface.rs` `DetailPalette`, `encode_detail`;
  `nif_surface_blend.rs` `NifSurfaceBlend`, `DetailQuadrant`; `nif_surface_bindings.wesl`
  `mesh_terrain_detail`]. [observed: the user's play-test at 1920x1080 near a road edge,
  round 9b] the colour cache alone reads as a smooth, slightly hazy band: it holds 4 game
  units per texel at 1024 (8 at round 10b's default 512), while the ground beside it is sharp gravel with normal maps.
  So, within 4096 units of the camera, a contact fragment also reads the near landscape's
  own layer textures at its game xy and composes them as the near draw does. *Capture*:
  while the blend is on, each quadrant's capture also records what its near draw is made
  of, from the very meshes and materials drawn (after `stitch_layers`, so stitched
  neighbour layers are included): the base draw's source (the folded `TerrainMaterial` or
  the single base material), the separately drawn overflow layers' materials in drawing
  order, the base mesh's per-vertex linear colour, normal and tangent with the tangents'
  handedness at its LAND vertices (`detail_vertices`), and one opacity grid per drawn
  overlay (the folded ones, then the overflow ones). [design] #2149/#2661, a known
  approximation: the tint and the opacities are linear on the LAND triangles, so their
  interpolation matches the drawn mesh exactly; with smoothing, the normal and tangent are
  the surface's at the LAND vertices interpolated linearly over the LAND triangle, while
  the drawn mesh has the surface's normal at every finer vertex, so the detail's shading
  follows the smoothed ground's curvature only from one LAND vertex to the next. Source material edits re-read it (`refresh_captures`); a source that is gone or
  no longer matches the prepared layers leaves that quadrant on the colour cache. Its
  images (normal maps included) join the cell's dependencies. *Publication*: with the
  capability, each publication's height atlas gains rows after its heights (201 with
  smoothing off, 576 by 300 RGBA8 instead of 576 by 99; #2149/#2661: 101 at 2 x 2 and 51 at
  4 x 4, the same words in fewer, wider rows; the bytes keep their 32-bit words, read with
  `textureLoad`): a 32-word entry
  per quadrant (flags with the folded and total overlay counts, palette numbers of the
  base's diffuse and normal map and of each overlay's, the base diffuse and normal
  samplers, normal decoding bits, the base material's colour and texture-coordinate
  transform, the repeat, whether the coordinate is cell-local as in a six-layer folded
  draw, the tangents' handedness), then 11 words per vertex of the quadrant's 17 by 17
  grid (linear colour as unorm16, normal and tangent as snorm16, up to 12 opacities as
  unorm16). A third texel in each cell's metadata row holds the four-bit mask of the
  quadrants whose detail it supplies. The palette (`TerrainPalette`) lists the distinct
  layer images and sampler descriptions (`ImageSampler` equality; texture.rs gives the
  landscape textures one description) of the admitted quadrants; every replacement repacks
  its own palette numbers. Quadrants are admitted whole, in a deterministic order (those the
  previous publication admitted for unchanged cells first, then by grid and quadrant); a
  quadrant that would overflow the palette, an image not loaded or an unresolved material
  keeps the colour cache, with its reason on the publication line (round 10a review: also
  `sampler-mismatch` and `overflow-material`, below). *Binding*: the blend
  type's bindless index table (binding 100) is 50..454 instead of 50..54: after the four
  ordinary entries come 384 texture and 16 sampler entries, registered through the blend
  material's own allocator (bindless slots are local to a material type's slab) and
  resolved through the clone's own table row; unused entries hold the fallback image. No
  binding is added, bindless or not, and without bindless the type's layout is exactly the
  colour interface's (uniform 50, textures 51 and 53, sampler 52). Every clone of a
  publication binds that publication's palette, restricted to its live coverage (round 10a
  review, `PublishedSurface::live_palette`): when cells are masked (a departing or changed
  cell, or one whose image was removed: a removal bumps the image's dependency revision
  and so masks every cell reading it in the same update), the entries only masked cells'
  quadrants read become the fallback, keeping their numbers (the immutable height atlas's
  numbers keep naming the same entries), and a sampler still sampled takes as its
  representative an image a covered quadrant reads with that description (second review
  round: a quadrant samples its diffuse sampler only with a base diffuse or an overlay
  and its normal sampler only with a base normal map, so a textureless quadrant keeps no
  sampler alive; a sampler without a live representative binds the fallback sampler,
  never the publication's original representative, which may be the removed image). The coverage
  update rebuilds retained clones (Bevy drops the old prepared material before preparing
  the new one), and new clones of the front take the same restricted palette, so neither
  can wait for an image that is gone and the retained clones keep drawing. The masked
  cell's replacement bake cannot wait for one either (second review round): a folded
  base's bake copy binds no normal map (`terrain_surface.rs::bake_copy`; the unlit bake
  returns the base colour only), and a cell whose bake copies would bind a colour image
  removed from the image assets (and not added back) is planned without that cell,
  excluded as `missing-colour-image` (an image not loaded yet is still waited for); it
  bakes again once the image is added back. A detail image that returns (added back after a
  removal) or arrives late (a replacement planned without it refused its quadrants,
  `image-not-loaded`) bumps its dependency revision too (third review round), so the
  resident cell's request changes and the quadrant's detail is rebuilt instead of staying
  on the colour cache; only the first of its Added / LoadedWithDependencies notifications
  does, so one return makes one replacement. *Composition* (`mesh_terrain_detail`): the
  fragment's ground triangle and its barycentric weights come from the height lookup's own
  square and diagonal (below the diagonal `(1-u) SW + (u-v) SE + v NE`, above it
  `(1-v) SW + u NE + (v-u) NW`); the colour starts as the base diffuse x interpolated vertex
  colour x base material colour, and each overlay in drawing order mixes in by
  `saturate(opacity)` (ordered opacities, not normalized weights; #3789: not its diffuse alpha), its
  colour the texel x vertex colour x the base material colour for folded overlays (as
  terrain.wesl) and x white for overflow ones (their own material's colour:
  `layer_material` draws only a layer with a texture, white). With full shading the
  normal starts as the base's mapped normal (or the plain interpolated one), Bevy's own
  `calculate_tbn_mikktspace` and `apply_normal_mapping` on the interpolated, not
  renormalized, normal and tangent, the normal maps decoded with green flipped and each
  map's two-component flag (folded overlays with the base's, as terrain.wesl; overflow
  ones with their own), mixed towards each overlay's mapped normal (or the plain one) by
  the same shares and normalized once. Texture reads use explicit gradients
  (`textureSampleGrad`): the derivatives of the fragment's game xy are taken in
  `nif_material.wesl` at the function's top level, before the lookup's branches, then
  mapped through `1 / repeat`, the base's texture transform and the view's mip bias; the
  base diffuse sampler reads every diffuse and overlay normal map, the base normal sampler
  the base normal map (terrain.wesl's rule for the folded overlays). *Samplers the detail
  does not reproduce* (round 10a review; the palette format keeps one diffuse and one
  normal sampler per quadrant): a textureless base binds Bevy's fallback image and so its
  fallback sampler, which reads the folded overlays in the near draw and need not equal
  any layer image's description (with anisotropic filtering off the configured fallback is
  1x, while texture.rs gives landscape textures their own description); separately drawn
  overflow layers read their diffuse and normal maps with those images' own samplers.
  So a quadrant with folded overlays on a textureless base, or with an overflow layer whose
  diffuse or normal image's description differs from the detail's diffuse sampler (the
  base diffuse's, or the first overlay's without a base texture), keeps the colour cache,
  logged as `sampler-mismatch` (`terrain_surface.rs::samplers_reproduced`). Toggling
  anisotropic filtering between texture loads can make such descriptions differ in
  ordinary play. The target is never taken from the mesh pixel's
  screen-space prepass normal. *Sources and distance*: colour validity (the quadrant and
  the whole footprint of the contributing mip levels, round 10b) and detail validity (the quadrant's detail bit, live
  coverage, nearer than 4096 units) are checked separately, so a valid detail lookup never
  fails for an unused colour tap, and the `status` view shows success where either source
  serves the fragment. With `d = 1 - smoothstep(2048, 4096, distance)`: both valid, the
  target colour is `mix(cache, layers, d)` and the target normal
  `normalize(mix(geometric, mapped, d))`; detail alone, the layers with the weight faded by
  `d`; colour alone, the cache and the geometric normal; neither, no blend. #2661 review:
  where the layers' share is 1 (`d = 1` with both valid, or detail alone) the cache is not
  read, and where valid detail is nearer than 2048 units (so `d` is exactly 1) the colour
  footprint is not checked either: status, weight and share are the same either way. The contact
  weight and the residency fade are unchanged. *Overflow layers and the cap* (NVForever's
  choice, round 10a): [observed: the main session's publication lines at the user's test
  road] cell (-17, 0) publishes quadrants `folded+1-layers, folded+2-layers, folded,
  folded+1-layers` and cell (-17, -1) `folded, folded+1-layers, folded, folded+2-layers`:
  road cells routinely have overlays beyond the six that fold, because `stitch_layers`
  adds neighbour textures, so sending such quadrants back to the colour cache would leave
  exactly the user's spot blurry. The detail therefore also composes the separately drawn
  overflow overlays, after the folded ones and in their drawing order, up to 12 overlays
  plus the base (12 opacities fit three vec4s; a vertex record is 11 words with the colour,
  normal and tangent); a quadrant with more keeps the colour cache, logged as
  `overlays-over-cap`. The detail composes every overflow layer with white at full alpha
  through the base's texture transform; the near draw uses each layer's own material
  colour, alpha and transform (`layer_material`: white, opaque, the default transform). A
  quadrant whose overflow layer's material has another colour or alpha, or another
  transform than the base's (a base transform edited alone included), keeps the colour
  cache, logged as `overflow-material` (round 10a review,
  `ResolvedDetail::from_materials`); the check runs at capture (turning the blend on, a
  backfilled cell, a cell drawn while it is on) and again at every source material refresh
  (`refresh_captures`), and both keep the capture with the reason (second review round:
  the capture too, `terrain.rs::DetailParts::capture`), so an edit back serves layer
  detail again. This is an approximation: the near terrain alpha-blends each
  overflow layer's *lit* result, while the detail composes albedo and normal before
  lighting once. [hypothesis] The difference is small because the layers share the
  surface's lighting and differ only in their mapped normals. Every match described here
  is of prepared triangle attributes, texture mapping, ordered surface composition and the
  normal-map convention for the supported quadrants; it does not promise identical final
  pixels (filtering footprints on differently oriented mesh geometry, lighting position,
  shadows, occlusion, the near landscape's prepass normals, scorch and coverage still
  differ) and does not remove geometric silhouettes. *Budget*: 384 images and 16 sampler
  descriptions per publication, so one material's resources (its standard textures, the
  two atlases, the palette) stay inside the 512-resource bindless slab; a quadrant reads at
  most 26 images (13 diffuse, 13 normal), so at least 14 entirely distinct quadrants fit,
  and quadrants sharing LTEX textures share entries. *Capability*: the blend material
  bindless on the device and non-uniform indexing of sampled-texture arrays
  (`mesh_terrain_blend.rs::detail_capability`); otherwise, or with the aid below,
  publications carry no detail rows and no palette and the colour cache alone serves, as
  before. Nothing of this runs while the blend is off (no capture, no rows, no palette).

**Selection** [design: `world.rs::spawn_model`, `blend_census`;
`mesh_terrain_blend.rs::reference_reasons`, `material_reasons`]: rigid exterior STAT and
SCOL shapes. A SCOL is a static collection: "composed of Static forms", "do not react to
Havok physics and cannot be destroyed", its Model "the NIF file used for the object", which
the editor generates when the collection is made [documented: GECK wiki "Static
Collection", revision 40901, and "Bethsoft Tutorial Static Collections", revision 38886,
offline snapshot 2026-10-28]. `world.rs` draws a SCOL reference as that one model, as it
draws a STAT, so the same reference, shape and material rules apply to both. A reference is
left out for any of: not in an exterior cell of the drawn world, a rig or skinning,
particles, jointed or loose bodies or a stored pose, damage-stage nodes, a DEST record, an
imposter; a shape for external emittance, a registered morph mesh, or one drawn under a
billboard holder that turns it to the camera (#2549; `excluded-billboard`, last in the
reasons' order, merge of upstream main f4dbc82f8). A material is left out
for Add, Multiply, Premultiplied or AlphaToCoverage alpha, unlit, refraction, first person,
depth write, transmission, falloff, holes, FaceGen, the Pip-Boy screen, linear add, soft
edges, a non-finite depth bias or an environment map, window shapes (#2462) and, since
#2841, every lit shape with the Environment_Mapping flag and a cube
(`excluded-environment-map`: the blend type has no environment bindings, its index
table's 54-453 hold the landscape palette). Where both could apply, the reflection wins
and the shape is drawn without the blend [design: the reflection is the original's
own effect, the blend NVForever's opt-in addition, off by default in both looks;
carrying the cube, sampler and mask into the blend type would move its palette indices];
ordinary Blend, a finite depth bias, the lit decal flag,
Mask and an alpha cut are admitted and keep their descriptors exactly. A material owned for
the moment by a fade, an effect shader or a material override never blends; the owner's
restore brings the blend back. Never by asset name.

**Excluded altogether**: interiors, distant LOD and imposters, actors and creatures,
first-person and interface meshes, movable clutter (MSTT, items, posed clutter), doors,
activators, trees and grass, water.

**Materials and switching** [design: `mesh_terrain_blend.rs`]: an eligible shape draws with
a separate material type, `ExtendedMaterial<StandardMaterial, NifSurfaceBlend>`, a clone of
its source shared per source handle and visible publication/configuration; the ordinary
`NifSurface` interface is
unchanged (its 144-byte uniform, bindings 50-53), and the blend type binds no more textures
than the ordinary pipeline it replaces (the atlases take the holes and FaceGen slots, which a
blend clone never has). A source edited under the same handle (blended cut-outs flipping
Blend and Mask, scripted NIF edits) rebuilds its clone in the same frame. Ordinary-component
change ticks are tracked even before a shape's first clone is ready, so an emissive edit
and reinsertion evicts the pending candidate and its later acknowledgement cannot enable it.
Texture swaps, fades, draw-distance ranges, door fades, effect shaders and shadow thinning
follow the source. Replacement clones are prepared and acknowledged with a bound render material before
existing shapes switch; their old enabled handles and image references stay alive meanwhile.
Request restarts leave enabled clones alone. Coverage changes on already drawable clones
change uniform POD only, at each clone's own publication origin; the colour image stays
immutable. A fresh back allocation waits for the previous publication's material switches;
a cancelled job has a three-main-frame retirement interval before another allocation.
Transfer plans hold strong image handles and cancellation drops their main-world references;
extracted views and material assets release theirs through Bevy's ordinary retirement.
The row switches the blend while the game runs: on, it opens the material gate
(below), logs its start line, checks the atlas against the device, records the census of
the shapes already drawn and the captures of the cells already drawn around the camera
(their landscape prepared again, the same quadrant stacks and cached materials as a cell
drawn with the blend on), and bakes; off, every converted shape leaves the blend material,
and the census, the captures, the atlas and the clones are dropped, with one `stop` line.
The restoration (`stop`) runs in PostUpdate where the conversion runs, after the Update
producers (texture swaps, fades, effect shaders, overrides) and the PostUpdate ones (the
NIF mirror, the blended cut-outs) with their commands applied: an ordinary material a
producer put on the shape that frame stays (a texture swap's copy, whose generation the
swap has consumed; a fade's or an override's copy), and only a shape with none gets its
recorded source back.

**Per frame** [design: `mesh_terrain_blend.rs::apply_configured_variants`, `pass_needed`,
`full_pass`]: the conversion is change-driven. Its full pass (every tracked shape's reason
mask, census, clone and material swap, then the collection of unused clones and
publications) runs only in a frame where one of its inputs changed since the last pass:
the blend's look, the publication (`None`, or its revision and colour atlas), a last pass
that ended with a replacement still pending (then every frame until it settles), a tracked
shape's components (a `BlendShape` or `BlendCensus` added, replaced or removed, a despawn
included; an ordinary material put on, written or removed on a convertible shape; on a
census-only shape, one put on or written with another handle than the last pass recorded,
or removed; a fade's, effect shader's or override's marker added or removed), or a source
material the last pass read (loaded,
removed, or edited under its handle so that its reasons or its `SourceStamp` differ),
compared only when the material store was changed since the conversion last ran. The stamp
is watched only for a source a shape that may blend read; a source only shapes kept out
read (a fade's or an override's copy, whose alpha moves every frame; a census-only shape's
material) is watched by its reasons and whether it is loaded, since such a shape becomes
allowed only through its reasons, its source loading, a temporary marker's removal, or its
ordinary material, `BlendShape` or `BlendCensus` added, replaced or removed, each a trigger.
Of a census-only shape's ordinary material (no `BlendShape`: an animated STAT's driven
shapes, kept out by their recorded `rig` reason) the pass reads only the handle, which
source's reasons join its mask; the source's own changes are the fingerprint's, and the
eviction of a clone whose owned source was republished under its handle reads only
convertible shapes. So an animated look published again under the same handle (nvf-assets
`material_anim.rs`, most frames while it moves) runs no pass. [observed] Smoke
`mesh-terrain-blend-verify-terrain-switches` (`--slots 3`, release build), final verify
line: at 070c331fa, before this rule, `skipped-frames=1277 passes=1289 pass-shapes=1275`;
at 398b61cda `skipped-frames=3354 passes=20 pass-shapes=4 shapes-ordinary-census-only=3`;
`mesh-terrain-blend-verify-crossing` went from `passes=3137 pass-shapes=1772` to
`passes=1547 pass-pending=1530 pass-shapes=5` (its readiness delay aid keeps it pending).
That the old shape passes were animated statics' same-handle republications is read from
this rule being the only change between the two heads and from the per-signal counts
398b61cda prints, not seen directly at 070c331fa. A convertible shape keeps the wider rule,
because there a same-handle republish is the signal of an in-place edit.
The first frame after the blend turns on always runs it. A pass's own commands can call for one more pass in the next frame, which changes
nothing. Every frame, pass or not, follows the coverage of the recorded publications
(`follow_coverage`: one check per publication, a walk over its clones only when its
coverage changed or it gained a clone). A quiet frame then stores `settled` again and
refreshes a ready line a census change deferred (60 frames, `REFRESH_FRAMES`) from the last
pass's census.
Before this, the full pass ran every frame: [observed] Tracy, RTX 5070 Ti, 2560x1440,
WastelandNV (-17, 0): `apply_variants` 0.61-0.65 ms a frame before this change; with the
blend on from the start (animated statics tracked), 0.61-0.63 ms at 443434169 and 0.023 ms
at a7b32dac7, at Goodsprings and at WastelandNV (-17, 0). The test
aid `NVF_PLAY_TEST_MESH_BLEND_VERIFY=1` runs the full pass in every frame the gate would
skip and logs any change it makes (`verify diverged`, at most 16 lines), and a count line
`verify skipped-frames=N diverged=M passes=P pass-<reason>=...` (each pass by its first
reason), then `shapes-<signal>=...` (which shape signal fired in a pass run for a shape
change), with each ready line, every 600 skipped frames and at the stop; at most 2
`verify shape-trigger` lines per signal name the model, shape index and reasons of a shape
that fired it.

**Off** [design: `mesh_terrain_blend.rs::BlendGate`, `close_gate`;
`terrain_surface.rs::SurfaceBakePlugin`; vendored bevy_pbr `MaterialGate` and bevy_render
`ErasedRenderAssetPlugin::gated`, engine/vendor/README.md]: both material types, the
clones' `NifBlendMaterial` and the bake's `SurfaceCaptureMaterial`, are registered behind
one gate, closed in a run that never turns the blend on. While it is closed none of their
own systems run (main world: change marking and the specialization check; render world:
asset extraction and preparation, instance extraction and sweep, specialization
extraction), their shaders are not loaded, no bind group allocator and no bindless
fallback buffer exists for them, the bake view is not extracted, the draw functions are
Bevy's own, round 10b's mip pipeline does not exist, and the receipt and span systems do not run (they need a bake view in the
render world). Nothing is captured, baked, allocated or logged. What still runs that main
does not have, every frame: `follow_setting` (the aid and the row compared with what is
drawn); the run conditions themselves, an atomic load for each gated system or group, a
resource check for each material type's opening and closing and for putting the draw
functions back, and an empty-query check for the receipt and span systems (the span
systems' once per view); and bevy_asset's tracking systems of the two asset types, whose
stores exist, empty (an asset type cannot be registered after startup). After the blend is
turned off, the gate stays open until no material of either type, as an asset or on an
entity, has been left for 3 frames (their removals extracted and unloaded); then the
render world drops both allocators and shader handles (and round 10b's mip pipeline), puts Bevy's own `DrawMaterial`
command back in the opaque and transparent phases, and an `idle` line is logged, so the
run is idle as above again. Pipelines compiled while it was on stay in Bevy's pipeline
cache, as every pipeline does (Bevy never evicts them).

**Cache and invalidation rules** [design: `terrain_surface.rs`, `terrain.rs`]: the 3 by 3
region follows the camera; cell identity includes session/worldspace epoch, grid, resident
entity incarnation, capture content revision, component change tick and the revisions of
its captured diffuse-image dependencies. Residency is confirmed against `Streamer.loaded`.
Terrain reconstruction replaces the capture with a new revision; reconstruction of an
already captured resident input also makes its cardinal
neighbours reprepare stitched layer inputs, including loaded captures outside the selected
region, retaining their primary revision so this cannot recursively dirty the whole world.
Failed refreshes remain pending by entity incarnation within the cache context, are excluded
from coverage/copy/bake until refreshed, and clear on epoch/off transitions or unload.
Component replacement/in-place capture edits and modifications/removal of dependent
diffuse images invalidate the affected cells.
Capture materials retain their source handles: source-material changes refresh the actual
unlit clones and their diffuse dependencies before replanning, rather than rebaking stale
copies. Source-material removal makes that capture height-only until reconstruction.
This supports these engine reconstruction/material/image routes; arbitrary live
LAND record editing, near-mesh mutation outside reconstruction and load-order editing are
not implemented by this cache. Weather/light changes need no rebake. Worldspace/session,
interior, off and incompatible-resolution transitions cancel jobs and clear publications
before receipts are accepted, even without a camera. Request IDs and visible publication
revisions are separate from that epoch. Source asset events are consumed after
`AssetEventSystems` and before receipt acceptance; conversion uses the result next frame.
The ready line's historical `generation` field now names the visible publication revision.

Colour resolution is 512 (default since round 10b), 1024 or 2048 per cell side, checked
against the device's texture limit (level 0's width) and refused when it does not fit; an
explicit 1024 or 2048 from the aid keeps its meaning. One RGBA16F atlas with its mip chain
(`terrain_surface.rs::colour_bytes`) is about 24, 96 or 384 MiB (level 0 alone 18, 72 or
288); front plus back at the default 512 is **about 48 MiB of colour** (36 without mips),
against 144 MiB at round 10a's default 1024 without mips. A baking replacement also holds
its single-level bake target (18 MiB at 512) until it commits or is cancelled, so a
crossing peaks at about 66 MiB of colour at 512 (front, back and target), and a cancelled
replacement's images retire with it; neither atlas nor target keeps a CPU copy any more.
HDR render intermediates and retiring/extracted handles come on top. There is one active replacement,
fresh destinations rather than recycling referenced images, readiness admission and
cancelled-job retirement; rapid replanning cannot enqueue an unbounded allocation list.
Readiness admission waits for the front's materials to settle, but never longer than 1800
frames in a row (round 10a review, `SETTLE_FRAMES`): a candidate that can never be prepared
cannot hold every later publication back; the replacement then starts with a
`settlement wait expired revision=... frames=...` warning, so at most one allocation per
1800 frames comes that way.
*Colour filtering (round 10b)* [design: `nif_surface_bindings.wesl` `terrain_colour_lod`,
`terrain_colour_taps`, `terrain_colour_mips`, `terrain_colour_footprint`; `terrain_surface.rs`
`lookup_level_status`, `colour_read_taps`]: the colour read's level of detail is log2 of the
larger of the fragment's two screen-axis footprints in level-0 texels (the continuous
game-xy derivatives `nif_material.wesl` takes at the function's top level, before every
branch, times the texels per game unit) plus the view's mip bias, clamped to the atlas's
levels. The read takes the level below it and the one above with the fraction as its
share, and mixes the two. Each level is filtered per cell (no texel of any level covers
two cells), but the 3 by 3 atlas puts neighbouring cells' texels side by side, so a level's
read is the plain bilinear footprint at the fragment's position: near an edge its taps are
the cell's outermost texels and the neighbour's adjacent ones, on one grid as regular
across the edge as inside a cell (round 10b review: round 10b clamped the read into the
fragment's own cell, and since the two cells' edge texels stand for different places,
a few units apart at level 0 and whole regions apart at coarse levels, even a smooth
colour jumped at the edge, by a whole cell's average at the last level). The shader loads
the four taps (`textureLoad`) and weights them itself. A tap counts when it lies inside the
atlas, its cell is live-covered and the texel lies in a captured quadrant of that cell (a
texel of a level with two or more texels per cell side covers one quadrant, the one its
position names; the last level's covers all four); one that does not gets no weight and
the others are renormalized. Where a whole neighbouring column or row is missing that is
exactly the read clamped to the fragment's own cell, which with the missing-region border
fade is the fallback at the region's edges; at a corner whose diagonal cell is missing the
other three taps remain. At the last level, one texel per cell at the cell's centre, the
read interpolates between the centres of valid neighbouring cells rather than taking one
whole-cell average. The weights depend on the position and the taps' validity only, never
on which cell the fragment is in, so the fragments on both sides of an edge between two
valid cells read the same value: the colour is continuous there at every level and level
of detail (`colour_reads_are_continuous_across_valid_cell_edges`, a linear ramp, which a
bilinear read across valid cells returns unchanged). The colour is valid when, at each
contributing level, the footprint holds a tap of the fragment's own cell and every own-cell
tap lies in a captured quadrant (the tap holding the position always has at least a
quarter of the weight); the cell's own live coverage is part of the height check. So a
fragment whose footprint reaches an uncaptured quadrant of its own cell, at a level it
reads, keeps the layers or no blend, and no texel of an uncaptured quadrant or uncovered
cell ever gets a weight: never a darkened mix. There are still no gutters; beyond 4096
units, and where a quadrant keeps the colour cache, geometric triangle normals remain the
target.
Round 10a's detail adds about 0.45 MiB of height atlas per publication (0.9 MiB front and
back; whole atlases with detail rows: 0.66 MiB with smoothing off, 1.3 MiB at 2 x 2, 3.85 MiB
at 4 x 4), 1600 bytes of index table per enabled clone and the palette's descriptor entries;
the layer images are the landscape's own, not copied. Scorch is excluded: capture has no
burn-map binding or synchronization, so full shading still differs through scorch,
separately lit overflow layers (composed before lighting, above), shadows/occlusion and
mesh geometry/coverage. The row stays height-8.

**Test aid** `NVF_PLAY_TEST_MESH_BLEND=[band:]view[:shape[:resolution[:shading[:flat,steep]]]]`
overrides the row for comparison runs and is not stored; a malformed value is refused in
the log and the row decides. Omitted parts take the row's look.

| Part | Values | Default |
| --- | --- | --- |
| band | positive finite number | 8 |
| view | `surface`; diagnostics `weight` (blue 0 to red 1, unlit), `normal` (the ground triangle's game-space normal as `0.5 + 0.5 n`, unlit), `selection` (a lit green tint on converted shapes), `status` (unlit: magenta outside the region, red no height, orange the quadrant's colour not captured, cyan a bilinear tap rejected, green success brightening with the weight) | (required) |
| shape | `height`; aid-only `penetration` (`1 - smoothstep(-2, 0, d)`), `hybrid` (v2: the band widened with the angle between the unperturbed mesh normal and the ground, from the flat width parallel to the ground ramping to the band at 5 degrees, the band from 5 to 35 degrees, ramping to the steep width at 75 degrees; the larger of that height band and the penetration term), `hybrid-v1` (round 5's orientation-gated band) | `height` |
| resolution | `512`, `1024`, `2048` | `512` (round 10b; `1024` before) |
| shading | `albedo` (colour only), `full` | `full` |
| flat,steep | hybrid's widths, `flat <= band <= steep`; only with `hybrid` | `band / 16`, `6 band` |

Old short forms keep working with these defaults (`24:status` is band 24, `height`, 512,
full). `NVF_PLAY_TEST_MESH_BLEND_DETAIL=colour` (round 10a) keeps the colour cache alone for
comparisons (any other value is refused in the log). Log lines: `mesh terrain blend: start
band=... variant=... resolution=... noise=0 shape=... shading=...` then ` flat=... steep=...
weight-version=2` for `hybrid` or ` weight-version=1`, then round 10a's resolved detail mode,
` detail=layers` or ` detail=colour detail-reason=aid|device|no-device`; `colour atlas resolution=... atlas=WxW rgba16f colour-mib=... levels=...` (round 10b: the whole mip chain's MiB, rounded, and its level count);
`backfill tracked-shapes=... convertible-shapes=...` as it turns on; an
`assembly epoch=... request=... copied=... baked=... retained-origin=... destination-origin=... levels=...`
line per replacement (round 10b: the levels copied per retained cell and filtered per baked one) and one `publication ...` line per commit (resident grids with each
quadrant's capture path and colour mask, the
excluded slots, a probe of the uploaded heights under the camera with its CPU lookup
status (the colour cache's classification), `probe-normal` and `probe-detail-mask`, the
image IDs, the resolution, then round 10a's ` detail=layers|colour palette-images=N
palette-samplers=N detail-quadrants=N colour-quadrants=[(x, y):qN:reason;...]` naming each
quadrant that keeps the colour cache with its reason, or ` detail=colour
detail-reason=capability` without the capability); `ready eligible-shapes=...
resident-cells=... generation=... variant=... shape=... resolution=... shading=...`, the
weight fields, then the census (`stat-shapes`, `excluded-shapes`,
`pending-material-shapes`, one `excluded-<reason>` count per reason, `admitted-blend`,
`-bias`, `-decal`, `-mask`, `-alpha-cut`, then `scol-shapes`) and round 10a's ` detail=layers
detail-quadrants=N` (the publication supplies layer detail) or ` detail=colour`, printed at
each publication
and refreshed at most once per 60 frames; `census-shape ref=... base=... model=... shape=...
status=eligible|excluded first-reason=... reasons=... base-type=STAT|SCOL`, one per distinct
base, shape and reason mask, at most 2048 a run; `stop restored-shapes=... tracked-shapes=...
released-cells=... kept-producer-materials=...` (`restored-shapes` counts every converted
shape back on an ordinary material, `kept-producer-materials` those that kept one a
producer put on that frame); `idle materials-released=true` when the gate closes. A road without a `census-shape` line need not be untracked: the lines
stop at 2048, a shape whose material is still loading waits for its line, and a line is
printed once per base, shape and mask with the first reference seen, so a search by
reference can miss it; the ready line's counts cover every resident shape.

**Continuity test aid** [design: `terrain_surface.rs::continuity_probe`]:
`NVF_PLAY_TEST_MESH_BLEND_CONTINUITY=start-seconds:duration-seconds` (`--play` only, on
the same world-aid clock as LOOK/MOVE/SHOTS, `nvf-play/src/pause.rs`) arms only after an
initial publication and prepared shape witnesses in the starting camera cell; duration is
bounded to 120 seconds and 20,000 counted frames. It counts every subsequent main frame,
checks retained unchanged witnesses using their actual enabled clone's origin, height and
coverage, and checks render preparation acknowledgements. Frame lines are
`mesh terrain blend: continuity frame=N camera-cell=... transition=BOOL pending=BOOL
published=BOOL retained=N missing-enabled=N missing-prepared=N failures=N copied=N baked=N`.
The final `continuity summary frames=N crossings=N pending-frames=N retained-lookups=N
pending-retained-lookups=N unwitnessed-pending-frames=N replacements=N published-false=N
missing-enabled=N missing-prepared=N failures=N copied=N baked=N settled=BOOL recentres=N
pending-reversals=N` totals the full interval. Recentres count selected-centre changes;
pending reversals count direction reversals when work was pending in the preceding probed
frame. Pending includes atlas work, retirement and
material readiness; settled requires the final publication's material switches to finish. `NVF_PLAY_TEST_MESH_BLEND_READY_DELAY=N` delays each
new material's acknowledgement by at most 120 render frames for deterministic pending work;
it changes no appearance controls. These CPU/binding checks do not prove rendered pixels;
`mesh-terrain-blend-crossing` also captures before/during/after/settled frames for review.
`NVF_PLAY_TEST_MESH_BLEND_CROSSING=<y>:<z>:<delay>/<x>[,...]` (round 10a, after the round-9b
review: the go-to aid ran on the aids' elapsed clock and its 0.1-second pairs could fall in
one frame) lets the probe drive the crossing on its own world clock after arming: a step
puts the player's feet at game `(x, y, z)` `delay` world seconds after the previous step
(after arming for the first), and each later step also waits until the probe has seen the
previous step's recentre in a later frame with work pending in the current one, so every
reversal lands on a distinct later frame while a replacement is pending; a step still
waiting 10 world seconds after it was due goes anyway and counts as a timeout. Round 10a
review: a step's replacement could be prepared and settle (the 120-frame readiness delay
is one second at 120 frames per second) before a step with a 3-second delay came due,
which would then wait for pending work that never came, up to its timeout. So, while
steps remain, the blend materials first seen after a step are held unacknowledged until
the driver issues the next step (`BlendReadiness::hold`, `mesh_terrain_blend.rs::acknowledgeable`):
the step's replacement stays pending until then, whatever the next delay. Materials seen
before a step (the retained clones among them) are never held, and nothing is held after
the last step or the probe's summary; without the aid nothing is held. Lines:
`continuity crossing step=N x=... y=... frame=N world-secs=... gated=BOOL pending=BOOL
timeouts=N`, then `continuity crossing done steps=N timeouts=N`. The staging go-to before
arming stays with `NVF_PLAY_TEST_GOTO`. Its rapid reversals move 384 units to either side
of the boundary, beyond both hysteresis thresholds, with delayed material readiness; the summary requires at least four crossings
and recentres, two reversals during pending work, and final settlement. Synthetic tests
also count live images, meshes and capture materials across repeated cancellations and
completion using Bevy asset retirement over real app updates (not just request replacement).
Its boundary road-contact location remains a hypothesis until the main session runs it.

**Height blending and anti-tiling** [design: `nif_surface_bindings.wesl::mesh_terrain_detail`,
`terrain_surface.rs::ResolvedDetail::from_materials`, `bake_root`, `SurfaceCaptureMaterial::from_source`,
`terrain_capture.wesl`; merge of upstream main f4dbc82f8, 2026-11-16]: §3.12 (`terrain-height-blend`,
#2550) and §3.13 (`terrain-anti-tiling`, #2551) change the near landscape's per-pixel
composition: the folded draw's layer weights and where every landscape texture is read. The
blend reproduces both, so its detail and its colour cache match the near draw with either
switch on, off or both; nothing falls back to the colour cache because of them.

- *Detail.* The switches are read from the very materials the near draw uses, as their
  pipelines select them: a folded base's `TerrainLayersKey` (height blending, with its
  uniform's depth, gain and source, the start-up values of `NVF_PLAY_TEST_HEIGHT_BLEND`;
  anti-tiling and its texture repeat), a single base's and each overflow layer's
  `NifSurfaceKey::anti_tiling` and `terrain_repeat`. They travel in the quadrant's table
  words (`DetailFeatures`: word 0 bits 4-6, words 28-31). With either on, the base draw is
  composed as terrain.wesl's `TERRAIN_HEIGHT_BLEND` and `TERRAIN_ANTI_TILING` paths compose
  it, with the same shared functions: `terrain_weights.wesl` (`terrain_ordered_shares`,
  `terrain_height`, `terrain_height_weights`) and `terrain_sampling.wesl` (`terrain_hex3`,
  the tap rule, `terrain_normal_tap`, `terrain_world_normal`). Anti-tiled, the layers are
  walked last to first and read only where a share can be above 0, the heights use the global
  reference always, and every anti-tiled read shares one footprint (the place in the world
  over the repeat, then the base's transform) and one set of taps. The height reference is
  placed as terrain.wesl's `height_reference_at` places it (transcribed; that function is
  not in a shared module). Overflow layers mix in afterwards as before, anti-tiled where
  their own material is; height blending never reaches them, as it does not in the near
  draw. A quadrant whose anti-tiled draws do not share one repeat keeps the colour cache
  (`anti-tiling-repeat`; terrain.rs gives them all `TerrainSettings::tile`).
- *Colour cache.* The folded bake copy is a clone of the source material with
  `TerrainLayersKey::surface_bake` beside the two switches, so terrain.wesl bakes the same
  height-blended or anti-tiled colour (`SURFACE_BAKE` returns after every composition path).
  A single-texture copy of an anti-tiled base or layer reads its texture anti-tiled too
  (`terrain_capture.wesl` with `TERRAIN_ANTI_TILING`). The bake now draws its copies at the
  cells' true world places (`bake_root`: the root moved to the publication's origin), so the
  world lattice the anti-tiled reads hang on is the near draw's. Mip levels come from the
  bake's own view (8 game units a texel at 512), as they always did there, so a local height
  reference picks its level from that view [hypothesis: a small difference from the near
  draw at the local source only, the default being global].
- *Switching.* The rows flip the source materials in place (`terrain.rs::sync_height_blend`,
  `sync_anti_tiling`); their Modified events make `terrain_surface.rs::refresh_captures`
  read the detail's switches again and remake the bake copies, and the changed cells bake
  and publish again, so both follow the rows while the game runs.

**Cost**: GPU cost of round 9 is unmeasured (checks only, no game run). The bake is timed
as the GPU span
`nvf_mesh_terrain_bake` (Tracy's GPU zones and the `--log-frame-times` list,
`terrain_surface.rs::bake_span`), from before the bake view's first pass to after its
last. Its two timestamps go into command encoders of their own, queued at the two systems'
places in the render order, so the span pairs the same with `NVF_PLAY_TEST_PROFILE_ENCODERS=1`
(whose automatic `enc:<system>` span opens on a render context's own encoder) as without
it. Retained rectangle transfers are submitted after that camera span and are not included
in it. The lookup in the meshes' fragments shows in the main opaque pass instead. While the
blend is on, the bake's receipts reach beyond the bake: from the first bake view on,
`install_bake_receipts` wraps Bevy's `DrawMaterial` for every Opaque3d and Transparent3d
draw, not only the bake's, and `reset_receipts` takes finished bake cameras out of Bevy's
`SortedCameras` in each frame a bake view is in the render world (Bevy rebuilds that list
every frame); `uninstall_bake_receipts` puts Bevy's own `DrawMaterial` back when the gate
closes, and frame costs measured with the blend on include both.

**Limits** [hypothesis where marked]: beyond 4096 units, and in quadrants that keep it, the
colour cache may soften contact detail, and a footprint turning invalid at a missing corner
can make a narrow transition [hypothesis]; there geometric triangle normals, not the
landscape's normal maps, are the shading target, so faceting can show [hypothesis]; a
quadrant on the colour cache beside one with layer detail can show a boundary
[hypothesis], including the quadrants refused as `sampler-mismatch` or `overflow-material`
(round 10a review: the palette carries neither per-layer samplers nor the overflow
layers' material colour, alpha and transform, so those quadrants are refused rather than
approximated); the detail's distance thresholds are initial choices for play-test review,
and its GPU cost (texture bandwidth, descriptor entries) is unmeasured; the visual
acceptance (gravel frequency and mapped-normal response at the close road shoulder at
1920x1080, grazing views, layer boundaries, the 2048-4096 transition, publication
crossings) needs the main session's frames, which logs and naga do not prove; shadows, occlusion and the separately lit layer draws still differ from
the landscape's own lighting; admitted Blend and decal shapes near the ground (glass,
markings) take the landscape's colour; buried visible surfaces take its look; the census
scans every tracked shape in each frame the full pass runs (only its line is limited;
streaming cells, fades and material edits on tracked shapes run it); rigged STATs and SCOLs drawn while the blend was off are not counted until their
cell is drawn again (they never convert); the colour-coded `excluded` view was not built.

**Tests and smoke**: `nvforever` test filters `mesh_terrain_blend` (switching off during a
texture swap, a fade and an override: `switching_off_during`), `terrain_surface` (round 10a:
`palette_admits_whole_quadrants_in_order_and_refuses_overflow`,
`colour_only_publications_carry_no_detail`,
`crossing_steps_wait_for_the_seen_recentre_and_pending_work`; round 10a review:
`delayed_steps_find_the_previous_work_still_pending_after_it_prepared`,
`samplers_the_detail_does_not_reproduce_keep_the_colour_cache`,
`overflow_material_edits_keep_the_colour_cache_until_reproducible_again`,
`settlement_guard_waits_but_never_forever`; second review round:
`a_textureless_survivor_never_restores_a_removed_sampler_representative`,
`removed_images_never_leave_the_replacement_waiting_on_an_unpreparable_draw`; third review round:
`a_normal_map_added_back_restores_the_detail_once`,
`a_normal_map_loaded_late_ends_up_with_detail`; round 10b:
`colour_mips_validate_every_contributing_level`,
`mip_dispatch_covers_each_baked_cell_and_no_more`,
`copy_only_replacements_draw_nothing_and_copy_every_level`; round 10b review:
`colour_reads_are_continuous_across_valid_cell_edges`,
`coverage_drops_unloaded_diagonal_taps_and_rejects_invalid_quadrants`), `mesh_terrain_blend` (round 10a review:
`removed_palette_image_keeps_retained_clones_drawable_and_the_replacement_publishes`; the
change-driven pass: `the_full_pass_runs_only_in_frames_where_an_input_changed`,
`a_pending_replacement_keeps_the_pass_running_until_it_is_usable`,
`a_deferred_census_report_comes_from_quiet_frames`,
`a_coverage_change_restricts_retained_clones_and_runs_the_pass`,
`a_kept_out_shapes_animated_copy_runs_no_pass_until_its_marker_goes`,
`an_allowed_shapes_source_alpha_edit_runs_the_pass`,
`the_verify_aid_runs_skipped_frames_and_reports_a_missed_change`),
`terrain::tests` (round 10a: `detail_capture_reconstructs_the_drawn_triangles`; second
review round: `an_initially_rejected_overflow_capture_recovers_when_the_edit_is_reverted`;
height blending and anti-tiling: `the_mesh_blend_detail_composes_as_the_near_draw_with_and_without_the_switches`
(a CPU mirror of the detail against the module's mirrors of terrain.wesl, every switch
combination and both references), `the_detail_reads_the_switches_from_the_materials_drawn`,
`the_switches_reach_the_detail_and_the_bake_copies_while_the_game_runs`),
`mesh_blend_shader_tests` (round 10a:
`shader_detail_transcribes_the_mesh_triangles_and_the_record_layout`; round 10b:
`colour_mip_filter_validates_and_reads_only_its_cell`; height blending and anti-tiling:
`shader_detail_composes_height_blending_and_anti_tiling_as_the_near_draw`, and the naga
compositions of the blend with the shared modules, of terrain.wesl's bake with both switches
and of the anti-tiled single-texture capture), `graphics::tests::mesh_terrain_blend`,
`settings::tests::the_mesh_blend_row`, `texture_swap`, `nif_mirror`, `fade`,
`draw_distance`, `door_fade`, `effect_shaders`, `shadow_thin`; `nvf-assets`
`nif_surface_blend::tests` (round 10a: `detail_layout_fits_the_height_atlas_and_round_trips`,
`detail_quadrant_words_name_folded_and_overflow_overlays`, with the switches' words), `nif_material::tests`;
`nvf-play` `settings_layout`, `fade`. Smoke: the `mesh-terrain-blend-*` scenarios, round 10a's
`mesh-terrain-blend-detail-colour` (the aid's colour fallback) among them.
The WGSL is composed and validated by naga in the tests, not executed. Smoke:
`mesh-terrain-blend-crossing` (retained witnesses, pending work, replacement and zero
publication/material/lookup outages), `mesh-terrain-blend-setting` (the row on from the
command line), `mesh-terrain-blend-live`
(on and off from the settings page, then the gate closing), the verify aid's
`mesh-terrain-blend-verify-crossing`, `-verify-live` and `-verify-terrain-switches` (the
gate's skipped frames change nothing), the aid's `mesh-terrain-blend-goodsprings`,
`-options` and `-widths`; `graphics-enhanced` checks that the enhanced look neither turns
it on nor logs it.

### 3.15 Heat haze at distance (`heat-haze`; a prototype, off in both presets)

[design] **A prototype**, an addition of NVForever's own for the enhanced look: not a fact
about the original game and never used to explain a difference from it. Off by default and
in no preset, `--graphics enhanced` (`Graphics::preset(Preset::Enhanced)`) included (a test
checks it); only the settings page's `enhanced` row went with the menu overhaul. The row
"Heat haze (experimental)" (Graphics > Lighting and Effects) or `--graphics-only heat-haze`
turns it on. Code: `heat_haze.rs`, `heat_haze.wesl`.

- **What.** The shimmer over hot ground in the distance: on a hot, clear day the far ground
  band near the horizon is moved a little, mostly up and down, by a slowly rising pattern
  that drifts sideways with the weather's wind. Easy to overdo, so slight.
- **Technique.** Screen-space refraction: the picture read again at a place offset by a
  perturbation, with a mask so that what stands in front does not leak into the refracted
  read, as T. Sousa describes it in "Generic Refraction Simulation", GPU Gems 2 (2005),
  ch. 19 [documented: cited publication]. The perturbation is two layers of lattice value
  noise (Ebert et al., "Texturing and Modeling: A Procedural Approach", lattice noises
  [documented: cited publication]) written in the shader: an integer hash of each cell
  (odd multipliers chosen at random), smoothstep-interpolated, repeating every 64 cells so
  the CPU keeps the scroll within one period without a seam. No texture and nothing of the
  game's data; no code taken from any source.
- **Where in the frame.** One full-screen pass on the main camera (`PostProcess`) after the
  temporal resolve (the game's TAA after tonemapping; FSR and DLSS in `EarlyPostProcess`),
  after the ambient occlusion and the sun shafts, before bloom and sharpening and the game's
  image-space blur and grade. A distortion before the resolve would move pixels the motion
  vectors do not describe, and smear. It reads the picture and the depth prepass only, and
  writes neither the depth nor the motion vectors. Both upscalers expand their render-size
  depth to the display size by nearest texels and copy it back over the prepass depth
  before this pass (`upscaler/prep.rs` `expand_depth`, run by `fsr_pass` and `dlss_pass`
  alike), so under FSR and DLSS the mask reads a display-size depth; with multisampling it
  reads sample 0, as the ambient occlusion does. DLSS frame generation and FSR's frame
  generation take their frame without the HUD after `PostProcess`, before the UI
  (`dlss/frame_gen.rs` `tag`, `frame_gen.rs` `capture_hudless`): that copy includes the
  shimmer, while the depth and motion they are given do not move with it.
- **What is never distorted.** The HUD, menus and the pause page are Bevy UI drawn after
  `PostProcess`; the Pip-Boy's screen and a 3D scope's lens are other cameras drawing into
  textures shown on first-person shapes, and the pass is only on the main camera. The
  first-person arms, weapon and Pip-Boy are drawn with their depth squeezed into the nearest
  share of the range (nvf_assets `first_person_depth.wesl`): read raw they lie within 1.02
  times the near plane's distance, so the distance mask covers them; near things are nearer than the mask's
  start. A pixel with a near direct neighbour is not moved (the anti-aliased fringe of the
  weapon or a near post), and a displaced read is not taken when any of the four texels of
  its bilinear footprint is nearer than 6,000 units (the pixel keeps its colour), so near
  outlines never bleed into the far shimmer. The exception: blended shapes that are not in
  the depth prepass (a muzzle flash, particles, glass) over the far band are moved with what
  lies behind them, by up to the amplitude.
- **The mask** (per pixel): distance along the view ray from the depth prepass (reverse
  infinite depth: view depth = near / depth): none nearer than 6,000 units, full from
  25,000 (smoothstep); the view ray's elevation: full from 6 degrees below the horizon to
  0.5 above it, gone at 4 above (the sky just above a far ridge may shimmer a little, the
  upper sky never) and at 15 below (a steep view down crosses little hot air); the sky dome
  itself (beyond `SKY_DEPTH_FROM`) at half.
- **The displacement.** Up to 1.5 pixels at 1080 lines at strength 1 and full mask, scaled
  with the picture's height; the horizontal part 0.35 of the vertical. Two layers (36 x 100
  and 85 x 230 cells per picture height, weighted 0.65 and 0.35), rising at 0.012 and 0.02
  picture heights a second, drifting sideways at 0.04 picture heights a second times the
  wind's strength along the screen's horizontal (`tree::wind_vector`, which follows
  SetWindDirection), and moved with the world as the camera turns. Time is the game's clock
  (`Time<Virtual>`), so a paused game holds still. One bilinear read of the colour where it
  moves and the pixel's own colour; the pixel's depth everywhere, its four neighbours' and
  the read's four only where the mask is above 0; no blur. When the largest displacement
  (strength x amplitude x the debug multiplier x the picture's height) would be under 0.05
  pixels the pass does not run.
- **What follows the weather** (`heat_haze::sky_heat`, a pure function of the live
  `SkyState` every frame, unit-tested; the weather records hold no temperature):
  strength = sun x weather, held to 0..1, finite.
  - Sun: smoothstep(0, 8 degrees) of the elevation now x smoothstep(10, 50 degrees) of the
    elevation an hour earlier (the ground is hottest after noon): none at and below the
    horizon.
  - Weather (each weather's own share, blended from the one fading out to the one fading in
    with the sky's fade, so a transition never pops; a scripted SetWeather or ForceWeather is
    followed the next frame): none in a weather classified Rainy or Snow or with the Rainy
    flag; a weather classified Cloudy keeps 0.2 (for mods and the Fallout 3 leftovers: no
    New Vegas weather is); the brightness of the weather's sunlight colour at the hour (WTHR
    "sunlight", Rec. 709 luma of its bytes as display values): none up to 0.15, full from 0.6
    (NVWastelandClear's day sunlight 255, 227, 170 gives 0.90, NVWastelandHazy's 0.95,
    HVDustStorm's 0, 0, 0 gives 0, so the storm is off by its own data); the weather's own
    fog share at 25,000 units by the curve the fog shader draws (((d - near) / (far -
    near))^power, the span held to at least 1, a power not above 0 read as 1; `--fog` in its
    place, linear): full up to 0.6, none from 0.88; the wind (WTHR DATA wind speed / 255):
    full up to 0.25, none from 0.45 (most weathers store 50, the dust storm 127).
  - The records' day fog share at 25,000 units [observed: FalloutNV.esm, fnvtool esm decode,
    2026-11-17; the curve: our arithmetic]: NVWastelandClear 0.29 (0.35 with its night
    power), NVHooverDamWeather 0.44, NVWastelandGS 0.46, NVWastelandBorders 0.52,
    NVWastelandClearEast and NVColoradoRiverWeather 0.58, NVSearchlightWeather2 0.73,
    NVWastelandHazy 0.89, HVDustStorm (far 0) 1.0: every clear-ish weather keeps its full
    share, the searchlight weather loses some, the hazy weather and the storm keep none.
  - **The base game's overcast is not weakened.** NVBlackMountainWeather, which looks
    overcast, is classified Pleasant like the clear weather, has the same four cloud layers
    (three of them the transparent placeholder `sky\alpha.dds`, as every New Vegas weather
    looked at has) and the same sunlight and ambient colours; only its fourth cloud texture
    and its fog colour differ [observed: FalloutNV.esm, fnvtool esm decode, 2026-11-17]. So
    neither the classification nor the layer count can tell it from clear (an earlier
    version that counted layers cut every base weather to 0.64). Cloud cover has to come
    from the cloud textures' own alpha: the composed cloud cover of #2904 (now on main)
    is to go in `weather_heat`, a follow-up.
  - None indoors. Values outside the base game's ranges (fog far 0 or a million, huge or
    negative near, power 0, wind 255, no cloud textures, values that are not numbers) give
    a strength within 0..1 (tests). With the strength 0 the pass does not run.
  It only adds to the picture the weather's own fog and colours made, and the haze in that
  fog bounds it: the hazier the weather, the less shimmer.
- **Log.** `heat haze: on, strength <s>, at most <p> px (sun <a>, weather <b>)` (or
  `(indoors)`; `, pass skipped` after the pixels when they are under 0.05) when it turns on,
  and `heat haze: strength <s>, ...` when the strength moves by a tenth, reaches or leaves 0
  or crosses the skip; never per frame. The render side logs once, when the pass first
  draws: `heat haze: pass drawn (<w>x<h>, multisampled depth yes|no)`. GPU time: the
  `nvf_heat_haze` span (`--log-frame-times`).
- **Debug aids** (development tools: they do not make a run unattended):
  `NVF_PLAY_DEBUG_HEAT_HAZE=<multiplier>` scales the amplitude (0 to 10),
  `NVF_PLAY_DEBUG_HEAT_HAZE_VIEW=mask` draws the mask times the strength as a grey picture,
  `NVF_PLAY_DEBUG_HEAT_HAZE_FORCE=1` holds the strength at 1 in any weather; each logs a
  `debug switch` line when it is set.
- **Smoke.** `heat-haze` (Goodsprings at noon under NVWastelandClear: a strength above 0,
  the pipeline ready and the pass drawn, no shader or rendering error) and
  `heat-haze-interior` (strength 0 indoors).
- **Measured** [observed 2026-11-18, PR #2977: Goodsprings start view at 13:00 under
  NVWastelandClear, 2560x1440, sun shadows on, `--no-actors`; median of five 4-second
  periods, two rounds, on the contributor's machine]: with the row off the frame time
  matches main's (3.09-3.10 ms against 3.11-3.13); on at full strength +0.10 ms a frame,
  the pass 0.09 ms on the GPU. With the row off the pictures match main's to within what
  two runs of main differ by; the mask-alone pictures show it under the clear weather and
  none under the hazy weather, the dust storm or at night.
- **Not done.** The sky band's height above a ridgeline (about 4 degrees at half weight)
  and the strength are starting values judged by eye in play at half, full and double
  strength; the other constants are choices by reasoning. No temperature or ground
  material is read (no record holds one); the base game's overcast keeps the clear
  weather's strength until the composed cloud cover of #2904 (now on main) is read in
  `weather_heat`, a follow-up; the pattern lies on the screen and follows turns, not the
  world's depth; a mirage (the sky refracted into the ground) is not attempted. Not
  measured: a slower GPU, 1920x1080, a weather mod.

### 3.16 Desert haze (`desert-haze`; off in every preset)

[design, #2675] An addition of NVForever's own, an engineering choice for the enhanced look:
not a fact about the original game, never a parity item, and never to be used to explain a
difference from the original's frames. Issue #2675 asked for "exponential height fog in low
ground" pooling in basins at dawn, as part of `enhanced`; the Mojave has dust and haze, not
mist, so it was reframed (on the issue): a **thin dust haze that thickens with distance and
toward the ground, far ridges fading in layers, warmer toward the sun**, outdoors only, as
far as the shown weather's own fog leaves room for it. The `enhanced` row the issue named no
longer exists (the settings menu overhaul); the haze is a row of its own, "Desert haze
(experimental)" under Graphics > Lighting and Effects, off by default and in every preset.

- **Technique.** Exponential height fog: the density falls as e^(-k y) with the height y
  above a base height, integrated in closed form along the view ray from the camera to the
  fragment: optical depth = density x L x (e^(-k y0) - e^(-k y1)) / (k (y1 - y0)), with the
  limit e^(-k (y0 + y1) / 2) where the two heights are within 1e-3 / k (2.7 units); share =
  1 - e^(-depth). y0 and y1 are the camera's and the fragment's own heights; only the length
  L is held to the reach (below), so for a fragment at a given height the share never falls
  with distance. The sun's forward glow is a power of the cosine between the view ray and
  the sun. Both as Inigo Quilez describes them in "Fog"
  (<https://iquilezles.org/articles/fog/>, the article Bevy's fog shader also cites)
  [documented: cited article]; our own code from that description, no code from any other
  project. Not a volume: Bevy's volumetric fog was dropped in 2026-10 (§4).
- **Where it sits.** Per fragment, in the world shaders' fog path, no pass of its own, so it
  covers everything the weather's fog covers (NIF materials, terrain, trees, grass, water,
  the distant LOD, transparent things) under every anti-aliasing and upscaler. Bevy's fog
  function gets only the distance; its caller `apply_fog` (bevy_pbr `pbr_functions.wesl`)
  has both positions. `haze.rs` `haze_edits()`, a list of its own beside lights.rs's lighting
  edits, puts `nvf_haze` before `apply_fog` and passes its result to the atmospheric fog
  call as the `scattering` argument, which Bevy reads only while the fog's directional
  light alpha is above 0 (never set here); only that call line changes. fog.rs `GAME_FOG`
  then blends the haze and the weather's fog **once**, in display values as the fog is
  blended outdoors: total share 1 - (1 - fog)(1 - haze), the colours weighted as the haze
  seen through the weather's fog (the same as mixing the haze first and the fog over it), so
  nothing is darkened twice. Interiors (the linear blend) are untouched.
- **Carried in the fog uniform's free fields** (the view bind group does not grow): `bi.z`
  the density at the base height (0 is off), `directional_light_exponent` the base height,
  `directional_light_color.rgb` the colour toward the sun, its alpha 0. Bevy reads those
  two fields only when that alpha is above 0 (its fog in-scattering, §4), so they were free.
  The shader's constants (k, the floor, the reach, the sun's power) are written into its
  text from `haze.rs`'s Rust constants when it is patched: one place.
- **Off is as on main.** With the switch off the density is 0, `nvf_haze` returns the
  argument it was given (vec3(0)) and `GAME_FOG` runs its old two lines: the same picture.
  The edits are made at the start whatever the switch, with no log line, so the log is as
  on main too. Both edits fail safe: if Bevy's text is not as expected the edit is not made
  (an error line), Bevy's shader stays, and the haze stays off; either edit alone changes
  nothing (no function signature changes).
- **What follows the weather** (from `SkyState` every frame, through weather fades, so
  scripted SetWeather/ForceWeather, transitions and the hour show at once and without a
  pop; no weather names, no hour table, no per-weather result kept):
  - *Weight* (0..1): the weather's own fog share at 50,000 units with its near, far and
    power as the fog uniform has them; full up to a share of 0.5, falling smoothly to none at
    0.85; times the fog's strength (a fade into or out of no fog). From the fog numbers only:
    continuous, never rising as the fog gets nearer or denser. NVWastelandClear (0..200,000,
    power 0.6 by day, 0.5 by night) has 0.44 and 0.5: full. NVWastelandGS (10..120,000, 0.5):
    0.63. NVTheStripWeather (2,000..200,000, 0.35): 0.77. NVWastelandClearEast (0..100,000,
    0.4): 0.17. Any fog that is full at 50,000 (NVWastelandHazy 1,800..30,000) and
    HVDustStorm get none [observed: FalloutNV.esm, fnvtool esm decode, 2026-11-17]. HVDustStorm's day fog 0/0 is no range, so the renderer shows its night fog
    510..55,000 power 1.0 all day (`Weather::fog_at`): share 0.91 at 50,000, none.
  - *Reach*: the extinction length at the base height is 0.9 x the fog far and the length
    counted is at most 0.5 x the fog far, so the optical depth stays under 0.56 (43 %)
    along the base height at normal wind, and at most 2 x 1.25 times that (1.39, 75 %) with
    the strongest wind and below the base (times the debug multiplier), whatever the far: a
    mod's far of 1,000,000 gives a haze five times thinner, not a wall, and the distant LOD
    keeps its shapes.
  - *Wind*: x(1 + 0.25 (wind - 50) / 50), held to 0.75..1.25 (the base game's weathers
    nearly all store 50; a wind of 255 gives 1.25 times). Blended across a fade.
  - *Colour*: the fog colour the weather's fog uses now (`SkyState::color(ColorType::Fog)`), moved
    toward the weather's sunlight colour by up to 0.4 looking into the sun, by the cosine to
    the sun to the 8th power (Bevy's own default for its fog's directional light; the two are
    `haze.rs` `SUN_MIX` and `SUN_POWER`, one line each); the sun
    term grows in as the sun climbs from the horizon to 4° and is nothing at night. The
    sun's direction is the view's first directional light (the sun; the first-person arms'
    shade light is its child and points the same way). No colour of our own.
  - *Base height*: the landscape under the camera (the cells' stored LAND heights, decoded
    once and kept for the last four cells), followed over 2 s, at once after a jump of a
    cell. Where the worldspace has no LAND under the camera the stand-in is 128 units below
    the camera, followed over 20 s, so it does not climb onto roofs with it. Below the base
    the density grows no further than 2 times (1,893 units down), so basins far below a
    ridge haze over without turning white.
- **Chosen values** (by eye, for the user to judge from pictures; constants at the top of
  `haze.rs`): at a clear noon, the camera on foot and a fragment at its height, the haze
  alone is 10 % at 20,000 units and 27 % at 60,000 (the starting values asked for were
  10-15 % and 25-35 %; the weaker end chosen); 3,000 units above the base a third of the
  optical depth.
- **Weather and mods** [design]: values outside the base game's ranges are tested as data
  (fog far 0 and 1,000,000, near above far, power 0 or below, NaN, wind 0 and 255): no NaN,
  no negative values, no division by zero, the weight monotone.
- **The sky and the horizon.** The sky dome and clouds are drawn without fog (`sky.wesl`);
  the dome's horizon band takes the weather's horizon colour (`sky.rs` `dome_color`), the
  fog the fog colour. The haze is continuous in distance and has no edge of its own, so it
  adds no seam at the horizon or at the LOD boundary; far land fades further toward the fog
  colour, so where the fog colour and the horizon colour differ, the edge between land and
  sky that main already shows stays (stronger or weaker by how far apart they are). The sky
  is unchanged with the switch on or off. A follow-up could move the haze's colour toward
  the dome's horizon colour at low view angles (a sky-side change: the horizon colour is not
  in the fog uniform).
- **Not done**: no pooling in low ground by the hour (the issue's dawn fog), no noise or
  drifting dust, nothing indoors, no shadowing of the haze by terrain (Bevy's fog
  in-scattering fetches a shadow map per fragment; §4). No land from a parent worldspace:
  NVForever draws each worldspace's own LAND only (worldspace.rs), so a city worldspace's
  cells without LAND (the Strip, Freeside) take the slow stand-in above, which follows the
  camera's height over 20 s instead of the ground.
- **The water reflection** (left as it is): the reflection camera clones the main camera's
  fog (`reflection.rs` `update_reflection`), so the planar reflection's haze is worked along
  the mirrored ray from the camera mirrored below the water. Its length is the real
  reflected path's, and its far part (water to fragment) lies at the real heights, but its
  near part (camera to water) lies at heights mirrored below the water, so far below the
  base that the density there is at the floor's 2 times. From a camera high over a lake a
  reflected ridge gets up to about 1.4 times the haze its real path would. The main
  view then fogs the water surface over the camera-to-water distance again, as it does the
  weather's fog on main. A fix needs the water height in the haze (fold the heights at the
  water), which the fog uniform has no field for; a base height worked for the reflection
  camera alone would make the far part too thin instead.
- **Switching**: the row, `--graphics-only desert-haze`, a `--graphics-cycle` set
  `desert-haze`, `NVF_PLAY_TEST_SETTINGS=<t>:desert-haze`. Switching changes no pipeline
  (only the uniform), so nothing compiles. Log lines: `haze: desert haze on: <weather>:
  weight <w>, wind x<f>, <a> % at 20,000 units and <b> % at 60,000 on the base height,
  reach <r>, base z <z>`; the same figures again (as `haze: <weather>: ...`) when they move
  notably (weight by 0.1, density by a quarter, another weather), not during a time-lapse;
  `haze: desert haze off` (`... off indoors` on going in). Debug aid
  `NVF_PLAY_DEBUG_DESERT_HAZE=<multiplier>`: the density times that (1 as designed; 0 to
  100), logged once when the haze first turns on; it does not turn the switch on. Smoke:
  `desert-haze` (switched on and off at Goodsprings at noon). The fog call is patched
  into Bevy's `apply_fog` at start for every player, row on or off; if Bevy's text no
  longer matches, one `warn!` line says so and the row does nothing (the fog stays
  Bevy's).
- **Measured** [observed 2026-11-18, PR #2976: Goodsprings start view at 13:00 under
  NVWastelandClear, 2560x1440, sun shadows on, `--no-actors`; median of five 4-second
  periods, two rounds, on the contributor's machine]: with the row off the frame time
  matches main's (3.09-3.10 ms against 3.11-3.13), on +0.03-0.04 ms. With the row off the
  pictures match main's to within what two runs of main differ by (pixels more than 2
  levels apart, HUD masked: the street at noon 0.34 % against 0.30 %, at 18:30 0.61 %
  against 0.41 %, an interior 0.002 % against 0.002 %); with it on, NVWastelandHazy,
  HVDustStorm and an interior stay as with it off (0.0-0.1 %). Pictures against main
  (1920x1080): from the air over Goodsprings at 12:00 looking north-east, at 18:30 west,
  at 06:30 east, under the overcast and the hazy weather, and the street, in the PR. The
  cost is not measured on a slower GPU or at 1920x1080.
### 3.17 ALCS (experimental) (`light-shadows`; off by default, not part of `enhanced`)

**Name.** ALCS stands for All Lights Cast Shadows; the row is ALCS (experimental) since
2026-11-24 (ALC shadows before; "ALC shadows" below is the same row).

**Rewrite of 2026-11-24** (a play test, two videos and two pictures of Doc Mitchell's
house: stripes over lit walls and faces, an actor going dark and back every few seconds, a
carried gourd's shadow left on the wall; "the shadows do not update real time"). Causes
found and what replaced them (light_shadows.rs, light_shadow_detail.rs, the patched
bevy_pbr):

| Fault | Cause | Now |
|---|---|---|
| Stripes on lit surfaces | Bevy's point-light soft shadows scale the blocker search and the filter by the light's size as a plain number (a fraction of a metre in mind); with a lamp size of 24 game units the search swept most of the cube and the filter grew to many texels, each tap compared against the fragment's one depth. The depth bias (0.08, a length in world units) was a millimetre. | The renderer's own soft shadows in world units, each tap compared against the fragment's surface along it (`nvf_sample_shadow_cubemap_soft`); biases of 1 unit and 1 texel (`DEPTH_BIAS`, `NORMAL_BIAS`). Spot lamps keep the camera's filter. |
| An actor goes dark, then pops back | A skeleton marked the lamps only when the box round all its joints changed by 4 units; a leaning or gesturing actor stood in the shadow of its old pose until then, or until the periodic refresh (600 frames, 4 s at 150 fps). | Any joint moving 0.5 units marks (`MOVE_EPS`, under the bias); shapes by centre and corner. |
| Shadows lag or stay behind | Lights took turns (every frame, or every 2nd or 4th), 3 a frame at most and the nearest always first, so with more marked lights than draws the rest never drew; any mesh despawned anywhere redrew every lamp near the camera in full; a shape with no record was baked into the static cache at its first move. | Every marked face in view draws in its frame, 8 lights a frame on Quality and 3 on Performance, the longest wait first (`draw_priority`); every caster's place is kept from the switch-on (`ShapeAt`), so one that goes or first moves marks exactly the faces it was in. |
| Shadows blink at a cache redraw | A face drawn into the static cache was copied out without its moving casters, which came back a frame or more later. | Such a face is drawn quietly (`HoldLightShadowMaps::static_quiet`) and composited next frame; a due composite goes before another cache redraw. |
| Static shadows missing after a turn of the view | A light told to draw on last frame's visibility and lost from view this frame counted as drawn. | `skip_fixtures` marks it again (`Pass::lost`). |

**Second round, same day** (the player's hand test of the first build: "the shadows on
Performance are very blocky and update slow", "on Quality also not 100% real-time"; the
session's own log showed no light waiting for a draw). The maps were current; their texels
showed. With a lamp close over a caster a 512 map's texel is several units wide on the
floor, Performance's filter (the camera's, a fraction of a texel wide on these maps) drew
each as a block, and a moving edge jumped block by block. Now: every lamp with a slot takes
the game's own filter, with a fixed width of 1.75 texels and 8 taps where it has no soft
size (Performance), and never narrower than that on Quality; Quality's cubes are 1024
(`cube_side`; 25 MB a cube and as much again for the static cache: 1.4 GB for 28 lamps,
logged). The soft size goes to the renderer in `HoldLightShadowMaps::soft_size` and no
longer through the light's `radius`, which the lighting reads too (a lamp's highlight grew,
dimmed and popped). The ambient occlusion's lamp factor (§3.1) now follows a lamp's shadow
fade (`ssao_lighting/hook.rs`): a lamp fading its shadow in or out had neither shadow nor
occlusion for the fade's length. Open from the occlusion's audit, for the player: with ALCS
on, the casting lamps' light takes no occlusion (it is their shadow's job), so indoors the
occlusion is left on the ambient term alone; its radius is capped at 80 render pixels, about
5% of the distance at 4K; a casting placed spot lamp would dim first-person arms as the
flashlight does (none seen cast in the scenes run).

**Casters not ready** (same day; the player: the shadows "load very slow, sometimes taking
up to 30 secs" after entering an interior, a load or a new game, and switching the row
"sometimes causes them to not load correctly"). A light's maps are drawn once and held; a
caster whose mesh or material was still loading, or whose shadow pipeline was still being
built, was not in them, and nothing drew the light again until the periodic refresh (3,600
frames: 24 s at 150 frames a second; 600 before the rewrite, which hid it). Loading the
player's save, the room's furniture cast no lamp shadow in pictures at 0.5 to 16 s and did at
24 s. Now the renderer tells the game which lights' views were drawn without all their
casters (`bevy::pbr::nvf_take_unready_shadow_lights`), and the game draws those lights
again in full, a little later each time (`SETTLE_FRAMES`) and every 48 frames while
pipelines are being built (`BUILD_WAIT`, a first run with a new picture mode); the loading
screen's pre-draw waits for it within its cap. The same save: the shadows are in the picture
at 0.5 s, the screen no longer up (1.8 s); a first run with SMAA or 8x multisampling, never
run before, has them at 1 s. Checked with and without upscaling on that save, the
occlusion and ALCS on Quality: no anti-aliasing, 4x multisampling, TAA, SMAA, FSR 3.1 and
DLSS, each without an error and with the shadows and the occlusion in the picture; the row
switched Off, Quality, Performance and Quality again mid-game, and the occlusion Off and
on, each back within a second. The occlusion alone does not change between 0.5 and 26 s
after a load: it was the lamps' shadows that came late.

**Two sizes of maps** (same day). An Ultra choice for both rows was built with them ("an
Ultra Quality for AO and ALCS that will run at full resolution": the near maps at the
Shadow resolution row's full side and the far at 1024, 1.8 GB for nine lamps; the occlusion
at full resolution, 0.41 ms against 0.13) and dropped after the player's play test:
"basically not noticeable even at 4k native". What follows is what stayed. The point lights' cubes are in two textures:
the `Tuning::near` lights nearest the camera (8, one already there keeping its place against
one 1.25 times nearer, `near_set`) in the near one, the rest in the far one, each with a
static cache of its own (the patched bevy_pbr's `HoldLightShadowMaps::far_slots`, a third
cube binding, 50, in the mesh views (45 at first; 45 and 46 went to the volumetric light, 47 to 49 are for the bounce light of #4442); a light's slot there is 128 + its slot, read in
`shadow_sampling.wesl` `nvf_point_far`). Sides (`cube_sides`): Performance 512 for all
(1024 with Shadow resolution 4096), no far texture; Quality 1024 and 512. A light that has to change texture does so in sight of
the camera, two a frame at most, and draws all its faces with every caster in that frame
(`LampState::reslot`), so its shadow does not leave the picture; its caches follow. Memory
(logged): the player's living room, 9 lights, 453 MB on Quality (604 MB with all at 1024). Checked: a teleport every 2 s round the Prospector Saloon (24 lights)
moved 41 lights between the textures without an error, a cache miss or a missing shadow in
the pictures; the row switched through Performance, Quality and Off; the Vulkan validation
layer reports nothing of the new binding or the copies. Cost against
the build before, the saloon walked: the same (GPU 4.4-4.9 ms, the main pass 0.7-0.9 ms).
A first try sized the near texture by every shadowed light drawing, the far ones too, and
made it anew whenever they drew; the `light-shadows*` scenarios' cache-miss checks caught
it.

**Light sizes** (same day, after the play test that dropped Ultra; the player: "Implement:
AO on highlights and soft-shadow width per lamp type/size"; the occlusion's part is in a
pull request of its own). A lamp's light size, which sets how soft its
shadow's edge is, is its fixture's (`light_shadow_detail.rs` `lamp_soft_size`: 0.8 of the
mean of the two larger half extents of the nearest fixture shape within 160 units, held to
4..40 units); a light with no fixture in reach, a fill or ambient light, takes a tenth of
its range (24..48); a muzzle flash 8, a grass fire 30, an explosion 48. Every lamp had 24.
The player's living room: its nine lamps come to 7..40 units, all by a fixture (a shape has
its bounds only a frame or more after it is spawned: the lamps look again when bounds
appear). In that room's picture 0.7% of pixels change by 4 levels or more with the sizes.

**Third round** (2026-11-25, after the play test of those: "character shadows look better on
Performance sometimes as the Quality shadows have some visual problems", "character shadows
seem to be very heavy", and a frame rate that fell over a session of switching the rows and
entering interiors until the game was restarted).

*The soft edges.* Compared in Doc Mitchell's opening (the doctor seated under a lamp by his
head, his shadow on the floor two metres below): Performance drew it whole; Quality left a
smudge, at the fixtures' sizes, at 24, at 12 and at 6 units alike. Two causes. The sizes were
a ball of light half a metre and more across, and the game hangs its lamps close over what
they light, so the penumbra on the floor came to tens of centimetres: they are a bulb's and
a shade's now (`FIXTURE_SHARE` 0.2 of the fixture, 2..6 units; 4 without a fixture; a muzzle
flash 2, a grass fire 6, an explosion 8). And the filter could be wider than the blocker
search (0.07 against `light_size / distance`): the shadow was blurred by the whole penumbra
but shown only where the search had found a blocker, a faint blob with a hard outline. The
search covers twice the light's width now, the filter is held to it, and both to
`NVF_SOFT_FILTER_MAX` 0.015 (about 0.9 degrees; at 0.03 the doctor's shadow was still a
smudge next to Performance's).

*What a drawn face cost.* The player's save in Primm's casino hall (25 lamps casting, a dozen
residents moving: 5 to 8 faces drawn a frame): the GPU took 3.9 ms a frame, the frame 7.2 to
8.1 ms against 6.3 ms with the row Off: the processor's, not the GPU's. A build with spans
(`--features profile`, `NVF_PLAY_TEST_PROFILE`) put it in the render thread, 8.6 ms a frame
against 4.6 ms: every face drawn ran Bevy's whole camera schedule for its view and recorded
five command buffers. The lamps' shadow views are drawn inside the camera's own passes now,
as the sun's cascades are (engine/vendor/README.md "hosted shadow views"): the render thread
6.0 ms in that build; in the release build that hall runs at 6.4 to 6.5 ms a frame on
Quality (Performance 6.4, Off 6.3; the display's 160 Hz is 6.25).

*The session that slowed.* The player's logs showed it: the same save, the same room, 75 to
125 frames a second at the end of an 82-minute session and 130 to 150 after the restart,
with the row's counters alike in both. Reproduced with the console (`coc` between the
Prospector Saloon, Doc Mitchell's house and Goodsprings, the rows switched off and on, 30 s a
round): the new log line `gpu resources:` (with `--log-frame-times`) counted 5,200 bind
groups at the start and 33,600 after eight rounds, 1,800 more for every cell entered, and the
submission of a frame's command buffers went from 0.58 ms to 1.27 ms. Switching the rows
alone left the count where it was; travelling with the row Off held 250 to 330. Upstream's
renderer never drops the bin-unpacking and uniform-allocation bind groups of a view that is
gone, and every lamp of every cell entered had six such views (engine/vendor/README.md
"stale unpacking bind groups"). They are dropped now: 250 to 390 bind groups in the saloon
after twelve cells, the submission 0.5 to 0.6 ms throughout.

**Fourth round** (2026-11-25, the play test of that build in Primm's casino: "shadows are
extremely dark, they basically become just pure black blobs"; a video of the dark patch
under a card table going from half gone to black as the camera moved; "create a slider for
ALCS and vanilla shadow intensity so I can experiment").

*Black shadows.* Not a fault of the maps: the hall is lit by one strong lamp at a time and
its cell's ambient light is near black, so what the lamp does not reach has nothing (a
character seen against the lamp is a black figure for the same reason). Two rows were
tried for a day, **ALCS shadow strength** and **Baked shadow strength** (0..100%, 75% by
default), and taken out again in the sixth round below: most of the black was the display
sum's crushed dark values (fixed in a pull request of its own), not the shadows. What
stayed of this round: the contact shadows weigh with their lamp's map shadow (bevy_pbr
pbr_functions.wesl; they were whole whatever the fade).

*The patch under the table* is the game's own (a model's baked shadow patch, drawn
opaque); its fix is in a pull request of its own.

*A slow shadow under the casino's protectron*: his hands' (the player's clip). The marking
kept each skeleton by its list's first joint and went over one skinned shape for each. A
body cut into parts has lists that start with the same joint and differ after it: the
protectron's one mesh is four parts, two lists under each of two first joints, and only one
list of each pair was watched; the other's joints (his hands, his fingers) marked nothing
by themselves, so their shadow moved only when the watched part moved. The key is the
whole list now (`skeleton_key`). It was not the protectron's alone
(`NVF_LIGHT_SHADOW_TRACE=1` counts them): in that hall 37 of 111 lists were unwatched, in
Goodsprings 23 of 98 (bighorners, Victor, crows, people), in the Prospector Saloon 11 of
27. After: his four parts mark by themselves (19 to 229 times in 300 frames standing).
The measuring before the clip found nothing because it looked at the draws: no lamp waited
for one, and the player's own shadow stays at the feet with DLSS and without.

*The floor round the player going dark* in that hall (the player's second clip) is not the
row's: the same views give the same pixels with the row Off, with the occlusion and the eye
adaptation off, and in a build without this branch (upstream main of 2026-11-23). The
floor there is bright where the eye looks along it towards a lamp, in the lamp's colour,
and dark red-brown underfoot. Left for work of its own.

**Fifth round** (2026-11-25, the play test of a build with the display-sum fix: "shadows
sometimes bug out", "standing in specific spots makes them bug out"; three clips of the
player's own shadow in the casino turning into a floor-wide slab with straight edges, into
shards, or losing all but the head).

*Slabs and shards of shadow.* Reproduced with the player's save by running their first four
seconds (up the stairs to the slot machines) and standing: the shadow is right for a quarter
second, then a slab covers the floor for seconds [observed, 7 runs of 7]. In third person
the body itself is right and the slab is a square patch of floor round it. Off with
`NVF_LIGHT_SHADOW_KEEP_PHASES=0`, so the kept shadow phases (above, 2026-11-19): a lamp's
draw list is kept while the lamp is switched off, and a caster only that lamp's views see
is dropped from the renderer's mesh instances while it is (the renderer keeps only meshes
some view sees). Seen again it has another place in the instance buffer, while the kept
list still names the old place, which by then holds another mesh's transform: the caster
is drawn into the map somewhere else, often right in front of the lamp. In the casino about
100 casters a lamp come and go like this with each of its draws (the player looking at the
floor sees none of them); most get their old place back, which is why it showed only in
some spots, and for good once other meshes (the player's body parts, a drawn weapon) had
taken a freed place. The resync of a kept list now drops and queues again every entry whose
place is no longer the mesh's own (engine/vendor/README.md, "Kept shadow phases"; event
`stale entries requeued`, 9,295 in ten seconds of that hall; unit test
`a_kept_shadow_phase_back_in_view_is_resynced_and_never_bins_twice`). With the check
eight runs of eight draw the player's shadow alone; with `NVF_LIGHT_SHADOW_STALE_CHECK=0`
the slab is back [observed].

*A check of the whole of it* the same day (three readings of the code by fresh eyes, a run
under the Vulkan validation layer, a minute of the casino with the view, the weapon, the
mode and the cell switched). The layer reports nothing of the shadows' (two threading
notices are Reflex's latency marker against the present). Found by reading, and changed:
- A kept list kept the entry of a mesh removed while its lamp was off, when that mesh got
  another model or material in a later frame (the change went unseen: the mesh was no
  longer among the lamp's pipelines). A removed mesh now leaves the kept list too.
- The lamps of the far texture had neither kept lists nor the caster grid (both went by
  the near texture's slots alone): each of their draws queued every caster anew and
  searched every caster in the world. In the casino a minute's run made 264 lists anew in
  its first ten seconds, was 636, and the grid served 1.7 lamps a frame, was 0.65.
- The contact shadow was faded apart from the map's shadow, so the two multiplied: at 75%
  a place both shade kept 6% of the lamp's light, the rest of the shadow 25% (a darker
  core where things touch). They now share one fade. And only the lamps that cast take
  contact shadows: on a lamp past the cap they stayed at full strength whatever the row.
- A lamp shown though no camera ever saw it (pre-drawn behind the player) had no frusta
  yet, and the default ones cull every caster [from the code; four views of the casino
  right after a load and 40 s later were the same, so not seen in that run].
- The redraws for casters not ready: a lamp's reports (one a face) are summed, a higher
  count than any before starts the steps over (any other count did, on every report), and
  a lamp quiet for 2,048 frames starts over (the steps were never given back).
- The near texture keeps the muzzle flashes' room from the first lamp on: grown for a
  flash's slot it was a new texture, and every lamp in it faded in again at the first shot.
Found and left then: see the sixth round.

**Sixth round** (2026-11-25, the same day: with the display sum's dark values shown as
they are, "the shadows and AO are not dark enough"; the two strength rows go; what the
fifth round left).

*Strengths.* The two rows are gone. The lamps' shadows weigh 1.1 (`light_shadows::SHADOW_STRENGTH`): the renderer takes
`1 - weight x (1 - shadow)` of a lamp's light, and `HoldLightShadowMaps::fade` carries
fade x weight in the light's flags (now `255 - weight x 200`, up to 1.27; bevy_pbr
`nvf_fade_bits`, shadows.wesl `nvf_shadow_weight`). Where a lamp's map shades in full, the
display sum gets that lamp's light times -0.1: the place loses a tenth of what the lamp
would have given, off the cell's ambient light and the other lamps there, and the sum is
held to 0. Nothing changes where the lamp does not reach (its part of the sum is 0 there).
Bevy's own sum, the highlights in a world view, takes `max(shadow, 0)`. The weight was
set by eye with the display sum's dark values shown by the sRGB curve (a pull request of
its own, with the occlusion's strength); on the 2.2 power the shadows read darker still.

*What the fifth round left.*
- The near texture's places: a lamp that has to change cube texture now does so out of
  sight too, shown to Bevy's culling for that frame so its new cubes are drawn at once
  (`reslot_unseen`). Before, it kept its place until it came into sight, the near texture
  stayed full of lamps behind the camera and those ahead stayed at 512.
- A texture that grows: its lamps in sight draw every face again in that frame, whatever
  the frame's draws, and keep their shadows (`regrown`, `keeps_shadow`); before, every lamp
  of the texture lost its shadow and faded in again. Lamps out of sight start over as in
  any new texture (they are drawn and fade in when seen, as a lamp never drawn). The cost
  is one frame with up to all the texture's lamps drawn in full.
- The pre-draw under a loading screen showed the lamps out of view only while no lamp in
  view had anything to draw; one whose faces are marked every frame (a fan or a flag in
  its light) kept them waiting for the whole pre-draw. Only a lamp in view never drawn
  keeps them waiting now.
- The cube numbers: a test holds the caps (lights and transients) under bevy_pbr's
  `NVF_FAR_CUBE`.
- The translated notes of the row are within 200 characters.
Left as they are: about 100 casters a lamp lose and regain their place in the instance
buffer with each of its draws (10 to 30 entries queued again a frame in the casino). They
draw right since the fifth round; sparing the work means keeping meshes no camera sees in
the renderer's instance table, a change to Bevy's extraction not made here.

Measured 2026-11-24 (RTX 5090, 4K DLSS Quality, frame generation 2x, the player's INI): Doc
Mitchell's opening, 25 lights casting, 1.7 lights switched on and 3.4 faces composited a
frame, `prepare_lights` 0.04 ms, the game's systems 0.04 ms, shadow views 0.1-0.2 ms of GPU
time, no light waited a frame; Novac at night walking, 14 lights, 1-2 lights and 1.5-6.2
faces a frame, none waited. The eight `light-shadows*` smoke scenarios pass. Not done: the
cube texture can still be made anew unseen by another shadowed point light taking a slot
past the lamps' room (a menu scene's), which costs the lamps their maps until they redraw.

**Before.** The row was called Lamp shadows until
2026-11-19 (renamed, [design, #3074]); the INI key `light-shadows`, the switch
`--light-shadows`, the log lines starting `light shadows:` and the smoke scenarios'
`light-shadows-*` names keep the old name, so saved settings and the scenarios still match.
"Lamp shadows" below, in older notes, is the same feature.

[design] An addition of NVForever's own. The original's placed lights cast no shadows, and
no LIGH of the masters sets the Spot Shadow flag [observed:
docs/handoff/esm-data-audit-2026-10-25.md, item 6], so lamps light the rooms behind their
walls. The row ALC shadows (Graphics › Shadows; `--light-shadows[=performance|quality|off]`)
offers Off, Performance or Quality, as the Sun shadows row does
(`engine/nvforever/src/light_shadows.rs`). It is built as the sun's shadows are ([design, #3074],
2026-11-18: "look how sun shadows are calculated and do the same", "use the same
performance optimizations also for the lamp shadows").

**With the row Off.** Nothing of the ALC shadows runs and the renderer's paths are
upstream's [design, #3077 review: "gate what you can behind the row"]. The game's systems
(`shadow_lights`, `classify_casters`, `watch_map_textures`, `skip_fixtures`, the caster grid,
soft edges and contact shadows) have the run condition `light_shadows::live`: they run while
the row is on and for 3 frames after it is switched off (or from the start with it off),
which is enough to switch every lamp's maps off, give back the soft edges, the contact
shadows, the casters' lists and the point lights' cube side, and hand the renderer an
inactive hold. The vendored bevy_pbr takes an inactive `HoldLightShadowMaps` (`active`
false) as no hold, so every patched point and spot light path is upstream's: no slots, no
held faces, no static cache, no kept phases, no cached shadow views (the sun's cascades'
included), the hidden lights' views as upstream, no counts or timers
(engine/vendor/README.md). Switched on again, every shape not yet marked dynamic is looked
at once (shapes spawned while it was off), and every light's shadow views are made again.
What stays in every build: the shaders' slot and fade bits (0 with the row Off: upstream's
layer, full strength) and Bevy's `experimental_pbr_pcss` feature, a compile-time switch
whose PCSS path a light takes only with soft shadows, which only the ALC shadows give
(docs/design/dependencies.md).

**First version (dropped the same day).** It used a budget of the 2, 4 or 8 lights that
scored highest on brightness × (range / distance)². A playtest [design, #3074]: "The shadows pop in
only when you are very close". Among the saloon's 24 lights, a lamp across the room cast
nothing until it beat the nearest ones. It was also costly: each shadowed point light cost
about 1 ms of frame time, most of it on the CPU. Each of a point light's six cube faces is
a view of its own, and each such view runs Bevy's whole Core3d schedule (culling,
preprocessing, indirect parameters, its pass), unlike the sun's cascades, which ride inside
the camera's. Holding the maps with the views kept alive did not fix that: Bevy still
culls, extracts and queues every face view every frame.

**How it works now.**

- **Which lights: by distance, as the sun's shadows.**
  - Every drawn placed light casts if its light reaches within the sun's shadow distance of
    the camera (`SkyState::shadow_distance` × `shadow_reach`: the Shadow distance and
    Longer sun shadows rows; 0% casts none). Nearest first, measured to the edge of each
    light's range.
  - At most 32 lights on Quality and 16 on Performance, a cap for the Strip at night.
  - Lights are not chosen by view. A light behind the camera keeps casting, so turning
    around draws nothing anew.
  - A light already casting keeps its place against one up to 1.25 times nearer (`KEEP`).
- **Held maps, view-less** (a patched bevy_pbr, engine/vendor/README.md "held light shadow
  maps").
  - A point light's cube, or a spot light's map, is drawn when the light starts casting,
    then kept.
  - Between draws the light's maps are switched off for Bevy (`shadow_maps_enabled` false:
    no views, culling or queueing). The patched renderer still sets its shadow flag and
    samples its cube once all six faces hold what that light drew into this texture, or
    its spot map once its layer does.
  - Each light keeps a slot while it casts: a point light a cube of the cube texture, a
    spot light a layer of the sun's shadow texture past the sun's cascades. Its GPU flags
    carry the cube or layer (bits 16..23), so other lights coming and going do not move its
    maps. Bevy put a spot light's map at its index among the shadowed spot lights, which
    moved whenever another one started or stopped casting.
  - Each texture keeps room for its slots, grown in fours while a mode stays on. The spot
    lights' room also counts the other shadowed spot lights (the flashlight, which takes a
    free slot of its own), so switching the flashlight on or off does not make a new
    texture.
  - A texture made anew holds no light's maps: the cube texture when its room grows or the
    Shadow resolution row changes, the sun's when its side, the spot room or the sun's
    cascades change (sun shadows on or off). Its lights then draw again as if new (first,
    fading in); those fading out are dropped. The renderer never samples a layer another
    light, the sun's cascades or another texture drew last.
- **Redrawn where something changed.**
  - A face is marked when, since it was drawn, a shadow-casting shape with its bounds in
    that face moved, appeared, or was shown or hidden. A skeleton (actors, the player's
    body) whose joints' box moved also marks the faces it reaches. The light itself marks
    all its faces once it is more than 8 units (about 11 cm) from where its maps were last
    marked or drawn in full (`LIGHT_MOVE_EPS`) [engineering choice, 2026-11-18: a light
    an animation or script sways by a few units is not drawn again each frame].
  - Shapes and skeletons count as moved once they are 4 units (about 6 cm) from where
    they last marked (`MOVE_EPS`). Small moves add up until they reach it.
  - Something removed (a taken item) marks the lights around the camera in full.
  - Every light is redrawn in full every 600 frames on Quality and 1,200 on Performance
    anyway, each at its own time.
- **Staggered, as the sun's cascades.**
  - A light with marked faces is switched on to draw them on every frame on Quality (every
    2nd on Performance) while its light reaches within 1,500 units of the camera (`NEAR`).
    A farther one draws every 2nd frame on Quality and every 4th on Performance. Each
    light takes its turn on frames of its own.
  - At most 3 lights (point or spot) are switched on in a frame on Quality, 1 on
    Performance. Lights that never drew go first, then the nearest; the rest wait.
  - Of a light switched on, only the marked faces are drawn; the others keep their maps.
    A light that has never drawn casts nothing until it has.
  - Only a light the camera saw the frame before is switched on; an unseen one keeps its
    marks. A transient light (below) instead counts as seen when its light reaches into
    this frame's view.
  - Spot lights (few among the placed ones) are held view-less as the point lights are
    (2026-11-18; before, they kept their maps switched on, their views alive, while they
    cast). Switched on only on the frames their map is drawn, they count against the same
    budget.
- **Fading** (`FADE_SECS`, 0.3 s of real time [engineering choice: no popping when a light
  comes into range, leaves it or is swapped at the cap; short enough that a light coming
  into range shows its shadow at once]).
  - A light's shadow fades in once its maps were first drawn.
  - A light no longer chosen is not dropped at once: it stays held (slot kept, nothing
    drawn, nothing marked) while its shadow fades out, then is dropped. While fading out it
    does not count against the caps (`Tuning::lights`, `Tuning::transients`). Chosen again
    meanwhile, it fades back in from where it was and is redrawn in full on its turn.
  - A light despawned is dropped at once. Transient lights do not fade: a muzzle flash
    lasts about a tenth of a second and shows its shadow in full at once.
  - The renderer gets each lamp's fade times the lamps' weight (`HoldLightShadowMaps::fade`,
    `SHADOW_STRENGTH`; "Sixth round" above) in bits 24..31 of the light's flags
    (`255 - round(weight × 200)`, so 0 is a weight of 1 for every other light) and takes
    `1 - weight × (1 - shadow)` in shadows.wesl's `nvf_weigh_shadow`.
- **Other lights** ([design, #3074]: all light sources cast, muzzle flashes and
  explosions first; `ShadowSource` on the light, `SourceKind`).
  - Grass fires: the light over about one burning patch in six (grass_fire_fx.rs). Still
    while it burns; its intensity flickers.
  - Lights a script attached to a node (JIP AttachLight, nif_mirror.rs). They move with
    their node.
  - These two rank and count with the placed lights, under the same rules and cap.
  - Muzzle flashes (shot_fx.rs, a light at the muzzle for the flash's time, about a tenth
    of a second, moving with the gun) and explosions (shot_fx.rs, 32 units above the blast
    point, fading over 0.6 s) are **transient**, chosen apart from the placed lights:
    - nearest first, measured to the edge of each light's range, only while that edge lies
      within 3,000 units (about 43 m; `TRANSIENT_REACH`) of the camera and within the sun's
      shadow distance [engineering choice: the firefight around the player; a farther flash
      is small on screen and gone in a tenth of a second];
    - only while their light reaches into the camera's view this frame (the camera's
      frustum, far plane left out, as Bevy culls lights). A new light was not seen the
      frame before, so the placed lights' rule would hold it back a frame; a light in view
      is one Bevy's culling finds visible, so its maps are drawn;
    - at most 2 at once on Quality and 1 on Performance (`Tuning::transients`), not counted
      against the placed lights' cap;
    - with their own budget of point lights switched on a frame, 2 on Quality and 1 on
      Performance (`Tuning::transient_draws`), besides the placed lights' 3 and 1, so a
      firefight does not starve the lamps nor the lamps it [engineering choice].
  - A transient light never drew, so it goes first and draws its whole cube in the frame
    it appears, **once**: it is held for the rest of its short life, whatever moves
    [observed 2026-11-18, Trudy's gunfight in the Prospector Saloon at 22:00: redrawn
    when the gun's recoil carried it past `LIGHT_MOVE_EPS`, a flash was drawn 3.5 times
    on average (4 flashes, 14 switched on); drawn once, 4 and 4].
  - Cost [observed 2026-11-18, the same gunfight, RTX 5090, 1280 × 720, `--log-frame-times
    3`]: 3-second means 3.12-3.29 ms on Quality against 3.04-3.21 ms off; the slowest
    frame of a window with a flash 8.8-10.7 ms against 4.8-7.6 ms off. A flash costs one
    frame a few ms longer, as a lamp switched on does (about 1.4 ms of processor time over
    every thread, against 0.7 ms a frame for a light kept on; `NVF_LIGHT_SHADOW_REFRESH`).
  - The effects' own shapes (flash, explosion and fire models, their particles) are
    `NotShadowCaster`, so a light is not blacked out by the flames around it. The gun a
    muzzle flash sits at is a small shape it sits against, skipped as a fixture.
  - Not these: the flashlight casts on its own (flashlight.rs); the lights of the menu and
    preview scenes (the character creator, the Vigor Tester, the poker table) and the
    sky's do not.
- **What casts.**
  - A casting light takes the sun's layers (0, `sky::SHADOW_LAYER`, `sky::BODY_LAYER`), so
    the player's body casts in both views. The first-person arms do not.
  - Everything `NotShadowCaster` stays out, as for the sun.
  - **Light fixtures cast no shadows from their own lights** (`LightFixture`, `skip_fixtures`; the sun's
    they still cast). A playtest (2026-11-18, [design, #3074]): a hanging box lamp in the Goodsprings
    General Store threw its housing's shadow, larger than the lamp, onto the ceiling. The
    game hangs its lights under their lamps, not in them [observed 2026-11-18,
    `fnvtool world refs`: the LIGH `BasementLightKeyWarm` sits 86 units under the box lamp
    `OffRmLightHanging01`; over the masters' interiors, 3,909 of the 7,543 LIGH references
    without a model of their own have a lamp model's reference within 160 units, half of
    them within 89].
    - A shape is a fixture when its model's file name holds `light`, `lamp`, `lantern`,
      `chandelier`, `sconce`, `bulb` or `candle` (`offrmlight01`, `lightceilingvaultr02on`,
      `shackhanginglight02`, `neoclassicalsconce`, …), or when it is a LIGH's own model.
      Marked when the cell is drawn (world.rs `spawn_model`); every casting light skips it.
    - A small shape (a bounding sphere up to 128 units) that a light sits in or within 16
      units of is skipped by that light too: a bulb or shade of another name.
    - Taken out of the shapes Bevy culled for the maps, after its culling, only on the
      frames a light draws.
    - Test aid `NVF_LIGHT_SHADOW_FIXTURES=1` logs, once a light, the shapes it skips and
      the small ones near it it keeps, by reference, size and distance.
  - A near plane of 8 units (`NEAR_Z`, as the flashlight's): what lies nearer the light
    casts nothing from it. It was 16 to keep a light's own bulb or shade out of its map,
    which let a wall within 16 units through; since the fixtures are skipped, 8 and 4
    drew the same pictures as 16 in the General Store and the Prospector Saloon
    [observed 2026-11-18, `NVF_LIGHT_SHADOW_NEAR`, three views each].
- **Soft edges and contact shadows** (Quality only; light_shadow_detail.rs).
  - Soft edges: the casting lights take Bevy's percentage-closer soft shadows (the
    `experimental_pbr_pcss` feature, engine/Cargo.toml) with a light size of 24 units
    (explosions 48). The edge widens with the distance between a shape and its shadow:
    a hanging wire's shadow is sharp where the wire touches the ceiling and soft away
    from it [observed 2026-11-18, General Store pictures at sizes 8, 24, 32: at 8 the edges
    looked as hard as without]. `NVF_LIGHT_SHADOW_SOFT=<size>` (0: none) for comparing.
  - Contact shadows: the world camera takes Bevy's `ContactShadows` (8 steps, thickness
    4, length 24 units) and so a depth prepass, and the placed lights, grass fires and
    attached lights within the shadow distance take `contact_shadows_enabled`: small
    shadows where things touch, and the only ones of the lights past the cap. Only while a
    temporal method (TAA, FSR or DLSS) is on:
    - Without one the march's per-pixel noise speckled walls and ceilings in a regular dot
      pattern, thinner settings worse; with TAA the picture was clean [observed
      2026-11-18, General Store pictures].
    - With FSR or DLSS Bevy's march threw hard-edged false shadows, a dark block over a
      door, streaks down a wall, that changed as the camera moved [observed 2026-11-18:
      playtest videos with DLSS; reproduced walking along the Prospector Saloon's porch
      at 14:00 with DLSS and with FSR; gone with `NVF_LIGHT_SHADOW_CONTACT=0`, not with
      `NVF_LIGHT_SHADOW_SOFT=0`]. The cause [documented: bevy_camera
      `MainPassResolutionOverride`, "In shaders, use `View::main_pass_viewport`"; read in
      bevy_pbr raymarch.wesl]: an upscaled camera draws its main passes into the top-left
      render-size part of full-size textures, and the march mapped its rays over the whole
      depth texture. The vendored march now reads the main pass's part only
      (raymarch.wesl `depth_uv_scale`, engine/vendor/README.md): the same walk with DLSS
      Quality, DLSS Performance and FSR was clean, with contact shadows on 6 lights, and
      standing on the porch with DLSS they darken the beams', frames' and door's edges
      [observed 2026-11-18].
    `NVF_LIGHT_SHADOW_CONTACT=0` turns them off, `NVF_LIGHT_SHADOW_CONTACT_MARCH=<steps>,
    <thickness>,<length>` tries other settings.
- **The lighting.** Nothing in the lighting shaders changes. The display-value sum
  (`lights/display_shader.rs`) already multiplies each point and spot light's diffuse term
  by Bevy's `shadow`. The cube lookup takes the light's slot (shadow_sampling.wesl
  `nvf_point_shadow_cube`), the spot lookup its layer (`nvf_spot_shadow_layer`), and the
  fade scales the shadow in shadows.wesl.
- **Maps.**
  - A point light's cube has 512² faces with Shadow resolution 2048, and 1024² with 4096
    (`PointLightShadowMap`): 6 or 24 MiB a light. The cube texture's room grows in fours
    while a mode stays on.
  - A spot light draws one layer of the sun's shadow texture, at the sun's side: each slot
    of the spot room (in fours) costs a layer of 2048² or 4096² (16 or 64 MiB at 32 bits
    a texel), the price of keeping the room from changing as spot lights come and go.
  - Bias: Bevy's defaults. `NVF_LIGHT_SHADOW_BIAS=<depth>,<normal>` and
    `NVF_LIGHT_SHADOW_NEAR=<units>` override them for a run.
  - `NVF_PLAY_TEST_LIGHT_SHADOW_HOLD=0` draws every map every frame, for comparing.

**Cost** [observed 2026-11-18: RTX 5090, 1280 × 720, the Prospector Saloon at 22:00, 24
placed lights, release builds, `--log-frame-times 6`; windows after loading]:

| Mode | Free camera standing still | `--play`, walking back and forth |
|---|---|---|
| Off | 2.7-2.9 ms | 3.0-3.25 ms |
| Performance (16 lights) | 3.0-3.3 ms | 3.3-3.9 ms |
| Quality (24 lights) | 2.75-2.9 ms | 3.3-3.7 ms |
| Quality, nothing held (test aid) | 14-15 ms | |

- Before the 4-unit rule, the actors' idle skeletons and the head parts that follow them
  (eyes, mouth, hat) marked about 116 times a frame. Quality then switched on its full 3
  lights every frame: 5.5 ms standing still.
- Standing still now, about one light in 50 frames is switched on (its full redraw due).
  The slowest frames were 8.3-8.7 ms against 3.3-6.8 ms with the lights off, so a light
  switched on costs a few ms in its frame.
- Walking, 0.5-0.9 lights a frame are switched on.

**Cost at the menus' top settings in 4K** [observed 2026-11-18: RTX 5090, 3840 × 2160
borderless, every graphics row at its top (ambient occlusion, TAA, sun shafts, water
reflections, shadow resolution 4096 so 1024² cubes, temporal shadow filter, all draw
distances 100%), upscaler and frame generation off; the Prospector Saloon at 22:00, 10 s
standing then Trudy's shotgun gunfight (7 flashes); `--log-frame-times 3`, 3-second means
after loading]:

| ALC shadows | Standing (3 windows) | Gunfight (6 windows) | Slowest frame in the gunfight |
|---|---|---|---|
| Off | 3.21-3.28 ms | 3.23-3.29 ms | 4.2-7.0 ms |
| Performance | 3.21-3.26 ms | 3.31-3.40 ms | 5.6-15.5 ms |
| Quality (soft edges, contact shadows on 24 lights) | 3.23-3.31 ms | 3.33-3.39 ms | 5.3-19.3 ms |

- 24 lamps cast, 5 flashes cast with one draw each. The mean costs about 0.1 ms; the
  slowest frames are those of a flash or a lamp switched on.

**Log.**
- `light shadows: <n> cast (<p> point, <s> spot lights) within <reach> units, <cap> at most;
  cubes <side>, room for <c>; spot maps room for <s>; transients (muzzle flashes,
  explosions) within <r> units, <t> at most` is written whenever these change. Its counts
  leave the transient lights out.
- Every 10 s it writes `light shadows: <l> point lights switched on and <f> faces drawn a
  frame … marked a frame by shapes <a>, skeletons <b>, removals <c>, lights <d>; shapes such
  as <names>; transients <n> cast, <s> switched on, <t> faces drawn`. The first counts are
  the placed lights' (with grass fires and attached lights; the lights switched on count
  the point lights only, the faces drawn include the spot lights' maps, and the lights
  casting leave out those fading out); the transients' are totals over the 10 s.

**Performance pass (2026-11-18).** The request [design, #3074]: "The shadows update at a low rate and cause
stuttering in object dense areas. Work on optimizations - the quality of the shadow can be
reduced with distance." The main session's runs in Primm at 22:00 (release, 1280 × 720,
`--play`, walking): Off 5.7-7.2 ms 2-s means; Quality windows of 10-84 ms with frames of
99-181 ms, in exactly the seconds a scripted mass toggle disabled and re-enabled about 490
references (751 fading back in), while the 10-s summary stayed low (0.11-0.9 lights
switched on a frame). A `profile` build put `prepare_lights` at up to 16 ms,
`check_point_light_mesh_visibility` 7.6, `specialize_shadows` 3.8, `queue_shadows` 3.1,
`shadow_lights` 4.1 and `skip_fixtures` 3.4 ms in those frames [observed 2026-11-18, the
main session]. What was built, each with a test aid that turns it off for comparing; **none
of it is measured in game yet**:

- **Hidden lights drop their views** (vendored bevy_pbr, the open bevyengine/bevy#26480,
  fix of #26059): a hidden shadowed light's views are despawned with it. Before, a lamp
  hidden by a turn of the camera in the frame after it drew left its six views running
  the Core3d schedule until it was seen again. No aid (a fix); the count below shows how
  often it happens.
- **Measuring lines** (no aid; engine/vendor/README.md "light view counts"): every 10 s
  beside the summary, `light shadows: renderer over <n> frames: light views a frame <a>
  alive, <r> run, <h> despawned with their light hidden; views spawned …, views despawned …,
  views remade …, phases new …, phases dropped …, new point textures …, new directional
  textures …, texture views made …; ms a frame mean/max: extract_lights …, prepare_lights …,
  uniform writer …, counts and slots …, textures …, gpu lights …, point views …, spot views
  …, cameras …, sweep …; game systems ms a frame mean/max: shadow_lights …, skip_fixtures …,
  soften …, contact …`. The `prepare_lights` stretches add up to its total, so the 16 ms can
  be placed.
- **Lamps changed only on change** (light_shadow_detail.rs `soften`, light_shadows.rs
  `skip_fixtures`): `soften` took every lamp's light through `Mut::into_inner`, which marks
  it changed: every lamp in the world was marked changed every frame, so Bevy made its cube
  frusta again and extracted it again each frame. `skip_fixtures` touched the lists of
  casting lights not drawing, with the same effect. Both now change a light only when
  something changes (`apply_soft`), and only drawing lights are filtered. No aid.
- **Cheaper marking per moved shape** (`shadow_lights`): a reference toggled or faded in
  town changes thousands of shapes a frame. Nothing is marked without a lamp; past 32
  shapes a frame the lamps are found through a grid (`LampGrid`); a lamp drawing in full
  anyway (fresh, or every face marked) and faces already marked are not tested again; no
  transform decomposition per shape; the chosen lights are hash sets. No aid (same marks).
- **Cached shadow views** (vendored bevy_pbr `prepare_lights`, engine/vendor/README.md):
  the shadow textures' array views and the per-layer views and attachments are kept while
  the texture is the same, instead of `create_view` every frame (and six for every drawing
  lamp every frame) [hypothesis: fewer device calls give `prepare_lights` less to wait on
  while materials are made and dropped on other render threads]. Aid
  `NVF_SHADOW_VIEW_CACHE=0`.
- **Steps of detail by distance and the small-caster cut-off** (`STEPS`, `cut_off`): each
  casting light has a step by the camera's distance to its light's edge (near within 1,500
  units, mid, far past 3,000; 10 % hysteresis). A caster whose bounding sphere covers fewer
  than 1, 2 or 4 texels across in the light's map at its distance (by step;
  `CUTOFF_TEXELS`, `map_scale`: the cube's side, or the sun texture's side over the tangent
  of a spot light's cone) is taken out of the light's lists (not queued, specialized or
  drawn) and its moves mark nothing on that light (under the cut-off where it is and where
  it last marked). A light stepping finer after casters were cut draws its maps again in
  full. [engineering choice: the thresholds.] Aid `NVF_LIGHT_SHADOW_CUTOFF=0`.
- **A grid of shadow casters** (light_shadow_casters.rs; vendored bevy_light,
  engine/vendor/README.md "shadow caster candidates"): Bevy tested every shadow caster in
  the world against each drawing light. Every mesh is now filed by its bounds' centre in
  1,024-unit cells (larger than 512 units, unbounded or not culled: listed for every light),
  kept up to date from what moved or went; each drawing lamp is tested only against the
  meshes of the cells its sphere (grown by 512) overlaps. The flashlight and the menu
  scenes' lights keep Bevy's search. Aid `NVF_LIGHT_SHADOW_CASTER_GRID=0`.
- **Fade by distance** (`distance_fade`, `DISTANCE_FADE` 20 %): a light's shadow weakens over
  the last fifth of the reach to none at it, times its time fade, instead of ending there.
  Transients keep full strength. Aid `NVF_LIGHT_SHADOW_DISTANCE_FADE=0`.
- **Faces out of view held** (`faces_in_view`, `FACE_VIEW_MARGIN` 64 units): of a point
  light on its turn, only the marked faces whose pyramid (the light and its square at the
  range) can reach the camera's frustum (side and near planes, moved out by the margin) are
  drawn; the others stay marked and held until they come into view, and a light with such
  faces coming into view goes before the others. Not for a light that never drew (the
  renderer needs all six faces), transients or spot lights. Mirrored water and other
  cameras could show a held face's stale shadow [hypothesis, not looked at]. Aid
  `NVF_LIGHT_SHADOW_FACE_CULL=0`.
- A third 10-s line gives what these did: `light shadows: performance pass over <n> frames:
  lights at steps near/mid/far …; small-caster cut-off (on) casters cut … a frame, marks
  skipped …, redraws for a finer step …; caster grid (on) … lamps served a frame, …
  candidates each, … meshes filed (… for every light); distance fade (on) … lights fading a
  frame; face culling (on) … marked faces held out of view a frame` (`OFF` for an aid set).

Not built in this pass (designs): **far lights redrawn less** (a third stagger band past
3,000 units, and marks from skeletons and shapes under 48 units on far lights batched into
one redraw a second); **a cost budget** instead of a count (a switch-on costing about six
face-frames on top of its faces, from the measured 1.4 ms switch-on against 0.7 ms a frame
kept on; at most one never-drawn light a frame; ranked by projected size × fade, waiting
lights aging up); **resolution tiers** (a second, half-size cube array for far lights, the
tier in the flags beside the slot, a second binding in `mesh_view_bindings` and
`shadow_sampling.wesl`, hysteresis on the tier, a full redraw on a change: it touches the
vendored renderer's bindings and shaders, left for after these are measured).

**Static shadow cache (2026-11-18).** The request [design, #3074]: "Since most of the lights in New Vegas are
static, could we not bake the shadows at least partially?" Approved: a static cache with the
dynamic casters drawn on top, and the lamps drawn in full under a loading screen. **Not
measured in game yet.**

- **Static and dynamic casters** (light_shadows.rs `ShadowDynamic`, `classify_casters`,
  `shape_change`). Dynamic from the start: skinned shapes, every shape under an actor's or
  creature's rig (`ActorRig`, `PlayerRig`: weapons, hats, eyes follow the skeleton) and the
  player's body (its layers, `sky::SHADOW_LAYER`, `sky::BODY_LAYER`). Every other shape is
  static until it moves after it marked the lamps (clutter pushed by physics, which
  nvf-play's `moved.rs` writes into the drawn transform; a door or activator turning; what a
  script moves): it becomes dynamic then, for good, and the caches of the faces it was and
  is in are drawn again without it [engineering choice: one rule for every way a thing can
  move, instead of a marker at each place that moves things]. Shown, hidden, new or gone, a
  static shape stays static (a cell loading, Primm's scripted mass enable and disable).
- **Bookkeeping per face.** `dirty` as before (the sampled map is out of date) and
  `static_dirty` within it (the cache is): a static shape's change, the light's first draw,
  a new texture, a refresh, a finer step, a removal of static shapes and a light chosen
  again mark both; a dynamic caster's move, show or hide, or a removal of dynamic shapes
  only, marks `dirty`. A dynamic caster marks where it was and where it is (one sphere round
  both), so the face it left is composited too.
- **Drawing** (`face_modes`, `keeps_caster`, `put_back`). Of a lamp's faces drawn this
  frame, those with `static_dirty` are drawn from the static casters into the cache and
  copied into the sampled cube; the others are composited: the cache's face copied into
  the sampled face and the dynamic casters drawn over it with the depth loaded, not cleared
  (vendored bevy_pbr, engine/vendor/README.md "static shadow cache"). A face drawn into the
  cache whose caster list held dynamic casters lacks them until its composite, which goes
  next, before other lights' turns (`pending`); a composite with no dynamic caster left is
  the cache copied back once. Transients, spot lights (few among the placed ones, one map:
  left on the old path, simpler) and lights that move (their cache would be drawn again each
  time) take the plain path: every caster into the sampled map, as before. Every rule
  before stays: slots, held maps, fades, the fixtures skipped (in both passes), the steps,
  cut-off, grid and face culling, DLSS and contact shadows.
- **Memory**: the cache is a second cube texture of the same slots and size (Depth32Float):
  at 512 × 512 6 MB a lamp, 151 MB for 24 lamps' room; at 1,024 (Shadow resolution 4096)
  25 MB a lamp. The start line says `static shadow cache on (<n> MB)`.
- **Log**: the performance line ends `static cache (on) <s> static redraws (faces) a frame,
  <c> dynamic recomposites a frame, <k> layer copies a frame, <p> faces put back after a
  static redraw, <d> shapes became dynamic`; the renderer line's events add `static cache
  draws`, `static cache composites` and `static cache misses` (a composite whose cache layer
  held no map of its light: drawn with the dynamic casters only; expected 0).
- **Aid** `NVF_LIGHT_SHADOW_STATIC_CACHE=0`: no cache, every face drawn with every caster,
  as before.
- **Pre-drawn under the loading screen** (light_shadows.rs `predrawing`, `predraw_busy`;
  nvf-play `Loading::lamp_shadows_busy`). While a loading screen is up (a travel, a cover,
  the start) and the renderer has its cells around the camera (the destination's lights are
  spawned with its cells; the camera is already at the destination, as the cells it waits
  for are those around it), every chosen lamp the camera sees draws all its marked faces at
  once: no per-frame budget, no turns, no face culling. Before the cells are in, the lamps
  draw as in play (each arriving cell would mark them again). The screen waits ("ALC
  shadows" in its phases line) while lamps drew in the last frame (their composites follow)
  or any chosen lamp still has marked faces, at most 3 s from the start of the pre-draw
  (`PREDRAW_CAP_SECS`) [engineering choice]. The performance line
  ends `loading-screen pre-draw (on) <n> lights`. Aid `NVF_LIGHT_SHADOW_PREDRAW=0`.
- **Out of view too (2026-11-19).** At first a lamp the camera did not see could not draw
  under the screen: Bevy culls a point light's casters only for lights a camera sees
  [observed in the code: vendored bevy_light `check_point_light_mesh_visibility`], and the
  renderer drops a hidden light. Now, once no lamp in view has maps left to draw, the chosen
  lamps out of view are shown to that culling, the nearest `PREDRAW_UNSEEN_PER_FRAME` (8) a
  frame (`shown_unseen`), through the patched bevy_light's `NvfShadowLightsShown`
  (engine/vendor/README.md "shown shadow lights"): it finds their casters as for a seen light
  and marks the light visible, so the renderer extracts it and draws its maps; it lights
  nothing on screen (it lies in no cluster). Their faces out of view go into the static cache
  as any. Within the same 3-s cap; 32 lamps take four frames. When the screen comes down:
  `light shadows: <n> lights pre-drawn under the loading screen (<f> faces, <s> into the
  static cache); <c> casting, <u> of them not drawn yet; <o> of those pre-drawn out of view`
  (`<u>` should now be 0 but for the cap). In play they then sample their held maps when
  seen, with nothing to draw.
- [hypothesis] Where it should help: a lamp near an idle or walking actor redrew every
  caster of the room on each skeleton mark; now the room is copied (a 1 MB layer copy at
  512²) and the actor alone drawn. Where it costs: a static change now draws the cache and,
  with an actor in the face, a composite after it (two switch-ons instead of one); the
  copies' bandwidth; the memory.

**Kept shadow phases (2026-11-19).** Switching a held light on cost about 1.4 ms of CPU once
(measured 2026-11-18; each of its views made, its six phases made anew, every caster in them
specialized and queued again). Bevy keeps a shadow view's phase, pipeline key and specialized
pipelines only while the view lives, and the game's held lights have none. The patched
bevy_pbr (engine/vendor/README.md "kept shadow phases") keeps them for every light the game
holds a slot for, with or without views, so a light switched on again finds its bins and
pipelines and queues only what changed since: the casters its lists gained or lost.
A kept view that missed a change (an entity in its pipelines whose mesh, material or flags
changed while it had no view) is dropped whole and the light drawn as new, as before; an
entity removed is only dropped from its pipelines. A kept phase back after frames without a
view is resynced with its light's whole visible list (the list's differences of those frames
were never queued): entities no longer visible leave their bins, visible ones not binned are
queued, and no entity is binned twice (queueing the differences alone did that, a renderer
panic in Novac at 22:00 on Quality, 2026-11-19). And a light re-extracted with the same
place, range, near plane and layers (every frame it draws: its casters' lists change) keeps
its views instead of having them made again ("kept shadow views"). Events on the renderer
line: `phases reused` (phases a light's new views found kept), `kept phases wiped`, `views
kept`, `kept phases resynced`, `entities rebinned` (none expected). Aid `NVF_LIGHT_SHADOW_KEEP_PHASES=0`; `NVF_SHADOW_VIEW_CACHE=0` makes the views anew
again too. **Not measured in game yet** [hypothesis: the switch-on's specialization and
queueing go; the view spawn and the faces' own schedule runs stay].

Also (2026-11-19): the caster grid finds the drawing lamps by their slots instead of going
over every entity with a place each frame (`caster_candidates`); an actor's skeleton is gone
over and marks once a frame, not once for each of its skinned shapes (`shadow_lights`).

**Not built yet.**
- The static cache for spot lights (a second set of layers past the cascades).
- Held faces of a drawing light still have their views batched each frame (Bevy's
  `batch_and_prepare_binned_render_phase` goes over every view with an `ExtractedView`);
  leaving them out needs the GPU preprocessing's buffers looked at.

**Smoke.** `light-shadows` (the Prospector Saloon at 22:00, `--light-shadows=quality`)
checks that lights cast and that their maps are drawn. The picture is checked by hand.

### 3.17 Authored materials (`authored-materials`; on in both looks)

A texture pack's authored PBR sets under `textures\pbr\` (#2981,
docs/spec-notes/pbr-materials.md): lit NIF shapes whose diffuse has a set take their
occlusion, roughness, highlight strength and normal from it. On in `vanilla` and `enhanced`,
like a diffuse replacer: without a pack nothing changes. Read once at the start (a change
applies at the next start); `--graphics-only no-authored-materials` or the settings switch
turns it off, and then the material path reads no file under `textures\pbr\`. Lit NIF shapes
with texture coordinates take a set; unlit, additive, multiplied and refracting shapes,
particles, hair, skin and FaceGen heads, landscape LOD and the Pip-Boy's and interface
textures never do; without an `_n` (or tangents on the drawn mesh) the game's own normal map
stays, its alpha no longer a highlight mask (docs/spec-notes/pbr-materials.md). On the
experimental height branch (stage 4) a set's `_p` (or a height-only set) is drawn as Bevy's
relief mapping, depth 0.03 unless the set's scalars say otherwise, on opaque, single-sided,
non-decal shapes with tangents. By default it marches up to eight layers and refines the
intersection with four additional steps. Each height sample reads the top mip. [design]

### 3.18 Aerial perspective (`aerial-perspective`; off in every preset)

[design, #2993] An addition of NVForever's own, an engineering choice for the enhanced look:
not a fact about the original game, never a parity item, and never to be used to explain a
difference from the original's frames. With the weather's distance fog alone every far ridge
fades to the same flat fog colour, so the Mojave's ranges stack into one band. With this row
far land takes on the sky's colours by distance instead: bluer and paler with distance under
a blue noon sky, warmer under a dusk sky, brighter toward the sun, so successive ridgelines
separate. The row is "Aerial perspective (experimental)" under Graphics > Lighting and
Effects, after Desert haze, off by default and in every preset (the `enhanced` row the issue
text would have used no longer exists since the settings menu overhaul). Code: `aerial.rs`,
fog.rs `GAME_FOG`, a patch of the vendored bevy_pbr fog uniform.

- **Technique.** After Hoffman and Preetham, "Rendering Outdoor Light Scattering in Real
  Time" (GDC 2002) [documented: cited paper]: the light reaching the eye is the surface colour
  times the extinction plus the in-scattered light times one minus the extinction, the
  extinction differing per colour channel and the in-scattered light depending on the view
  ray's direction (a phase function toward the sun). The sun's phase function is the
  Henyey-Greenstein one (Henyey and Greenstein, "Diffuse radiation in the galaxy",
  Astrophysical Journal 93, 1941) [documented: cited paper], divided by its value at the sun.
  Our own code from those descriptions; no code from any other project. The amounts are the
  weather's, not a physical atmosphere's [design]: Bevy's procedural atmosphere (after
  Hillaire, EGSR 2020) stays rejected (§2); the game's sky dome and the weather's colours stay.
- **What it changes** (constants at the top of `aerial.rs`, each an engineering choice by eye
  [design]):
  - *Per-channel share.* The weather's fog share f (`GAME_FOG`'s, with its strength) becomes
    f_c = 1 - (1 - f)^(w_c) per channel: 0 at 0, 1 at 1, rising with f. The weights come from
    the chroma of the sky colour behind far land (the drawn dome's at the horizon, below): the
    deviation d_c = (c_c - Y) / max(Y, 0.01) (Y the Rec. 709 luminance; `DARK_FLOOR`), whose
    luminance-weighted mean is 0 by construction, then w_c = 1 + k d_c with k = 1
    (`CHROMA_GAIN`) or less where that would take a weight out of 0.5..2 (`WEIGHT_LOW`,
    `WEIGHT_HIGH`). w is linear in k and 1 at k = 0, so the luminance-weighted mean stays
    exactly 1 and the hue keeps its direction, with no clamp. Below a luminance of 0.01 the
    chroma fades out continuously, so a fade to or from a near-black sky does not step
    (tested). A grey or black sky gives 1, 1, 1 (the weather's own fog). The amount of fog
    stays the weather's in that sense: the mean optical-depth multiplier is 1; a channel with
    w below 1 fogs less than f, one above more.
  - *In-scatter colour.* Instead of the one fog colour, the fog colour moved toward the sky
    colour in the view ray's direction by up to 0.8 (`DOME_MIX`). The sky colour is the drawn
    dome's at the horizon (0°, used at and below it) and at 20° up (`LOWER_SIN`), mixed by a
    smoothstep in the ray's sine between them. Those two colours are the dome's as the
    renderer draws them: sky.rs `dome_color` of each vertex's own weights on the rings nearest
    below and above, averaged per ring, then mixed in linear RGB at the point where the ray at
    that elevation crosses the mesh's edge between the two rings (each ring's mean horizontal
    distance and height), as the rasteriser interpolates the vertex colours along that edge
    (tested with rings of equal and unequal radius). The rings are read from the dome mesh in
    use (its positions and `SkyDome` weights, again only for a new dome mesh): the stock
    atmosphere.nif's weights are about 0.69 horizon + 0.35 sky-lower at 0° and 0.57 sky-lower
    + 0.53 sky-upper at 20° [documented: docs/spec-notes/world.md "Sky meshes"]; the fallback
    dome has its own (all sky-lower at 0°, sky.rs `fallback_dome`). Before a dome mesh has been
    read, the bare horizon and sky-lower colours stand in. Each is taken times the dome's sky
    level (`SkyState::sky_scale`, the factor sky.wesl draws the dome with [documented:
    engine/nvforever/src/sky.wesl]), so a far ridge meets the sky drawn behind it. All mixes:
    the colour stays within the weather's fog colour and the dome's colours as drawn, so it is
    above the display range only where the drawn sky already is (#2915: no clamp added).
  - *Sun lobe.* The finished pixel (linear light) is moved toward the same blend with the
    weather's sun colour as the in-scatter colour, by the normalised Henyey-Greenstein lobe
    ((1 - g)^2 / (1 + g^2 - 2 g cos))^1.5 with g = 0.6 (`HG_G`), at most 0.35 at the sun
    (`LOBE_MOST`), fading in as the sun rises through -2..3° as the sky's disc and glare do
    (`SkyState::light`; nothing at night), times the two pixels' luminance contrast,
    (Y_toward - Y_plain) / max(Y_toward, 1e-4) held to 0..1. A linear mix toward a brighter
    pixel only raises luminance, so the lobe never darkens what is shown, for any surface, fog
    share, channel weights and haze (tested over a grid through the whole blend and decode); a
    weather with a black sun, or one no brighter than its sky, gets none. The lobe follows the
    real sun: below the horizon sky.rs `aim_sun` turns the directional light to the moon's
    side while the fade still runs, so the CPU packs which side the light points to (the sign
    of the lobe's strength, from the light's own clock) and the shader turns the light's
    direction back toward the sun; the pixel is continuous through 0° (tested).
  - *Weight.* All of it scales with the desert haze's weight (§3.16, `haze::weight`, reused):
    full under a clear weather, none as the weather's own fog gets short or dense, so
    NVWastelandHazy and HVDustStorm (weight 0: `haze::weight` of their fog distances, which
    are [observed: FalloutNV.esm, fnvtool esm decode, 2026-11-17]; the picture runs logged
    "weight 0.00, none" for both [observed: NVForever run, 2026-11-18]) are exactly as with
    the row off: the uniform's vectors are zero there.
- **Where it sits.** Per fragment in the world shaders' fog path, no pass: everything the
  weather's fog covers, under every anti-aliasing and upscaler. `nvf_aerial` (aerial.rs,
  written before Bevy's `apply_fog` in `pbr_functions.wesl`) wraps the desert haze's call of
  the atmospheric fog: it composes with the haze's replacement of Bevy's call instead of
  replacing that call again, and its edit is made after the haze's, only once those are in
  place. It works out the in-scatter colour and the lobe's weight for the fragment's view ray
  and hands them, with the three weights and the sun colour, to `GAME_FOG` in a copy of the
  fog uniform (`nvf_params[0]` the colour with alpha -2, which the CPU never writes,
  `nvf_params[1].rgb` the weights, `nvf_params[2]` the sun colour and the lobe's weight).
  `GAME_FOG`'s first outdoor branch, taken only on that -2, blends **once**, per channel, in
  the values the fog is blended in outdoors (the pow(1/2.2) encoding: the main view is HDR,
  but the outdoor fog blend is in that encoding [documented: engine/nvforever/src/fog.rs], and
  the colours are encoded on the CPU the same way; `dome_color` mixes in the sRGB curve, a
  small difference), then applies the lobe in linear light. Interiors (the linear blend) are
  untouched, and the CPU writes zeros indoors.
- **With Desert haze also on**: the haze's share and colour stay as they are; the aerial terms
  apply to the weather's fog part: each channel's total share 1 - (1 - f_c)(1 - h), the
  colours weighted as the haze seen through the aerial fog (as in §3.16), the lobe's blend
  with the same haze. Tested: alone or together every channel's total is within 0..1, the
  haze never lowers a share, and the total is at least the haze's. Together, the near and
  middle distance take the haze's warm dust and the far ranges the sky's colours behind them,
  the haze's share over them.
- **Carried** in three vec4s appended to Bevy's fog uniform (`DistanceFog::nvf_params`,
  `GpuFog`, the shader's `Fog`; a patch of engine/vendor/bevy_pbr, listed in its README): one
  uniform grows by 48 bytes, the view bind group keeps its bindings. [0] the sun colour
  (display values, blended like the air colour, so nothing is converted per fragment) and the
  lobe's strength, negative while the directional light points away from the sun; [1] the
  in-scatter colour at the horizon and the red weight; [2] the one at 20° and the blue
  weight; green follows from the mean of 1, in the shader. The haze uses up the uniform's
  free fields, per-frame values written into shader text are not acceptable and the view
  bind group is not to grow, hence the patch. The reflection and lens cameras clone the main
  camera's fog, so they get the same vectors; the reflection camera's rays are the mirrored
  ones, which see the sky above the reflected land. The 3D scope's lens camera copies the
  main camera's fog after both writers of it after the sky (fog.rs `FogWriters`: the desert
  haze and this; the reflection camera copies in PostUpdate), so neither shows a frame with
  the sun's side not yet turned or the aerial fog without the haze (a scheduling test).
- **Off.** With the switch off (or a weight of 0) the vectors are zero, `nvf_aerial` makes
  the call as the haze left it and `GAME_FOG` runs its old lines: the picture and the log are
  as on main. What stays for every player: the fog uniform is 48 bytes larger, and the fog
  path has one more uniform branch in `nvf_aerial` and one in `GAME_FOG`; the edits are made
  at the start, with no log line. Both fail safe (one `warn!` line, the shader as it was, the
  row does nothing). With the row on, the weather's fog numbers, five of its colours, the sky
  level and the sun's height are read each frame; the derivation (the dome's colours, the
  weights, the encodings) is done again only when one of them or the dome mesh changes.
- **What follows the weather** (from `SkyState` every frame, through weather fades, scripted
  SetWeather/ForceWeather and the hour; no weather names, no hour table, no result kept past
  a change): the weight from the fog numbers, the channel weights and in-scatter colours from
  the fog, horizon, sky-lower and sky-upper colours as the dome draws them and the sky level,
  the lobe from the sun colour and the sun's height. Values outside the base game's ranges
  are tested as data (fog far 0 and 1,000,000, power 0, NaN and infinite colours, a black sky,
  one channel 0, a black sun): no NaN, nothing negative, no division by zero.
- **Switching**: the row, `--graphics-only aerial-perspective`, a `--graphics-cycle` set,
  `NVF_PLAY_TEST_SETTINGS=<t>:aerial-perspective`. Only the uniform changes; nothing compiles.
  Log lines: `aerial: aerial perspective on: <weather>: weight <w>, channel weights
  <r>/<g>/<b>, sky share <s>, sun lobe <l>` (or `..., none (the weather's own fog leaves no
  room)`); the same as `aerial: <weather>: ...` when the weather, the weight (by 0.1) or a
  channel weight (by 0.1) changes, not during a time-lapse; `aerial: aerial perspective off`
  (`... off indoors`). Debug aid `NVF_PLAY_DEBUG_AERIAL=<multiplier>`: the weight times that
  (1 as designed, 0 to 4; the sky share and the lobe are held to 1, the weights to their
  range), logged once; it does not turn the switch on. Smoke: `aerial-perspective`,
  `aerial-perspective-with-haze`.
- **Cost**: none that the runs can show: 3.23-3.28 ms a frame with the row off, 3.23-3.29 ms
  on, main 3.25-3.28 ms (three rounds each, the median of five 4-second periods, 2560x1440,
  Goodsprings at 13:00 under NVWastelandClear, Sun shadows on, on the contributor's machine)
  [observed: NVForever run, 2026-11-19]. Not measured on a slower GPU or at other resolutions.
- **Not done**: between the two reference elevations the sky colour is a smoothstep between
  the dome's two colours, not the dome's own curve; no height dependence (the weather's fog
  has none; the haze has its own); no shadowing of the in-scattered light; the sun's
  direction is the view's first directional light, turned back toward the sun where it is
  the moon's side (the first-person arms' shade light points the same way); the cloud cover
  is not read, so the base game's overcast is not told from clear (their classification
  flags and cloud layer counts are the same [observed: FalloutNV.esm, fnvtool esm decode,
  2026-11-17]).

### 3.19 Night sky detail (`night-sky-detail`; off in every preset)

[design, #2995] An addition of NVForever's own, an engineering choice for the enhanced look:
not a fact about the original game, never a parity item, and never to be used to explain a
difference from the original's frames (`vanilla` stays the parity picture). The row "Night
sky detail (experimental)" (Graphics > Lighting and Effects, after Aerial perspective) is off by
default and in every preset, `--graphics enhanced` included (`graphics.rs` test). Issue #2995
predates the settings menu overhaul; the old `enhanced` row no longer exists, so this is a
row of its own. `--graphics-only night-sky-detail` and a `--graphics-cycle` set turn it on
too. Code: `night_sky.rs` and the night part of `sky.wesl`.

- **What** [design]. Three additions, built only from the climate's own stars and moon and
  the shown weather's own colours and fog: the stars **twinkle**, more toward the horizon; a
  **glow round the moon**, wider and stronger in hazier weather; **moonlit clouds**, the cloud
  layers lit a little on the side of the sky facing the moon. Nothing new is read from the
  data and nothing is shipped.
- **Where it sits** [design]. No pass, texture or mesh of its own: the sky's own fragment
  shader (`sky.wesl`, used by every sky part) does all three from two `vec4`s at the end of
  the sky parts' uniform, `sky::SkyLight::moon` and `::night` (the Rust and WGSL structs in
  the same order; the composed shader's layout is tested). Their meaning depends on the part,
  as `params.w` already does: the star dome (the additive parts the daylight does not apply
  to, `params.z` 0) takes the twinkle; the atmosphere dome (`params.w` 2) the glow, drawn
  with the sun's disc and glare, so behind the clouds and under the moon disc; the cloud
  layers (`params.w` 1) the moonlight. The moon disc takes nothing. The additions use the
  arithmetic of the sun's glare beside them: linear RGB added to the part's colour before
  `tony_inverse`, which is the identity under the game's clipping tonemapper (`lights.rs`;
  the main view is HDR). The twinkle multiplies what a star adds.
- **How the values get there** [design]. `night_sky.rs` `update_detail` runs after
  `sky::update_night_sky` and works the detail out from the live `SkyState`, again only when
  the climate, the weather, the weather fading out, the fade, the hour, the days passed or
  `--fog` changed. Every frame it compares the result with each sky part's stored fields and
  writes only those that moved by more than 1e-3 (the disc's place by 1e-3 x its distance,
  the same angle), because each write remakes the material's uniform on the GPU (#897); zero
  is written exactly. So a part made later (a new worldspace's sky) takes the values the
  frame it appears. `update_sky` keeps the two fields when it replaces the rest of the
  uniform. A system test covers a later part, a weather fade, and off then on.
- **With the row off** the fields stay zero, `update_detail` returns at its first test, each
  branch in the shader is skipped by a uniform test, and nothing is logged.
- **Twinkle** (stars) [design]. Each star's added light is multiplied by
  1 + A x air(e) x s(dir, t):
  - s is lattice value noise in time over the dome direction (Ebert et al., "Texturing and
    Modeling: A Procedural Approach", lattice noises [documented: cited publication]). The
    direction times the cells per unit falls in a cell of a 3D integer lattice; each of its 8
    corners has a wave, the mean of two sines whose phases and rates (3,600 to 14,400 whole
    cycles an hour, 1 to 4 Hz) come from an integer hash of the corner: the corner times the
    primes of Teschner et al., "Optimized Spatial Hashing for Collision Detection of
    Deformable Objects" (2003), xor-ed, then MurmurHash3's 32-bit finaliser [documented: the
    published constants]. The waves are interpolated with smoothstep weights, so there is no
    edge between cells. s has mean 0 over time and stays within -1..1, so the factor's mean
    is 1 (the night is as bright as without) and it stays within 1 ± A (tested through a
    Rust mirror of the shader, `twinkle_factor`).
  - The lattice is read along the direction from the star dome's own centre (its mesh's
    place, `get_world_from_local`), not from the eye. The dome never turns and follows the
    main camera, so the cells belong to the dome: a turning camera, the temporal
    anti-aliasing's jitter and the upscalers see no change of cell, and the water
    reflection's camera (the main camera mirrored below the water, `reflection.rs`) gives a
    reflected star the same phase at any height above the water. A 3D lattice has no pole
    and no seam.
  - Cell size: about the size of a star on screen. At 1080 lines with the game's default
    field of view (75, the horizontal angle at 4:3; vertical 59.8°, `free_cam::vertical_fov`)
    a pixel spans 9.7e-4 rad; a star of the base game's star dome covers 2 to 4 pixels there
    [hypothesis], so a cell of 4 pixels (3.9e-3 rad, 0.22°; about 258 cells per unit of
    direction) gives neighbouring stars their own phases while a star's own pixels change
    nearly together.
  - Time: Bevy's `globals.time`, the extracted `Time`, which is the game's virtual clock
    wrapped at an hour (`bevy_render` `globals.rs` [documented: vendored source]); the pause
    menu holds that clock (nvf-play `pause.rs`), and with it the twinkle. Bevy writes it
    every frame, so no material is rewritten for time. The rates are whole cycles an hour,
    so the wrap joins without a jump; the phase is the fraction of the hour times the cycle
    count, within 1e-3 of a cycle in f32. At 60 frames a second the fastest wave moves 0.42
    rad a frame: smooth, never a per-frame random value.
  - air(e) = 1 - 1/X, X the air mass by Rozenberg's formula 1 / (sin e + 0.025 e^(-11 sin e))
    (G. V. Rozenberg, "Twilight", 1966 [documented: cited publication]): 0 at the zenith,
    0.5 at 30°, 0.82 at 10°, 0.975 at and below the horizon; bounded and monotone (tested).
  - A = (0.15 + 0.25 q) x the debug multiplier, held to at most 0.9 so a star never dims
    below a tenth [engineering choice]; q is the weather's haze (below). The clear Mojave
    night (q 0.5) twinkles ±0.27 at the horizon and ±0.14 at 30° up. A fragment whose added
    light is under 1e-4 (most of the sky between the stars) skips the lattice.
- **Moon glow** (dome) [design]. A Henyey-Greenstein lobe (L. G. Henyey and J. L.
  Greenstein, "Diffuse radiation in the Galaxy", Astrophysical Journal 93, 1941 [documented:
  cited publication]) divided by its value on the axis: (1 - g)^3 / ((1 - g)^2 + 2 g (1 -
  cos))^1.5, the denominator written so that it stays exact on the axis in f32; 1 at the
  centre, falling monotonically (tested).
  - Axis: from the rendering eye to the disc's centre. `moon.xyz` is where the disc is drawn
    from the sky's centre (`-sun_dir` from `SkyState::hour` x `SUN_DISTANCE`, as
    `update_night_sky` places it), and the shader adds the sky part's own place (the main
    camera's). In the main view that is the moon's direction; the water reflection's
    mirrored eye sees the disc elsewhere (3.97° off the moon's direction with the camera
    5,000 units above the water and the moon 10° up: our arithmetic, a test), and its glow
    stays round the disc it sees.
  - Width [engineering choice]: g = 0.9 in clear air to 0.75 under a full fog share, half
    the peak at 4.6° to 12.7° from the centre (the disc is 3.6° across, `sky.rs`
    `MOON_SIZE`).
  - Strength at the centre [engineering choice]: 0.06 x (1 + 0.5 q) (at most 0.09, linear)
    x the moon's strength. The moon's strength is its visibility
    (`SkyState::night_visibility`, fading in as the sun goes from +5° to -6°) x the phase's
    lit share (from the phase index in `MOON_PHASES` order: full 1, three quarters 0.75,
    half 0.5, a quarter 0.25, new 0, then back up) x the debug multiplier. It is 0 while the
    disc is hidden (its direction's height below -0.1, `sky::MOON_HIDDEN_Y`, the disc's own
    rule), indoors, in a climate without a moon and by day (visibility 0).
  - Colour: the hue of the weather's **Sunlight** colour now (`ColorType::Sunlight`, blended
    across a weather fade), its largest channel set to 1. At night that colour is the light
    the renderer lights the world with as moonlight (`sky.rs` `MOON_SHARE`, `aim_sun`), so
    the glow takes the light that weather's moon gives [design]. A colour darker than 0.05 in
    its largest channel keeps its hue with its brightness scaled down, so a black night
    sunlight gives no glow. No colour constant.
- **Moonlit clouds** (cloud layers) [design]. Added: the cloud's own colour (its weather's
  PNAM colour at the hour, as `update_clouds` sets it, times its texture and the sky level,
  without a lightning flash's light, which the sky already adds once) x 0.5 x a broader lobe
  (g 0.6, half at about 23°, aimed as the glow is) x the moon's strength, scaled down so
  that no channel adds more than 0.06 x the strength. The blend weighs it by the texture's
  alpha; through a weather fade both sets of cloud shapes take it, each at its own alpha.
  Every cloud layer that is drawn counts as cloud. The base game's starfield layer texture
  (layer 0 of the night weather) is in no archive (docs/spec-notes/world.md §6), so that
  layer is never drawn.
- **What follows the weather** [design] (from `SkyState`, no weather names, no per-weather
  table): the haze q, the weather's fog share at 50,000 units (the desert haze's reference
  distance, §3.16) by the curve the fog shader draws, with the fog's near, far, power and
  strength as the fog uniform takes them (or `--fog`), so it blends through a fade and
  follows SetWeather / ForceWeather the next frame; the moonlight colour; the cloud colours
  (in the cloud parts' own colour); the hour (the moon's place and visibility); the days
  passed and the climate's phase length (the phase); the climate's moon flags. A script's
  moon textures (SetMoonTexture) change nothing here. It adds to the weather's own colours
  and never replaces them. A weather whose two fog ranges are both unset has no fog
  (`Weather::fog_at`) and so no haze: the clear-air values.
- **Weather and mods** [design]: tested as data in `night_sky.rs`: fog far 0 and 1,000,000,
  near above far, power 0 or negative, values that are not numbers, both fog ranges unset, a
  moonlight of 1e6 or negative, phase length 0, every phase index and beyond, the sun exactly
  at the horizon, directions at elevation 0 and 90°, no direction, the multiplier 0 and above
  4: no NaN, nothing negative, every field within its bound.
- **Switching**: the row, `--graphics-only night-sky-detail`,
  `NVF_PLAY_TEST_SETTINGS=<t>:night-sky-detail`. Only uniform values change, so no pipeline
  is compiled. Log lines:
  - `night sky: detail on: <weather>: haze <q>, twinkle <A>, moon <phase> lit <l>, strength
    <s>, glow <peak> half width <deg>°` (`no moon` for a climate without one);
  - the same figures as `night sky: <weather>: ...` when the phase's lit share changes and,
    outside a time-lapse, when the weather changes or the haze moves by 0.1;
  - `night sky: applied: <n> star material(s) twinkling (amplitude <A>), the dome's glow
    <g>, <k> cloud material(s) moonlit`, once each time it turns on, read back from the
    materials after the writes (only checked while this frame's values can give it: not by
    day, at a new moon, without a moon or at the multiplier 0; the cloud count includes
    hidden layers);
  - `night sky: detail off` (`... off indoors` on going in).
- **Debug aid** `NVF_PLAY_DEBUG_NIGHT_SKY=<multiplier>` (0 to 4, 1 as designed): all three
  times that, read once and logged (`debug switch NVF_PLAY_DEBUG_NIGHT_SKY: ...`) when the
  detail first turns on; it does not turn the switch on and does not make a run unattended.
- **Tests.** `night_sky.rs`: the pure parts above, the mirrored view (`glow_axis`,
  `star_direction`), the write tolerances, the kept derivation (system test), and that
  `sky.wesl` holds the constants its Rust mirrors use. `engine/nvf-shader-tests/tests/sky.rs`
  composes the production `sky.wesl` with Bevy's own imports through Bevy's shader cache and
  validates it with Naga, with no definitions (the dome, clouds and moon) and with the
  definitions Bevy's mesh pipeline sets for the additive stars (`PREMULTIPLY_ALPHA`,
  `BLEND_PREMULTIPLIED_ALPHA`), and checks the uniform's layout. Smoke: `night-sky-detail`
  (switched on in the pause menu at Goodsprings at 23:00 on day 0, a full moon, left on after
  the game resumes; it expects the `applied` line).
- **Seen** [observed: NVForever run, 2026-11-19, pictures at Goodsprings]: the glow follows
  the phase (centre 0.075 at the full moon, 0.037 at the half moon, none at the new moon, as
  logged), is wider under NVWastelandHazy, and nothing shows at noon.
- **Cost.** Measured 2026-11-19, indicative: 3.13 ms off, 3.13 ms on at 2560x1440,
  Goodsprings 23:00, on the contributor's machine [observed: NVForever run, 2026-11-19]. Not
  measured on a slower GPU, at handheld resolution, or with FSR. What the code does: every
  sky fragment pays a few uniform tests; a dome or cloud fragment with the moon up also a
  matrix fetch, a normalisation, a `pow` and a few multiplies; a star fragment that adds
  light runs the lattice, eight corner waves of two hash finalisations and two sines each,
  and an `exp`. With FSR the stars, the clouds and the moon are drawn a second time into
  FSR's opaque reference (`fsr.rs`, the reactive reference pass), so the stars' and clouds'
  terms run twice there. The water reflection's and a 3D scope's cameras draw the sky too.
- **Not done**:
  - The twinkle was judged only from the log and still pictures; no clip of it was taken.
  - No colour change in the twinkle (chromatic scintillation) and no twinkle of the moon.
  - A mod's starfield texture on a cloud layer counts as cloud: it takes the moonlight and
    does not twinkle (no classifier is guessed).
  - FSR draws the stars and clouds twice (above); nothing skips the night terms in its
    second draw.
  - No halo ring (the 22° ice halo), no moonlight on the fog or the land beyond what the
    weather's own night light does, nothing indoors.
  - The constants are starting values chosen by reasoning and checked in pictures at a few
    strengths (the debug multiplier), not tuned against anything.

### 3.20 Full-detail scenery (`full-detail`; 7 x 7 in `enhanced`, off in vanilla, #2701 stages 1 and 2)

Past the loaded cells (5 x 5 around the player's cell, `uGridsToLoad` 5 [observed: the
owner's `FalloutPrefs.ini`, world.md §7]) only references flagged "visible when distant"
have a distant form, the object LOD blocks, and everything else appears only when its cell
loads (world.md §7). The row draws the full models of static scenery a few cells farther,
draw-only. Every rule below is NVForever's own choice [design], not the original's
behaviour; the code is `engine/nvforever/src/full_detail.rs`.

- **Presets**: `enhanced` turns on the 7 x 7 ring, `vanilla` keeps it off (the original
  draws no full models past the loaded cells) [maintainer decision 2026-11-21 on #3048].
  The cost is about 0.32 ms a frame at 7 x 7 and 0.56 ms at 9 x 9 (measured for #3048).
  The row is a choice of three, not a switch, so `quality::enhanced_keys` (the switches a
  preset name compares) does not list it; `--graphics enhanced` and the preset set it.
- **Sizes**: Off, 7 x 7 (the cells 3 grid steps from the loaded grid's centre) and 9 x 9
  (3 and 4 steps): 24 and 56 cells beyond the 25 loaded ones. `--graphics-only
  full-detail-7x7` or `full-detail-9x9`. The row switches in play: turned off or made
  smaller, the cells out of the new reach fade out as leaving cells do, the LOD coming
  back under them with the complementary dither; made larger, the new cells stream in.
  No ring indoors.
- **Draw-only**: the game loads exactly the cells it loads with the row off. Ring cells
  are never in the streamer's loaded cells (`Game::loaded_cells`, physics, the AI's
  process levels, water, rooms and reports are as before) and their entity is no render
  cell, so nothing of nvf-play draws into it. Its models keep their reference's
  `RefVisual` (the simulation hides and shows them, deletions and texture swaps reach
  them) and carry `nvf_play::DrawOnly`: the console's and the editor's picks, the
  crosshair, the loaded counts, the scripts' NIF functions, scripted effect shaders (with
  their add-on models and particles) and the actors' hit volumes and blocking
  (nvf-play hitvol.rs, both where meshes are marked and where their bounds are read) leave
  them out. No collider.
- **Game-side checks**: the test aid `NVF_PLAY_TEST_GAME_DIGEST=<seconds>,...` (a
  measuring aid, it drives nothing; nvforever `game_digest.rs`) logs at each time `game
  digest t=<s>: cells <hash> residents-high <hash> triggers <n> zone-levels <n> onload <n>
  end`: an order-independent hash of `Game::loaded_cells` and of the residents in the high
  process, and the triggers entered, encounter zones given their level (counted where the
  level is set) and reference OnLoad blocks entered (counted where the script runs each
  block) so far; `end` anchors the last number for the smoke checks, which match text. The smoke pair `full-detail-ring-off` / `full-detail-ring-9x9` takes the same digests
  on the same route; with the ring at 9 x 9 they must match the ring-off run's.
- **Drawn**: STAT, SCOL and MSTT references (a moveable static with its idle sequence
  playing, nothing else of its rig, its pose never published to physics), SpeedTrees
  (their generated geometry, else their billboard), and every other reference with a
  model flagged visible when distant (REFR record flag 0x8000 [documented: GECK wiki
  "Reference", GetReferenceFlag]), as plain static scenery: the model's shapes as stored,
  no rig, light source, emitter, stored pose or collision. A moveable static's idle stands
  still while its ring cell draws nothing (spawning, standing by fully under a loaded
  cell). Trees: the ring draws exactly the trees the distant billboards draw, by one shared
  rule (treelod.rs `distant_tree_ref`: a TREE with a SpeedTree, no imposter, no primitive,
  enabled at the start, at its authored place), and the billboards crossfade with ring
  cells' trees as with loaded cells' (treelod.rs reads `DrawnCells`). Sharing the rule
  leaves the billboards with the ring off as they were in the game's data: no exterior
  TREE reference there is an imposter or a primitive [observed 2026-11-19: 18,702 exterior
  TREE references across FalloutNV.esm, the four story DLC and the five packs, in eleven
  worldspaces (WastelandNV 18,050), 0 with the XSRF imposter bit, 0 with XPRM; 323
  references of any type carry the imposter bit and 8,819 XPRM; a read-only count of the
  plugins]. A mod's imposter or primitive tree would lose its billboard (an imposter is
  drawn by region, a primitive never). A cell where a tree
  drew nothing keeps all its billboards, so its other trees show twice (full tree and
  card): the billboards are one mesh per cell and TREE base, so keeping one tree's card
  alone would mean rebuilding that mesh; the doubling is the accepted fallback. A tree base the loaded cells have not built is waited
  for 3 seconds (`TREE_WAIT_SECS`), then built on the main thread, one per frame (tree.rs
  has no worker-thread path; one build is not bounded in time). A cell's object LOD
  segment holds the
  LOD of its flagged references (world.md §7), and the ring hides it as a loaded cell does,
  so nothing flagged may be left out. The landscape at full resolution (under 4 x 4 near
  the 2 x 2 drawing the loaded cells start with, which meets their edge without cracks),
  counted nowhere (the terrain summary is the loaded cells').
- **The game's state**: with the game running, references not enabled at the start
  (not deleted) are drawn too, and the simulation hides and shows every ring copy
  (`activate::sync_visuals`: on spawn, and on each `RefChanged`, which the game sends for a
  script's Enable or Disable of any reference, loaded or not). In the viewer (no
  simulation) only references enabled at the start are drawn. A reference the game moved
  is drawn where it is (nvf-play `moved.rs` finds ring cells by `nvf_play::DrawOnlyCell`);
  a destroyed reference's ring copy takes its destruction model (destroyed.rs): the
  reference stays pending until the model is prepared on the worker threads (tried again at
  each look, every half second or so), and a new model that draws nothing (missing, no
  shape) leaves the old copy in place.
- **Left out** [design]: imposters (drawn by region), primitives, markers, deleted
  references, actors, light sources, sounds, placed and cell water (distant water keeps
  covering ring cells), grass, and non-flagged activators, doors, containers, furniture,
  terminals and items.
- **Kept segments**: a cell keeps its LOD segment while a flagged reference in it is not
  drawn here: an actor, placed water (types the ring never draws, kept for good and never
  drawn by a model swap), or one whose model spawned no shape (missing, hidden, empty).
  Those are kept per reference against the cell's own entity, from its spawn through its
  publication to its retirement (`KeptRefs`); unflagged ones too, as retries without a
  segment. A later model swap may draw such a reference in that cell (destroyed.rs), and
  a draw clears its reason; a swap's result never reaches another cell, or a later cell at
  the same square, or a retiring one. There the segment shows beside the full
  models of the cell's other references, and since a segment holds the LOD of all the
  cell's flagged references, those show twice (full model and LOD) for as long as the
  cell keeps it: a doubling rather than a hole. Everywhere else nothing is doubled at rest.
- **LOD hand-off**: lod.rs takes the drawn cells from both (`DrawnCells`): a ring cell's
  block segment crossfades with its fade and its LOD terrain is cut out once it is fully
  drawn, exactly as for a loaded cell. One rule decides how the LOD segment, a ring cell
  and the loaded cell of the same square share the pixels (`hand_off`): a ring cell under
  a loaded cell is held at rest and draws the complement of the loaded cell's fade step,
  so the segment (at level 1) draws nothing; a cell finishing its spawn under a loaded
  cell goes through the same rule before it is drawn; a ring cell already standing by
  under a loaded cell that is told to leave (the row turned off) keeps that complement
  until the loaded cell is at rest or gone, then fades out alone; a ring cell fading out
  when a loaded cell first comes is dropped at once. A ring cell fading in when its square
  loads therefore jumps to rest (a visible step, rare: the player must cross a cell within
  a fade). The ring cell goes once the loaded one is fully drawn inside the loaded radius;
  in the loaded grid's step of hysteresis it may stand by under a loaded cell (culled), to
  take over when that one fades out. What the tests prove (`full_detail.rs`): by the ideal
  dither masks of the three, the rule draws each pixel once at every fade step and through
  scripted frame sequences (a stand-by switched off, a reversal, the six switches of the
  row, a cell finishing under a fading loaded cell). Not covered by that, and real: the
  LOD terrain stays under full terrain that is only partly drawn (it is cut out once the
  cell is fully drawn, and sits behind by its depth bias meanwhile); LOD blocks and
  terrain are rebuilt within the LOD's rebuild budget, so for some frames after a cell
  starts or stops fading the old block or cut stays (the block part of a cell that starts
  fading out is missing until its block is rebuilt, as before the ring); and kept
  segments double flagged scenery (above).
- **Streaming**: ring cells are read ahead on the worker threads only while the loaded
  cells' read-ahead is idle, and spawned nearest first, only while every loaded cell is in
  and none is being spawned. No model or landscape is prepared on the main thread for the
  ring: a cell waits for its read-ahead, and a reference whose model is not prepared (a
  destruction stage's or a script's model, a persistent reference in a square with no cell
  record) is asked of the worker threads and tried again once the cell's other references
  are in. Kept one step beyond the ring, then faded out. A door's travel
  (`Streamer::travels`), a change of worldspace or going indoors drops the ring and its
  readings at once, as the loaded cells go, and never waits for it.
- **What the 1.5 ms budget bounds, and what it does not**: within a frame's budget the
  read-ahead starts new readings and takes in finished ones only until the deadline, and
  references are spawned one at a time until it. Not bounded by it: the one reference or
  cell landscape under way when the deadline passes; a tree base built after its wait (one
  a frame, of no fixed length); the fade and shadow walks over a cell's meshes when its
  fade step or shadow rule changes; retiring and despawning cells (at most 400 children a
  frame, as the loaded cells); a destroyed reference's ring copies drawn again. The
  summary's milliseconds are the whole time of the ring's own systems on the main thread
  (stream, fade, the ring's part of destroyed.rs), these included; the commands they queue
  are applied by Bevy later and are not measured. At rest (the same centre, reach and
  loaded cells, none fading or spawning, every ring cell in and at rest) the ring's systems
  do nothing but compare that state; never while a loaded cell over or under a ring cell
  fades (the state holds each fading cell's step). Ring copies still cost every frame once drawn: their
  meshes, the animated materials of their models and the leaf-order sorting of their
  trees; their moveable statics' idles stand still while the cell draws nothing and while
  a retired cell despawns, and a rig without shapes is not spawned.
- **Size cut**: in the ring's outermost cells a reference not flagged visible when distant
  is left out when its model's bounding radius is under 0.004 of its distance from the
  loaded grid's centre (`SIZE_CUT`; about 57 units at 4 cells). The cell keeps the list,
  and the references are spawned (within the budget) once the cell is no longer
  outermost. The summary counts the references cut.
- **Shadows and sliders**: ring meshes cast no sun shadow while the cascades end inside
  the ring's inner edge (the default 8,000 units against the 8,192 a ring cell is at least
  from the camera's cell's edge); with longer shadows they cast. The rule uses that
  nominal distance, not each mesh's bounds or the sun's direction. The draw-distance
  sliders never cull ring copies (`DrawClass::Unranged`): their cells hide their LOD
  segments, so a culled copy would leave neither model nor LOD. The Object slider works on
  the loaded cells as before the ring, on, off and indoors alike; below 100% it can cull a
  loaded cell's object whose LOD segment is hidden too (an existing limit, upstream before
  the ring).
- **Off**: with the row off nothing of the ring is drawn or logged; the only log
  difference is the settings page's count of rows in its start-up line, as for any new
  row. Three systems still run and return at once (the ring's stream and fade, the exit
  report), and lod.rs builds its list of drawn cells through `DrawnCells`.
- **Readers left as they are** [design]: occlusion planes of the loaded cells
  (rooms.rs) may cull ring meshes behind them, which is what they are for, and its
  diagnostics count them; damage-stage shapes of ring copies follow the game's stage
  (destroyed.rs `staged_shapes`); activation sync, reference
  fades, texture swaps, draw distance, shadow thinning (a test aid) and billboard facing
  act on ring meshes as on loaded ones (visual only); the Modern minimap's camera can show
  ring geometry at its edge (visual only); the stress sample counts ring entities in its
  totals and names `ring meshes` apart.
- **Logs**: `full detail: 9 x 9 ring (radius 4)` when it comes on; at the exit `full
  detail summary: 9x9, <n> cells, <m> references drawn, <k> skipped by type, <t> ms in its
  systems on the main thread, <s> too small for their distance` (`<t>`: above, the queued
  commands' application not included); the stress sample adds `ring meshes <n>` while
  the ring draws any.
- **Not measured yet**: frame cost and streaming hitches on the traversal routes
  (Goodsprings to Primm, Novac), ring off against 7 x 7 and 9 x 9.

### 3.21 Distant objects (`distant-objects`; off in both presets, #3014 first stage)

Past the loaded cells (and the full-detail ring, §3.20) only what the worldspace's object
LOD blocks hold is drawn. The editor bakes a reference into them when it, or its base, is
flagged "Visible When Distant" and a `<model>_lod.nif` lies beside the full model
[documented: GECK wiki "Static", "World LOD"; world.md §7 "What goes into the object
blocks"], for one load order: `_lod` models a LOD pack or a later plugin adds do nothing
until the blocks are made again. This row draws the rest of the static scenery directly,
following the load order as it is. Every rule below is NVForever's own choice [design],
not the original's behaviour; the code is `engine/nvforever/src/distant_objects.rs`.

- **Reach**: Off, 8 cells and 12 cells (`REACH_NEAR`, `REACH_FAR`): the cells from one
  step beyond the larger of the loaded radius and the ring's out to that many grid steps
  from the loaded grid's middle cell, leaving out the square's corners whose nearest point
  lies past the reach's distance. `--graphics-only distant-objects-8` or
  `distant-objects-12`. Greyed on the page while Distant terrain and objects is off, and
  inert then (without the LOD terrain the objects would stand on nothing). Switches in
  play: cells out of a smaller reach fade out, new ones stream in. None indoors.
- **Which references**: exterior STAT and SCOL references that pass the ring's scenery
  filter (not deleted, not imposters, primitives or markers, not among the model paths
  hidden for diagnostics); with the game running, those disabled at the start are drawn
  too and the simulation shows and hides them, as in the ring. A reference with a model
  override (a destroyed object's stage, a swap) is not drawn here at all: when its
  override comes, its shapes are despawned at once from every distant cell holding it
  (drawn, being replaced or being spawned) and its triangles leave the budget; when an
  override comes or goes, the cells holding that reference are judged again, even while
  fading or covered (the loaded cells and the ring draw the override). Moveable statics are
  left out of this stage (an animated rig per reference at a distance, and their effect
  shapes without the emitters).
- **The rule** (`choose`, conservative, free of doubling while the blocks match the load
  order): a reference flagged visible when distant, by its own flag or its base's in any
  plugin's version of either record (`fnv_world::placed::visible_when_distant`), is left
  to the block while its cell's segment is not empty or not known yet (`lod::BlockContents`,
  filled when the LOD prepares the square's block; a square with no block file holds
  nothing). Every other reference is drawn from its LOD model when the load order has one
  (`<model>_lod.nif`, else `<model>_lod_0.nif`, one pack's naming), else from its full
  model when its bound radius (OBND, times its scale) is at least `SIZE_RATIO` 0.01 of its
  distance from the middle cell's centre (328 units at 8 cells, 492 at 12), else not at
  all. The full model is the angular rule `shadow_thin.rs` uses.
- **Triangle budget**: the full models drawn sum to at most `FULL_TRIANGLE_BUDGET`
  (4,000,000) triangles, a model not prepared yet counted as `UNKNOWN_TRIANGLES` (5,000,
  about the base game's median full model with a LOD sibling). Past it the size ratio of
  the farthest ring of cells that still draws a full model is raised by 1.5 (up to 0.08),
  then the next nearer, until the sum fits (the fit runs on a worker); a full model that
  would still pass the cap is left out. The sum is read from the cells alive, in three
  parts (`Budget`): the drawn cells' models, the reservations of cells being spawned and
  the predecessors of rebuilt cells still fading out, so a cell dropped half-spawned or
  retired gives its share back by leaving. One rebuilt cell at a time is judged without
  its own predecessor (whose models go once it rests); every other spawn counts
  everything, so the total passes the cap by at most that one predecessor's share, from
  the replacement's first full model until the predecessor has faded out. A model
  refused for the budget is counted apart from one too small (the summary's `past the
  triangle budget`), and its cell is judged again once room for its smallest refused
  model comes back (looked at before spawning and again after the frame's replacements,
  so room freed late in a frame counts at once). A worker's fit is taken only while it is for the centre and reach
  judged now. Logged once per centre: `distant objects: full models past 4000000 triangles;
  size ratio raised from <n> cells out (up to <r>)`.
- **Draw-only**: as the ring's cells. A distant cell's entity is a `DistantCell` and an
  `nvf_play::DrawOnlyCell` (moved references follow), never a loaded or render cell;
  models keep `RefVisual` and carry `nvf_play::DrawOnly` (`world::SpawnMode::Scenery`). No
  collider, light, emitter, script or sun shadow (`NotShadowCaster` on every mesh). The
  draw-distance sliders leave them alone (`DrawClass::Distant`).
- **Streaming**: each cell's list is read on the worker threads (placements, base records,
  bounds, which LOD model exists in the archives and loose files), never on the main
  thread, `MAX_LISTING` (8) at a time, only while the loaded cells' and the ring's
  read-ahead are idle; lists are kept within two steps beyond the reach. References with
  neither a flag nor a LOD model and too small to pass the rule at the nearest distance a
  distant cell can be at are dropped there. Cells are spawned nearest first within 1 ms of
  the main thread a frame, after the ring, only while the loaded grid and the ring are
  complete, and only with models already prepared on the workers: a cell waits for its
  models while the next four cells' models are asked for. What the 1 ms bounds: starting
  lists, the scan for cells to judge again, and the cells planned and spawned stop at the
  deadline between items; one cell's plan and one reference's spawn (every shape of its
  model) are never split, so a frame can pass by one of them. Taking finished lists in is
  bounded by the eight in flight; the override check is one pass over the overrides. A
  settled frame (nothing under way, every loaded and ring cell at rest, the re-judge scan
  through a whole lap for the current block revision, centre, reach and ratio table,
  every wanted cell at rest and judged for now, and the same centre, radii, loaded and
  ring cell counts, block revision and overrides) returns after comparing those, with no
  pass over the cells; the summary's
  milliseconds cover both of the feature's systems from their start. A
  door's travel or a change of worldspace drops everything at once (behind the loading
  screen); going indoors or turning the distant LOD off hides everything at once and
  despawns it over the next frames in the loaded cells' bounded slices.
- **Fades** (`distant_range`, tested pixel by pixel): a distant cell in a square drawn by
  a loaded or ring cell draws the complement of that cell's fade step and is held at rest
  under it (the ring's `hand_off`), so a reference drawn there in full and here from its
  LOD model covers each pixel once; it goes once that cell is fully drawn inside the inner
  radius. The cover is the loaded or ring cell's scenery level (`scenery_level`), not
  the ring's tree level: a ring cell whose trees fell back to the billboards still draws
  its statics. Alone a distant cell fades in and out by its own step and always dithers
  out with distance between `reach - 1` and `reach` cell widths from the camera
  (`edge_range`). One visibility range cannot multiply two dithers, so a fading mesh is
  drawn at the smaller of its cell's step and the reach's fade at its own distance
  (`band_range`); from the reach on it is culled outright (an empty far margin at the
  reach), so nothing at or beyond the reach is drawn at any step. At rest the plain
  distance fade takes over at the same level; in the band the two dithers keep different
  pixels at the same count, so when a fading mesh there comes to rest its pattern
  changes, not its coverage. Cells fade out past `reach + 1`, out of a smaller new reach,
  or, after Off, all of them, with the last reach's fade; `distant objects: off, 0 cells
  left` is logged once the last has gone.
- **Re-judging**: each cell keeps a stamp of the centre and the ratio table it was judged
  for, and its block's answer then. When the loaded grid's centre has moved two cells the
  ratios are fitted again; a cell whose stamp is stale (a new centre or new ratios), or
  that holds flagged references whose block now answers otherwise (unknown to empty or to
  held), is judged again once it is at rest, alone and not being replaced, so a cell
  that was fading or covered catches up later. One whose drawn set changes is built
  again: crossfaded with the old one (the new cell's step against the old one's
  complement) where the whole cell lies within `reach - 2` cells of the middle cell's
  centre, at once nearer the edge (a complement cannot also carry the reach's fade); a
  predecessor that no longer passes (the reach shrank, the centre moved) goes at once.
  The scan for cells to judge again goes round a stable order from where it stopped. A
  model drawn full stays while its radius is at least 0.8 of the threshold
  (`HYSTERESIS`), so nothing flickers at it.
- **Logs** (only while on): `distant objects: reach 8 cells (ratio 0.01)` when it comes
  on; `distant objects: off, 0 cells left` after Off once the last cell is gone; at the
  exit `distant objects summary: <cells> cells, <drawn> drawn (<lod> lod, <full> full),
  <held> held by blocks, <unknown> block unknown, <small> too small, <b> past the
  triangle budget, <t> ms on the main thread`. With the row off nothing is drawn or logged; its two systems and
  the exit report return at once, and lod.rs records each block's cells as it takes the
  block in (no log).
- **Failure cases** [design, known]:
  - The shipped blocks with plugins that flag more bases (a LOD plugin without its
    regenerated blocks): those references count as held and are not drawn, as in the
    original, which shows nothing for them either.
  - Blocks regenerated for another load order: a reference the blocks hold but the load
    order no longer flags is drawn twice (block and here); one flagged now but missing
    from the blocks leaves a hole.
  - Overhang of the conservative rule: a flagged reference whose model has no `_lod`
    sibling is in no block yet counts as held while its segment holds anything; a whole
    square waits on its block's preparation; squares past the LOD's block distance are
    never known, so their flagged references are never drawn.
  - The rule is judged per cell: a block segment holds or does not hold a cell's flagged
    references as a whole, so a block regenerated for another load order can double some
    of a cell's objects or hide others, cell by cell.
  - A child worldspace that uses its parent's LOD files draws the parent's blocks: its own
    flagged references count as held while the parent's segment for that cell holds
    anything, though the parent's block was not made from them.
  - During a crossfade, and where a distant cell stands under a loaded or ring cell, the
    two layers draw complementary halves of a dither pattern, but the LOD model and the
    full model differ in outline: where one has geometry and the other none, a pixel is
    drawn by neither or by one only, so the hand-off is pixel-once for the pattern, not
    for the silhouettes.
  - Gaps: in the ring's outermost step a reference the ring's size cut leaves out is not
    drawn here either (the distant cell under it is culled), and a reduced Object
    distance culls loaded and ring models that this layer, standing under them, does not
    draw: as without this row.
- **Tests** (no game data): the rule's branches, the budget's fit and parts through a
  cell's lifecycle (spawn, cancel, finish, rebuild, rejected rebuild, retire, clear), a
  replacement at the cap, deferred re-judging, a late block, overrides applied and
  cleared, pixel-once fades with the loaded and ring cells and across a rebuild, the
  reach's fade at every temporal step. Not covered: the streaming system itself against
  real cells, travel into a child worldspace.
- **Not measured yet**: frame cost and streaming hitches with the row off against 8 and 12
  cells on the Goodsprings to Primm route and at Novac, with the base game and with LOD
  resource packs; the issue asks for them in the pull request.
- **Follow-ups** (not in this stage): judging the flags from the plugin the block files
  come from (so regenerated blocks neither double nor leave holes); flagged non-static
  types (activators, doors, moveable statics); merging each cell's meshes by material;
  a link to the Object distance slider.

### 3.22 Distant lights at night (`distant-lights`; off in both presets)

[design, #3013] An addition of NVForever's own, an engineering choice for the enhanced look:
not a fact about the original game, never a parity item, and never to be used to explain a
difference from the original's frames. Placed lights exist only with their cell (world.rs,
lights.rs), so past the loaded 5 x 5 cells no lamp is drawn and a town at night is a dark
shape until its cells load. With the row "Distant lights (experimental)" (Graphics >
Detail and Distance, the last row, with the other distance rows; it needs no other row:
it does not draw on the distant LOD) on, the placed lights of the cells beyond the loaded ones, and the
statics that are the places' lights, show as small glows. Sprites only: no light entity, no
shadow, no game state. Code: `distant_lights.rs`, `distant_lights.wesl`,
`nvf_assets::model_glow`.

The Mojave holds few placed lights: 357 references with a LIGH base in the whole worldspace,
56 within 12 cells of Novac and 56 of Goodsprings, none disabled at the start [observed:
FalloutNV.esm, `fnvtool world refs --world WastelandNV`, 2026-11-19]. The towns light
themselves mostly with statics: glow cards, light beams, lamps with a self-lit bulb, sign
panels (Novac's within two cells: 12 LIGH references beside light beams, recessed lighting,
glow cards, lamps and signs) [observed: the same listing]. So the glow statics below are
what the feature mostly shows.

- **Source.** The exterior cells whose closest point lies within 12 cells of the camera
  (`treelod.rs` `cells_within`, as the distant trees choose theirs). The cells not read yet
  are listed on the read pool's worker thread (as world.rs reads cells ahead), from the
  references world.rs spawns a loaded cell from, through the renderer's own filters
  (`world::drawn_with_cell`: no imposters, which their regions draw, no editor primitives
  such as a trigger copied from a glowing activator; `world::marker_model`: no marker
  models)
  (`cell_refs`: the cell's own and the worldspace's persistent ones standing in it): those
  whose base is a LIGH, not deleted, not negative, not off by default, with a radius (DATA
  radius plus the reference's XRDS) and a colour of the record's own (`fnv_world::lighting`;
  the flag bits as the GECK wiki "GetLightFlag" page lists them [documented]). A light whose
  colour comes only from its model is left out. Lights disabled at the start (their own flag
  or the enable parent chain, `world::enabled`) are kept with that state, so a script that
  enables one later shows it. The lists are kept per cell and dropped with the worldspace.
  A script's SetInternalMarker change (`world::script_markers_generation` moves on) lists
  the cells again, since the marker filter above classified them: a running scan is
  stopped, and the glows of the cells in reach stay drawn until their new listing replaces
  them.
- **Glow statics.** Beside LIGH references, statics and moveable statics (STAT, MSTT),
  activators (ACTI) and static collections (SCOL) whose model glows, judged from the model's shapes and never from its file name
  (`nvf_assets::model_glow`), by the properties the renderer already reads
  (`nif_spawn::shape_props`, `shape_alpha_mode`, `shape_emissive_mult`): an unlit shape
  added to the frame (glow cards, light beams: the additive shapes of nif.md §6 "Additive
  blend"); an unlit shape whose emitted light, after the picture rule, is over white (what
  nif_material.wesl burns as a hot source and the neon bright pass picks up); a lit shape
  self-lit at full emission (emissive colour × multiplier at 1 or more: a lamp's bulb, a
  sign's glow panel) [design: the last threshold is ours]. Apart from the added unlit
  shapes, which may be long beams, a shape wider than 512 units (bounding-sphere radius)
  does not count, so a self-lit band across a building is the building's picture, not a
  lamp. Helper geometry the near renderer leaves out (`Props::is_helper`) and hidden shapes
  do not count. A model with one small glowing part among ordinary geometry (a lamp post)
  qualifies through that part, as long as its glowing shapes together show enough light:
  the sum of brightness (the glowing texture's mean, times the emission) x radius^2 at
  least 25 (a bright bulb 5 units in radius; emissive trim and specks show less) [design].
  A shape whose texture is known dark (black or fully transparent where it blends) shows
  nothing; an unreadable texture (BC5, a file that does not parse) or none counts as fully
  bright, the emissive hue alone. Per model, read once from the NIF alone (no meshes,
  materials or textures for the GPU) and kept for the run: the glowing shapes' centre below
  the root, weighted by the light they show (the reference's transform takes it to the
  world, so a lamp post's glow sits at its bulb), their reach (feeding the size and budget
  rules as a light's radius would, times 4) and their colour (the emissive hue times the
  mean hue of the glowing texture: every texel of its first mip level of at most 256 x 256
  (without one, a grid of about as many from the smallest level), compressed blocks decoded
  with their selectors and an edge block's padding left out, DXT1's transparent selector
  honoured,
  weighted by brightness, and by alpha where the shape blends). A glow-mapped lit shape
  takes the glow map's hue without the diffuse texture the real material multiplies it by. Glow statics take the
  same present state, merge, cap, night share, fog and hand-over as lights, strength 1, no
  shimmer. A static collection is drawn by its own combined model, as world.rs draws it
  (the GECK wiki "Static Collection": its Model is the NIF the editor generates for the
  collection [documented: offline snapshot 2026-10-28]), so its glowing parts are found in
  that model, each at its place in it; a part as wide as a building is left out by the same
  512-unit bound. An activator shows its model's stored look: whether a script switched
  it is not known beyond the loaded cells (activators.rs follows only radio receivers' on
  state, and only while drawn), so it glows while enabled, and a sign switched off by a
  script without disabling it still glows [known limit]. Counted with an approximation of
  these rules over the records and the meshes (2026-11-19, the same tools): within 12
  cells of Novac 120 static references of 22 glowing models (of 833 static models there),
  of Goodsprings 55 of 23 (of 826): glow cards (`fxglowsimpfill*`), light beams
  (`fxlightbeam*`, HELIOS One's god rays), lamps (`llamp*`, `lamplightlamp`, street
  lights), recessed lighting, the NoVac sign's glow panel, tent lights; also some utility
  girders and fans whose small self-lit part is a fixture. Activators and collections add
  27 references of 4 glowing models around Novac (light beams, the motel's lit window,
  vault control panels, force-field effects) and 16 of 6 around Goodsprings (Primm's lit
  windows, the Prospector Saloon's neon, a sign, a collection with a cave lamp, force-field
  effects) [observed, the same approximation, which leaves out the visible-light rule].
- **The scan.** One job at a time on the read pool: first the nearest cells not listed
  yet, 32 at a time (their references copied on the main thread for those 32 only), then the
  static models they ask for, 32 at a time, at most 1,024 models per camera cell (the rest
  wait for the next scan; 895 were read within reach six cells west of Novac [observed
  2026-11-19]). Models are asked for on the main thread only, each once, so two jobs never
  read the same model. Every job checks a shared epoch between cells and between models;
  switching off, another worldspace, going indoors or daylight move it on, so a running job
  stops after the read under way. Each model's glow is handed back as soon as it is read,
  so a stopped job's reads are kept and only its unread models go back to the front of the
  queue; no job starts while a stopped one still runs (never two scans at once). A cell's
  sources are its lights and the statics whose model is read and glows; they are completed
  as the models come in (only the cells that show a model just read are resolved again,
  from a model-to-cells index), so a town fills in over the scan.
- **Present state.** Whether a listed light shows is the simulation's present state
  (`SimState::present`: not deleted, enabled now), or without a simulation its state at the
  start. Cells listed but not merged yet are checked at once, and every 2 s a pass over all
  cells starts; both within 2,048 units a frame, a hard bound (a light checked or merged one
  unit, starting a cell or handing a merge on one; a check stopped inside a cell resumes
  there in the next frame, a merge that does not fit waits for the next frame, and a cell
  with more present lights than the budget is merged on a worker and its glows swapped in
  when done). A cell is
  merged again when the set of its present lights changed (a key of their FormIDs, so
  disabling one lamp and enabling another in the same cell between two checks is seen).
- **Hand-over.** No glow for a cell inside the loaded grid (the square of `Streamer::radius`
  steps around the grid's centre that world.rs loads), whether loaded already or still
  pending: its own lights draw there, or will within moments. A cell outside the grid that
  is still drawn as it fades out gives its glows 1 - its fade level (`Streamer::cell_level`,
  fade.rs), so they come back as it goes; during that fade the cell draws both forms, its
  dithered models and lights and the glows growing back. The full-detail ring (#2701, another branch) draws
  more cells as loaded cells, which hand over by their level; `in_loaded_grid` is the hook.
  The batched mesh is rebuilt when the camera's cell, a cell's share (16 steps) or a cell's
  set of present lights changes: a few hundred cards, a few times a second at most during
  fades.
- **Budget.** Lights of one cell within 192 units of each other are one glow (brightest
  first, each joining the earliest glow in reach; the strength-weighted mean position and
  colour, the largest radius, the summed strength held to 2), found through a grid of
  buckets 192 units wide by the glows' current positions (3 x 3 buckets per light, the
  same result as a search over every glow, tested). Of all glows the 400 with the most light
  for their distance (strength x brightest channel x size / (distance^2 + one cell^2)) are
  drawn, taken by a partial selection with only those 400 sorted.
- **Drawing.** One mesh for all glows under the world root, no frustum culling (one
  draw). Each card faces the camera, spread in the view's plane in the vertex shader (as the
  leaf cards, tree.wesl). Half size: the light's radius x 0.5, held to 24..320 units, and
  never under 2 output pixels nor 1 render pixel, so a lamp 12 cells away stays a few pixels
  wide (fitted by pictures, 2026-11-18); a card widened to that floor keeps its own area's
  share of light per pixel, held to 0.85. Under an upscaler the floor is in output pixels, so
  the glows keep their size on screen whatever the render scale (the view uniform has both
  sizes). Unlit and additive (Bevy's `AlphaMode::Add`, the transparent pass: depth-tested
  against the world, no depth written, no prepass, no shadows), faded out over the reach's
  last 2 cells. Three kinds of card, all in the one mesh and material (the kind and a
  shaft's ends and widths are vertex attributes, laid out at the top of
  `distant_lights.wesl`):
  - *Round* (a static's round glow): a soft spot, e^(-4 r^2) (1 - r^2).
  - *Core and halo* (a placed light): a small bright core and a wider, fainter halo in the
    same card, (1 - h) e^(-14 r^2) + h e^(-3 r^2) (1 - r^2), the halo's share h = 0.35
    (`HALO_SHARE`), sized from the light's radius as before.
  - *Shaft* (a long glowing part: a light beam, a neon tube): `model_glow` gives each
    glowing part its shape, the principal axis of its vertices (in the model's space), its
    length along it and its width across at each end; a part under 1.5 times as long as
    wide stays round, and so does a part whose own triangles cover less than a quarter of
    its length x mean width (the widths measure all its vertices, so two rows of slivers
    would make a wide shaft over nothing drawn; `MIN_SHAFT_COVER`), and a model keeps its 4 brightest long parts as shafts, the rest in
    its round glow. The bright end is told by the vertex colours (alpha x luminance, the end
    thirds' means 1.25 times apart), else by the cone (the narrow end, the other 1.5 times
    wider, is the source), else the shaft is symmetric. The card spans the segment between
    the ends, widened across by each end's half width (so a cone tapers toward its source)
    and facing the camera around the axis (an axial billboard), each width held to the pixel
    floors; soft across (e^(-4 x^2) (1 - x^2)) and along it fading from the source to nothing
    at the far end, smoothstep(0, 0.08, v) (1 - v)^2 (a short soft start, `SOURCE_RAMP`), or
    bright in its middle when symmetric; a directed shaft also gets a small round core at
    its source (the lamp, strength 0.6, `SOURCE_CORE`). Its light per pixel is set so its
    integrated light (the profile's integral over its card) is `SHAFT_BRIGHTNESS` (1.5, the
    first pictures read too dim at 1 against the real beam models) times its round glow's,
    held to 4 times (`MAX_SHAFT_GAIN`), so a hair-thin shaft does not become a white bar.
    The shader maps each fragment across as its offset from the axis over the half width
    at that point along (both linear over each of the card's two triangles, `shaft_x`), so
    the profile is centred on the axis at every cross-section and the triangles draw the
    integral computed (interpolating the -1..1 corner coordinate itself, a tapered beam drew
    about 30 % less: 20,231 against 28,907 for the test beam). The profile is one function
    across times one along, so its integrals are computed once per kind (`shaft_moments`)
    and a shaft's costs a few multiplies when the mesh is built.
    Whether a shaft shows as one is decided once for its whole card from both ends (never
    half shaft, half dot): both ends between the near and far planes, neither seen end-on,
    and its length on screen at least two floors in output and in render pixels; otherwise
    it is drawn as its round card. Shafts are never merged with other glows. On the
    beam models of the Mojave the vertex colours carry the gradient (their alpha is near 0
    at the far end; `fxlightbeambrt03`, `fxlightbeamw01dim`, HELIOS One's `fxhelios_godray1`
    are brighter at their narrow source end), so the vertex test decides; a beam with even
    vertex colours and no taper (`fxlightbeamw02` by a rough survey) is symmetric
    [observed: fnvtool nif decode, 2026-11-19, a rough per-axis survey].
- **Colour, fog and the HDR path.** As the NIF materials' additive shapes are drawn
  (nif.md §6 "Additive blend", "Fog on added light"; nvf_assets `add_display`,
  `fogged_addition`): the glow's colour is the record's colour bytes / 255 (display
  values) times its fade (held to 2), the merged strength and 1.0 at the core (0.8 before
  the glows were added in display values; a few-pixel glow's centre then sat near 0.5); it
  is made linear (raised to 2.2), taken through the view's fog (`apply_fog`, with the game's curve
  of fog.rs and the desert haze of §3.16) and, in a view without HDR (Light glow off),
  Bevy's tonemapping, less black through the same, so fog dims the glow and adds no fog
  colour of its own; then added to the opaque frame behind it in display values (the view
  transmission texture, which the anchor in nif_material.rs keeps in every view), the sum
  held to 2. So a glow of the record's colour over a black night shows that colour, and
  the path is the same with Light glow on (the HDR view, its tonemapper holding display
  values to 1 per channel, lights.rs `GAME_CLIP`, and its bloom giving bright cores a halo)
  and off. Eye adaptation (eye_adapt.rs) only brightens dark views, so a lifted night view
  brightens the glows with it; a merged glow (strength 2) or a lift can clip the core's
  brightest channel.
- **Daylight.** The view's exposure never darkens for daylight (eye_adapt.rs brightens
  only), so an additive glow would show as a dot on a sunlit hillside. The glows are scaled
  by the sun's elevation instead: full at -4 degrees and below, gone at 6 degrees and above,
  smooth between. While the share is 0 the mesh is hidden and nothing is listed, checked or
  built. A fitted choice of ours [design], not the original's (which draws no light beyond
  its loaded cells). Glow statics take the same share, though many of their shapes glow by
  day in the original too: by day, at a distance, a glow card is not a light one looks for
  [design].
- **Flicker and pulse.** Lights with the flicker flags (flicker, flicker slow) and the pulse
  flags (pulse, pulse slow) get a slow shimmer of up to 15 % (a sine with a phase from the
  reference), slower than the near light's own motion (lights.rs `LightMotion`), which from
  afar would read as noise.
- **Doubling with other layers.** A glow source in a cell inside the loaded grid draws
  nothing (its own model or light draws). The full-detail ring and distant objects on other
  branches would draw the real model of a glow static at a distance; then the sprite and the
  model show together (a beam's real cone with its shaft over it). `in_loaded_grid` stays the
  named hook, to be wired when those branches are in.
- **Accepted limits.** A shaft is a flat sprite facing the camera around its axis: seen
  end-on it is a dot (it falls back to its round card). The glows are depthless additive sprites: they write no depth and no
  motion, so under temporal anti-aliasing, FSR or DLSS they are resolved like other
  additive shapes (smeared a little while the view moves; a card of a few pixels may
  shimmer). The fog on them is the fog at the card's own pixels, and outdoors the fog blends
  in display values, so the subtraction of fogged black is exact only where the fog is
  linear; a glow fades a little faster with distance than the plain fog share. No light
  colour from the model (lights whose LIGH colour is black), no emittance from a region
  (XEMI), no glows indoors or in other worldspaces than the camera's, no occlusion beyond
  the depth test (a glow behind the LOD's hills is hidden by them; one behind a building not
  loaded is not, as the building is not drawn). Not a script's light colour or fade edits
  (lights.rs session lights). An enabled activator that a script switched dark still glows,
  and one whose stored model is dark misses its switched-on glow. A collection's model is
  read as stored: alternate textures and damage-stage visibility are not applied. Up to
  four shafts per model bypass merging and each takes one of the 400 slots. A model not
  read within a scan's cap of 1,024 waits until the camera changes cell.
- **Known misses of the classification** (a visual rule from the shapes' properties, not a
  list): lights that exist only as part of a large building's own texture (lit windows, a
  casino's facade; the 512-unit bound), dim bulbs (a lit shape under emission 1), large
  lit signs that neither add to the frame nor burn, shapes hidden in the file and shown by
  an animation (hidden shapes do not count), a collection's part merged into a shape wider
  than 512 units in its combined model, and an activator's switched-off look. **False-positive
  risks**: fully self-lit decorative panels and additive effects that are not lights (the
  counted Novac area has utility girders and fans whose small self-lit part may be a
  fixture or trim, and force-field effect activators);
  the visible-light bound and the helper exclusion take out the smallest. The counts show
  that the rule finds candidates, not its false-positive rate.
- **Switching**: the row, `--graphics-only distant-lights`, a `--graphics-cycle` set
  `distant-lights`, `NVF_PLAY_TEST_SETTINGS=<t>:distant-lights`. The first switch-on
  compiles the glows' pipeline; switching off drops the glows and stops the scan. With the
  test aid `NVF_PLAY_TEST_PIPELINE_LOG=1` the pipeline log names it once compiled:
  `distant lights pipeline <n> ready` (pipeline_gate.rs, from a fragment definition the
  material adds, `NVF_DISTANT_LIGHTS`). Smoke: `distant-lights`, `distant-lights-switch`.
  Log lines, only with the row on: `distant lights: on (reach 12 cells, cap 400)` when it
  turns on, `distant lights: off` when it is turned off again, and at the exit `distant
  lights summary: <cells> cells listed (<refs> references), <lights> lights, <g> glow
  statics, <a> glow activators, <c> glow collections (<m> glowing models of <k> read), <d>
  disabled at the start, <drawn> drawn at most, skipped: <x> deleted, <y> no colour, <z>
  negative/off, <e> no radius`, the reasons
  counted over the light references read. The start line names no counts (nothing is
  listed yet). With the row off nothing runs past the switch's check and the log is as on
  main. The graphics
  line names it (`..., distant lights`) only when it is on.
- **Cost**: not measured yet (to be measured on the Goodsprings to Primm route at night,
  #3013): one draw of at most 400 cards, each fragment reading the frame behind once; the
  record listing on a worker, 32 cells a job, and each static model in reach read once for
  its glow (about 830 to 900 around a town, a NIF decode and at most a 256 x 256 mip of a
  texture each, 32 a job, one job at a time); at most 2,048 units of present-state work a
  frame.

### 3.23 Volumetric clouds (`volumetric-clouds`; experimental, off in every preset)

[design, #2991, #3032] An addition of NVForever's own, an engineering choice for the enhanced look:
not a fact about the original game (which draws its weather's cloud layers as flat textures on
the sky dome, sky.rs), never a parity item, and never to be used to explain a difference from
the original's frames. The row "Volumetric clouds (experimental)" (Graphics > Lighting and Effects:
Off, Performance, Normal, High) or `--graphics-only volumetric-clouds` (Normal;
`volumetric-clouds-performance`, `volumetric-clouds-high`) turns it on.

The clouds are the cloud source of the volumetric medium (docs/design/volumetric-medium.md: the far
buffer, the levels, the tiles, the history, the upsample). The medium casts a ray per 3x3 tile of a
buffer a share of the render size and calls the march once for each; the proxy in the main pass
composes the result over the dome. This section is the source: what cloud is where, how it is lit,
how it is marched, and what else reads it while the volume is drawn (the cloud shadows on the ground,
"Ground shadows from the light volume" below, and with them the light in the air). One density model ships (the code calls it the lean model
and names its functions `vc_m2_*`); a run draws with it and nothing else.

| File (`engine/nvforever/src`) | Holds |
|---|---|
| `volumetric_clouds.rs` | the switch, the proxy, the uniform, the start values, the aids, the march's CPU mirror |
| `volumetric_clouds_march.wesl` | the march: plain functions, called by the medium's trace |
| `volumetric_clouds_model2.rs`, `_model2_tests.rs` | the body's CPU mirror and its tests |
| `volumetric_clouds_light.wesl`, `.rs` | the light of one sample (a leaf module), its mirror, the light's colours |
| `volumetric_clouds_lightvol.wesl`, `.rs`, `_lightvol_tests.rs` | the light volume's producer, its schedule, the shared reader |
| `volumetric_clouds_wisp.rs`, `_air.rs`, `_tune.rs` | the wisps' mirror, the haze's dome colours, the live file |
| `cloud_noise.rs`, `cloud_shadows.rs` | the generated noise; the weather's cover |

- **Technique** [documented: the cited publications]. The form is Bauer's ("Creating the Atmospheric
  World of Red Dead Redemption 2", SIGGRAPH 2019, slides 14-18: a cover map, a height gradient per cloud
  type, a threshold of the cover against it, one 3D Perlin-Worley read carved in by a linear rescale,
  the read displaced) and Schneider's (Schneider and Vos, "The Real-time Volumetric Cloudscapes of Horizon
  Zero Dawn", SIGGRAPH 2015; Schneider, "Nubis", SIGGRAPH 2017: a low-frequency Perlin-Worley base
  remapped by the coverage makes the lobes, a Worley fBm erodes the edges, wispy near the base and
  billowy above). Placement and vigour are lattice value noise (Ebert et al., "Texturing and
  Modeling: A Procedural Approach"). The light is Beer-Lambert extinction with a two-lobe
  Henyey-Greenstein phase and the energy-conserving step integral (Hillaire, "Physically Based Sky,
  Atmosphere and Cloud Rendering in Frostbite", SIGGRAPH 2016), two scattering octaves (Wrenninge,
  Kulla and Lundqvist 2013) and Schneider and Vos's powder term; the depth toward the light is an
  opacity shadow map (Kim and Neumann 2001). Our own code from those descriptions; no code from any
  other project, no texture shipped.
- **Where it sits.** A sky part in the main pass, not a pass of its own: the sky dome's own
  mesh drawn again (its exact vertices, at the dome's transform; `proxy_mesh`) with the depth
  test Equal (`draw_on_dome`), with a material of our own (`AlphaMode::
  Premultiplied`, no prepass, no shadows, no face culled) whose fragment reads the medium's far
  buffer where its view ray lies. The dome is opaque and writes depth, and the vertex
  position is `@invariant` in bevy_pbr's vertex outputs (the vendored "equal-depth passes"
  change), so the proxy's depth equals the dome's to the bit exactly where the dome is the
  nearest thing drawn (sky), and differs wherever anything of the world lies in front of the
  dome, however close to it. Two
  vertices no triangle uses centre the mesh's bounds on the camera, so its sort key stays
  where it was. Its
  transparent sort key is in the sky's band (the
  sky's bias + 10: after the stars, the moon and the flat cloud layers, before the world's
  blended things); a material of our own gives its depth bias to the sort only (a
  StandardMaterial-based part also carries it as the raster depth bias, which would cross the
  dome's depth). Depth-tested, so pixels behind the world cost nothing; premultiplied over
  the dome (its sun disc and glare), the stars and the moon, so thick cloud hides them.
  Before the temporal resolve: TAA reprojects it as it does the sky (camera motion and the
  dome's depth); FSR draws the sky's band into its reactive mask's opaque reference, so the
  volume is not reactive (that draw reads the same far buffer; the march itself runs once, in
  the medium's trace pass); DLSS gets no mask for it. Only the main world
  camera draws it (render layer 21, which only that camera has). A sky-only volume: the
  clouds never pass in front of terrain or anything of the world.
- **Space.** The main view is HDR (`glow`, on by default, inserts it; the temporal modes force
  it), and the sky's parts are the weather's linear colours times the sky level, blended in
  linear RGB, through sky.wesl's inverse of Tony McMapface only when the clip tonemapper is
  not installed (lights.rs `display_clip`). The volume does the same arithmetic: its colour
  is built from the weather's linear colours times the sky level, premultiplied, and mapped
  through the same table (un-premultiplied) in the Tony case.
- **The weather's amount, our placement** [design]. The weather decides **how
  much** cloud there is and its colours; **where** the clouds stand is a field of our own. The
  weather's cloud texture pattern is not the clouds' pattern: the textures' patches are a few thousand
  units wide, and a cloud wider than tall on such a footprint is a low cloud, so a volume that follows
  them is one thin layer whatever height it is given; the talk's cover map is likewise a few large
  blobs [documented: Bauer 2019, slides 14-15].
  - *The amount* is one number per frame from the live sky: each live weather's own cover, the mean of
    `1 - Π (1 - alpha)` over its own drawn layers at full weight, composed by Cloud shadows' producer
    from verified textures (`WeatherCover`, published by the weathers' form IDs; a weather that has left
    the sky is never looked up), blended by the live fade's weights (the shown weather at the fade, the
    one fading out at one minus it). The tint shares (each layer's visible share, which weights the
    cloud colour) take the same normalised weights, so the two cannot disagree (`blend_covers`).
  - *The rule*: the volume is drawn only while every live weather with a positive weight has a
    validated cover of its own, with no memory of when a cover came; otherwise the whole sky is flat.
    Nothing is published while an active texture of either weather is still being read. A live weather
    that lacks a cover is composed at once, not at the interval, so a fade whose incoming cover is late
    is flat for a short while and then hands over. A composition that landed without the cover while a
    texture was refused is made again as soon as the refusal and the pending reads have cleared.
    One exception: while the incoming weather of a fade counts for at most 5 % and its first cover is still
    being composed (no texture refused, the outgoing one measured), the measured outgoing cover is taken
    whole for at most one real second (`CoverWait`, `CloudCoverOutput::contributors`), so a fade's start
    does not flash the flat layers. Nothing else depends on time: a paused game repeats its frame.
  - *The composition*: the shadows' published image is relative (divided by a clear reference), so while
    the volume is on the producer also composes the absolute product (`compose_parts` with a
    `CoverInput`), in a job slot of its own, at a fixed sampling phase (every layer's shift 0, so a
    weather's mean is a property of its textures and drift alone composes nothing again). The relative
    image stays byte for byte what it was, with its own due rule and game-clock interval; the cover
    recomposes on the real clock. With Cloud shadows off no relative image is made or published; with
    this row off nothing of the volume's is made.
  - *A weather with an active layer whose texture cannot give plain 2D alpha* (an RGB, L8 or 565 cloud
    texture of a mod, a cube map, a cut-off file, one in no archive: `volume_alpha_ok`, from the DDS
    header alone) gets no absolute cover at all, also while it is one side of a fade, and is shown flat.
  - **What a mod can change**: the amount (its cloud textures' alpha and the layers' weights), the
    colours (the layers' PNAM colours, the sunlight, the sky's colours and level), the drift (the layers'
    ONAM speeds and the wind). **What it cannot**: where the clouds stand and their shapes (a mod whose
    point is painted cloud forms loses them while this row is on; the row is off by default).
- **The domain** [engineering choice]. Its base is the cloud shadows' plane: 20,000 units above the
  ground reference, the camera's height (snapped to 1,024) when the worldspace's first composition is
  made, held until the worldspace changes. That suits a start on the ground (about 290 m up at 70
  units a metre: low for real clouds, but the cloud shadows on the ground and the clouds must agree, and
  `NVF_PLAY_DEBUG_CLOUD_SHADOW=<height>,...` moves both together); a start in the air puts the plane,
  and so the clouds, 20,000 units above that height. Above it cloud may occupy 24,000 units
  (`NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_HEIGHT=<16000..80000>`). The sizes that scale with the domain (the
  probe spacing, the placement's cells) are stated at 40,000 in the code (`REFERENCE_HEIGHT`).
  - *The bend*: the domain is bent down over the distance as a shell centred under the camera: a
    sample's height is raised by its horizontal distance from the camera squared over twice a radius
    `R`, chosen so the base meets a horizontal ray from the ground reference at 400,000 units
    (`R = 400,000² / (2 x 20,000) = 4,000,000`; `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_HORIZON=<units>`,
    100,000 to 1,500,000). From the ground a ray 2 degrees up enters the base at about 284,000 units, one
    1 degree up at about 336,000. The talk bounds its rays by a "cloud dome" the same way [documented:
    Bauer 2019, slide 50]; the radius is our own. A camera that walks moves the shell with it: 1,000
    units of walking shift the height 300,000 units away by about 75 units [design: accepted].
  - *The fade*: the clouds fade out by the distance where the ray enters the domain, from 0.75 of the
    horizon distance (300,000) to all of it, into the dome's own colour: whole down to about 1.7
    degrees, about two thirds at 1 degree, none at the horizon.
  - *The stretches*: the ray itself stays straight; the bend is in the density's height and the ray's
    span only (entry and exit from a cancellation-resistant quadratic). A ray that dips under the base
    and comes back far away has two stretches, each faded by its own entry; one that just clears the
    base is split at its lowest bent height, so its far part takes its own fade on either side of that
    tangency and a two-unit camera move cannot show a distant deck at once. The weighted distance is
    along the straight ray, each stretch weighted by its transmittance in front times its fade as its
    colour is, so a nearly faded far stretch cannot pull the reprojection distance. The march runs to
    the source's range, where a horizontal ray from the ground leaves the bent top, times 1.05.
- **The generated noise.** Two tileable 3D textures made by our own generator (`cloud_noise.rs`) on a
  thread of its own when the row is first switched on, deterministic, nothing shipped: a 128³ `R8Unorm`
  Perlin-Worley scalar for the base (Worley cells 16 / 32 / 64 across a 96,000-unit repeat, so 6,000-unit
  lobes, modulated by periodic gradient noise at 8 / 16 / 32 lattice cells; eight levels) and a 32³
  `Rgba8Unorm` set of three Worley bands for the erosion (3 / 6 / 12 cells across 12,000 units; six
  levels): 2.43 MiB with their level chains, uploaded once. Cellular noise after Worley (1996), gradient
  noise after Perlin (2002) [documented]; the code, layout and values are ours. Reads take an explicit
  level from the filter width, so far cloud reads coarse levels. The noise moves with the drift by a
  phase reduced on the CPU in f64 (bounded however far the drift goes). The base's middle half (its
  level-0 values from the 25th to the 75th percentile, 0.37 and 0.545) and each band's (0.365 and 0.615)
  are mapped linearly to 0.25..0.75, held to 0..1 [measured from the bake;
  `the_noise_constants_are_the_bakes`].
  Row off releases the images (and the medium its bind group and the noise's sampler); the finished
  bytes are kept at their exact length (the one exception to "off holds nothing"), so switching on again
  is an upload. One job is in flight at a time, shared by any number of quick toggles; a job that panics
  ends in a logged failure that the next activation retries.
- **Placement: masses from one field** [engineering choice]. Three octaves of lattice value noise in
  world x and z, not repeating (signed lattice cells, no period), weighted 0.40, 0.35 and 0.25, the
  second and third 2.17 and 4.73 times finer (not whole numbers, so the sum never repeats). The first
  octave has 1.41 cells a domain height at the defaults (`place_frequency`: about 17,000 units at 24,000)
  and the same wavelength in every weather, so a fade does not sweep distant formations. Each octave is
  moved by the producer's unwrapped drift (the visible-share-weighted wind displacement, cloud_shadows.rs
  `advance_drift`, as whole cells plus a fraction per octave), so the clouds travel with the weather's
  wind and layer speeds.
  - *The threshold*: a place holds cloud where the field passes a threshold set from the amount: the
    share of sky asked for is `1 - (1 - a)^2` (0.19 at amount 0.1, 0.36 at 0.2, 0.51 at 0.3, 0.75 at 0.5),
    read from the field's measured distribution (`PLACE_THRESHOLDS`: the CPU mirror over three areas of
    2,160,000 units square, which differ by up to 0.016; knots down to a share of 1/1024). From amount
    0.55 to 0.85 the threshold moves under the field's deepest trough, so a deck closes. What follows
    opens fair weather beyond that share.
  - *Two bases*: fair weather reads the field in one fixed diagonal basis, a storm in another; between
    amounts 0.55 and 0.72 the two covers are mixed, never the coordinates, so nothing sweeps
    [hypothesis: the code's reason]. Settled weather samples one.
  - *The banks* (`vc_m2_bank`, `vc_m2_regions`, `vc_m2_lane_noise`) [hypothesis, shaped from pictures]:
    pits inside a broad updraft are lifted to their spill point so they do not puncture its foot; moist
    lanes alternate with dry ones; finite humid regions end even long streets; in a humid air mass a
    strong updraft may carry a developed bank across several patches. The lanes and regions are gone
    between amounts 0.35 and 0.55 (`vc_m2_open_bank_weight`), where the gaps fill.
  - *Width*: an octave finer than the ray's footprint on the shell fades toward its mean and the field's
    spread is restored, so the share of sky stays the amount's. The footprint is the distance between
    neighbouring rays where they meet the shell, at grazing angles far wider than the transverse one (at
    the 2560x1440 buffer of scale 2 and side 2, one degree up from the ground, about 10,600 units; at
    scale 4 about 42,500). Where it no longer spans the first octave's cell twice the sample goes toward
    the expected cover of such a footprint, a uniform thin cover of the requested share one 1,200-unit
    sheet tall, at the weather table's extinction, the two optical densities mixed by the first octave's
    keep (`col.res`).
  - *Vigour*: a second slow field (cells of three domain heights) scales a column's height by
    `0.75 + 0.5 v`.
- **Cloud types** [engineering choice; Bauer 2019, slide 16: a strip of gradients over height per
  weather, blended across transitions (documented); ours are our own]. Three types, each with a profile
  over a column's own height (eight knots at h = 0, 1/7 .. 1, linear between):

  | Type | Profile (h = 0 .. 1) | Height, share of the domain | Relief | Transition, units | Extinction per unit | Erosion |
  |---|---|---|---|---|---|---|
  | sheet | 1, 1, 0.9, 0.7, 0.45, 0.2, 0.05, 0 | 0.11 | 0.25 | 600 | 1.5e-3 | 0.3 |
  | cumulus | 1, 1, 0.95, 0.85, 0.72, 0.55, 0.32, 0 | 0.14 | 0.75 | 240 | 2.5e-3 | 1 |
  | tower | 1, 1, 1, 0.97, 0.92, 0.85, 0.65, 0 | 0.48 | 0.6 | 300 | 3e-3 | 0.8 |

  The weather's mix comes from the amount: sheets `0.55 (1 - smoothstep(0.05, 0.3, a)) + 0.3
  smoothstep(0.55, 0.9, a)`, towers `0.3 smoothstep(0.35, 0.85, a)`, the cumulus the rest; plus the
  classification's bias, blended across a fade by the fade (pleasant toward cumulus, cloudy toward the
  deck and the tower, rainy toward the tower, snow toward the sheet; the bits add; unknown bits, and
  none, the amount alone); held non-negative and normalised. Nothing is keyed to a weather's identity.
  Locally the tower's weight is raised fourfold where a place is deep in a vigorous mass (the excess
  from 0.04 to 0.12, the vigour from 0.55 to 0.85) and none elsewhere. `..._TYPE=sheet|cumulus|tower`
  forces one type everywhere.
- **The body: the lean model** [design; the look judged by eye, 2026-11-23]. Functions `vc_m2_*`
  of the march, CPU mirror `volumetric_clouds_model2.rs`. Columns share the domain's base in the bent
  frame, so the bases line up toward the horizon. Density fades in over the lowest 300 units and
  can start higher in dry pockets. What the column gives (`vc_m2_column`, `vc_prepare_column`) depends on world
  x, z and the footprint alone: the placement's excess, the types, the vigour, the height.
  - *Cover and height*: the cover is a smooth step of the excess up to 0.12 (`vc_m2_cover`); the parent
    bank's height the types' share of the domain times the vigour's scale, up to 55 % more in a deep deck
    and 20 % more in a fair core, at least a sheet. Over the lowest 36 % of it (`VC_M2_FOOT`) the bank is
    whole; heads widen and stretch what is above.
  - *Fair lobes* (`vc_m2_lobes`; weight `PUFF` times the share that is not sheet, whole between amounts
    0.1 and 0.55, gone at 0.72): broad overlapping ellipsoids rising from the band
    (`vc_m2_lobe_field`, eight a base repeat), joined inside a strong updraft by a shallow core, their
    plumes' height following the local water. A signed field, the larger of the shelf and the heads, is
    displaced by the base noise at 2, 6, 13 and 27 times its frequency and the three erosion bands (the
    fine read itself displaced by the bands, `VC_M2_FRAY`); the density rises over a transition
    that widens with the filter, whole inside, with dilute fragments beyond the edge and a ragged lower
    surface where drier air condenses higher (`vc_m2_base_onset`). A narrow patch is kept shallow
    (`vc_m2_fair_profile`), so the masses are wider than tall (test
    `the_fair_masses_are_wider_than_tall`: at domains of 24,000 and 40,000 and amounts 0.15 and 0.29 the
    median width over height at least 1.7, no mass under 0.7).
  - *Isolated mixed banks* (Goodsprings Hazy): the sampling interval includes headroom for the lobes.
    Inside that interval, the sheet's crown tapers toward the placement boundary, and drier parts of its
    base condense higher. The sheet holds more water under dense lobes and near the center of the bank,
    allowing thin ends to fade in the view and in the shadows they cast. At grazing angles the projected
    sample can span several shoulders. Placement uses the texel that the temporal pass reconstructs,
    and the sheet's texture filter accounts for both that texel and the length of the step along the ray.
    As the projection across the layer grows long enough to span gaps, the view sample gives more weight
    to the rounded lobes for both density and shading height. It retains at least a tenth of the sheet
    contribution. The local sun probes and the light grid use the same physical mixture at every angle.
  - *Banks, decks and towers* (`vc_m2_density`): a signed field `F = cover x profile(h) - R (1 - n) - G D`
    whose zero is the surface: the envelope against the base noise `n` (relief `R` by type, times
    `SHAPE`), less the erosion's displacement `D` (world units, taken into field units through `G`, the
    base noise's measured gradient times `R`). The density rises to whole over the type's transition,
    never under two filter widths, so the silhouette is crisp and the inside dense. `D` is one erosion
    read at twice its texture's frequency (2,000 / 1,000 / 500-unit bands), each band moving the surface
    by up to 1.15 x `EROSION` of its cell (by type), signed, so it makes both heads and recesses; tears
    open where two bands disagree and keep a thin veil. A band finer than the filter fades to its mean.
    The storm deck's lower surface is displaced by its own relief (`vc_m2_deck_relief`) and noise.
  - *Wisps* (`vc_wisp`, mirror `volumetric_clouds_wisp.rs`): that erosion read is displaced sideways by
    `A (0.6 sin t0 + 0.4 sin t1, 0.6 sin t2 + 0.4 sin t3)`, `A = 300 x WISP` units, the phases from the base
    noise's coordinates and drift (whole cycles per repeat: no jump at a wrap, no motion against the body)
    and the height (wavelengths 6,000 / 3,700 / 5,300 / 3,300 units). Each component is at most `A`, the
    gradient's norm at most `A x 2.035e-3` (tested), and the read's width is stretched by it. After Bauer
    2019, slides 17-18 (the detail's lookup displaced) [documented]; the field is ours.
  - *Overcast undersides* [design]: near the base of a mixed deck, a second read of the base texture
    at three times its frequency breaks the broad scallops into smaller folds. The existing front and
    base noise bend those folds. Dry pockets rise farther into the bank, while nearby wetter patches
    reach the lower base. This relief drifts with the bank and fades upward. The view and sunlight
    sample the same density field and filter these folds at their respective footprints.
  - *Extinction and water*: the types' extinction per unit times `DENSITY`: 400 units of whole cumulus are
    one optical depth at `DENSITY` 1. Water varies inside a fair bank at the scale of a group of lobes and
    mottles its lower face (one base read of the column, two more there); a storm's columns carry between
    about a quarter and 2.4 times the type's extinction by their mass (`vc_m2_shape`). The light and the
    view take the same water.
  - *The weather table* (`EXTINCTION_TABLE`, at amounts 0.005, 0.05, 0.1, 0.2, 0.35, 0.55, 0.75, 0.95;
    linear between): 5.741, 3.775, 3.560, 2.732, 2.596, 2.709, 3.212, 4.344. The values are commit
    44e923c33's, solved by that commit's test so that its carved field's mean opacity straight up was `1 -
    (1 - 0.98 a)^1.25`; that field is gone and the numbers are kept as the unresolved sheet's extinction and
    the light's column under an overcast (test `the_extinction_table_is_the_accepted_one`). What the body
    gives per amount is printed by `lean_density_report`; it is judged by eye and not fitted to that target.
  - *The light's field* is the same column, body and water without the wisps, filtered to twice the wider
    of the footprint and the light volume's cell (`lcell.x`).
- **The light of a sample** (`vc_incident`, `vc_lean_shade`; mirror `lean_shade`) [design; terms after
  the cited publications]. Each step adds `T (1 - e^(-s dt)) S` and keeps `e^(-s dt)` of the background
  (albedo 1). The leaf module holds the phase, the ambients and the shading; it takes small structs of
  its own, never the march's uniform (the march imports it: the other way round would be circular), and
  a ray's angular part is prepared once (`vc_ray_light`).
  - *Direct*: two octaves (Bauer 2019, slides 28-29, after Wrenninge et al.; the factors one half each for
    the depth, the weight and the phase's eccentricity, weights normalised to 2/3 and 1/3): `w_i P_i
    exp(-0.5^i depth)`. The phase is two Henyey-Greenstein lobes (g 0.6 and -0.25 mixed 0.7 / 0.3),
    isotropic 1, capped at 4, and held from below by a back-scatter floor growing with the extinction,
    `min(extinction x 2,000 x BACKSCATTER, 0.5 x BACKSCATTER)` (slide 30; there `1/pi`, which is 4 in these
    units, the phase's cap, and clipped every dense lit sample [hypothesis: judged by eye in an A/B of
    2026-11-21, the development build 734673c09; its view and steps were not kept]). The powder
    term (Schneider and Vos 2015) takes up to half of the light of thin cloud seen away from the sun.
  - *Where the measured depth does not serve*: under a high sun the coarse cache misses part of the cloud
    over a lower face and averages heads smaller than its cell. At a fair bank's foot the path of the
    parent bank is taken, on its shoulders a short probe toward the light (`vc_m2_crown_depth`) and a
    smoother path for the scattered octave, in a closed deck the layer's own depth spread across
    neighbouring columns (`sheet_depth`), and at an open flank fewer paths survive two events
    [hypothesis each, judged from pictures].
  - *From above and below*: the sky (0.7 SkyUpper + 0.3 Horizon, gain 1.4) and the ground (reflectance 0.3
    [hypothesis: a dry desert's albedo, from memory] times the weather's Ambient plus its Sunlight by the
    sun's daylight and `1 - target opacity`, 35 % of it at the sky's hue), each with hemisphere weight one
    half and at half the designed gain (`LEAN_AMBIENT`: the designed ambient flattened the cloud
    [hypothesis: the A/B above]), each through its column as `1 / (1 + 0.655 t x OCCLUDE)`, the two-stream diffuse
    transmission's form (Bohren 1987 [hypothesis for the factor]). The column is the sample's own extinction
    over the estimated height, 60 % of it cloud, blended under a light 30 to 60 degrees high to the measured
    depth times the light's elevation sine (slide 36 reuses the directional sample for the ambient). At
    night the sunlight's part of the light from below is gone; the weather's Ambient stays.
  - *Colours are the weather's alone* [design]: the direct light's hue is the visible cloud
    layers' PNAM mean at the Sunlight's luminance, moved to the Sunlight colour by `SUNCOLOUR` (0: the PNAM
    hue). The flat layers are drawn in those colours, the look of lit cloud the weather's author chose for
    that hour: a volume lit in the Sunlight alone was a more saturated yellow (blue over red 0.60 to 0.66
    against the flat layers' 0.71 at clear noon, the baseline) [hypothesis: stills of 2026-11-20 with that
    day's density and light, the mean colour of their brightest tenth; of one pair the colours are kept,
    255/241/158 with the volume and 222/203/159 without; the start point and the view were not]. A low sun warms it and lifts it by up to 15 %; a weather with no visible cloud layer keeps
    the Sunlight colour. Test `the_lean_light_takes_the_weathers_colours`.
  - *The light's direction*: by day the sun's disc, by night the moon's, with the moon's share of the
    sun's strength as on the landscape; it grows from nothing at the horizon to full at 4 degrees.
  - *By moonlight* [design]: in many weathers the night Sunlight is far brighter than the cloud layers, which
    the flat sky draws nearly black, so clouds lit at its luminance glowed against the sky
    [observed: NVForever runs, 2026-11-25, Goodsprings at -71680,2048, 1920x1080, NVWastelandClear at 22:00, heading 80, pitch 22:
    the sky's 99th percentile 122 of 255 with the volumetric clouds, 22 with the flat layers]. Below the horizon the direct
    light and the light from below are each dimmed toward the visible layers' PNAM luminance by
    `min(1, L(cloud) / L(light))^q`, q `NIGHTPULL` (0.45; 0.5 would be the geometric mean of the two) times the night's
    share (0 at the horizon, 1 from 4 degrees under it, smooth). It only ever dims, keeps every hue, follows the weather's values alone (no weather named,
    no level fixed), and leaves the direct light of a weather whose night Sunlight is no brighter than its clouds as it was (the light from below has its own gain). With no
    visible cloud colour nothing is dimmed, and the dimming grows in over eight times that least luminance.
    The sky level, the moon's share and MoonlightNVSE.ini's factor are outside of it. The bound is taken of
    the undimmed light. It is part of Volumetric clouds, with Volumetric light on or off; without a moon the light from below
    dims alone. `MOONFORWARD` and `MOONBACK` can give the phase's forward lobe and back-scatter floor weights of their own by
    the same share: 1.6 and 0.1 as tuned, so the cloud facing the moon stays bright while its body darkens, and `MOONCAP` (9) is the phase's cap by
    the same share in place of the day's 4, so the cloud right at the moon can be the brightest thing in the sky (all three [design]: the contributor's pick from stills, 2026-11-25).
    The bound on the cloud light takes the larger of the two caps. By day and at the horizon none of it is computed (test
    `the_day_is_untouched_by_the_night_pull`, bit for bit).
  - *The bound*: the most a step emits per channel, computed in f64, sets one factor `min(1, 30,000 / B)` for
    every colour, so a mod's huge sky level stays inside the HDR target (`light_bound`).
  - *What is not modelled*: a third octave, after-sunset light from below, moon bounce; the column
    estimates are wrong at overhangs and towers beside sheets.
- **The light volume** [design; documented for the form: Kim and Neumann 2001, Bauer 2019, slides 33 and
  35]. The depth toward the light is kept on two nested lattices around the camera and read once a
  sample. Near: 128 x 128 nodes 500 apart (about ±31,500 units); far: 128 x 128 nodes 4,000 apart (about
  ±254,000); both a layer every 1,000 units up the domain (25 at 24,000), world-aligned in field
  coordinates (the camera's position plus the wind's displacement, snapped to whole nodes, so a node is
  the same piece of cloud frame to frame) with the bend inverted at each node. A texel holds the depth
  from its node to the bent top along the light, with no reach cut.
  - *The sweep* (`volumetric_clouds_lightvol.wesl`): top down, the march's own `vc_shape` (uneroded,
    footprint 64 near and 4,000 far) integrated to the next layer's bent height in steps of at most 250
    units (at most 64 of them: under a sun below about 3.6 degrees they are longer), plus the layer above
    read there. The lattice goes up to the domain's top. The near level takes the far one beyond its window
    through a guard band of four cells; the far one has nothing beyond. Each level is a 2D array of R16Float
    layers, one render pass a layer (no vendor patch, no device feature).
  - *Two sets a level* (3.12 MB in all): the front, which the trace reads, and the back, swept over 8 (near)
    and 32 (far) frames from ONE snapshot of the uniform taken when its sweep starts. When the last
    layer is written the sets swap and the trace is given that set's own window, so a set is never read
    through another's. The far set is promoted the frame a near sweep begins, so a near sweep reads one
    finished far set throughout. At the default spreads a set is read at most 16 (near) or 80 (far) frames
    after its snapshot, 0.27 and 1.33 s at 60 fps, the sun 0.03 and 0.14 degrees on at the default time
    scale. The guard band holds: running plus the fastest wind (600 units a second) move the camera 252
    and 1,259 units in those frames against bands of 2,000 and 16,000. Density evaluations a frame: 0.31,
    0.37 and 0.98 million at suns of 80, 45 and 15 degrees [design: the sweep's arithmetic for 25 layers].
  - *Jumps*: the light more than 0.5 degrees in one frame restarts the back sweeps; the light more than 3
    degrees from a front's, the sun to the moon, a weather or tuning input of the density moving more than
    5 % in one frame, the camera more than the guard band from the front's, or a sweep not drawn drop
    the front: the next level out, or the taps, take its place until a sweep finishes. A `DENSITY`,
    `SHAPE` or `EROSION` edit of any size restarts the sweeps.
  - *The reader* (`vc_light_handoff`, shader def `LIGHT_VOLUME`): the first two (`LIGHT_TAPS`; at least one) of
    eight intervals growing by half from the sample outward are marched through the light's field, so the
    folds right above still shadow sharply; then the near level is read at the handoff point, mixed over
    the far level through the near band, mixed over the remaining taps through the far band (marched only
    there, and where the handoff point is under the base). Without a ready level the trace runs its
    legacy variant: all eight taps over at most 24,000 units.
  - *Why*: without eight taps the trace's span fell from 5.19 to 2.30 ms (clear) and from 5.9-6.7 to
    1.9-2.0 ms (overcast), and they made the depth faceted [observed, 2026-11-20, a
    development build with the body and the dense march of that day: 2560x1440, Goodsprings at 13:00, `--still-sky`,
    `NVF_PLAY_DEBUG_CLOUD_PHASE=100`, clear and overcast, runs as built and with `vc_light` returning at
    once alternated, the trace's span of the periods after the view's turn; the facets judged by eye in
    the depth view of 653b07c02 from under the clouds]. Test `the_lean_light_volume_reader_against_a_fine_integral`: the direct light's transmittance
    against a fine integral to the top, the error at most 0.05 at the median and 0.25 at the 95th
    percentile, amounts 0.3 and 0.8, suns of 45 and 15 degrees.
  - Not built: a cross-fade between generations (the sun moves 0.05 degrees between two far swaps), a
    debug view of the volume.
- **The view march** (`vc_trace_refined`, `vc_march_refined`; CPU mirror `refined_march`) [engineering
  choice; the technique is Bauer 2019, slide 50 (a large base step, halved and moved back on meeting
  the base shape a fixed number of times, the small step kept inside cloud, with the risk of stepping
  over a thin cloud) and Schneider 2015 (cheaper samples far off) (documented); our own form and code].
  - *Support*: empty air is skipped by what is cheap to know (`vc_m2_support`: inside the column's
    height over the flat base, or the unresolved sheet; no texture). It holds every place the density can
    be above nothing (test `the_lean_support_holds_every_density_on_the_real_field`).
  - *Blocks*: the fine grid is fixed by the ray alone. Blocks run from each stretch's start, each of `n`
    cells of the fine step at the block's start, `n` the coarse spacing over it (1 to 16), the last cell
    clipped at the stretch's end; the start offset only moves each cell's sample, weighted by the cell's
    real length. The fine step is 400 units near (longer, up to two footprints, far off) but never longer
    than half the finest detail there: one base texel at the footprint's level, half a sheet, half the
    finest erosion band kept over the wisps' stretch, half the narrowest transition (120 units near); it
    never falls along the ray. `STEPS` scales the 400 (28 = 1). The fair lobes' two finest bands, at 13
    and 27 times the base read, are not in this bound: [hypothesis] view rays in motion may shimmer
    slightly there. Still repeat 0.12 of 255 with them, 0.13 without; moving-cloud change a second 1.62
    with them, 1.53 without (the baseline is the same build without the two bands) [hypothesis: captures of
    2026-11-23 at Goodsprings looking up (heading 13, pitch 56), a development build between 9a978a19a and
    c5a6c2993; the shader without the bands and the rest of the steps were not kept].
  - *Coarse spacing*: 800 units at amounts up to 0.1 and 1,600 above at a domain of 40,000, with the domain
    (480 and 960 at 24,000), twice that at Normal and Performance, widened by the placement's footprint
    over the finest octave's cell once that octave is gone.
  - *Probes*: the support is probed at each stretch's start and each block's end. A block is integrated
    whole where both its ends are inside or where it meets the base band (from the base to 1,800 units
    over it: every cloud has density there), from its entry cell where only its end is inside (bracketed
    by at most four halvings), to its exit cell where only its start is, and skipped where neither is.
    What can be missed is a whole chord between two probes above the base band, found exactly when a probe
    lands in it: 0.41 % of the mass chords at amount 0.05 and 0.45 % at 0.3 with 800 and 1,600, none at 0.8;
    at twice the spacing 1.55 % and 1.66 % [hypothesis: counted on a numerical mirror, not committed, of the
    placement before the banks, 96 horizontal lines of 1,000,000 units sampled every 100, the baseline every
    chord of that sampling; not repeated on the bank field].
  - *Strides and reuse*: inside a block whose both ends are inside, where the extinction is steady, up to
    `REFINED_STRIDE` (4) cells are merged into one sample (its optical depth and length bounded, the last
    cell of the block kept native); a light estimate may serve the immediately next sample of a like
    extinction and height within a short optical distance (`LIGHT_REUSE`). Light taps run only where
    the shape is above 0.02.
  - *Early ends*: 2 % of the background left; a stretch faded out at its entry is not marched; the second
    only while the first left enough.
  - *Budget*: at most 25,088 evaluations a ray whatever the settings: at most `(25,088 - 2) / (2 + taps) - 4`
    cells for both stretches, each at most one probe, one sample and its taps; past that every fine cell
    grows by the same factor, logged once per activation (`volumetric clouds: the refined march is capped
    at ...`, found on rays every 0.1 degree of elevation over the stretches the shader marches, looked for
    at the start, at a change of level, at a live edit of the aids its plan reads and at a new weather once
    its fade is over, from a buffer of the wanted size, never every frame).
  - *The ceiling clip*: under pure fair weather the stretches end at the tallest possible bank
    (`vc_sampling_ceiling`) instead of the domain's top, for a camera under it; past it there is no support
    and no density (test `the_stretch_past_the_sampling_ceiling_holds_nothing`). The light's taps
    (inside their reach, `reach.x`) and the producer's sweep still run to the domain's top, and find no
    density past the ceiling. The block the
    ceiling ends is clipped there, so its probes and lengths can differ slightly.
  - *The flat loop*: one loop whose turn evaluates one point, whatever it is for, so the column and the
    density have one call site each (test `the_flat_march_takes_the_refined_marchs_turns`, turn for turn
    against the mirror). That the compiled size and the registers live across the sites, not how often
    they run, set the cost is the explanation taken [hypothesis].
  - *The set-up of the cost figures of 2026-11-23*, here and in volumetric-medium.md: an RTX 4080 SUPER in
    a virtual machine, `--play --cell Goodsprings --pos=-71680,2048,8372 --hour 12.35 --weather <w>
    --still-sky --no-actors --no-messages --no-voice --graphics-only anisotropic,volumetric-clouds --size
    1920x1080 --log-frame-times 5` with `NVF_PLAY_DEBUG_CLOUD_PHASE=100`, the weather NVWastelandClear
    ("fair") or NVDLC02ZionValleyWeather ("Zion"); the view turned by `NVF_PLAY_TEST_LOOK` to a heading and
    pitch held 24 s, the first from 40 s: 60:20 ("mid"), 13:56 ("up") or 150:18; the median of the
    `nvf_medium_trace` span of the `gpu time:` lines from 9 s after a turn (30 s in the first view). Commits
    are of the development branch; before the levels the row was a switch at High's buffer and spacing.
  - *The cost it answers*, trace in ms [observed, 2026-11-23: that set-up, the shaders of c5a6c2993 edited to
    each variant, one run each; built as 35a37690e (one model), a52518357 (the flat loop) and ae4136eed (the
    clip); the levels' build 99c5df603 with `volumetric-clouds-high` gave 1.28 and 1.19 fair and 2.08 Zion,
    mid]. The baseline of every row is the first: the nested march compiled with three density models and
    the debug views:

    | Variant | fair, mid | fair, up | Zion, mid | Zion, up |
    |---|---|---|---|---|
    | nested, three models and views (the baseline) | 4.97 | 1.92 | 10.49 | 2.77 |
    | one model, no views, nested | 2.80 | 1.54 | 6.38 | 1.91 |
    | and the flat loop | 1.30 | 1.29 | 2.00 | 1.62 |
    | and the ceiling clip (what ships) | 1.23 | 1.17 | 2.00 | 1.55 |

    Stills of the flat loop against the one-model nested march, the baseline of that comparison, were
    within 0.02 to 0.12 of 255, a repeat's noise; with and without the clip likewise [observed, 2026-11-23: a
    still at the end of each view of the same runs, the mean difference over the sky, columns 0 to 1,499 of
    rows 60 to 619].
  - *Pipelines*: the trace (tiles and whole texel grid, with the light volume and the legacy one) has a view
    variant (shader def `CLOUD_VIEWS`, labels ending `_views`), the only one that reads the view's code and
    counts; it is asked for when a debug view is. Turned on live with neither of its traces built, the
    buffer is not written: the last image survives, the volume hands back to the flat layers (logged) until
    a whole draw is reported, then takes over again; back to `none` the plain one, if built, draws at once.
    A run without a view builds and draws the plain ones.
  - Test `the_lean_refined_march_matches_the_dense_reference`: the march with the shipped stride against a
    dense reference on the real field, grazing rays, thin weather, overcast and towers; the baseline of its
    tolerances is the dense reference's own half-step and phase differences.
- **Haze between the camera and the cloud** (`AIR`, 0.82; `vc_air_colour`) [design]. The covered share of a
  ray mixes toward diffuse light sampled from the current weather's dome (two elevations,
  volumetric_clouds_air.rs) by the opacity-weighted distance through air thinning with height; empty sky,
  coverage and reprojection stay as they are.
- **Ground shadows from the light volume** [design; Kim and Neumann 2001, "Opacity Shadow Maps", and
  Bauer 2019 slides 33 and 35, documented, for the form: the clouds' shadow looked up in a precomputed
  product toward the sun]. Cloud shadows (sun-visibility.md) alone take the weather's flat layers,
  composed on the 20,000-unit plane, which are not the clouds the volume draws. The light volume
  already holds the optical depth toward the light through the real field (slant and bend included),
  its layer zero at the slab floor, the shadows' plane bent with the horizon.
  - *The shared reader* (`volumetric_clouds_lightvol.rs` `gpu::LightVolumeReader`, the WESL module
    `nvforever::light_volume_reader`, whose header is the contract; CPU mirror `lvr_optical_depth`):
    one reader for every user outside the trace (the shadows and the light in the air now;
    reflections later). Per level it holds the front a reader may sample this frame, its window
    measured as the trace measures its front's (the camera's position in nodes from the front's
    corner under this frame's drift), the floor, the bend and the guard. The front is one whose
    every layer was encoded in an earlier frame and that survived this frame's drop checks
    (`LvSchedule::reader_bakes`), so no texel of it is written during the frame, whatever order the
    views draw in (the water reflection draws before the trace pass that sweeps) and if this frame's
    sweep fails; a front promoted this frame is read from the next. `lvr_optical_depth` moves a point
    under the bent floor along the light to it (exact: the slab holds no cloud under its floor) and
    reads a point above it at its own bent height (a mountain top: the depth from there, not from the
    floor); the near level is mixed over the far one through its guard band, the far one over nothing
    through its own, each times its presence; it returns the depth over the covered share and that
    share (0: no data). The trace's binding and reader are unchanged.
  - *The route*: the receivers sample the reader themselves (bindings 45 and 46 of the mesh view bind group,
    the sun visibility's own, and five vec4s more in its uniform), not a cached transmission map in the
    existing 2D binding: that map would still need window coordinates and an edge fallback in the receiver
    (the old binding repeats, which would tile it), a pass, and either two maps or one at the far level's
    resolution. Cost per lit fragment of a sun-lit view: one texture read where one level covers it (a
    receiver under the floor reads one layer), two in a guard band, plus the weather's one where the volume
    covers it in part or not at all; two a level for a receiver above the floor; one square root and one
    exponential. The layers' path alone, the baseline, is one read.
  - *The fragment texture budget* [design; counted in the code]: wgpu counts every sampled texture a pipeline layout shows the fragment stage,
    outside binding arrays, against `max_sampled_textures_per_shader_stage`. The fullest mesh view
    layout of a view with the two arrays holds 10 (shadow maps 2, depth and motion prepasses 2,
    the transmission texture every 3D camera has 1, the sun
    visibility 2 + 2, the ambient occlusion 1; no tonemapping table, for such views are HDR), and two more
    for a reflection cube on a device without binding arrays; the fullest material drawn holds 12 when not
    bindless (the NIF material's: StandardMaterial's 8 with `pbr_specular_textures`, its own 4; the folded
    terrain's 27 are drawn with bindless textures alone) and 10 when the
    world's materials are bindless (the sky's, which stays non-bindless). So the fullest pipeline
    needs 24 not bindless, 20 bindless. The
    arrays are bound only while the Volumetric clouds row is on with Cloud shadows (`volume_layout`),
    and when the fullest count (measured once, when they are first wanted, from bevy_pbr's own layout
    and the materials' derived layouts on the device) plus 4 fits the limit; otherwise the views
    take the weather-only layout (no bindings 45-46, no `NVF_CLOUD_VOLUME`: the shader compiles
    without the reader and K is the layers'), logged in one line `cloud shadows: ...`
    (sun_visibility.rs `CloudVolumeRoom`, tests `cloud_volume_room_decides_the_layout`,
    `the_weather_only_layout_leaves_out_bindings_45_and_46`, `shader_variants_are_valid_wgsl`).
    So the arrays need 28 not bindless, 24 bindless, the margin included; an adapter with 16 keeps the
    weather-only layout. Either row's switch asks for the other layout's pipelines, kept
    from then on, as the Cloud shadows row's switch does without the volume.
  - *K*: `1 - strength (1 - exp(-depth))` with the layers' strength (`STRENGTH`, the calibration aid
    included) and their low-sun band (sine 0.02 to 0.1), mixed over the layers' K by the covered
    share; where the volume has no data (beyond the far window, no front yet, the light volume off,
    the volumetric clouds off) the layers' K whole. The automatic source takes the volume only while
    the volumetric sky is shown (the medium's buffer written whole with its composing material
    ready, so the flat sky is hidden), its clouds are asked of the medium and a level is readable;
    otherwise the layers' K whole (sun_visibility.rs `AutoGate`, test
    `auto_source_follows_the_sky_the_player_sees`). A level that becomes readable fades in over one
    real second (`LVR_ARRIVAL_SECS`; counted in frames first, which at the few frames a second of a
    start took many seconds), so a front's arrival is no pop; a dropped front (a teleport, a
    weather jump) falls back at once. With the volume not in use the uniform is the packed one and
    the shader's first statement hands K to the layers' function, word for word: the
    picture is theirs alone. Not relative: the layers' K is divided by a percentile of its own map so an
    overcast does not dim the whole scene; the volume's is absolute, so under a full overcast the
    ground takes the whole strength [engineering choice].
  - *Where each level serves* [arithmetic, not looked at]: a receiver at the ground reference reads
    the bent floor toward the sun, about 20,000 / tan(elevation) units away while that is short (29,500
    at 34 degrees), less and less than that for a lower sun. The near window covers about 31,500 units
    each way (2,000 of them guard), so it serves the ground around the camera whole only with the sun above
    about 34 degrees, in part to about 32; below, the far level (cells of 4,000 units, its light filtered to 8,000) gives
    the shadow next to the player too, a broad soft dimming rather than a cloud's outline. The far
    window (about 252,000 each way, 16,000 guard) reaches the camera's ground much lower than a flat floor
    would have it, since the floor bends down to meet the ray (`lvr_rise`, R = 4,000,000: at 4.9 degrees
    it is met 184,000 units away, inside the guard): whole down to about 3.2 degrees, where the low-sun band
    leaves about 41 % of the shadow, and gone at about 2.7, where it leaves 28 %. Between the two the
    hand-over to the layers sweeps across the scene (smooth, no seam, but the pattern changes).
    A start above the ground moves all of this up (the ground reference is the first output's camera
    height rounded to 1,024, cloud_shadows.rs `GROUND_SNAP`): the floor is then 20,000 units over that
    height, so its point lies farther sunward for a receiver on the ground.
  - *In motion*: the levels are addressed in the field's coordinates under this frame's drift, so the
    shadows move with the wind every frame and need no reprojection; a swap of fronts (every 8 frames
    near, 32 to 40 far) changes them only by the sun's progress, a weather fade's and the bend's tilt
    since the snapshot, and moves the guard bands by whole cells (the window is re-snapped per bake).
  - Developer aid (not in release builds, below) `NVF_PLAY_DEBUG_CLOUD_SHADOWS_SOURCE=weather|volume|auto|share|depth` (volumetric-medium.md,
    "Debug aids"); log line `cloud shadows: source ...` when the source in use changes while the
    Volumetric clouds row is on, and when it goes off. Smoke: `volumetric-clouds-shadows`. Tests:
    `volumetric_clouds_lightvol.rs` `reader_tests` (the bookkeeping, the windows against the trace's,
    the projection, the hand-over, the fallback, the module's text and validity) and
    `sun_visibility.rs` (`cloud_volume_off_is_todays_cloud`, `cloud_volume_hands_over_to_the_weather`,
    `with_the_volumetric_clouds_off_the_cloud_shadows_are_the_weathers_alone`, the variants linked with the reader).
- **#3032 stage 3: the light in the air** (sunlight seen in the air: shafts where it is lit, shade
  behind ridges, buildings and clouds; the whole account is docs/design/volumetric-medium.md, "The
  near grid and the light in the air"). An engineering choice for the enhanced look, not a fact
  about the original game, never a parity item; off by default and in every preset, with or
  without this section's row. Switched by the row "Volumetric light (experimental)"
  (`volumetric-light`) with its strength slider (`volumetric-light-strength`, 0 to 200 %, one
  overall scale of the applied correction), or the developer aid `NVF_PLAY_DEBUG_MEDIUM_LIGHT=<strength>`
  (not in release builds, §3.23);
  the screen-space Sun shafts skip their pass on a view in any frame in which the light's
  composite drew there (decided in the render world), and are greyed on the page. Code: `medium_air.rs`,
  `medium_air_demand.rs`, `medium_air_light.rs`, `medium_air_grid.rs` and `.wesl`,
  `medium_air_filter.wesl`, `medium_air_compose.rs` and `.wesl`.
  - *Technique*: the talk's near representation [documented: Bauer 2019, slides 42 and 47; Wronski
    2014]: a froxel grid of 160 x 88 x 64 at 16:9 to 48,000 units of view depth (exponential slices
    from a first of 50 units), marched front to back per column (on discrete NVIDIA cards in four
    chunks of slices at once, their sums joined) by a compute pass with its own bind group after the main pass, four dithered
    visibility samples a froxel (`V = K x min(C, T)`: the sun's cascades as Bevy picks them, the terrain map read as surfaces read it, the clouds' shadow
    from the surfaces' own source: the volumetric clouds' light volume, else the Cloud shadows
    image), filtered 3 x 3 within each slice, then composed by a fullscreen pass before the early
    post-processing (so before FSR, DLSS, bloom, eye adaptation, tonemapping and TAA). The medium
    is the weather's own fog: each froxel stores the fog share's steps and their visibility-weighted
    sum, and the light is a re-shading of the fog's own light in the fog's display-value blend, with
    the fog colour as the budget near and the pixel's own colour on the sky and on land the fog covers
    (slide 62), shadow never taking more than the share the fog gave, a gain never lifting a bright sky
    pixel past white, every result held inside the HDR target's range.
  - *What follows the weather*: the fog's share curve and colour from the camera's fog each frame,
    the sunlit share from the sun and ambient lights as sky.rs sets them, weakened under a closed deck by
    what the clouds let through of a light from above (Cloud shadows on), the clouds' mean shadow
    as the reference the rays are measured against; a fog the light cannot read, a fog not blended
    in display values, night in a climate without a moon, interiors and under water are shown the plain way. By
    night the light is the moon's, the weather's night sunlight from the opposite side. The look's
    constants (ray contrast, lift, shade limit, phase floor, sky weights) are artistic choices made
    from pictures, all live in a tune file (`NVF_PLAY_DEBUG_MEDIUM_LIGHT_TUNE`, a
    developer aid too); the ends of the fog blend, the deck's share and the bright hold are constants.
  - *Cost so far*: volumetric-medium.md, "Cost and memory" (the grid pass's two walks, measured on two
    GPUs); 6.9 MiB a volume, two with the filter. Not
    done: a temporal filter, the cost of the light volume's reads, the moon, local lights, FSR's reactive
    mask and the jitter offset, a whole-frame time against a frame without the light. Smoke:
    `volumetric-light`, `-taa`, `-fsr`, `-night`, `-off`, `-row`, `-clouds`.
- **What follows the weather** (from `SkyState` and the cover every frame, through weather
  fades: no weather names, no per-weather result kept): the amount (so the threshold, the types and the
  share of sky move smoothly through a fade), the drift (the layers' ONAM speeds along the wind, as the
  shadows'), the cloud colours and their weights (both sets, by the fade; a fade that starts or turns
  round before the next composition takes the colours by form ID), the sunlight, sky and Ambient
  colours, the sky level, the hour and the sun's direction. A weather with no cloud texture has no
  cover (the producer says there is none to come, `CloudCoverEmpty`), and one whose layers are all transparent (the clear night's placeholders) an empty cover (mean
  0): in both nothing is drawn or marched (the cover view still draws) and the flat layers are as on
  main. Values outside the base game's ranges are tested as data (NaN, negative and huge colours, sky
  level, density, steps, offsets): every uniform value finite, every colour non-negative.
  The uniform is written when a value has moved by more than its tolerance (`needs_write`: the placement's
  and the noise's exactly, the light's colours by a thousandth). PNAM reaches it through the direct light's
  hue and the bound, so a change of the cloud colour too small to move either waits for the next write. The
  cover is written again when its unwrapped drift has moved by 1/4096 of a repeat, or when another of its values has changed.
- **Off is as on main**: the picture, the log, and each frame's work but for the few checks below. No
  mesh, no material, no texture, no pipeline; the material type's systems wait behind a
  closed gate (the vendored `MaterialGate`, §3.14), closed again three frames after the last
  volume material went, and the medium's render systems behind their own; the producer is not started
  for it; the flat layers draw as before. The volume's own systems run only while the row is on or
  something of it is left to take down (`follow_volume`, `trace_above_water`, `show_volume`,
  `follow_medium`; `follow_noise` until no job is in flight), while it is drawn (`hide_flat_layers`),
  and while the cover is wanted or a word of it still published (`publish_cloud_cover`). Indoors, under
  water and with no cover nothing is drawn and the flat layers show. What is left with the row off:
  - The row: on the settings page and in the launcher, and its key in the settings file, written with the
    others like any row's.
  - Each frame those run conditions (a few resource reads); the loading screen's two cloud fields kept
    false (`hold_loading_screen`; the render world's link is not read); `underwater_fog` notes whether the
    camera is under water; `sky::fade_cadence` finds the row off. A weather fade then
    moves with the sky's half-minute steps alone, as without the row; with every clock reading only while
    the volume is drawn, and a switch between two steps moves nothing: the next step gives the fade the
    time it has not had (`SkyState::follow_game_clock`).
  - Render layers with nothing on them: the volume's on the main camera, the flat clouds' on the mirror's,
    the lens's and the extra cameras'.
  - The sky material's uniform is a vector longer (the fair sun's disc, zero) and sky.wesl tests it.
  - The shaders' sources are registered at the start; none is compiled.
  - With Cloud shadows or Terrain shadows on, the buffer of their uniform is six vectors (96 bytes) longer,
    zero and outside the shader's struct, and the views' component holds two texture views more, not bound;
    layouts, pipelines and the shader's text are as without the row. (The buffer keeps one size, for it is
    bound in the views' groups, and one type, for the light's other readers take it whole.) The shared
    reader's run condition finds no light volume.
  - With Cloud shadows on, the cloud textures are read as without the row (the volume's look through a
    texture's alpha waits until it wants the cover, and then reads the file once more); a colour texture's
    shape is checked before it is decoded, not after.
  - Once it has been on in the run: the noise's finished bytes (2.43 MiB), the pipelines asked for (kept,
    and used again by the next activation: the pipeline cache tells no second request from a new one) and
    what is known of the cloud textures read then. Textures, buffers and bind groups go.
- **Reflected clouds** [design]. An active water mirror traces the same cloud field from its reflected
  eye into a small image. The sky dome composites it behind the reflected landscape and the water
  material samples that image of the scene through its usual ripples. Looking down can reflect clouds
  above the main camera's field of view. The mirror keeps the weather's flat layers until its own
  trace and dome material are ready. The scope and extra cameras still see the flat layers.
- **Switching**: the row, `--graphics-only volumetric-clouds`, `NVF_PLAY_TEST_SETTINGS=<t>:
  volumetric-clouds=<choice>`. Log lines (`volumetric clouds: ...`):
  - `noise generated: base 128x128x128 R8 with 8 levels, erosion 32x32x32 RGBA8 with 6 levels, ...` once a
    run; `making the noise textures failed (...)`.
  - `level <name> (far buffer scale <s>, probe spacing x<f>)` at each activation and change of level.
  - At the first activation: `start: SHAPE ..., EROSION ..., DENSITY x... (start defaults: ...)`,
    `start-only: HEIGHT ..., PUFF ..., AIR ..., LIGHT_TAPS ..., REFINED_STRIDE ..., LIGHT_REUSE ...`,
    `body: one flat base for every column; ...; WISP <w> (...)`, `march: the refined march (...)`,
    `light: two octaves (...); weights: <those off 1>`.
  - `on, slab <base>..<top> (the cloud shadows' plane), quality <q> (the refined march: the support probed
    every <n> units, ...), ... placed by our own field (amount <a>: sky share <s>, threshold <t>, cells <c> a
    repeat), types ..., bent to meet the horizon at <H> ..., traced into the medium's far buffer, ...` when it
    is first drawn.
  - `none (<reason>)` once per reason nothing is drawn (`no cloud cover` until the first composition,
    `a weather without cloud layers`, `indoors`, `a clear sky: the cover is empty`, `the camera is under water`, `a cloud texture without plain
    2D alpha: the weather is shown flat`, `a weather value the volume cannot use (not a finite number): the
    weather is shown flat`); `cover <mean> (<weather>)` when the mean moves by a tenth or the weather
    changes; `off`, then `idle, materials released`.
  - `light volume on (<levels>; near 128x128x25 at 500 across, far ...; <MB> MB in R16Float (two sets a
    level); ...; sweeps over 8 and 32 frames; 2 near taps; ...)`, `light volume: <k> near taps now
    (LIGHT_TAPS <asked>)`, `light volume sets dropped (<why>); ...` (a jump, never per frame), `light volume
    off (textures released)`; `volumetric medium: the trace read a completed near set of the light volume`
    (and `far`), once a run each, when its trace first drew with one. GPU spans `nvf_cloud_light_far`, `nvf_cloud_light_near`, `nvf_medium_trace`.
  - The producer's own `cloud shadows: layers ...` line comes with the volume alone too.
- **Debug aids** (variables `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_<KEY>`, `..._CLOUDS=<0..4>` for `DENSITY`;
  read once at the start; they do not make a run unattended). Developer aids: they exist only in tests, in
  debug builds and in a build with the `dev-aids` feature (engine/nvforever/src/dev_aids.rs; in engine/:
  `cargo build --release -p nvforever --features dev-aids`). A release build without `dev-aids` reads none of them, has no tune
  file follower and starts from the values below; nor does it know `--preview-weather <EDID>` (with `--play
  --weather`: a fade to that weather and back), which it answers as any unknown option. The start values (`START_VALUES`,
  `START_LIGHT`, `START_WISP`; test `a_default_run_is_the_accepted_launch`) are the look judged by eye on
  2026-11-23:

  | Key | Starts at | Range and meaning |
  |---|---|---|
  | `SHAPE`, `EROSION`, `DENSITY` | 1, 1.26, 0.76 | 0..4, 1 as designed; `SHAPE` is drawn at 0.01 at least: 0 and every value under 0.01 draw as 0.01 (no plain envelope) |
  | `WISP` | 0.52 | 0..4; 0 skips the displacement |
  | `OCTAVE`, `BACKSCATTER`, `FORWARD` | 1, 1.65, 1 | 0..4, 1 as designed |
  | `UPPER`, `LOWER`, `GROUND`, `OCCLUDE` | 1, 1, 1, 1 | 0..4; the light from above, below, the reflectance, the ambient's fall |
  | `SUN`, `LEVEL`, `POWDER` | 1.3, 1.12, 1 | 0..4; the direct light, one level on the whole, the powder |
  | `SUNCOLOUR` (alias `SUNCOLOR`) | 0 | 0..1; the Sunlight's share in the direct hue |
  | `NIGHTPULL` | 0.45 | 0..2; the night's pull toward the cloud layers' luminance (0 none, 1 that luminance) |
  | `MOONFORWARD`, `MOONBACK` | 1.6, 0.1 | 0..4; `FORWARD` and `BACKSCATTER` by moonlight |
  | `MOONCAP` | 9 | 0..16; the phase's cap by moonlight (the day's is 4) |
  | `STEPS`, `TYPE`, `VIEW` | 28, none, none | 4..128; a type's name; a view's name |
  | `HEIGHT`, `HORIZON` (start-only) | 24,000, 400,000 | the domain |
  | `PUFF`, `AIR` (start-only) | 1, 0.82 | the fair lobes' weight, the haze |
  | `LIGHT_TAPS`, `REFINED_STRIDE`, `LIGHT_REUSE` (start-only) | 2, 4, 1 | 0..8 near taps (8 the legacy path); 1, 2 or 4; 0 or 1 |
  | `LIGHT_VOLUME`, `..._CELLS`, `..._SPREAD` (start-only) | both, 500,4000, 8,32 | off, near, far or both; spacings; frames a sweep takes (1..256) |

  - *Live tuning*: `..._TUNE=<file>` names a text file of `NAME=value` lines (`#` comments; short names or the
    variables' full names, any case; a later line wins), looked at at most every 0.5 s of real time while
    the row is on (volumetric_clouds_tune.rs). The start-only keys are refused in it; a removed key goes back
    to its start value; a bad line is ignored with one log line; a file that cannot be read keeps the last
    good values; each change of the effective values drops the medium's history once and is logged (`tune
    file <file>: ... (changed from the start: <names>)`).
  - *Views* (`VIEW`): `cover` the placement against its threshold and the cover, `vigour` the column's
    height as a share of the domain, `noise` the generated textures on the bent base (bands one base
    repeat wide: grey the base, colour the erosion), all three drawn on the bent base; `count` the work of a
    ray (red the probes, white at 512, green the fine samples at 256, blue the light taps at 2,048);
    `sun`, `ambient` the direct light or the ambients alone; `depth` the optical depth toward the light
    (grey `0.9 log2(1 + depth) / 6`, white from 63, magenta where the light was skipped); `opacity`;
    `body-height` the height share the light is given. Read them in the reference mode
    (`NVF_PLAY_DEBUG_MEDIUM=reference`) for figures: the default mode blends them with the history.
  - Cloud shadows' `NVF_PLAY_DEBUG_CLOUD_SHADOW` (plane, repeat) and `NVF_PLAY_DEBUG_CLOUD_PHASE` (a frozen
    drift, for pictures) apply to the volume too; the medium's own aids are in §6.
  - Smoke: `volumetric-clouds`, `-light-volume`, `-taa`, `-fsr`, `-live-change`, `-level-change`, `-no-aids` (a release build
    ignores the aids); `-light-model`, `-taa-turn`, `-fsr-turn` with `NVF_SMOKE_DEV_AIDS` set (an exe built
    with the feature).
- **The sun over the fair volume sky** [design; judged by eye from stills and a play test, 2026-11-24] (sky.rs
  `FairSun`, sky.wesl `sun_texel`), by the fair weather's weight (`fair_daylight_weight`; the disc by the
  same weight without the sun's elevation, so it stays to the horizon): the climate's sun texture is spread
  over 11 degrees at 32 degrees of elevation and above, widening to the plain sky's 20 at 14 and below; its
  glare over 100 degrees at 0.75 (the plain sky's: 130 at 1, which washed the whole fair sky beige, judged by eye). The
  texture's bright middle is 16 of 512 texels across [observed, 2026-11-24: `textures\sky\sun.dds` of the
  Steam game's `Fallout - Textures2.bsa`, extracted with `fnvtool bsa extract` and decoded, the mean of alpha
  over rings of one texel around the centre: 255 to 4 texels, 135 at 8, 109 at 12, falling to 0 at 252],
  a third of a degree at 11; it is drawn as a disc 1.5 degrees wide, and the rest of the texture at 0.6, so
  that the disc shows against its halo. The warp reads the middle enlarged and eases back to the texture's
  own scale outside it. Without the fair volume sky the sun is as on main.
  `NVF_PLAY_DEBUG_FAIR_SUN=size=..,glare=..,gain=..,lowsize=..,lowglare=..,lowgain=..,from=..,to=..,disc=..,halo=..`
  sets any of them, for pictures (a developer aid, as above).
- **Not done**: clouds in front of the world; the clouds' own texture colours (only their alpha reaches
  the CPU, so the PNAM colours alone colour the volume, as if the textures were white); the lightning
  flash and the sun's glare on the clouds (the flat layers drew the glare; the dome still does). Which
  layer is a cloud: every drawn cloud layer (a texture that loaded and a shape). The base game's one
  starfield layer (`sky\NVWastelandStarfieldSky.dds`, the night weather's layer 0) is in no archive and
  so never drawn (docs/spec-notes/world.md §6), but a mod that ships a starfield on a cloud layer would
  be taken for cloud; no classifier was invented. The visible shares order the layers by the dome's sort
  ranks (`2 x layer + set`) alone, while Bevy's transparent sort also takes each shape's bounding-box
  centre, so with some camera pitches two overlapping flat layers can be drawn the other way round than
  the shares assume (a wrong tint and drift share where two opaque layers overlap). The absolute
  composition samples every layer once and runs the transmission product twice, up to about 10 MiB of
  working memory at eight layers on the compute pool; folding both into one pass is not done.

### 3.25 Physical lighting (`physical-pbr`; off in every preset, #3827)

**Carried over** from #3828's branch (its split, part 5, #4199): the light loop, lamps, fog, the sky and its image-based light, exposure and tone, bloom and the materials below. The rest of that branch's physical work comes in its own pull requests, each stacked on this one: the ambient occlusion inside the physical light loop (part 2, #4287; until then the occlusion stays on the picture under this row, logged once), the canopy shading (part 3b, #4266), and skin, eyes and hair (part 6, #4262, §3.26). Not measured or pictured yet.

**With the volumetric rows** [design: agreed with the volumetric work, 2026-11-26]: Volumetric
clouds and Volumetric light run under Physical lighting. The clouds are lit as the physical
dome they lie on is drawn, by the surfaces' sun colour and the physical sky's colours, in
linear light, and go to the tonemapper without its inverse (sky.rs `physical_cloud_light`,
volumetric_clouds.rs `compose_uniform`). Their sun has the ground's gain (`PHYSICAL_SUN_GAIN`,
by night `PHYSICAL_MOON_GAIN`): without it they were lit by about a seventh of the ground's
sun and came out grey against the sky [observed: Goodsprings at 17:30, 2026-11-26]. Their
direct light takes the physical sun's own colour whole (`PHYSICAL_SUN_COLOUR_SHARE` 1, the
`SUNCOLOUR` aid's value under this row), not the weather's cloud colours at its luminance,
and their light from below is the dome's mean of overhead and horizon, not the weather's
Ambient [design]. Volumetric light's air composite still reads the
picture as display values (medium_air_compose.wesl); how it meets linear light is open with
its author.

**With authored reflections** (#2981; a material pack and its Reflections row on) [design:
spooknik's review of #3828, which switched them off]: Bevy gives a view one environment map,
and both want it. In such a run the reflection cube keeps it: authored materials keep their
cube, `metal_gain` and room fill through the reflection gate, which goes in after the
physical light loop (lights/reflection_gate.rs `PHYSICAL_GATE_EDITS`), and every other
surface takes the light loop's flat physical ambient in place of the sky's image-based light
(physical_ibl.rs makes none in such a run). Without a pack the sky's light reaches every
surface. Two cubes at once (the sky's for all, the room's for authored materials) are a
follow-up.

[design, #3827] An addition of NVForever's own for the enhanced look: not a fact about the
original game, never a parity item, and never to be used to explain a difference from the
original's frames. Off in both presets; with every row below off, the picture is the
display-value lighting of world.md §8 unchanged. Every value in the table below is an
engineering choice tuned by eye on this machine (`[design]`) unless the row says otherwise.

**Physical PBR lighting** (`physical-pbr`, Graphics > Lighting and Effects; read once at
the start, so a change applies at the next start; in a build with the developer aids,
`NVF_PHYSICAL_PBR=1` / `0` overrides the row, settings.rs `physical_pbr_after_ini`).
The display-value lighting sum (world.md §8) is replaced by linear lighting:

- *Diffuse*: the energy-preserving Oren-Nayar model (EON) [documented: Portsmouth, Kutz
  and Hill, "EON: A practical energy-preserving rough diffuse BRDF", JCGT 2025; the
  OpenPBR surface specification's diffuse]. *Specular*: Cook-Torrance with the GGX
  distribution [documented: Walter et al. 2007, "Microfacet Models for Refraction through
  Rough Surfaces"] and Bevy's Schlick Fresnel; the perceptual roughness floor is 0.045,
  Filament's desktop figure, in place of Bevy's mobile 0.089 [documented: Filament's
  material documentation]. In the display-value mode the highlight keeps F = F0 in world
  views (#2266, `lights.rs` `FRESNEL_EDIT`); physical PBR takes Bevy's Fresnel on
  purpose. Code: `lights/display_shader.rs` (`lighting_patched`, `oren_nayar_patched`).
- *Lamps*: inverse-square falloff with a finite bulb core and a smooth window at the
  light's radius, `w² · 0.22 / (u + 0.09)` with `u = (d / R)²` and
  `w = 1 − u²` [design: the inverse-square law with a core and window after Karis 2013,
  "Real Shading in Unreal Engine 4"; the 0.22 and 0.09 put a lamp's light at about 0.35 R
  where the display-value falloff had it in the rooms checked]. Light colours are decoded
  from sRGB to linear light (`PHYSICAL_SPECULAR_EDITS`).
- *Sky and ambient*: Bevy's flat ambient is replaced in world views by an image-based
  light from cube maps built each time the sky changes (`physical_ibl.rs`): outdoors the
  sky dome's zenith, middle and horizon colours, a sunlit ground below and the sun's disc
  and aureole for the specular; indoors the cell's ambient as ceiling and floor and its
  directional colour as a lobe.
- *Sky*: a single-scattering atmosphere (Rayleigh, Mie and ozone) for the sky's colour and
  the sun's light (`sky.rs`, `sky.wesl`) [documented: Bruneton and Neyret 2008, "Precomputed
  Atmospheric Scattering"; Hillaire 2020, "A Scalable and Production Ready Sky and
  Atmosphere Rendering Technique", for the model and the Earth and atmosphere radii].
- *Fog*: the game's fog (fog.rs), with the Desert haze and Aerial perspective rows over it
  as in the display-value look, and with Volumetric light its air [design: agreed with the
  volumetric work, 2026-11-26]; outdoors its colour is the physical dome's horizon
  (sky.rs `physical_horizon_color`), so far land fades into the sky behind it.
- *Exposure and tone*: the world view keeps Bevy's Tony McMapface curve in place of the
  display-value clip (lights.rs `physical_tony`), the curve the volumetric clouds and the
  air are matched to [design: agreed with the volumetric work, 2026-11-26; a choice of
  tonemapper and editable presets for every view is #4690]; outdoors the weather's own grade
  (tint, contrast, brightness) is set aside so it is not laid over linear light twice, while
  the game's image-space modifiers still apply (imagespace.rs `effective_grade_base`). The
  eye adaptation meters both ways (lifting dark scenes and darkening bright ones) with its
  own limits (`eye_adapt.rs`), and runs with the row even when Glow is off. The view is HDR under the row.
- *Bloom*: with Glow on, the bloom takes the HDR picture's linear light (post.rs
  `new_with_hdr_glow`).
- *Materials* (nif_material.wesl and .rs `NVF_PHYSICAL_PBR`, terrain.wesl): the shade light
  is mixed in linear light and returned as such; a shape's hot (emissive) light is added
  on top; the environment map is weighted by its Fresnel term; blended unlit colours are
  scaled before the fog and multiply decals fade with it. A normal-mapped shape without the
  specular flag takes a faint reflectance (0.32, F0 about 0.016) and a roughness of at
  least 0.86, so it never reads as polished (nif_spawn.rs `specular`); an authored set's
  metallic channel counts. #3828's branch changed the shade light's return, the unlit
  colours' place before the fog and the decals' fade for every view; here they are under
  this row only, so the default look is unchanged.
- The weather sources' daylight brightness (the window light rays' weather regions) is kept
  for the indoor image-based light's day lift (`emittance.rs` `weather_daylight_luma`); the
  rays themselves are drawn as in the display-value look.

- *Canopy shading* (part 3b, #4266): SpeedTree leaf normals form a dome
  around the crown's lowered core (35 % of its height): out from that core, with an upward
  part of three quarters of the leaf's height above it plus a quarter of the crown's reach
  (fnv-spt `canopy_normals`, applied in tree.rs only under this row), so a crown is lit as one
  rounded mass instead of card by card [design]. Without the row trees keep the
  original-look rule, out of the crown and tilted up 45° (speedtree.md §6). Under the
  row too (`NVF_CANOPY`, nvf_assets `enable_foliage_canopy`) [design, tuned by eye on
  #3828's branch]:
  - leaf cards darkest at the crown's core (0.60 of their colour) out to whole at its rim
    (tree.rs), their normals bent out toward the card's corners (tree.wesl);
  - grass tufts' normals from the ground's at the root to radial at the tips (grass.rs
    `grass_canopy_normal`), leaning with the wind; each plant's brightness and hue varied
    by its foot; the root darker (occlusion 0.32, colour 0.68 at the foot); and the sun's
    whole light in place of grass.md §5's weights (grass.wesl).

  Still to come: canopy normals on placed plants, which need the foliage detection of
  §3.24 (#4193).

| Constant | Value | Tag and basis |
|---|---|---|
| `PHYSICAL_ATTENUATION` (display_shader.rs) | `w² · 0.22 / (u + 0.09)` | [design] core of 0.30 R, matched at 0.35 R |
| roughness floor (`ROUGHNESS_FLOOR_EDIT`) | 0.045 | [documented: Filament] |
| unflagged shapes' reflectance and roughness (nif_spawn.rs) | 0.32, at least 0.86 | [design] |
| `PHYSICAL_SUN_GAIN`, `PHYSICAL_MOON_GAIN`, `PHYSICAL_INTERIOR_DIR_GAIN` (sky.rs) | 6.8, 1.2, 0.85 | [design] the display-value shares (world.md §8) carried into linear light |
| `ATMO_RE`, `ATMO_RA` (sky.rs, sky.wesl) | 6360, 6460 km | [documented: Bruneton and Neyret 2008; Hillaire 2020] |
| `ATMO_H0` | 1.1 km | [hypothesis] the Mojave floor's height above the sea |
| `ATMO_HR`, `ATMO_HM` | 8.0, 1.0 km | [design] Rayleigh's usual 8 km scale height [documented: Bruneton and Neyret 2008]; Mie lowered from their 1.2 km for desert air |
| `BETA_R`, `BETA_O` | (4.60, 13.00, 35.50)·10⁻³, (1.10, 2.35, 0.06)·10⁻³ per km | [design] tuned from the published sea-level coefficients for the desert's colour |
| `ATMO_SUN_IRRADIANCE` | 8.2 | [design] |
| `OUTDOOR_IBL_SCALE`, `INDOOR_IBL_SCALE` (physical_ibl.rs) | 0.62, 0.45 | [design] |
| cube map sizes (physical_ibl.rs) | 16 (diffuse), 32 with 5 mips (specular), 16-step sky table | [design] cost |
| `PHYSICAL_LOW_REF`, `PHYSICAL_HIGH_REF` (eye_adapt.rs) | −6.0, −1.2 EV | [design] |
| `PHYSICAL_LIFT_SLOPE`, `PHYSICAL_MAX_LIFT`, `PHYSICAL_DARKEN_SLOPE`, `PHYSICAL_MAX_DARKEN` | 0.68, 2.6, 0.45, 1.4 | [design] |
| `PHYSICAL_METER_FILTER` | 10-98 % | [design] the display-value meter's 10-100 % (eye_adapt.rs `METER_FILTER`) less the brightest 2 % |

- **Cost**: not measured yet (to be measured as in §5, the Goodsprings and Prospector
  Saloon frames, with the row on and off).
- **Screenshots**: before and after, for the owner, still to be taken.


## 4. Tried and dropped

- **Bevy `ContactShadows`** on the sun (16 steps, 40 units, thickness 12): the right effect on
  sunlit bushes, but a band of dark speckles on near surfaces seen at a grazing angle (the
  near roof at Goodsprings) and per-frame noise, which Bevy expects TAA to clean up.
- **Fog in-scattering towards the sun**: invisible in NVWastelandClear (fog to 200,000) and
  a shadow-map fetch per fragment. (The desert haze, §3.16, warms toward the sun without it.)
- **Bevy bloom with an HDR main view** and **Bevy CAS**: see §2.
- **Bevy volumetric fog** (`VolumetricFog` on the camera, `VolumetricLight` on the sun, a
  30,000 x 6,000 x 30,000-unit `FogVolume` following the camera, density 0.0005, 64 steps) at
  Goodsprings 19:00: the whole picture went dark brown and the volume's box edge showed as a
  hard step across the mountains [observed 2026-10-12, private frame
  `C:\fnv-private\scratch\graphics-2\vol-dusk.png`]. Its pass blends absorption and linear
  in-scattered light (times the exposure) onto the main texture, which in this non-HDR view
  already holds tonemapped display values, so it darkens without adding visible light; and a
  box big enough for the Mojave's distances needs coarse steps. It would also replace the
  weather's distance fog look rather than follow it. The sun shafts (§3.7) give the light
  shafts it was wanted for.

## 5. Frame rate and the defaults

### 5.1 Follow-ups (2026-10-12, this machine)

Measured on the machine that ran the follow-ups (GeForce RTX 4070, not the owner's GTX 1050),
1280 x 720, free camera standing still at Goodsprings 19:00 facing the sunset, with the test aids
`--log-frame-times 6 --graphics-cycle "vanilla;taa;light-shafts;enhanced;..."` (one run switching
between the sets every 6 s, three rounds; GPU time per pass from timestamp queries, Bevy's
`RenderDiagnosticsPlugin`, about the last second's average). The frame rate itself stayed at the
display's 60 Hz in every set (presented with vsync whatever was asked), so the GPU figures are
the measure:

| Pass | GPU ms |
|---|---|
| Main opaque pass, MSAA 4x / TAA (no multisampling) | 1.00-1.03 / 0.70-0.74 |
| Depth prepass, MSAA 4x / TAA | 0.32-0.33 / 0.22-0.23 |
| Temporal anti-aliasing pass | 0.04-0.05 |
| Sun shafts (four quarter-size passes) | 0.02-0.09, mostly 0.03 |
| Ambient occlusion (§3.1, for comparison) | 0.11-0.19 |
| Bloom and sharpening (§3.3-§3.4) | 0.03-0.05 |

So on this GPU each addition costs a few hundredths of a millisecond, and TAA saves about
0.4 ms against MSAA 4x. On the GTX 1050 (several times slower [hypothesis]) the same passes
would cost several times as much; not measured there.

### 5.2 First set (PR #236)

Measured 2026-10-12 on the owner's PC (GeForce GTX 1050 4 GB, 8 cores), 1280 × 720, Goodsprings
at noon (`--pos=-69000,3500,8900 --heading 30 --pitch -8`), free camera standing still,
`--run-seconds 60` (frames over the whole run, cell loading included), one after the other.
Other sessions' game runs and builds shared the machine, so the figures are noisy: the same
vanilla run gave 14.5 and later 7.2 frames per second.

| Run | Frames per second |
|---|---|
| `--graphics vanilla` (first) | 14.5 |
| `--graphics enhanced` (right after) | 10.0 |
| `--graphics-only fine-shadows` | 19.6 |
| `--graphics-only ambient-occlusion,bloom,sharpen,anisotropic` | 10.6 |
| `--graphics vanilla` (again, last) | 7.2 |

The back-to-back pair says enhanced costs about a third of the frame rate; the noise does not
allow a figure per feature. On this GPU the vanilla picture is already below 20 frames per
second, so:

- **Normal play defaults to `vanilla`** (`graphics::PLAY_DEFAULT`); `--graphics enhanced` or
  the settings page's "Graphics" switches turn the improvements on. On a faster GPU
  changing `PLAY_DEFAULT` to `Enhanced` is the one-line switch.
- `vanilla` is also the look of every test run: `--screenshot`, `--timelapse`, `--frame-json` and
  `--run-seconds` default to it whatever `PLAY_DEFAULT` says, so the parity lab, the picture
  checks and every smoke scenario keep their pictures, timings and expectations. `--graphics
  enhanced` on such a run asks for the enhanced look explicitly (the smoke scenario
  `graphics-enhanced` does).

### 5.3 Sun shadows (2026-10-12, playtest list 4 item 6)

The request [design, #3074]: "shadow rendering seriously affects performance; maybe limits on draw distance or
quality". Bevy times no shadow pass (they do not appear in the GPU list of `--log-frame-times`),
so the cost was measured as frame time with the cycle's shadow keys (§6) and the casters each
cascade drew (the log's "shadow casters per cascade" line). Machine: GeForce RTX 4070, 1280 × 800,
`--play` standing still at Goodsprings, (−69698, 2246), 14:00 looking east down the road (where
the owner's screenshots of item 5 were taken), `vanilla`, cycle switched every 4 s, three rounds
each; the figures are the round means (vsync off). The GPU's own time stayed between 1.9 and
4.1 ms in every set: the cost is the CPU's, one pass per cascade per view, each culling,
specialising and queueing its casters.

| Sun shadows | Casters per cascade | Frame time (ms) |
|---|---|---|
| off | | 7.3-7.9 |
| 8,000 in 2 cascades | 61, 2,000 | 12.4-13.8 |
| 8,000 in 3 cascades | 61, 340, 1,900 | 13.7-15.2 |
| 8,000 in 4 cascades | 61, 220, 710, 1,880 | 15.0-16.8 |
| 16,000 in 3 cascades | 200, 1,090, 3,060 | 16.4-18.3 |
| 16,000 in 4 cascades (the old default) | 200, 570, 1,690, 3,030 | 19.0-20.8 |

All with the arms' shade light (`player_body.rs`) taking the sun's first cascade, about 190
casters, as it did. Changed [engineering choices]:

- **The shade light reaches 256 units** from the camera (`ARMS_SHADE_REACH`): about 30 casters, and
  the old default 16,000 in 4 went from 19.0-20.8 to 16.9-17.6 ms. A cascade keeps every caster
  between it and the sun, so walls and roofs still shade the arms; their shadows on the arms
  are finer than before.
- **The sun's shadows default to 8,000 units in 3 cascades** (`sky::DEFAULT_SHADOW_DISTANCE`,
  and then `DEFAULT_SHADOW_CASCADES`, since #2843 `CascadeLayout`; `--shadow-distance` still sets the reach): 12.5-12.9 ms against
  the old 19.0-20.8, about 7 ms (a third of the frame) saved; shadows off is 7.3-7.5. The first
  cascade, a twelfth of the reach, is now 667 units instead of 1,333, so shadows near the player
  are finer. Visible change: nothing farther than 8,000 units (about 114 m) casts a sun shadow;
  at noon the view down the road is unchanged
  (`C:\fnv-private\references\shadows_sun\shadow_limits_before_after.png`); at dusk the long
  shadows of distant rocks and buildings end nearer. The original casts no sun shadows from
  the world at all (world.md §6), so this is no further from its look.
  **Proposed in #2843** [engineering choice: the contributor's proposal, for the
  maintainer to decide]: Quality draws 4 cascades again, still to 8,000 units and
  staggered (never more than two drawn in a frame), with far bounds at 0.0875, 0.25 and
  0.625 of the distance (700, 2,000, 5,000) with 2048² maps, or 0.1875, 0.4375 and 0.875
  with the middle two capped at 0.3 and 0.6 of the last bound (1,500, 3,500, 7,000 with
  Longer shadows, 1,500, 2,400, 4,800 without, the latter not measured) and a blend band
  of 0.4 with 4096² maps (the Shadow resolution row); Performance draws 3,
  ending at 0.1875 and 0.5625 of it (1,500, 4,500), the third every fourth frame.
  `--graphics-cycle cascades=<n>` keeps the layout above. The measurements and the
  reasons: frame-rate-audit.md §12.
- Finer shadows (§3.2) keep their 4096² maps and reach 1.5 × the distance, now 12,000
  (the 4096² maps went in #1367; the longer reach stays).

Not changed: the water's reflection camera, while active, gets its own cascades of the sun (Bevy
builds them for every active camera, with no way to leave one out), doubling the shadow passes
while water is in view; casters are not thinned by size with distance (no per-cascade filter in
Bevy; built and measured in §5.5).

### 5.4 Anti-aliasing and shadow-filter choices (2026-10-16, this machine)

GeForce RTX 4070 (not the owner's GTX 1050), 1280 × 720, free camera standing still at
Goodsprings 14:00 (`--pos=-69000,3500,8900 --heading 30 --pitch=-8 --sun-shadows`),
`--log-frame-times 4|5 --graphics-cycle` over vanilla (MSAA 4x), fxaa, smaa, taa and the two
shadow filters, five runs. Other sessions shared the machine; the frame time (11.8-12.5 ms in the
quiet runs) was the CPU's and did not tell the choices apart, so the GPU passes are the measure:

| Pass | GPU ms |
|---|---|
| Main opaque pass, MSAA 4x / without multisampling (FXAA, SMAA, TAA) | 0.72-0.78 / 0.56-0.58 |
| Depth prepass, MSAA 4x / without | 0.30-0.31 / 0.24 |
| FXAA pass | 0.04 |
| SMAA (its three passes) | 0.08-0.11 |
| Main opaque pass with the shadow filter Fast / Smooth / Temporal | 0.69-0.77 / 0.73-0.78 / 0.73-0.76 (within the noise) |

So FXAA and SMAA, like TAA, cost less than MSAA 4x in all (the main pass and the prepass save more
than their own pass), FXAA the least; the shadow filters' difference does not show at this
scale. On the GTX 1050 every figure would be several times larger [hypothesis, not measured
there], with the same order: MSAA 8x dearest, then 4x, then SMAA, TAA, FXAA.

One run stopped on a validation error ("A query of type PipelineStatistics was not ended") at
a switch into vanilla after the cycle had gone through SMAA and the shadow filters; four more
runs making the same switches (about 45 in all) did not repeat it. PipelineStatistics queries
come only from the measuring aid's `RenderDiagnosticsPlugin`; the smoke scenario
`graphics-settings`, which makes every switch without it, saw no error.

### 5.5 Where the sun shadows' cost goes (2026-10-23)

The owner: sun shadows drain performance, presumably the main thread. Measured with the span
profile (`NVF_PLAY_TEST_PROFILE`, a `--features nvforever/profile` build; frame-rate-audit.md
§1) and `--log-frame-times 8 --graphics-cycle`, so each comparison is one run. Desktop
(32 threads, RTX 4070), 1280 × 800, `--play` at the Goodsprings start, 14:00, standing still,
vanilla; other sessions built and ran the game at the same time, so only the cleanest rounds
(those that agree within about 0.5 ms) are quoted [observed].

**It is the render thread.** With shadows on the main thread's own work hardly changes
(about 7 against 6 ms a frame, Bevy's caster culling `check_dir_light_mesh_visibility` about
1 ms of it); the render thread goes from about 9.7 to 16.3 ms, and the main thread waits for
it (the `RenderExtractApp` span holds the wait). The render spans that grow: writing the
binned instance buffers (1.6 → 4.0 ms), preparing the preprocessing bind groups (1.2 → 2.4),
recording the passes (`RenderContextState::apply` 2.2 → 3.7) and submitting (`queue_submit`
1.3 → 2.4). The GPU's time stays small (§5.3).

| Sun shadows (8,000 units) | Casters per cascade | Frame (ms) | Render thread (ms) |
|---|---|---|---|
| off | | 10.2 | 9.7 |
| 1 cascade | 2,200 | 14.3 | 13.8 |
| 2 cascades | 85, 2,200 | 14.9 | 14.3 |
| 3 cascades (default) | 85, 460, 2,130 | 16.8 | 16.3 |

All with the arms' shade light (about 30 casters). So about 4 ms comes with having sun
shadows at all and the third cascade adds about 2 ms.

**Thinning the casters does not pay** (`shadow_thin.rs`, `--shadow-thin <ratio>`, off by
default): beyond 1,000 units a placed reference whose bounding radius is under `ratio` × its
distance casts nothing. The sun's last cascade, broken down by that angular size (the
frame-time log's `sun's last cascade:` line): about 75 meshes under 0.01, 450 at 0.01-0.02,
490 at 0.02-0.04, 340 at 0.04-0.08, 280 over 0.08, the rest near or not placed references.
Clutter whose shadow covers a few pixels is a small part.

| `--shadow-thin` | Meshes thinned | Last cascade | Render thread (ms) |
|---|---|---|---|
| 0 | 0 | 2,080-2,330 | 16.2-17.3 |
| 0.01 | 235 | 2,000-2,260 | 16.0-16.6 |
| 0.02 | 1,240 | 1,535-1,670 | 15.4-16.4 |
| 0.04 | 2,140 | 1,030-1,150 | 14.4-16.2 |

Halving the far cascade saves 1-2 ms of about 6.5, and at 0.04 an object under 2 m in radius
loses its shadow beyond 50 m: a visible change for little. The rule stays off.

**Why** [hypothesis, from Bevy 0.19.1's sources]: the per-view work scales with batch sets
more than with casters. `write_binned_instance_buffers` writes two GPU buffers and a GPU
unpacking job per batch set of every view, and the preprocessing bind groups are made per
view each frame. A shadow draw that only writes depth shares one batch set across materials,
but an alpha-tested NIF shape (plants, fences, signs) needs its material's textures, and
`NifMaterial` is not bindless, so each such material is a batch set of its own in every cascade.
Fewer casters rarely removes a whole material.

What would cut it, in order of what it saves:
- **A bindless `NifSurface`** (frame-rate-audit.md §4): alpha-tested materials would share
  batch sets in every view, the main view included. Built in §5.6: it takes the shadows'
  cost almost to nothing.
- **2 cascades instead of 3**: about 2 ms, with coarser shadows from 667 to about 2,300 units
  (the middle cascade's range; the first cascade stays 667). The owner's open choice from §5.3.
- **The arms' shade light** is one more shadow view while the first-person arms are drawn.
- **Water in view** gives the reflection camera a set of cascades of its own (§5.3).

### 5.6 Bindless NIF materials (2026-10-23)

`NifSurface`, the NIF material's extension of `StandardMaterial`, is bindless where the device
allows it, as `StandardMaterial` already was (Bevy turns bindless on for an extended material
only when both parts are): its values go to one storage array for all NIF materials and its
textures (holes, FaceGen offsets) to Bevy's shared texture arrays, and the shaders read them
through the index tables (`nif_surface_bindings.wesl`; without bindless the plain bindings
50-53). [engineering choice; nif_material.rs "Bindless"]

**Batch sets** (the profile aid's `profile batches:` line, Goodsprings start, 14:00, sun shadows
on) [observed]:

| | Before | Bindless |
|---|---|---|
| Shadow phases (4 views: 3 cascades and the arms' shade light) | ~830 | 49 |
| Main view, opaque / alpha-mask (and their prepasses) | 169 / 147 | 9 / 14 |

**Frame time** (same place, `--log-frame-times 8 --graphics-cycle "vanilla;vanilla,no-sun-shadows"`,
the span-profile build; the runs of each build alternated, and only uncapped windows are quoted:
in some runs the frames were held to the display's 60 Hz whatever the build, the window or the
compositor's doing) [observed]:

| | Before | Bindless |
|---|---|---|
| Sun shadows on (8,000 units, 3 cascades) | 16.5-17.3 ms | 6.4-6.6 ms |
| Sun shadows off | 9.9-10.0 ms | 6.7-6.8 ms |
| Render thread, shadows on / off | 16.3-17.0 / 10.0-10.4 ms | 6.3-8.3 / 5.2-6.1 ms |

The render spans that had grown with the shadows shrank most: writing the binned instance
buffers 4.0 → 0.4 ms, preparing the preprocessing bind groups 2.4 → 0.5, recording the passes
3.7 → 1.7, submitting 2.4 → 0.8. On the GPU the bin-unpacking pass went from about 1.7 to 0.15 ms.
The main thread is now about as busy as the render thread (6-8 ms), so the frame is no longer
held by either at this place; sun shadows cost next to nothing here.

**Same picture** [observed, screenshots of both builds at 1280 × 720 compared pixel by pixel]:
an NPC's face (Doc Mitchell, FaceGen tint: identical), the Pip-Boy (largest difference 12 of
255), the Prospector Saloon, Goodsprings at noon and dusk with sun shadows, a weapon in first
person, Gomorrah's roof fires: the only differences are what changes from run to run anyway
(wind-blown plants, a debug text line, HUD messages shown a moment apart). The refraction smoke
(`stealth-boy-refraction`, a moving bend) passes with both builds and its frames agree within
10 of 255. A build with the bindless attribute taken out (the fallback for devices without
bindless) drew the same face, Pip-Boy and first-person frames and passed the refraction smoke.
No pipeline or shader error was logged by any of the three builds.

Not measured: the owner's GTX 1050 (Bevy decides bindless per device; without it the plain
bindings are used, as before this change), and other places than Goodsprings.

### 5.7 Casters over the slice, not the square (2026-11-03, #1378)

The sun's shadows cost more at a low sun: at Novac the frame was 11.4 ms at 07:00 against 10.7
at noon, with the shadow passes' GPU time at 2.17 against 1.84 ms. Counting the casters each cascade
drew showed why [observed: a temporary probe of `CascadesVisibleEntities`]:

- Bevy fits each cascade to the light-space box around its slice of the camera's view and
  draws every caster in that box's square, plus everything between the box and the sun
  (`check_dir_light_mesh_visibility` skips the near plane). At 07:00 the sun stands 15°
  above the horizon, so more ground lies up-sun of each slice. That was 32, 201 and 13
  casters in the three cascades that lay wholly toward the sun past the near plane, against
  none at noon.
- Those are not waste. A test of whether any ray from the lowest ground could pass through
  them, given their height, removed 10 of them. They are the long morning shadows.
- The waste is the square. Seen from the sun, a slice of a perspective view is a trapezoid,
  and the square around it holds about as many casters again that shade nothing in the slice.
  This holds at every hour.

Changed (`sky.rs` `sun_casters_over_slice`, after Bevy's light visibility): each cascade
keeps only the casters whose light-space box overlaps the outline of its slice seen from
the sun, plus 8 shadow-map texels for the filter's reach. A cascade's receivers all lie in its
slice (the shader picks a cascade by view depth, and the overlap with the next cascade is in
both slices), and a caster can only shade what lies under it seen from the sun.

| Novac, casters per cascade | 07:00 | 12:00 |
|---|---|---|
| before | 154, 1,438, 3,248 | 120, 1,283, 2,928 |
| over the slice | 80, 761, 1,767 | 65, 480, 1,549 |

[observed 2026-11-03] Release builds, i9-14900K, RTX 4070, 1920 × 1080, `--graphics enhanced`,
`--play` standing still, `NVFOREVER_HOME` without stored settings. Each run is 45 s and logs its
frame time every 10 s; main and this change were interleaved over 3 rounds. One main run at 07:00
hit a throttled window at 16.67 ms and is left out.

| Novac | frame, main | frame, over the slice | shadow passes GPU, main / over the slice |
|---|---|---|---|
| 12:00 | 10.70-10.76 ms | 9.41-9.49 ms | 1.84 / 1.24 ms |
| 07:00 | 11.39-11.47 ms | 9.75-9.80 ms | 2.17 / 1.39 ms |

So 07:00 is now 0.33 ms from noon, against about 0.7 before. Screenshots of both builds at
07:00 and noon, from the free camera in four headings, differ only in wind-blown grass. The
casters dropped from the passes are still extracted for the frame (they stay visible to the
light), so the saving is in the passes, not in extraction.

## 6. Test aids for measuring

`--log-frame-times <seconds>` logs the mean and slowest frame time and the GPU time of the
costliest render passes every so many seconds (and asks for presenting without vsync, which the
driver may still impose); `--graphics-cycle "<set>;<set>;..."` switches between sets of
improvements after each logged period (`vanilla`, `enhanced` or keys as for `--graphics-only`;
the first half second after a switch is left out), so one run compares them under the same load.
A set may also change the sun's shadows: `no-sun-shadows`, `shadow-distance=<units>`,
`cascades=<n>`, `shadow-thin=<ratio>` (§5.5) among its keys
(`vanilla;no-sun-shadows;shadow-distance=16000,cascades=4`). The
anti-aliasing and shadow filters have keys too: `fxaa`, `smaa`, `taa`, `shadow-filter-hardware`,
`shadow-filter-temporal` (a set without one of them has `--msaa` and the Gaussian filter); the
log then gives the sun's shadow reach and cascades with each frame time, the sun's heading and
elevation, and the casters each cascade of each light drew; with sun shadows on also what
`--shadow-thin` thinned and the sun's last cascade broken down by angular size (§5.5).

Debug switch `NVF_PLAY_DEBUG_AO` (read once at the start; for before/after pictures and cost
runs of the ambient occlusion, §3.1.1, §3.1.2): unset, empty, `0`, `off` or `false` put the
ground-truth estimate in the lighting (the default), `view-lighting` does that and draws the
factor each fragment took; on the picture, `post` runs the ground-truth estimate, `old` the old
estimate and its blur, `view` the ground-truth estimate's factor alone as a grey picture,
`view-old` the old one's. Anything else is unknown (a warning; the default runs).
`NVF_PLAY_DEBUG_AO_STRENGTH=<number>` (read once at the start) replaces the ground-truth
estimate's strength (0.92 in the lighting, `LIGHTING_STRENGTH`; 0.46 on the picture,
`GTAO_STRENGTH`), held to 0..1; the old estimate keeps its 0.55; the log line then names it
`strength <number> (NVF_PLAY_DEBUG_AO_STRENGTH)`. `NVF_PLAY_DEBUG_AO_VARIANT=<names>` picks the
remedies for the plants' shimmer (unset: `thin+soft` in the lighting, none on the picture;
`none` the plain estimate), `NVF_PLAY_DEBUG_AO_LAMPS=<0..1>` the lamps' weight in the lighting
(default 1, 0 the ambient term alone), `NVF_PLAY_DEBUG_AO_EXPONENT=<0.1..2>` its exponent
(default 1). Before a temporal upscaler
the grey replaces the HDR picture and still passes through the upscaler and tonemapping;
`NVF_AO_BEFORE_FSR=0` shows it after tonemapping instead. Its pass's GPU time is the
`nvf_ambient_occlusion` span of `--log-frame-times`.

Debug switches of the volumetric medium (§3.23, docs/design/volumetric-medium.md; developer aids
that only tests, debug builds and builds with the `dev-aids` feature have, §3.23; read once at
the start, usable in a played run, each logged once as `debug switch ...` when it changes
the default, `NVF_PLAY_DEBUG_MEDIUM_CLAMP_K` set alone included, since it changes the default
clip's band):
`NVF_PLAY_DEBUG_MEDIUM=reference` traces every pixel of the render size every frame with no
history (the first cloud prototype's picture under MSAA; the default is the buffer at the level's share
rebuilt over nine frames); `NVF_PLAY_DEBUG_MEDIUM_VIEW=fresh|age|history|clamped|distance` shows,
opaque over the sky, which texels were cast this frame (green; grey rebuilt from history, red
none), their age, where no valid history was found, how much the clamp moved the history, or the
weighted distance; `NVF_PLAY_DEBUG_MEDIUM_FRESH=<0..1>` sets the fresh share's floor (0.1);
`NVF_PLAY_DEBUG_MEDIUM_FRESH_MOVING=<0..1>` the share it rises to while the view moves (0.1);
`NVF_PLAY_DEBUG_MEDIUM_OFFSETS=turning|fixed|blue|centre` the rays' start offsets (turning by
default; fixed the same per texel at every cast; blue interleaved gradient noise turned at each
cast; centre 0.5 for every ray, no offset pattern at all);
`NVF_PLAY_DEBUG_MEDIUM_CLAMP=moving|box|variance|off` the history's clamp (moving by default:
none while still or slow, the variance clip fading in with motion; box the first default;
variance the mean plus or minus k standard deviations of the fresh rays, k from
`NVF_PLAY_DEBUG_MEDIUM_CLAMP_K=<0.5..8>`, 1.25; off none, for diagnosis);
`NVF_PLAY_DEBUG_MEDIUM_TURN=<degrees a second>` turns the view at a constant rate (positive to
the right; the player's heading in a played run, the free camera's yaw in the viewer), for
pictures and cost runs mid-turn; `NVF_PLAY_DEBUG_MEDIUM_SCALE=<1..4>` (a whole number, or 2.5) makes
the default mode's buffer the render size divided by that, rounded up, whatever the row's level;
`NVF_PLAY_DEBUG_MEDIUM_TILE=<1|2|3>` casts one ray per tile of that side a frame (3; 1 every texel
every frame, still through the history); `NVF_PLAY_DEBUG_MEDIUM_HISTORY_FILTER=linear|cubic|adaptive`
the history's interpolation (adaptive). The passes' GPU times are the `nvf_medium_trace` and
`nvf_medium_reconstruct` spans of `--log-frame-times`.

`NVF_CAUSTICS_BEFORE_UPSCALER=0` (read once at the start) draws the submerged caustics
after tonemapping at the display size under a temporal upscaler, as builds before it moved
did (§3.8.2), for before/after pictures; without an upscaler it changes nothing. The pass's
GPU time is the `nvf_underwater_caustics` span in either placement.

## 7. Next candidates

- Bloom and eye adaptation driven by the image spaces' own HDR values (IMGS DNAM's 14 HDR
  floats and 3 bloom floats), once their meanings are documented from xEdit: that would move
  bloom from an engine choice towards parity.
- Bevy SSAO once the custom prepasses write normals (an own GTAO from the depth was built
  instead, §3.1.1; its stage b, the occlusion on the ambient light only, is open); motion
  vectors from the custom prepasses (last frame's wind and skinning) would let the TAA
  follow what moves by itself.
- Sun shafts through interior windows are the game's own light-ray meshes already; none from
  the moon.
- A shadow distance and map size setting with more steps than on/off (the cycle keys of §6
  measure them).
- Cloud shadows (#2670) and terrain shadows (#2671) on the sun: the shared plumbing, one
  visibility factor on the sun's direct light, is in [sun-visibility.md](sun-visibility.md).
  Cloud shadows is the settings row Cloud shadows (experimental) (`cloud-shadows`,
  Graphics › Shadows),
  off by default and in both presets until its strength is calibrated
  ([sun-visibility.md §7-§8](sun-visibility.md#7-the-switch-stage-c2)). Terrain shadows is
  the row Terrain shadows (experimental) (`terrain-shadows`, Graphics › Shadows, after Cloud
  shadows), off by default and in both presets until its softness and bias are tuned
  ([terrain-shadows.md](terrain-shadows.md)).
