// The lean model (#3032; volumetric_clouds_model2.rs) on the real field: its support, its march
// plans, its light through the swept light volume against fine integrals, and its density per amount.
// Pulled into volumetric_clouds.rs `mod tests` by one `include!` (it reads that module's private real-field
// mirror: `m_place`, `m_vigour`, `m_hash`, `m_dense`, `c1_spread`, `c1_dir`,
// `c1_check_cells`, and the light volume's test sweep, `lv_sweep_fill`, `lv_sweep_lattice`), so it is
// test code only and adds no module of its own. The density comes from the one CPU function of the lean
// model (`volumetric_clouds_model2::density`) on the production noise bytes at the levels the march reads,
// with the step's filter (`fw`, march `vc_detail_filter`) and the wisps' shift and stretch (volumetric_clouds_wisp.rs
// `wisp`) as the march takes them.

/// Mirror of the overlapping cap envelope at one projected footprint.
fn lean_caps(uv: Vec2, period: u32, shift: f32, width: f32, u: &CloudVolumeUniform) -> f32 {
    let cell_width = 1.0 / (u.nscale.x * period as f32);
    let keep = 1.0 - smoothstep(0.35 * cell_width, 0.85 * cell_width, width);
    if keep <= 0.0 {
        return 0.0;
    }
    let p = (uv - uv.floor()) * period as f32 + Vec2::new(shift, 0.6 * shift);
    let tile = p.floor().as_uvec2();
    let local = p - p.floor();
    let mut peak = 0.0_f32;
    for k in 0..4u32 {
        let (x, z) = (k & 1, k >> 1);
        let bits = (m_hash((tile.x + x) & (period - 1), period, (tile.y + z) & (period - 1)) * 16777216.0) as u32;
        let jitter = Vec2::new((bits & 255) as f32, ((bits >> 8) & 255) as f32) / 255.0 - Vec2::splat(0.5);
        let size = (bits >> 16) as f32 / 255.0;
        let delta = local - Vec2::new(x as f32, z as f32) - jitter * 0.48;
        let radius = 0.24 + 0.51 * size;
        let q = (1.0 - delta.length_squared() / (radius * radius)).max(0.0);
        peak = peak.max((0.58 + 0.42 * size) * q.sqrt() * smoothstep(0.0, 0.16, q));
    }
    keep * (peak - 0.33)
}

/// Interpolated lift to the lowest cardinal outlet of each medium lattice pit.
fn lean_pit_lift(origin: IVec2, f: Vec2, seed: u32) -> f32 {
    let blend = f * f * (Vec2::splat(3.0) - 2.0 * f);
    let at = |cell: IVec2| m_hash(cell.x as u32, seed, cell.y as u32);
    let lift = |corner: IVec2| {
        let cell = origin.wrapping_add(corner);
        let spill = [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y].map(|d| at(cell.wrapping_add(d))).into_iter().fold(1.0, f32::min);
        (spill - at(cell)).max(0.0)
    };
    let row = |z| {
        let left = lift(IVec2::new(0, z));
        left + (lift(IVec2::new(1, z)) - left) * blend.x
    };
    let low = row(0);
    low + (row(1) - low) * blend.y
}

/// Smooth interpolation at four corners whose parent-cell endpoints can wrap.
fn lean_lattice_noise(cell: IVec2, next: IVec2, f: Vec2, seed: u32) -> f32 {
    let blend = f * f * (Vec2::splat(3.0) - 2.0 * f);
    let row = |z| {
        let a = m_hash(cell.x as u32, seed, z);
        a + (m_hash(next.x as u32, seed, z) - a) * blend.x
    };
    let lo = row(cell.y as u32);
    lo + (row(next.y as u32) - lo) * blend.y
}

use crate::volumetric_clouds_model2::open_bank_weight as lean_open_bank_weight;

fn lean_lane_noise(tile: IVec2, f: Vec2) -> f32 {
    let along = f.x + f.y;
    let across = f.x - f.y;
    let sum = tile.x.wrapping_add(tile.y).wrapping_add(along.floor() as i32);
    let diff = tile.x.wrapping_sub(tile.y).wrapping_add(across.floor() as i32);
    let cell = IVec2::new(sum.div_euclid(2), diff);
    let next = IVec2::new(sum.wrapping_add(2).div_euclid(2), diff.wrapping_add(1));
    let uv = Vec2::new(0.5 * (sum.rem_euclid(2) as f32 + along - along.floor()), across - across.floor());
    lean_lattice_noise(cell, next, uv, M_SEED_TOP2)
}

/// Distance inside the small patch and moisture over the larger region.
fn lean_regions(tile: IVec2, f: Vec2, broad: f32, mid: f32, lane: f32) -> Vec2 {
    let spacing = IVec2::new(2, 4);
    let warp = Vec2::new(0.54 * (broad - 0.5) + 0.42 * (mid - 0.5), 0.53 * (lane - 0.5) - 0.38 * (mid - 0.5));
    let shift = Vec2::new(f.x + f.y, f.x - f.y) + spacing.as_vec2() * warp;
    let whole = IVec2::new(tile.x.wrapping_add(tile.y), tile.x.wrapping_sub(tile.y)).wrapping_add(shift.floor().as_ivec2());
    let local = ((whole & IVec2::new(1, 3)).as_vec2() + (shift - shift.floor())) / spacing.as_vec2();
    let mut reach = -0.085_f32;
    for i in 0..4 {
        let corner = IVec2::new(i & 1, i >> 1);
        let cell = whole.wrapping_add(spacing * corner);
        let bits = (m_hash(cell.x.div_euclid(2) as u32, M_SEED_TOP2, cell.y.div_euclid(4) as u32) * 16777216.0) as u32;
        let random = Vec3::new((bits & 255) as f32, ((bits >> 8) & 255) as f32, (bits >> 16) as f32) / 255.0;
        let delta = local - corner.as_vec2() - 0.33 * (random.xy() - Vec2::splat(0.5));
        let axes = Vec2::new((delta.x + 0.35 * (random.x - 0.5) * delta.y) / 1.08, delta.y / (0.84 + 0.22 * random.y));
        reach = reach.max(0.29 + 0.24 * random.z - axes.length());
    }
    let coarse_cell = IVec2::new(whole.x.div_euclid(4), whole.y.div_euclid(8));
    let coarse_next = IVec2::new(whole.x.wrapping_add(4).div_euclid(4), whole.y.wrapping_add(8).div_euclid(8));
    let coarse_uv = ((whole & IVec2::new(3, 7)).as_vec2() + (shift - shift.floor())) / Vec2::new(4.0, 8.0);
    Vec2::new(reach, lean_lattice_noise(coarse_cell, coarse_next, coarse_uv, M_SEED_PLACE[0]))
}

/// Sample one of the wind-advected placement fields (march `vc_m2_bank`).
fn lean_bank_at(xz: Vec2, width: f32, storm: f32, crosswise: bool, u: &CloudVolumeUniform) -> Vec3 {
    let swapped = Vec2::new(xz.y, xz.x);
    let (placed, pw) = if crosswise {
        (0.75 * xz - 0.29 * swapped, 1.04 * width)
    } else {
        let shear = crate::volumetric_clouds_model2::SHEAR;
        (xz + shear * swapped, (1.0 + shear) * width)
    };
    let cell = 1.0 / (u.map.x * u.field.x).max(1e-9);
    let bank_width = pw.max(cell * (0.19 + 0.18 * storm));
    let freqs = [1.0, PLACE_OCTAVES[0], PLACE_OCTAVES[1]];
    let keeps = freqs.map(|f| octave_keep(bank_width, cell / f));
    let uv = placed * u.map.xy();
    let mut lattice = [(IVec2::ZERO, Vec2::ZERO); 3];
    let values = std::array::from_fn(|i| {
        let (phase, whole) = if crosswise {
            let p = [u.cross0, u.cross1, u.cross2][i];
            (Vec2::new(f32::from_bits(p.x), f32::from_bits(p.y)), p.zw().as_ivec2())
        } else {
            ([u.place0.xy(), u.place1.xy(), u.place2.xy()][i], [u.cells_p.xy(), u.cells_p.zw(), u.cells_q.xy()][i].as_ivec2())
        };
        let p = uv * (u.field.x * freqs[i]) + phase;
        lattice[i] = (p.floor().as_ivec2().wrapping_add(whole), p - p.floor());
        m_noise2(lattice[i].0, lattice[i].1, M_SEED_PLACE[i])
    });
    let mut excess = place_from(values, keeps) - u.field.y;
    let interior = (1.0 - storm) * smoothstep(0.30, 0.45, values[0]);
    if interior > 0.0 && keeps[1] > 0.0 {
        let weights = Vec3::from_array(PLACE_WEIGHTS) * Vec3::from_array(keeps);
        let gain = Vec3::from_array(PLACE_WEIGHTS).length() / weights.length();
        let pit = weights.y * lean_pit_lift(lattice[1].0, lattice[1].1, M_SEED_PLACE[1]);
        let gate = smoothstep(0.0, 0.05, pit);
        excess += interior * gain * gate * pit;
        let lip = smoothstep(0.0, 0.035, excess) * smoothstep(0.38, 0.62, values[1]);
        let deficit = (crate::volumetric_clouds_model2::COVER - excess).max(0.0);
        if pit > 0.015 || (lip > 0.0 && deficit > 0.0) {
            let basin = weights.x * lean_pit_lift(lattice[0].0, lattice[0].1, M_SEED_PLACE[0]) * interior * gain;
            excess += (basin * gate * smoothstep(0.015, 0.07, pit)).max(basin.min(deficit) * lip);
        }
    }
    let mut developed = 0.0;
    let open_banks = lean_open_bank_weight(u);
    if crosswise && keeps[1] > 0.0 && open_banks > 0.0 {
        let (tile, fraction) = lattice[1];
        let lane = lean_lane_noise(tile, fraction);
        let moist = smoothstep(0.32, 0.66, lane + 0.24 * (values[1] - 0.5));
        let credit = 0.16 * moist * (1.0 - smoothstep(-0.11, 0.20, excess));
        let weight = open_banks * keeps[1];
        let parent_excess = excess;
        excess += weight * (credit - 0.22 * (1.0 - moist));
        let region = lean_regions(tile, fraction, values[0], values[1], lane);
        let capacity = 1.05 * region.x + 0.50 * (1.0 - open_banks);
        let humidity = region.y + 0.22 * (values[0] - 0.5);
        let wet = smoothstep(0.38, 0.76, humidity);
        let supply = smoothstep(-0.085, 0.10, region.x);
        let ordinary = parent_excess.min(1.40 * (parent_excess - (0.33 + wet * (0.035 - 0.33)))) - 0.095 * (1.0 - supply);
        let fed = parent_excess + 0.067;
        let front = fed.min(1.40 * (fed - 0.035));
        let mature_excess = ordinary + smoothstep(0.59, 0.83, humidity) * (front - ordinary).max(0.0);
        excess += weight * (excess.min(capacity).max(mature_excess) - excess);
        developed = weight * smoothstep(-0.015, 0.13, mature_excess);
    }
    Vec3::new(excess, octave_keep(pw, cell), developed)
}

