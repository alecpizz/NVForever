// The light volume's tests on the CPU mirror (#3032 stage 2g): the sweep, the windows, the schedule, the
// variants and the shaders' text. Pulled into volumetric_clouds.rs `mod tests` by one `include!` (it reads
// that module's private helpers), so it is test code only and adds no module of its own.

use crate::volumetric_clouds_lightvol as lv;

const LV_PLACES: [Vec2; 5] = [Vec2::new(123_456.0, -654_321.0), Vec2::new(-250_000.0, 80_000.0), Vec2::new(40_000.0, 300_000.0), Vec2::new(-77_000.0, -410_000.0), Vec2::new(510_000.0, 9_000.0)];

/// A live edit of a uniform's tune input: the key's name and the edit.
type LvEdit = (&'static str, fn(&mut CloudVolumeUniform));

/// The longest light path [`le_full`] follows (units): the bent top along a 15-degree sun from the base is
/// about 155,000 units away.
const LE_FULL_MOST: f32 = 400_000.0;
/// [`le_full`]'s step and its least count.
const LE_FULL_STEP: f32 = 100.0;
const LE_FULL_LEAST: u32 = 1_000;

/// The eight-tap light estimate from `rel` along the unit `ld`, without a volume.
fn le_taps(sigma: &dyn Fn(Vec3) -> f32, rel: Vec3, ld: Vec3, u: &CloudVolumeUniform) -> f32 {
    let reach = light_reach(rel, 0.0, ld, u);
    let (mut s, mut tau) = (0.0f32, 0.0f32);
    for ds in light_intervals(reach, u.march.y as u32) {
        tau += sigma(rel + ld * (s + 0.5 * ds)) * ds;
        s += ds;
    }
    tau
}

/// The depth from `rel` along the unit `ld` to the bent domain's top, by a fine midpoint integral.
fn le_full(sigma: &dyn Fn(Vec3) -> f32, rel: Vec3, ld: Vec3, u: &CloudVolumeUniform) -> f32 {
    let mut whole = *u;
    whole.reach.x = LE_FULL_MOST;
    let length = light_reach(rel, 0.0, ld, &whole);
    let n = ((length / LE_FULL_STEP).ceil() as u32).max(LE_FULL_LEAST);
    let ds = length / n as f32;
    (0..n).map(|i| sigma(rel + ld * ((i as f32 + 0.5) * ds)) * ds).sum()
}

/// A filled sweep lattice.
struct LvSwept {
    sw: lv::LvSweep,
    data: Vec<f32>,
}

impl LvSwept {
    /// The depth to the top at `q` from the camera; `None` outside the lattice or under the base.
    fn read(&self, q: Vec3, u: &CloudVolumeUniform) -> Option<f32> {
        let c = self.sw.coord(q, u);
        self.sw.covers(c).then(|| lv::lv_trilinear(c, self.sw.layers, &|n| self.data[self.sw.index(n)]))
    }
}

/// The top-down sweep over the lattice `sw`: the top layer 0, each
/// layer below from the one above ([`lv::lv_sweep_node`]: the step to the next layer's bent
/// height integrated, the layer above read bilinearly where the ray crosses it, zero beyond the
/// lattice), every texel half-rounded as the R16Float target holds it; rows of a layer on up to
/// eight threads (each node independent within a layer: the result does not depend on them).
fn lv_sweep_fill(sw: lv::LvSweep, sigma: &(dyn Fn(Vec3) -> f32 + Sync), ld: Vec3, u: &CloudVolumeUniform) -> LvSwept {
    let (nx, nz) = (sw.nx as usize, sw.nz as usize);
    let mut data = vec![0.0f32; sw.texels() as usize];
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 8);
    for j in (0..sw.layers - 1).rev() {
        let upper: Vec<f32> = (0..nz).flat_map(|k| (0..nx).map(move |i| (i, k))).map(|(i, k)| data[sw.index(IVec3::new(i as i32, j as i32 + 1, k as i32))]).collect();
        let read = |p: Vec3| -> Option<f32> {
            let xz = (Vec2::new(p.x, p.z) - sw.layer_origin((j + 1) as f32, u)) / sw.across;
            if xz.x < 0.0 || xz.y < 0.0 || xz.x > (nx - 1) as f32 || xz.y > (nz - 1) as f32 {
                return None;
            }
            let (i0, k0) = ((xz.x.floor() as usize).min(nx - 2), (xz.y.floor() as usize).min(nz - 2));
            let (fx, fz) = (xz.x - i0 as f32, xz.y - k0 as f32);
            let at = |i: usize, k: usize| upper[k * nx + i];
            Some((at(i0, k0) * (1.0 - fx) + at(i0 + 1, k0) * fx) * (1.0 - fz) + (at(i0, k0 + 1) * (1.0 - fx) + at(i0 + 1, k0 + 1) * fx) * fz)
        };
        let chunk = nz.div_ceil(threads);
        let parts: Vec<Vec<(f32, u32)>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..nz)
                .step_by(chunk)
                .map(|k0| {
                    let read = &read;
                    scope.spawn(move || {
                        (k0..(k0 + chunk).min(nz))
                            .flat_map(|k| (0..nx).map(move |i| (i, k)))
                            .map(|(i, k)| lv::lv_sweep_node(sigma, sw.node_rel(IVec3::new(i as i32, j as i32, k as i32), u), ld, sw.up, read, u))
                            .collect::<Vec<(f32, u32)>>()
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().expect("a sweep row")).collect()
        });
        for (idx, (v, _)) in parts.into_iter().flatten().enumerate() {
            let (i, k) = (idx % nx, idx / nx);
            data[sw.index(IVec3::new(i as i32, j as i32, k as i32))] = v;
        }
    }
    LvSwept { sw, data }
}

/// The light-aligned lattice's margin (cells) about its receivers: a receiver's ray away from the
/// camera's column bends a little apart from the lattice's columns (about 1.4 cells at 15 degrees
/// for one 5,000 units out).
const LV_MARGIN: f32 = 8.0;

/// A light-aligned sweep lattice for `probes` (from the camera): the probes' positions in its columns,
/// bounded, and [`LV_MARGIN`] cells about them.
fn lv_sweep_lattice(across: f32, probes: &[Vec3], ld: Vec3, u: &CloudVolumeUniform) -> lv::LvSweep {
    let g = lv::LvGrid::new(across, lv::LV_UP, lv::LV_NODES, u.slab.y);
    let mut sw = lv::LvSweep { across, up: g.up, nx: 2, nz: 2, layers: g.layers, origin: Vec2::ZERO, lean: ld, cam_y: 0.0 };
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for &p in probes {
        let c = sw.coord(p, u);
        let xz = Vec2::new(c.x, c.z) * across;
        lo = lo.min(xz);
        hi = hi.max(xz);
    }
    sw.origin = lo - Vec2::splat(LV_MARGIN * across);
    let n = ((hi - lo) / across).ceil() + 2.0 * LV_MARGIN + 1.0;
    sw.nx = n.x as u32;
    sw.nz = n.y as u32;
    sw
}

