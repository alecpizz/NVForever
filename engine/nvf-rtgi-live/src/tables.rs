//! A prepared scene, a state of the light and the points of a volume as the words a kernel reads. What the shader
//! could not compute as the solver does is refused here, with the reason, and is the CPU solver's alone.

use nvf_rtgi::{Cancel, Cull, Face, LampTable, Level, LightState, Material, PreparedScene, Sheared, SolvedVolume, Texels, Wrap, V3};
use std::collections::HashMap;

/// Words of a material, a texture and a direction.
const MATERIAL: usize = 16;
const TEXTURE: usize = 8;
pub(crate) const DIRECTION: usize = 13;
/// Where a direction's parts are in its words: as given (for the moments), as the rays go (still axes 0: for the
/// tree, the triangle test and the side met), the shear, the axes (two bits each, then a bit each for the still ones)
/// and the reciprocals.
pub(crate) const GIVEN: usize = 0;
pub(crate) const ALONG: usize = 3;
pub(crate) const SHEAR: usize = 6;
pub(crate) const AXES: usize = 9;
pub(crate) const INVERSE: usize = 10;
/// Words of a point.
pub(crate) const PLACE: usize = 7;
/// Of the lights' words, before the rows.
const LIGHTS: usize = 16;
const NONE: u32 = u32::MAX;
/// Of a face's first word: its texture is read at 0, 0.
const UV_ZERO: u32 = 1 << 31;
/// No corner and no point is further off: an f32's step is 0.008 there.
const FAR: f32 = 1e5;
/// No colour's factor and no part of a texture's transform is more; their products are no infinities then.
const MOST: f32 = 1e10;
/// A triangle as near as this to a point is tested for the point's rays by the solver's test alone; fewer are where
/// there are more than [`MOST_NEAR`], down to [`LEAST_NEAR`].
const NEAR: f32 = 0.5;
const LEAST_NEAR: f32 = 0.02;
const MOST_NEAR: usize = 64;
/// No texture's coordinate is more, transformed, with a little to spare for the rounding: a texel's number, of at most
/// [`SIDE`] a side, is no more than an i32 then.
const REPEATS: f32 = 1e5;
/// Nor, as given, is any corner's: a blend of them, of weights that add up to about 1, is no infinity.
const RAW: f32 = 1e30;
const SIDE: usize = 1 << 14;
/// All lights' display values together are no more, whatever they add or take.
const BRIGHT: f32 = 1e6;
/// No surface's albedo (its colour, as linear, times its material's) is more on any channel: times a light sum's
/// power, at most BRIGHT^2.2 = 1.6e13, it is at most 2e37, below f32's end, so the solver's min(albedo x power, 1)
/// is no product that overflows on a device.
const ALBEDO: f64 = 1_208_925_819_614_629_174_706_176.0; // 2^80
/// No lamp's range that is above 0 is less. Its square is at least 1e-12; a part of d^2 that a device may take as 0
/// (a square below 2^-126) then changes neither whether d^2 >= range^2 nor whether the attenuation is 0: where every
/// part of d^2 is below 2^-60, d^2 < 3 x 2^-60 < range^2 on both sides and 1 - d^2 / range^2 is above 1/4; where one
/// is 2^-60 or more, half a step of any sum with it is at least 2^-85, and a sum below 2^-100, which is all that holds
/// a part a device would drop, adds nothing to it on either side.
///
/// Beyond these bounds the contract is this: the GPU may treat subnormal values (below f32::MIN_POSITIVE) as zero
/// (WGSL allows it). A lamp's contribution below the smallest normal float, and a spot's direction within about 2^-63
/// units of its lamp, may then be zero on the GPU where the CPU has a subnormal value: the light differs by less than
/// 1.2e-38 and a lamp or shadow-ray count may differ by such lamps. Nothing visible changes.
const LEAST_RANGE: f32 = 1e-6;

/// The scene's part: what does not change while a room is the same.
#[derive(Clone, Debug, PartialEq)]
pub struct Tables {
    /// Three a primitive: those without an alpha test first, else in the scene's order.
    corners: Vec<[f32; 3]>,
    faces: Vec<u32>,
    /// Materials, textures, vertex colours, the sRGB table, directions.
    data: Vec<u32>,
    texels: Vec<u32>,
    /// The scene's number of each primitive.
    order: Vec<u32>,
    /// Primitives without an alpha test.
    opaque: u32,
    /// Where the parts of `data` after the materials start.
    at: [u32; 4],
    /// The directions: rays of a point.
    rays: u32,
}

fn words(f: &[f32]) -> impl Iterator<Item = u32> + '_ {
    f.iter().map(|f| f.to_bits())
}

/// What a face's alpha test comes to.
#[derive(Clone, Copy, PartialEq)]
enum Test {
    None,
    Asked,
}

/// Whether the shader can do the face's alpha test. The product of the colour's and the material's alpha may be past
/// f32's end: the solver has it infinite, or no number where the texel is 0, and both stop a ray.
fn test(face: &Face, m: &Material) -> Result<Test, String> {
    if m.cutoff <= 0.0 {
        return Ok(Test::None);
    }
    let a = face.colors.map(|c| c[3]);
    let least = a.iter().fold(f32::INFINITY, |l, a| l.min(*a));
    if least > 0.0 && least * 0.5 * m.base[3] == f32::INFINITY {
        return Ok(Test::None);
    }
    if a.iter().chain([&m.base[3]]).any(|a| a.abs() > MOST) {
        return Err(format!("an alpha of {a:?} x {}", m.base[3]));
    }
    Ok(Test::Asked)
}