/// Crossing either middle lattice axis or its diagonal seam preserves moisture.
#[test]
fn the_fair_lane_field_is_continuous_across_wrapped_cells() {
    let mut transition_samples = 0;
    let mut patch_edges = 0;
    let fields = |tile, frac, broad, mid| {
        let lane = lean_lane_noise(tile, frac);
        let region = lean_regions(tile, frac, broad, mid, lane);
        Vec3::new(smoothstep(0.32, 0.66, lane + 0.24 * (mid - 0.5)), region.x, region.y)
    };
    for tile in [IVec2::new(3, -7), IVec2::new(i32::MAX, 0), IVec2::new(0, i32::MIN), IVec2::new(i32::MAX - 8, 8)] {
        for i in 1..20 {
            let f = i as f32 / 20.0;
            for axis in [IVec2::X, IVec2::Y] {
                let edge = if axis == IVec2::X { Vec2::new(1.0, f) } else { Vec2::new(f, 1.0) };
                let a = fields(tile, edge - 1e-6 * axis.as_vec2(), 1.0 - f, f);
                let b = fields(tile.wrapping_add(axis), edge - axis.as_vec2() + 1e-6 * axis.as_vec2(), 1.0 - f, f);
                assert!(a.distance(b) < 5e-5, "middle cell seam at {tile}, {axis}: {a} {b}");
                patch_edges += usize::from(a.y.abs() < 0.08);
                transition_samples += usize::from(a.x > 0.1 && a.x < 0.9);
            }
            // The half-frequency diagonal can also cross inside a single cell.
            let diagonal = Vec2::new(f, 1.0 - f);
            let a = fields(tile, diagonal - 1e-6 * Vec2::X, 1.0 - f, f);
            let b = fields(tile, diagonal + 1e-6 * Vec2::X, 1.0 - f, f);
            assert!(a.distance(b) < 5e-5);
        }
    }
    assert!(transition_samples >= 12, "only {transition_samples} partially moist edges");
    assert!(patch_edges >= 12, "only {patch_edges} flooded patch edges");
}

#[test]
fn fair_weather_contains_both_shallow_and_developed_updrafts() {
    let fair = lean_ground_with_puff(0.35, None, 1.0);
    let overcast = lean_ground_with_puff(0.85, None, 1.0);
    let mut shallow = 0;
    let mut shoulders = 0;
    let mut developed = 0;
    for z in -24..=24 {
        for x in -24..=24 {
            let pos = Vec2::new(x as f32, z as f32) * 6300.0;
            let bank = lean_bank_at(pos, 450.0, 0.0, true, &fair);
            if bank.x > 0.015 {
                if bank.z < 0.05 { shallow += 1; }
                else if bank.z > 0.85 { developed += 1; }
                else { shoulders += 1; }
            }
            assert_eq!(lean_bank_at(pos, 450.0, 1.0, false, &overcast).z, 0.0);
        }
    }
    assert!(shallow > 24 && shoulders > 24 && developed > 24,
        "expected distinct updrafts and a continuous transition: {shallow} shallow, {shoulders} shoulders, {developed} developed");
}

#[test]
fn both_bank_fields_follow_the_same_wind_through_a_weather_fade() {
    let map = Vec2::new(1.0 / 64000.0, 1.0 / 54000.0);
    let travel = Vec2::new(-8935.0, 17200.0);
    for drift in [[-0.006, 0.63], [67_108_863.81, -33_554_433.19]] {
        for amount in [0.3, 0.63, 0.9] {
            let input = VolumeInputs {
                uv_scale: map, drift, puff: 1.0, amount, domain: 24000.0,
                ..Default::default()
            };
            let start = uniform(&input);
            let moved = uniform(&VolumeInputs {
                drift: [drift[0] + travel.x as f64 * map.x as f64, drift[1] + travel.y as f64 * map.y as f64], ..input
            });
            let storm = smoothstep(0.55, 0.72, amount);
            for i in 0..17 {
                let xz = Vec2::new(1499.0 * i as f32 - 76600.0, 6361.0 * i as f32 - 4000.0);
                for layout in [false, true] {
                    let a = lean_bank_at(xz + travel, 450.0, storm, layout, &start);
                    let b = lean_bank_at(xz, 450.0, storm, layout, &moved);
                    assert!((a - b).abs().max_element() < 2e-5, "advected field {layout}: {a} {b}");
                }
                let first = lean_deck_relief_at(xz + travel, 450.0, &start);
                let next = lean_deck_relief_at(xz, 450.0, &moved);
                assert!((first - next).abs().max_element() < 2e-5, "advected storm relief: {first} {next}");
                let cross = 1.0 - storm;
                let expected = lean_bank_at(xz, 450.0, storm, false, &moved).lerp(lean_bank_at(xz, 450.0, storm, true, &moved), cross);
                let col = lean_column_at(xz, amount, None, 450.0, &moved);
                assert!((col.over - expected.x).abs() < 1e-6);
                assert!((col.developed_bank - expected.z).abs() < 1e-6);
            }
            let delta = crate::volumetric_clouds_lightvol::lv_drift_units(&moved) - crate::volumetric_clouds_lightvol::lv_drift_units(&start);
            assert!((delta - travel.as_dvec2()).length() < 0.08, "light cache drift {delta}");
        }
    }
}

/// Independent CPU sample of the two storm fields at the unwrapped wind phase.
fn lean_deck_relief_at(xz: Vec2, width: f32, u: &CloudVolumeUniform) -> Vec2 {
    let shear = crate::volumetric_clouds_model2::SHEAR;
    let uv = (xz + shear * Vec2::new(xz.y, xz.x)) * u.map.xy() * u.field.x;
    let lattice = |p: Vec2, whole: UVec2| (p.floor().as_ivec2().wrapping_add(whole.as_ivec2()), p - p.floor());
    let (cell, frac) = lattice(uv + u.place0.xy(), u.cells_p.xy());
    let span = IVec2::new(4, 2);
    let sector = IVec2::new(cell.x.div_euclid(span.x), cell.y.div_euclid(span.y));
    let rem = IVec2::new(cell.x.rem_euclid(span.x), cell.y.rem_euclid(span.y));
    let next = cell.wrapping_add(span);
    let next = IVec2::new(next.x.div_euclid(span.x), next.y.div_euclid(span.y));
    let f = (rem.as_vec2() + frac) / span.as_vec2();
    let broad = lean_lattice_noise(sector, next, f, M_SEED_TOP2);
    let local = m_noise2(cell, frac, M_SEED_TOP);
    let (fine_cell, fine_frac) = lattice(uv * PLACE_OCTAVES[0] + u.place1.xy(), u.cells_p.zw());
    let detail = m_noise2(fine_cell, fine_frac, M_SEED_TOP2);
    let pw = width * (1.0 + shear);
    let spacing = 1.0 / (u.map.x * u.field.x).max(1e-9);
    let band = octave_keep(pw, spacing) * (local - 0.5);
    let bed = 0.78 * octave_keep(pw, 2.0 * spacing) * (broad - 0.5) + 0.22 * band;
    let water = 0.65 * band + 0.35 * octave_keep(pw, spacing / PLACE_OCTAVES[0]) * (detail - 0.5);
    Vec2::new(bed, water)
}

#[test]
fn the_regional_storm_field_crosses_signed_wind_cell_boundaries() {
    let mut before = uniform(&VolumeInputs { amount: 0.9, domain: 24000.0, ..Default::default() });
    for axes in [(true, false), (false, true), (true, true)] {
        before.place0.x = if axes.0 { 0.9999 } else { 0.42 };
        before.place0.y = if axes.1 { 0.9999 } else { 0.68 };
        before.cells_p.x = if axes.0 { i32::MAX as u32 } else { 13 };
        before.cells_p.y = if axes.1 { i32::MAX as u32 } else { (-9i32) as u32 };
        let mut after = before;
        if axes.0 {
            after.place0.x = 0.0001;
            after.cells_p.x = i32::MIN as u32;
        }
        if axes.1 {
            after.place0.y = 0.0001;
            after.cells_p.y = i32::MIN as u32;
        }
        let delta = lean_deck_relief_at(Vec2::ZERO, 10.0, &before) - lean_deck_relief_at(Vec2::ZERO, 10.0, &after);
        assert!(delta.abs().max_element() < 1e-5, "storm seam at {axes:?}: {delta}");
    }
}

/// The lean model's column at world (x, z) `xz` under the amount `a` (march `vc_m2_column` at the placement
/// footprint `pw`, no drift; 0 the resolved field), a type forced everywhere or the local choice.
fn lean_column_at(xz: Vec2, a: f32, forced: Option<CloudType>, pw: f32, u: &CloudVolumeUniform) -> crate::volumetric_clouds_model2::Column {
    let domain = u.field.z;
    let repeat = crate::cloud_shadows::REPEAT_UNITS;
    let cells = vigour_cells(domain, repeat);
    let storm = smoothstep(0.55, 0.72, a);
    let cross = u.kind.x * (1.0 - storm);
    let mut bank = Vec3::ZERO;
    if cross < 1.0 { bank += (1.0 - cross) * lean_bank_at(xz, pw, storm, false, u); }
    if cross > 0.0 { bank += cross * lean_bank_at(xz, pw, storm, true, u); }
    let (over, res) = (bank.x, bank.y);
    let fair = u.nscale.z.clamp(0.0, 1.0) * smoothstep(0.0, 0.10, a) * (1.0 - storm);
    let mut head = 0.0;
    if fair > 0.0 && over > 0.0 {
        let uv = xz * u.nscale.x + Vec2::new(u.nphase.x, u.nphase.y);
        let shoulder = lean_caps(uv, 16, 0.37, pw, u);
        let cap = lean_caps(uv, 32, 0.0, pw, u);
        let tuft_uv = Vec2::new(uv.x + uv.y, uv.y);
        let bud = lean_caps(tuft_uv, 64, 0.37, 1.6181 * pw, u);
        let crest = 0.72 * shoulder + (0.75 + 0.60 * smoothstep(-0.30, 0.35, shoulder)) * cap;
        let development = smoothstep(0.035, 0.15, over);
        let cluster = smoothstep(0.10, 0.50, shoulder);
        let tower = development * (0.80 * shoulder.max(0.0) + cluster * (0.35 + 4.40 * cap.max(0.0)));
        let nested = tower + crest + (0.55 + 0.57 * smoothstep(-0.25, 0.40, cap)) * bud;
        head = fair * smoothstep(0.0, 0.045, over) * (nested.max(0.0) + 0.40 * nested.min(0.0));
    }
    let vigour = m_vigour(xz / repeat, cells, Vec4::ZERO, Vec4::ZERO).max(1e-6);
    let mix = match forced {
        Some(t) => t.weights().extend(1.0),
        None => type_mix(a, Vec3::ZERO).extend(0.0),
    };
    let types = local_types(mix, over.max(0.0), vigour);
    if over > 0.0 {
        head += (1.55 - head) * crate::volumetric_clouds_model2::lobe_weight(types, u);
    }
    let deck = storm * smoothstep(0.0, 0.08, types.x);
    let deck_relief = if deck > 0.0 && over > 0.0 { deck * lean_deck_relief_at(xz, pw, u) } else { Vec2::ZERO };
    let (fair_water_excess, base_humidity) = if over > 0.0 && a < 0.75 {
        use crate::cloud_noise::{level_for, production, BASE_REPEAT, BASE_SIZE};
        let uv = xz * u.nscale.x + Vec2::new(u.nphase.x, u.nphase.y);
        let level = level_for(pw, BASE_SIZE, BASE_REPEAT, 8);
        let value = production().base.sample([uv.x + 0.31, 0.27, uv.y + 0.63], level)[0];
        let moisture = crate::volumetric_clouds_model2::spread(value, u);
        let core = crate::volumetric_clouds_model2::cover(over);
        ((-0.36 + core * (0.98 + 0.95 * moisture)) * u.nscale.z.clamp(0.0, 1.0), moisture)
    } else {
        (0.0, 0.5)
    };
    crate::volumetric_clouds_model2::Column { head, over, types, vigour, res, deck_relief, fair_water_excess, base_humidity, developed_bank: bank.z }
}

