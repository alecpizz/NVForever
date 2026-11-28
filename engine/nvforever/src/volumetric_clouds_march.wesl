// Volumetric clouds' march (volumetric_clouds.rs; docs/design/graphics-enhanced.md §3.23), the
// cloud source of the volumetric medium: imported by the medium's trace pass
// (medium_trace.wesl, medium.rs) as `nvforever::volumetric_clouds_march`. Plain WGSL on
// purpose: no conditions, no bindings, and one import, the light's leaf module
// (volumetric_clouds_light.wesl, #3032 stage 2e), so the engine's tests parse and validate this
// very file with naga once that one import line is replaced by the leaf's text
// (volumetric_clouds_light.rs `plain_march_wgsl`).
//
// The seam to the medium [design, #3032 decision 1; stage 2a added the ray's spread]: a ray in
// (`MediumRay`: origin, unit direction, the stretch [t_min, t_max] of it the medium owns, world
// units, and its spread: the width per unit of distance that one cast ray answers for), the
// light it gathers out (`MediumSample`: the colour, the transmittance and the transmittance-
// weighted distance) from `vc_trace_refined`. Inside the loop each sample asks three questions of
// its own functions: how much it takes (`vc_shape_with_column`), whether the light reaches it (the
// taps, `vc_light_handoff`) and what light arrives from all around (`vc_incident`). A second source over
// the same stretch (haze beyond the near field, later) adds its extinction and emission at
// the same samples of the same loop; adding a second finished colour over the first would
// count the light through the shared stretch twice.
//
// A domain of cloud above the world (#3032 stage 2a): its base the cloud shadows' plane, its
// height a parameter (40,000 units by default), bent down over the distance as a shell
// centred under the camera (the sample's height raised by its horizontal distance squared
// over twice the shell's radius), so the base meets a horizontal ray from the ground at the
// horizon distance and the clouds go down to the horizon, smaller and flatter, fading into
// the dome by the distance where the ray enters (Bauer, "Creating the Atmospheric World of Red
// Dead Redemption 2", SIGGRAPH 2019, slide 50: rays bounded by a cloud dome). How much cloud
// there is comes from the weather (the mean of the composed cover, `field.y`'s threshold); where
// it stands is a slow 2D field of our own (three octaves of lattice value noise in world x and z,
// not repeating, moved by the producer's unwrapped drift: the talk's cover map is a few large
// blobs, slides 14-15): a place holds cloud where the field passes the threshold, and how far
// it passes is how deep inside its mass it is, so its local top rises with it (a dome over each
// mass, nothing at its edge), scaled by a second slow field (the vigour); each place mixes three
// cloud types (a sheet, a flat-based cumulus, a tower: their profile over the place's own height,
// how tall the bank stands, how deep its relief is) from the weather's mix and, locally, how deep in its
// mass and how vigorous it is (Bauer 2019, slides 16 and 18: a strip of gradients over height per
// weather, blended across weather transitions, shifting the thresholds the cover passes). The density is
// the lean model's (`vc_m2_*`): one flat condensation base, a signed field (the cover times a gradient per
// type against a generated 3D Perlin-Worley noise, cloud_noise.rs, the surface carved by one finer Worley
// read, wispy low and billowy high) with a crisp surface and its own extinction, read at the level the
// ray's footprint picks; lit by its own leaf (volumetric_clouds_light.wesl `vc_lean_shade`: two octaves of
// a two-lobe Henyey-Greenstein phase, the extinction's floor, powder, the weather's sunlight and sky
// colours through the column) with Beer-Lambert extinction and the energy-conserving step integral as
// Hillaire's "Physically Based Sky, Atmosphere and Cloud Rendering in Frostbite" (SIGGRAPH 2016) describes
// it. Written from Bauer 2019 (slides 14-18, 28-36), Schneider's "The Real-Time Volumetric Cloudscapes of
// Horizon Zero Dawn" (SIGGRAPH 2015) and "Nubis" (SIGGRAPH 2017) [documented: cited publications], from
// those descriptions; no code from any project. The constants come from volumetric_clouds.rs
// through the uniform. An engineering choice, not the original game's.

import nvforever::volumetric_clouds_light::{VcRayLight, VcShading, vc_ray_light, vc_lean_shade, vc_depth_shown};

// volumetric_clouds.rs CloudVolumeUniform: the same fields in the same order.
struct CloudVolume {
    // x: the base (Bevy y, the cloud shadows' plane), y: the domain's height above it, z: the
    // source's range along the ray, w: the horizon distance (where the bent base meets a
    // horizontal ray from the ground reference; the fade by entry distance ends there).
    slab: vec4<f32>,
    // x: the near step's length (world units), y: light taps, z: the density multiplier (1 as
    // designed), w: shape above which the light is marched.
    march: vec4<f32>,
    // x: the light march's longest reach, y: the shell's radius, z: the transmittance at which
    // the march stops, w: the most view steps a ray takes.
    reach: vec4<f32>,
    // x: forward g, y: back g, z: the back lobe's weight, w: the phase's cap.
    phase: vec4<f32>,
    // xyz: unit direction to the light, w: its strength 0..1.
    light_dir: vec4<f32>,
    // rgb: the direct light (linear).
    light: vec4<f32>,
    // xy: world (x, z) to the cover's texture coordinates (one over the repeat: the noise's and
    // the fields' scale); zw unused.
    map: vec4<f32>,
    // x: the height (units) over which a cloud's base rounds in, y: the sky share the weather's
    // amount asks for (an unresolved footprint's cover), z: the refined march's coarse
    // spacing, w not read here.
    profile: vec4<f32>,
    // x: 1 when the view shows display values clipped, y: 1 to rotate the dither each frame,
    // z: the debug view (0 the clouds, 1 the placement field against its threshold, 2 the local
    // height, 3 the evaluations per ray), w: a step is at most this many footprints at the ray's
    // entry.
    flags: vec4<f32>,
    // z, w: the low noise's range spread over 0..1; x, y not read here.
    tone: vec4<f32>,
    // The vigour field's offsets in its own cells (the producer's drift, unwrapped), for its
    // first and its second octave: xy the fraction (zw unused; the whole cells are `cells_v`).
    vigour: vec4<f32>,
    vigour2: vec4<f32>,
    // x: vigour cells per texture repeat (its first octave), y: the second octave's frequency
    // over the first's, w: how far over the domain's base the band always integrated begins (units: `vc_guard`
    // takes a sheet more); z not read here.
    height: vec4<f32>,
    // The placement: x the field's first-octave cells per texture repeat, y the threshold the
    // field must pass to hold cloud (from the weather's amount), z the domain's height (the types'
    // heights are in it), w the least local height of a cloud (units: a sheet).
    field: vec4<f32>,
    // The vigour's scale of the local height `scale.x + (scale.y - scale.x) v^scale.z`; w the
    // weather's amount (the mean of its composed cover, 0..1).
    scale: vec4<f32>,
    // The placement field's offsets per octave (the producer's drift, unwrapped, in each
    // octave's own cells): xy the fraction (zw unused; the whole cells are `cells_p`, `cells_q`).
    place0: vec4<f32>,
    place1: vec4<f32>,
    // xy: third octave drift; w not read here.
    place2: vec4<f32>,
    // The offsets' whole cells as the lattice's integer bits (two's complement, wrapping,
    // so nothing clamps or jumps however far the drift goes): `cells_v` the vigour's
    // first octave (x, z) and second (x, z), `cells_p` the placement's first and second,
    // `cells_q` its third (x, z; z, w unused).
    cells_v: vec4<u32>,
    cells_p: vec4<u32>,
    cells_q: vec4<u32>,
    // Crosswise fair banks. XY are fraction bits, ZW are signed cell bits.
    cross0: vec4<u32>,
    cross1: vec4<u32>,
    cross2: vec4<u32>,
    // The weather's mix of the cloud types: x the sheet, y the cumulus, z the tower (summing to
    // 1); w 1 when one type is forced everywhere (NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_TYPE: no local
    // choice).
    mix: vec4<f32>,
    // The local choice of the tower: x, y the vigour over which it comes in, z, w the field over
    // the threshold over which it comes in; the tower's weight there is the weather's times
    // `kind.y`.
    tower: vec4<f32>,
    // x the fair lobes' strength (PUFF), y the local tower's weight; z, w not read here.
    kind: vec4<f32>,
    // Per type, its profile over the place's own height: eight knots at h = 0, 1/7 .. 1, linear
    // between (a knot list may hold a gap: zeros between two bands).
    knot_s0: vec4<f32>,
    knot_s1: vec4<f32>,
    knot_c0: vec4<f32>,
    knot_c1: vec4<f32>,
    knot_t0: vec4<f32>,
    knot_t1: vec4<f32>,
    // The extinction table's amounts (the first four, the last four) and its optical depths
    // per unit of mean shape (likewise): volumetric_clouds.rs CAL_COVERS, EXTINCTION_TABLE.
    cal_c0: vec4<f32>,
    cal_c1: vec4<f32>,
    cal_k0: vec4<f32>,
    cal_k1: vec4<f32>,
    // The generated noise (#3032 stage 2b, cloud_noise.rs): the drift's phase in each texture's
    // repeats, xy the base's (world x, z), zw the erosion's (bounded: reduced on the CPU).
    nphase: vec4<f32>,
    // x: one over the base's repeat (world units), y: one over the erosion's, z: the base relief's
    // strength (NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_SHAPE, 1 as designed), w: the erosion's
    // (NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_EROSION).
    nscale: vec4<f32>,
    // x: the base's side in texels, y: its last level, z: the erosion's side, w: its last level.
    nlevel: vec4<f32>,
    // x, y: the base noise's middle half (`vc_spread_base`); z, w not read here.
    nlobe: vec4<f32>,
    // The erosion's three bands' means at level 0 (xyz); w not read here.
    nband: vec4<f32>,
    // x, y: a band's middle half; z, w not read here.
    nerode: vec4<f32>,
    // detail (stage 2h)
    // z: the wisps' strength (WISP, W2: 0 none), x, y not read here, w: reciprocal trace tile side (1 for a non-tiled view).
    dshape: vec4<f32>,
    // light volume (stage 2g)
    // The light volume (#3032 stage 2g, volumetric_clouds_lightvol.rs `LvSchedule`; all zero with
    // it off): x the near taps (of the `march.y` intervals), y 1 with the near level read (its
    // front set ready), z 1 with the far level read, w the guard band (cells of each level).
    lv: vec4<f32>,
    // Per level (near, far): xy the camera `lv_cam`'s position in nodes from the corner of the set
    // read (the trace: the front set's own window, this frame's camera; a producer snapshot: the
    // window being swept, its bake camera), z the spacing across (units), w the spacing up (units).
    lv_near: vec4<f32>,
    lv_far: vec4<f32>,
    // x, y: the near and the far level's nodes across; z, w: their layers.
    lv_size: vec4<f32>,
    // xyz: the camera the positions are measured from (world: the trace's origin, or the bake's
    // camera in a producer snapshot); w unused.
    lv_cam: vec4<f32>,
    // light (stage 2e)
    // Packed by volumetric_clouds_light.rs `lighting`, read only by `vc_shading`, `vc_ray_light_of` and
    // `vc_incident` (lmode.y). y the column depth per unit height share, z the ambient's fall per optical
    // depth, w the direct light's weight (the `SUN` aid); x not read here.
    lmode: vec4<f32>,
    // rgb: the weighted light from above (linear); w not read here.
    lupper: vec4<f32>,
    // rgb: the weighted ground light; w not read here.
    llower: vec4<f32>,
    // x: the second octave's weight, y: its depth factor, z: its phase's eccentricity factor, w: the
    // forward lobe's weight aid.
    loct: vec4<f32>,
    // x: the back-scatter floor's length and y its most (times the aid), z, w: the light's
    // elevation sines over which a measured depth takes over the light from above.
    lfloor: vec4<f32>,
    // x: the powder darkening's share (the `POWDER` aid); y, z, w not read here.
    lpowder: vec4<f32>,
    // x: the light's filter cell (units; a light volume level's spacing, 0 for none: `vc_light_cell`);
    // y, z, w not read here.
    lcell: vec4<f32>,
    // Diffuse dome at the horizon / at 20 degrees. W holds strength / ground reference.
    air_low: vec4<f32>,
    air_high: vec4<f32>,
    // x: the largest longitudinal stride (REFINED_STRIDE), w: 1 to reuse a neighbouring cell's light
    // (LIGHT_REUSE); y, z not read here.
    families: vec4<f32>,
}

// A view ray of the medium: from `origin` along the unit `dir`, the stretch t_min..t_max of it
// (world units) belongs to the medium; `spread` is the width per unit of distance that this
// ray answers for (medium.rs `ray_spread`: the cast texel times the tile side), so a source can
// leave out what is finer than its footprint.
struct MediumRay {
    origin: vec3<f32>,
    dir: vec3<f32>,
    t_min: f32,
    t_max: f32,
    spread: f32,
}

// What a ray gathers: `colour` the light scattered toward the camera (linear, already
// weighted by how much it hides: premultiplied), `transmittance` how much of what lies behind
// still shows (1: nothing in the way), `hit_distance` the distance along the ray weighted by
// what each stretch hid (medium.rs EMPTY_DISTANCE, -1, when nothing did).
struct MediumSample {
    colour: vec3<f32>,
    transmittance: f32,
    hit_distance: f32,
}

// The vigour field's lattice seeds (the hash's middle coordinate): two fields that share no
// cell value with each other or with the 3D noise's small heights.
const VC_SEED_TOP: u32 = 0x9e3779b9u;
const VC_SEED_TOP2: u32 = 0x7f4a7c15u;
// The placement field's three octaves (volumetric_clouds.rs PLACE_OCTAVES): their frequencies
// over the first's (not whole numbers, so the sum never repeats), their weights and seeds.
const VC_PLACE_F1: f32 = 2.17;
const VC_PLACE_F2: f32 = 4.73;
const VC_PLACE_W0: f32 = 0.40;
const VC_PLACE_W1: f32 = 0.35;
const VC_PLACE_W2: f32 = 0.25;
const VC_SEED_PLACE0: u32 = 0x51ed270bu;
const VC_SEED_PLACE1: u32 = 0x2545f491u;
const VC_SEED_PLACE2: u32 = 0x9e6c63d1u;
// How far over its threshold the field rounds a mass in (volumetric_clouds.rs MASS_EDGE).
const VC_MASS_EDGE: f32 = 0.03;
// The wisps (#3032 stage 2h, W2; volumetric_clouds_wisp.rs WISP_*): the displacement's peak per
// component at WISP 1 (units), its two terms' weights, their cycles per base-noise repeat (whole
// numbers: no jump where the drift's phase wraps), their wavelengths over the height (units) and fixed
// phases (radians).
const VC_WISP_AMP: f32 = 300.0;
const VC_WISP_A0: f32 = 0.6;
const VC_WISP_A1: f32 = 0.4;
const VC_WISP_N0: f32 = 7.0;
const VC_WISP_N1: f32 = 9.0;
const VC_WISP_LY: vec4<f32> = vec4<f32>(6000.0, 3700.0, 5300.0, 3300.0);
const VC_WISP_PHI: vec4<f32> = vec4<f32>(0.0, 2.0, 4.0, 1.0);
// The light march's intervals grow by this from the sample outward (volumetric_clouds.rs
// LIGHT_GROWTH).
const VC_LIGHT_GROWTH: f32 = 1.5;

// A lattice cell's value, 0..1: an integer hash with odd multipliers chosen for this.
fn vc_hash(c: vec3<u32>) -> f32 {
    var h = (c.x * 0x7a3d1c95u) ^ (c.y * 0x4f1bbcddu) ^ (c.z * 0x2b8f3a61u);
    h = (h ^ (h >> 15u)) * 0x93c467e5u;
    h = (h ^ (h >> 13u)) * 0xd6e8feb9u;
    h = h ^ (h >> 16u);
    return f32(h >> 8u) * (1.0 / 16777216.0);
}

// 2D value noise on a lattice that does not repeat: the cell's signed whole coordinates and
// the place within it, one field per seed (the vigour).
fn vc_lattice_corners(cell: vec2<i32>, next: vec2<i32>, seed: u32) -> vec4<f32> {
    let x0 = bitcast<u32>(cell.x);
    let x1 = bitcast<u32>(next.x);
    let z0 = bitcast<u32>(cell.y);
    let z1 = bitcast<u32>(next.y);
    return vec4<f32>(vc_hash(vec3<u32>(x0, seed, z0)), vc_hash(vec3<u32>(x1, seed, z0)),
                     vc_hash(vec3<u32>(x0, seed, z1)), vc_hash(vec3<u32>(x1, seed, z1)));
}

fn vc_noise_corners(cell: vec2<i32>, seed: u32) -> vec4<f32> {
    return vc_lattice_corners(cell, cell + vec2<i32>(1), seed);
}

fn vc_noise_interp(corners: vec4<f32>, f: vec2<f32>) -> f32 {
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(corners.x, corners.y, u.x);
    let b = mix(corners.z, corners.w, u.x);
    return mix(a, b, u.y);
}

fn vc_noise2(cell: vec2<i32>, f: vec2<f32>, seed: u32) -> f32 {
    return vc_noise_interp(vc_noise_corners(cell, seed), f);
}

// A profile's value at height share h from its eight knots (h = 0, 1/7 .. 1), linear between
// (volumetric_clouds.rs `knots_at`); 0 outside 0..1.
fn vc_knots(h: f32, k0: vec4<f32>, k1: vec4<f32>) -> f32 {
    if h <= 0.0 || h >= 1.0 {
        return 0.0;
    }
    var k = array<f32, 8>(k0.x, k0.y, k0.z, k0.w, k1.x, k1.y, k1.z, k1.w);
    let x = h * 7.0;
    let i = min(u32(floor(x)), 6u);
    return mix(k[i], k[i + 1u], x - f32(i));
}