impl Tables {
    /// For points with rays along `directions` ([`nvf_rtgi::directions`]).
    pub fn new(scene: &PreparedScene, directions: &[V3]) -> Result<Tables, String> {
        Tables::until(scene, directions, &|| false)
    }

    /// [`Tables::new`], given up once `cancel` says so.
    pub fn until(scene: &PreparedScene, directions: &[V3], cancel: Cancel) -> Result<Tables, String> {
        let given_up = |i: usize| i.is_multiple_of(4096) && cancel();
        let e = scene.export();
        let mut out = Tables { corners: Vec::new(), faces: Vec::new(), data: Vec::new(), texels: Vec::new(), order: Vec::new(), opaque: 0, at: [0; 4], rays: directions.len() as u32 };
        if directions.is_empty() || directions.len() > nvf_rtgi::MOST_RAYS || directions.iter().any(|d| !(0.999..1.001).contains(&d.length())) {
            return Err(format!("{} directions, or one that is not unit", directions.len()));
        }
        if e.corners.is_empty() {
            return Err("no triangles".into());
        }
        // Counted before any is numbered.
        if e.corners.len() >= 1 << 26 || e.materials.len() >= 1 << 27 || e.textures.len() >= 1 << 27 {
            return Err(format!("{} triangles, {} materials, {} textures", e.corners.len(), e.materials.len(), e.textures.len()));
        }
        let mut tested = vec![false; e.textures.len()];
        let (mut first, mut second) = (Vec::new(), Vec::new());
        for (i, face) in e.faces.iter().enumerate() {
            if given_up(i) {
                return Err("cancelled".into());
            }
            let m = e.materials.get(face.material as usize).ok_or("a face of a material that is not there")?;
            if e.corners[i].iter().any(|c| !c.finite() || c.max_abs() > FAR) || !face.normal.finite() || face.normals.as_flattened().iter().any(|n| !n.is_finite()) {
                return Err(format!("triangle {i}: a corner or a normal"));
            }
            if face.colors.iter().any(|c| c[..3].iter().any(|c| !(0.0..=MOST).contains(c))) {
                return Err(format!("triangle {i}: its colours"));
            }
            // The most of a blend of the corners' colours, as linear (an sRGB display value c above 1 is at most
            // c^2.4 then), times the material's; a texel is at most 1.
            let linear = |c: f32| if m.display_colors && c > 1.0 { f64::from(c).powf(2.4) } else { f64::from(c.max(1.0)) };
            if (0..3).any(|k| face.colors.iter().fold(0.0f64, |a, c| a.max(linear(c[k]))) * f64::from(m.base[k]) > ALBEDO) {
                return Err(format!("triangle {i}: an albedo past 2^80"));
            }
            if m.texture.is_some() && face.uv.as_flattened().iter().all(|c| c.is_finite()) {
                // The coordinates are blended, then transformed: no blend is past the corners' largest, and each term of
                // any place between them is no larger than this, which holds no cancellation of large terms.
                let most = |k: usize| face.uv.iter().fold(0.0f32, |m, uv| m.max(uv[k].abs()));
                let terms = |r: &[f32; 3]| (r[0].abs() * most(0) + r[1].abs() * most(1) + r[2].abs()) * 1.001;
                let far = most(0).max(most(1)) > RAW || m.uv.iter().any(|r| terms(r) > REPEATS);
                if far {
                    return Err(format!("triangle {i}: its texture's coordinates"));
                }
            }
            match test(face, m).map_err(|why| format!("triangle {i}: {why}"))? {
                Test::None => first.push(i as u32),
                Test::Asked => {
                    second.push(i as u32);
                    if let Some(t) = m.texture.and_then(|t| tested.get_mut(t)) {
                        *t = true;
                    }
                }
            }
        }
        out.opaque = first.len() as u32;
        first.append(&mut second);
        out.order = first;
        for m in e.materials {
            out.material(m, e.textures.len())?;
        }
        out.textures(&e.textures, &tested)?;
        let mut rows: HashMap<[u32; 12], u32> = HashMap::new();
        let mut colors = Vec::new();
        for (i, tri) in out.order.iter().enumerate() {
            if given_up(i) {
                return Err("cancelled".into());
            }
            let face = &e.faces[*tri as usize];
            out.corners.extend(e.corners[*tri as usize].map(|c| c.0));
            let row: [u32; 12] = std::array::from_fn(|k| face.colors[k / 4][k % 4].to_bits());
            let next = rows.len() as u32;
            let at = *rows.entry(row).or_insert_with(|| {
                colors.extend(row);
                next
            });
            let whole = face.uv.as_flattened().iter().all(|c| c.is_finite());
            out.faces.extend([face.material | if whole { 0 } else { UV_ZERO }, at]);
            out.faces.extend(words(&face.normal.0));
            out.faces.extend(words(face.normals.as_flattened()));
            out.faces.extend(face.uv.as_flattened().iter().map(|c| if whole { c.to_bits() } else { 0 }));
            out.faces.push(*tri);
        }
        out.at[1] = out.data.len() as u32;
        out.data.append(&mut colors);
        out.at[2] = out.data.len() as u32;
        // One table is all sRGB textures'.
        match e.textures.iter().flatten().find(|t| t.srgb) {
            Some(t) => out.data.extend(words(t.to_linear)),
            None => out.data.extend([0; 256]),
        }
        out.at[3] = out.data.len() as u32;
        // The direction as given is for the moments; as the rays go, for the tree, the test and the side met.
        out.data.reserve(DIRECTION * directions.len());
        for d in directions {
            let Sheared { along, axes: [x, y, z], shear, inverse, still } = Sheared::of(*d);
            let mut row = [0u32; DIRECTION];
            for (at, part) in [(GIVEN, d.0), (ALONG, along), (SHEAR, shear), (INVERSE, inverse)] {
                row[at..at + 3].copy_from_slice(&part.map(f32::to_bits));
            }
            row[AXES] = x | y << 2 | z << 4 | u32::from(still[0]) << 6 | u32::from(still[1]) << 7 | u32::from(still[2]) << 8;
            out.data.extend(row);
        }
        Ok(out)
    }