/// Three samples along the R4 Goodsprings noon aperture and one through the
/// adjacent open lane. Exercise actual density, including its support check.
#[test]
fn the_goodsprings_aperture_has_a_foot_but_the_next_lane_is_open() {
    let amount = 0.29;
    let u = uniform(&VolumeInputs {
        plane_height: 28192.0, above_ground: 20000.0, domain: 24000.0, amount, density: 0.76,
        ..Default::default()
    });
    let sample = |x, z, h| lean_at(Vec2::new(x, z), u.slab.x + h, (180.0, 400.0, 600.0), amount, None, &u, true);
    for (x, z, h) in [(-35738.0, 2944.0, 400.0), (-35472.0, 2981.0, 550.0), (-35207.0, 3017.0, 700.0)] {
        let (supported, (ext, _)) = sample(x, z, h);
        assert!(supported && ext > 0.0013, "broken bank at {x}, {z}, {h}: {ext}");
    }
    // The R5 light oval sat on the lip outside the deepest part of the aperture.
    for xz in [Vec2::new(-30712.0, 3095.0), Vec2::new(-31300.0, 3100.0)] {
        let col = lean_column_at(xz, amount, None, 400.0, &u);
        assert!(crate::volumetric_clouds_model2::cover(col.over) > 0.90, "thin lip at {xz}: {}", col.over);
    }
    assert_eq!(sample(-11091.0, 10264.0, 550.0), (false, (0.0, 0.0)));
}

/// The upper head of a northeast bank clears its own foreground shoulder as
/// seen from Goodsprings. Integrate the median roof along three ground rays,
/// including curvature and other banks between the camera and the crest.
#[test]
fn a_noon_head_breaks_the_projected_skyline() {
    use crate::volumetric_clouds_model2::geometry;
    let amount = 0.29;
    let u = uniform(&VolumeInputs {
        plane_height: 28192.0, above_ground: 20000.0, domain: 24000.0, horizon: 400000.0, amount,
        ..Default::default()
    });
    let eye = Vec2::new(-71680.0, -2048.0);
    let crest = Vec2::new(-20610.0, -47540.0);
    let across = (crest - eye).normalize().perp();
    let chord = |offset: f32| {
        let sight = crest + across * offset - eye;
        let range = sight.length();
        let rise = (u.slab.x + 7300.0 - 8440.0) / range - range / (2.0 * u.reach.y);
        let mut inside = 0.0;
        for i in 100..735 {
            let distance = 150.0 * i as f32;
            let above = 8440.0 - u.slab.x + distance * (rise + distance / (2.0 * u.reach.y));
            if above <= 0.0 || above >= u.slab.y {
                continue;
            }
            let col = lean_column_at(eye + sight * (distance / range), amount, None, 320.0, &u);
            if col.over > 0.0 && above < geometry(&col, &u).crown_top {
                inside += 150.0;
            }
        }
        inside
    };
    let centre = chord(0.0);
    let gaps = [chord(-2400.0), chord(2400.0)];
    assert!(centre >= 1050.0 && gaps.iter().all(|&gap| gap < 450.0), "crest {centre}, adjoining sky {gaps:?}");
}

/// A side section through real fair-weather density. A visible head must have
/// a saddle on both sides while its body stays attached across the section.
#[test]
fn fair_clouds_have_attached_heads_at_the_projected_footprint() {
    for puff in [0.0, 1.0] {
        assert_attached_heads(puff);
    }
}

fn assert_attached_heads(puff: f32) {
    let amount = 0.3;
    let u = lean_ground_with_puff(amount, None, puff);
    let spacing = 375.0;
    let footprints = (180.0, 400.0, 600.0); // cast, projected on base, march step
    for centre in lean_clouds_in(amount, None, 24, &u) {
        let mut contour = [0_i32; 49];
        for (i, height) in contour.iter_mut().enumerate() {
            let xz = centre + Vec2::X * ((i as f32 - 24.0) * spacing);
            let col = lean_column_at(xz, amount, None, footprints.1, &u);
            let ceiling = crate::volumetric_clouds_model2::extent(&col, &u);
            for k in (1..=(ceiling / 200.0) as i32).rev() {
                let y = u.slab.x + 200.0 * k as f32;
                let (_, (ext, _)) = lean_at(xz, y, footprints, amount, None, &u, true);
                if 1.0 - (-ext * spacing).exp() > 0.15 {
                    *height = k;
                    break;
                }
            }
        }
        for body in contour.split(|h| *h == 0) {
            let mut heads = 0;
            let mut next = 0;
            for (i, w) in body.windows(9).enumerate() {
                if i < next || w[4] != *w.iter().max().unwrap() {
                    continue;
                }
                if w[4] - w[..4].iter().min().unwrap() >= 2 && w[4] - w[5..].iter().min().unwrap() >= 2 {
                    heads += 1;
                    next = i + 4;
                }
            }
            if heads >= 3 {
                return;
            }
        }
    }
    panic!("PUFF {puff}: no attached 18,000-unit section retained three heads with 400-unit saddles");
}

#[test]
fn heads_cannot_seed_an_island_or_pattern_the_storm_deck() {
    for amount in [0.29, 0.73] {
        let u = lean_ground(amount, None);
        let mut bare = u;
        bare.nscale.z = 0.0; // Hold the parent field fixed and remove the head contribution.
        let mut occupied = 0;
        let mut headed = 0;
        for z in 0..80 {
            for x in 0..80 {
                let p = Vec2::new(x as f32, z as f32) * 1200.0;
                let col = lean_column_at(p, amount, None, 400.0, &u);
                let parent = lean_column_at(p, amount, None, 400.0, &bare);
                assert_eq!(col.over, parent.over);
                if col.over > 0.0 {
                    occupied += 1;
                } else {
                    assert_eq!(col.head, 0.0);
                }
                headed += usize::from(col.head.abs() > 0.01);
            }
        }
        assert!(occupied > 0);
        if amount < 0.5 {
            assert!(headed > 0 && occupied < 6400);
        } else {
            assert_eq!(headed, 0);
        }
    }
}

#[test]
fn fair_head_tiers_wrap_and_filter_continuously() {
    let u = lean_ground(0.3, None);
    for (period, shift) in [(16, 0.37), (32, 0.0), (64, 0.37)] {
        let at = |uv, width| lean_caps(uv, period, shift, width, &u);
        let p = Vec2::new(-0.1875, 0.25);
        assert_eq!(at(p, 400.0), at(p + Vec2::new(1.0, -1.0), 400.0));
        let epsilon = Vec2::X / 16777216.0;
        for i in 0..=period {
            let boundary = Vec2::new((i as f32 - shift) / period as f32, 0.25);
            assert!((at(boundary + epsilon, 400.0) - at(boundary - epsilon, 400.0)).abs() < 0.002);
        }
        assert_eq!(at(p, 1.0 / (period as f32 * u.nscale.x)), 0.0);
    }
}

/// The ground uniform (plane 20,000 over the reference at y 0) at the amount `a`, a type forced or
/// not, the direct light on (the taps counted), the wisps as designed (WISP 1).
fn lean_ground(a: f32, forced: Option<CloudType>) -> CloudVolumeUniform {
    lean_ground_with_puff(a, forced, 0.0)
}

fn lean_ground_with_puff(a: f32, forced: Option<CloudType>, puff: f32) -> CloudVolumeUniform {
    let mut u = uniform(&VolumeInputs { plane_height: 20_000.0, above_ground: 20_000.0, domain: REFERENCE_HEIGHT, horizon: HORIZON, steps: VIEW_STEPS, density: 1.0, sky_scale: 1.0, amount: a, forced, puff, ..Default::default() });
    u.light_dir.w = 1.0;
    u
}

#[test]
fn fair_banks_filter_by_projected_area_and_decks_by_projected_length() {
    let mut u = lean_ground(0.3, None);
    let dir = c1_dir(5.0, 60.0);
    let rel = dir * 180_000.0;
    let width = 800.0;
    let long = place_width(rel, dir, width, u.reach.y);
    let fair = cloud_place_width(rel, dir, width, &u);
    assert!(long > 4.0 * width);
    assert!((fair * fair / (width * long) - 1.0).abs() < 1e-5);
    assert_eq!(cloud_place_width(Vec3::Y, Vec3::Y, width, &u), width);
    u.scale.w = 0.64;
    let transition = cloud_place_width(rel, dir, width, &u);
    assert!(fair < transition && transition < long);
    u.scale.w = 0.72;
    assert_eq!(cloud_place_width(rel, dir, width, &u), long);
}

#[test]
fn tiled_mixed_banks_recover_the_placement_of_their_reconstructed_texel() {
    let mut u = lean_ground_with_puff(0.07, None, 1.0);
    u.mix = Vec3::new(0.5, 0.5, 0.0).extend(0.0);
    let dir = c1_dir(5.0, 60.0);
    let rel = dir * 180_000.0;
    let width = 900.0;
    let untiled = cloud_place_width(rel, dir, width, &u);
    let whole = (width * place_width(rel, dir, width, u.reach.y)).sqrt();
    assert_eq!(untiled, whole);
    for side in [2.0, 3.0] {
        let mut tiled = u;
        tiled.dshape.w = 1.0 / side;
        let got = cloud_place_width(rel, dir, width, &tiled);
        let reconstructed = cloud_place_width(rel, dir, width / side, &u);
        assert!((got - reconstructed).abs() < 0.001 * reconstructed);
    }
    for (sheet, amount) in [(0.0, 0.29), (1.0, 0.07), (0.5, 0.73)] {
        u.scale.w = amount;
        u.mix = Vec3::new(sheet, 1.0 - sheet, 0.0).extend(0.0);
        u.dshape.w = 1.0;
        let baseline = cloud_place_width(rel, dir, width, &u);
        u.dshape.w = 1.0 / 3.0;
        assert_eq!(cloud_place_width(rel, dir, width, &u), baseline);
    }
}