// The profile at a place's own height share h for the local type weights `w` (sheet, cumulus,
// tower) (volumetric_clouds.rs `profile_at`).
fn vc_profile(h: f32, w: vec3<f32>, v: CloudVolume) -> f32 {
    return w.x * vc_knots(h, v.knot_s0, v.knot_s1) + w.y * vc_knots(h, v.knot_c0, v.knot_c1) + w.z * vc_knots(h, v.knot_t0, v.knot_t1);
}

// The low noise spread evenly over 0..1 (volumetric_clouds.rs `spread`).
fn vc_spread(n: f32, tone: vec4<f32>) -> f32 {
    return clamp((n - tone.z) / max(tone.w - tone.z, 1e-3), 0.0, 1.0);
}

// The extinction table at cover c: linear between its covers, held outside
// (volumetric_clouds.rs `table_depth`).
fn vc_table(c: f32, v: CloudVolume) -> f32 {
    var cs = array<f32, 8>(v.cal_c0.x, v.cal_c0.y, v.cal_c0.z, v.cal_c0.w, v.cal_c1.x, v.cal_c1.y, v.cal_c1.z, v.cal_c1.w);
    var ks = array<f32, 8>(v.cal_k0.x, v.cal_k0.y, v.cal_k0.z, v.cal_k0.w, v.cal_k1.x, v.cal_k1.y, v.cal_k1.z, v.cal_k1.w);
    if c <= cs[0] {
        return ks[0];
    }
    for (var i = 1u; i < 8u; i++) {
        if c <= cs[i] {
            let t = (c - cs[i - 1u]) / max(cs[i] - cs[i - 1u], 1e-4);
            return mix(ks[i - 1u], ks[i], t);
        }
    }
    return ks[7];
}

// Extinction per unit at shape 1 in a cloud of cover c whose own height is `local` units: the
// table's optical depth (measured so that the mean opacity straight up is the cover's target)
// over that height, times the density multiplier (volumetric_clouds.rs `extinction`).
fn vc_extinction(c: f32, local: f32, v: CloudVolume) -> f32 {
    if c <= 0.0 {
        return 0.0;
    }
    return vc_table(c, v) / max(local, 1.0) * v.march.z;
}

// The level a footprint `width` (world units) reads from a texture of `size` texels a side
// repeating every 1 / `inv_repeat` units with `last` its last level (cloud_noise.rs
// `level_for`): log2 of the footprint in texels, 0 to the last level, so far cloud reads coarse
// levels instead of aliasing (#3032 stage 2b).
fn vc_level(width: f32, size: f32, inv_repeat: f32, last: f32) -> f32 {
    return clamp(log2(max(width * size * inv_repeat, 1.0)), 0.0, last);
}

// The base noise's coordinates at `pos` (world) whose bent height is `yb`, in its repeats: world
// x and z moved by the drift's phase, up from the domain's base (one function per scale, so a
// later displacement can offset the lookup without moving the placement).
fn vc_base_coord(pos: vec3<f32>, yb: f32, v: CloudVolume) -> vec3<f32> {
    return vec3<f32>(pos.x * v.nscale.x + v.nphase.x, (yb - v.slab.x) * v.nscale.x, pos.z * v.nscale.x + v.nphase.y);
}

// The erosion noise's coordinates likewise, in its own repeats.
fn vc_erosion_coord(pos: vec3<f32>, yb: f32, v: CloudVolume) -> vec3<f32> {
    return vec3<f32>(pos.x * v.nscale.y + v.nphase.z, (yb - v.slab.x) * v.nscale.y, pos.z * v.nscale.y + v.nphase.w);
}

// How much of a band of cells `cell` units wide a cast width `width` keeps (volumetric_clouds.rs
// `band_keep`): all from four cast widths a cell, none from two.
fn vc_band_keep(cell: f32, width: f32) -> f32 {
    return 1.0 - smoothstep(0.25 * cell, 0.5 * cell, width);
}

// --- The wisps (#3032 stage 2h, checkpoint W2; volumetric_clouds_wisp.rs `wisp`;
// docs/design/graphics-enhanced.md §3.23). Bauer 2019, slides 17-18: the detail's 3D lookup displaced by
// a quasi-3D vector from two 2D displacement samples and the wind, so fringes stretch and tear. Ours is
// an analytic field (no texture, no read) displacing only the erosion's lookups; the column, the bent
// height, `inside`, the hold and the support stay at the point. Run only where `eroded`, the detail and
// WISP are on. ---

// The displacement at `pos` (bent height `yb`) and its local stretch.
struct VcWisp {
    // World (x, z) units.
    d: vec2<f32>,
    // The Frobenius norm of dD/d(world x, y, z) here: the lookup map's stretch is at most 1 + g.
    g: f32,
}

// The field (volumetric_clouds_wisp.rs `wisp`): two horizontal components, each two sines of the base
// noise's body coordinates (x and z in its repeats moved by the producer's unwrapped drift, `nphase.xy`:
// it moves with the body, no drift of its own) and the height over the domain's base. Its amplitude is
// at most WISP x VC_WISP_AMP per component and its gradient at most WISP x VC_WISP_AMP x 2.035e-3
// (volumetric_clouds_wisp.rs `wisp_bounds`); the horizontal entries stay under 0.33 at WISP 4, so the
// lookup never folds over.
fn vc_wisp(pos: vec3<f32>, yb: f32, v: CloudVolume) -> VcWisp {
    let a = VC_WISP_AMP * v.dshape.z;
    let u = pos.x * v.nscale.x + v.nphase.x;
    let w = pos.z * v.nscale.x + v.nphase.y;
    let ky = vec4<f32>(6.2831855) / VC_WISP_LY;
    let t = 6.2831855 * vec4<f32>(VC_WISP_N0 * w, VC_WISP_N1 * u, VC_WISP_N0 * u, VC_WISP_N1 * w) + (yb - v.slab.x) * ky + VC_WISP_PHI;
    let s = sin(t);
    let c = cos(t);
    let k0 = 6.2831855 * VC_WISP_N0 * v.nscale.x;
    let k1 = 6.2831855 * VC_WISP_N1 * v.nscale.x;
    let dxx = VC_WISP_A1 * k1 * c.y;
    let dxy = VC_WISP_A0 * ky.x * c.x + VC_WISP_A1 * ky.y * c.y;
    let dxz = VC_WISP_A0 * k0 * c.x;
    let dzx = VC_WISP_A0 * k0 * c.z;
    let dzy = VC_WISP_A0 * ky.z * c.z + VC_WISP_A1 * ky.w * c.w;
    let dzz = VC_WISP_A1 * k1 * c.w;
    let g = a * sqrt(dxx * dxx + dxy * dxy + dxz * dxz + dzx * dzx + dzy * dzy + dzz * dzz);
    return VcWisp(a * vec2<f32>(VC_WISP_A0 * s.x + VC_WISP_A1 * s.y, VC_WISP_A0 * s.z + VC_WISP_A1 * s.w), g);
}

// The filter width of a view sample of cast width `width` whose step is `dt` long:
// the wider of the two, so along-ray variation is filtered before the start offset can alias it.
fn vc_detail_filter(width: f32, dt: f32) -> f32 {
    return max(width, max(dt, 0.0));
}

// The vigour at pos, 0..1 (volumetric_clouds.rs `vigour`): two octaves of 2D value noise, the
// first at `height.x` cells per texture repeat (its cell three times the domain's
// height: slow enough that it cannot raise a needle), the second `height.y` times finer (not
// a whole number, so the sum never repeats with the cover), each moved by its own share of the
// producer's unwrapped drift (whole cells plus a fraction), spread over 0..1 from the low
// noise's range (`tone.zw`).
fn vc_vigour(pos: vec3<f32>, v: CloudVolume) -> f32 {
    let p = pos.xz * v.map.xy * v.height.x + v.vigour.xy;
    let c1 = floor(p);
    let n1 = vc_noise2(vec2<i32>(c1) + bitcast<vec2<i32>>(v.cells_v.xy), p - c1, VC_SEED_TOP);
    let p2 = pos.xz * v.map.xy * v.height.x * v.height.y + v.vigour2.xy;
    let c2 = floor(p2);
    let n2 = vc_noise2(vec2<i32>(c2) + bitcast<vec2<i32>>(v.cells_v.zw), p2 - c2, VC_SEED_TOP2);
    return vc_spread(n1 * 0.6667 + n2 * 0.3333, v.tone);
}

// One octave's lattice value at pos (`cells` its cells per texture repeat, `off` its offset's
// fraction, `whole` its whole cells).
fn vc_place_octave(pos: vec3<f32>, cells: f32, off: vec2<f32>, whole: vec2<i32>, seed: u32, v: CloudVolume) -> f32 {
    let p = pos.xz * v.map.xy * cells + off;
    let c = floor(p);
    return vc_noise2(vec2<i32>(c) + whole, p - c, seed);
}

// The placement's footprint at `rel` from the camera on a ray along `dir` whose transverse
// footprint there is `width` (volumetric_clouds.rs `place_width`): the distance
// between neighbouring rays where they meet the bent shell, in world x and z: the transverse
// width over the cosine between the ray and the shell's normal there (its height grows with the
// horizontal distance squared over twice the radius), so a grazing ray's footprint on the base
// is as long as it is.
fn vc_place_width(rel: vec3<f32>, dir: vec3<f32>, width: f32, v: CloudVolume) -> f32 {
    let r = max(v.reach.y, 1.0);
    let n = normalize(vec3<f32>(rel.x / r, 1.0, rel.z / r));
    let projected = width / max(abs(dot(dir, n)), 1e-3);
    if v.scale.w < 0.72 {
        // [hypothesis] A fair bank has sidewalls as well as a horizontal base. Filtering
        // both axes by the longer projected axis erases distant groups. An isotropic
        // footprint of equal area preserves their spacing. A sheet uses the long axis.
        let deck = smoothstep(0.55, 0.72, v.scale.w);
        let footprint = sqrt(width * projected);
        // Each mixed-bank shoulder occupies individual reconstructed texels along the skyline.
        let texel = mix(1.0, v.dshape.w, vc_m2_mixed_sheet(v));
        return mix(footprint * texel, projected, deck);
    }
    return projected;
}

// How much of an octave of `cells` cells per texture repeat a footprint `width` keeps
// (volumetric_clouds.rs `octave_keep`): whole while its cell spans two footprints, none from one.
fn vc_octave_keep(cells: f32, width: f32, v: CloudVolume) -> f32 {
    let cell = 1.0 / max(v.map.x * cells, 1e-9);
    return 1.0 - smoothstep(0.5 * cell, cell, width);
}

// The weights of the placement field's three octaves for a footprint `width` (`vc_m2_bank`): in world x
// and z, not repeating, moved by the unwrapped drift, the first with `field.x` cells a texture repeat.
// An octave finer than the footprint fades toward its mean, and the field's spread about its
// mean is restored by the ratio of the full weights' length to the kept ones', so the share of
// sky the threshold passes stays the amount's as octaves fade; where even the first octave fades
// (a grazing footprint at scale 4) the column takes an unresolved footprint's cover (`vc_m2_shape`).
fn vc_place_weights(width: f32, v: CloudVolume) -> vec3<f32> {
    let k0 = vc_octave_keep(v.field.x, width, v);
    let k1 = vc_octave_keep(v.field.x * VC_PLACE_F1, width, v);
    let k2 = vc_octave_keep(v.field.x * VC_PLACE_F2, width, v);
    return vec3<f32>(VC_PLACE_W0 * k0, VC_PLACE_W1 * k1, VC_PLACE_W2 * k2);
}

// A place's column (`vc_m2_column`; volumetric_clouds_model2.rs `Column`).
struct VcColumn {
    // How much a broad, moist updraft can deepen this fair-weather column.
    developed_bank: f32,
    // Signed relief of the resolved heads, shared by support and density.
    head: f32,
    medium_head: f32,
    // The parent bank's height (units); 0 where it was not needed.
    local: f32,
    // How far inside its mass the place is (0 at the edge, 1 from MASS_EDGE in).
    edge: f32,
    // The local type weights (sheet, cumulus, tower).
    types: vec3<f32>,
    // How much of the placement the footprint resolves (its first octave's keep):
    // below 1 the sample is blended with an unresolved footprint's thin cover (`vc_m2_shape`).
    res: f32,
    // The vigour the column was made with (0..1; 0 where it was not needed).
    vigour: f32,
    // The placement field's excess over its threshold (negative outside a mass; the cover,
    // `vc_m2_cover`, reads it before the edge share saturates).
    over: f32,
}

// The local type weights for a place over its threshold by `over` with vigour `vg`
// (volumetric_clouds.rs `local_types`): the weather's mix, the tower's weight raised where the
// place is deep in a vigorous mass (none at the mass's shallow parts or under a low vigour);
// a forced type is taken as it is.
fn vc_types(over: f32, vg: f32, v: CloudVolume) -> vec3<f32> {
    if v.mix.w > 0.5 {
        return v.mix.xyz;
    }
    let local = smoothstep(v.tower.x, v.tower.y, vg) * smoothstep(v.tower.z, v.tower.w, over);
    let raw = vec3<f32>(v.mix.x, v.mix.y, v.mix.z * v.kind.y * local);
    let total = raw.x + raw.y + raw.z;
    if total <= 1e-6 {
        return vec3<f32>(0.0, 1.0, 0.0);
    }
    return raw / total;
}

// The height of a point `rel` from the camera at `cam`, in the bent frame: raised by its
// horizontal distance from the camera squared over twice the shell's radius (the domain bent
// down over the distance, centred under the camera).
fn vc_bent_y(rel: vec3<f32>, cam: vec3<f32>, v: CloudVolume) -> f32 {
    return cam.y + rel.y + dot(rel.xz, rel.xz) / (2.0 * max(v.reach.y, 1.0));
}

// The base noise's spread value's mean gradient per repeat where it is 0.35..0.65 (measured from the bake's
// CPU copy, volumetric_clouds_model2.rs `the_base_gradient_is_the_bakes`: 30.9).
const VC_BASE_GRAD: f32 = 30.0;
// The light's filter cell where no level gives one (the near level's default 500-unit cell).
const VC_LIGHT_CELL: f32 = 500.0;

// The light's filter cell (units; stage 2i M1): the light volume level's own spacing, which the level's
// frozen snapshot carries in `lcell.x` (volumetric_clouds_lightvol.rs `lv_start`; the trace's the near
// level's, `trace_block`), so the cells aid moves the filter with the grid; VC_LIGHT_CELL where none
// is given (the light volume off).
fn vc_light_cell(v: CloudVolume) -> f32 {
    if v.lcell.x > 0.0 {
        return v.lcell.x;
    }
    return VC_LIGHT_CELL;
}

// The base noise's spread value, 0..1 (its middle half onto 0.25..0.75).
fn vc_spread_base(b: f32, v: CloudVolume) -> f32 {
    return clamp(0.25 + 0.5 * (b - v.nlobe.x) / max(v.nlobe.y - v.nlobe.x, 1e-3), 0.0, 1.0);
}

// --- The lean model (#3032; volumetric_clouds_model2.rs is its CPU mirror; docs/design/
// graphics-enhanced.md §3.23). Written from the published descriptions of real-time cloud systems
// [documented: Bauer, "Creating the Atmospheric World of
// Red Dead Redemption 2", SIGGRAPH 2019, slides 14-18: a top-down cover map, a gradient over height per
// cloud type, the base shape the cover thresholded against that gradient, detail carved in from one 3D
// Perlin-Worley read by a linear rescale, the read displaced by a 2D vector field and the wind; Schneider
// and Vos, "The Real-time Volumetric Cloudscapes of Horizon Zero Dawn", SIGGRAPH 2015 Advances course, and
// Schneider, "Nubis: Authoring Real-Time Volumetric Cloudscapes with the Decima Engine", SIGGRAPH 2017: a
// low-frequency Perlin-Worley base remapped by the coverage makes the billowy lobes, a higher-frequency
// Worley fBm erodes the edge region, wispy (the Worley value) near the base and billowy (inverted)
// above].
// [hypothesis] Fair-weather placement sets the horizontal span independently of the type's
// shallow vertical extent. A level fair-weather base supports the rounded crown.
// The carrier is cover times a height profile. Signed cellular relief displaces its sides
// and crown about the mean Perlin-Worley surface. The column also samples a finer band
// of the base texture at the foot of a mixed deck. The light filters the same field at
// its own footprint.

// [hypothesis] Surface of four overlapping spherical caps. The farthest a cap can
// reach on either axis is 0.24 + 0.75 cells, so four vertices suffice even across
// a cell boundary. Wrap only the IDs; distances stay local across the repeat.
fn vc_m2_caps(uv: vec2<f32>, period: u32, shift: f32, width: f32, v: CloudVolume) -> f32 {
    let cell_width = 1.0 / (v.nscale.x * f32(period));
    let keep = 1.0 - smoothstep(0.35 * cell_width, 0.85 * cell_width, width);
    if keep <= 0.0 {
        return 0.0;
    }
    let p = fract(uv) * f32(period) + vec2<f32>(shift, 0.6 * shift);
    let tile = vec2<u32>(floor(p));
    let local = fract(p);
    var peak = 0.0;
    for (var k = 0u; k < 4u; k++) {
        let corner = vec2<u32>(k & 1u, k >> 1u);
        let id = (tile + corner) & vec2<u32>(period - 1u);
        let bits = u32(vc_hash(vec3<u32>(id.x, period, id.y)) * 16777216.0);
        let rnd = vec3<f32>(vec3<u32>(bits & 255u, (bits >> 8u) & 255u, bits >> 16u)) / 255.0;
        let delta = local - vec2<f32>(corner) - (rnd.xy - vec2<f32>(0.5)) * 0.48;
        let radius = 0.24 + 0.51 * rnd.z;
        let q = max(0.0, 1.0 - dot(delta, delta) / (radius * radius));
        peak = max(peak, (0.58 + 0.42 * rnd.z) * sqrt(q) * smoothstep(0.0, 0.16, q));
    }
    return keep * (peak - 0.33);
}