    fn material(&mut self, m: &Material, textures: usize) -> Result<(), String> {
        let (color, uv) = (m.base[..3].iter().all(|c| (0.0..=MOST).contains(c)), m.uv.iter().flatten().all(|c| c.abs() <= MOST));
        if !(color && uv && m.base[3].is_finite() && m.cutoff.is_finite()) {
            return Err(format!("material {}: {m:?}", self.data.len() / MATERIAL));
        }
        let cull = match m.cull {
            Cull::None => 0,
            Cull::Back => 1,
            Cull::Front => 2,
        };
        let flags = cull | u32::from(m.double_sided) << 2 | u32::from(m.unlit) << 3 | u32::from(m.display_colors) << 4;
        self.data.extend(words(&m.base));
        self.data.extend(words(&m.uv[0]));
        self.data.push(m.cutoff.to_bits());
        self.data.extend(words(&m.uv[1]));
        self.data.push(m.texture.filter(|t| *t < textures).map_or(NONE, |t| t as u32));
        self.data.extend([flags, 0, 0, 0]);
        Ok(())
    }

    /// Colour levels of all, alpha levels of those of `tested`; a byte an alpha. Materials of a missing one lose it.
    fn textures(&mut self, all: &[Option<Texels>], tested: &[bool]) -> Result<(), String> {
        for m in self.data.as_chunks_mut::<MATERIAL>().0 {
            if m[11] != NONE && all[m[11] as usize].is_none() {
                m[11] = NONE;
            }
        }
        self.at[0] = self.data.len() as u32;
        let wrap = |w: Wrap| match w {
            Wrap::Repeat => 0,
            Wrap::Mirror => 1,
            Wrap::Clamp => 2,
        };
        for (t, tested) in all.iter().zip(tested) {
            let mut row = [0u32; TEXTURE];
            if let Some(t) = t {
                let side = |l: &Level| [l.width, l.height].map(|s| s as u32);
                if [t.alpha, t.color].iter().any(|l| l.width.max(l.height) > SIDE) {
                    return Err(format!("a level of {} x {} texels", t.alpha.width, t.alpha.height));
                }
                // A colour is a word, an alpha a byte: the first and the last of each read are numbers of a u32.
                let fits = |first: usize, n: usize| u32::try_from(first + n.saturating_sub(1)).map(|_| first as u32).map_err(|_| "too many texels".to_string());
                [row[4], row[5]] = side(t.color);
                row[3] = fits(self.texels.len(), t.color.texels.len())?;
                self.texels.extend(t.color.texels.iter().map(|c| u32::from_le_bytes(*c)));
                if *tested {
                    [row[1], row[2]] = side(t.alpha);
                    row[0] = fits(4 * self.texels.len(), t.alpha.texels.len())?;
                    self.texels.extend(t.alpha.texels.chunks(4).map(|four| u32::from_le_bytes(std::array::from_fn(|k| four.get(k).map_or(0, |c| c[3])))));
                }
                row[6] = wrap(t.wrap[0]) | wrap(t.wrap[1]) << 2 | u32::from(t.srgb) << 4;
            }
            self.data.extend(row);
        }
        Ok(())
    }

    pub fn triangles(&self) -> u32 {
        self.order.len() as u32
    }

    /// The scene's number of each primitive.
    pub fn order(&self) -> &[u32] {
        &self.order
    }

    /// The primitives without an alpha test: the first ones.
    pub fn opaque(&self) -> u32 {
        self.opaque
    }

    pub fn rays(&self) -> u32 {
        self.rays
    }

    /// Three a primitive, in [`Tables::order`].
    pub(crate) fn corners(&self) -> &[[f32; 3]] {
        &self.corners
    }

    /// A primitive's material (and whether its texture is read at 0, 0), its row of colours, its normal, its corners'
    /// normals and texture coordinates, and the scene's number of it.
    pub(crate) fn faces(&self) -> &[u32] {
        &self.faces
    }

    /// Materials, then from [`Tables::starts`] on the textures, the rows of colours, the sRGB table and the directions:
    /// as given, as the rays go, the shear, the axes and the still ones, the reciprocals.
    pub(crate) fn data(&self) -> &[u32] {
        &self.data
    }

    pub(crate) fn starts(&self) -> [u32; 4] {
        self.at
    }

    /// The colour levels' texels, and the alpha tests' four alphas a word.
    pub(crate) fn texels(&self) -> &[u32] {
        &self.texels
    }

    /// Of its buffers on a device, without the structures.
    pub fn bytes(&self) -> u64 {
        4 * (3 * self.corners.len() + self.faces.len() + self.data.len() + self.texels.len()) as u64
    }
}

/// A state of the light for the shader. Its grid takes milliseconds: it may be made on any thread.
#[derive(Clone, Debug)]
pub struct PackedLight {
    words: Vec<u32>,
    /// The most of one cell.
    lamps: u32,
}

