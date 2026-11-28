// Volumetric clouds' light (volumetric_clouds_light.rs; docs/design/graphics-enhanced.md §3.23;
// #3032 stage 2e): how one view sample of the cloud march is lit, a leaf module imported by
// volumetric_clouds_march.wesl as `nvforever::volumetric_clouds_light`. Plain WGSL: no imports,
// no bindings, no texture and no `CloudVolume` (the march imports this module, so this one cannot
// import the march's struct back): its inputs are its own small structs and plain values, which the
// march fills from its uniform once per ray (`vc_ray_light`, the angular part) and hands in per
// sample (`vc_lean_shade`: the sample's density result, the optical depth toward the light and the
// march's estimates of its column). Written from published descriptions, each named where it is
// used (Bauer, "Creating the Atmospheric World of Red Dead Redemption 2", SIGGRAPH 2019; Wrenninge,
// Kulla and Lundqvist 2013; Schneider and Vos, "The Real-time Volumetric Cloudscapes of Horizon Zero
// Dawn", SIGGRAPH 2015); no code from any project. The constants come from volumetric_clouds_light.rs through the
// march's uniform. An engineering choice for the enhanced look, not the original game's.

// Henyey-Greenstein's phase times 4 pi (isotropic = 1) (volumetric_clouds.rs `hg`).
fn vc_hg(cos_theta: f32, g: f32) -> f32 {
    let g2 = g * g;
    let d = max(1.0 + g2 - 2.0 * g * cos_theta, 1e-4);
    return (1.0 - g2) / (d * sqrt(d));
}

// What the shading takes of the march's uniform, the same for every sample of a ray
// (volumetric_clouds_light.rs `Shading`).
struct VcShading {
    // The debug view in force (`flags.z`, the march's `vc_view_code`): 0 the clouds; 6 the direct
    // term alone, 7 the ambient alone, 8 the optical depth toward the light (the other codes shade
    // as 0; the march does not call this for the views it draws on the bent base).
    mode: u32,
    // The uniform's `lmode`: y the column depth per unit height share, z the ambient's fall, w the
    // direct light's weight (the `SUN` aid).
    column: vec4<f32>,
    // The light from above and from below, each times its hemisphere weight (`lupper`, `llower`).
    upper: vec3<f32>,
    lower: vec3<f32>,
    // The uniform's `loct`: x the second octave's weight, y its depth factor.
    octave: vec4<f32>,
    // The uniform's `lfloor`: x the back-scatter floor's length, y its most, z, w the elevation
    // sines over which a measured depth takes over the light from above.
    back: vec4<f32>,
    // The light's elevation sine (`light_dir.y`).
    rise: f32,
}

// What a ray prepares once (the angular part of the light, volumetric_clouds_light.rs `RayLight`):
// `light` the direct light's colour times its strength, `p0` and `p1` the two octaves' mixed phases
// (before the floor and the cap), `cap` the cap, `powder` the powder darkening's share along this ray.
struct VcRayLight {
    light: vec3<f32>,
    p0: f32,
    p1: f32,
    cap: f32,
    powder: f32,
}

// The powder darkening's share along a ray (#3032, form and contrast; Schneider and Vos 2015's
// "powder" term): `share` (the `POWDER` aid times POWDER_SHARE, at most 1) times
// `(1 - cos_theta) / 2`, whole looking away from the light and none toward it [engineering choice].
fn vc_powder_share(cos_theta: f32, share: f32) -> f32 {
    return clamp(share, 0.0, 1.0) * clamp(0.5 - 0.5 * cos_theta, 0.0, 1.0);
}

// The powder term's length (volumetric_clouds_light.rs POWDER_LENGTH): a sample's local optical
// depth for the darkening is its extinction per unit times this many units.
const VC_POWDER_LENGTH: f32 = 2000.0;

// What the powder term keeps of the direct light at a local extinction `ext` per unit:
// `1 - share e^(-2 ext L)`, i.e. the light times `mix(1, 1 - e^(-2 ext L), share)` (Schneider and
// Vos 2015's `1 - e^(-2d)`): thin wisps lose up to `share`, a dense core nearly nothing; never more
// than 1, so it only darkens.
fn vc_powder(ext: f32, share: f32) -> f32 {
    return 1.0 - share * exp(-2.0 * max(ext, 0.0) * VC_POWDER_LENGTH);
}

// An octave's two lobes mixed (#3032 stage 2e, L2): the forward lobe's weight `fw` (1 - the back
// lobe's weight, times its aid), the back lobe's `bw`, both lobes' g times `c` (1 for the first
// octave, the eccentricity factor for the second). Not capped: the floor comes first.
fn vc_octave_phase(cos_theta: f32, ph: vec4<f32>, fw: f32, c: f32) -> f32 {
    return fw * vc_hg(cos_theta, ph.x * c) + ph.z * vc_hg(cos_theta, ph.y * c);
}