#[test]
fn overhead_mixed_banks_and_sunlight_taps_keep_their_original_material_mix() {
    use crate::volumetric_clouds_model2 as m2;
    let mut u = lean_ground_with_puff(0.45, None, 1.0);
    u.mix = Vec3::new(0.5, 0.5, 0.0).extend(0.0);
    u.dshape.w = 1.0 / 3.0;
    assert!((m2::mixed_sheet(&u) - 0.5).abs() < 1e-5);
    let width = 900.0;
    let cam_sample = m2::Sample { width, fw: width, eroded: true, ..Default::default() };
    let sun_tap = m2::Sample { eroded: false, ..cam_sample };
    let overhead = cloud_place_width(Vec3::Y * 20_000.0, Vec3::Y, width, &u);
    let dir = c1_dir(5.0, 60.0);
    let grazing = cloud_place_width(dir * 180_000.0, dir, width, &u);
    assert!(overhead > 1.5 * width * u.dshape.w && grazing > 2.0 * overhead);
    let at = |s: &m2::Sample, pwidth, blend| m2::mixed_material((1.0, 0.0), (1.0, 1.0), 0.5, blend, s, pwidth, &u).1;
    let blend = 0.35;
    assert_eq!(at(&cam_sample, overhead, blend), blend);
    assert!(at(&cam_sample, grazing, blend) > blend + 0.1);
    assert_eq!(at(&sun_tap, grazing, blend), blend, "near sunlight inherits the camera's projected footprint");
    assert_eq!(at(&sun_tap, width, blend), blend, "a square world-light cell");
    assert_eq!(at(&cam_sample, grazing, 0.0), 0.0);
    assert!(at(&cam_sample, grazing, 1e-5) < 2e-5);
    let bank = (0.80, 0.40);
    let dry_puff = (0.01, 0.85);
    for s in [&cam_sample, &sun_tap] {
        for share in [0.0, 1e-5] {
            let mixed = m2::mixed_material(bank, dry_puff, 0.05, share, s, grazing, &u);
            assert!((mixed.0 - bank.0).abs() < 2e-5 && (mixed.1 - bank.1).abs() < 2e-5, "blend {share}: {mixed:?}");
        }
    }
}

/// Signed section and upper edge of the four contributing ellipsoids (march `vc_m2_lobe_field`).
fn lean_lobe_field(uv: Vec2, h: f32, period: u32, lift: f32, shift: Vec2) -> Vec2 {
    let p = (uv - uv.floor()) * period as f32 + shift;
    let tile = p.floor().as_uvec2();
    let local = p - p.floor();
    let mut surface = -2.0f32;
    let mut roof = 0.0f32;
    for k in 0..4 {
        let corner = UVec2::new(k & 1, k >> 1);
        let id = (tile + corner) & UVec2::splat(period - 1);
        let bits = (m_hash(id.x, 91 + period, id.y) * 16777216.0) as u32;
        let rnd = Vec3::new((bits & 255) as f32, ((bits >> 8) & 255) as f32, (bits >> 16) as f32) / 255.0;
        let offset = local - corner.as_vec2();
        let reach = 1.0 - smoothstep(0.82, 1.0, offset.abs().max_element());
        let delta = offset - (rnd.truncate() - Vec2::splat(0.5)) * 0.26;
        let radius = 0.54 + 0.31 * rnd.z;
        let lateral = delta.length_squared() / (radius * radius);
        let half_height = lift * (0.23 + 0.47 * rnd.z);
        let centre = 0.14 + half_height + 0.10 * rnd.y;
        let vertical = (h - centre) / half_height;
        let field = half_height * (1.0 - (lateral + vertical * vertical).sqrt());
        surface = surface.max(-2.0 + reach * (field + 2.0));
        let top = centre + half_height * (1.0 - lateral).max(0.0).sqrt();
        roof = roof.max(reach * top * (1.0 - smoothstep(0.82, 1.04, lateral)));
    }
    Vec2::new(surface, roof)
}

#[test]
fn displaced_lobes_join_across_tile_edges_and_fade_with_the_weather() {
    for (period, lift, shift) in [(8, 0.70, Vec2::new(0.23, 0.47)), (32, 0.44, Vec2::new(0.61, 0.29))] {
        for k in 0..period {
            for step in 0..32 {
                let uv = Vec2::new((k as f32 - shift.x) / period as f32, step as f32 / 32.0);
                for height in [0.15, 0.5, 0.95, 1.35] {
                    let at = |p| lean_lobe_field(p, height, period, lift, shift);
                    assert!((at(uv + Vec2::X * 0.000001) - at(uv - Vec2::X * 0.000001)).abs().max_element() < 0.002);
                    assert!((at(uv + Vec2::new(1.0, -1.0)) - at(uv)).abs().max_element() < 0.0001);
                }
            }
        }
    }
    let weights = [0.3, 0.55, 0.60, 0.65, 0.70, 0.72, 0.9].map(|a| {
        let u = lean_ground_with_puff(a, None, 1.0);
        assert_eq!(u.field.x, lean_ground_with_puff(0.3, None, 1.0).field.x, "placement stays anchored through weather changes");
        crate::volumetric_clouds_model2::lobe_weight(Vec3::Y, &u)
    });
    assert_eq!((weights[0], weights[1], weights[5], weights[6]), (1.0, 1.0, 0.0, 0.0));
    assert!(weights[1..6].windows(2).all(|w| w[0] > w[1]));
}

#[test]
fn fair_material_resolves_the_reconstructed_texel_across_the_view_ray() {
    use crate::volumetric_clouds_model2 as m2;
    let untiled = lean_ground_with_puff(0.3, None, 1.0);
    let centres = lean_clouds_in(0.3, None, 12, &untiled);
    for side in [2.0, 3.0] {
        let mut tiled = untiled;
        tiled.dshape.w = 1.0 / side;
        let mut changed = false;
        for &xz in &centres {
            let col = lean_column_at(xz, 0.3, None, 400.0, &untiled);
            let g = m2::geometry(&col, &untiled);
            for k in 1..20 {
                let above = g.parent * k as f32 / 20.0;
                let sample = |u, width, eroded| lean_lobes(xz, &col, &g, &m2::Sample {
                    above, width, fw: 900.0, eroded, detail: true, wisp_g: 0.0,
                }, u);
                let view = sample(&tiled, 600.0, true);
                assert_eq!(view, sample(&untiled, 600.0 / side, true));
                assert_eq!(sample(&tiled, 600.0, false), sample(&untiled, 600.0, false));
                changed |= (view.0 - sample(&untiled, 600.0, true).0).abs() > 0.001;
            }
        }
        assert!(changed, "tile side {side} must recover material detail in the sampled clouds");
    }
}

/// Fair material against the production noise bytes, including its view/light footprint.
fn lean_lobes(xz: Vec2, col: &crate::volumetric_clouds_model2::Column, g: &crate::volumetric_clouds_model2::Geometry,
    s: &crate::volumetric_clouds_model2::Sample, u: &CloudVolumeUniform) -> (f32, f32) {
    use crate::cloud_noise::{level_for, production, BASE_REPEAT, BASE_SIZE, EROSION_REPEAT, EROSION_SIZE};
    let across = (s.width * u.dshape.w).max(1.0);
    let along = s.width.max(1.0).max(s.fw);
    let width = if s.eroded { (across * along).sqrt() } else { 2.0 * s.width.max(crate::volumetric_clouds_model2::light_cell(u)) };
    let xyz = Vec3::new(xz.x, s.above, xz.y);
    let uvw = xyz * u.nscale.x + Vec3::new(u.nphase.x, 0.0, u.nphase.y);
    let noise = production();
    let base = |coords: Vec3, freq| {
        let lod = level_for(width, BASE_SIZE, BASE_REPEAT / freq, 8);
        crate::volumetric_clouds_model2::spread(noise.base.sample(coords.to_array(), lod)[0], u) - 0.5
    };
    let warp = base(uvw + Vec3::new(0.17, 0.42, 0.21), 1.0);
    let uv = Vec2::new(uvw.x, uvw.z) + Vec2::new(0.014, -0.017) * warp;
    let profile = crate::volumetric_clouds_model2::fair_profile(col, g, u);
    let bank = profile.bank;
    let foot = crate::volumetric_clouds_model2::FOOT * g.parent;
    let developed_above = s.above / profile.depth_scale;
    let uncompressed = crate::volumetric_clouds_model2::plume_height(developed_above, g.parent, 1.0 / profile.plume_scale);
    let y = crate::volumetric_clouds_model2::stretch(uncompressed, profile.crown, g.parent, foot);
    let coarse = base(2.0 * uvw + Vec3::new(0.17, 0.39, 0.68), 2.0);
    let coords = xyz * u.nscale.y + Vec3::new(u.nphase.z, 0.0, u.nphase.w);
    let raw = noise.erosion.sample(coords.to_array(), level_for(width, EROSION_SIZE, EROSION_REPEAT, 6));
    let erosion = Vec3::new(raw[0], raw[1], raw[2]) - u.nband.truncate();
    let fray = crate::volumetric_clouds_model2::FRAY * Vec3::new(erosion.y, erosion.z, erosion.x);
    let fine = base(6.0 * uvw + fray, 6.0);
    let raw_height = y / profile.height;
    let onset = crate::volumetric_clouds_model2::base_onset(col.base_humidity, 0.75 * coarse + 0.30 * fine + 0.20 * warp, u);
    let h = raw_height - (0.52 - raw_height).max(0.0) * onset / (0.52 - onset);
    let large = lean_lobe_field(uv, h, 8, 0.70, Vec2::new(0.23, 0.47));
    let exterior = 1.0 - smoothstep(0.0, 0.14, col.over);
    let bridge = 0.31 + 0.075 * warp - (h - 0.39 - 0.045 * warp).abs() - 0.25 * (1.0 - bank);
    let rim = base(13.0 * uvw + Vec3::new(0.61, 0.07, 0.33), 13.0);
    let finest = base(27.0 * uvw + Vec3::new(0.18, 0.52, 0.77), 27.0);
    let detail = 0.32 * fine + erosion.dot(Vec3::new(0.46, 0.21, 0.10)) + 0.20 * rim + 0.12 * finest;
    let shelf = bridge + 0.11 * coarse - 0.38 * exterior;
    let shoulder = large.y * smoothstep(0.025, 0.17, col.over);
    let tower = smoothstep(0.22, 0.71, shoulder);
    let small_top = shoulder + tower * (0.08 + 0.58 * smoothstep(-0.21, 0.25, coarse + 0.35 * fine));
    let cap = (h - (0.68 * shoulder).max(0.30)).min(small_top - h);
    let heads = (large.x + 0.42 * coarse).max(cap - 0.59 * (1.0 - tower)) + 0.16 * warp - 0.67 * exterior;
    let erosion_strength = (u.nscale.w / 1.26).clamp(0.0, 2.0);
    let field = shelf.max(heads) + erosion_strength * detail;
    let soft = (width / g.parent).clamp(0.14, 0.32);
    let roof = 0.75 + 0.44 * g.cover;
    let roof = roof + 0.45 * (large.y.max(small_top).max(0.5) - roof);
    let core = smoothstep(-soft, soft, field) * (0.30 + 0.70 * smoothstep(-0.06, 0.26, field));
    let margin = smoothstep(-0.42 - soft, -0.06, field);
    let fragments = smoothstep(-0.10, 0.34, 0.70 * fine + 0.40 * coarse + erosion.dot(Vec3::new(0.58, 0.32, 0.18)));
    let fringe_water = 0.065 + 0.075 * smoothstep(0.32, 0.72, h) + 0.035 * exterior;
    let density = core.max(lean_open_bank_weight(u) * fringe_water * margin * fragments * erosion_strength.min(1.0));
    let rim_width = 0.012 + (0.045 - 0.012) * crate::volumetric_clouds_model2::mixed_sheet(u);
    let boundary = (1.0 - smoothstep(0.90, 1.0, developed_above / g.extent)) * smoothstep(0.0, rim_width, col.over);
    (smoothstep(0.02, 0.13, h) * density * boundary, (h / roof).clamp(0.0, 1.0))
}