impl PackedLight {
    /// What a solve refuses is refused, and lights too bright together.
    pub fn of(light: &LightState) -> Result<PackedLight, String> {
        light_words(light).map(|(words, lamps)| PackedLight { words, lamps })
    }

    /// A head (the ambient, how many directionals | the grid's least corner, its side | its cells, how many lamps |
    /// where the directionals, the lamps, the grid's starts and its entries are), then those.
    pub fn words(&self) -> &[u32] {
        &self.words
    }

    /// The most lamps of one cell: of one hit's shadow rays.
    pub fn most_lamps(&self) -> u32 {
        self.lamps
    }

    pub fn bytes(&self) -> u64 {
        4 * self.words.len() as u64
    }
}

/// The light's words ([`PackedLight::words`]), and the most lamps of one cell. The grid is the solver's own
/// ([`LampTable`]), which also refuses what a solve refuses.
fn light_words(light: &LightState) -> Result<(Vec<u32>, u32), String> {
    let grid = LampTable::of(light, &|| false).map_err(|e| e.to_string())?;
    let displays = light.directionals.iter().map(|d| d.display).chain(light.lamps.iter().map(|l| l.display)).chain([light.ambient]);
    let sum = displays.fold(0.0, |s, d| s + d.max_abs());
    if sum.is_nan() || sum > BRIGHT {
        return Err(format!("lights of {sum} together"));
    }
    // A range below LEAST_RANGE: of one whose square is 0 the solver divides by it (0 / 0 at the lamp itself, a light
    // that is no number and a shadow ray); of a small one a device may drop parts of d^2 that decide.
    if let Some(l) = light.lamps.iter().find(|l| l.range > 0.0 && l.range < LEAST_RANGE) {
        return Err(format!("a lamp of range {}", l.range));
    }
    let mut out = Vec::with_capacity(LIGHTS + 6 * light.directionals.len() + 16 * light.lamps.len() + grid.start.len() + grid.lamps.len());
    out.extend(words(&light.ambient.0));
    out.push(light.directionals.len() as u32);
    out.extend(words(&grid.min.0));
    out.push(grid.side.to_bits());
    out.extend(grid.cells);
    out.push(light.lamps.len() as u32);
    out.extend([0; 4]);
    out[12] = out.len() as u32;
    for d in &light.directionals {
        out.extend(words(&d.display.0));
        out.extend(words(&d.to_source.0));
    }
    out[13] = out.len() as u32;
    for l in &light.lamps {
        out.extend(words(&l.position.0));
        out.push(l.range.to_bits());
        out.extend(words(&l.display.0));
        out.push(u32::from(l.cone.is_some()));
        out.extend(words(&l.cone.map_or([0.0; 5], |c| c.numbers())));
        out.extend([0; 3]);
    }
    out[14] = out.len() as u32;
    out.extend(&grid.start);
    out[15] = out.len() as u32;
    out.extend(&grid.lamps);
    let most = grid.start.windows(2).map(|w| w[1] - w[0]).max().unwrap_or(0);
    Ok((out, most))
}

/// The points of a volume that count, and how its texture is laid out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Points {
    positions: Vec<V3>,
    /// Of the first slab.
    texels: Vec<[u32; 3]>,
    /// 0: of no texture.
    slab_step: u32,
    /// The texture's size.
    atlas: [u32; 3],
    /// Of each point, where its triangles are in `near` and how many; a primitive and its box each.
    lists: Vec<[u32; 2]>,
    near: Vec<u32>,
}

impl Points {
    /// In the order of [`SolvedVolume::points`]; the tables are the scene's.
    pub fn of(volume: &SolvedVolume, scene: &PreparedScene, tables: &Tables) -> Result<Points, String> {
        Points::until(volume, scene, tables, &|| false)
    }

    /// [`Points::of`], given up once `cancel` says so.
    pub fn until(volume: &SolvedVolume, scene: &PreparedScene, tables: &Tables, cancel: Cancel) -> Result<Points, String> {
        let points = volume.points().map_err(|e| e.to_string())?;
        let texels = points.iter().map(|p| p.texel).collect();
        let out = Points::near_of(points.iter().map(|p| p.position).collect(), scene, tables, cancel)?;
        Ok(Points { texels, slab_step: volume.slab_step(), atlas: volume.atlas, ..out })
    }

    /// Of no texture.
    pub fn at(positions: Vec<V3>, scene: &PreparedScene, tables: &Tables) -> Result<Points, String> {
        Points::near_of(positions, scene, tables, &|| false)
    }

    fn near_of(positions: Vec<V3>, scene: &PreparedScene, tables: &Tables, cancel: Cancel) -> Result<Points, String> {
        let mut out = Points::bare(positions);
        let mut primitive = vec![0u32; tables.order.len()];
        for (i, tri) in tables.order.iter().enumerate() {
            *primitive.get_mut(*tri as usize).ok_or("tables of another scene")? = i as u32;
        }
        for (i, (p, list)) in out.positions.iter().zip(&mut out.lists).enumerate() {
            if i.is_multiple_of(64) && cancel() {
                return Err("cancelled".into());
            }
            let mut far = NEAR;
            let mut near = scene.near(*p, far);
            while near.len() > MOST_NEAR && far > LEAST_NEAR {
                far = (0.5 * far).max(LEAST_NEAR);
                near = scene.near(*p, far);
            }
            if near.len() > MOST_NEAR {
                return Err(format!("{} triangles within {far} of the point at {:?}", near.len(), p.0));
            }
            *list = [out.near.len() as u32, near.len() as u32];
            for n in near {
                out.near.push(*primitive.get(n.tri as usize).ok_or("tables of another scene")?);
                out.near.extend(words(&n.min.0).chain(words(&n.max.0)));
            }
        }
        Ok(out)
    }