// [hypothesis] Diagonal placement stretches fair bodies across the plain while their
// upper heads vary in span and aspect. The unwrapped wind takes the same transform on
// the CPU. Filter at the larger principal frequency. Only these column fields feed M2.
const VC_M2_SHEAR: f32 = 0.10;

fn vc_m2_place_pos(pos: vec3<f32>) -> vec3<f32> {
    return pos + VC_M2_SHEAR * vec3<f32>(pos.z, 0.0, pos.x);
}

// [hypothesis] A short-wave lattice pit inside a broad updraft can puncture its
// foot. Lift only strict minima to their lowest spill point. A valley with an
// outlet retains its value. Each shared vertex sees the same four neighbours.
fn vc_m2_pit_lift(tile: vec2<i32>, frac: vec2<f32>, corners: vec4<f32>, seed: u32) -> f32 {
    var lift = vec4<f32>(0.0);
    for (var k = 0u; k < 4u; k++) {
        let inside = min(corners[k ^ 1u], corners[k ^ 2u]);
        if corners[k] < inside {
            let corner = vec2<i32>(i32(k & 1u), i32(k >> 1u));
            let cell = tile + corner;
            let step = 2 * corner - vec2<i32>(1);
            let ax = bitcast<vec2<u32>>(cell + vec2<i32>(step.x, 0));
            let az = bitcast<vec2<u32>>(cell + vec2<i32>(0, step.y));
            let spill = min(inside, min(vc_hash(vec3<u32>(ax.x, seed, ax.y)), vc_hash(vec3<u32>(az.x, seed, az.y))));
            lift[k] = max(0.0, spill - corners[k]);
        }
    }
    return vc_noise_interp(lift, frac);
}

// As cloud cover increases, the gaps fill and the dilute fringe joins the
// surrounding air before the fair and overcast placement fields crossfade.
fn vc_m2_open_bank_weight(v: CloudVolume) -> f32 {
    return 1.0 - smoothstep(0.35, 0.55, v.scale.w);
}

// Where isolated banks carry both sheet and convective material, their shallow
// column and the lobes above it share a taper at the ends of the cloud.
fn vc_m2_mixed_sheet(v: CloudVolume) -> f32 {
    let sheet = v.mix.x;
    return 4.0 * sheet * (1.0 - sheet) * vc_m2_open_bank_weight(v);
}

// Moist updrafts alternate with longer dry lanes on the middle placement
// lattice. Keep the integer phase until the diagonal cells have been selected.
// The next corner wraps in the original lattice before its two-cell division.
fn vc_m2_lane_noise(tile: vec2<i32>, f: vec2<f32>) -> f32 {
    let along = f.x + f.y;
    let across = f.x - f.y;
    let sum = tile.x + tile.y + i32(floor(along));
    let diff = tile.x - tile.y + i32(floor(across));
    let cell = vec2<i32>(sum >> 1u, diff);
    let next = vec2<i32>((sum + 2) >> 1u, diff + 1);
    let uv = vec2<f32>(0.5 * (f32(sum & 1) + fract(along)), fract(across));
    return vc_noise_interp(vc_lattice_corners(cell, next, VC_SEED_TOP2), uv);
}

// [documented: Bauer 2019, coverage as a separate horizontal field.]
// [hypothesis] Finite humid regions end even long streets. Distort their boundaries
// with the same advected weather before measuring distance to nearby wet centres.
// Return the distance inside the small patch and the moisture of the larger region.
fn vc_m2_regions(tile: vec2<i32>, f: vec2<f32>, broad: f32, mid: f32, lane: f32) -> vec2<f32> {
    let spacing = vec2<f32>(2.0, 4.0);
    let warp = vec2<f32>(0.54 * (broad - 0.5) + 0.42 * (mid - 0.5),
        0.53 * (lane - 0.5) - 0.38 * (mid - 0.5));
    let shift = vec2<f32>(f.x + f.y, f.x - f.y) + spacing * warp;
    let whole = vec2<i32>(tile.x + tile.y, tile.x - tile.y) + vec2<i32>(floor(shift));
    let local = (vec2<f32>(whole & vec2<i32>(1, 3)) + fract(shift)) / spacing;
    // The floor lies outside every wet core and inside the four-site stencil.
    // This also makes the interpolated dry field continuous as overcast closes.
    var reach = -0.085;
    for (var i = 0u; i < 4u; i++) {
        let corner = vec2<u32>(i & 1u, i >> 1u);
        let site = (whole + vec2<i32>(corner) * vec2<i32>(2, 4)) >> vec2<u32>(1u, 2u);
        let id = bitcast<vec2<u32>>(site);
        let bits = u32(vc_hash(vec3<u32>(id.x, VC_SEED_TOP2, id.y)) * 16777216.0);
        let random = vec3<f32>(vec3<u32>(bits & 255u, (bits >> 8u) & 255u, bits >> 16u)) / 255.0;
        let delta = local - vec2<f32>(corner) - 0.33 * (random.xy - vec2<f32>(0.5));
        let axes = vec2<f32>((delta.x + 0.35 * (random.x - 0.5) * delta.y) / 1.08,
            delta.y / (0.84 + 0.22 * random.y));
        reach = max(reach, 0.29 + 0.24 * random.z - length(axes));
    }
    let coarse_cell = whole >> vec2<u32>(2u, 3u);
    let coarse_next = (whole + vec2<i32>(4, 8)) >> vec2<u32>(2u, 3u);
    let coarse_uv = (vec2<f32>(whole & vec2<i32>(3, 7)) + fract(shift)) / vec2<f32>(4.0, 8.0);
    let moisture = vc_noise_interp(vc_lattice_corners(coarse_cell, coarse_next, VC_SEED_PLACE0), coarse_uv);
    return vec2<f32>(reach, moisture);
}

// Use the existing placement octaves and drift. The second octave supplies
// the cavity boundary. Where it encloses a deep pit, the broad octave can
// also hold water up to its own outlet. Reuse the four values of each cell.
fn vc_m2_bank(pos: vec3<f32>, width: f32, storm: f32, crosswise: bool, v: CloudVolume) -> vec2<f32> {
    let k = vc_place_weights(width, v);
    let kept = length(k);
    if kept <= 1e-4 {
        return vec2<f32>(0.5, 0.0);
    }
    let uv = pos.xz * v.map.xy;
    let p0 = uv * v.field.x + select(v.place0.xy, bitcast<vec2<f32>>(v.cross0.xy), crosswise);
    let tile0 = vec2<i32>(floor(p0)) + bitcast<vec2<i32>>(select(v.cells_p.xy, v.cross0.zw, crosswise));
    let corners0 = vc_noise_corners(tile0, VC_SEED_PLACE0);
    let broad = vc_noise_interp(corners0, fract(p0));
    let gain = length(vec3<f32>(VC_PLACE_W0, VC_PLACE_W1, VC_PLACE_W2)) / kept;
    if k.y <= 0.0 {
        // The finest octave fades out before the middle one.
        return vec2<f32>(0.5 + k.x * (broad - 0.5) * gain, 0.0);
    }
    let p1 = uv * (v.field.x * VC_PLACE_F1) + select(v.place1.xy, bitcast<vec2<f32>>(v.cross1.xy), crosswise);
    let tile1 = vec2<i32>(floor(p1)) + bitcast<vec2<i32>>(select(v.cells_p.zw, v.cross1.zw, crosswise));
    let corners1 = vc_noise_corners(tile1, VC_SEED_PLACE1);
    let mid = vc_noise_interp(corners1, fract(p1));
    var sum = k.x * (broad - 0.5) + k.y * (mid - 0.5);
    if k.z > 0.0 {
        let frac = select(v.place2.xy, bitcast<vec2<f32>>(v.cross2.xy), crosswise);
        let whole = bitcast<vec2<i32>>(select(v.cells_q.xy, v.cross2.zw, crosswise));
        sum += k.z * (vc_place_octave(pos, v.field.x * VC_PLACE_F2, frac, whole, VC_SEED_PLACE2, v) - 0.5);
    }
    var bank = 0.5 + sum * gain;
    let interior = (1.0 - storm) * smoothstep(0.30, 0.45, broad);
    if interior > 0.0 {
        let pit = k.y * vc_m2_pit_lift(tile1, fract(p1), corners1, VC_SEED_PLACE1);
        let gate = smoothstep(0.0, 0.05, pit);
        bank += interior * gain * gate * pit;
        let over = bank - v.field.y;
        // [hypothesis] At the spill point the medium pit ends inside the bank.
        // Carry broad support onto its occupied lip, up to ordinary full cover.
        let lip = smoothstep(0.0, 0.035, over) * smoothstep(0.38, 0.62, mid);
        let deficit = max(0.0, VC_M2_COVER - over);
        if pit > 0.015 || (lip > 0.0 && deficit > 0.0) {
            let basin = k.x * vc_m2_pit_lift(tile0, fract(p0), corners0, VC_SEED_PLACE0) * interior * gain;
            bank += max(basin * gate * smoothstep(0.015, 0.07, pit), min(basin, deficit) * lip);
        }
    }
    var developed = 0.0;
    let open_banks = vc_m2_open_bank_weight(v);
    if crosswise && open_banks > 0.0 {
        let lane = vc_m2_lane_noise(tile1, fract(p1));
        let moist = smoothstep(0.32, 0.66, lane + 0.24 * (mid - 0.5));
        // Dry air separates neighbouring banks. Return some water to their
        // developing shoulders while leaving the wettest crowns at their height.
        let credit = 0.16 * moist * (1.0 - smoothstep(-0.11, 0.20, bank - v.field.y));
        let weight = open_banks * (k.y / VC_PLACE_W1);
        let parent_excess = bank - v.field.y;
        bank += weight * (credit - 0.22 * (1.0 - moist));
        let region = vc_m2_regions(tile1, fract(p1), broad, mid, lane);
        // As the air column moistens, the spaces between updrafts can hold
        // water before the original unbroken weather field takes over.
        let capacity = 1.05 * region.x + 0.50 * (1.0 - open_banks);
        let small_bank = min(bank, v.field.y + capacity);
        // [hypothesis] A strong updraft can also grow across several small patches
        // where the larger air mass is humid. Its shoulders stay shallower than its core.
        let humidity = region.y + 0.22 * (broad - 0.5);
        let wet = smoothstep(0.38, 0.76, humidity);
        let supply = smoothstep(-0.085, 0.10, region.x);
        let ordinary = min(parent_excess, 1.40 * (parent_excess - mix(0.33, 0.035, wet))) - 0.095 * (1.0 - supply);
        // [hypothesis] A humid air mass can sustain a shared bank beyond one
        // updraft. Adjacent dry patches still separate its smaller neighbours.
        let fed = parent_excess + 0.067;
        let front = min(fed, 1.40 * (fed - 0.035));
        let mature_excess = mix(ordinary, max(ordinary, front), smoothstep(0.59, 0.83, humidity));
        bank = mix(bank, max(small_bank, v.field.y + mature_excess), weight);
        developed = weight * smoothstep(-0.015, 0.13, mature_excess);
    }
    return vec2<f32>(bank, developed);
}

fn vc_m2_lobe_weight(types: vec3<f32>, v: CloudVolume) -> f32 {
    return v.kind.x * (1.0 - types.x) * clamp(v.nscale.z, 0.0, 1.0)
        * (1.0 - smoothstep(0.55, 0.72, v.scale.w)) * smoothstep(0.0, 0.10, v.scale.w);
}

fn vc_m2_column(pos: vec3<f32>, width: f32, v: CloudVolume) -> VcColumn {
    var col: VcColumn;
    // [hypothesis] A fair bank's shortest placement octave can roughen a shoulder
    // but should rarely seed an island. A storm bank spans mostly the first octave.
    let storm = smoothstep(0.55, 0.72, v.scale.w);
    let cell = 1.0 / max(v.map.x * v.field.x, 1e-9);
    let min_width = cell * mix(0.19, 0.37, storm);
    // [hypothesis] Each weather field moves with the wind in its own fixed basis.
    // Dissolve their cover as the front passes. Interpolating world coordinates
    // would sweep distant banks across the horizon as the weather changes.
    let cross = v.kind.x * (1.0 - storm);
    var bank = vec2<f32>(0.0);
    // One placement sampler serves both lattices. Settled weather samples one.
    let first = select(0u, 1u, cross >= 1.0);
    let last = select(0u, 1u, cross > 0.0);
    for (var field_index = first; field_index <= last; field_index++) {
        let fair = field_index == 1u;
        let placed = select(vc_m2_place_pos(pos), 0.75 * pos - 0.29 * vec3<f32>(pos.z, 0.0, pos.x), fair);
        let pw = width * select(1.0 + VC_M2_SHEAR, 1.04, fair);
        let weight = select(1.0 - cross, cross, fair);
        bank += weight * vc_m2_bank(placed, max(pw, min_width), storm, fair, v);
        col.res += weight * vc_octave_keep(v.field.x, pw, v);
    }
    col.over = bank.x - v.field.y;
    col.developed_bank = bank.y;
    col.types = vec3<f32>(0.0, 1.0, 0.0);
    if col.over > 0.0 || col.res < 1.0 {
        col.vigour = max(vc_vigour(pos, v), 1e-6);
        col.types = vc_types(max(col.over, 0.0), col.vigour, v);
        col.edge = smoothstep(0.0, VC_MASS_EDGE, col.over);
        col.local = vc_m2_parent_extent(col, v);
    }
    let lobes = vc_m2_lobe_weight(col.types, v);
    let fair = clamp(v.nscale.z, 0.0, 1.0) * smoothstep(0.0, 0.10, v.scale.w) * (1.0 - storm);
    if fair > 0.0 && col.over > 0.0 && lobes < 1.0 {
        let uv = pos.xz * v.nscale.x + v.nphase.xy;
        let shoulder = vc_m2_caps(uv, 16u, 0.37, width, v);
        let head = vc_m2_caps(uv, 32u, 0.0, width, v);
        col.medium_head = head;
        // [hypothesis] Lean the smallest heads along one diagonal. The integer
        // shear wraps with the noise tile; its largest singular value filters the tuft.
        let tuft_uv = vec2<f32>(uv.x + uv.y, uv.y);
        let bud = vc_m2_caps(tuft_uv, 64u, 0.37, 1.6181 * width, v);
        let crest = 0.72 * shoulder + mix(0.75, 1.35, smoothstep(-0.30, 0.35, shoulder)) * head;
        // [hypothesis] Lift a whole shoulder and carry narrower domes up it.
        // The bank gates development while the individual radii select the peaks.
        let development = smoothstep(0.035, 0.15, col.over);
        let cluster = smoothstep(0.10, 0.50, shoulder);
        let tower = development * (0.80 * max(shoulder, 0.0) + cluster * (0.35 + 4.40 * max(head, 0.0)));
        let nested = tower + crest + mix(0.55, 1.12, smoothstep(-0.25, 0.40, head)) * bud;
        let attachment = smoothstep(0.0, 0.045, col.over);
        col.head = fair * attachment * (max(nested, 0.0) + 0.40 * min(nested, 0.0));
    }
    if col.over > 0.0 {
        col.head = mix(col.head, 1.55, lobes);
    }
    return col;
}

// A storm front spans four placement cells along one axis and two across. Keep
// the whole signed cell separate from its fraction so a wrapped wind phase
// preserves the front and its smaller water pockets. Return (bed, local water).
fn vc_m2_deck_relief(pos: vec3<f32>, width: f32, v: CloudVolume) -> vec2<f32> {
    let p = vc_m2_place_pos(pos);
    let pw = width * (1.0 + VC_M2_SHEAR);
    let medium = v.field.x;
    let uv = p.xz * v.map.xy * medium + v.place0.xy;
    let cell = vec2<i32>(floor(uv)) + bitcast<vec2<i32>>(v.cells_p.xy);
    let frac = (vec2<f32>(cell & vec2<i32>(3, 1)) + fract(uv)) * vec2<f32>(0.25, 0.5);
    let sector = cell >> vec2<u32>(2u, 1u);
    // Wrap the fine cell before division on both sides of a signed phase seam.
    let next_sector = (cell + vec2<i32>(4, 2)) >> vec2<u32>(2u, 1u);
    let broad = vc_noise_interp(vc_lattice_corners(sector, next_sector, VC_SEED_TOP2), frac);
    let local = vc_noise2(cell, fract(uv), VC_SEED_TOP);
    let band = vc_octave_keep(medium, pw, v) * (local - 0.5);
    let fine = medium * VC_PLACE_F1;
    let detail = vc_place_octave(p, fine, v.place1.xy, bitcast<vec2<i32>>(v.cells_p.zw), VC_SEED_TOP2, v);
    let bed = 0.78 * vc_octave_keep(0.5 * medium, pw, v) * (broad - 0.5) + 0.22 * band;
    let water = 0.65 * band + 0.35 * vc_octave_keep(fine, pw, v) * (detail - 0.5);
    return vec2<f32>(bed, water);
}

// The placement excess over its threshold at which the cover is whole (a smooth step up to it).
const VC_M2_COVER: f32 = 0.12;
// Per type (sheet, cumulus, tower): the height as a share of the domain (times the vigour's scale), the
// base rescale's relief at SHAPE 1, the surface's least transition (units), the extinction per unit at
// density 1 and DENSITY 1, and the erosion's strength.
const VC_M2_EXTENT: vec3<f32> = vec3<f32>(0.11, 0.14, 0.48);
const VC_M2_RELIEF: vec3<f32> = vec3<f32>(0.25, 0.75, 0.6);
const VC_M2_EDGE: vec3<f32> = vec3<f32>(600.0, 240.0, 300.0);
const VC_M2_SIGMA: vec3<f32> = vec3<f32>(1.5e-3, 2.5e-3, 3.0e-3);
const VC_M2_ERODE_TYPE: vec3<f32> = vec3<f32>(0.3, 1.0, 0.8);
// The erosion read's frequency over its texture's own repeat (cells 2,000 / 1,000 / 500 units; a whole number, so the
// drift's phase scaled by it moves whole repeats where it wraps).
const VC_M2_ERODE_FREQ: f32 = 2.0;
// Each erosion band's surface displacement at EROSION 1, as a share of its cell.
const VC_M2_BILLOW: f32 = 1.15;
// The least field change per unit (`vc_m2_grad`): where the relief is small the envelope's own slope
// rules.
const VC_M2_GRAD_LEAST: f32 = 1.0e-4;