fn lean_material(xz: Vec2, col: &crate::volumetric_clouds_model2::Column, g: &crate::volumetric_clouds_model2::Geometry,
    s: &crate::volumetric_clouds_model2::Sample, pwidth: f32, u: &CloudVolumeUniform, reads: &mut crate::volumetric_clouds_model2::Reads) -> (f32, f32) {
    use crate::volumetric_clouds_model2 as m2;
    let blend = m2::lobe_weight(col.types, u);
    if blend <= 0.0 {
        return m2::density(g, col.types, s, u, reads);
    }
    if s.above >= g.extent || g.cover <= 0.0 {
        return (0.0, 0.0);
    }
    let puff = lean_lobes(xz, col, g, s, u);
    if blend >= 1.0 {
        return puff;
    }
    let bank = m2::density(g, col.types, s, u, reads);
    m2::mixed_material(bank, puff, g.cover, blend, s, pwidth, u)
}

/// The wind-advected water concentration within a resolved fair bank (march `vc_m2_shape`).
fn lean_water_content(xz: Vec2, above: f32, width: f32, col: &crate::volumetric_clouds_model2::Column, d: (f32, f32), u: &CloudVolumeUniform) -> f32 {
    use crate::cloud_noise::{level_for, BASE_REPEAT, BASE_SIZE};
    let lower_bank = crate::volumetric_clouds_model2::lobe_weight(col.types, u) * lean_open_bank_weight(u) * col.res
        * smoothstep(0.04, 0.20, col.over) * (1.0 - smoothstep(0.36, 0.76, d.1));
    if d.0 <= 0.0 || lower_bank <= 0.0 {
        return 1.0;
    }
    let uvw = Vec3::new(xz.x, above, xz.y) * u.nscale.x + Vec3::new(u.nphase.x, 0.0, u.nphase.y);
    let band = |frequency: f32, offset: Vec3| {
        let mip = level_for(width, BASE_SIZE, BASE_REPEAT / frequency, 8);
        let raw = crate::cloud_noise::production().base.sample((uvw * frequency + offset).to_array(), mip)[0];
        crate::volumetric_clouds_model2::spread(raw, u) - 0.5
    };
    let broad = band(3.0, Vec3::new(0.41, 0.37, 0.18));
    let local = band(6.0, Vec3::new(0.23, 0.65, 0.53));
    (lower_bank * (0.45 * broad + 0.72 * local)).exp2()
}

#[test]
fn water_content_varies_inside_fair_cloud_bases_and_wraps_with_wind() {
    let fair = lean_ground_with_puff(0.35, Some(CloudType::Cumulus), 1.0);
    let mut column = lean_column_at(Vec2::ZERO, 0.35, Some(CloudType::Cumulus), 0.0, &fair);
    column.over = 0.35;
    column.res = 1.0;
    let mut wrapped = fair;
    wrapped.nphase.x += 1.0;
    wrapped.nphase.y -= 1.0;
    let storm = lean_ground_with_puff(0.8, Some(CloudType::Cumulus), 1.0);
    let mut range = (f32::MAX, f32::MIN);
    for i in 0..24 {
        let xz = Vec2::new(673.0, -437.0) * i as f32;
        let factor = |height, uniform| lean_water_content(xz, 500.0, 80.0, &column, (1.0, height), uniform);
        let low = factor(0.2, &fair);
        range = (range.0.min(low), range.1.max(low));
        assert!((low - factor(0.2, &wrapped)).abs() < 1e-4);
        assert_eq!(factor(0.85, &fair), 1.0);
        assert_eq!(factor(0.2, &storm), 1.0);
    }
    assert!(range.0 < 0.95 && range.1 > 1.05, "water concentration range {range:?}");
}

/// Read the column material's base texture at the requested frequency and its own mip.
fn lean_base_reader(xz: Vec2, above: f32, u: &CloudVolumeUniform) -> impl Fn(f32, f32, Vec3) -> f32 + '_ {
    use crate::cloud_noise::{level_for, production, BASE_SIZE};
    let uv = Vec3::new(xz.x, above, xz.y) * u.nscale.x + Vec3::new(u.nphase.x, 0.0, u.nphase.y);
    let texture = &production().base;
    move |width: f32, frequency: f32, offset: Vec3| {
        let mip = level_for(width, BASE_SIZE, (u.nscale.x * frequency).recip(), 8);
        texture.sample((uv * frequency + offset).to_array(), mip)[0]
    }
}

#[test]
fn the_deck_detail_samples_its_own_mip_and_wraps_with_the_wind() {
    use crate::cloud_noise::{production, BASE_REPEAT, BASE_SIZE};
    let mut u = lean_ground_with_puff(0.73, None, 1.0);
    u.nphase.x = 0.25;
    u.nphase.y = 0.375;
    let mut wrapped = u;
    wrapped.nphase.x -= 1.0;
    wrapped.nphase.y += 1.0;
    let xz = Vec2::new(26_000.0, -17_000.0);
    let above = 385.0;
    let offset = Vec3::new(0.44, 0.37, 0.192);
    let read = lean_base_reader(xz, above, &u);
    let wrapped_read = lean_base_reader(xz, above, &wrapped);
    let uv = Vec3::new(xz.x, above, xz.y) / BASE_REPEAT + Vec3::new(u.nphase.x, 0.0, u.nphase.y);
    for lod in [1.5_f32, 3.5] {
        let width = BASE_REPEAT / (3.0 * BASE_SIZE as f32) * lod.exp2();
        let expected = production().base.sample((3.0 * uv + offset).to_array(), lod)[0];
        assert!((read(width, 3.0, offset) - expected).abs() < 1e-5);
        assert!((wrapped_read(width, 3.0, offset) - expected).abs() < 1e-5);
    }
}

/// The lean field at world (x, z) `xz`, bent height `yb`, cast width `width`, placement footprint `pw` and
/// the view's step filter `fw` (march `vc_m2_support` and `vc_shape`, no drift): (the
/// support, (the optical density per unit, the shape)); `eroded` false is the light's read (no wisps,
/// its own filter). The view's erosion read is displaced and stretched by the wisps as the march does.
#[allow(clippy::too_many_arguments)]
fn lean_at(xz: Vec2, yb: f32, (width, pw, fw): (f32, f32, f32), a: f32, forced: Option<CloudType>, u: &CloudVolumeUniform, eroded: bool) -> (bool, (f32, f32)) {
    use crate::cloud_noise::{level_for, EROSION_REPEAT, EROSION_SIZE};
    use crate::volumetric_clouds_model2 as m2;
    if yb <= u.slab.x || yb >= u.slab.x + u.slab.y {
        return (false, (0.0, 0.0));
    }
    let col = lean_column_at(xz, a, forced, pw, u);
    let above = yb - u.slab.x;
    // Leave the density oracle on the full physical domain. Only the support
    // takes the earlier height rejection, so any lost density fails the test.
    let support = yb < m2::sampling_ceiling(u) && m2::support(&col, above, u);
    if col.over <= 0.0 && col.res >= 1.0 {
        return (support, (0.0, 0.0));
    }
    let mut d = (0.0, 0.0);
    if col.over > 0.0 {
        let g = m2::geometry(&col, u);
        let noise = crate::cloud_noise::production();
        let pos = Vec3::new(xz.x, yb, xz.y);
        let (mut epos, mut wisp_g) = (pos, 0.0);
        if eroded && u.dshape.z > 0.0 {
            let uw = Vec2::new(pos.x * u.nscale.x + u.nphase.x, pos.z * u.nscale.x + u.nphase.y);
            let wp = crate::volumetric_clouds_wisp::wisp(uw, above, u.nscale.x, u.dshape.z);
            epos = pos + Vec3::new(wp.d.x, 0.0, wp.d.y);
            wisp_g = wp.g;
        }
        let mut base = lean_base_reader(xz, above, u);
        let mut erode = |w: f32| {
            let c = m2::erode_coord(epos, yb, u);
            let e = noise.erosion.sample([c.x, c.y, c.z], level_for(w, EROSION_SIZE, EROSION_REPEAT / m2::ERODE_FREQ, 6));
            [e[0], e[1], e[2]]
        };
        let s = m2::Sample { above, width, fw, eroded, detail: true, wisp_g };
        d = lean_material(xz, &col, &g, &s, pw, u, &mut m2::Reads { base: &mut base, erode: &mut erode });
    }
    let water_content = lean_water_content(xz, above, width, &col, d, u);
    let (shape, k, _) = m2::shape(&col, above, d, water_content, u);
    if shape <= 0.0 {
        return (support, (0.0, 0.0));
    }
    (support, (shape * k, shape))
}

/// The lean light's optical density per unit at `q` from a camera on the ground reference at world (x,
/// z) `off`, at the cast and placement footprints `(w, pw)` (march `vc_shape(.., false, v)`).
fn lean_light(off: Vec2, q: Vec3, a: f32, u: &CloudVolumeUniform, (w, pw): (f32, f32)) -> f32 {
    lean_at(off + Vec2::new(q.x, q.z), bent_y(q, 0.0, u.reach.y), (w, pw, w), a, None, u, false).1 .0
}

/// The lean field along a ray from a camera on the ground reference at world (x, z) `off`: the support
/// and the density at `t` for the step `dt` the march takes there (the view's filter, march
/// `vc_detail_filter`: the wider of the cast width and the step).
#[allow(clippy::type_complexity)]
fn lean_field(off: Vec2, dir: Vec3, spread: f32, a: f32, forced: Option<CloudType>, u: CloudVolumeUniform) -> (impl Fn(f32) -> bool, impl Fn(f32) -> (f32, f32), impl Fn(f32, f32) -> (f32, f32)) {
    let at = move |t: f32, dt: f32| {
        let rel = dir * t;
        let width = spread * t;
        lean_at(off + Vec2::new(rel.x, rel.z), bent_y(rel, 0.0, u.reach.y), (width, cloud_place_width(rel, dir, width, &u), width.max(dt)), a, forced, &u, true)
    };
    (move |t: f32| at(t, 0.0).0, move |t: f32| at(t, 0.0).1, move |t: f32, dt: f32| at(t, dt).1)
}