/// The lattice and its window: the layers fit to the domain, its bytes; a window at negative field positions
/// (floor-snapped, the camera between the middle nodes) and sliding by a cell; the trilinear read exact for a
/// linear field, the height clamped; the half-float texel; the sweep's depth to the top on a uniform medium.
#[test]
fn the_light_volume_addresses_its_window() {
    use bevy::math::DVec2;
    let u = ground_uniform(REFERENCE_HEIGHT, HORIZON);
    let g = lv::LvGrid::new(500.0, 500.0, 128, u.slab.y);
    assert_eq!((g.layers, g.up), (81, 500.0));
    assert_eq!((g.texels(), g.bytes()), (1_327_104, 2_654_208), "the volume's size");
    // A window at negative field positions.
    let cf = DVec2::new(-1_234.5, -0.25);
    let w = lv::lv_window(cf, &g);
    assert_eq!(w.lo, [-66, -64]);
    assert!((w.cam - Vec2::new(63.531, 63.9995)).abs().max_element() < 1e-3, "{:?}", w.cam);
    assert_eq!(lv::lv_field(DVec2::new(10.0, -20.0), [0.5, -0.25], 60_000.0), DVec2::new(30_010.0, -15_020.0));
    // Sliding by a cell each way.
    assert_eq!(lv::lv_window(cf + DVec2::new(500.0, -500.0), &g).lo, [-65, -65]);
    // Trilinear: exact for a linear field, the height clamped.
    let lin = |n: IVec3| n.x as f32 + 10.0 * n.y as f32 + 100.0 * n.z as f32;
    for (c, want) in [(Vec3::new(3.25, 7.5, 9.75), 3.25 + 75.0 + 975.0), (Vec3::new(3.0, -2.0, 1.0), 103.0), (Vec3::new(3.0, 99.0, 1.0), 903.0)] {
        assert!((lv::lv_trilinear(c, 81, &lin) - want).abs() < 1e-3, "{c}");
    }
    // The texel.
    assert_eq!((lv::lv_store(1.0), lv::lv_store(1e6), lv::lv_store(f32::NAN), lv::lv_store(-1.0)), (1.0, lv::LV_HALF_MOST, 0.0, 0.0));
    assert!((lv::lv_store(0.1) - 0.1).abs() < 1e-4);
    // A uniform medium (1e-4 a unit inside the bent slab): the sweep's light-aligned depth to the top.
    let r = u.reach.y;
    let k = 1.0e-4f32;
    let slab = move |q: Vec3| {
        let h = bent_y(q, 0.0, r) - u.slab.x;
        if h > 0.0 && h < u.slab.y { k } else { 0.0 }
    };
    let p = Vec3::new(0.0, u.slab.x + 100.0, 0.0);
    let mut whole = u;
    whole.reach.x = 1.0e6;
    for sun in [45.0f32, 15.0] {
        let ld = c1_dir(sun, 0.0);
        let truth = k * light_reach(p, 0.0, ld, &whole);
        let light = lv_sweep_fill(lv_sweep_lattice(500.0, &[p], ld, &u), &slab, ld, &u);
        let got = light.read(p, &u).expect("inside the light-aligned lattice");
        assert!((got - truth).abs() < 0.01 * truth, "sun {sun}: {got} against {truth}");
    }
}

/// The density moves rigidly with the drift
/// (checked here against the uniform the march reads): the vigour's two octaves' offsets (whole cells
/// plus the fraction, over the lattice's cells a repeat) are the one displacement `repeat x drift`, the
/// placement's three and the alternate placement's three are the same transform of it as the march's of
/// a position (the shear; the cross basis), and the base and erosion noises' phases are it modulo their
/// repeats; the vertical noise coordinate is the bent height above the base, with no drift. So a node
/// addressed in field coordinates (`lv_field`) holds the same cloud however far the wind has carried it.
#[test]
fn the_field_moves_rigidly_with_the_drift() {
    for d in [0.0f64, 0.37, -0.25, -3.7, 123.456, -987.654] {
        let drift = [d, -0.6 * d];
        let u = uniform(&VolumeInputs { plane_height: 20_000.0, above_ground: 20_000.0, domain: REFERENCE_HEIGHT, horizon: HORIZON, steps: VIEW_STEPS, density: 1.0, sky_scale: 1.0, amount: 0.5, drift, ..Default::default() });
        let repeat = 1.0 / f64::from(u.map.x);
        let want = [drift[0] * repeat, drift[1] * repeat];
        let shear = f64::from(crate::volumetric_clouds_model2::SHEAR);
        let sheared = [want[0] + shear * want[1], want[1] + shear * want[0]];
        let crossed = [0.75 * want[0] - 0.29 * want[1], 0.75 * want[1] - 0.29 * want[0]];
        let cross = |c: UVec4| (Vec2::new(f32::from_bits(c.x), f32::from_bits(c.y)), [c.z, c.w]);
        let (c0, c1, c2) = (cross(u.cross0), cross(u.cross1), cross(u.cross2));
        let lattices = [
            ("vigour", u.vigour.xy(), [u.cells_v.x, u.cells_v.y], u.height.x, want),
            ("vigour 2", u.vigour2.xy(), [u.cells_v.z, u.cells_v.w], u.height.x * u.height.y, want),
            ("place 0", u.place0.xy(), [u.cells_p.x, u.cells_p.y], u.field.x, sheared),
            ("place 1", u.place1.xy(), [u.cells_p.z, u.cells_p.w], u.field.x * PLACE_OCTAVES[0], sheared),
            ("place 2", u.place2.xy(), [u.cells_q.x, u.cells_q.y], u.field.x * PLACE_OCTAVES[1], sheared),
            ("cross 0", c0.0, c0.1, u.field.x, crossed),
            ("cross 1", c1.0, c1.1, u.field.x * PLACE_OCTAVES[0], crossed),
            ("cross 2", c2.0, c2.1, u.field.x * PLACE_OCTAVES[1], crossed),
        ];
        for (name, frac, whole, cells, want) in lattices {
            for (axis, (f, w)) in [frac.x, frac.y].into_iter().zip(whole).enumerate() {
                let got = (f64::from(w as i32) + f64::from(f)) * repeat / f64::from(cells);
                assert!((got - want[axis]).abs() < 0.05, "drift {d}, {name} axis {axis}: {got} against {}", want[axis]);
            }
        }
        for (phase, noise_repeat) in [(u.nphase.xy(), crate::cloud_noise::BASE_REPEAT), (u.nphase.zw(), crate::cloud_noise::EROSION_REPEAT)] {
            let nr = f64::from(noise_repeat);
            for (axis, p) in [phase.x, phase.y].into_iter().enumerate() {
                let off = (f64::from(p) * nr - want[axis]).rem_euclid(nr);
                let gap = off.min(nr - off);
                // The noise takes the repeat in f32 (`1 / uv_scale`): a relative 1e-7 of the drift.
                assert!(gap < 0.05 + 1e-7 * want[axis].abs(), "drift {d}, noise {noise_repeat} axis {axis}: {gap}");
            }
        }
        let field = lv::lv_field(bevy::math::DVec2::new(1_000.0, -2_000.0), drift, repeat);
        assert!((field.x - (1_000.0 + want[0])).abs() < 1e-6 && (field.y - (-2_000.0 + want[1])).abs() < 1e-6);
    }
}