// The share of the column above a sample that holds cloud, for the light's vertical estimate
// (`vc_incident`).
const VC_M2_FILL: f32 = 0.6;
// The wisps' gradient per WISP per unit of amplitude, a little over `vc_wisp`'s bound
// (volumetric_clouds_wisp.rs `wisp_bounds`: 2.035e-3), for the erosion's step bound.
const VC_M2_WISP_GRAD: f32 = 2.04e-3;

// The cover of a column, 0..1: a smooth step of its placement excess up to VC_M2_COVER (0 wherever the
// field does not pass the threshold, so the cover is above 0 exactly where the edge share is).
fn vc_m2_cover(col: VcColumn) -> f32 {
    return smoothstep(0.0, VC_M2_COVER, col.over);
}

// A column's height (units): the types' shares of the domain times the vigour's scale, at least a sheet,
// at most the domain.
fn vc_m2_parent_extent(col: VcColumn, v: CloudVolume) -> f32 {
    let depth = max(v.slab.y, 1.0);
    let scale = v.scale.x + (v.scale.y - v.scale.x) * pow(clamp(col.vigour, 0.0, 1.0), max(v.scale.z, 1e-3));
    let bank = smoothstep(0.55, 0.85, v.scale.w) * smoothstep(0.26, 0.85, col.over);
    let core = (1.0 - smoothstep(0.55, 0.72, v.scale.w)) * smoothstep(0.055, 0.16, col.over) * clamp(v.nscale.z, 0.0, 1.0);
    return clamp(dot(col.types, VC_M2_EXTENT) * depth * scale * (1.0 + 0.55 * bank + 0.20 * core), v.field.w, depth);
}

// [hypothesis] The lowest 36% of the parent supports the whole bank. Heads can
// widen its upper sides and stretch its roof. The placement still owns occupancy.
const VC_M2_FOOT: f32 = 0.36;
fn vc_m2_extent(col: VcColumn, v: CloudVolume) -> f32 {
    return clamp(col.local + (1.0 - VC_M2_FOOT) * col.local * col.head, v.field.w, max(v.slab.y, 1.0));
}

// [documented: vc_types, vc_m2_parent_extent and vc_m2_extent.] In pure fair
// cumulus the local type cannot change, the core gain is at most 0.20, and
// every occupied lobe column has head 1.55. Reject the empty height above
// even the most vigorous bank before looking up placement at a sample.
fn vc_sampling_ceiling(v: CloudVolume) -> f32 {
    let domain = max(v.slab.y, 1.0);
    if v.mix.x == 0.0 && v.mix.z == 0.0 &&
        vc_m2_lobe_weight(vec3<f32>(0.0, 1.0, 0.0), v) >= 1.0 {
        let parent = max(v.field.w, VC_M2_EXTENT.y * domain * max(v.scale.x, v.scale.y) * 1.20);
        return v.slab.x + min(domain, parent * (1.0 + (1.0 - VC_M2_FOOT) * 1.55) + 1.0);
    }
    return v.slab.x + domain;
}

fn vc_m2_stretch(y: f32, src: f32, dst: f32, foot: f32) -> f32 {
    return min(y, foot) + max(y - foot, 0.0) * ((dst - foot) / max(src - foot, 1.0));
}

// The base rescale's relief for the types `w` (sheets low so overcast stays whole, cumulus high), times
// SHAPE.
fn vc_m2_relief(w: vec3<f32>, v: CloudVolume) -> f32 {
    return dot(w, VC_M2_RELIEF) * max(v.nscale.z, 0.0);
}

// The field's change per world unit at relief `r`: the base noise's spread value's measured mean gradient
// (VC_BASE_GRAD per repeat) times the relief, at least VC_M2_GRAD_LEAST.
fn vc_m2_grad(r: f32, v: CloudVolume) -> f32 {
    return max(r * VC_BASE_GRAD * v.nscale.x, VC_M2_GRAD_LEAST);
}

// The erosion read's three cells (units).
fn vc_m2_cells(v: CloudVolume) -> vec3<f32> {
    let repeat = 1.0 / max(v.nscale.y * VC_M2_ERODE_FREQ, 1e-9);
    return vec3<f32>(repeat / 3.0, repeat / 6.0, repeat / 12.0);
}

// The erosion's displacement per unit of a band's form (world units per cell unit) for the types `w`:
// VC_M2_BILLOW times EROSION times the type's strength.
fn vc_m2_billow(w: vec3<f32>, v: CloudVolume) -> f32 {
    return VC_M2_BILLOW * max(v.nscale.w, 0.0) * dot(w, VC_M2_ERODE_TYPE);
}

// The height share where the envelope at cover `c` meets the threshold `tau` (the mixed profile is 1 at
// its foot and falls to 0 at its top), times `min(c / tau, 1)`: continuous in the cover (no step where the
// cover passes the threshold) and 0 with no cover. The shading's estimate of how
// high this column's cloud stands.
fn vc_m2_top(c: f32, tau: f32, w: vec3<f32>, v: CloudVolume) -> f32 {
    if tau <= 0.0 {
        return 1.0;
    }
    let a = w.x * v.knot_s0 + w.y * v.knot_c0 + w.z * v.knot_t0;
    let b = w.x * v.knot_s1 + w.y * v.knot_c1 + w.z * v.knot_t1;
    let need = min(tau / max(c, 1e-6), a.x);
    var k = array<f32, 8>(a.x, a.y, a.z, a.w, b.x, b.y, b.z, b.w);
    var top = 1.0;
    for (var i = 1u; i < 8u; i++) {
        if k[i] < need {
            let f = clamp((k[i - 1u] - need) / max(k[i - 1u] - k[i], 1e-6), 0.0, 1.0);
            top = (f32(i - 1u) + f) / 7.0;
            break;
        }
    }
    return top * min(c / tau, 1.0);
}

// A column's lean geometry, once per evaluation (`vc_prepare_column`): x the cover, y the height (units),
// z the parent bank's top share at median noise, w the relief.
fn vc_m2_geometry(col: VcColumn, v: CloudVolume) -> vec4<f32> {
    let c = vc_m2_cover(col);
    let r = vc_m2_relief(col.types, v);
    let tau = 0.5 * r;
    let height = vc_m2_extent(col, v);
    let top = vc_m2_top(c, tau, col.types, v);
    return vec4<f32>(c, height, top, r);
}

// The texture and drift use the same whole repeat on every axis.
fn vc_m2_erode_coord(pos: vec3<f32>, yb: f32, v: CloudVolume) -> vec3<f32> {
    return VC_M2_ERODE_FREQ * vc_erosion_coord(pos, yb, v);
}

// How much of each of the erosion read's three bands a filter width `ew` keeps (`vc_band_keep`).
fn vc_m2_keeps(ew: f32, v: CloudVolume) -> vec3<f32> {
    let cells = vc_m2_cells(v);
    return vec3<f32>(vc_band_keep(cells.x, ew), vc_band_keep(cells.y, ew), vc_band_keep(cells.z, ew));
}

// [hypothesis] Near the foot the diffuse vertical path sees the bank. Transferring
// every local cap into that path paints its circular footprint on the underside.
fn vc_m2_light_height(column: VcPreparedColumn, above: f32) -> f32 {
    let parent = column.col.local;
    let base_top = parent * column.m2.z;
    return mix(base_top, column.crown_top, smoothstep(0.52, 0.88, above / max(parent, 1.0)));
}

// The lean model's density (0..1) at `above` units over the flat base, and the height share for the
// shading (from the base to the column's estimated top). `eroded` false is the light: the base and the
// erosion read at the light's filter width (twice the wider of the footprint and the light volume's
// cell), no wisps; the view's filter is the wider of the cast width and `fw` (the
// step it takes, `vc_detail_filter`).
fn vc_m2_density(column: VcPreparedColumn, above: f32, pos: vec3<f32>, yb: f32, width: f32, fw: f32, eroded: bool, detail: bool, nb: texture_3d<f32>, ns: sampler, ne: texture_3d<f32>, v: CloudVolume) -> vec2<f32> {
    let c = column.m2.x;
    let physical_h = above / max(column.m2.y, 1.0);
    let hs = clamp(above / max(vc_m2_light_height(column, above), 1.0), 0.0, 1.0);
    if c <= 0.0 || physical_h >= 1.0 {
        return vec2<f32>(0.0, hs);
    }
    let w = column.col.types;
    let parent = column.col.local;
    let h = vc_m2_stretch(above, column.m2.y, parent, VC_M2_FOOT * parent) / parent;
    let storm = smoothstep(0.55, 0.72, v.scale.w) * smoothstep(0.0, 0.08, w.x);
    let upper = smoothstep(VC_M2_FOOT, 0.56, h);
    // [hypothesis] Retreat between the broad shoulder and its elevated head.
    // The variable join height follows the local plume. Its axis remains filled.
    let lift = max(0.0, column.m2.y / parent - 1.0);
    let join = abs(above / parent - (0.88 + 0.24 * lift));
    let neck = 0.46 * smoothstep(0.45, 1.45, lift) * (1.0 - smoothstep(0.0, 0.33, join));
    let e = mix(c, column.crown_cover, upper) * clamp(vc_profile(h, w, v), 0.0, 1.0) * (1.0 - neck);
    if e <= 0.0 {
        return vec2<f32>(0.0, hs);
    }
    var lw = width;
    if eroded {
        let step_width = max(width, fw);
        // Filter the sheet over the same view footprint as the lobes it connects.
        let sample_area = sqrt(max(1.0, width * v.dshape.w) * step_width);
        lw = mix(step_width, sample_area, vc_m2_mixed_sheet(v));
    } else {
        lw = 2.0 * max(width, vc_light_cell(v));
    }
    // Keep the fair interior optically connected below the heads, including
    // where the base texture reaches its deepest cut. The outer rim stays thin.
    let core = (1.0 - smoothstep(0.55, 0.72, v.scale.w)) * smoothstep(0.50, 0.90, c) * (1.0 - upper);
    let r = column.m2.w * (1.0 - 0.60 * core);
    let g = vc_m2_grad(r, v);
    // [hypothesis] The filtered light field needs a symmetric transition about the near
    // isosurface. Its footprint can be wider than a cloud, even deep inside an opaque bank.
    let intrinsic = g * dot(w, VC_M2_EDGE);
    let footprint = 2.0 * g * lw;
    let ramp = max(intrinsic, select(min(footprint, 0.42), footprint, eroded));
    let offset = select(min(0.5 * (ramp - intrinsic), 0.5 * e), 0.0, eroded);
    let cells = vc_m2_cells(v);
    // [hypothesis] Thin sides fray below the crowns as well. The placement
    // envelope keeps the clear lanes open and the core optically connected.
    let fair = (1.0 - smoothstep(0.55, 0.72, v.scale.w)) * clamp(v.nscale.z, 0.0, 1.0);
    let side = fair * (1.0 - smoothstep(0.16, 0.46, c));
    let amp = (1.0 - storm) * vc_m2_billow(w, v) * max(upper, side) * min(1.0, 0.12 + 2.0 * (1.0 - e));
    let most = g * amp * (0.5 * (cells.x + cells.y + cells.z) + cells.y);
    let widest = ramp + 0.5 * g * amp * cells.y;
    // [hypothesis] A resolved tongue can extend past the dense isosurface.
    // Bound its reach before the empty-space rejection, including in the light field.
    let reach = 0.58 * g * amp * cells.x * (1.0 - smoothstep(0.55, 0.72, v.scale.w));
    // [hypothesis] Dry patches in a shallow mixed bank condense a little higher.
    // Use the humidity the lobes above this column already sample.
    let mixed_sheet = vc_m2_mixed_sheet(v);
    let dry_base = 1.0 - smoothstep(0.25, 0.75, column.base_humidity);
    let bed = 0.48 * parent * max(-column.deck_relief.x, 0.0)
        + mixed_sheet * dry_base * min(0.16 * parent, 600.0);
    let soft = max(0.6 * v.profile.x, 1.0);
    // [hypothesis] Resolve a displaced lower surface inside the storm bank.
    // Pure sheets keep their continuous floor. Bound its highest possible bed
    // before skipping either the noise read or the lower transition.
    // [hypothesis] A common condensation altitude bounds a shallow turbulent
    // skin. It tapers inward, so the bank stays level when seen side-on.
    let fair_skin = fair * (0.32 + 0.68 * side) * min(0.34 * parent, 1150.0);
    let storm_skin = storm * (1.0 - w.x) * min(0.25 * parent, 1350.0);
    let skin = fair_skin + storm_skin;
    // At zero noise the dry pocket reaches its highest edge. Only above its
    // water ramp can the whole sample be accepted without a texture read.
    let edge_lift = 0.12 * storm * storm_skin;
    if e - r - most + offset >= widest && above >= bed + edge_lift + 0.65 * skin + max(soft, 1.5 * skin) {
        return vec2<f32>(1.0, hs);
    }
    // Filter individual base cells near the foot while retaining broader texture in the deck.
    let base_width = max(lw, mix(1600.0 * (1.0 - max(upper, side)), 240.0, storm));
    let uv = vc_base_coord(pos, yb, v);
    let b = textureSampleLevel(nb, ns, uv, vc_level(base_width, v.nlevel.x, v.nscale.x, v.nlevel.y)).r;
    var n = vc_spread_base(b, v);
    // Break up the lower face of a mixed deck inside its existing support. Three
    // whole repeats keep the detail attached as the wind phase wraps.
    let deck_foot = storm * (1.0 - w.x) * (1.0 - smoothstep(0.30, 0.65, h));
    if deck_foot > 0.0 {
        let bend = vec3<f32>(column.deck_relief.y, n - 0.5, column.deck_relief.x);
        let detail_uv = 3.0 * uv + vec3<f32>(0.41, 0.37, 0.18) + 0.10 * bend;
        let local = vc_spread_base(textureSampleLevel(nb, ns, detail_uv,
            vc_level(base_width, v.nlevel.x, 3.0 * v.nscale.x, v.nlevel.y)).r, v);
        n = clamp(n + deck_foot * 0.90 * (local - 0.5), 0.0, 1.0);
    }
    let lower_edge = max(0.0, bed + edge_lift + skin * (0.65 - n));
    // Thin pockets contain less water just above the displaced edge. Keep
    // this local optical contrast under the broad deck, including from below.
    let height_in_bank = above - lower_edge;
    let pocket = max(storm * (1.0 - w.x), 0.45 * mixed_sheet) * (1.0 - smoothstep(0.20, 0.65, n));
    let water = 1.0 - 0.76 * pocket * (1.0 - smoothstep(0.0, max(1.5 * skin, 1.0), height_in_bank));
    let bottom = water * smoothstep(0.0, soft, height_in_bank);
    let cut = r * (1.0 - mix(n, 0.5, core));
    let least = 0.08 * r;
    if e - max(cut - most, least) + offset + reach <= 0.0 {
        return vec2<f32>(0.0, hs);
    }
    if e - max(cut + most, least) + offset >= widest || amp <= 0.0 {
        return vec2<f32>(bottom * smoothstep(0.0, ramp, e - max(cut, least) + offset), hs);
    }
    var epos = pos;
    var ew = lw;
    if eroded && detail && v.dshape.z > 0.0 {
        let wp = vc_wisp(pos, yb, v);
        epos = pos + vec3<f32>(wp.d.x, 0.0, wp.d.y);
        ew = lw * (1.0 + wp.g);
    }
    let keep = vc_m2_keeps(ew, v);
    var ep = vec3<f32>(0.5);
    if keep.x + keep.y + keep.z > 0.0 {
        let q = v.nscale.y * VC_M2_ERODE_FREQ;
        let ev = textureSampleLevel(ne, ns, vc_m2_erode_coord(epos, yb, v), vc_level(ew, v.nlevel.z, q, v.nlevel.w)).rgb;
        ep = clamp(vec3<f32>(0.25) + 0.5 * (ev - vec3<f32>(v.nerode.x)) / max(v.nerode.y - v.nerode.x, 1e-3), vec3<f32>(0.0), vec3<f32>(1.0));
    }
    // The upper shell uses signed billows. Each unresolved band converges to its mean.
    let x = vec3<f32>(0.5) + keep * (ep - vec3<f32>(0.5));
    // [documented: Schneider & Vos 2015, Worley detail and height-dependent inversion.]
    // [hypothesis] Centring each band gives both protruding heads and recesses. Limit
    // outward growth to the placement envelope so even a high head tapers at its support.
    let form = vec3<f32>(0.5) - x;
    // [hypothesis] Where two resolved erosion scales disagree, cut into the
    // upper surface. The finer band interrupts these channels along their length.
    let cross_band = 2.5 * abs(x.y - x.x) + 1.4 * abs(x.z - x.y);
    let tear = keep.x * min(1.0, cross_band) * (0.35 + 0.65 * x.z);
    let f = e - max(cut + g * amp * (dot(form, cells) + tear * cells.y), least) + offset;
    // [hypothesis] Retain a thin, broken veil within the material removed by
    // the crossing bands. It vanishes with the resolved tear at coarse footprints.
    let fringe = g * amp * tear * cells.y;
    let dense = smoothstep(0.0, ramp + 0.5 * fringe, f);
    let veil = 0.16 * tear * smoothstep(0.0, ramp, f + 0.70 * fringe);
    // Use the mip-filtered coarse sample here. Applying the footprint fade
    // before this threshold as well would erase the fringe twice.
    let tuft = keep.x * smoothstep(0.52, 0.78, ep.x) * smoothstep(0.12, 0.42, x.y) * (1.0 - smoothstep(0.60, 0.82, x.y));
    let tip = 0.26 * tuft * (smoothstep(0.0, ramp, f + reach * tuft) - smoothstep(0.0, ramp, f));
    return vec2<f32>(bottom * max(dense, max(veil, tip)), hs);
}

