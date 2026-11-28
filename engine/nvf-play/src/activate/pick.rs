//! What the crosshair points at: the collision ray's hit, or a reference's render bounds
//! when the ray finds no reference (pure, testable without Bevy's app).
//!
//! The rule [hypothesis, docs/spec-notes/simulation.md §5 "Crosshair targeting"]: the
//! target is the activatable reference nearest along the view within [`REACH`] of the eye.
//! Candidates are the reference whose collision the ray hits first, and references whose
//! render bounds (a mesh's box in its own space, i.e. an oriented box in the world) the
//! ray enters before the first collision hit of any kind, or within [`SURFACE_SLACK`]
//! behind it (walls, floors and tables hide what is behind them; an item resting on one
//! does not sink out of reach). Many small items have no collision, so the bounds are what
//! make them pickable. A box the eye is inside does not count, nor one that encloses
//! another candidate's along the ray (a bed's around the suitcase lying on it,
//! [`set_aside_enclosing`]).

use bevy::math::{Affine3A, Vec3};

/// How far from the eye the player can activate, game units [hypothesis: a placeholder of
/// about two metres at ~70 units per metre; no game setting in the masters names an
/// activation distance (simulation.md §5). In game the setting `iActivatePickLength`
/// reads 150 [observed: capture mv_trial, physics.md §6.3]; that it is this distance is
/// [documented: GECK wiki GetCrosshairRef] but unmeasured (job mv_values)].
pub const REACH: f32 = 150.0;

/// The first collision hit along the view (any owner), from `fnv_phys`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayHit {
    /// The placed reference hit, when the collider belongs to one.
    pub form_id: Option<u32>,
    pub distance: f32,
}

/// How far behind the surface the ray first hits a door may stand and still be the
/// collision hit, game units. Some buildings' collision has a face a few units in front
/// of their door: the Nipton town hall's mesh lies 8.5 units before its door's slab
/// (0x00130CFF), so the door was never targeted (#1339) [observed 2026-11-01; design:
/// the reach].
pub const DOOR_SLACK: f32 = 16.0;

/// How far behind the ray's first collision hit a reference's render box may be entered
/// and still be a candidate, game units. A collision surface lies a little above its
/// triangles (the shape's shell, its convex radius), so a small item resting on it can
/// have its whole box under the surface the ray stops at: Goodsprings Home's safe key
/// (VZKeyDirtyGSHome2SafeKey 0x001767AB, box 0.78 units thick, its top at z 2314.39) lies
/// on the bottom board of a broken shelf (0x0010B1E7) whose surface the ray meets at
/// z 2314.6, the board's 0.7-unit shell, and it was never the target (#3333) [observed:
/// FalloutNV.esm and the meshes, `fnvtool phys cell --ray`, the key on screen under the
/// crosshair]. Walls are far thicker than this, so what is behind them stays hidden
/// [design]. The first hit is still nearer, so it wins whenever it can be activated.
pub const SURFACE_SLACK: f32 = 2.0;