// The ray's angular part for the cosine `cos_theta` between the view ray and the direction to the
// light, the phase's values `ph` (x forward g, y back g, z the back lobe's weight, w the cap), the
// light's colour `light` and strength `strength`, the octaves' values `oct` (the uniform's `loct`: z the
// eccentricity factor, w the forward lobe's aid), the powder's share `powder`.
fn vc_ray_light(cos_theta: f32, ph: vec4<f32>, light: vec3<f32>, strength: f32, oct: vec4<f32>, powder: f32) -> VcRayLight {
    let fw = oct.w * (1.0 - ph.z);
    return VcRayLight(light * strength, vc_octave_phase(cos_theta, ph, fw, 1.0), vc_octave_phase(cos_theta, ph, fw, oct.z), ph.w, vc_powder_share(cos_theta, powder));
}

// The back-scatter floor at a local extinction `ext` per unit (Bauer 2019, slide 30: the phase
// held from below by a value growing with the extinction; bounded).
fn vc_floor(ext: f32, s: VcShading) -> f32 {
    return min(max(ext, 0.0) * s.back.x, s.back.y);
}

// The share of the light arriving from a whole hemisphere that crosses a column of optical depth
// `t` (volumetric_clouds_light.rs AMBIENT_FALL: the two-stream form `1 / (1 + fall t)`, bounded
// by 1, with the slow tail of multiply scattered light).
fn vc_through(t: f32, fall: f32) -> f32 {
    return 1.0 / (1.0 + fall * max(t, 0.0));
}

// The share of the lean light's scattered octave that follows the smooth path estimate in fair weather [design].
const VC_LEAN_BROAD: f32 = 0.8;
// The light from below a fair cloud's face receives, against the deck's [design].
const VC_LEAN_FAIR_LOWER: f32 = 0.7;