/// Where on the ground a camera must stand for a ray at elevation `el` (degrees) and azimuth `az` to
/// meet the bent height `above` over the base at world (x, z) `target` (the ray's first crossing).
fn lean_aim(target: Vec2, el: f32, az: f32, above: f32, u: &CloudVolumeUniform) -> Vec2 {
    let d = c1_dir(el, az);
    let a = (d.x * d.x + d.z * d.z) / (2.0 * u.reach.y.max(1.0));
    let t = roots(a, d.y, -(u.slab.x + above)).map_or(0.0, |(lo, hi)| if lo > 0.0 { lo } else { hi });
    target - Vec2::new(d.x, d.z) * t
}

/// Places where the lean field holds cloud at the amount `a` (resolved columns with cover past a half),
/// searched on a lattice from a fixed corner: the thin weather's few clouds, so its rays meet some.
fn lean_clouds(a: f32, forced: Option<CloudType>, n: usize) -> Vec<Vec2> {
    let u = lean_ground(a, forced);
    lean_clouds_in(a, forced, n, &u)
}

fn lean_clouds_in(a: f32, forced: Option<CloudType>, n: usize, u: &CloudVolumeUniform) -> Vec<Vec2> {
    let mut out = Vec::new();
    'search: for i in 0..400 {
        for j in 0..400 {
            let xz = Vec2::new(-4_000_000.0 + 20_000.0 * i as f32, 3_000_000.0 - 20_000.0 * j as f32);
            if crate::volumetric_clouds_model2::cover(lean_column_at(xz, a, forced, 0.0, u).over) > 0.5 {
                out.push(xz);
                if out.len() >= n {
                    break 'search;
                }
            }
        }
    }
    out
}

#[test]
fn only_fully_lobed_cumulus_rejects_the_air_above_its_tallest_bank() {
    use crate::volumetric_clouds_model2 as m2;
    for (amount, forced, puff, bounded) in [
        (0.30, Some(CloudType::Cumulus), 1.0, true), (0.55, Some(CloudType::Cumulus), 1.0, true),
        (0.30, Some(CloudType::Cumulus), 0.99, false), (0.30, Some(CloudType::Tower), 1.0, false),
        (0.65, Some(CloudType::Cumulus), 1.0, false), (0.8, None, 1.0, false),
    ] {
        let u = lean_ground_with_puff(amount, forced, puff);
        let ceiling = m2::sampling_ceiling(&u);
        if bounded {
            let peak = m2::Column { head: 1.55, over: 1.0, types: Vec3::Y, vigour: 1.0, res: 1.0, ..Default::default() };
            let tallest = u.slab.x + m2::extent(&peak, &u);
            assert!(ceiling >= tallest && ceiling < tallest + 2.0);
            assert!(ceiling < u.slab.x + 0.5 * u.slab.y);
        } else {
            assert_eq!(ceiling, u.slab.x + u.slab.y);
        }
    }
}

/// The refined march's stretches end at the sampling ceiling for a camera under it (march
/// `vc_refined_spans`, CPU `lean_spans`). In fair weather from the ground: each stretch keeps its start and ends no later, and every
/// cell the unclipped march integrates where the medium holds support everywhere, past that end, has
/// neither the column's support (asked without the ceiling's own rejection) nor a density. Where the
/// ceiling is the domain's top, and from a camera at or above the ceiling (between it and the top, over the
/// top), every stretch is the unclipped one to the bit. The plan: from any height, whatever the budget, no
/// ray's growth rises (so an uncapped ray stays at 1).
#[test]
fn the_stretch_past_the_sampling_ceiling_holds_nothing() {
    use crate::volumetric_clouds_model2 as m2;
    let spread = c1_spread();
    let end = crate::medium::RAY_END;
    let mut past = 0usize;
    for (a, place) in [(0.3f32, Vec2::ZERO), (0.55, Vec2::new(123_456.0, -654_321.0))] {
        let forced = Some(CloudType::Cumulus);
        let u = lean_ground_with_puff(a, forced, 1.0);
        let ceiling = m2::sampling_ceiling(&u);
        assert!(ceiling < u.slab.x + u.slab.y);
        for el in [2.0f32, 10.0, 45.0, 90.0] {
            for az in [20.0f32, 200.0] {
                let d = c1_dir(el, az);
                let (whole, cut) = (spans(Vec3::ZERO, d, 0.0, end, &u), lean_spans(Vec3::ZERO, d, 0.0, end, &u));
                for (w, c) in whole.iter().zip(&cut) {
                    if w.1 > w.0 {
                        assert!(c.0.to_bits() == w.0.to_bits() && c.1 > c.0 && c.1 < w.1, "{el} degrees: {w:?} cut to {c:?}");
                    } else {
                        assert!(c.1 <= c.0, "{el} degrees: {w:?} cut to {c:?}");
                    }
                }
                let last = cut.iter().filter(|c| c.1 > c.0).map(|c| c.1).fold(0.0f32, f32::max);
                let (support, density) = (|_: f32| true, |_: f32| (0.0f32, 0.0f32));
                let run = refined_march(Vec3::ZERO, d, spread, (0.0, end), 0.5, &u, &RayField { support: &support, density: &density, stepped: None });
                for &(lo, hi) in &run.cells {
                    for k in 0..4 {
                        let t = lo + (hi - lo) * k as f32 / 3.0;
                        if t < last {
                            continue;
                        }
                        past += 1;
                        let rel = d * t;
                        let (yb, pw) = (bent_y(rel, 0.0, u.reach.y), cloud_place_width(rel, d, spread * t, &u));
                        let xz = place + Vec2::new(rel.x, rel.z);
                        let col = lean_column_at(xz, a, forced, pw, &u);
                        assert!(!m2::support(&col, yb - u.slab.x, &u), "support at {t}, height {yb}, over the ceiling {ceiling}");
                        assert_eq!(lean_at(xz, yb, (spread * t, pw, spread * t), a, forced, &u, true).1 .1, 0.0, "density at {t}, height {yb}, over the ceiling {ceiling}");
                    }
                }
            }
        }
    }
    assert!(past > 1_000, "the unclipped marches go on past the ceiling: {past} points");
    let full = lean_ground_with_puff(0.8, None, 1.0);
    assert_eq!(m2::sampling_ceiling(&full), full.slab.x + full.slab.y);
    let fair = lean_ground_with_puff(0.3, Some(CloudType::Cumulus), 1.0);
    let ceiling = m2::sampling_ceiling(&fair);
    let (mut capped, mut shorter) = (0usize, 0usize);
    for height in [0.0f32, 19_999.0, 25_000.0, ceiling - 1.0, ceiling, ceiling + 1.0, 55_000.0, 59_999.0, 70_000.0, 150_000.0] {
        for i in 0..721 {
            let el = (-90.0 + 0.25 * i as f32).to_radians();
            let d = Vec3::new(el.cos(), el.sin(), 0.0);
            let o = Vec3::new(0.0, height, 0.0);
            let bits = |s: [(f32, f32); 2]| s.map(|(a, b)| (a.to_bits(), b.to_bits()));
            assert_eq!(bits(lean_spans(o, d, 0.0, end, &full)), bits(spans(o, d, 0.0, end, &full)), "the ceiling is the top: {o} {d}");
            for budget in [RAY_BUDGET as f32, 1_000.0] {
                let mut u = fair;
                u.reach.w = budget;
                let (whole, cut) = (spans(o, d, 0.0, end, &u), lean_spans(o, d, 0.0, end, &u));
                if height >= ceiling {
                    assert_eq!(bits(cut), bits(whole), "a camera above the ceiling: {o} {d}");
                }
                let ((grow, cap), (cut_grow, cut_cap)) = (refined_plan(whole, spread, &u), refined_plan(cut, spread, &u));
                assert!(cut_grow <= grow && (cap || (grow == 1.0 && cut_grow == 1.0 && !cut_cap)), "{o} {d}: growth {grow} to {cut_grow}");
                capped += usize::from(cap);
                shorter += usize::from(cut_grow < grow);
            }
        }
    }
    // The probe behind the capped line (`refined_rays_capped`) plans the stretches the shader marches
    // (`vc_trace_refined` through `vc_refined_spans`): under the ceiling there are budgets that cap a
    // whole-domain stretch and no ray drawn.
    let march = include_str!("volumetric_clouds_march.wesl").replace("\r\n", "\n");
    assert!(march.contains("    let spans = vc_refined_spans(ray, v);\n") && march.contains("    let most = max(floor((v.reach.w - 2.0) / (2.0 + taps)) - 4.0, 1.0);\n"));
    let o = Vec3::new(0.0, 25_000.0, 0.0);
    let mut apart = 0usize;
    for k in 1..=200 {
        let mut u = fair;
        u.reach.w = 500.0 * k as f32;
        let any = |of: &dyn Fn(Vec3) -> [(f32, f32); 2]| {
            (0..=1_800).any(|i| {
                let (s, c) = (-90.0 + i as f32 * CAP_PROBE_DEG).to_radians().sin_cos();
                refined_plan(of(Vec3::new(c, s, 0.0)), spread, &u).1
            })
        };
        let (drawn, domain) = (any(&|d| lean_spans(o, d, 0.0, end, &u)), any(&|d| spans(o, d, 0.0, end, &u)));
        assert_eq!(refined_rays_capped(o, spread, &u), drawn, "budget {}", u.reach.w);
        assert!(domain || !drawn);
        apart += usize::from(domain && !drawn);
    }
    assert!(apart > 0, "the two tell some budget apart");
    println!("{past} points past the ceiling; {capped} capped plans, {shorter} of them grow less");
    assert!(capped > 100 && shorter > 10);
}

/// The lean support holds every density on the real field (conservative): columns 3,000 units apart,
/// heights every 250 units through the domain, amounts 0.05, 0.3 and 0.8, the local types and a tower
/// forced, footprints that resolve the placement, fade its octaves and leave it unresolved, step filters
/// of the cast width and of 2,000 units, the view's (wisps displacing the erosion read) and the light's
/// reads; no point outside the support has a density, and some points have one.
#[test]
fn the_lean_support_holds_every_density_on_the_real_field() {
    for (a, forced, puff) in [(0.05f32, None, 0.0), (0.3, None, 0.0), (0.8, None, 0.0), (0.8, Some(CloudType::Tower), 0.0), (0.3, None, 1.0), (0.62, None, 1.0)] {
        let u = lean_ground_with_puff(a, forced, puff);
        let mut total = 0usize;
        for pw in [0.0f32, 30_000.0, 90_000.0, 150_000.0] {
            let (mut dense, mut held, mut points) = (0usize, 0usize, 0usize);
            for i in 0..24 {
                for j in 0..24 {
                    let xz = Vec2::new(-61_000.0 + 3_000.0 * i as f32, 44_000.0 + 3_000.0 * j as f32);
                    for k in 0..160 {
                        let yb = u.slab.x + 125.0 + 250.0 * k as f32;
                        for (fw, eroded) in [(64.0f32, true), (2_000.0, true), (64.0, false)] {
                            let (support, (sigma, shape)) = lean_at(xz, yb, (64.0, pw, fw), a, forced, &u, eroded);
                            points += 1;
                            held += usize::from(support);
                            if shape > 0.0 {
                                dense += 1;
                                assert!(sigma.is_finite() && sigma > 0.0);
                                assert!(support, "amount {a} {forced:?} footprint {pw} filter {fw} eroded {eroded}: density {sigma} at {xz} height {yb} outside the support");
                            }
                        }
                    }
                }
            }
            total += dense;
            println!("lean: amount {a} {forced:?} footprint {pw}: {points} points, {dense} with density, support {held}");
        }
        assert!(total > 0, "amount {a} {forced:?}: no density anywhere in the sampled area");
    }
}