// [hypothesis] Broad overlapping ellipsoids rise from one condensation band. Distances
// use local coordinates while lattice IDs wrap. Each vertex's influence fades out before
// it leaves the four-vertex stencil, including where turbulence displaces its surface.
fn vc_m2_lobe_field(uv: vec2<f32>, h: f32, period: u32, lift: f32, shift: vec2<f32>) -> vec2<f32> {
    let p = fract(uv) * f32(period) + shift;
    let tile = vec2<u32>(floor(p));
    let local = fract(p);
    var surface = -2.0;
    var roof = 0.0;
    for (var k = 0u; k < 4u; k++) {
        let corner = vec2<u32>(k & 1u, k >> 1u);
        let id = (tile + corner) & vec2<u32>(period - 1u);
        let bits = u32(vc_hash(vec3<u32>(id.x, 91u + period, id.y)) * 16777216.0);
        let rnd = vec3<f32>(vec3<u32>(bits & 255u, (bits >> 8u) & 255u, bits >> 16u)) / 255.0;
        let offset = local - vec2<f32>(corner);
        let reach = 1.0 - smoothstep(0.82, 1.0, max(abs(offset.x), abs(offset.y)));
        let delta = offset - (rnd.xy - vec2<f32>(0.5)) * 0.26;
        let radius = 0.54 + 0.31 * rnd.z;
        let lateral = dot(delta, delta) / (radius * radius);
        let half_height = lift * (0.23 + 0.47 * rnd.z);
        let centre = 0.14 + half_height + 0.10 * rnd.y;
        let vertical = (h - centre) / half_height;
        let distance = sqrt(lateral + vertical * vertical);
        surface = max(surface, mix(-2.0, half_height * (1.0 - distance), reach));
        let top = centre + half_height * sqrt(max(0.0, 1.0 - lateral));
        roof = max(roof, reach * top * (1.0 - smoothstep(0.82, 1.04, lateral)));
    }
    return vec2<f32>(surface, roof);
}

// Use the same physical lobe dimensions for the density and its vertical light path.
struct VcFairProfile {
    bank: f32,
    crown: f32,
    height: f32,
    depth_scale: f32,
    plume_scale: f32,
}

// [hypothesis] Plumes share a trunk. Above it, their height follows local water supply.
// The material uses the inverse scale to sample the original lobe field.
fn vc_m2_plume_height(height: f32, parent: f32, scale: f32) -> f32 {
    return height + max(height - 0.28 * parent, 0.0) * (scale - 1.0);
}

fn vc_m2_fair_profile(column: VcPreparedColumn, v: CloudVolume) -> VcFairProfile {
    let bank = smoothstep(0.012, 0.20, column.col.over);
    let parent = column.col.local;
    let growth = 0.58 * bank * smoothstep(0.32, 1.30, column.fair_water_excess);
    let crown = mix(parent, column.m2.y, growth);
    // [hypothesis] A narrow humid patch sustains less vertical growth.
    let radius = 1.24 * max(0.0, column.col.over) / max(1e-9, v.field.x * v.map.x);
    let permitted = 320.0 + 0.43 * radius;
    let depth_scale = mix(1.0, min(1.0, permitted / max(crown, 1.0)), vc_m2_open_bank_weight(v) * (1.0 - column.col.developed_bank));
    let water = smoothstep(0.80, 1.38, column.fair_water_excess);
    let plume_scale = mix(1.0, 0.42 + 0.58 * water, vc_m2_open_bank_weight(v) * column.col.developed_bank);
    return VcFairProfile(bank, crown, parent * (0.53 + 0.47 * bank), depth_scale, plume_scale);
}

// [hypothesis] Drier air rises farther before condensing. Restrict the displacement
// to the lower part of the resolved fair bank, below its shared middle altitude.
fn vc_m2_base_onset(base_humidity: f32, turbulence: f32, v: CloudVolume) -> f32 {
    let filament = smoothstep(-0.22, 0.20, turbulence);
    let humidity = 0.52 * base_humidity + 0.48 * filament;
    let dry = 1.0 - smoothstep(0.25, 0.76, humidity);
    return (0.025 + 0.305 * dry) * vc_m2_open_bank_weight(v);
}

// How far the erosion bands move the lobes' fine read, in its noise coordinates [design].
const VC_M2_FRAY: f32 = 0.10;

// [documented: Schneider and Vos 2015, Perlin-Worley base and detail erosion.]
// [hypothesis] The signed displacement crosses the whole flank. A shallow core connects
// neighbouring lobes only inside a strong updraft. Thinner margins stay translucent.
fn vc_m2_lobes(column: VcPreparedColumn, above: f32, pos: vec3<f32>, yb: f32, width: f32, nb: texture_3d<f32>, ns: sampler, ne: texture_3d<f32>, v: CloudVolume) -> vec2<f32> {
    let uvw = vc_base_coord(pos, yb, v);
    let lod = vc_level(width, v.nlevel.x, v.nscale.x, v.nlevel.y);
    let layer = textureSampleLevel(nb, ns, uvw + vec3<f32>(0.17, 0.42, 0.21), lod).r;
    let warp = vc_spread_base(layer, v) - 0.5;
    let uv = uvw.xz + vec2<f32>(0.014, -0.017) * warp;
    let profile = vc_m2_fair_profile(column, v);
    let bank = profile.bank;
    let parent = column.col.local;
    let developed_above = above / profile.depth_scale;
    let uncompressed = vc_m2_plume_height(developed_above, parent, 1.0 / profile.plume_scale);
    let y = vc_m2_stretch(uncompressed, profile.crown, parent, VC_M2_FOOT * parent);
    let coarse_uv = 2.0 * uvw + vec3<f32>(0.17, 0.39, 0.68);
    let coarse = vc_spread_base(textureSampleLevel(nb, ns, coarse_uv,
        vc_level(width, v.nlevel.x, 2.0 * v.nscale.x, v.nlevel.y)).r, v) - 0.5;
    let erosion = textureSampleLevel(ne, ns, vc_erosion_coord(pos, yb, v),
        vc_level(width, v.nlevel.z, v.nscale.y, v.nlevel.w)).rgb - v.nband.xyz;
    // The three independent erosion bands displace the fine read. One scalar along one
    // direction would shear it into strands.
    let fine_uv = 6.0 * uvw + VC_M2_FRAY * erosion.yzx;
    let fine = vc_spread_base(textureSampleLevel(nb, ns, fine_uv,
        vc_level(width, v.nlevel.x, 6.0 * v.nscale.x, v.nlevel.y)).r, v) - 0.5;
    let raw_height = y / profile.height;
    let onset = vc_m2_base_onset(column.base_humidity, 0.75 * coarse + 0.30 * fine + 0.20 * warp, v);
    let h = raw_height - max(0.52 - raw_height, 0.0) * onset / (0.52 - onset);
    let large = vc_m2_lobe_field(uv, h, 8u, 0.70, vec2<f32>(0.23, 0.47));
    let exterior = 1.0 - smoothstep(0.0, 0.14, column.col.over);
    let bridge = 0.31 + 0.075 * warp - abs(h - 0.39 - 0.045 * warp) - 0.25 * (1.0 - bank);
    // Two finer bands of the base noise break up the rim, each gone where its own footprint is too wide.
    let rim = vc_spread_base(textureSampleLevel(nb, ns, 13.0 * uvw + vec3<f32>(0.61, 0.07, 0.33),
        vc_level(width, v.nlevel.x, 13.0 * v.nscale.x, v.nlevel.y)).r, v) - 0.5;
    let finest = vc_spread_base(textureSampleLevel(nb, ns, 27.0 * uvw + vec3<f32>(0.18, 0.52, 0.77),
        vc_level(width, v.nlevel.x, 27.0 * v.nscale.x, v.nlevel.y)).r, v) - 0.5;
    let detail = 0.32 * fine + dot(erosion, vec3<f32>(0.46, 0.21, 0.10)) + 0.20 * rim + 0.12 * finest;
    let shelf = bridge + 0.11 * coarse - 0.38 * exterior;
    // The two filtered volumes form narrower, irregular heads on the large
    // ellipsoids, using the same samples that break up their sides.
    let shoulder = large.y * smoothstep(0.025, 0.17, column.col.over);
    let tower = smoothstep(0.22, 0.71, shoulder);
    let small_top = shoulder + tower * (0.08 + 0.58 * smoothstep(-0.21, 0.25, coarse + 0.35 * fine));
    let cap = min(h - max(0.30, 0.68 * shoulder), small_top - h);
    let heads = max(large.x + 0.42 * coarse, cap - 0.59 * (1.0 - tower)) + 0.16 * warp - 0.67 * exterior;
    let erosion_strength = clamp(v.nscale.w / 1.26, 0.0, 2.0);
    let field = max(shelf, heads) + erosion_strength * detail;
    let soft = clamp(width / column.col.local, 0.14, 0.32);
    let foot = smoothstep(0.02, 0.13, h);
    let roof = mix(0.75 + 0.44 * column.m2.x, max(0.5, max(large.y, small_top)), 0.45);
    let core = smoothstep(-soft, soft, field) * (0.30 + 0.70 * smoothstep(-0.06, 0.26, field));
    // A few dilute fragments beyond the edge inherit the bank's filtered
    // turbulence. Keep the lower band more sharply defined than its crown.
    let margin = smoothstep(-0.42 - soft, -0.06, field);
    let fragments = smoothstep(-0.10, 0.34, 0.70 * fine + 0.40 * coarse + dot(erosion, vec3<f32>(0.58, 0.32, 0.18)));
    let fringe_water = 0.065 + 0.075 * smoothstep(0.32, 0.72, h) + 0.035 * exterior;
    let density = max(core, vc_m2_open_bank_weight(v) * fringe_water * margin * fragments * min(erosion_strength, 1.0));
    let rim_width = mix(0.012, 0.045, vc_m2_mixed_sheet(v));
    let boundary = (1.0 - smoothstep(0.90, 1.0, developed_above / column.m2.y)) * smoothstep(0.0, rim_width, column.col.over);
    return vec2<f32>(foot * density * boundary, clamp(h / roof, 0.0, 1.0));
}

fn vc_m2_material(column: VcPreparedColumn, above: f32, pos: vec3<f32>, yb: f32, width: f32, pwidth: f32, fw: f32, eroded: bool, detail: bool, nb: texture_3d<f32>, ns: sampler, ne: texture_3d<f32>, v: CloudVolume) -> vec2<f32> {
    let blend = vc_m2_lobe_weight(column.col.types, v);
    if blend <= 0.0 {
        return vc_m2_density(column, above, pos, yb, width, fw, eroded, detail, nb, ns, ne, v);
    }
    if above >= column.m2.y || column.m2.x <= 0.0 {
        return vec2<f32>(0.0);
    }
    // The reconstructed texel resolves detail across the ray. Along it, the march
    // still integrates the full step. Light-volume samples filter at their own cell size.
    let across = max(width * v.dshape.w, 1.0);
    let along = max(max(width, 1.0), fw);
    let footprint = select(2.0 * max(width, vc_light_cell(v)), sqrt(across * along), eroded);
    let puff = vc_m2_lobes(column, above, pos, yb, footprint, nb, ns, ne, v);
    if blend >= 1.0 {
        return puff;
    }
    let bank = vc_m2_density(column, above, pos, yb, width, fw, eroded, detail, nb, ns, ne, v);
    // The shallow sheet connects wet lobes through the center of a bank.
    // Away from them, let the sheet thin as the column approaches its edge.
    let condensed = smoothstep(0.025, 0.48, puff.x);
    let shallow_water = mix(0.015 + 0.40 * column.m2.x, 1.0, condensed);
    let mixed_sheet = vc_m2_mixed_sheet(v);
    let attached = smoothstep(0.0, 0.25, blend);
    let bank_water = mix(1.0, shallow_water, mixed_sheet * attached);
    var lobe_share = blend;
    if eroded {
        // Recover the projected area's aspect from like placement widths, including
        // during weather transitions. Estimate the gaps crossed by the view texel.
        // Sun taps and the world light grid both integrate the physical sheet.
        let shoulder = 1.0 / max(32.0 * v.nscale.x, 1e-8);
        let placement_across = max(width * mix(1.0, v.dshape.w, mixed_sheet), 1.0);
        let aspect = pwidth / placement_across;
        let extra_span = across * max(aspect * aspect - 1.0, 0.0);
        let clustered = 0.90 * mixed_sheet * attached * smoothstep(0.09 * shoulder, 0.31 * shoulder, extra_span);
        lobe_share = mix(blend, 1.0, clustered);
    }
    let shares = vec2<f32>((1.0 - lobe_share) * bank_water * bank.x, lobe_share * puff.x);
    let total = shares.x + shares.y;
    return vec2<f32>(total, dot(shares, vec2<f32>(bank.y, puff.y)) / max(total, 1e-8));
}

// The lean model's extinction per unit at density 1 for the types `w` (times DENSITY).
fn vc_m2_sigma(w: vec3<f32>, v: CloudVolume) -> f32 {
    return dot(w, VC_M2_SIGMA) * v.march.z;
}

// The lean model's sample (x the shape, y the extinction per unit at shape 1 there, z the weather's amount,
// w the height share for the shading): the density over the flat base,
// faded in over `profile.x` units, at its own extinction per unit; where the footprint does not resolve
// the placement (`col.res` under 1: a grazing footprint wider than twice the first octave's
// cell), blended with the expected cover of an unresolved footprint, a uniform thin cover of the requested sky
// share (`profile.y`) one sheet tall, the types' profile over it, at the weather table's extinction over a
// sheet, never carved: the two optical densities mixed by `res`, the extinction mixed by `res` (so
// res 1 is the resolved sample exactly), the height share weighted by the optical densities.
fn vc_m2_shape(pos: vec3<f32>, yb: f32, above: f32, column: VcPreparedColumn, width: f32, pwidth: f32, nb: texture_3d<f32>, ns: sampler, ne: texture_3d<f32>, eroded: bool, detail: bool, fw: f32, v: CloudVolume) -> vec4<f32> {
    let col = column.col;
    let fade = smoothstep(0.0, max(v.profile.x, 1.0), above);
    let amount = clamp(v.scale.w, 0.0, 1.0);
    // [hypothesis] A water-rich storm bank needs a longer optical path than fair
    // cloud of the same type and thickness. Thin banks still transmit sunlight
    // through their cloud cover. The light and view marches use the same water.
    let deck = smoothstep(0.55, 0.75, amount);
    let bank_mass = 0.72 * (col.over + v.field.y) + 0.28 * col.vigour + 0.65 * column.deck_relief.y;
    let local_water = 0.23 + 2.12 * smoothstep(0.28, 0.74, bank_mass);
    let front_mass = 0.48 + 0.53 * (col.over + v.field.y - 0.5) + 0.20 * (col.vigour - 0.5) + 0.68 * column.deck_relief.x;
    let front_water = 0.48 + 1.88 * smoothstep(0.25, 0.73, front_mass);
    // A thin column at the edge of a front still transmits sunlight through the deck.
    let front_cover = mix(0.24, 1.0, smoothstep(0.30, 0.52, col.over + v.field.y));
    let water = max(0.88 * local_water, front_water * front_cover);
    var sigma = vc_m2_sigma(col.types, v) * mix(1.0 + column.fair_water_excess, water, deck);
    var shape = 0.0;
    var h = 0.0;
    if col.edge > 0.0 {
        let m2 = vc_m2_material(column, above, pos, yb, width, pwidth, fw, eroded, detail, nb, ns, ne, v);
        shape = m2.x;
        h = m2.y;
    }
    // [hypothesis] Mixing within the lower part of a fair bank changes the
    // water concentration even where its geometric envelope is fully occupied.
    // Both the camera and sunlight encounter the same wind-advected parcels.
    let lower_bank = vc_m2_lobe_weight(col.types, v) * vc_m2_open_bank_weight(v) * col.res
        * smoothstep(0.04, 0.20, col.over) * (1.0 - smoothstep(0.36, 0.76, h));
    if shape > 0.0 && lower_bank > 0.0 {
        let uvw = vc_base_coord(pos, yb, v);
        let broad = vc_spread_base(textureSampleLevel(nb, ns, 3.0 * uvw + vec3<f32>(0.41, 0.37, 0.18),
            vc_level(width, v.nlevel.x, 3.0 * v.nscale.x, v.nlevel.y)).r, v) - 0.5;
        let local = vc_spread_base(textureSampleLevel(nb, ns, 6.0 * uvw + vec3<f32>(0.23, 0.65, 0.53),
            vc_level(width, v.nlevel.x, 6.0 * v.nscale.x, v.nlevel.y)).r, v) - 0.5;
        // The broad parcels are large round patches: a small share only. The finer ones mottle the face.
        sigma *= exp2(lower_bank * (0.45 * broad + 0.72 * local));
    }
    if col.res >= 1.0 {
        if shape <= 0.0 {
            return vec4<f32>(0.0);
        }
        return vec4<f32>(shape * fade, sigma, amount, h);
    }
    let sheet_h = max(v.field.w, 1.0);
    var sheet = 0.0;
    if above < sheet_h {
        sheet = clamp(v.profile.y, 0.0, 1.0) * vc_profile(above / sheet_h, col.types, v);
    }
    let ks = vc_extinction(amount, sheet_h, v);
    let cr = col.res * shape * sigma;
    let cs = (1.0 - col.res) * sheet * ks;
    let k = mix(ks, sigma, col.res);
    if cr + cs <= 0.0 || k <= 0.0 {
        return vec4<f32>(0.0);
    }
    return vec4<f32>((cr + cs) / k * fade, k, amount, (cr * h + cs * (above / sheet_h)) / (cr + cs));
}