    /// Without the triangles near them: what surface a point is on is the structure's rounding.
    pub fn bare(positions: Vec<V3>) -> Points {
        let n = positions.len();
        Points { texels: vec![[0; 3]; n], positions, slab_step: 0, atlas: [0; 3], lists: vec![[0; 2]; n], near: Vec::new() }
    }

    /// The points with triangles near them, all those triangles, and the most of a point.
    pub fn near(&self) -> [usize; 3] {
        [self.lists.iter().filter(|l| l[1] > 0).count(), self.lists.iter().map(|l| l[1] as usize).sum(), self.lists.iter().map(|l| l[1] as usize).max().unwrap_or(0)]
    }

    pub fn positions(&self) -> &[V3] {
        &self.positions
    }

    /// From a slab of the texture to the next along z, and the texture's size; 0 of no texture.
    pub fn layout(&self) -> (u32, [u32; 3]) {
        (self.slab_step, self.atlas)
    }

    /// Of its buffer on a device.
    pub fn bytes(&self) -> u64 {
        4 * ((PLACE + 1) * self.positions.len() + self.near.len()) as u64
    }

    /// Seven words a point (its position, its texel in two words, where its triangles' list is and how long), the
    /// points' order, then the lists: a primitive and its leaf's box, seven words each.
    pub fn words(&self) -> Result<Vec<u32>, String> {
        let texel = |t: &[u32; 3]| t.iter().all(|c| *c < 1 << 16);
        if self.positions.is_empty() || self.positions.len() != self.texels.len() || !self.texels.iter().all(texel) {
            return Err(format!("{} points of {} texels", self.positions.len(), self.texels.len()));
        }
        if let Some(p) = self.positions.iter().find(|p| !p.finite() || p.max_abs() > FAR) {
            return Err(format!("a point at {:?}", p.0));
        }
        let n = self.positions.len();
        let lists_at = (PLACE + 1) * n;
        if u32::try_from(lists_at + self.near.len()).is_err() {
            return Err(format!("{n} points and {} triangles near them", self.near.len()));
        }
        let mut out = Vec::with_capacity(lists_at + self.near.len());
        for ((p, t), l) in self.positions.iter().zip(&self.texels).zip(&self.lists) {
            out.extend([p[0].to_bits(), p[1].to_bits(), p[2].to_bits(), t[0] | t[1] << 16, t[2], lists_at as u32 + l[0], l[1]]);
        }
        out.extend(0..n as u32);
        out.extend(&self.near);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nvf_rtgi::{directions, prepare, solve, Cone, Directional, GatherConfig, Instance, Lamp, Mesh, SceneInput, SceneLimits, Seeds, SolveConfig, Texture};

    /// Words of a face.
    const FACE: usize = 21;

    fn made(materials: Vec<Material>, textures: Vec<Option<Texture>>, edit: impl Fn(&mut Mesh)) -> PreparedScene {
        let mut mesh = Mesh { positions: vec![[0.0; 3], [8.0, 0.0, 0.0], [0.0, 8.0, 0.0], [8.0, 8.0, 1.0]], uv0: vec![[0.0, 0.5]; 4], indices: vec![0, 1, 2, 1, 3, 2], ..Mesh::default() };
        edit(&mut mesh);
        let instances = (0..materials.len()).map(|material| Instance { mesh: 0, material, world_from_local: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, material as f32]] }).collect();
        prepare(SceneInput { meshes: vec![mesh], instances, materials, textures }, &SceneLimits::default(), 1, &|| false).unwrap()
    }

    fn cut() -> Material {
        Material { cutoff: 0.5, texture: Some(1), ..Material::default() }
    }

    fn texture() -> Texture {
        let level = |side: usize| Level { width: side, height: side, texels: (0..side * side).map(|i| [i as u8, 2, 3, (i as u8).wrapping_add(1)]).collect() };
        Texture { levels: vec![level(128), level(8)], srgb: true, wrap: [Wrap::Mirror, Wrap::Clamp] }
    }

    /// Tested ones last, the rows where the shader looks for them.
    #[test]
    fn a_scene_in_words() {
        let bare = Material { texture: Some(0), cull: Cull::Front, double_sided: true, unlit: true, display_colors: true, ..Material::default() };
        let w = made(vec![cut(), bare, Material::default()], vec![None, Some(texture())], |m| m.uv0[3] = [f32::NAN, 0.0]);
        let t = Tables::new(&w, &directions(64)).unwrap();
        assert_eq!((t.order(), t.opaque(), t.rays(), t.triangles()), (&[2, 3, 4, 5, 0, 1][..], 4, 64, 6));
        assert_eq!((t.faces.len(), t.corners.len(), t.corners[12]), (6 * FACE, 18, [0.0; 3]));
        // The first material's texture is there, the second's is missing.
        assert_eq!([t.data[11], t.data[MATERIAL + 11], t.data[2 * MATERIAL + 11]], [1, NONE, NONE]);
        assert_eq!((t.data[MATERIAL + 12], t.data[12]), (2 | 4 | 8 | 16, 1));
        assert_eq!(t.starts(), [48, 48 + 16, 64 + 12, 76 + 256], "one row of colours");
        let row = &t.data[t.at[0] as usize + TEXTURE..][..TEXTURE];
        assert_eq!(row[..7], [4 * 64, 128, 128, 0, 8, 8, 1 | 2 << 2 | 1 << 4]);
        assert_eq!((t.texels.len(), t.texels[9]), (64 + 128 * 128 / 4, u32::from_le_bytes([9, 2, 3, 10])));
        assert_eq!(t.texels[64 + 1].to_le_bytes(), [5, 6, 7, 8]);
        assert_eq!(t.faces[FACE], 1 | UV_ZERO, "the second triangle's of that material");
        assert_eq!(t.faces[0], 1);
        assert_eq!([0, 4, 5].map(|k| t.faces[k * FACE + 20]), [2, 0, 1], "their numbers");
        assert!((f32::from_bits(t.data[t.at[2] as usize + 255]) - 1.0).abs() < 1e-6);
        assert_eq!((t.data.len(), t.bytes()), (t.at[3] as usize + DIRECTION * 64, 4 * (18 * 3 + 6 * FACE + t.data.len() + t.texels.len()) as u64));
        // The fifth direction: as given, as the rays go (the same, none of its axes is still), its axes and reciprocal.
        let (row, d) = (&t.data[t.at[3] as usize + 5 * DIRECTION..][..DIRECTION], directions(64)[5]);
        let z = (row[AXES] >> 4 & 3) as usize;
        assert!((0..3).all(|k| row[GIVEN + k] == d[k].to_bits() && row[ALONG + k] == row[GIVEN + k] && d[z].abs() >= d[k].abs()));
        assert!([SHEAR + 2, INVERSE + z].map(|k| f32::from_bits(row[k])) == [1.0 / d[z]; 2] && row[AXES] >> 6 == 0);
    }

    /// A grazing direction's row, where the one given and the one the rays go differ; the last texel of each level.
    #[test]
    fn rows_at_their_ends() {
        let w = made(vec![cut()], vec![None, Some(texture())], |_| ());
        let t = Tables::new(&w, &[V3::new(0.0, 0.6, 0.8), V3::new(1.0, 1e-30, 0.0)]).unwrap();
        let row = &t.data()[t.starts()[3] as usize + DIRECTION..][..DIRECTION];
        let f = |k: usize| f32::from_bits(row[k]);
        assert_eq!([f(GIVEN), f(GIVEN + 1), f(GIVEN + 2)], [1.0, 1e-30, 0.0], "as given, for the moments");
        assert_eq!([f(ALONG), f(ALONG + 1), f(ALONG + 2)], [1.0, 0.0, 0.0], "as the rays go");
        assert_eq!([f(SHEAR), f(SHEAR + 1), f(SHEAR + 2)], [0.0, 0.0, 1.0]);
        assert_eq!((row[AXES] & 63, row[AXES] >> 6), (1 | 2 << 2, 0b110), "axes y, z, x; y and z still");
        assert_eq!([f(INVERSE), f(INVERSE + 1), f(INVERSE + 2)], [1.0, 0.0, 0.0]);
        assert_eq!(t.data().len(), t.starts()[3] as usize + 2 * DIRECTION);
        // The colour level (8 x 8) from its first word, the alpha level (128 x 128) a byte each from its first byte.
        let texture = &t.data()[t.starts()[0] as usize + TEXTURE..][..TEXTURE];
        let (colour, alpha) = (texture[3] as usize, texture[0] as usize);
        assert_eq!((t.texels()[colour + 63], t.texels().len()), (u32::from_le_bytes([63, 2, 3, 64]), 64 + 128 * 128 / 4));
        let byte = |i: usize| t.texels()[(alpha + i) / 4].to_le_bytes()[(alpha + i) % 4];
        assert_eq!([byte(0), byte(128 * 128 - 1)], [1, (16383u32 as u8).wrapping_add(1)]);
        // A level of a size that is no multiple of four: its last alpha in a word of its own, the rest of it 0.
        let odd = Texture { levels: vec![Level { width: 3, height: 3, texels: (0..9).map(|i| [i, 0, 0, 10 + i]).collect() }], srgb: false, wrap: [Wrap::Repeat; 2] };
        let t = Tables::new(&made(vec![cut()], vec![None, Some(odd)], |_| ()), &directions(8)).unwrap();
        let texture = &t.data()[t.starts()[0] as usize + TEXTURE..][..TEXTURE];
        assert_eq!((texture[..6].to_vec(), t.texels().len()), (vec![4 * 9, 3, 3, 0, 3, 3], 9 + 3));
        assert_eq!((t.texels()[8], t.texels()[11]), (u32::from_le_bytes([8, 0, 0, 18]), 18));
    }

    #[test]
    fn what_is_refused() {
        let of = |m: Material, edit: &dyn Fn(&mut Mesh)| Tables::new(&made(vec![m], vec![None, Some(texture())], edit), &directions(8)).map(|t| t.opaque);
        assert_eq!(of(cut(), &|_| ()), Ok(0));
        assert!(of(cut(), &|m| m.positions[0][0] = 2e5).is_err());
        assert!([1e11, -0.1].iter().all(|c| of(Material { base: [*c, 1.0, 1.0, 1.0], ..cut() }, &|_| ()).is_err()));
        // The solver's own colour that overflows: black there, refused here.
        let overflow = Material { unlit: true, base: [f32::MAX, 0.5, 0.5, 1.0], ..Material::default() };
        assert!(of(overflow, &|_| ()).is_err() && of(Material::default(), &|m| m.colors = vec![[f32::MAX, 1.0, 1.0, 1.0]; 4]).is_err());
        // An albedo at 2^80 and past it: a display colour of 2^25 (2^60 at most, as linear) times a factor of 2^20 and
        // of the float above; a factor alone is refused past 1e10 before.
        let shown = |f: f32| Material { base: [1.0, 1.0, f, 1.0], display_colors: true, ..Material::default() };
        let bright = |m: &mut Mesh| m.colors = vec![[1.0, 1.0, 2f32.powi(25), 1.0]; 4];
        assert_eq!(of(shown(2f32.powi(20)), &bright), Ok(2));
        assert!(of(shown(2f32.powi(20).next_up()), &bright).is_err());
        // A coordinate too far, read; one that nothing reads.
        assert!(of(cut(), &|m| m.uv0[0][0] = 2e5).is_err() && of(Material { uv: [[1e6, 0.0, 0.0], [0.0, 1.0, 0.0]], ..cut() }, &|m| m.uv0[0][0] = 1.0).is_err());
        assert_eq!(of(Material { texture: None, ..cut() }, &|m| m.uv0[0][0] = 2e5), Ok(0));
        // 3u - v is 0 at each corner, u 2^48 and the next f32, v 3u rounded; between them it is not.
        let u = [2f32.powi(48), f32::from_bits(2f32.powi(48).to_bits() + 1)];
        let corners = move |m: &mut Mesh| m.uv0 = [0, 1, 0, 1].map(|i| [u[i], 3.0 * u[i]]).to_vec();
        assert!(of(Material { uv: [[3.0, -1.0, 0.0], [0.0; 3]], ..cut() }, &corners).is_err());
        assert_eq!(of(Material { uv: [[3.0, -1.0, 0.0], [0.0; 3]], ..cut() }, &|m| m.uv0 = vec![[1e4, 3e4]; 4]), Ok(0));
        assert!(of(Material { uv: [[3.0, -1.0, 0.0], [0.0; 3]], ..cut() }, &|m| m.uv0 = vec![[3e4, 9e4]; 4]).is_err(), "3 x 3e4 and 9e4");
        // A blend of the largest coordinates is past f32's end before a small transform brings it back.
        let tiny = Material { uv: [[2f32.powi(-112), 0.0, 0.0], [0.0; 3]], ..cut() };
        assert!(of(tiny, &|m| m.uv0 = vec![[f32::MAX, 0.0]; 4]).is_err() && of(tiny, &|m| m.uv0 = vec![[1e30, 0.0]; 4]) == Ok(0));
        // An alpha past the end of f32 stops as one that is no number does; where it is so in part there is no telling.
        let alpha = |a: [f32; 4]| move |m: &mut Mesh| m.colors = a.map(|a| [1.0, 1.0, 1.0, a]).to_vec();
        let large = Material { base: [1.0, 1.0, 1.0, 1e3], ..cut() };
        assert_eq!(of(large, &alpha([3e38; 4])), Ok(2));
        assert!(of(large, &alpha([3e38, 1.0, 3e38, 1.0])).is_err() && of(large, &alpha([-3e38; 4])).is_err());
        let w = made(vec![cut()], vec![None, Some(texture())], |_| ());
        assert!(Tables::new(&w, &[]).is_err() && Tables::new(&w, &[V3::ZERO]).is_err() && Tables::new(&w, &[V3([1e20; 3])]).is_err());
        assert_eq!(Tables::until(&w, &directions(8), &|| true), Err("cancelled".into()));
        let none = prepare(SceneInput::default(), &SceneLimits::default(), 1, &|| false).unwrap();
        assert!(Tables::new(&none, &directions(8)).is_err());
        assert!([vec![], vec![V3([f32::NAN; 3])], vec![V3([2e5; 3])]].into_iter().all(|p| Points::bare(p).words().is_err()));
        assert_eq!(Points { texels: vec![[3, 4, 5]], ..Points::bare(vec![V3::new(1.0, 2.0, 3.0)]) }.words().unwrap()[3..], [3 | 4 << 16, 5, 8, 0, 0]);
        // At a corner of the first, on the edge of both, and far from them, of a tested sheet, which comes last.
        let sheets = made(vec![cut(), Material::default()], vec![None, Some(texture())], |_| ());
        let tables = Tables::new(&sheets, &directions(8)).unwrap();
        let points = Points::at(vec![V3::ZERO, V3::new(4.0, 4.0, 0.2), V3([50.0; 3])], &sheets, &tables).unwrap();
        let near = |i: usize| (points.near[7 * i], [1, 2, 3, 4, 5, 6].map(|k| f32::from_bits(points.near[7 * i + k])));
        assert_eq!((points.lists.clone(), points.near(), points.near.len()), (vec![[0, 1], [7, 2], [21, 0]], [2, 3, 2], 21));
        // Boxes of leaves: of these and perhaps more.
        assert!([2, 2, 3].iter().enumerate().all(|(i, k)| near(i).0 == *k && near(i).1[..3].iter().all(|c| *c <= 0.0) && near(i).1[3] == 8.0));
        assert_eq!(points.words().unwrap()[21..25], [0, 1, 2, 2]);
        assert!(Points::at(vec![V3::new(0.0, 0.0, 1.0)], &sheets, &Tables::new(&w, &directions(8)).unwrap()).is_err(), "tables of another scene");
    }

    #[test]
    fn lights_in_words() {
        let lamp = |x: f32| Lamp { position: V3::new(x, 0.0, 0.0), display: V3([0.5; 3]), range: 100.0, cone: None };
        let spot = Lamp { cone: Some(Cone::new(V3::new(0.0, -1.0, 0.0), 0.2, 0.4)), ..lamp(900.0) };
        let light = LightState { ambient: V3([0.1; 3]), directionals: vec![Directional { display: V3([0.2; 3]), to_source: V3::new(0.0, 1.0, 0.0) }], lamps: vec![lamp(0.0), spot] };
        let packed = PackedLight::of(&light).unwrap();
        let (w, t) = (packed.words(), LampTable::of(&light, &|| false).unwrap());
        assert_eq!(packed.most_lamps(), 1, "the spheres are far apart");
        assert_eq!((w[3], w[11], w[12], w[13], w[14]), (1, 2, 16, 22, 54));
        assert_eq!((&w[8..11], w[15] as usize, w.len()), (&t.cells[..], 54 + t.start.len(), 54 + t.start.len() + t.lamps.len()));
        assert_eq!((w[7], &w[54..w[15] as usize], &w[w[15] as usize..]), (t.side.to_bits(), &t.start[..], &t.lamps[..]));
        assert_eq!([w[22 + 7], w[38 + 7]], [0, 1]);
        assert_eq!(w[38 + 8..38 + 13], spot.cone.unwrap().numbers().map(f32::to_bits));
        assert!([1e7, 6e5].map(|b| PackedLight::of(&LightState { ambient: V3([b; 3]), lamps: vec![Lamp { display: V3([-b; 3]), ..lamp(0.0) }], ..light.clone() }).is_err()) == [true; 2]);
        // What a solve refuses: a lamp past 2^21, a spot of no direction, a number that is none.
        for lamps in [vec![lamp(2_097_153.0)], vec![Lamp { cone: Some(Cone::new(V3::ZERO, 0.2, 0.4)), ..lamp(0.0) }]] {
            assert!(PackedLight::of(&LightState { lamps, ..light.clone() }).is_err());
        }
        assert!(PackedLight::of(&LightState { ambient: V3([f32::NAN; 3]), ..light.clone() }).is_err());
        // A range whose square is 0, at the place a ray meets: the solver's light there is no number, and it counts a
        // shadow ray; refused here. One below 1e-6 too; none at all, or one of 1e-6 and more, is not.
        let w = made(vec![Material::default()], Vec::new(), |_| ());
        let at = |range: f32| LightState { lamps: vec![Lamp { range, ..lamp(0.0) }], ..light.clone() };
        let coincident = LightState { lamps: vec![Lamp { position: V3::new(2.0, 2.0, 0.0), range: 1e-24, ..lamp(0.0) }], ..light.clone() };
        let b = nvf_rtgi::bounces(&w, &coincident, &[(V3::new(2.0, 2.0, 5.0), V3::new(0.0, 0.0, -1.0))], 0.25, 1, &|| false).unwrap()[0].unwrap();
        assert!(b.p == V3::new(2.0, 2.0, 0.0) && b.shadow_rays == 1 && PackedLight::of(&coincident).is_err());
        assert!([1e-24, 2e-19, LEAST_RANGE.next_down()].iter().all(|r| PackedLight::of(&at(*r)).is_err()) && [0.0, -1.0, LEAST_RANGE, 1e-3].iter().all(|r| PackedLight::of(&at(*r)).is_ok()));
    }

    /// The points of a solve where its texture has them, with the triangles near them.
    #[test]
    fn the_points_of_a_volume() {
        // Two sheets seen from either side, 0.3 below and above the points of the first and the last row.
        let mesh = |z: f32| Mesh { positions: vec![[-200.0, -200.0, z], [200.0, -200.0, z], [200.0, 200.0, z], [-200.0, 200.0, z]], indices: vec![0, 1, 2, 0, 2, 3], ..Mesh::default() };
        let input = SceneInput {
            instances: (0..2).map(|i| Instance { mesh: i, material: 0, world_from_local: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]] }).collect(),
            meshes: vec![mesh(-0.3), mesh(128.3)],
            materials: vec![Material { cull: Cull::None, double_sided: true, ..Material::default() }],
            textures: Vec::new(),
        };
        let scene = prepare(input, &SceneLimits::default(), 1, &|| false).unwrap();
        let light = LightState { lamps: vec![Lamp { position: V3::new(0.0, 0.0, 64.0), display: V3([0.8; 3]), range: 300.0, cone: None }], ..LightState::default() };
        let config = SolveConfig { gather: GatherConfig { rays: 64, ..GatherConfig::default() }, ..SolveConfig::default() };
        let volume = solve(&scene, &light, &Seeds::at(vec![V3::new(10.0, 10.0, 64.0)]), &config, &|| false).unwrap();
        let tables = Tables::new(&scene, &directions(64)).unwrap();
        let points = Points::of(&volume, &scene, &tables).unwrap();
        let of = volume.points().unwrap();
        assert!(points.positions().iter().eq(of.iter().map(|p| &p.position)) && points.layout() == (volume.slab_step(), volume.atlas));
        let words = points.words().unwrap();
        for (i, p) in of.iter().enumerate() {
            assert_eq!(words[PLACE * i + 3..PLACE * i + 5], [p.texel[0] | p.texel[1] << 16, p.texel[2]]);
        }
        // The points beside the sheets have a triangle near them, both on the diagonal; those between have none.
        let beside = of.iter().filter(|p| p.position[2] == 0.0 || p.position[2] == 128.0).count();
        let [with, all, most] = points.near();
        assert!(beside >= 49 && with == beside && all > beside && all < 2 * beside && most == 2, "{:?} {beside}", points.near());
    }
}