// ---- Checkpoint B1 on the GPU: aids, the uniform's block, the nesting, the shaders ----

/// The aids parse: `..._LIGHT_VOLUME`, `..._LIGHT_VOLUME_CELLS`,
/// `..._LIGHT_TAPS`; and the off states hold nothing and leave the uniform as it came (the trace's
/// legacy variant): the volume off, all eight taps, a light with no strength or not finite.
#[test]
fn the_light_volume_aids_parse_and_off_holds_nothing() {
    use lv::LvLevels::*;
    for (v, want) in [(None, Both), (Some("off"), Off), (Some(" OFF "), Off), (Some("0"), Off), (Some("near"), Near), (Some("far"), Far), (Some("both"), Both), (Some("junk"), Both)] {
        assert_eq!(lv::debug_levels(v), want, "{v:?}");
    }
    assert_eq!(lv::debug_cells(None), [500.0, 4_000.0]);
    assert_eq!(lv::debug_cells(Some("250, 2000")), [250.0, 2_000.0]);
    assert_eq!(lv::debug_cells(Some("x,1e9")), [500.0, 4_000.0], "bad values keep the defaults");
    assert_eq!(lv::debug_cells(Some("1000")), [1_000.0, 4_000.0]);
    for (v, want) in [(None, 0), (Some("0"), 0), (Some("2"), 2), (Some("3"), 3), (Some("8"), 8), (Some("9"), 0), (Some("-1"), 0)] {
        assert_eq!(lv::start_taps(v, 0), want, "{v:?}");
    }
    assert_eq!((lv::start_taps(None, 2), lv::start_taps(Some("9"), 2), lv::start_taps(Some("0"), 2)), (2, 2, 0), "the start's when unset or bad");
    assert_eq!(lv::lv_config_from(|_| None).taps, 2);
    assert_eq!(lv::debug_spread(None), [8, 32]);
    assert_eq!(lv::debug_spread(Some("1,1")), [1, 1], "B1's whole sweep every frame");
    assert_eq!(lv::debug_spread(Some(" 4 , 16 ")), [4, 16]);
    assert_eq!(lv::debug_spread(Some("0,9999")), [8, 32], "bad values keep the defaults");
    assert_eq!(lv::debug_spread(Some("2")), [2, 32]);
    let mut u = ground_uniform(REFERENCE_HEIGHT, HORIZON);
    u.light_dir = c1_dir(45.0, 30.0).extend(1.0);
    let on = lv::LvConfig { levels: Both, cells: [500.0, 4_000.0], taps: 2, spread: lv::LV_SPREAD };
    assert!(lv::lv_active(&u, &on));
    assert!(!lv::lv_active(&u, &lv::LvConfig { levels: Off, ..on }), "off");
    assert!(!lv::lv_active(&u, &lv::LvConfig { taps: 8, ..on }), "all eight taps: the legacy path");
    let mut dark = u;
    dark.light_dir.w = 0.0;
    assert!(!lv::lv_active(&dark, &on), "no light: no sweep");
    dark.light_dir = Vec4::new(f32::NAN, 1.0, 0.0, 1.0);
    assert!(!lv::lv_active(&dark, &on), "a light that is not finite");
    // The uniform as it comes from `uniform` has an all-zero block (today's upload).
    assert_eq!((u.lv, u.lv_near, u.lv_far, u.lv_size, u.lv_cam), (Vec4::ZERO, Vec4::ZERO, Vec4::ZERO, Vec4::ZERO, Vec4::ZERO));
    let mut gpu = lv::gpu::LvGpu::default();
    assert!(gpu.is_empty());
    gpu.clear();
    assert!(gpu.is_empty(), "off holds nothing");
}

/// The pipelines are asked for once in a run, whatever goes off and on again: the pipeline cache
/// keeps each one it is given and tells no second request from a new pipeline.
#[test]
fn the_light_volumes_pipelines_are_asked_for_once_across_activations() {
    let id = bevy::render::render_resource::CachedRenderPipelineId::INVALID;
    let asked = std::cell::RefCell::new(Vec::new());
    let mut gpu = lv::gpu::LvGpu::default();
    for views in [false, true, false] {
        for entry in ["tiles", "reference", "tiles"] {
            gpu.2.want_trace(entry, views, || {
                asked.borrow_mut().push((entry, views));
                id
            });
        }
        gpu.2.want_producer(|| {
            asked.borrow_mut().push(("producer", false));
            id
        });
        gpu.clear();
        assert!(gpu.is_empty());
    }
    assert_eq!(asked.into_inner(), [("tiles", false), ("reference", false), ("producer", false), ("tiles", true), ("reference", true)]);
    // Nothing else of the module queues one.
    let here = include_str!("volumetric_clouds_lightvol.rs");
    assert_eq!(here.matches("cache.queue_render_pipeline(").count(), 2);
}

/// The uniform's block: the windows in field coordinates
/// under a negative drift and at negative positions (floor-snapped, the camera between the middle
/// nodes), the same field point the same node whether the camera or the wind moved, a near cell's
/// move shifting the corner by one node and leaving the camera's place in its cell, the sizes.
#[test]
fn the_light_volume_block_places_the_windows_in_field_coordinates() {
    let cfg = lv::LvConfig { levels: lv::LvLevels::Both, cells: [500.0, 4_000.0], taps: 2, spread: lv::LV_SPREAD };
    let make = |drift: [f64; 2]| uniform(&VolumeInputs { plane_height: 20_000.0, above_ground: 20_000.0, domain: REFERENCE_HEIGHT, horizon: HORIZON, steps: VIEW_STEPS, density: 1.0, sky_scale: 1.0, amount: 0.5, drift, ..Default::default() });
    let drift = [-3.7f64, 1.25];
    let mut u = make(drift);
    let repeat = 1.0 / f64::from(u.map.x);
    let d = lv::lv_drift_units(&u);
    assert!((d.x - drift[0] * repeat).abs() < 0.05 && (d.y - drift[1] * repeat).abs() < 0.05, "{d:?}");
    let cam = Vec3::new(-1_234.5, 900.0, -0.25);
    lv::lv_fill(&mut u, cam, &cfg);
    let field = bevy::math::DVec2::new(f64::from(cam.x), f64::from(cam.z)) + d;
    for (got, across) in [(u.lv_near, 500.0f32), (u.lv_far, 4_000.0)] {
        let w = lv::lv_window(field, &lv::lv_grid(across, &u));
        assert_eq!(got, Vec4::new(w.cam.x, w.cam.y, across, 1_000.0));
        assert!(got.x >= 63.0 && got.x < 64.0 && got.y >= 63.0 && got.y < 64.0, "the camera between the middle nodes: {got}");
    }
    assert_eq!(u.lv, Vec4::new(2.0, 1.0, 1.0, lv::LV_GUARD));
    assert_eq!(u.lv_size, Vec4::new(128.0, 128.0, 41.0, 41.0));
    assert_eq!(u.lv_cam, cam.extend(0.0));
    // The wind carries the field by one near cell along x: the camera moved back by it sees the
    // same window.
    let cells = f64::from(u.field.x);
    let moved = make([drift[0] + 500.0 / repeat, drift[1]]);
    let mut m = moved;
    lv::lv_fill(&mut m, cam - Vec3::X * 500.0, &cfg);
    assert!((m.lv_near - u.lv_near).abs().max_element() < 2e-3, "{} against {} ({cells} cells a repeat)", m.lv_near, u.lv_near);
    // A near cell's move of the camera: the same place in its cell, the corner one node on.
    let mut c = make(drift);
    lv::lv_fill(&mut c, cam + Vec3::X * 500.0, &cfg);
    assert!((c.lv_near.x - u.lv_near.x).abs() < 1e-3 && c.lv_near.y == u.lv_near.y, "{} against {}", c.lv_near, u.lv_near);
    assert!(lv::lv_changed(&u, &c) && !lv::lv_changed(&u, &u), "a moved camera rewrites the uniform");
    assert!(needs_write(&u, &c), "needs_write sees the block");
}