// Half the finest band of the lean model's erosion read a cast width `width` keeps, over the wisps' most
// stretch (a large number with the erosion off or every band gone): its spacing bound
// (volumetric_clouds.rs `erosion_bound`).
fn vc_m2_erosion_bound(width: f32, v: CloudVolume) -> f32 {
    var bound = 1.0e9;
    if v.nscale.w > 0.0 {
        var cells = vc_m2_cells(v);
        let stretch = 1.0 + VC_WISP_AMP * VC_M2_WISP_GRAD * max(v.dshape.z, 0.0);
        for (var k = 0u; k < 3u; k++) {
            let cell = cells[k];
            if width < 0.5 * cell {
                bound = min(bound, 0.5 * cell / stretch);
            }
        }
    }
    return bound;
}

// Half the lean model's narrowest surface transition at a cast width `width` (volumetric_clouds_model2.rs
// `edge_bound`): the narrowest type's VC_M2_EDGE or two cast widths, the wider, so two samples cross it.
fn vc_m2_edge_bound(width: f32) -> f32 {
    return 0.5 * max(min(VC_M2_EDGE.x, min(VC_M2_EDGE.y, VC_M2_EDGE.z)), 2.0 * max(width, 0.0));
}

// The shape at `rel` from the camera for a footprint `width`, as `vc_m2_shape` gives it, with the wisps'
// filter width the cast width. `eroded` false is the light's field: what the light volume's producer
// imports (this signature, its result and the bindings stay as they are).
fn vc_shape(rel: vec3<f32>, cam: vec3<f32>, width: f32, pwidth: f32, nb: texture_3d<f32>, ns: sampler, ne: texture_3d<f32>, eroded: bool, v: CloudVolume) -> vec4<f32> {
    let yb = vc_bent_y(rel, cam, v);
    if yb <= v.slab.x || yb >= vc_sampling_ceiling(v) {
        return vec4<f32>(0.0);
    }
    let pos = cam + rel;
    var column: VcPreparedColumn;
    return vc_shape_with_column(pos, yb, vc_m2_column(pos, pwidth, v), width, pwidth, nb, ns, ne, eroded, true, width, v, &column);
}

// These fields depend only on world XZ and the placement footprint.
struct VcPreparedColumn {
    col: VcColumn,
    // The geometry (`vc_m2_geometry`: the cover, the height, the shading's top share, the relief).
    m2: vec4<f32>,
    // Upper cover, median crown height in world units, and storm thickness variation.
    crown_cover: f32,
    crown_top: f32,
    deck_relief: vec2<f32>,
    fair_water_excess: f32,
    base_humidity: f32,
}

fn vc_prepare_column(pos: vec3<f32>, pwidth: f32, col: VcColumn, v: CloudVolume) -> VcPreparedColumn {
    // A fully resolved lobe sample gets its light height from its own roof.
    // Its material and the bounds diagnostics use only cover and extent.
    if vc_m2_lobe_weight(col.types, v) * col.res >= 1.0 {
        let cover = vc_m2_cover(col);
        let height = vc_m2_extent(col, v);
        return VcPreparedColumn(col, vec4<f32>(cover, height, 1.0, 0.0), cover, height, vec2<f32>(0.0), 0.0, 0.5);
    }
    let m2 = vc_m2_geometry(col, v);
    // Lobe headroom extends the sampling interval even at an open bank's edge.
    // Only the part anchored in the footprint can supply density to the crown.
    var crown_head = col.head;
    let attachment_width = 0.045 * vc_m2_mixed_sheet(v);
    if attachment_width > 0.0 {
        crown_head *= smoothstep(0.0, attachment_width, col.over);
    }
    var crown_cover = smoothstep(0.0, VC_M2_COVER, col.over + 0.18 * crown_head);
    // [hypothesis] Leave density saddles between upper heads in the middle
    // of a bank, even where the parent cover has reached one.
    crown_cover *= 1.0 - 0.44 * smoothstep(0.50, 1.0, m2.x) * smoothstep(0.0, 0.28, -col.head);
    var peak = m2.z;
    if crown_cover != m2.x {
        peak = vc_m2_top(crown_cover, 0.5 * m2.w, col.types, v);
    }
    let crown_top = vc_m2_stretch(col.local * peak, col.local, m2.y, VC_M2_FOOT * col.local);
    let deck = smoothstep(0.55, 0.72, v.scale.w) * smoothstep(0.0, 0.08, col.types.x);
    var deck_relief = vec2<f32>(0.0);
    if deck > 0.0 && col.over > 0.0 {
        deck_relief = deck * vc_m2_deck_relief(pos, pwidth, v);
    }
    return VcPreparedColumn(col, m2, crown_cover, crown_top, deck_relief, 0.0, 0.5);
}

// The shape from its column on: `col` the field at `pos`, a point inside the domain (the refined march
// evaluates the column at one place for its probes, samples and taps). One flat condensation base for
// every column: the domain's in the bent frame.
fn vc_shape_with_column(pos: vec3<f32>, yb: f32, col: VcColumn, width: f32, pwidth: f32, nb: texture_3d<f32>, ns: sampler, ne: texture_3d<f32>, eroded: bool, detail: bool, fw: f32, v: CloudVolume, column_out: ptr<function, VcPreparedColumn>) -> vec4<f32> {
    let above = yb - v.slab.x;
    if above <= 0.0 || (col.edge <= 0.0 && col.res >= 1.0) {
        return vec4<f32>(0.0);
    }
    var column = vc_prepare_column(pos, pwidth, col, v);
    if col.edge > 0.0 && v.scale.w < 0.75 {
        // [hypothesis] Water content varies within a fair bank at the scale of a
        // group of lobes. Integrate the same filtered column in the light and view
        // marches so dense pockets both block sunlight and absorb it along the ray.
        let uv = vc_base_coord(pos, v.slab.x, v) + vec3<f32>(0.31, 0.27, 0.63);
        let level = vc_level(pwidth, v.nlevel.x, v.nscale.x, v.nlevel.y);
        let moisture = vc_spread_base(textureSampleLevel(nb, ns, uv, level).r, v);
        let core = smoothstep(0.0, VC_M2_COVER, col.over);
        column.base_humidity = moisture;
        column.fair_water_excess = mix(-0.36, 0.62 + 0.95 * moisture, core)
            * clamp(v.nscale.z, 0.0, 1.0);
    }
    *column_out = column;
    return vc_m2_shape(pos, yb, above, column, width, pwidth, nb, ns, ne, eroded, detail, fw, v);
}

// 1 up to 0.75 of the horizon distance, 0 at it, by the distance where the ray enters the
// domain (volumetric_clouds.rs `horizon_fade`, HORIZON_FADE_FROM).
fn vc_horizon_fade(t: f32, horizon: f32) -> f32 {
    let r = max(horizon, 1.0);
    return 1.0 - smoothstep(0.75 * r, r, t);
}

// The real roots of a t^2 + b t + c = 0 (a >= 0), cancellation-resistant (volumetric_clouds.rs
// `roots`): x 1 when there are any, y the smaller, z the larger; a vanishing a is the linear
// equation's one root.
fn vc_roots(a: f32, b: f32, c: f32) -> vec3<f32> {
    if a <= 1e-12 {
        if abs(b) <= 1e-12 {
            return vec3<f32>(0.0);
        }
        let r = -c / b;
        return vec3<f32>(1.0, r, r);
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return vec3<f32>(0.0);
    }
    let q = -0.5 * (b + select(-1.0, 1.0, b >= 0.0) * sqrt(disc));
    let r1 = q / a;
    var r2 = r1;
    if abs(q) > 1e-30 {
        r2 = c / q;
    }
    return vec3<f32>(1.0, min(r1, r2), max(r1, r2));
}

// The ray's stretches inside the bent domain with its bent height ending at `top`, within the range and
// the ray's own stretch (volumetric_clouds.rs `spans_to`): (t0, t1) and (t2, t3); each a miss when its
// end is not past its start. The bent height along the ray is convex: the ray is in the domain from the top's
// smaller root (or its start) to the top's larger root, less where the bent height dips below
// the base (between the base's two roots): below the base only the second stretch is there, a
// ray from inside or above the domain that dips under the base has both
// (each is faded by its own entry, `vc_trace_refined`). A ray that clears the base is split at its
// lowest bent height, where the base's two roots meet as it grazes the base, so
// its far part takes its own fade on either side of that tangency. A vertical ray is the flat
// slab's one stretch.
fn vc_spans_to(ray: MediumRay, top: f32, v: CloudVolume) -> vec4<f32> {
    let base = v.slab.x;
    let y0 = ray.origin.y;
    let a = dot(ray.dir.xz, ray.dir.xz) / (2.0 * max(v.reach.y, 1.0));
    let b = ray.dir.y;
    var s = vec4<f32>(1.0, 0.0, 1.0, 0.0);
    if a <= 1e-12 {
        if abs(b) < 1e-6 {
            if y0 > base && y0 < top {
                s = vec4<f32>(0.0, v.slab.z, 1.0, 0.0);
            }
        } else {
            let ta = (base - y0) / b;
            let tb = (top - y0) / b;
            s = vec4<f32>(max(min(ta, tb), 0.0), max(ta, tb), 1.0, 0.0);
        }
    } else {
        let rt = vc_roots(a, b, y0 - top);
        if rt.x < 0.5 {
            return s;
        }
        let lo = max(rt.y, 0.0);
        let hi = rt.z;
        let rb = vc_roots(a, b, y0 - base);
        if rb.x > 0.5 && rb.z > lo && rb.y < hi {
            s = vec4<f32>(lo, min(hi, rb.y), max(rb.z, lo), hi);
        } else {
            let mid = min(max(-b / (2.0 * a), lo), hi);
            s = vec4<f32>(lo, mid, mid, hi);
        }
    }
    let far = min(ray.t_max, v.slab.z);
    return vec4<f32>(max(s.x, ray.t_min), min(s.y, far), max(s.z, ray.t_min), min(s.w, far));
}

// The near taps the light volume's reader marches whatever the configured count: one, a local tap toward
// the light on a cloud sample before the volume's coarse remainder (Bauer 2019, slide 33, a single
// transmittance lookup toward the sun on cloud samples beside the shadow map [documented:
// Bauer 2019, slide 33]). The first interval is counted once: the volume is read from its end.
const VC_NEAR_LEAST: u32 = 1u;

// The light volume's two levels (#3032 stage 2g, volumetric_clouds_lightvol.rs): each a 2D array
// of R16Float layers, a texel the optical depth from its node to the bent top along the light.
// Bound only in the trace's light-volume variant (shader def LIGHT_VOLUME; medium_trace.wesl's
// bindings 0-4 are its own, these two are declared here so the march's signatures stay as they
// are).
@if(LIGHT_VOLUME) @group(0) @binding(5) var vc_lv_near: texture_2d_array<f32>;
@if(LIGHT_VOLUME) @group(0) @binding(6) var vc_lv_far: texture_2d_array<f32>;

// One level of the light volume at a point already addressed (mirror: volumetric_clouds_lightvol.rs
// `LvSweep::coord`, `lv_guard`, `lv_trilinear` on a world-aligned level): `c` its position in
// nodes from the window's corner (field x, z), `bh` its bent height above the base (units), `up`
// the level's spacing up, `size` its nodes across (x) and layers (y). Returns x how far inside the
// window the point is (cells; -1 outside it or under the base), y the depth there (bilinear across
// in two layers, the height interpolated here; 0 outside). Outside the window nothing is sampled.
fn vc_lv_fetch(t: texture_2d_array<f32>, ns: sampler, c: vec2<f32>, bh: f32, up: f32, size: vec2<f32>) -> vec2<f32> {
    let nodes = max(size.x, 2.0);
    let layers = max(size.y, 2.0);
    let edge = min(min(c.x, nodes - 1.0 - c.x), min(c.y, nodes - 1.0 - c.y));
    if bh < 0.0 || edge < 0.0 {
        return vec2<f32>(-1.0, 0.0);
    }
    let y = min(bh / max(up, 1.0), layers - 1.0);
    let y0 = min(floor(y), layers - 2.0);
    let uv = (c + 0.5) / nodes;
    let d0 = textureSampleLevel(t, ns, uv, i32(y0), 0.0).r;
    let d1 = textureSampleLevel(t, ns, uv, i32(y0) + 1, 0.0).r;
    return vec2<f32>(edge, mix(d0, d1, y - y0));
}

// One level of the light volume at `q` from the camera at `cam`: `lvl` the camera's position in
// nodes from the level's corner (xy), its spacing across (z) and up (w), `size` as `vc_lv_fetch`.
// The producer's nesting reads the far level through it.
fn vc_lv_read(t: texture_2d_array<f32>, ns: sampler, q: vec3<f32>, cam: vec3<f32>, lvl: vec4<f32>, size: vec2<f32>, v: CloudVolume) -> vec2<f32> {
    return vc_lv_fetch(t, ns, lvl.xy + q.xz / max(lvl.z, 1.0), vc_bent_y(q, cam, v) - v.slab.x, lvl.w, size);
}

// The first of the `n` light intervals from `rel`: the reach to the bent top along the light, at most
// `reach.x` (volumetric_clouds.rs `light_reach`), split growing by VC_LIGHT_GROWTH from the sample
// outward (`light_intervals`). The march's taps give the optical depth toward the light (#3032 stage 2e:
// the depth, not its transmittance, so the shading can use it at several scales), each the uneroded shape
// at the interval's middle at the view sample's footprint, counted with the interval's own length.
fn vc_light_first(rel: vec3<f32>, cam: vec3<f32>, n: u32, v: CloudVolume) -> f32 {
    let ld = v.light_dir.xyz;
    let twice_r = 2.0 * max(v.reach.y, 1.0);
    let a = dot(ld.xz, ld.xz) / twice_r;
    let b = ld.y + 2.0 * dot(rel.xz, ld.xz) / twice_r;
    let c = vc_bent_y(rel, cam, v) - (v.slab.x + max(v.slab.y, 1.0));
    let out = vc_roots(a, b, c);
    var reach = v.reach.x;
    if out.x > 0.5 && out.z > 0.0 {
        reach = min(out.z, v.reach.x);
    }
    return reach * (VC_LIGHT_GROWTH - 1.0) / (pow(VC_LIGHT_GROWTH, f32(n)) - 1.0);
}

// How many of the `n` light intervals are marched before the volume is read (volumetric_clouds_lightvol.rs
// `lv_near_taps`); all of them with no volume bound.
@if(LIGHT_VOLUME) fn vc_near_count(n: u32, v: CloudVolume) -> u32 {
    let requested = u32(max(v.lv.x, 0.0) + 0.5);
    return min(max(requested, VC_NEAR_LEAST), n);
}

@if(!LIGHT_VOLUME) fn vc_near_count(n: u32, v: CloudVolume) -> u32 {
    return n;
}

// The light volume read at the handoff point `p` (#3032 stage 2g; mirror: volumetric_clouds_lightvol.rs
// `lv_nest`): the near level, mixed over the far level through the near level's guard band, mixed over
// the remaining taps through the far level's. x the depth `near` marched so far and what the levels add,
// y the weight left to the remaining taps, which are marched only where it is above 0 (outside both
// windows, or a handoff point under the base; a weight of 1 is exact). With no volume bound nothing is
// added and the whole weight is left. The levels hold the depth to the bent top with no cap
// (where they are read the 24,000-unit reach cut is gone). A level is read only while
// its front set is ready (`lv.y`, `lv.z`), at the camera's position in nodes from that set's own corner
// (`lv_near.xy`, `lv_far.xy`: B2's front windows). One bent height serves both levels.
@if(LIGHT_VOLUME) fn vc_light_handoff(p: vec3<f32>, cam: vec3<f32>, near: f32, ns: sampler, v: CloudVolume) -> vec2<f32> {
    let bh = vc_bent_y(p, cam, v) - v.slab.x;
    let guard = max(v.lv.w, 1e-3);
    var optical = near;
    var keep = 1.0;
    if v.lv.y > 0.5 {
        let r = vc_lv_fetch(vc_lv_near, ns, v.lv_near.xy + p.xz / max(v.lv_near.z, 1.0), bh, v.lv_near.w, v.lv_size.xz);
        let w = clamp(r.x / guard, 0.0, 1.0);
        optical += w * r.y;
        keep = 1.0 - w;
    }
    if v.lv.z > 0.5 && keep > 0.0 {
        let r = vc_lv_fetch(vc_lv_far, ns, v.lv_far.xy + p.xz / max(v.lv_far.z, 1.0), bh, v.lv_far.w, v.lv_size.yw);
        let w = clamp(r.x / guard, 0.0, 1.0);
        optical += keep * w * r.y;
        keep *= 1.0 - w;
    }
    return vec2<f32>(optical, keep);
}

@if(!LIGHT_VOLUME) fn vc_light_handoff(p: vec3<f32>, cam: vec3<f32>, near: f32, ns: sampler, v: CloudVolume) -> vec2<f32> {
    return vec2<f32>(near, 1.0);
}

// A per-pixel start offset 0..1 for the march's steps: interleaved gradient noise of the
// pixel (Jimenez, "Next Generation Post Processing in Call of Duty: Advanced Warfare",
// SIGGRAPH 2014), turned by the golden ratio each frame when a temporal mode resolves it.
fn vc_dither(frag: vec2<f32>, frame: u32, temporal: bool) -> f32 {
    var d = fract(52.9829189 * fract(dot(frag, vec2<f32>(0.06711056, 0.00583715))));
    if temporal {
        d = fract(d + 0.61803398875 * f32(frame % 64u));
    }
    return d;
}