/// Fair weather's masses are wider than tall (PUFF 1, as a run draws them): the columns over the threshold
/// labelled into masses on a grid of a fortieth of the domain, each mass's equivalent width over its tallest
/// column's height (the fair profile's crown under its depth limit, `fair_profile`; the growing plumes only
/// lower it), for the masses wider than 0.15 of the domain. At the run's 24,000 and at 40,000, amounts 0.15
/// and 0.29, and at 64,000 too: at least 20 masses; the median width over height within a fifth of what a
/// numeric copy of the field gives (`FAIR_ASPECT`), so neither stretched crowns nor flattened ones pass; none
/// under 1.5 (the copy: 2.05 at the least, whatever the moisture). One cloudscape at three scales: the median
/// width grows with the domain (within a tenth; the copy: exactly), the height with it but less (the depth
/// limit's fixed part; the copy: 1.51 for 1.67 and for 1.6), the median at most a fifth thinner (prints the figures).
#[test]
fn the_fair_masses_are_wider_than_tall() {
    use crate::volumetric_clouds_model2 as m2;
    // The copy's median width over height, per domain, at amounts 0.15 and 0.29 [measured: a numeric copy of
    // the placement, the types, the vigour and the fair profile on this test's grid].
    const FAIR_ASPECT: [(f32, [f32; 2]); 3] = [(24_000.0, [6.48, 6.78]), (40_000.0, [7.18, 7.57]), (64_000.0, [7.96, 8.10])];
    const N: usize = 300;
    let stats = |domain: f32, amount: f32| -> (usize, f32, f32, f32, f32) {
        let u = uniform(&VolumeInputs { plane_height: 20_000.0, above_ground: 20_000.0, domain, density: 1.0, amount, puff: 1.0, ..Default::default() });
        let step = domain / 40.0;
        let height: Vec<f32> = (0..N * N)
            .map(|k| {
                let xz = Vec2::new((k / N) as f32, (k % N) as f32) * step + Vec2::splat(domain * 3.3);
                let col = lean_column_at(xz, amount, None, 0.0, &u);
                if col.over <= 0.0 {
                    return 0.0;
                }
                let p = m2::fair_profile(&col, &m2::geometry(&col, &u), &u);
                p.crown * p.depth_scale
            })
            .collect();
        let mut label = vec![false; N * N];
        let mut masses: Vec<(f32, f32)> = Vec::new();
        for start in 0..N * N {
            if height[start] <= 0.0 || label[start] {
                continue;
            }
            let (mut count, mut tallest, mut edge) = (0usize, 0.0f32, false);
            let mut stack = vec![start];
            label[start] = true;
            while let Some(k) = stack.pop() {
                count += 1;
                tallest = tallest.max(height[k]);
                let (i, j) = (k / N, k % N);
                edge |= i == 0 || j == 0 || i == N - 1 || j == N - 1;
                for (ok, q) in [(i > 0, k.wrapping_sub(N)), (i + 1 < N, k + N), (j > 0, k.wrapping_sub(1)), (j + 1 < N, k + 1)] {
                    if ok && height[q] > 0.0 && !label[q] {
                        label[q] = true;
                        stack.push(q);
                    }
                }
            }
            // A mass cut by the grid's edge is left out (its width is not known).
            let width = 2.0 * (count as f32 * step * step / std::f32::consts::PI).sqrt();
            if !edge && width > 0.15 * domain {
                masses.push((width, tallest));
            }
        }
        let median = |mut v: Vec<f32>| -> f32 {
            v.sort_by(f32::total_cmp);
            v.get(v.len() / 2).copied().unwrap_or(0.0)
        };
        let ratios: Vec<f32> = masses.iter().map(|m| m.0 / m.1).collect();
        let least = ratios.iter().copied().fold(f32::MAX, f32::min);
        let out = (masses.len(), median(masses.iter().map(|m| m.0).collect()), median(masses.iter().map(|m| m.1).collect()), median(ratios), least);
        println!("domain {domain}, amount {amount}: {} masses, median width {:.0}, height {:.0}, width over height median {:.2}, least {:.2}", out.0, out.1, out.2, out.3, out.4);
        out
    };
    for (k, amount) in [0.15f32, 0.29].into_iter().enumerate() {
        let figures = FAIR_ASPECT.map(|(domain, aspect)| (domain, aspect[k], stats(domain, amount)));
        for (domain, aspect, (count, _, _, ratio, least)) in figures {
            assert!(count >= 20, "domain {domain}, amount {amount}: {count} masses");
            assert!((ratio / aspect - 1.0).abs() <= 0.2, "domain {domain}, amount {amount}: width over height median {ratio} against {aspect}");
            assert!(least >= 1.5, "domain {domain}, amount {amount}: the steepest mass {least}");
        }
        for pair in figures.windows(2) {
            let ((low, _, a), (tall, _, b)) = (pair[0], pair[1]);
            let (scale, wider, taller) = (tall / low, b.1 / a.1, b.2 / a.2);
            assert!((wider / scale - 1.0).abs() < 0.1, "amount {amount}: broader with the domain, {wider} for {scale}");
            assert!(taller > 1.25 && taller < 1.05 * scale, "amount {amount}: taller with the domain, {taller} for {scale}");
            assert!(b.3 >= 0.8 * a.3, "amount {amount}: at most a fifth thinner, {} against {}", b.3, a.3);
        }
    }
}