/// The nesting: a level's weight is 1 inside, falls over the guard band,
/// 0 outside and under the base; a point outside every band is read from exactly one level, inside
/// a band from a convex mix; the legacy taps' rest is taken only when weight is left.
#[test]
fn the_light_volume_nesting_reads_one_level_outside_the_band() {
    let g = |x: f32, y: f32| lv::lv_guard(Vec3::new(x, y, 64.0), 128, 128, lv::LV_GUARD);
    assert_eq!((g(64.0, 3.0), g(4.0, 3.0), g(2.0, 3.0), g(0.0, 3.0), g(-0.5, 3.0), g(125.0, 3.0), g(64.0, -0.1), g(64.0, f32::NAN)), (1.0, 1.0, 0.5, 0.0, 0.0, 0.5, 0.0, 0.0));
    let never = || -> f32 { panic!("the legacy rest taken with no weight left") };
    assert_eq!(lv::lv_nest(&[(1.0, 3.0), (1.0, 5.0)], &never), (3.0, 0, true), "the near level alone");
    assert_eq!(lv::lv_nest(&[(0.0, 0.0), (1.0, 5.0)], &never), (5.0, 1, true), "the far level alone");
    let (v, used, whole) = lv::lv_nest(&[(0.5, 3.0), (1.0, 5.0)], &never);
    assert!((v - 4.0).abs() < 1e-6 && used == 0 && !whole, "a convex mix in the near band");
    assert_eq!(lv::lv_nest(&[(0.0, 9.0), (0.0, 9.0)], &|| 7.0), (7.0, 2, false), "the legacy taps alone");
    let (v, used, whole) = lv::lv_nest(&[(0.0, 9.0), (0.25, 8.0)], &|| 4.0);
    assert!((v - (0.25 * 8.0 + 0.75 * 4.0)).abs() < 1e-6 && used == 1 && !whole, "the far level's band");
    for w in [0.0f32, 0.1, 0.5, 0.9, 1.0] {
        let (v, ..) = lv::lv_nest(&[(w, 2.0), (1.0, 6.0)], &never);
        assert!((2.0..=6.0).contains(&v), "convex: {w} gives {v}");
    }
}

/// The march module's two variants: with the light volume off (no LIGHT_VOLUME)
/// nothing of the volume is left and every interval is marched; with it on the two levels are bindings 5
/// and 6; one handoff and one count either way.
#[test]
fn the_light_volume_is_bound_in_its_variant_alone() {
    let march = include_str!("volumetric_clouds_march.wesl").replace("\r\n", "\n");
    let off = lv::lv_resolve(&march, false);
    let on = lv::lv_resolve(&march, true);
    for (text, name) in [(&off, "off"), (&on, "on")] {
        assert_eq!((text.matches("\nfn vc_light_handoff(").count(), text.matches("\nfn vc_near_count(").count()), (1, 1), "{name}");
        assert!(!text.contains("@if"), "{name}");
    }
    assert!(off.contains("    return n;\n") && off.contains("    return vec2<f32>(near, 1.0);\n"), "off: every tap, the whole weight");
    assert!(!off.contains("vc_lv_near") && !off.contains("vc_lv_far"), "off binds nothing new");
    assert!(on.contains("@group(0) @binding(5) var vc_lv_near: texture_2d_array<f32>;") && on.contains("@group(0) @binding(6) var vc_lv_far: texture_2d_array<f32>;"));
    // The uniform block's marker as a line of its own, at the end of the struct.
    let start = march.find("struct CloudVolume {").expect("the struct");
    let body = &march[start..start + march[start..].find("\n}").expect("its end")];
    let marker = body.find("\n    // light volume (stage 2g)\n").expect("the marker line");
    // The light's block (stage 2e) follows it; the volume's own block still ends with lv_cam.
    let light_block = body.find("\n    // light (stage 2e)\n").expect("the light's marker");
    assert!(marker < light_block, "the light volume's block, then the light's");
    let own = &body[marker..];
    let own = &own[..own.find("// light (stage 2e)").unwrap_or(own.len())];
    let last = own.lines().map(str::trim).rfind(|l| !l.is_empty() && !l.starts_with("//"));
    assert_eq!(last, Some("lv_cam: vec4<f32>,"), "the light volume's block ends with lv_cam");
}