// What the light's shading (volumetric_clouds_light.wesl `vc_lean_shade`) takes of the uniform, once per
// ray (#3032 stage 2e: the leaf takes plain values, never `CloudVolume`).
fn vc_shading(v: CloudVolume) -> VcShading {
    return VcShading(vc_view_code(v), v.lmode, v.lupper.rgb, v.llower.rgb, v.loct, v.lfloor, v.light_dir.y);
}

// The light's angular part for a ray along `dir` (volumetric_clouds_light.wesl `vc_ray_light`): the
// direct light times its strength and the two octaves' phases (#3032 stage 2e, L2), once per ray.
fn vc_ray_light_of(dir: vec3<f32>, v: CloudVolume) -> VcRayLight {
    return vc_ray_light(dot(dir, v.light_dir.xyz), v.phase, v.light.rgb, v.light_dir.w, v.loct, v.lpowder.x);
}

// What a ray that meets nothing gathers: no light, everything behind shows, no distance
// (medium.rs EMPTY_DISTANCE).
fn vc_empty() -> MediumSample {
    return MediumSample(vec3<f32>(0.0), 1.0, -1.0);
}

// The debug view in force (`flags.z`): 0 the clouds, 1 the placement field against its
// threshold, 2 the local height, 3 the evaluations per ray, 4 the generated noise on the bent
// base (#3032 stage 2b); 5 reserved for the cloud map's view (stage 2d), 6 the direct light
// alone, 7 the ambient alone, 8 the optical depth toward the light (stage 2e, the light's leaf
// `vc_lean_shade`), 10 the ray's opacity, 13 the visible height in the cloud's body; the others are not used.
// Only the trace's view variant (shader def CLOUD_VIEWS; volumetric_clouds.rs `view_variant`), drawn while
// a view is asked for, reads the code; without it the views' code is the compiler's to drop.
@if(!CLOUD_VIEWS) fn vc_view_code(v: CloudVolume) -> u32 {
    return 0u;
}

@if(CLOUD_VIEWS) fn vc_view_code(v: CloudVolume) -> u32 {
    return u32(clamp(v.flags.z, 0.0, 13.0) + 0.5);
}

// Integral of exp(-max(height,0)) in scale heights, with constant density below ground.
fn vc_air_column(height: f32) -> f32 {
    return min(height, 0.0) + (1.0 - exp(-max(height, 0.0)));
}

// Approximate air transport to the opacity-weighted cloud distance. Only the covered
// fraction picks up diffuse skylight. Empty sky, coverage and reprojection stay intact.
fn vc_air_colour(colour: vec3<f32>, trans: f32, dist: f32, ray: MediumRay, v: CloudVolume) -> vec3<f32> {
    if v.air_low.w <= 0.0 || dist <= 0.0 || vc_view_code(v) != 0u {
        return colour;
    }
    // Tuned world-unit scales: a shallow slant crosses more dense air than a steep ray.
    let height = (ray.origin.y - v.air_high.w) / 28000.0;
    let rise = dist * ray.dir.y / 28000.0;
    var density = exp(-max(height + 0.5 * rise, 0.0));
    if abs(rise) > 0.01 {
        density = (vc_air_column(height + rise) - vc_air_column(height)) / rise;
    }
    let share = 1.0 - exp(-v.air_low.w * (dist / 280000.0) * clamp(density, 0.0, 1.0));
    let sky = mix(v.air_low.rgb, v.air_high.rgb, clamp(ray.dir.y / 0.34202014, 0.0, 1.0));
    return mix(colour, sky * (1.0 - trans), share);
}

// [documented: Bauer 2019, attenuation by optical depth toward the sun.]
// [hypothesis] A light-cache cell spans several heads. Estimate the first short
// segment using the same medium caps as the volume. Only the resolved upper
// interior can shelter a sample; an isolated thin rim keeps its forward light.
fn vc_m2_crown_depth(column: VcPreparedColumn, above: f32, pos: vec3<f32>, width: f32, sigma: f32, v: CloudVolume) -> f32 {
    let parent = max(column.col.local, 1.0);
    let tier = 1.0 / (32.0 * v.nscale.x);
    let weight = smoothstep(0.52, 0.88, above / parent) * smoothstep(0.35, 0.90, column.m2.x)
        * (1.0 - smoothstep(0.55, 0.72, v.scale.w)) * column.col.res * clamp(v.nscale.z, 0.0, 1.0);
    if weight <= 0.0 || width >= 0.85 * tier {
        return 0.0;
    }
    let distance = min(0.48 * tier, 0.48 * parent);
    let toward = v.light_dir.xyz * distance;
    let uv = (pos.xz + toward.xz) * v.nscale.x + v.nphase.xy;
    let neighbour = vc_m2_caps(uv, 32u, 0.0, width, v);
    let plume = smoothstep(0.45, 1.45, column.m2.y / parent - 1.0);
    let height = column.crown_top + (neighbour - column.col.medium_head) * (1.0 + 2.8 * plume) * (1.0 - VC_M2_FOOT) * parent;
    let buried = smoothstep(0.0, distance, height - above - toward.y);
    let resolved = 1.0 - smoothstep(0.35 * tier, 0.85 * tier, width);
    return sigma * distance * weight * buried * resolved;
}

// The light a view sample `d` at `rel` gives toward the camera (linear), through the lean light
// (volumetric_clouds_light.wesl `vc_lean_shade`), with the march's estimates of its column.
fn vc_incident(d: vec4<f32>, depth: f32, sampled: bool, ray: VcRayLight, s: VcShading, rel: vec3<f32>, cam: vec3<f32>, width: f32, column: VcPreparedColumn, v: CloudVolume) -> vec3<f32> {
    if s.mode == 13u {
        // The height share the light is given (from the flat base to the column's estimated top).
        return vec3<f32>(0.9 * clamp(d.w, 0.0, 1.0));
    }
    // The opacity view only uses the accumulated camera transmittance.
    if s.mode == 10u {
        return vec3<f32>(0.0);
    }
    // The vertical estimate of the cloud above and below: the sample's own extinction per unit over
    // the column's estimated height, VC_M2_FILL of it holding cloud.
    var ls = s;
    let above = vc_bent_y(rel, cam, v) - v.slab.x;
    ls.column.y = d.y * VC_M2_FILL * max(vc_m2_light_height(column, above), 1.0);
    // Blended samples carry extinction-weighted relative height. Recover the
    // corresponding column length, including the pure unresolved sheet.
    if column.col.res < 1.0 && d.w > 0.0 {
        ls.column.y = d.y * VC_M2_FILL * max(above / d.w, 1.0);
    }
    let pw = vc_place_width(rel, normalize(rel), width, v);
    let lobes = vc_m2_lobe_weight(column.col.types, v) * column.col.res;
    var crown_depth = 0.0;
    if lobes < 1.0 {
        crown_depth = (1.0 - lobes) * vc_m2_crown_depth(column, above, cam + rel, pw, d.y, v);
    }
    if lobes > 0.0 && d.w > 0.0 {
        ls.column.y = mix(ls.column.y, d.y * VC_M2_FILL * above / max(d.w, 0.08), lobes);
    }
    // Humidity raises the lower surface and narrows the occupied column.
    // Use its mean onset for light, with the existing view and sunlight colours.
    let fair_path = lobes * vc_m2_open_bank_weight(v) * smoothstep(0.0, 0.02, column.m2.x);
    var open_flank = 0.0;
    if fair_path > 0.0 {
        let profile = vc_m2_fair_profile(column, v);
        open_flank = fair_path * (1.0 - profile.bank);
        let onset = vc_m2_base_onset(column.base_humidity, 0.0, v) * profile.height;
        let height = vc_m2_light_height(column, above / profile.depth_scale);
        let occupied = profile.depth_scale * (vc_m2_plume_height(height, column.col.local, profile.plume_scale) - onset);
        ls.column.y = mix(ls.column.y, d.y * VC_M2_FILL * max(occupied, 1.0), fair_path);
    }
    // [hypothesis] On the opaque face use the parent bank's optical path.
    // At the rim or above the shoulder, recover the directional cache and the head probe.
    let foot = column.col.res * (1.0 - smoothstep(0.55, 0.72, d.z)) * smoothstep(0.22, 0.78, column.m2.x)
        * (1.0 - smoothstep(0.52, 0.88, above / max(column.col.local, 1.0)));
    var sheet_depth = 0.0;
    if d.z > 0.55 {
        // Beyond the resolved placement, the layer has the weather's mean optical depth.
        // Inside it, the broad bed of the front supplies the variation between columns.
        let filled = mix(1.0, smoothstep(0.0, 0.20, column.col.over), column.col.res);
        sheet_depth = v.lmode.y * filled * exp2(2.0 * column.col.res * column.deck_relief.x);
    }
    return vc_lean_shade(d, above, depth, sampled, ray, ls, crown_depth, foot * (1.0 - 0.70 * lobes), sheet_depth, open_flank);
}

// The view steps a stretch (t0, t1) asks for at the spacing bound (volumetric_clouds.rs
// `stretch_steps`), a float count from 1 to ten million: the stretch over the step, `march.x`
// long, longer where the stretch starts far off (at most `flags.w` footprints there) but never
// longer than half the finest detail still there (#3032 stage 2b: one base texel
// at the level the entry's footprint reads, the noise's finest detail spanning two; half a sheet,
// the thinnest profile band; half the finest erosion band kept (`vc_m2_erosion_bound`); half the
// surface's narrowest transition (`vc_m2_edge_bound`)), also where that is shorter than `march.x`
// (the detail bound before the budget's scaling, so near the camera the
// transition's 120 units hold while budget is left).
fn vc_stretch_steps(t0: f32, t1: f32, spread: f32, v: CloudVolume) -> f32 {
    let lod = vc_level(spread * t0, v.nlevel.x, v.nscale.x, v.nlevel.y);
    let bound = min(min(min(exp2(lod) / max(v.nscale.x * v.nlevel.x, 1e-9), 0.5 * v.field.w), vc_m2_erosion_bound(spread * t0, v)), vc_m2_edge_bound(spread * t0));
    let step_len = max(min(max(v.march.x, v.flags.w * spread * t0), bound), 1.0);
    return clamp(ceil(max(t1 - t0, 0.0) / step_len), 1.0, 1.0e7);
}

// One previous full illumination estimate, valid only for the immediately adjacent cell.
struct VcIncidentCache {
    incident: vec3<f32>,
    t: f32,
    extinction: f32,
    height: f32,
    valid: bool,
}

fn vc_reuse_incident(cache: VcIncidentCache, d: vec4<f32>, t: f32, threshold: f32) -> bool {
    let distance = t - cache.t;
    let extinction = d.x * d.y;
    let larger = max(cache.extinction, extinction);
    return cache.valid && d.x > 2.0 * threshold && distance <= 2500.0
        && abs(d.w - cache.height) <= 0.10 && abs(extinction - cache.extinction) <= 0.30 * larger
        && distance * larger <= 0.45;
}

// --- The view march (#3032 stage 2c, checkpoint C1; the medium's trace entries `medium_trace_refined`
// and `medium_trace_tiles_refined` call it). Empty air is skipped by what is cheap
// to know: the field's support, probed at the ends of coarse blocks anchored to the stretch, an entry
// or an exit bracketed by halving down to one fine cell, the fine cells integrated with the
// energy-conserving step integral and the light's taps. Bauer 2019, slide 50 (a large base step, halved and
// moved back on meeting the base shape a fixed number of times, the small step kept inside cloud, a
// thin cloud between two base steps can be missed) and Schneider 2015 (the same idea, cheaper samples
// far off). Our own form: volumetric_clouds.rs `refined_march` is its CPU mirror. ---

// The most cells a block of the refined march holds (volumetric_clouds.rs REFINE_BLOCK_MOST): its
// bracketing takes at most four probes.
const VC_BLOCK_MOST: u32 = 16u;
// The refined count view's whites per channel (volumetric_clouds.rs COUNT_PROBES, COUNT_FINE,
// COUNT_TAPS): R the occupancy probes, G the fine density samples, B the light taps.
const VC_COUNT_PROBES: f32 = 512.0;
const VC_COUNT_FINE: f32 = 256.0;
const VC_COUNT_TAPS: f32 = 2048.0;

// Whether a point `above` units over the flat base, inside the bent domain, may hold cloud, from the
// placement and the column alone (no texture): the uncarved envelope's vertical interval (its density is
// zero at and above the column's height, `vc_m2_density`), union the unresolved sheet over the same base.
// The carve and the erosion only take, so a point outside it holds none; a point inside may still hold
// none (a hollow).
fn vc_m2_support(col: VcColumn, above: f32, v: CloudVolume) -> bool {
    let sheet2 = col.res < 1.0 && above < max(v.field.w, 1.0) && v.profile.y > 0.0;
    return (col.edge > 0.0 && above < vc_m2_extent(col, v)) || sheet2;
}

// The fine step at `t` (volumetric_clouds.rs `fine_step`): the spacing rule of `vc_stretch_steps`
// evaluated at `t` instead of the stretch's entry, the cast width `spread x t` for the 3D detail; it
// never falls as `t` grows, so it is never shorter than the entry's.
fn vc_fine_step(t: f32, spread: f32, v: CloudVolume) -> f32 {
    let w = spread * t;
    let lod = vc_level(w, v.nlevel.x, v.nscale.x, v.nlevel.y);
    let bound = min(min(min(exp2(lod) / max(v.nscale.x * v.nlevel.x, 1e-9), 0.5 * v.field.w), vc_m2_erosion_bound(w, v)), vc_m2_edge_bound(w));
    return max(min(max(v.march.x, v.flags.w * w), bound), 1.0);
}

// The coarse spacing at `t` (volumetric_clouds.rs `coarse_step`): the weather's (`profile.z`, 800
// units at amounts up to 0.1, 1,600 above, with the domain), widened by the placement's footprint
// on the shell over the finest placement octave's cell once that octave is gone (masses finer than
// the footprint are faded out of the field).
fn vc_coarse_step(ray: MediumRay, t: f32, v: CloudVolume) -> f32 {
    let rel = ray.dir * t;
    let pw = vc_place_width(rel, ray.dir, max(ray.spread, 0.0) * t, v);
    let finest = 1.0 / max(v.map.x * v.field.x * VC_PLACE_F2, 1e-9);
    return max(v.profile.z, 1.0) * max(pw / finest, 1.0);
}

// Whether the stretch (b, e) of the ray meets the band where every cloud has density
// (volumetric_clouds.rs `guard`): from the domain's base to the highest base (`height.w`) plus a sheet
// (`field.w`), by the ray's bent height over the stretch (convex: its least at an end or at its
// lowest point).
fn vc_guard(ray: MediumRay, b: f32, e: f32, v: CloudVolume) -> bool {
    let a = dot(ray.dir.xz, ray.dir.xz) / (2.0 * max(v.reach.y, 1.0));
    let yb = ray.origin.y + ray.dir.y * b + a * b * b;
    let ye = ray.origin.y + ray.dir.y * e + a * e * e;
    var lo = min(yb, ye);
    if a > 1e-12 {
        let ts = clamp(-ray.dir.y / (2.0 * a), b, e);
        lo = min(lo, ray.origin.y + ray.dir.y * ts + a * ts * ts);
    }
    return lo < v.slab.x + v.height.w + v.field.w && max(yb, ye) > v.slab.x;
}

// Node `i` of a block from `b` of `n` cells `f` long that ends at `e` (the last cell may be shorter).
// Keep a native exit cell, cap the integrated optical depth, and increase longitudinal
// spacing gradually with distance. The support probes and their bisection keep the fine grid.
fn vc_interior_stride(limit: u32, remaining: u32, fine: f32, t: f32, sigma: f32) -> u32 {
    let max_length = 420.0 + 780.0 * smoothstep(15000.0, 85000.0, t);
    let length_limit = min(max_length, 0.65 / max(sigma, 1e-12));
    return max(1u, min(min(limit, remaining - 1u), u32(floor(length_limit / fine))));
}

fn vc_node(b: f32, f: f32, i: u32, n: u32, e: f32) -> f32 {
    if i >= n {
        return e;
    }
    return b + f32(i) * f;
}

// What one stretch of the refined march gathers, unfaded, from its own start.
struct VcRefined {
    colour: vec3<f32>,
    trans: f32,
    hidden: f32,
    hidden_t: f32,
    // The work, counted in the view variant alone: x the occupancy probes (the start, each block's end,
    // the halvings), y the fine density samples, z the light taps marched.
    work: vec3<f32>,
}

// What a turn of the refined march evaluates: a probe (the stretch's start, a block's end, a
// halving's middle node), a cell's view sample, or a tap toward the light.
const VC_TURN_START: u32 = 0u;
const VC_TURN_END: u32 = 1u;
const VC_TURN_HALVE: u32 = 2u;
const VC_TURN_SAMPLE: u32 = 3u;
const VC_TURN_TAP: u32 = 4u;

// The count view's work (`flags.z` 3): the turn's evaluation, by kind. Nothing is counted without a view.
@if(CLOUD_VIEWS) fn vc_count_turn(work: ptr<function, vec3<f32>>, turn: u32) {
    if turn <= VC_TURN_HALVE {
        (*work).x += 1.0;
    } else if turn == VC_TURN_SAMPLE {
        (*work).y += 1.0;
    } else {
        (*work).z += 1.0;
    }
}

@if(!CLOUD_VIEWS) fn vc_count_turn(work: ptr<function, vec3<f32>>, turn: u32) {
}