/// The collision hit the target is chosen from: the first hit, or a door found within
/// [`DOOR_SLACK`] behind it when the first hit is not something the player can
/// activate (`activatable`) and the door is.
pub fn door_behind(first: Option<RayHit>, behind: Option<RayHit>, mut activatable: impl FnMut(u32) -> bool) -> Option<RayHit> {
    match (first, behind) {
        (Some(f), Some(b)) if b.distance <= f.distance + DOOR_SLACK && !f.form_id.is_some_and(&mut activatable) && b.form_id.is_some_and(&mut activatable) => Some(b),
        (f, _) => f,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Collision,
    Bounds,
    /// The ray picked a bed someone lies on, and the target is the sleeper
    /// ([`to_occupant`]); the distance is to the sleeper's body.
    Occupant,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pick {
    pub form_id: u32,
    /// Distance from the eye along the view, game units.
    pub distance: f32,
    pub source: Source,
}

/// Choose the target from the collision hit and the render-bounds hits (reference,
/// entry distance; boxes entered beyond the collision hit by more than [`SURFACE_SLACK`]
/// are hidden by it). `accept` says whether a reference can be activated now (present in
/// the simulation, of an activatable type); it is asked only for candidates that would
/// win. Ties go to the collision hit.
pub fn choose(
    hit: Option<RayHit>,
    bounds: impl IntoIterator<Item = (u32, f32)>,
    reach: f32,
    mut accept: impl FnMut(u32) -> bool,
) -> Option<Pick> {
    let limit = hit.map_or(reach, |h| (h.distance + SURFACE_SLACK).min(reach));
    let mut best = hit
        .and_then(|h| Some((h.form_id?, h.distance)))
        .filter(|&(id, d)| d <= reach && accept(id))
        .map(|(form_id, distance)| Pick { form_id, distance, source: Source::Collision });
    for (id, d) in bounds {
        if !(0.0..=limit).contains(&d) {
            continue;
        }
        match best {
            Some(b) if b.distance <= d => {}
            Some(b) if b.form_id == id => best = Some(Pick { distance: d, ..b }),
            _ if accept(id) => best = Some(Pick { form_id: id, distance: d, source: Source::Bounds }),
            _ => {}
        }
    }
    best
}

/// [`choose`] for the crosshair, with some boxes only rough (`rough`: an actor's render
/// box, its skinned mesh's bind-pose bounds, which do not follow a body lying on the
/// floor [hypothesis]). Any precise candidate — a precise box entered before the
/// collision hit (#3125), or an activatable collision hit itself (#3664: a body's hit
/// volume behind another body's oversized rough box) — wins over any rough box, nearer or
/// not. Without one, rough boxes within reach and in front of a non-activatable collision
/// hit compete by [`choose_precise_or_rough`].
pub fn choose_precise_first(
    hit: Option<RayHit>,
    bounds: impl IntoIterator<Item = (u32, f32, bool)>,
    reach: f32,
    accept: impl FnMut(u32) -> bool,
) -> Option<Pick> {
    choose_precise_or_rough(hit, bounds, reach, |_| None, accept)
}

/// [`choose_precise_first`], breaking ties among rough boxes only (when neither a precise
/// box nor an activatable collision hit won) by `body_distance`: the shortest distance
/// from the crosshair ray to each actor's posed body segments (`None` when the actor has
/// no posed segments). Among rough boxes within reach and in front of `hit`, the actor
/// whose posed body is nearest the ray wins instead of the nearest rough box entry
/// ([design, #3664]: two nearby ragdolls both picked by bounds where one's oversized
/// bind-pose box reaches over the other). Without posed segments, the nearest rough box
/// entry wins as before, so a body without collision or posed segments stays searchable.
pub fn choose_precise_or_rough(
    hit: Option<RayHit>,
    bounds: impl IntoIterator<Item = (u32, f32, bool)>,
    reach: f32,
    mut body_distance: impl FnMut(u32) -> Option<f32>,
    mut accept: impl FnMut(u32) -> bool,
) -> Option<Pick> {
    let all: Vec<(u32, f32, bool)> = bounds.into_iter().collect();
    if let Some(precise) = choose(hit, all.iter().filter(|b| !b.2).map(|b| (b.0, b.1)), reach, &mut accept) {
        return Some(precise);
    }
    let limit = hit.map_or(reach, |h| (h.distance + SURFACE_SLACK).min(reach));
    let mut rough: Vec<(u32, f32)> = Vec::new();
    for &(id, d, is_rough) in &all {
        if !is_rough || !(0.0..=limit).contains(&d) {
            continue;
        }
        if let Some(existing) = rough.iter_mut().find(|r| r.0 == id) {
            existing.1 = existing.1.min(d);
        } else {
            rough.push((id, d));
        }
    }
    rough.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    let mut best: Option<(u32, f32, Option<f32>)> = None;
    for (id, entry) in rough {
        if !accept(id) {
            continue;
        }
        let miss = body_distance(id);
        let better = match best {
            None => true,
            Some((_, best_entry, best_miss)) => match (miss, best_miss) {
                (Some(m), Some(bm)) => m < bm || (m == bm && entry < best_entry),
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => entry < best_entry,
            },
        };
        if better {
            best = Some((id, entry, miss));
        }
    }
    best.map(|(form_id, distance, _)| Pick { form_id, distance, source: Source::Bounds })
}

/// One render box along the view: the reference, where the ray enters it and where it
/// leaves it (game units from the eye), and whether the box is only rough
/// ([`choose_precise_first`]).
pub type BoxSpan = (u32, f32, f32, bool);

/// [`choose_precise_first`], then what is inside the target wins over it: while the target
/// has a render box, an activatable reference the ray reaches within that box takes its
/// place, the nearest such first. "Within" is the other reference's collision hit lying
/// between the box's entry and exit, or its own precise box lying wholly between them
/// (rough boxes do not count: an actor is not inside a doorway's box in any useful sense).
/// What a collision hit hides stays hidden ([`choose`]'s limit), so a closed door or lid
/// still covers what is behind it. Only where `yields(target, inner)` does the target give
/// way: a container to anything it holds, an activator to an item lying in its box (the
/// note on the Old World Blues radio, #3937), as only a bed does in
/// [`set_aside_enclosing`]; `raw` is the first collision hit as the ray met it (not a door
/// promoted from behind it), so nothing behind a wall counts.
///
/// [design, #3937] In an Old World Blues interior (0x030086CB) a note lies on top of a
/// table radio (0x0301057F, an activator). The ray enters the radio's render box, which
/// reaches over the note, first, so the radio was the target wherever the crosshair was
/// on the note [observed: player report R-FQ39PUHZ, `target 0x0301057F Radio (Activate)
/// by Bounds at 55-87`, the note never a target]. An activator yields only to an item
/// (something the prompt offers to take), never to another activator, so a switch panel
/// keeps priority over a smaller switch behind it (#3323).
///
/// [design, #3351] An open locker in the Sloan barracks (0x000E7610,
/// `clutter\lockercabinet02.nif`, a container) holds a first aid kit on its upper shelf
/// (0x000E7612, a container with collision of its own) and books on the lower one
/// (0x000E7613 and others). The locker's collision is hollow: a ray into the open half
/// hits the kit at 132.5 units or the back wall at 149 [observed: `fnvtool phys cell
/// --ray`, FalloutNV.esm]. But the ray enters the locker's own render box at its front,
/// before anything inside it, and the nearest candidate wins, so the locker was the
/// target wherever the crosshair pointed [observed: player reports R-BHSP2X8C and
/// R-C6VJTJFT, `target 0x000E7610 Locker … by Bounds at 140`]. What the original does at
/// such a spot is not captured; that the things on an open shelf can be taken is the
/// reports' expectation.
pub fn choose_inside(
    hit: Option<RayHit>,
    raw: Option<RayHit>,
    boxes: impl IntoIterator<Item = BoxSpan>,
    reach: f32,
    yields: impl Fn(u32, u32) -> bool,
    accept: impl FnMut(u32) -> bool,
) -> Option<Pick> {
    choose_inside_with_body(hit, raw, boxes, reach, yields, |_| None, accept)
}

/// [`choose_inside`] with [`choose_precise_or_rough`]'s `body_distance` for rough actor
/// boxes (#3664).
pub fn choose_inside_with_body(
    hit: Option<RayHit>,
    raw: Option<RayHit>,
    boxes: impl IntoIterator<Item = BoxSpan>,
    reach: f32,
    yields: impl Fn(u32, u32) -> bool,
    body_distance: impl FnMut(u32) -> Option<f32>,
    mut accept: impl FnMut(u32) -> bool,
) -> Option<Pick> {
    let all: Vec<BoxSpan> = boxes.into_iter().collect();
    let mut picked = choose_precise_or_rough(hit, all.iter().map(|b| (b.0, b.1, b.3)), reach, body_distance, &mut accept)?;
    let limit = raw.map_or(reach, |h| (h.distance + SURFACE_SLACK).min(reach));
    // Each step goes to a reference inside the last one; a reference is taken once.
    let mut seen = vec![picked.form_id];
    loop {
        // Only a target that yields to something gives way (a container to what it holds,
        // an activator to an item); a chair or a switch panel keeps priority over another
        // activator its box encloses (#3323).
        let outer = picked.form_id;
        // The target's boxes as one stretch of the ray.
        let span = all.iter().filter(|b| b.0 == picked.form_id).fold(None, |s: Option<(f32, f32)>, b| Some(s.map_or((b.1, b.2), |(from, to)| (from.min(b.1), to.max(b.2)))));
        let Some((from, to)) = span else { return Some(picked) };
        let mut inner: Option<Pick> = None;
        if let Some((id, d)) = hit.and_then(|h| Some((h.form_id?, h.distance))) {
            if !seen.contains(&id) && (from..=to).contains(&d) && d <= reach && yields(outer, id) && accept(id) {
                inner = Some(Pick { form_id: id, distance: d, source: Source::Collision });
            }
        }
        for &(id, entry, exit, rough) in &all {
            if rough || seen.contains(&id) || entry < from || exit > to || !(0.0..=limit).contains(&entry) || !yields(outer, id) {
                continue;
            }
            // Nearer wins; the collision hit's own box does not take its place.
            if inner.is_some_and(|i| i.distance <= entry || i.form_id == id) {
                continue;
            }
            if accept(id) {
                inner = Some(Pick { form_id: id, distance: entry, source: Source::Bounds });
            }
        }
        match inner {
            Some(i) => {
                seen.push(i.form_id);
                picked = i;
            }
            None => return Some(picked),
        }
    }
}

/// Render boxes (reference, entry, exit, rough) without those of a bed (`bed`: furniture
/// to sleep on) when one of its boxes holds, along the ray, the whole stretch through
/// another reference's precise box (not an actor's rough one, [`choose_precise_first`])
/// that is itself a candidate (entered before `hit`, the first collision hit as the ray
/// met it, or within [`SURFACE_SLACK`] behind it, as [`choose`] takes it; within `reach`) and can be activated (`accept`): every box of that bed is set
/// aside (a bed drawn as several meshes yields whole), the thing lying in it stays
/// [design, #3323]. A bed's box runs from the floor to the top of its headboard and
/// encloses what lies on the mattress, so the ray enters it before a suitcase lying there
/// and the bed, nearest, won though the suitcase was what the crosshair was on. Full
/// containment decides only this setting aside; everything else is [`choose`]'s nearest
/// rule. Only a bed yields: a chair, or an activator's box (a switch panel), keeps
/// priority over a smaller activator behind it. Never a collision hit: the mattress still
/// picks the bed, and a wall in front still hides the suitcase. `hit` is the raw first
/// hit, not a door promoted from behind it (`door_behind`), so nothing behind the wall
/// sets a box aside. A box of the reference that `hit` strikes counts as entered before it
/// whatever its entry: the ray demonstrably meets that reference, and its mesh's box may
/// begin a hair past its collision (the Bison Steve suitcase: hit at 98.15, box from
/// 98.2). Two boxes over the same stretch both stay; a sleeper's rough box
/// sets no bed's aside, so [`to_occupant`] still sees the bed; nor does another bed's box
/// (two beds never set each other aside). The kept references' boxes are
/// [`choose_inside`]'s input.
pub fn set_aside_enclosing(
    hit: Option<RayHit>,
    boxes: &[(u32, f32, f32, bool)],
    reach: f32,
    bed: impl Fn(u32) -> bool,
    mut accept: impl FnMut(u32) -> bool,
) -> Vec<(u32, f32, bool)> {
    let limit = hit.map_or(reach, |h| (h.distance + SURFACE_SLACK).min(reach));
    let mut yielding: Vec<u32> = Vec::new();
    for &(id, enter, exit, _) in boxes {
        if yielding.contains(&id) || !bed(id) {
            continue;
        }
        let candidate = |other: u32, e: f32| (0.0..=limit).contains(&e) || (e >= 0.0 && e <= reach && hit.is_some_and(|h| h.form_id == Some(other)));
        let holds = |&&(other, e, x, r): &&(u32, f32, f32, bool)| other != id && !r && !bed(other) && candidate(other, e) && enter <= e && x <= exit && (enter < e || x < exit);
        if boxes.iter().filter(holds).any(|b| accept(b.0)) {
            yielding.push(id);
        }
    }
    boxes.iter().filter(|b| !yielding.contains(&b.0)).map(|&(id, enter, _, rough)| (id, enter, rough)).collect()
}

/// A bed an actor lies on (or is getting onto or off) yields the crosshair to that actor
/// when the actor's body is within `reach` of the eye [design, #3231: what the original
/// targets is not captured]. The ray enters the bed's box, which encloses the mattress,
/// before the sleeper's, and the sleeper's render box is rough (its bind-pose bounds, not
/// the lying body), so without this the bed was the target, refused as in use, and the
/// sleeper could not be woken and talked to. A bed is one user per reference
/// (docs/spec-notes/furniture.md §5), so nothing the player could use is lost.
/// `occupant` names the actor on a bed reference (not the player) and how far its body is
/// from the eye; a body beyond `reach` (the far end of a wide bed, aimed at its foot)
/// leaves the bed the target. `accept` is [`choose`]'s; a sleeper that cannot be
/// activated now leaves the bed too. The pick's distance becomes the body's.
pub fn to_occupant(picked: Option<Pick>, occupant: impl FnOnce(u32) -> Option<(u32, f32)>, reach: f32, accept: impl FnOnce(u32) -> bool) -> Option<Pick> {
    let p = picked?;
    match occupant(p.form_id) {
        Some((actor, d)) if actor != p.form_id && (0.0..=reach).contains(&d) && accept(actor) => Some(Pick { form_id: actor, distance: d, source: Source::Occupant }),
        _ => Some(p),
    }
}

/// Distance from `p` to the segment `a`–`b` (a posed bone).
pub fn segment_distance(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = b - a;
    let len2 = ab.length_squared();
    let t = if len2 > 1e-12 { ((p - a).dot(ab) / len2).clamp(0.0, 1.0) } else { 0.0 };
    p.distance(a + ab * t)
}

/// Shortest distance between the bounded ray `origin + t * dir` (`0 <= t <= max_t`) and
/// the segment `a`–`b` (a posed bone) [design, #3664].
pub fn ray_segment_distance(origin: Vec3, dir: Vec3, max_t: f32, a: Vec3, b: Vec3) -> f32 {
    let max_t = max_t.max(0.0);
    let e = b - a;
    let w0 = origin - a;
    let aa = dir.length_squared();
    let bb = dir.dot(e);
    let cc = e.length_squared();
    let dd = dir.dot(w0);
    let ee = e.dot(w0);
    let denom = aa * cc - bb * bb;
    let mut best = f32::INFINITY;
    if denom > 1e-12 {
        let t = (bb * ee - cc * dd) / denom;
        let u = (aa * ee - bb * dd) / denom;
        if (0.0..=max_t).contains(&t) && (0.0..=1.0).contains(&u) {
            best = best.min((origin + dir * t).distance_squared(a + e * u));
        }
    }
    if aa > 1e-12 {
        let t_a = (-dd / aa).clamp(0.0, max_t);
        best = best.min((origin + dir * t_a).distance_squared(a));
        let t_b = ((bb - dd) / aa).clamp(0.0, max_t);
        best = best.min((origin + dir * t_b).distance_squared(b));
    }
    let d0 = segment_distance(origin, a, b);
    best = best.min(d0 * d0);
    let d1 = segment_distance(origin + dir * max_t, a, b);
    best = best.min(d1 * d1);
    best.sqrt()
}

/// Distance from `p` (world space) to the nearest point of a box given in an entity's own
/// space (`center`, `half`) placed by `to_world`; 0 inside. Exact for a uniform scale.
/// `None` for a degenerate transform.
pub fn box_distance(p: Vec3, to_world: &Affine3A, center: Vec3, half: Vec3) -> Option<f32> {
    if to_world.matrix3.determinant().abs() < 1e-12 {
        return None;
    }
    let local = to_world.inverse().transform_point3(p) - center;
    let near = center + local.clamp(-half, half);
    Some(p.distance(to_world.transform_point3(near)))
}

/// Distance along a ray (`origin`, unit `dir`, world space) at which it enters a box
/// given in an entity's own space (`center`, `half` extents) placed by `to_world`. `None`
/// when the ray misses, the box is behind, or `origin` is inside the box.
pub fn ray_box(origin: Vec3, dir: Vec3, to_world: &Affine3A, center: Vec3, half: Vec3) -> Option<f32> {
    ray_box_span(origin, dir, to_world, center, half).map(|(entry, _)| entry)
}

/// [`ray_box`] with where the ray leaves the box as well: (entry, exit), distances along
/// the ray. `None` in the same cases.
pub fn ray_box_span(origin: Vec3, dir: Vec3, to_world: &Affine3A, center: Vec3, half: Vec3) -> Option<(f32, f32)> {
    if to_world.matrix3.determinant().abs() < 1e-12 {
        return None;
    }
    let inv = to_world.inverse();
    // An affine map keeps the ray's parameter: t in local space is t in world space.
    let o = inv.transform_point3(origin) - center;
    let d = inv.transform_vector3(dir);
    let (mut t0, mut t1) = (f32::NEG_INFINITY, f32::INFINITY);
    for i in 0..3 {
        if d[i].abs() < 1e-12 {
            if o[i].abs() > half[i] {
                return None;
            }
            continue;
        }
        let a = (-half[i] - o[i]) / d[i];
        let b = (half[i] - o[i]) / d[i];
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
    }
    (t0 <= t1 && t0 >= 0.0).then_some((t0, t1))
}

/// A sphere around the placed box (world centre, radius), for a cheap range test first.
pub fn bounding_sphere(to_world: &Affine3A, center: Vec3, half: Vec3) -> (Vec3, f32) {
    let m = to_world.matrix3;
    let scale = m.x_axis.length().max(m.y_axis.length()).max(m.z_axis.length());
    (to_world.transform_point3(center), half.length() * scale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::{Quat, Vec3};

    const ANY: fn(u32) -> bool = |_| true;
    const YIELDS: fn(u32, u32) -> bool = |_, _| true;

    #[test]
    fn collision_hit_on_an_activatable_reference_wins() {
        let p = choose(Some(RayHit { form_id: Some(7), distance: 90.0 }), [], REACH, ANY).unwrap();
        assert_eq!(p, Pick { form_id: 7, distance: 90.0, source: Source::Collision });
    }

    #[test]
    fn beyond_reach_or_not_activatable_is_no_target() {
        assert!(choose(Some(RayHit { form_id: Some(7), distance: 151.0 }), [], REACH, ANY).is_none());
        assert!(choose(Some(RayHit { form_id: Some(7), distance: 50.0 }), [], REACH, |id| id != 7).is_none());
        assert!(choose(Some(RayHit { form_id: None, distance: 50.0 }), [], REACH, ANY).is_none());
        assert!(choose(None, [(3, 151.0)], REACH, ANY).is_none());
    }

    #[test]
    fn bounds_find_items_without_collision() {
        // Nothing collides (the item has no collision; nothing behind within reach).
        let p = choose(None, [(3, 120.0), (4, 80.0), (5, 100.0)], REACH, ANY).unwrap();
        assert_eq!(p, Pick { form_id: 4, distance: 80.0, source: Source::Bounds });
        // An item on a table: the ray enters its box before it hits the table's top.
        let table = RayHit { form_id: Some(100), distance: 95.0 };
        let p = choose(Some(table), [(4, 90.0)], REACH, |id| id != 100).unwrap();
        assert_eq!((p.form_id, p.source), (4, Source::Bounds));
    }

    /// #3333: the safe key lies on a shelf's board, its box just under the board's
    /// collision surface (the shape's shell): the key is the target. Further behind the
    /// surface, or behind a hit the player can activate, it is not.
    #[test]
    fn an_item_resting_in_a_surface_s_shell_is_picked() {
        let shelf = RayHit { form_id: Some(100), distance: 126.0 };
        let not_shelf = |id| id != 100;
        let p = choose(Some(shelf), [(7, 126.3)], REACH, not_shelf).unwrap();
        assert_eq!(p, Pick { form_id: 7, distance: 126.3, source: Source::Bounds });
        // The ground (no reference) too.
        let ground = RayHit { form_id: None, distance: 80.0 };
        assert_eq!(choose(Some(ground), [(7, 81.5)], REACH, ANY).unwrap().form_id, 7);
        // More than the slack behind: hidden.
        assert!(choose(Some(shelf), [(7, 126.0 + SURFACE_SLACK + 0.1)], REACH, not_shelf).is_none());
        // A container's lid the key rests on: the container, nearer, is the target.
        let crate_ = RayHit { form_id: Some(100), distance: 126.0 };
        assert_eq!(choose(Some(crate_), [(7, 126.3)], REACH, ANY).unwrap(), Pick { form_id: 100, distance: 126.0, source: Source::Collision });
        // Still within reach only.
        assert!(choose(Some(RayHit { form_id: None, distance: REACH - 0.5 }), [(7, REACH + 0.5)], REACH, ANY).is_none());
    }

    #[test]
    fn walls_hide_what_is_behind_them() {
        let wall = RayHit { form_id: None, distance: 60.0 };
        assert!(choose(Some(wall), [(4, 80.0)], REACH, ANY).is_none());
        let wall = RayHit { form_id: Some(100), distance: 60.0 };
        assert!(choose(Some(wall), [(4, 80.0)], REACH, |id| id != 100).is_none());
    }

    #[test]
    fn nearest_wins_and_ties_go_to_collision() {
        // An item in front of a door: the item is nearer.
        let door = RayHit { form_id: Some(9), distance: 120.0 };
        assert_eq!(choose(Some(door), [(4, 70.0)], REACH, ANY).unwrap().form_id, 4);
        // The door's own render box at the same distance keeps the collision source.
        let p = choose(Some(door), [(9, 120.0), (9, 119.0)], REACH, ANY).unwrap();
        assert_eq!((p.form_id, p.source), (9, Source::Collision));
        assert_eq!(p.distance, 119.0);
        // Rejected candidates are skipped, not chosen.
        assert_eq!(choose(None, [(1, 10.0), (2, 20.0)], REACH, |id| id == 2).unwrap().form_id, 2);
    }

    /// #3125: a body's render box entered first does not hide an item's box entered
    /// after it; without an item, the body is still picked by its box; a precise box
    /// behind a collision hit stays hidden.
    #[test]
    fn an_item_beside_a_body_wins_over_the_body_s_box() {
        // The ray enters the ghoul's box (8, rough) at 60 and the ammo box's (4) at 95.
        let p = choose_precise_first(None, [(8, 60.0, true), (4, 95.0, false)], REACH, ANY).unwrap();
        assert_eq!(p, Pick { form_id: 4, distance: 95.0, source: Source::Bounds });
        // Plain `choose` (the console's click) keeps the nearest.
        assert_eq!(choose(None, [(8, 60.0), (4, 95.0)], REACH, ANY).unwrap().form_id, 8);
        // Only the body: searched by its box.
        let p = choose_precise_first(None, [(8, 60.0, true)], REACH, ANY).unwrap();
        assert_eq!((p.form_id, p.source), (8, Source::Bounds));
        // An item that cannot be taken does not stand in the body's way.
        assert_eq!(choose_precise_first(None, [(8, 60.0, true), (4, 95.0, false)], REACH, |id| id != 4).unwrap().form_id, 8);
        // The floor at 90 hides an item box entered at 95.
        let floor = RayHit { form_id: None, distance: 90.0 };
        assert_eq!(choose_precise_first(Some(floor), [(8, 60.0, true), (4, 95.0, false)], REACH, ANY).unwrap().form_id, 8);
        // A living actor's collision hit nearer than its box: collision, as before.
        let actor = RayHit { form_id: Some(8), distance: 70.0 };
        let p = choose_precise_first(Some(actor), [(8, 65.0, true)], REACH, ANY).unwrap();
        assert_eq!((p.form_id, p.source), (8, Source::Collision));
    }

    /// #3351: the open locker in the Sloan barracks. The ray enters the locker's box (10)
    /// at 114 and leaves it at 151; inside it are a first aid kit (12, with collision) and
    /// a book (13, a box only).
    #[test]
    fn what_is_inside_an_open_container_wins_over_it() {
        let locker = (10, 114.0, 151.0, false);
        // Aimed at the kit: its collision is hit at 132.5, inside the locker's box.
        let kit = RayHit { form_id: Some(12), distance: 132.5 };
        let p = choose_inside(Some(kit), Some(kit), [locker, (12, 131.0, 141.0, false)], REACH, YIELDS, ANY).unwrap();
        assert_eq!(p, Pick { form_id: 12, distance: 132.5, source: Source::Collision });
        // The rule before: the locker's box, entered first, won.
        assert_eq!(choose_precise_first(Some(kit), [(10, 114.0, false), (12, 131.0, false)], REACH, ANY).unwrap().form_id, 10);
        // Aimed at the book: the ray goes on to the locker's back wall at 149; the book's
        // box lies inside the locker's.
        let back = RayHit { form_id: Some(10), distance: 149.0 };
        let p = choose_inside(Some(back), Some(back), [locker, (13, 128.0, 133.0, false)], REACH, YIELDS, ANY).unwrap();
        assert_eq!(p, Pick { form_id: 13, distance: 128.0, source: Source::Bounds });
        // Aimed at an empty shelf: the locker.
        let p = choose_inside(Some(back), Some(back), [locker], REACH, YIELDS, ANY).unwrap();
        assert_eq!((p.form_id, p.source), (10, Source::Collision));
        // Two things inside: the nearer.
        let p = choose_inside(Some(back), Some(back), [locker, (13, 128.0, 133.0, false), (14, 120.0, 126.0, false)], REACH, YIELDS, ANY).unwrap();
        assert_eq!(p.form_id, 14);
        // One that cannot be taken does not stand in the other's way, nor in the locker's.
        assert_eq!(choose_inside(Some(back), Some(back), [locker, (13, 128.0, 133.0, false), (14, 120.0, 126.0, false)], REACH, YIELDS, |id| id != 14).unwrap().form_id, 13);
        assert_eq!(choose_inside(Some(back), Some(back), [locker, (13, 128.0, 133.0, false)], REACH, YIELDS, |id| id != 13).unwrap().form_id, 10);
        // The closed half: the door's collision at 114.6 hides what is behind it, and the
        // locked locker stays the target.
        let door = RayHit { form_id: Some(11), distance: 114.6 };
        let p = choose_inside(Some(door), Some(door), [(11, 114.0, 151.0, false), (13, 128.0, 133.0, false)], REACH, YIELDS, ANY).unwrap();
        assert_eq!((p.form_id, p.source), (11, Source::Collision));
        // A wall between: nothing behind it, inside a box or not.
        let wall = RayHit { form_id: None, distance: 100.0 };
        assert!(choose_inside(Some(wall), Some(wall), [locker, (13, 128.0, 133.0, false)], REACH, YIELDS, ANY).is_none());
        // Something that only sticks into the locker's box, or pokes out of its back, is
        // not inside it: the nearest wins as before.
        assert_eq!(choose_inside(Some(back), Some(back), [locker, (15, 100.0, 120.0, false)], REACH, YIELDS, ANY).unwrap().form_id, 15);
        assert_eq!(choose_inside(Some(back), Some(back), [locker, (16, 140.0, 160.0, false)], REACH, YIELDS, ANY).unwrap().form_id, 10);
        // An actor's rough box inside a box does not take the target.
        assert_eq!(choose_inside(Some(back), Some(back), [locker, (8, 120.0, 140.0, true)], REACH, YIELDS, ANY).unwrap().form_id, 10);
        // A box inside a box inside a box: the innermost.
        let p = choose_inside(None, None, [(20, 50.0, 140.0, false), (21, 60.0, 120.0, false), (22, 70.0, 80.0, false)], REACH, YIELDS, ANY).unwrap();
        assert_eq!(p.form_id, 22);
        // Without boxes it is `choose_precise_first`.
        assert_eq!(choose_inside(Some(kit), Some(kit), [], REACH, YIELDS, ANY), Some(Pick { form_id: 12, distance: 132.5, source: Source::Collision }));
        assert!(choose_inside(None, None, [], REACH, YIELDS, ANY).is_none());
        // Only a holder gives way: a locker that is not a container (a chair, a panel) keeps
        // the crosshair over what its box encloses (#3323).
        assert_eq!(choose_inside(Some(back), Some(back), [locker, (13, 128.0, 133.0, false)], REACH, |outer, _| outer != 10, ANY).unwrap().form_id, 10);
        // The cutoff is the raw hit: a wall at 125 hides the book at 128 even when a door
        // behind the wall was promoted to `hit`.
        let wall_at = RayHit { form_id: None, distance: 125.0 };
        assert_eq!(choose_inside(Some(back), Some(wall_at), [locker, (13, 128.0, 133.0, false)], REACH, YIELDS, ANY).unwrap().form_id, 10);
    }

    /// #3937: a note (31, a box only) lying on a table radio (30, an activator) whose render
    /// box reaches over it. The radio's collision is its body below the note.
    #[test]
    fn an_item_lying_in_an_activator_s_box_wins_over_it() {
        const RADIO: u32 = 30;
        const NOTE: u32 = 31;
        const SWITCH: u32 = 32;
        // An activator yields to an item, never to another activator.
        let yields = |outer: u32, inner: u32| outer == RADIO && inner == NOTE;
        let radio = (RADIO, 55.0, 87.0, false);
        // Aimed at the note: the ray passes over the radio's body and on to the wall at 120;
        // the note's box lies inside the radio's.
        let wall = RayHit { form_id: None, distance: 120.0 };
        let p = choose_inside(Some(wall), Some(wall), [radio, (NOTE, 60.0, 64.0, false)], REACH, yields, ANY).unwrap();
        assert_eq!(p, Pick { form_id: NOTE, distance: 60.0, source: Source::Bounds });
        // The rule before: the radio's box, entered first, won.
        assert_eq!(choose_inside(Some(wall), Some(wall), [radio, (NOTE, 60.0, 64.0, false)], REACH, |_, _| false, ANY).unwrap().form_id, RADIO);
        // Aimed at the radio's body: its collision at 70, the note not on the ray.
        let body = RayHit { form_id: Some(RADIO), distance: 70.0 };
        let p = choose_inside(Some(body), Some(body), [radio], REACH, yields, ANY).unwrap();
        assert_eq!((p.form_id, p.source), (RADIO, Source::Collision));
        // A note that cannot be taken now leaves the radio the target.
        assert_eq!(choose_inside(Some(wall), Some(wall), [radio, (NOTE, 60.0, 64.0, false)], REACH, yields, |id| id != NOTE).unwrap().form_id, RADIO);
        // A smaller activator inside the radio's box does not take its place (#3323).
        assert_eq!(choose_inside(Some(wall), Some(wall), [radio, (SWITCH, 60.0, 64.0, false)], REACH, yields, ANY).unwrap().form_id, RADIO);
    }

    #[test]
    fn a_ray_s_span_through_a_box() {
        let unit = Affine3A::IDENTITY;
        let span = ray_box_span(Vec3::new(-5.0, 0.0, 0.0), Vec3::X, &unit, Vec3::ZERO, Vec3::splat(1.0)).unwrap();
        assert!((span.0 - 4.0).abs() < 1e-5 && (span.1 - 6.0).abs() < 1e-5, "{span:?}");
        assert_eq!(ray_box(Vec3::new(-5.0, 0.0, 0.0), Vec3::X, &unit, Vec3::ZERO, Vec3::splat(1.0)), Some(span.0));
        assert!(ray_box_span(Vec3::new(-5.0, 3.0, 0.0), Vec3::X, &unit, Vec3::ZERO, Vec3::splat(1.0)).is_none());
    }

    /// #3231: Elder Lyons asleep in his bed. The ray enters the bed's box (20) before
    /// his rough box (8), and the bed's precise box wins; the bed then yields to him
    /// while his body is within reach.
    #[test]
    fn a_bed_in_use_yields_to_its_sleeper_in_reach() {
        let (bed, lyons, crate_) = (20, 8, 4);
        let at = |d: f32| move |id: u32| (id == bed).then_some((lyons, d));
        let picked = choose_precise_first(None, [(bed, 67.0, false), (lyons, 80.0, true)], REACH, ANY);
        assert_eq!(picked.unwrap().form_id, bed);
        assert_eq!(to_occupant(picked, at(72.0), REACH, ANY), Some(Pick { form_id: lyons, distance: 72.0, source: Source::Occupant }));
        // By the bed's collision as well.
        let hit = Some(RayHit { form_id: Some(bed), distance: 140.0 });
        assert_eq!(to_occupant(choose_precise_first(hit, [], REACH, ANY), at(149.0), REACH, ANY).unwrap().form_id, lyons);
        // Aimed at the foot of a wide bed with him beyond reach: the bed stays.
        assert_eq!(to_occupant(picked, at(REACH + 1.0), REACH, ANY), picked);
        // A free bed stays the bed.
        let free = choose_precise_first(None, [(bed, 67.0, false)], REACH, ANY);
        assert_eq!(to_occupant(free, |_| None, REACH, ANY), free);
        // A sleeper that cannot be activated now (interaction disabled) leaves the bed.
        assert_eq!(to_occupant(picked, at(72.0), REACH, |id| id != lyons), picked);
        // Something else picked, or nothing: unchanged.
        let item = choose(None, [(crate_, 50.0)], REACH, ANY);
        assert_eq!(to_occupant(item, at(72.0), REACH, ANY), item);
        assert_eq!(to_occupant(None, at(72.0), REACH, ANY), None);
    }

    /// #3323: a suitcase lying on a bed at the Bison Steve. Along the ray the bed's box
    /// (20) runs from 60 to 140, the suitcase's (5) from 98 to 102, the mattress's
    /// collision is hit at 110.
    #[test]
    fn a_suitcase_lying_in_a_bed_s_box_wins_over_the_bed() {
        const BED: u32 = 20;
        const SUITCASE: u32 = 5;
        let lyons = 8;
        let (bed_box, case_box) = ((BED, 60.0, 140.0, false), (SUITCASE, 98.0, 102.0, false));
        let at = |id: u32, distance: f32| Some(RayHit { form_id: Some(id), distance });
        let bed = |id: u32| id == BED;
        let pick = |hit: Option<RayHit>, boxes: &[(u32, f32, f32, bool)], accept: fn(u32) -> bool| {
            choose_precise_first(hit, set_aside_enclosing(hit, boxes, REACH, bed, accept), REACH, accept)
        };
        // Before: the bed's box, entered first, won.
        let all = [bed_box, case_box].map(|b| (b.0, b.1, b.3));
        assert_eq!(choose_precise_first(at(BED, 110.0), all, REACH, ANY).unwrap().form_id, BED);
        // Now the suitcase, whether it has collision of its own or not.
        assert_eq!(pick(at(BED, 110.0), &[bed_box, case_box], ANY), Some(Pick { form_id: SUITCASE, distance: 98.0, source: Source::Bounds }));
        assert_eq!(pick(at(SUITCASE, 99.0), &[bed_box, case_box], ANY).unwrap().form_id, SUITCASE);
        assert_eq!(pick(None, &[case_box, bed_box], ANY).unwrap().form_id, SUITCASE);
        // The bare mattress beside it: the bed, by its collision or its box.
        assert_eq!(pick(at(BED, 110.0), &[bed_box], ANY), Some(Pick { form_id: BED, distance: 60.0, source: Source::Collision }));
        assert_eq!(pick(None, &[bed_box], ANY).unwrap().form_id, BED);
        // A wall at 80 hides the suitcase, which then sets nothing aside.
        let wall = Some(RayHit { form_id: None, distance: 80.0 });
        assert_eq!(pick(wall, &[bed_box, case_box], ANY).unwrap().form_id, BED);
        assert!(pick(wall, &[bed_box, case_box], |id| id != BED).is_none());
        // A suitcase that cannot be taken leaves the bed.
        assert_eq!(pick(at(BED, 110.0), &[bed_box, case_box], |id| id != SUITCASE).unwrap().form_id, BED);
        // Over the same stretch, or only overlapping, neither is set aside: the nearest.
        assert_eq!(pick(None, &[(BED, 98.0, 102.0, false), case_box], ANY).unwrap().form_id, BED);
        assert_eq!(pick(None, &[bed_box, (SUITCASE, 98.0, 150.0, false)], ANY).unwrap().form_id, BED);
        // A sleeper's rough box inside the bed's sets nothing aside: the bed is picked and
        // yields to the sleeper (#3231).
        let picked = pick(None, &[bed_box, (lyons, 80.0, 130.0, true)], ANY);
        assert_eq!(picked.unwrap().form_id, BED);
        let p = to_occupant(picked, |id| (id == BED).then_some((lyons, 90.0)), REACH, ANY).unwrap();
        assert_eq!((p.form_id, p.source), (lyons, Source::Occupant));
        // Only a bed yields: a switch panel's box (30, an activator) or a chair's back (32)
        // from 50 to 130 keeps priority over a smaller activator's (31) behind it.
        assert_eq!(pick(None, &[(30, 50.0, 130.0, false), (31, 80.0, 90.0, false)], ANY).unwrap().form_id, 30);
        assert_eq!(pick(None, &[(32, 50.0, 130.0, false), (31, 80.0, 90.0, false)], ANY).unwrap().form_id, 32);
        // A bed drawn as two meshes (60 to 140 and 70 to 90) yields whole: its second box
        // does not win at 70 over the suitcase hit at 99.
        let two = [bed_box, (BED, 70.0, 90.0, false), case_box];
        assert_eq!(pick(at(SUITCASE, 99.0), &two, ANY).unwrap().form_id, SUITCASE);
        assert_eq!(pick(at(BED, 110.0), &two[..2], ANY).unwrap().form_id, BED);
        // Two beds of two meshes each, each with a box inside the other's (21: 60-100
        // and 115-125; 22: 70-80 and 105-140): neither sets the other aside, the nearest.
        let beds = |id: u32| id == 21 || id == 22;
        let boxes = [(21, 60.0, 100.0, false), (22, 70.0, 80.0, false), (22, 105.0, 140.0, false), (21, 115.0, 125.0, false)];
        let kept = set_aside_enclosing(None, &boxes, REACH, beds, ANY);
        assert_eq!(kept.len(), 4);
        assert_eq!(choose_precise_first(None, kept, REACH, ANY).unwrap().form_id, 21);
    }

    /// #3323, the smoke run at the Bison Steve (pick log): the suitcase's collision is
    /// hit at 98.15, its box entered at 98.2, a hair past it, inside the bed's box
    /// (96.9 to 189.6). The struck reference's box still counts: the suitcase wins.
    #[test]
    fn the_struck_reference_s_box_counts_though_it_begins_past_the_hit() {
        let (bed, suitcase) = (0x000D72C7, 0x000AFA24);
        let hit = Some(RayHit { form_id: Some(suitcase), distance: 98.15 });
        let boxes = [(suitcase, 111.7, 130.8, false), (suitcase, 98.2, 114.8, false), (suitcase, 103.9, 104.0, false), (0x000E4B99, 186.5, 187.1, false), (bed, 96.9, 189.6, false), (bed, 130.8, 130.8, false), (0x000D7302, 188.4, 188.4, false)];
        let kept = set_aside_enclosing(hit, &boxes, REACH, |id| id == bed, ANY);
        assert!(kept.iter().all(|k| k.0 != bed), "{kept:?}");
        assert_eq!(choose_precise_first(hit, kept, REACH, ANY), Some(Pick { form_id: suitcase, distance: 98.15, source: Source::Collision }));
        // Another reference's box beginning more than SURFACE_SLACK past the hit is still
        // hidden by it (a wall at 96, the suitcase's box from 98.2); within the slack it is
        // a candidate, as `choose` takes it (#3333), and the bed yields.
        let wall = Some(RayHit { form_id: None, distance: 96.0 });
        assert_eq!(set_aside_enclosing(wall, &boxes, REACH, |id| id == bed, ANY).len(), boxes.len());
        let wall = Some(RayHit { form_id: None, distance: 98.15 });
        assert!(set_aside_enclosing(wall, &boxes, REACH, |id| id == bed, ANY).iter().all(|k| k.0 != bed));
    }

    /// #3323 review: the cutoff for setting a box aside is the raw first hit (a wall at
    /// 80), not the door promoted from 94 behind it (`door_behind`): a suitcase box behind
    /// the wall (85 to 90) inside a door's box (a bed here, 65 to 120) leaves the door.
    #[test]
    fn nothing_behind_the_wall_sets_a_box_aside() {
        const DOOR: u32 = 2;
        let (raw, door) = (Some(RayHit { form_id: None, distance: 80.0 }), Some(RayHit { form_id: Some(DOOR), distance: 94.0 }));
        let boxes = [(DOOR, 65.0, 120.0, false), (5, 85.0, 90.0, false)];
        let kept = set_aside_enclosing(raw, &boxes, REACH, |id| id == DOOR, ANY);
        assert_eq!(choose_precise_first(door, kept, REACH, ANY), Some(Pick { form_id: DOOR, distance: 65.0, source: Source::Collision }));
        assert_eq!(set_aside_enclosing(raw, &boxes, REACH, |id| id == DOOR, ANY).len(), 2, "the raw hit sets nothing aside");
        // With the promoted hit as the cutoff the door's box would be set aside.
        assert_eq!(set_aside_enclosing(door, &boxes, REACH, |id| id == DOOR, ANY).len(), 1);
    }

    #[test]
    fn distances_to_a_bone_and_to_a_placed_box() {
        let (a, b) = (Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0));
        assert!((segment_distance(Vec3::new(5.0, 3.0, 0.0), a, b) - 3.0).abs() < 1e-5);
        assert!((segment_distance(Vec3::new(-4.0, 3.0, 0.0), a, b) - 5.0).abs() < 1e-5);
        assert!((segment_distance(Vec3::new(1.0, 0.0, 0.0), a, a) - 1.0).abs() < 1e-5);
        // A 10-unit cube scaled ×2 and turned 90° about the up axis, 100 units ahead.
        let t = Affine3A::from_scale_rotation_translation(Vec3::splat(2.0), Quat::from_rotation_y(std::f32::consts::FRAC_PI_2), Vec3::new(0.0, 0.0, -100.0));
        let d = box_distance(Vec3::ZERO, &t, Vec3::ZERO, Vec3::splat(5.0)).unwrap();
        assert!((d - 90.0).abs() < 1e-3, "{d}");
        assert_eq!(box_distance(Vec3::new(0.0, 0.0, -100.0), &t, Vec3::ZERO, Vec3::splat(5.0)), Some(0.0));
        assert!(box_distance(Vec3::ZERO, &Affine3A::from_scale(Vec3::ZERO), Vec3::ZERO, Vec3::ONE).is_none());
    }

    #[test]
    fn ray_enters_a_placed_box() {
        // A 10-unit cube scaled ×2 and turned 45° about the up axis, 100 units ahead.
        let t = Affine3A::from_scale_rotation_translation(Vec3::splat(2.0), Quat::from_rotation_y(std::f32::consts::FRAC_PI_4), Vec3::new(0.0, 0.0, -100.0));
        let half = Vec3::splat(5.0);
        let d = ray_box(Vec3::ZERO, Vec3::NEG_Z, &t, Vec3::ZERO, half).unwrap();
        let (_, out) = ray_box_span(Vec3::ZERO, Vec3::NEG_Z, &t, Vec3::ZERO, half).unwrap();
        assert!((out - (100.0 + 10.0 * 2f32.sqrt())).abs() < 1e-3, "{out}");
        // Corner first: 10 · √2 from the centre after scaling.
        assert!((d - (100.0 - 10.0 * 2f32.sqrt())).abs() < 1e-3, "{d}");
        // Off to the side, behind, and from inside.
        assert!(ray_box(Vec3::new(30.0, 0.0, 0.0), Vec3::NEG_Z, &t, Vec3::ZERO, half).is_none());
        assert!(ray_box(Vec3::ZERO, Vec3::Z, &t, Vec3::ZERO, half).is_none());
        assert!(ray_box(Vec3::new(0.0, 0.0, -100.0), Vec3::NEG_Z, &t, Vec3::ZERO, half).is_none());
        // An off-centre box in the entity's space.
        let d = ray_box(Vec3::new(0.0, 3.0, 0.0), Vec3::NEG_Z, &Affine3A::IDENTITY, Vec3::new(0.0, 3.0, -50.0), Vec3::ONE).unwrap();
        assert!((d - 49.0).abs() < 1e-4);
        // A flat box (a sheet of paper) is still hit face-on.
        let d = ray_box(Vec3::ZERO, Vec3::NEG_Z, &Affine3A::IDENTITY, Vec3::new(0.0, 0.0, -20.0), Vec3::new(1.0, 1.0, 0.0)).unwrap();
        assert!((d - 20.0).abs() < 1e-4);
        // Degenerate transforms are ignored.
        assert!(ray_box(Vec3::ZERO, Vec3::NEG_Z, &Affine3A::from_scale(Vec3::ZERO), Vec3::ZERO, half).is_none());
    }

    #[test]
    fn sphere_covers_the_box() {
        let t = Affine3A::from_scale_rotation_translation(Vec3::new(1.0, 3.0, 1.0), Quat::IDENTITY, Vec3::X * 10.0);
        let (c, r) = bounding_sphere(&t, Vec3::Y, Vec3::ONE);
        assert_eq!(c, Vec3::new(10.0, 3.0, 0.0));
        assert!((r - 3.0 * 3f32.sqrt()).abs() < 1e-5);
    }

    #[test]
    fn a_door_just_behind_a_wall_face_is_the_hit() {
        let (wall, door, crate_) = (1, 2, 3);
        let at = |id: u32, distance: f32| Some(RayHit { form_id: Some(id), distance });
        let usable = |id: u32| id != wall;
        // The Nipton town hall: its mesh at 89.2, the door 8.5 behind it.
        assert_eq!(door_behind(at(wall, 89.2), at(door, 97.7), usable), at(door, 97.7));
        // Too far behind, or the first hit is usable itself: the first hit stands.
        assert_eq!(door_behind(at(wall, 50.0), at(door, 50.0 + DOOR_SLACK + 1.0), usable), at(wall, 50.0));
        assert_eq!(door_behind(at(crate_, 40.0), at(door, 45.0), usable), at(crate_, 40.0));
        // Ground (no reference) counts as not usable; nothing behind leaves it.
        assert_eq!(door_behind(Some(RayHit { form_id: None, distance: 30.0 }), at(door, 35.0), usable), at(door, 35.0));
        assert_eq!(door_behind(at(wall, 89.2), None, usable), at(wall, 89.2));
    }

    /// #3664: a collision hit on body A behind body B's rough render box picks A, while
    /// #3125's precise item box still wins before it and a floor/wall hit still falls back
    /// to rough boxes in front of it.
    #[test]
    fn a_collision_hit_on_body_a_behind_body_b_s_rough_box_picks_a() {
        const FRED: u32 = 0x0A0393EA;
        const ANT: u32 = 0x0A0C8064;
        const ITEM: u32 = 4;
        let hit_fred = Some(RayHit { form_id: Some(FRED), distance: 125.0 });
        // The ant's rough box is entered at 83, well ahead of Fred's collision hit at 125.
        let p = choose_precise_first(hit_fred, [(ANT, 83.0, true), (FRED, 120.0, true)], REACH, ANY).unwrap();
        assert_eq!(p, Pick { form_id: FRED, distance: 125.0, source: Source::Collision });
        // Same through `choose_inside_with_body`.
        let p = choose_inside_with_body(
            hit_fred,
            hit_fred,
            [(ANT, 83.0, 140.0, true), (FRED, 120.0, 150.0, true)],
            REACH,
            |_, _| false,
            |_| None,
            ANY,
        )
        .unwrap();
        assert_eq!(p, Pick { form_id: FRED, distance: 125.0, source: Source::Collision });
        // A precise item box entered before Fred's collision hit still wins (#3125).
        let p = choose_precise_first(hit_fred, [(ANT, 83.0, true), (ITEM, 95.0, false)], REACH, ANY).unwrap();
        assert_eq!(p, Pick { form_id: ITEM, distance: 95.0, source: Source::Bounds });
        // A non-activatable floor hit behind the rough box still lets the body be searched.
        let floor = Some(RayHit { form_id: None, distance: 130.0 });
        let p = choose_precise_first(floor, [(ANT, 83.0, true)], REACH, ANY).unwrap();
        assert_eq!(p, Pick { form_id: ANT, distance: 83.0, source: Source::Bounds });
    }

    /// #3664 (Wilks' house): when two nearby ragdolls are both picked only by rough boxes
    /// (no collision hit on either), the actor whose posed body is nearest the crosshair
    /// ray wins instead of the nearer rough box entry.
    #[test]
    fn among_two_rough_bodies_the_one_whose_posed_body_is_nearest_the_ray_wins() {
        const FRED: u32 = 0x0A0393EA;
        const ANT: u32 = 0x0A0C8064;
        let eye = Vec3::new(1994.0, -1562.0, 570.0);
        let fred_hip = (Vec3::new(1896.0, -1574.0, 454.0), Vec3::new(1916.0, -1574.0, 458.0));
        let ant_torso = (Vec3::new(2120.0, -1562.0, 458.0), Vec3::new(2146.0, -1562.0, 458.0));
        let boxes = [(ANT, 83.0, 145.0, true), (FRED, 125.0, 165.0, true)];

        let body_miss = |dir: Vec3| {
            move |id: u32| match id {
                FRED => Some(ray_segment_distance(eye, dir, REACH, fred_hip.0, fred_hip.1)),
                ANT => Some(ray_segment_distance(eye, dir, REACH, ant_torso.0, ant_torso.1)),
                _ => None,
            }
        };

        // Crosshair aimed at Fred Wilks' hip: Fred wins even though the ant's rough box starts nearer (83 vs 125).
        let to_fred = (Vec3::new(1906.0, -1574.0, 456.0) - eye).normalize();
        let p = choose_inside_with_body(None, None, boxes, REACH, |_, _| false, body_miss(to_fred), ANY).unwrap();
        assert_eq!(p, Pick { form_id: FRED, distance: 125.0, source: Source::Bounds });

        // Crosshair aimed at the ant: the ant wins.
        let to_ant = (Vec3::new(2133.0, -1562.0, 458.0) - eye).normalize();
        let p = choose_inside_with_body(None, None, boxes, REACH, |_, _| false, body_miss(to_ant), ANY).unwrap();
        assert_eq!(p, Pick { form_id: ANT, distance: 83.0, source: Source::Bounds });

        // A lone body without collision hit or posed parts can still be searched.
        let p = choose_inside_with_body(None, None, [(FRED, 125.0, 165.0, true)], REACH, |_, _| false, |_| None, ANY).unwrap();
        assert_eq!(p, Pick { form_id: FRED, distance: 125.0, source: Source::Bounds });

        // Rejected nearer-body actor does not block the other body.
        let p = choose_inside_with_body(None, None, boxes, REACH, |_, _| false, body_miss(to_fred), |id| id != FRED).unwrap();
        assert_eq!(p, Pick { form_id: ANT, distance: 83.0, source: Source::Bounds });
    }

    #[test]
    fn ray_segment_distance_covers_crossing_skew_parallel_degenerate_and_bounds() {
        // Crossing ray: distance 0.
        assert!(ray_segment_distance(Vec3::ZERO, Vec3::X, 100.0, Vec3::new(50.0, -5.0, 0.0), Vec3::new(50.0, 5.0, 0.0)) < 1e-5);
        // Skew ray passing 4 units above a bone at x = 50.
        let d = ray_segment_distance(Vec3::ZERO, Vec3::X, 100.0, Vec3::new(50.0, -5.0, 4.0), Vec3::new(50.0, 5.0, 4.0));
        assert!((d - 4.0).abs() < 1e-5, "{d}");
        // Parallel segment 3 units off the ray.
        let d = ray_segment_distance(Vec3::ZERO, Vec3::X, 100.0, Vec3::new(20.0, 3.0, 0.0), Vec3::new(40.0, 3.0, 0.0));
        assert!((d - 3.0).abs() < 1e-5, "{d}");
        // Degenerate zero-length segment (single joint).
        let d = ray_segment_distance(Vec3::ZERO, Vec3::X, 100.0, Vec3::new(30.0, 0.0, 7.0), Vec3::new(30.0, 0.0, 7.0));
        assert!((d - 7.0).abs() < 1e-5, "{d}");
        // Segment behind the ray origin clamps to t = 0.
        let d = ray_segment_distance(Vec3::ZERO, Vec3::X, 100.0, Vec3::new(-3.0, 4.0, 0.0), Vec3::new(-3.0, 10.0, 0.0));
        assert!((d - 5.0).abs() < 1e-5, "{d}");
        // Segment beyond max_t clamps to t = max_t.
        let d = ray_segment_distance(Vec3::ZERO, Vec3::X, 50.0, Vec3::new(53.0, 4.0, 0.0), Vec3::new(53.0, 10.0, 0.0));
        assert!((d - 5.0).abs() < 1e-5, "{d}");
    }
}