/// The producer's text as plain WGSL: the fullscreen vertex output declared in place of its import
/// and the march module (its own import resolved, no LIGHT_VOLUME and no view) pasted in.
fn lv_plain_producer() -> String {
    let producer = include_str!("volumetric_clouds_lightvol.wesl").replace("\r\n", "\n");
    let mut out = String::from("struct FullscreenVertexOutput {\n    @builtin(position) position: vec4<f32>,\n    @location(0) uv: vec2<f32>,\n};\n");
    out.push_str(&lv::lv_resolve(&crate::volumetric_clouds_light::plain_march_wgsl(), false));
    out.push('\n');
    for line in producer.lines() {
        if line.starts_with("import ") {
            continue;
        }
        assert!(!line.contains("@if"), "a WESL condition this does not resolve: {line}");
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// The trace's text as plain WGSL in one variant (medium.rs `plain_trace_wgsl` with the march's
/// conditions resolved for it).
fn lv_plain_trace(light_volume: bool, views: bool) -> String {
    let shader = include_str!("medium_trace.wesl").replace("\r\n", "\n");
    let mut out = String::from("struct FullscreenVertexOutput {\n    @builtin(position) position: vec4<f32>,\n    @location(0) uv: vec2<f32>,\n};\n");
    out.push_str(&lv::lv_resolve_variant(&crate::volumetric_clouds_light::plain_march_wgsl(), light_volume, views));
    out.push('\n');
    for line in shader.lines() {
        if !line.starts_with("import ") {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn lv_validate(source: &str) -> wgpu::naga::Module {
    use wgpu::naga;
    let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).unwrap_or_else(|e| panic!("{e:?}"));
    module
}

/// naga: the producer, and the trace's two entries in both variants, each without a
/// view and with (CLOUD_VIEWS), are valid WGSL; the producer's bindings are the Rust layout's in order (gpu `producer_layout`: the slice
/// with a dynamic offset, the march's uniform, the base noise, its sampler, the erosion noise, the
/// layer above, the outer level), its slice uniform one vec4, its one target an f32.
#[test]
fn the_light_volume_shaders_are_valid_wgsl() {
    let module = lv_validate(&lv_plain_producer());
    let entry = module.entry_points.iter().find(|e| e.name == "lv_produce").expect("the producer's entry");
    assert_eq!(entry.stage, wgpu::naga::ShaderStage::Fragment);
    for (light_volume, views) in [(false, false), (true, false), (false, true), (true, true)] {
        let module = lv_validate(&lv_plain_trace(light_volume, views));
        for name in ["medium_trace_refined", "medium_trace_tiles_refined"] {
            let entry = module.entry_points.iter().find(|e| e.name == name).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(entry.stage, wgpu::naga::ShaderStage::Fragment);
        }
    }
    let producer = include_str!("volumetric_clouds_lightvol.wesl").replace("\r\n", "\n");
    let bindings: Vec<&str> = producer.lines().filter(|l| l.starts_with("@group(0) @binding(")).collect();
    assert_eq!(
        bindings,
        [
            "@group(0) @binding(0) var<uniform> lv_slice: LvSlice;",
            "@group(0) @binding(1) var<uniform> volume: CloudVolume;",
            "@group(0) @binding(2) var noise_base: texture_3d<f32>;",
            "@group(0) @binding(3) var noise_sampler: sampler;",
            "@group(0) @binding(4) var noise_erosion: texture_3d<f32>;",
            "@group(0) @binding(5) var lv_above: texture_2d_array<f32>;",
            "@group(0) @binding(6) var lv_outer: texture_2d_array<f32>;",
        ]
    );
    assert!(producer.contains("struct LvSlice {\n") && producer.contains("    slot: vec4<f32>,\n}"));
    assert_eq!(<lv::gpu::LvSliceUniform as bevy::render::render_resource::ShaderType>::min_size().get(), 16);
    assert!(producer.contains("fn lv_produce(in: FullscreenVertexOutput) -> @location(0) f32 {"));
    assert_eq!(lv::gpu::LV_FORMAT, bevy::render::render_resource::TextureFormat::R16Float);
    assert_eq!(lv::gpu::LV_DEF, "LIGHT_VOLUME");
    // The march's sub-step bound is the mirror's.
    assert!(producer.contains(&format!("const LV_STEP: f32 = {:.1};", lv::LV_SWEEP_STEP)));
}

/// The view variant of the march differs from what a run draws in two functions' bodies alone (with the
/// light volume off and on): `vc_view_code`, 0 without a view, and `vc_count_turn`, empty without one; the
/// refined march's stretches end at the sampling ceiling for a camera under it (volumetric_clouds_model2_tests.rs
/// `the_stretch_past_the_sampling_ceiling_holds_nothing`); and the trace's pipelines take it by this frame's
/// uniform.
#[test]
fn the_view_variant_is_two_functions() {
    use crate::volumetric_clouds::VIEWS_DEF;
    let march = include_str!("volumetric_clouds_march.wesl").replace("\r\n", "\n");
    let counted = "    if turn <= VC_TURN_HALVE {\n        (*work).x += 1.0;\n    } else if turn == VC_TURN_SAMPLE {\n        (*work).y += 1.0;\n    } else {\n        (*work).z += 1.0;\n    }\n";
    let lines = [
        ("fn vc_view_code(v: CloudVolume) -> u32 {\n", "    return 0u;\n", "    return u32(clamp(v.flags.z, 0.0, 13.0) + 0.5);\n"),
        ("fn vc_count_turn(work: ptr<function, vec3<f32>>, turn: u32) {\n", "", counted),
    ];
    for light_volume in [false, true] {
        let squeeze = |s: String| s.lines().filter(|l| !l.is_empty()).map(|l| format!("{l}\n")).collect::<String>();
        let mut plain = squeeze(lv::lv_resolve(&march, light_volume));
        let mut views = squeeze(lv::lv_resolve_variant(&march, light_volume, true));
        assert!(!plain.contains("@if") && !views.contains("@if"));
        for (head, none, read) in lines {
            let (p, w) = (format!("{head}{none}}}\n"), format!("{head}{read}}}\n"));
            assert_eq!((plain.matches(head).count(), views.matches(head).count()), (1, 1), "{head}");
            assert!(plain.contains(&p) && views.contains(&w), "{head}");
            (plain, views) = (plain.replace(&p, ""), views.replace(&w, ""));
        }
        assert!(plain == views, "the rest of the march is the same text (light volume {light_volume})");
    }
    assert_eq!(march.matches(&format!("@if({VIEWS_DEF}) ")).count(), 2);
    assert_eq!(march.matches(&format!("@if(!{VIEWS_DEF}) ")).count(), 2);
    // The trace takes the clipped stretches.
    let of = |head: &str| {
        let f = &march[march.find(head).unwrap()..];
        f[..f.find("\n}\n").unwrap()].to_string()
    };
    assert!(of("fn vc_trace_refined(").contains("let spans = vc_refined_spans(ray, v);"));
    assert!(of("fn vc_refined_spans(").contains("let ceiling = vc_sampling_ceiling(v);\n    return vc_spans_to(ray, select(v.slab.x + max(v.slab.y, 1.0), ceiling, ray.origin.y < ceiling), v);"));
    // The trace's variant by this frame's uniform, asked for only when it is the one to draw with.
    let here = include_str!("volumetric_clouds_lightvol.rs").replace("\r\n", "\n");
    for text in [
        "let views = view_variant(clouds);\n        state.views = views;\n        pipelines.want_trace(cx.entry.1, views, ",
        "cache.get_render_pipeline(pipelines.trace(state.key.entry, state.views)?)?",
    ] {
        assert_eq!(here.matches(text).count(), 1, "{text}");
    }
    let medium = include_str!("medium.rs").replace("\r\n", "\n");
    for text in ["let views = view_variant(&trace.clouds);", "pl.reference[usize::from(views)].get_or_insert_with(", "pl.tiles[usize::from(views)].get_or_insert_with(", "pl.reference[usize::from(p.views)].and_then(|id| cache.get_render_pipeline(id))", "pl.tiles[usize::from(p.views)].and_then(|id| cache.get_render_pipeline(id))"] {
        assert_eq!(medium.matches(text).count(), 1, "{text}");
    }
    // What the choice reads is written whenever it changes (`needs_write` compares it exactly).
    let mut u = uniform(&VolumeInputs::default());
    (u.nscale.z, u.flags.z) = (0.0, 0.499);
    for k in 0..2 {
        let mut w = u;
        match k {
            0 => w.nscale.z += 1e-5,
            _ => w.flags.z += 1e-5,
        }
        assert!(crate::volumetric_clouds::needs_write(&u, &w), "field {k}");
    }
}

/// The cost of a frame's sweep (texels x evaluations per level):
/// 128 x 128 x 40 nodes under the top a level, the sub-steps over one 1,000-unit layer at suns 80,
/// 45 and 15 (5, 6 and 16), and the default lattices' bytes.
#[test]
fn the_light_volume_sweep_costs_what_the_report_says() {
    let u = ground_uniform(REFERENCE_HEIGHT, HORIZON);
    let (gn, gf) = (lv::lv_grid(500.0, &u), lv::lv_grid(4_000.0, &u));
    assert_eq!((gn.layers, gn.up, gf.layers), (41, 1_000.0, 41));
    assert_eq!((gn.texels(), gn.bytes()), (671_744, 1_343_488));
    let texels = 128u64 * 128 * 40;
    for (sun, steps) in [(80.0f32, 5u64), (45.0, 6), (15.0, 16)] {
        assert_eq!(lv::lv_sweep_evaluations(&gn, sun.to_radians().sin()), texels * steps, "sun {sun}");
    }
}

// ---- Checkpoint B2: the swap bookkeeping on the CPU ----

/// A frame's inputs for the schedule tests: the uniform of a fixed weather at `amount` with the
/// light at `elevation` degrees (azimuth 30) and the wind's published drift `drift` (repeats).
fn lv_frame_uniform(amount: f32, elevation: f32, drift: [f64; 2]) -> CloudVolumeUniform {
    uniform(&VolumeInputs {
        plane_height: 20_000.0,
        above_ground: 20_000.0,
        domain: REFERENCE_HEIGHT,
        horizon: HORIZON,
        steps: VIEW_STEPS,
        density: 1.0,
        sky_scale: 1.0,
        amount,
        drift,
        light_dir: c1_dir(elevation, 30.0),
        light_strength: 1.0,
        ..Default::default()
    })
}

/// The bookkeeping the schedule is held to, frame by frame: per level and set the layers written
/// since its sweep's snapshot was uploaded, that snapshot, the frame it started, the far set it
/// read; the far set a near sweep under way reads; the fronts the trace read last frame.
struct LvBook {
    spread: [u32; 2],
    counts: [[Vec<u32>; 2]; 2],
    snap: [[Option<CloudVolumeUniform>; 2]; 2],
    start: [[u32; 2]; 2],
    outer: [[Option<usize>; 2]; 2],
    next: [[u32; 2]; 2],
    near_outer: Option<usize>,
    fronts: [Option<usize>; 2],
}

impl LvBook {
    fn new(cfg: &lv::LvConfig) -> LvBook {
        LvBook { spread: cfg.spread, counts: Default::default(), snap: [[None; 2]; 2], start: [[0; 2]; 2], outer: [[None; 2]; 2], next: [[0; 2]; 2], near_outer: None, fronts: [None; 2] }
    }

    /// Checks one frame's plan: a sweep's snapshot only when it starts, every layer of a set
    /// written once per sweep, a set never written while the trace reads it (but the frame it is
    /// finished), a near sweep's far set never written under it, a set promoted only whole, the
    /// trace's window for each level its front's own (the front's bake camera's position plus the
    /// field-relative move since), a sweep `spread` frames long.
    fn check(&mut self, frame: u32, plan: &lv::LvPlan, u: &CloudVolumeUniform, cam: Vec3) {
        // The far level never writes the set a near sweep under way reads (this frame's passes
        // included: the far passes come first); a near sweep starts on the far front, finished.
        let starting = plan.levels[0].filter(|p| p.upload.is_some());
        if starting.is_some() {
            self.near_outer = None;
        }
        if let (Some(o), Some(far)) = (self.near_outer, plan.levels[1].as_ref()) {
            assert_ne!(far.set, o, "frame {frame}: the far set a near sweep reads was swept over");
        }
        if let Some(near) = starting {
            assert_eq!(near.outer, plan.fronts[1], "frame {frame}: a near sweep starts on the far front");
            self.near_outer = near.outer;
        }
        for (i, p) in plan.levels.iter().enumerate() {
            let Some(p) = p else { continue };
            let s = p.set;
            if let Some(snap) = p.upload {
                assert_eq!(p.from, 0, "frame {frame} level {i}: a snapshot only where a sweep starts");
                self.counts[i][s] = vec![0; p.layers as usize];
                self.snap[i][s] = Some(snap);
                self.start[i][s] = frame;
                self.outer[i][s] = p.outer;
                self.next[i][s] = 0;
                if i == 0 {
                    assert_eq!(snap.lv.z, if p.outer.is_some() { 1.0 } else { 0.0 }, "frame {frame}: the near snapshot nests only with a far set");
                }
            } else {
                assert!(self.snap[i][s].is_some(), "frame {frame} level {i}: no snapshot for set {s}");
                assert_eq!(self.outer[i][s], p.outer, "frame {frame}: a near sweep changed its far set");
            }
            assert_eq!(p.from, self.next[i][s], "frame {frame} level {i}: the sweep's layers follow on, top down");
            self.next[i][s] = p.to;
            if plan.fronts[i] == Some(s) {
                assert_eq!(p.to, p.layers, "frame {frame} level {i}: set {s} written while the trace reads it");
            }
            for j in p.written() {
                let c = &mut self.counts[i][s][j as usize];
                *c += 1;
                assert_eq!(*c, 1, "frame {frame} level {i} set {s}: layer {j} written twice in one sweep");
            }
            if p.to == p.layers && self.spread[i] <= p.layers {
                assert_eq!(frame - self.start[i][s] + 1, self.spread[i], "frame {frame} level {i}: the sweep's length");
            }
        }
        if plan.levels[0].as_ref().is_some_and(|p| p.to == p.layers) {
            self.near_outer = None;
        }
        let field = |c: Vec3, u: &CloudVolumeUniform| bevy::math::DVec2::new(f64::from(c.x), f64::from(c.z)) + lv::lv_drift_units(u);
        for (i, (front, got)) in plan.fronts.iter().zip([plan.trace.lv_near, plan.trace.lv_far]).enumerate() {
            let flag = if i == 0 { plan.trace.lv.y } else { plan.trace.lv.z };
            assert_eq!(flag, if front.is_some() { 1.0 } else { 0.0 }, "frame {frame} level {i}: the trace's flag");
            let Some(s) = *front else { continue };
            if self.fronts[i] != Some(s) {
                assert!(self.counts[i][s].iter().all(|&c| c == 1), "frame {frame} level {i}: set {s} promoted before every layer was written: {:?}", self.counts[i][s]);
            }
            let snap = self.snap[i][s].expect("a front has its snapshot");
            let own = if i == 0 { snap.lv_near } else { snap.lv_far };
            let moved = (field(cam, u) - field(snap.lv_cam.truncate(), &snap)) / f64::from(own.z);
            let want = Vec2::new(own.x + moved.x as f32, own.y + moved.y as f32);
            let at = Vec2::new(got.x, got.y);
            assert!((at - want).abs().max_element() < 2e-3, "frame {frame} level {i}: the trace reads set {s} at {at} but its window puts the camera at {want}");
            assert_eq!((got.z, got.w), (own.z, own.w), "frame {frame} level {i}: the front's own spacings");
        }
        self.fronts = plan.fronts;
    }
}

/// The swap bookkeeping at the default spreads with the camera running (344 units a second,
/// diagonally) and the fastest wind (600 a second along x) at 60 frames a second, the sun creeping
/// (0.0025 degrees a frame): every layer of a back set written once per sweep from one snapshot,
/// the trace reading each front at its own window, a near sweep 8 frames, a far one 32; and the
/// guard band holds: once both levels have a set, neither is ever dropped (no event).
#[test]
fn the_light_volume_swaps_whole_sets_read_at_their_own_windows() {
    let cfg = lv::LvConfig { levels: lv::LvLevels::Both, cells: [lv::LV_ACROSS, lv::LV_FAR_ACROSS], taps: 0, spread: lv::LV_SPREAD };
    let mut sched = lv::LvSchedule::new(cfg);
    let mut book = LvBook::new(&cfg);
    let repeat = 1.0 / f64::from(lv_frame_uniform(0.5, 45.0, [0.0; 2]).map.x);
    let (mut near_swaps, mut far_swaps) = (0usize, 0usize);
    for frame in 0..400u32 {
        let t = f64::from(frame) / 60.0;
        let u = lv_frame_uniform(0.5, 45.0 + 0.0025 * frame as f32, [-3.0 + 600.0 * t / repeat, 1.5]);
        let run = 344.0 * t as f32 / std::f32::consts::SQRT_2;
        let cam = Vec3::new(-20_000.0 + run, 900.0, 5_000.0 + run);
        let before = sched.fronts();
        let plan = sched.plan(&u, cam);
        sched.encoded();
        book.check(frame, &plan, &u, cam);
        near_swaps += usize::from(plan.fronts[0] != before[0]);
        far_swaps += usize::from(plan.fronts[1] != before[1]);
        assert!(plan.event.is_none(), "frame {frame}: {:?}", plan.event);
        if frame >= 7 {
            assert!(plan.fronts[0].is_some(), "frame {frame}: the near level has a set from its first sweep on");
        }
        if frame >= 32 {
            assert!(plan.fronts[1].is_some(), "frame {frame}: the far level has a set once its first sweep was promoted");
        }
        // The near level's layers a frame: 41 over 8 frames, five or six.
        if let Some(p) = plan.levels[0].as_ref() {
            assert!((5..=6).contains(&(p.to - p.from)), "frame {frame}: {} near layers", p.to - p.from);
        }
    }
    assert!(near_swaps >= 45 && far_swaps >= 10, "swaps near {near_swaps}, far {far_swaps}");
}

/// Spread 1 (`..._LIGHT_VOLUME_SPREAD=1,1`) is B1's behaviour: both levels swept whole every frame
/// from that frame's uniform, read the same frame, the trace's block and the near producer's
/// snapshot exactly `lv_fill`'s for the frame's camera.
#[test]
fn the_light_volume_spread_one_is_the_whole_sweep_every_frame() {
    let cfg = lv::LvConfig { levels: lv::LvLevels::Both, cells: [lv::LV_ACROSS, lv::LV_FAR_ACROSS], taps: 2, spread: [1, 1] };
    let mut sched = lv::LvSchedule::new(cfg);
    let mut book = LvBook::new(&cfg);
    for frame in 0..6u32 {
        let u = lv_frame_uniform(0.4, 60.0, [1.25 + 0.01 * f64::from(frame), -7.5]);
        let cam = Vec3::new(-1_234.5 + 400.0 * frame as f32, 900.0, -0.25);
        let plan = sched.plan(&u, cam);
        sched.encoded();
        book.check(frame, &plan, &u, cam);
        let mut want = u;
        lv::lv_fill(&mut want, cam, &cfg);
        assert_eq!((plan.trace.lv, plan.trace.lv_near, plan.trace.lv_far, plan.trace.lv_size, plan.trace.lv_cam), (want.lv, want.lv_near, want.lv_far, want.lv_size, want.lv_cam), "frame {frame}");
        for p in plan.levels.iter() {
            let p = p.expect("both levels every frame");
            assert_eq!((p.from, p.to), (0, p.layers), "frame {frame}: the whole level");
            assert!(p.upload.is_some(), "frame {frame}: from this frame's uniform");
        }
        let near = plan.levels[0].and_then(|p| p.upload).expect("the near snapshot");
        assert_eq!((near.lv, near.lv_near, near.lv_far, near.lv_cam), (want.lv, want.lv_near, want.lv_far, want.lv_cam), "frame {frame}: the near producer nests in this frame's far set");
        assert_eq!(plan.levels[0].and_then(|p| p.outer), plan.fronts[1], "frame {frame}: the far set read is the one the trace reads");
    }
}

/// A keyed jump restarts the back sweeps; where the front no longer fits it is dropped (the level
/// not read, the legacy taps behind it) [design, B2]: the weather's inputs jumping, the light
/// stepping a degree (backs restart, fronts kept), turning 5 degrees from a front's or to the
/// moon (fronts dropped), a teleport past the near level's guard band (the near front dropped,
/// the far kept), a plan never encoded (everything reset); with the far level off the near level
/// nests in nothing; the far level alone promotes the frame its sweep ends.
#[test]
fn the_light_volume_restarts_on_a_jump() {
    let cfg = lv::LvConfig { levels: lv::LvLevels::Both, cells: [lv::LV_ACROSS, lv::LV_FAR_ACROSS], taps: 0, spread: lv::LV_SPREAD };
    let cam = Vec3::new(3_000.0, 900.0, -8_000.0);
    let warm = |sched: &mut lv::LvSchedule, u: &CloudVolumeUniform, at: Vec3| {
        for _ in 0..48 {
            sched.plan(u, at);
            sched.encoded();
        }
        assert!(sched.fronts().iter().all(Option::is_some), "warm");
    };
    let u = lv_frame_uniform(0.5, 45.0, [2.0, -1.0]);
    let restarted = |p: &lv::LvPlan, i: usize| p.levels[i].is_some_and(|l| l.upload.is_some() && l.from == 0);
    // The weather's inputs jump: both fronts dropped, both sweeps restarted.
    let mut s = lv::LvSchedule::new(cfg);
    warm(&mut s, &u, cam);
    assert!(!lv::lv_weather_jumped(&u, &lv_frame_uniform(0.505, 45.0, [2.0, -1.0])), "a fade's frame is no jump");
    let jumped = lv_frame_uniform(0.9, 45.0, [2.0, -1.0]);
    assert!(lv::lv_weather_jumped(&u, &jumped));
    let p = s.plan(&jumped, cam);
    s.encoded();
    assert_eq!(p.fronts, [None, None], "the weather jumped");
    assert_eq!((p.trace.lv.y, p.trace.lv.z), (0.0, 0.0));
    assert!(p.event.is_some() && restarted(&p, 0) && restarted(&p, 1));
    // The light steps one degree: the sweeps restart, the fronts stay.
    let mut s = lv::LvSchedule::new(cfg);
    warm(&mut s, &u, cam);
    let fronts = s.fronts();
    let p = s.plan(&lv_frame_uniform(0.5, 46.0, [2.0, -1.0]), cam);
    s.encoded();
    assert_eq!(p.fronts, fronts, "a one-degree step keeps the fronts");
    assert!(p.event.is_none() && restarted(&p, 0) && restarted(&p, 1), "{p:?}");
    // Five degrees from the fronts' light (a clock jump), or the moon: dropped.
    for (elevation, what) in [(50.0f32, "a clock jump"), (-45.0, "the moon")] {
        let mut s = lv::LvSchedule::new(cfg);
        warm(&mut s, &u, cam);
        let p = s.plan(&lv_frame_uniform(0.5, elevation, [2.0, -1.0]), cam);
        assert_eq!(p.fronts, [None, None], "{what}");
        assert_eq!(p.event, Some("the light jumped"), "{what}");
    }
    // A teleport of 10,000 units: past the near band (4 cells of 500), inside the far one (4 of
    // 4,000).
    let mut s = lv::LvSchedule::new(cfg);
    warm(&mut s, &u, cam);
    let far = s.fronts()[1];
    let p = s.plan(&u, cam + Vec3::X * 10_000.0);
    assert_eq!(p.fronts, [None, far], "a teleport drops the near front only");
    assert_eq!(p.event, Some("the window jumped"));
    assert!(restarted(&p, 0));
    // A plan never encoded: the next frame starts again.
    let mut s = lv::LvSchedule::new(cfg);
    warm(&mut s, &u, cam);
    s.plan(&u, cam);
    let p = s.plan(&u, cam);
    assert_eq!(p.fronts, [None, None], "nothing claimed that was not written");
    assert!(p.event.is_some() && restarted(&p, 0) && restarted(&p, 1));
    // The far level off: the near level alone, nesting in nothing, the trace's far block empty.
    let near_only = lv::LvConfig { levels: lv::LvLevels::Near, ..cfg };
    let mut s = lv::LvSchedule::new(near_only);
    for frame in 0..20 {
        let p = s.plan(&u, cam);
        s.encoded();
        assert!(p.levels[1].is_none() && p.fronts[1].is_none() && p.trace.lv_far == Vec4::ZERO, "frame {frame}");
        if let Some(snap) = p.levels[0].and_then(|l| l.upload) {
            assert_eq!(snap.lv.z, 0.0, "frame {frame}: no far set to nest in");
        }
    }
    // The far level alone promotes the frame its sweep ends.
    let far_only = lv::LvConfig { levels: lv::LvLevels::Far, ..cfg };
    let mut s = lv::LvSchedule::new(far_only);
    for frame in 0..33u32 {
        let p = s.plan(&u, cam);
        s.encoded();
        assert_eq!(p.fronts[1].is_some(), frame >= 31, "frame {frame}");
    }
}

/// Small `DENSITY`, `SHAPE` and `EROSION` edits (the inputs the light's field reads), below the weather's
/// jump share, restart the sweeps from the edit and keep the fronts; nothing edited restarts nothing.
#[test]
fn the_light_volume_follows_the_live_aids() {
    let cfg = lv::LvConfig { levels: lv::LvLevels::Both, cells: [lv::LV_ACROSS, lv::LV_FAR_ACROSS], taps: 0, spread: lv::LV_SPREAD };
    let cam = Vec3::new(3_000.0, 900.0, -8_000.0);
    let warm = |sched: &mut lv::LvSchedule, u: &CloudVolumeUniform| {
        for _ in 0..48 {
            sched.plan(u, cam);
            sched.encoded();
        }
        assert!(sched.fronts().iter().all(Option::is_some), "warm");
    };
    let restarted = |p: &lv::LvPlan, i: usize| p.levels[i].is_some_and(|l| l.upload.is_some() && l.from == 0);
    let u = lv_frame_uniform(0.5, 45.0, [2.0, -1.0]);
    let edits: [LvEdit; 3] = [("DENSITY", |u| u.march.z *= 1.02), ("SHAPE", |u| u.nscale.z *= 0.98), ("EROSION", |u| u.nscale.w *= 0.98)];
    for (key, edit) in edits {
        let mut e = u;
        edit(&mut e);
        assert!(!lv::lv_weather_jumped(&u, &e) && lv::lv_tune_changed(&u, &e), "{key}");
        let mut s = lv::LvSchedule::new(cfg);
        warm(&mut s, &u);
        let fronts = s.fronts();
        let p = s.plan(&e, cam);
        s.encoded();
        assert_eq!(p.fronts, fronts, "{key}: the fronts stay");
        assert!(p.event.is_none() && restarted(&p, 0) && restarted(&p, 1), "{key}: {p:?}");
        for l in p.levels.iter().flatten() {
            let snap = l.upload.expect("restarted");
            assert_eq!((snap.march.z, snap.nscale.z, snap.nscale.w), (e.march.z, e.nscale.z, e.nscale.w), "{key}: the new sweep bakes the edit");
        }
    }
    assert!(!lv::lv_tune_changed(&u, &u));
}

/// The producers' light filter follows the level's real spacing (#3032 stage 2i
/// M1): each level's frozen snapshot carries its own spacing in `lcell.x` (march
/// `vc_light_cell`), the trace's taps the near level's, so the cells aid moves the filter
/// (`2 x max(footprint, cell)`) with the grid; the uniform as the march makes it carries none (the
/// shader's 500 with the light volume off).
#[test]
fn the_light_filter_follows_the_cells_aid() {
    let cam = Vec3::new(3_000.0, 900.0, -8_000.0);
    let u = lv_frame_uniform(0.5, 45.0, [2.0, -1.0]);
    assert_eq!(u.lcell.x, 0.0, "none without the light volume");
    for cells in [[lv::LV_ACROSS, lv::LV_FAR_ACROSS], [250.0, 8_000.0], [1_000.0, 2_000.0]] {
        let cfg = lv::LvConfig { levels: lv::LvLevels::Both, cells, taps: 0, spread: lv::LV_SPREAD };
        let mut s = lv::LvSchedule::new(cfg);
        let mut seen = [false; 2];
        for frame in 0..40u32 {
            let p = s.plan(&u, cam);
            s.encoded();
            assert_eq!(p.trace.lcell.x, cells[0], "frame {frame}: the taps filter to the near cell");
            for (i, l) in p.levels.iter().enumerate() {
                if let Some(snap) = l.and_then(|l| l.upload) {
                    assert_eq!(snap.lcell.x, cells[i], "frame {frame} level {i}: the level's own spacing");
                    seen[i] = true;
                }
            }
        }
        assert_eq!(seen, [true, true], "{cells:?}");
    }
    let src = include_str!("volumetric_clouds_march.wesl").replace("\r\n", "\n");
    for line in ["lw = 2.0 * max(width, vc_light_cell(v));", "let footprint = select(2.0 * max(width, vc_light_cell(v)), sqrt(across * along), eroded);", "if v.lcell.x > 0.0 {", "return v.lcell.x;", "return VC_LIGHT_CELL;", "const VC_LIGHT_CELL: f32 = 500.0;"] {
        assert!(src.contains(line), "the march no longer has `{line}`");
    }
    assert!(!src.contains("max(width, VC_LIGHT_CELL)"), "no fixed cell in the light's filter");
}