// The light a view sample emits toward the camera (linear; the march weights it by what the step hides,
// albedo 1; #3032; volumetric_clouds_light.rs `lean_shade`; the march calls it from `vc_incident`): `d` the
// sample's density result (x the shape, y the extinction per unit at shape 1, z the weather's amount, w the
// place's height share), `altitude` its height over the base, `depth` the optical depth toward the light the
// march measured, `sampled` whether it measured it (the gate on the shape and a light above the horizon; a
// skipped sample's depth is 0 and says nothing of the cloud above it). The debug views (`s.mode`): 6 the
// direct term alone, 7 the ambient alone, both through the normal march; 8 the optical depth's code
// (`vc_depth_code`), which the trace averages by opacity (`vc_depth_shown`). The light as Bauer 2019
// describes it for clouds (slides 28-33: two Henyey-Greenstein octaves whose depth, weight and
// eccentricity fall each iteration after Wrenninge 2013, blended by artist weights and the extinction,
// the phase clamped from below by a back-scatter floor that grows with the extinction, one transmittance
// lookup toward the sun on cloud samples; slide 36: the ambient a sky average, its visibility reusing the
// directional sample) with Schneider and Vos 2015's powder term [documented: cited publications]: the
// normalised two-octave weights (`octave.x`), `exp(-t)` and `exp(-k t)` (`octave.y` k), the
// extinction-dependent floor (`vc_floor`), the powder darkening (`vc_powder`), the direct light's weight
// (`column.w`, SUN); the light from above through the column above (the vertical estimate, or the measured
// depth times the light's elevation sine under a high light) and from below through the column below
// (`vc_through`, OCCLUDE's fall). A continuous storm layer redistributes intercepted sunlight
// across neighbouring columns. The colours are the weather's alone, packed
// on the CPU (volumetric_clouds_light.rs `lighting`): the direct light's hue the weather's
// cloud colour (PNAM) moved to its sunlight colour by SUNCOLOUR, the light from above its sky colours
// (SkyUpper and Horizon), from below the zenith hue at the ground bounce luminance. The column above and below
// is the march's per-sample estimate (`vc_incident`). The floor's convention: this leaf's
// phase is Henyey-Greenstein times 4 pi (isotropic 1), where the publication's Lambertian clamp of 1 / pi
// reads 4; the floor's most is half of FLOOR_MAX (1) times BACKSCATTER [design]. Written
// from those descriptions; no code from any project.
fn vc_lean_shade(d: vec4<f32>, altitude: f32, depth: f32, sampled: bool, ray: VcRayLight, s: VcShading, crown_depth: f32, foot: f32, sheet_depth: f32, open_flank: f32) -> vec3<f32> {
    if s.mode == 8u {
        return vc_depth_code(depth, sampled);
    }
    let ext = d.x * d.y;
    let h = clamp(d.w, 0.0, 1.0);
    // [documented: Bauer 2019, optical depth along the light path and HG octaves.]
    // [hypothesis] Under a high sun the coarse light cache misses part of the overhead
    // column at a lower face. Fade its depth estimate out at the shoulder and at thin rims.
    let measured = select(0.0, max(depth, 0.0), sampled);
    let high_sun = smoothstep(0.18, 0.60, s.rise);
    // [hypothesis] In fair weather the diffuse transition sits above the common
    // condensation level across all the heads. Deep decks use local height instead.
    let deck = smoothstep(0.55, 0.85, d.z);
    // The sheet's own ramp: the fair terms are gone where its scattering is complete.
    let fair = 1.0 - smoothstep(0.55, 0.73, d.z);
    let face = mix(smoothstep(450.0, 1300.0, altitude), smoothstep(0.22, 0.52, h), deck);
    let base = (1.0 - face) * clamp(d.x, 0.0, 1.0);
    let lid = s.column.y * base * high_sun;
    let bulk = s.column.y * (1.0 - h);
    let t = mix(max(max(measured, lid), crown_depth), bulk / max(s.rise, 0.25), foot);
    let v0 = exp(-t);
    // [hypothesis] Upper lobes smaller than a light-cache cell receive scattered
    // sunlight from the local upper boundary. Keep the measured single-scatter path.
    let shoulder = high_sun * face;
    let cap_depth = 0.15 + 0.35 * s.column.y * (1.0 - h);
    let fill_depth = max((1.0 - foot) * crown_depth, mix(t, min(t, cap_depth), shoulder));
    // [hypothesis] Light scattered more than once reaches a fair cloud's lower face from a
    // broad area, so most of its path follows the smooth estimate. The measured path keeps
    // the single-scatter beam and the deck.
    let broad = max((1.0 - foot) * crown_depth, lid);
    let v1 = exp(-s.octave.y * mix(fill_depth, broad, VC_LEAN_BROAD * fair));
    var above = s.column.y * (1.0 - h);
    if sampled {
        above = mix(above, measured * max(s.rise, 0.0), smoothstep(s.back.z, s.back.w, s.rise));
    }
    above = mix(max(above, max(lid, crown_depth * max(s.rise, 0.0))), bulk, foot);
    let f = vc_floor(ext, s);
    // [hypothesis] Light can escape sideways from the lower flank of a finite bank.
    // The directional beam keeps its phase. Each scattering event retains a share
    // of the light, so fewer paths survive two events near an open flank.
    let retained = 1.0 - open_flank * high_sun * (1.0 - smoothstep(0.25, 0.65, h));
    let q0 = min(max(ray.p0, f * retained), ray.cap);
    let q1 = min(max(max(ray.p1, f), shoulder), ray.cap);
    let phase = (1.0 - s.octave.x) * q0 * v0 + retained * retained * s.octave.x * q1 * v1;
    var source = vc_powder(ext, ray.powder) * phase;
    if sheet_depth > 0.0 {
        // [hypothesis] In a continuous deck, the intercepted beam reaches the lower face
        // from a broader area. Estimate its path from the front and the column overhead.
        let covered = smoothstep(0.55, 0.73, d.z) * (1.0 - exp(-sheet_depth / max(s.rise, 0.25)));
        let path = sheet_depth + mix(bulk, sheet_depth * (1.0 - h), 0.6);
        let scattered = s.octave.x * max(s.rise, 0.0) * vc_through(s.octave.y * path, 1.0);
        source = mix(source, scattered, covered);
    }
    var direct = ray.light * (s.column.w * source);
    let lower = s.lower * mix(1.0, VC_LEAN_FAIR_LOWER, fair);
    var ambient = s.upper * vc_through(above, s.column.z) + lower * vc_through(s.column.y * h, s.column.z);
    if s.mode == 6u {
        ambient = vec3<f32>(0.0);
    }
    if s.mode == 7u {
        direct = vec3<f32>(0.0);
    }
    return ambient + direct;
}

// The depth view's scale (volumetric_clouds_light.rs DEPTH_WHITE_LOG2, DEPTH_GREY): a measured
// depth's grey is log2(1 + depth) over 6, times 0.9: black at depth 0 (the light reaches the sample
// whole), each sixth of the grey a doubling of 1 + depth (1, 3, 7, 15, 31), white (0.9, under the
// tonemapper inverse's saturation) from 63 on.
const VC_DEPTH_WHITE_LOG2: f32 = 6.0;
const VC_DEPTH_GREY: f32 = 0.9;
// The opacity over which the depth view shows a ray's mean opaque (below it the ray's own
// premultiplied colour, so empty sky stays empty).
const VC_DEPTH_SHOWN: f32 = 0.01;

// A sample's code in the depth view: the grey of its measured depth, magenta (0.9, 0, 0.9) where the
// march skipped the light (the gate: shape at or under the threshold, or no light).
fn vc_depth_code(depth: f32, sampled: bool) -> vec3<f32> {
    if !sampled {
        return vec3<f32>(VC_DEPTH_GREY, 0.0, VC_DEPTH_GREY);
    }
    return vec3<f32>(VC_DEPTH_GREY * clamp(log2(1.0 + max(depth, 0.0)) / VC_DEPTH_WHITE_LOG2, 0.0, 1.0));
}

// Whether the depth view shows a ray whose samples hid `hidden` (the opacity-weighted sum the trace
// keeps for the distance) as the opaque mean `colour / hidden` of its samples' codes.
fn vc_depth_shown(hidden: f32) -> bool {
    return hidden > VC_DEPTH_SHOWN;
}