// One stretch (t0, t1) of the refined march (volumetric_clouds.rs `refined_march`). Blocks from t0,
// each of `n` cells `f = vc_fine_step(b) x grow` long (`grow` the ray's budget, at least 1), `n` the
// coarse spacing over `f`, 1 to VC_BLOCK_MOST, the last block clipped at t1: the fine grid is fixed by
// the ray alone, never by the start offset, and runs on across blocks. The support is probed at t0 and
// at each block's end; a block is integrated whole where it meets the base band (`vc_guard`) or both
// its ends are inside, from its entry cell where only its end is (the entry bracketed by halving: the
// start known empty, the end inside), to and with its exit cell where only its start is, and skipped
// where neither is. Each integrated cell (lo, hi) is sampled at `lo + offset x (hi - lo)` and counted
// with its real length; stopped once the transmittance (with what lies in front, `ahead`) is under
// `reach.z`. `blocks` bounds the blocks.
// The march is flat: one loop whose turn evaluates one point, whatever it is for, in the order the
// nested loops of the mirror take (volumetric_clouds.rs tests
// `the_flat_march_takes_the_refined_marchs_turns`). So the column and the density have one call site
// each instead of one for every kind of point: the trace fell to a half to a third where it cost most
// [observed, 2026-11-23: the cost table of graphics-enhanced.md §3.23, its set-up there], their compiled size and the registers live
// across them, not how often they run, taken to be why [hypothesis].
fn vc_march_refined(ray: MediumRay, t0: f32, t1: f32, grow: f32, blocks: u32, offset: f32, ahead: f32, nb: texture_3d<f32>, ns: sampler, ne: texture_3d<f32>, v: CloudVolume) -> VcRefined {
    var m: VcRefined;
    m.colour = vec3<f32>(0.0);
    m.trans = 1.0;
    m.hidden = 0.0;
    m.hidden_t = 0.0;
    m.work = vec3<f32>(0.0);
    let spread = max(ray.spread, 0.0);
    let ceiling = vc_sampling_ceiling(v);
    let ray_light = vc_ray_light_of(ray.dir, v);
    let shading = vc_shading(v);
    let allow_reuse = v.families.w > 0.5 && v.light_dir.w > 0.0;
    var cache: VcIncidentCache;
    let stride_limit = u32(v.families.x);
    let ld = v.light_dir.xyz;
    let taps = clamp(u32(v.march.y), 1u, 16u);
    // 1 to `taps` (VC_NEAR_LEAST), so a tap's turn meets it.
    let near = vc_near_count(taps, v);
    // A turn is one evaluation: the start's probe, at most five probes a block (its end, four halvings),
    // and a sample with its taps for each cell. The loop ends itself (t1, `blocks`, the stop), never here.
    let turns = 1u + blocks * (5u + VC_BLOCK_MOST * (1u + taps));
    var turn = VC_TURN_START;
    // The block: its start and end, fine step, cells, number, whether its ends are inside, and the
    // halvings' nodes (lo as the start is, hi as the end is).
    var b = t0;
    var e = t0;
    var f = 1.0;
    var n = 1u;
    var k = 0u;
    var occ_s = false;
    var occ_e = false;
    var lo = 0u;
    var hi = 1u;
    var halvings = 0u;
    // Its cells: the next one, the first one not integrated, the strides' state.
    var i = 0u;
    var c_to = 0u;
    var interior_block = false;
    var stable = false;
    var previous_sigma = 0.0;
    // The view sample.
    var t = 0.0;
    var dt = 0.0;
    var rel = vec3<f32>(0.0);
    var width = 0.0;
    var pwidth = 0.0;
    var d = vec4<f32>(0.0);
    var column: VcPreparedColumn;
    // Its taps: the next one, its start and length; the depth up to the handoff (the volume's with it),
    // the depth marched since the start or the handoff, and the weight that has.
    var j = 0u;
    var s = 0.0;
    var ds = 0.0;
    var handed = 0.0;
    var optical = 0.0;
    var keep = 1.0;
    var work = vec3<f32>(0.0);
    for (var spent = 0u; spent < turns; spent++) {
        vc_count_turn(&work, turn);
        // The turn's point.
        var q = rel;
        var qwidth = width;
        var qpwidth = pwidth;
        if turn <= VC_TURN_HALVE {
            var at = e;
            if turn == VC_TURN_HALVE {
                at = vc_node(b, f, (lo + hi) / 2u, n, e);
            }
            q = ray.dir * at;
            qwidth = spread * at;
            qpwidth = vc_place_width(q, ray.dir, qwidth, v);
        } else if turn == VC_TURN_TAP {
            q = rel + ld * (s + 0.5 * ds);
        }
        let yb = vc_bent_y(q, ray.origin, v);
        let pos = ray.origin + q;
        let in_domain = !(yb <= v.slab.x || yb >= ceiling);
        var col: VcColumn;
        if in_domain {
            col = vc_m2_column(pos, qpwidth, v);
        }
        var found = vec4<f32>(0.0);
        if turn >= VC_TURN_SAMPLE && in_domain {
            // The view's sample is eroded and filtered by its step; a tap is neither.
            let viewed = turn == VC_TURN_SAMPLE;
            let fw = select(qwidth, vc_detail_filter(qwidth, dt), viewed);
            var prepared: VcPreparedColumn;
            found = vc_shape_with_column(pos, yb, col, qwidth, qpwidth, nb, ns, ne, viewed, true, fw, v, &prepared);
            if viewed {
                column = prepared;
            }
        }
        // What follows from it.
        var reuse = false;
        var depth = 0.0;
        var sampled = false;
        var shade = false;
        var next_cell = false;
        var next_block = false;
        if turn <= VC_TURN_HALVE {
            let inside = in_domain && vc_m2_support(col, yb - v.slab.x, v);
            var whole = false;
            var settled = false;
            if turn == VC_TURN_START {
                occ_s = inside;
                next_block = true;
            } else if turn == VC_TURN_END {
                occ_e = inside;
                whole = (occ_s && occ_e) || vc_guard(ray, b, e, v);
                settled = whole || occ_s == occ_e || n <= 1u;
                turn = VC_TURN_HALVE;
            } else {
                let mid = (lo + hi) / 2u;
                if inside == occ_e {
                    hi = mid;
                } else {
                    lo = mid;
                }
                halvings += 1u;
                settled = hi - lo <= 1u || halvings >= 4u;
            }
            if settled {
                // The cells integrated: i .. c_to - 1 (none when c_to <= i).
                i = 0u;
                c_to = 0u;
                if whole {
                    c_to = n;
                } else if occ_e {
                    i = lo;
                    c_to = n;
                } else if occ_s {
                    c_to = lo + 1u;
                }
                if i > 0u { cache.valid = false; }
                interior_block = stride_limit >= 2u && occ_s && occ_e;
                if !interior_block || stride_limit < 4u {
                    stable = false;
                    previous_sigma = 0.0;
                }
                next_cell = true;
            }
        } else if turn == VC_TURN_SAMPLE {
            d = found;
            if interior_block {
                let sigma = d.x * d.y;
                let interior = d.x > max(2.0 * v.march.w, 0.12) && sigma > 0.0;
                stable = interior && previous_sigma > 0.0 && abs(sigma - previous_sigma) <= 0.22 * max(sigma, previous_sigma);
                previous_sigma = select(0.0, sigma, interior);
            }
            if d.x <= 0.0 {
                cache.valid = false;
                next_cell = true;
            } else {
                reuse = allow_reuse && vc_reuse_incident(cache, d, t, v.march.w);
                if !reuse && d.x > v.march.w && v.light_dir.w > 0.0 {
                    j = 0u;
                    s = 0.0;
                    ds = vc_light_first(rel, ray.origin, taps, v);
                    optical = 0.0;
                    keep = 1.0;
                    turn = VC_TURN_TAP;
                } else {
                    shade = true;
                }
            }
        } else {
            optical += found.x * found.y * ds;
            s += ds;
            ds *= VC_LIGHT_GROWTH;
            j += 1u;
            if j == near {
                let given = vc_light_handoff(rel + ld * s, ray.origin, optical, ns, v);
                handed = given.x;
                keep = given.y;
                optical = 0.0;
            }
            if j >= taps || keep <= 0.0 {
                depth = handed + keep * optical;
                sampled = true;
                shade = true;
            }
        }
        if shade {
            var emitted = cache.incident;
            if reuse {
                cache.valid = false;
            } else {
                emitted = vc_incident(d, depth, sampled, ray_light, shading, rel, ray.origin, width, column, v);
                if allow_reuse {
                    cache = VcIncidentCache(emitted, t, d.x * d.y, d.w, sampled && d.x > 2.0 * v.march.w);
                }
            }
            let kept = exp(-d.x * d.y * dt);
            let taken = m.trans * (1.0 - kept);
            m.colour += taken * emitted;
            m.hidden += taken;
            m.hidden_t += taken * t;
            m.trans *= kept;
            if m.trans * ahead < v.reach.z {
                break;
            }
            next_cell = true;
        }
        if next_cell {
            if i < c_to {
                let c0 = vc_node(b, f, i, n, e);
                var stride = 1u;
                let deep = interior_block && stride_limit >= 4u && previous_sigma > 0.0
                    && (c0 >= 55000.0 || m.trans * ahead < 0.40);
                if deep {
                    let limit = select(2u, 4u, stable);
                    let length_limit = min(1000.0 + 1000.0 * smoothstep(15000.0, 85000.0, c0), 0.85 / previous_sigma);
                    stride = max(1u, min(min(limit, c_to - i), u32(floor(length_limit / f))));
                } else if interior_block && stable {
                    stride = vc_interior_stride(stride_limit, c_to - i, f, c0, previous_sigma);
                }
                i += stride;
                dt = vc_node(b, f, i, n, e) - c0;
                t = c0 + offset * dt;
                rel = ray.dir * t;
                width = spread * t;
                pwidth = vc_place_width(rel, ray.dir, width, v);
                turn = VC_TURN_SAMPLE;
            } else {
                if c_to < n { cache.valid = false; }
                occ_s = occ_e;
                b = e;
                next_block = true;
            }
        }
        if next_block {
            if k >= blocks || b >= t1 {
                break;
            }
            k += 1u;
            f = vc_fine_step(b, spread, v) * grow;
            let cells = clamp(u32(clamp(floor(vc_coarse_step(ray, b, v) / f), 1.0, f32(VC_BLOCK_MOST))), 1u, VC_BLOCK_MOST);
            e = b + f32(cells) * f;
            n = cells;
            if e >= t1 {
                e = t1;
                n = clamp(u32(clamp(ceil((t1 - b) / f), 1.0, f32(VC_BLOCK_MOST))), 1u, cells);
            }
            lo = 0u;
            hi = n;
            halvings = 0u;
            turn = VC_TURN_END;
        }
    }
    m.work = work;
    return m;
}

// The count view's sample (`flags.z` 3): the work as RGB, R the
// probes, G the fine samples, B the light taps, each white at its VC_COUNT_* (saturating), opaque.
fn vc_count_rgb(work: vec3<f32>, dist: f32) -> MediumSample {
    return MediumSample(clamp(work / vec3<f32>(VC_COUNT_PROBES, VC_COUNT_FINE, VC_COUNT_TAPS), vec3<f32>(0.0), vec3<f32>(1.0)), 0.0, dist);
}

// The refined march's stretches (volumetric_clouds.rs `lean_spans`), ended at the sampling ceiling for a
// camera under it: past the ceiling there is no support (`vc_sampling_ceiling`); the block the ceiling
// ends is clipped there, so its probes, samples and lengths can differ slightly from the whole domain's,
// and a capped ray may get finer cells. From above the ceiling the stretches are the domain's.
fn vc_refined_spans(ray: MediumRay, v: CloudVolume) -> vec4<f32> {
    let ceiling = vc_sampling_ceiling(v);
    return vc_spans_to(ray, select(v.slab.x + max(v.slab.y, 1.0), ceiling, ray.origin.y < ceiling), v);
}

// The seam takes no texture: since #3032 A3 the clouds' placement is the field's and the
// weather's amount reaches the march through the uniform.
// The domain along the ray: its stretches front to back (`vc_refined_spans`), each marched
// (`vc_march_refined`) and faded toward the horizon by its own entry distance (a far
// stretch behind a near one never takes the near one's fade), the second only while the first left
// enough of the background: the colour, premultiplied, the transmittance, and the distance weighted by
// what each sample hid, along the straight world ray, each stretch's weight its transmittance in front
// times its fade as for the colour (a nearly faded far stretch does not pull the
// reprojection distance). The ray's work is bounded whatever the settings (volumetric_clouds.rs
// `refined_plan`): the cells of both stretches together at most `(reach.w - 2) / (2 + taps) - 4`, each
// at most one probe, one sample and its taps, so a ray spends at most `reach.w` evaluations; where the
// count at the entries' steps (`vc_stretch_steps`, an upper bound of the cells) passes that, every fine
// cell grows by the same factor.
fn vc_trace_refined(ray: MediumRay, offset: f32, nb: texture_3d<f32>, ns: sampler, ne: texture_3d<f32>, v: CloudVolume) -> MediumSample {
    let counting = vc_view_code(v) == 3u;
    let spans = vc_refined_spans(ray, v);
    let spread = max(ray.spread, 0.0);
    let taps = f32(clamp(u32(v.march.y), 1u, 16u));
    let most = max(floor((v.reach.w - 2.0) / (2.0 + taps)) - 4.0, 1.0);
    var want = vec2<f32>(0.0);
    if spans.y > spans.x && vc_horizon_fade(spans.x, v.slab.w) > 0.0 {
        want.x = vc_stretch_steps(spans.x, spans.y, spread, v);
    }
    if spans.w > spans.z && vc_horizon_fade(spans.z, v.slab.w) > 0.0 {
        want.y = vc_stretch_steps(spans.z, spans.w, spread, v);
    }
    let grow = max((want.x + want.y) / most, 1.0);
    let blocks = u32(clamp(most, 1.0, 1.0e6)) + 4u;
    var colour = vec3<f32>(0.0);
    var trans = 1.0;
    var hidden = 0.0;
    var hidden_t = 0.0;
    var work = vec3<f32>(0.0);
    for (var k = 0u; k < 2u; k++) {
        var t0 = spans.x;
        var t1 = spans.y;
        if k == 1u {
            t0 = spans.z;
            t1 = spans.w;
        }
        if t1 <= t0 || trans < v.reach.z {
            continue;
        }
        let fade = vc_horizon_fade(t0, v.slab.w);
        if fade <= 0.0 {
            continue;
        }
        let m = vc_march_refined(ray, t0, t1, grow, blocks, offset, trans, nb, ns, ne, v);
        colour += trans * fade * m.colour;
        hidden += trans * fade * m.hidden;
        hidden_t += trans * fade * m.hidden_t;
        work += m.work;
        trans *= 1.0 - (1.0 - m.trans) * fade;
    }
    var dist = -1.0;
    if hidden > 1e-6 {
        dist = hidden_t / hidden;
    }
    if counting {
        return vc_count_rgb(work, dist);
    }
    if vc_view_code(v) == 10u {
        return MediumSample(vec3<f32>(0.9 * clamp(1.0 - trans, 0.0, 1.0)), 0.0, dist);
    }
    // The depth view (#3032 stage 2e) and the body height: the opacity-weighted mean of the samples'
    // codes, opaque.
    if (vc_view_code(v) == 8u || vc_view_code(v) == 13u) && vc_depth_shown(hidden) {
        return MediumSample(colour / hidden, 0.0, dist);
    }
    colour = vc_air_colour(colour, trans, dist, ray, v);
    return MediumSample(colour, trans, dist);
}

// The placement and height debug views (`view` 1 and 2): grey, opaque, where the ray meets the
// bent base within the range (its distance there): the placement field against its threshold
// (dark below it, the field's value scaled to a half; from a half to white above it, by how far
// inside its mass), or the local occupied height's share of the domain.
fn vc_view_sample(ray: MediumRay, view: u32, nb: texture_3d<f32>, ns: sampler, ne: texture_3d<f32>, v: CloudVolume) -> MediumSample {
    let a = dot(ray.dir.xz, ray.dir.xz) / (2.0 * max(v.reach.y, 1.0));
    let r = vc_roots(a, ray.dir.y, ray.origin.y - v.slab.x);
    if r.x < 0.5 {
        return vc_empty();
    }
    var t = r.z;
    if ray.origin.y > v.slab.x && r.y > 0.0 {
        t = r.y;
    }
    if t <= 0.0 || t > v.slab.z {
        return vc_empty();
    }
    let pos = ray.origin + ray.dir * t;
    let width = max(ray.spread, 0.0) * t;
    let pw = vc_place_width(ray.dir * t, ray.dir, width, v);
    // The noise view (#3032 stage 2b): a slice of the generated textures on the bent base, at the
    // level the shell's footprint picks; bands of one base repeat across world x alternate: grey
    // the base scalar, colour the erosion's three bands (R 4,000, G 2,000, B 1,000 units).
    if view == 4u {
        let yb = v.slab.x + 0.5 * max(v.slab.y, 1.0);
        if fract(pos.x * v.nscale.x * 0.5) < 0.5 {
            let b = textureSampleLevel(nb, ns, vc_base_coord(pos, yb, v), vc_level(pw, v.nlevel.x, v.nscale.x, v.nlevel.y)).r;
            return MediumSample(vec3<f32>(b), 0.0, t);
        }
        let e = textureSampleLevel(ne, ns, vc_erosion_coord(pos, yb, v), vc_level(pw, v.nlevel.z, v.nscale.y, v.nlevel.w)).rgb;
        return MediumSample(e, 0.0, t);
    }
    let col = vc_m2_column(pos, pw, v);
    var g = 0.0;
    if view == 2u {
        g = select(0.0, vc_m2_extent(col, v) / max(v.slab.y, 1.0), col.edge > 0.0);
    } else if col.edge > 0.0 {
        g = 0.5 + 0.5 * vc_m2_cover(col);
    } else {
        g = 0.45 * clamp((col.over + v.field.y) / max(v.field.y, 1e-3), 0.0, 1.0);
    }
    return MediumSample(vec3<f32>(g), 0.0, t);
}