/// The refined march against the dense reference on the lean field (prints): grazing rays (1 and 2
/// degrees, amount 0.3), thin weather (0.05; 10, 45, 90 degrees, each ray aimed at one of the weather's few
/// clouds so the case measures something), overcast (0.8; 10, 90) and towers (forced, 0.8; 10, 90), from
/// four places, four azimuths, start offsets 0.02, 0.5, 0.98; each march's medium filtered by the step it
/// takes (the release's), the wisps on, and the refined march also with the release's stride (4: interior
/// cells merged). The tolerances come
/// from the dense march's own variation (three times the larger of its half-step and phase differences,
/// mean and 95th percentile, capped at 0.01 and 0.03 of opacity), misses at most 6 % of the pairs; at least
/// three visible rays a case; every refined cell inside a stretch, every ray within the budget.
#[test]
fn the_lean_refined_march_matches_the_dense_reference() {
    let spread = c1_spread();
    let offsets = [0.02f32, 0.5, 0.98];
    let pct95 = |v: &mut [f32]| -> f32 {
        v.sort_by(f32::total_cmp);
        v[((v.len() as f32 * 0.95) as usize).min(v.len().max(1) - 1)]
    };
    let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len().max(1) as f32;
    let places = [Vec2::ZERO, Vec2::new(123_456.0, -654_321.0), Vec2::new(-777_000.0, 333_000.0), Vec2::new(2_000_000.0, 1_500_000.0)];
    let thin_clouds = lean_clouds(0.05, None, 4);
    assert!(thin_clouds.len() >= 4, "the thin weather holds clouds to aim at: {thin_clouds:?}");
    type Case = (&'static str, f32, Option<CloudType>, &'static [f32], bool, f32);
    let cases: [Case; 8] = [
        ("grazing", 0.3, None, &[1.0, 2.0], false, 0.0), ("thin", 0.05, None, &[10.0, 45.0, 90.0], true, 0.0),
        ("overcast", 0.8, None, &[10.0, 90.0], false, 0.0), ("tower", 0.8, Some(CloudType::Tower), &[10.0, 90.0], false, 0.0),
        ("fair lobes", 0.3, None, &[1.0, 10.0, 45.0, 90.0], false, 1.0),
        ("humid patches", 0.40, None, &[2.0, 45.0], false, 1.0), ("filling lanes", 0.46, None, &[2.0, 45.0], false, 1.0), ("closing deck", 0.62, None, &[2.0, 45.0], false, 1.0),
    ];
    let mut failures: Vec<String> = Vec::new();
    for stride in [0.0f32, 4.0] {
        for (name, a, forced, els, aimed, puff) in cases {
            let mut u = lean_ground_with_puff(a, forced, puff);
            u.families.x = stride;
            let (mut half, mut phase, mut refined) = (Vec::new(), Vec::new(), Vec::new());
            let (mut misses, mut visible, mut pairs) = (0usize, 0usize, 0usize);
            for &el in els {
                let (mut steps, mut capped, mut rays, mut probes, mut fine) = (0u32, 0usize, 0usize, 0u32, 0u32);
                for (pi, place) in places.iter().enumerate() {
                    for k in 0..4 {
                        if el == 90.0 && k > 0 {
                            continue;
                        }
                        let az = k as f32 * 90.0 + 20.0;
                        // The thin weather: the camera placed so the ray meets a cloud 2,000 units over the base.
                        let off = if aimed { lean_aim(thin_clouds[(pi + k) % thin_clouds.len()], el, az, 2_000.0, &u) } else { *place };
                        let d = c1_dir(el, az);
                        let span = (0.0, crate::medium::RAY_END);
                        let stretches = spans(Vec3::ZERO, d, span.0, span.1, &u);
                        let (plan, cap) = ray_plan(stretches, spread, &u);
                        steps += plan[0] + plan[1];
                        capped += usize::from(cap || refined_plan(stretches, spread, &u).1);
                        rays += 1;
                        let (support, density, stepped) = lean_field(off, d, spread, a, forced, u);
                        let stepped: &dyn Fn(f32, f32) -> (f32, f32) = &stepped;
                        let field = RayField { support: &support, density: &density, stepped: Some(stepped) };
                        let dense: Vec<MarchRun> = offsets.iter().map(|&o| m_dense(Vec3::ZERO, d, spread, span, o, 1, &u, &field)).collect();
                        let halved: Vec<MarchRun> = offsets.iter().map(|&o| m_dense(Vec3::ZERO, d, spread, span, o, 2, &u, &field)).collect();
                        let runs: Vec<MarchRun> = offsets.iter().map(|&o| refined_march(Vec3::ZERO, d, spread, span, o, &u, &field)).collect();
                        for r in &runs {
                            c1_check_cells(r, stretches, 0.0);
                            assert!(r.evals() <= RAY_BUDGET);
                        }
                        probes += runs[1].probes;
                        fine += runs[1].fine;
                        if dense.iter().all(|m| m.opacity() < 0.02) {
                            continue;
                        }
                        visible += 1;
                        for i in 0..3 {
                            pairs += 1;
                            half.push((dense[i].opacity() - halved[i].opacity()).abs());
                            phase.push((dense[i].opacity() - dense[(i + 1) % 3].opacity()).abs());
                            refined.push((runs[i].opacity() - dense[i].opacity()).abs());
                            if dense[i].opacity() > 0.02 && runs[i].opacity() < 0.25 * dense[i].opacity() {
                                misses += 1;
                            }
                        }
                    }
                }
                let n = rays.max(1) as f32;
                println!("lean {name} (stride {stride}), {el} degrees: dense steps per ray {:.0}; refined {:.0} probes + {:.0} fine; {capped} of {rays} rays capped", steps as f32 / n, probes as f32 / n, fine as f32 / n);
            }
            let (hm, pm) = (mean(&half), mean(&phase));
            let (h95, p95) = if half.is_empty() { (0.0, 0.0) } else { (pct95(&mut half.clone()), pct95(&mut phase.clone())) };
            let tol_mean = (3.0 * hm.max(pm)).min(0.01);
            let tol_95 = (3.0 * h95.max(p95)).min(0.03);
            let (rm, r95) = if refined.is_empty() { (0.0, 0.0) } else { (mean(&refined), pct95(&mut refined.clone())) };
            println!("lean {name} (stride {stride}): {visible} visible rays; dense half step {hm:.4} / {h95:.4}, phase {pm:.4} / {p95:.4}; tolerances {tol_mean:.4} / {tol_95:.4}; refined {rm:.4} / {r95:.4}; {misses} misses of {pairs}");
            if visible < 3 {
                failures.push(format!("{name} (stride {stride}): only {visible} visible rays"));
            }
            if rm > tol_mean || r95 > tol_95 {
                failures.push(format!("{name} (stride {stride}): refined {rm} / {r95} against {tol_mean} / {tol_95}"));
            }
            if misses * 50 > pairs * 3 {
                failures.push(format!("{name} (stride {stride}): {misses} misses of {pairs} pairs"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The lean light through the swept light volume against a fine integral: the
/// trace's estimate as the march makes it under the light volume (`VC_NEAR_LEAST` 1: one
/// near tap of the receiver's own light field, at its cast and placement footprints, over the first of the
/// eight intervals, then the near level's sweep read at that interval's end, trilinear, its texels
/// half-rounded), the level's producer taking the level's footprint for both footprints and its spacing as
/// the light's filter cell, against the fine integral of the receiver's light field over the whole path to
/// the bent top. Lit view samples (the shape past the gate) along rays from three fixed places and additional cloud sites, amounts 0.3 and
/// 0.8, suns 45 and 15 degrees. Asserts: at least 40 samples a case, every depth finite and
/// non-negative, the direct light's transmittance error `exp(-est) - exp(-fine)` at most 0.05 at the
/// median and 0.25 at the 95th percentile (prints both, and how often the read fell back to the taps).
#[test]
fn the_lean_light_volume_reader_against_a_fine_integral() {
    let spread = c1_spread();
    let across = lv::LV_ACROSS;
    let rays: Vec<Vec3> = std::iter::once(Vec3::Y).chain([70.0f32, 50.0, 35.0].iter().flat_map(|&el| [0.0f32, 120.0, 240.0].map(|az| c1_dir(el, az)))).collect();
    // Additional steep rays around real cloud columns, selected by coverage alone. Cap the
    // receivers on each ray, so a single long dense crossing cannot supply the quota.
    let aimed: Vec<Vec3> = std::iter::once(Vec3::Y)
        .chain([82.0f32, 74.0].into_iter().flat_map(|el| (0..8).map(move |k| c1_dir(el, 17.0 + 45.0 * k as f32))))
        .collect();
    let mut failures: Vec<String> = Vec::new();
    for (a, puff) in [(0.3f32, 0.0), (0.8, 0.0), (0.3, 1.0), (0.62, 1.0)] {
        let u = lean_ground_with_puff(a, None, puff);
        let clouds = lean_clouds_in(a, None, 16, &u);
        assert_eq!(clouds.len(), 16, "field probes at amount {a}");
        // The trace's taps and the level's producer read the light with the near level's spacing as the cell.
        let mut ul = u;
        ul.lcell.x = across;
        for sun in [45.0f32, 15.0] {
            let ld = c1_dir(sun, 30.0);
            let (mut errs, mut cached, mut fell) = (Vec::new(), Vec::new(), 0usize);
            let cameras = LV_PLACES[..3].iter().map(|&p| (p, &rays)).chain(clouds.iter().map(|&p| (p, &aimed)));
            for (index, (off, directions)) in cameras.enumerate() {
                // Always include the original three cameras and at least four cloud sites.
                // Additional sites are selected only by sample count, never by error.
                if index >= 7 && cached.len() >= 96 {
                    break;
                }
                let mut seen = Vec::new();
                for d in directions {
                    let mut taken = 0;
                    for (t0, t1) in spans(Vec3::ZERO, *d, 0.0, crate::medium::RAY_END, &u) {
                        let mut t = t0 + 75.0;
                        while t < t1.min(40_000.0) && taken < 3 {
                            let rel = *d * t;
                            let width = spread * t;
                            let pw = cloud_place_width(rel, *d, width, &u);
                            if lean_at(off + Vec2::new(rel.x, rel.z), bent_y(rel, 0.0, u.reach.y), (width, pw, width), a, None, &u, true).1 .1 > u.march.w {
                                seen.push((rel, width, pw));
                                taken += 1;
                                t += 2_000.0;
                            }
                            t += 150.0;
                        }
                    }
                }
                if seen.is_empty() {
                    continue;
                }
                let probes: Vec<Vec3> = seen.iter().map(|s| s.0).collect();
                let vol = move |q: Vec3| lean_light(off, q, a, &ul, (lv::LV_FOOTPRINT, lv::LV_FOOTPRINT));
                let swept = lv_sweep_fill(lv_sweep_lattice(across, &probes, ld, &u), &vol, ld, &u);
                for &(rel, width, pw) in &seen {
                    let recv = move |q: Vec3| lean_light(off, q, a, &ul, (width, pw));
                    let (near, at) = lv::lv_near(&recv, (rel, 0.0), ld, 1, &u);
                    let read = swept.read(rel + ld * at, &u);
                    let est = match read {
                        Some(t) => near + t,
                        None => {
                            fell += 1;
                            le_taps(&recv, rel, ld, &u)
                        }
                    };
                    let oracle = le_full(&recv, rel, ld, &u);
                    assert!(est.is_finite() && est >= 0.0 && oracle.is_finite() && oracle >= 0.0, "{est} {oracle}");
                    let error = (-est).exp() - (-oracle).exp();
                    errs.push(error);
                    if read.is_some() {
                        cached.push(error.abs());
                    }
                }
            }
            let mut abs: Vec<f32> = errs.iter().map(|e| e.abs()).collect();
            abs.sort_by(f32::total_cmp);
            let percentile = |sorted: &[f32], q: f32| {
                if sorted.is_empty() { f32::NAN } else { sorted[((sorted.len() - 1) as f32 * q).round() as usize] }
            };
            let (p50, p95) = (percentile(&abs, 0.50), percentile(&abs, 0.95));
            println!("lean light volume, amount {a:.2}, sun {sun:.0}: {} samples ({fell} fell back to the taps); vis err mean {:+.3} |p50| {p50:.3} |p95| {p95:.3}", errs.len(), errs.iter().sum::<f32>() / errs.len().max(1) as f32);
            cached.sort_by(f32::total_cmp);
            let (cache50, cache95) = (percentile(&cached, 0.50), percentile(&cached, 0.95));
            if cached.len() < 40 {
                failures.push(format!("amount {a} sun {sun}: only {} lit samples read the volume", cached.len()));
            } else if !(cache50 <= 0.05 && cache95 <= 0.25) {
                failures.push(format!("amount {a} sun {sun}: cache-only |p50| {cache50} |p95| {cache95} against 0.05 / 0.25"));
            } else if !(p50 <= 0.05 && p95 <= 0.25) {
                failures.push(format!("amount {a} sun {sun}: transmittance error |p50| {p50} |p95| {p95} against 0.05 / 0.25"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The lean model's density per amount (prints: `cargo test --release lean_density_report --
/// --nocapture`): per amount of [`CAL_COVERS`] the share of columns holding cloud and the mean opacity
/// straight up at DENSITY 1, over 8 regions of 16² columns 12,000 units apart, 64 heights each. Asserts
/// what must hold of any calibration: figures in 0..1, cloud present from amount 0.1 on, the occupied share
/// rising with the amount over 0.1 to 0.95, overcast (0.95) hiding most of the sky.
#[test]
fn lean_density_report() {
    use crate::volumetric_clouds_model2 as m2;
    let noise = crate::cloud_noise::production();
    println!("lean density: amount, occupied share, mean opacity overhead");
    let mut figures = Vec::new();
    for &a in &CAL_COVERS {
        let u = lean_ground(a, None);
        let (mut occupied, mut opacity, mut n) = (0usize, 0.0f32, 0usize);
        for r in 0..8u32 {
            let region = Vec2::new(r as f32 * 731_000.0, r as f32 * -413_000.0);
            for i in 0..16u32 {
                for j in 0..16u32 {
                    let jitter = Vec2::new(m_hash(r, i, j), m_hash(j, r, i));
                    let xz = region + (Vec2::new(i as f32, j as f32) + jitter) * 12_000.0;
                    let col = lean_column_at(xz, a, None, 0.0, &u);
                    n += 1;
                    if col.over <= 0.0 {
                        continue;
                    }
                    let g = m2::geometry(&col, &u);
                    let dy = g.extent / 64.0;
                    let mut tau = 0.0f32;
                    for s in 0..64 {
                        let above = (s as f32 + 0.5) * dy;
                        let pos = Vec3::new(xz.x, u.slab.x + above, xz.y);
                        let mut base = lean_base_reader(xz, above, &u);
                        let mut erode = |_w: f32| {
                            let c = m2::erode_coord(pos, pos.y, &u);
                            let e = noise.erosion.sample([c.x, c.y, c.z], 0.0);
                            [e[0], e[1], e[2]]
                        };
                        let smp = m2::Sample { above, width: 0.0, fw: dy, eroded: true, detail: true, wisp_g: 0.0 };
                        let d = lean_material(xz, &col, &g, &smp, 0.0, &u, &mut m2::Reads { base: &mut base, erode: &mut erode });
                        let water_content = lean_water_content(xz, above, smp.width, &col, d, &u);
                        let (shape, kk, _) = m2::shape(&col, above, d, water_content, &u);
                        tau += shape * kk * dy;
                    }
                    if tau > 0.0 {
                        occupied += 1;
                    }
                    opacity += 1.0 - (-tau).exp();
                }
            }
        }
        let (occ, op) = (occupied as f32 / n as f32, opacity / n as f32);
        assert!(occ.is_finite() && op.is_finite() && (0.0..=1.0).contains(&occ) && (0.0..=1.0).contains(&op));
        println!("  {a:.3}: occupied {occ:.4}, opacity {op:.4}");
        figures.push((a, occ, op));
    }
    for w in figures.windows(2) {
        if w[0].0 >= 0.1 {
            assert!(w[1].1 >= w[0].1, "the occupied share falls from amount {} to {}: {figures:?}", w[0].0, w[1].0);
        }
    }
    for &(a, occ, _) in &figures {
        if a >= 0.1 {
            assert!(occ > 0.0, "amount {a}: no cloud");
        }
    }
    assert!(figures.last().is_some_and(|f| f.2 >= 0.5), "overcast hides little: {figures:?}");
}
