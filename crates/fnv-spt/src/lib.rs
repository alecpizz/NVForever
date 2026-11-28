//! SpeedTree `.spt` files, the models of TREE records (docs/spec-notes/speedtree.md).
//!
//! An `.spt` file is a parameter set, not a mesh: a flat run of tagged entries whose
//! payload type depends on the tag (§2-§3). [`parse_entries`] reads that run, [`Tree`]
//! picks out what the generator uses, and [`generate`] builds NVForever's own branch
//! mesh and leaf cards from it (§5).

pub mod curve;
pub mod generate;
pub mod probe;

pub use curve::Curve;
pub use generate::{canopy_normals, generate, Geometry, Leaf};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SptError {
    #[error("unknown tag {tag} at offset {offset:#x}")]
    UnknownTag { tag: u32, offset: usize },
    #[error("entry with tag {tag} at offset {offset:#x} runs past the end of the file")]
    Truncated { tag: u32, offset: usize },
    #[error("not a SpeedTree file (first entry is not the 1000 magic string)")]
    BadMagic,
    #[error("tag {tag}: {msg}")]
    Bad { tag: u32, msg: String },
}

/// Payload of one entry (§2).
#[derive(Debug, Clone, PartialEq)]
pub enum Payload {
    None,
    U8(u8),
    /// A 4-byte value: an integer or a float depending on the tag (§3).
    Four([u8; 4]),
    Str(String),
    Floats(Vec<f32>),
    /// u32 count, then count × 8 floats (tag 10002).
    UvList(Vec<[f32; 8]>),
}

impl Payload {
    pub fn u32(&self) -> Option<u32> {
        match self {
            Payload::Four(b) => Some(u32::from_le_bytes(*b)),
            _ => None,
        }
    }
    pub fn f32(&self) -> Option<f32> {
        match self {
            Payload::Four(b) => Some(f32::from_le_bytes(*b)),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            Payload::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn floats(&self) -> Option<&[f32]> {
        match self {
            Payload::Floats(v) => Some(v),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub tag: u32,
    /// Byte offset of the tag.
    pub offset: usize,
    pub payload: Payload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    None,
    U8,
    Four,
    Str,
    Floats(usize),
    UvList,
}

/// Payload type of each tag, as observed in all ten files (§3).
fn kind(tag: u32) -> Option<Kind> {
    use Kind::*;
    Some(match tag {
        1000 | 2000 | 4003 | 13005 | 14002 | 18005 | 6000..=6007 | 6017 => Str,
        1006 | 1014 => Four,
        1001..=1017 => None,
        2002 => U8,
        2001 | 2003..=2007 => Four,
        3003 | 3006 | 3009 => U8,
        3000..=3010 => Four,
        4000 => U8,
        4001 | 4004..=4006 => Floats(3),
        4002 | 4007 => Four,
        5000..=5004 => Floats(3),
        5005 => Four,
        5006 => U8,
        6015 | 6016 => U8,
        6008..=6014 => Four,
        8000 | 8001 => None,
        8003 | 8005 | 8009 => Floats(13),
        8002 | 8004 | 8006..=8008 => Four,
        9000 | 9001 | 9005 | 9006 => None,
        9002..=9014 => Four,
        10000 | 10001 => None,
        10002 => UvList,
        10003 | 10004 => Four,
        11000 | 11001 => None,
        11002 => Four,
        12000 | 12001 => None,
        12002 => Floats(4),
        12003 => Floats(5),
        13000 => None,
        13007 => U8,
        13002..=13013 => Four,
        14000 => None,
        14001 | 14003..=14008 => Four,
        15000 | 15001 => None,
        15002 => U8,
        15003 => Four,
        16000 | 16001 => None,
        16002..=16014 => Four,
        18000 | 18001 => None,
        18002..=18004 => Floats(3),
        19000 | 19001 => None,
        19002 => Four,
        20000 | 20001 => None,
        20002 => Four,
        20003 | 20004 => U8,
        20005 => Floats(8),
        21000 | 21001 => Four,
        22000 => U8,
        _ => return Option::None,
    })
}

/// Every entry of a file, in order. Fails on a tag outside the table or a payload that
/// runs past the end.
pub fn parse_entries(data: &[u8]) -> Result<Vec<Entry>, SptError> {
    let mut out = Vec::new();
    let mut o = 0;
    let u32_at = |o: usize| data.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    while o < data.len() {
        let offset = o;
        let tag = u32_at(o).ok_or(SptError::Truncated { tag: 0, offset })?;
        o += 4;
        let k = kind(tag).ok_or(SptError::UnknownTag { tag, offset })?;
        let short = SptError::Truncated { tag, offset };
        let take = |o: usize, n: usize| data.get(o..o + n).ok_or(short.clone());
        let payload = match k {
            Kind::None => Payload::None,
            Kind::U8 => {
                let b = take(o, 1)?[0];
                o += 1;
                Payload::U8(b)
            }
            Kind::Four => {
                let b = take(o, 4)?.try_into().unwrap();
                o += 4;
                Payload::Four(b)
            }
            Kind::Str => {
                let n = u32_at(o).ok_or(short.clone())? as usize;
                let s = take(o + 4, n)?;
                o += 4 + n;
                Payload::Str(s.iter().map(|&c| c as char).collect())
            }
            Kind::Floats(n) => {
                let b = take(o, 4 * n)?;
                o += 4 * n;
                Payload::Floats(b.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect())
            }
            Kind::UvList => {
                let n = u32_at(o).ok_or(short.clone())? as usize;
                let b = take(o + 4, n.checked_mul(32).ok_or(short.clone())?)?;
                o += 4 + 32 * n;
                let f: Vec<f32> = b.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
                Payload::UvList(f.as_chunks::<8>().0.to_vec())
            }
        };
        out.push(Entry { tag, offset, payload });
    }
    Ok(out)
}

/// One branch level (§4). `curves[i]` is tag 6000 + i.
#[derive(Debug, Clone, PartialEq)]
pub struct Level {
    pub curves: [Curve; 8],
    /// Segments along the branch (6008).
    pub segments_along: u32,
    /// Segments around the branch (6009).
    pub segments_around: u32,
    /// Children are spread between these positions along the branch, 0-1 (6010, 6011).
    pub first: f32,
    pub last: f32,
    /// Children per unit of branch length (6012) [hypothesis].
    pub frequency: f32,
    /// Bark texture repeats around and along the branch (6013, 6014) [hypothesis].
    pub texture_repeat: [f32; 2],
}

impl Level {
    pub fn length(&self) -> &Curve {
        &self.curves[4]
    }
    pub fn radius(&self) -> &Curve {
        &self.curves[5]
    }
    pub fn taper(&self) -> &Curve {
        &self.curves[6]
    }
    pub fn angle(&self) -> &Curve {
        &self.curves[7]
    }
    pub fn bend(&self) -> &Curve {
        &self.curves[0]
    }
}

/// One leaf map (1007 … 1008).
#[derive(Debug, Clone, PartialEq)]
pub struct LeafMap {
    /// Texture path as authored (4003); the game reads one combined texture instead (§6).
    pub texture: String,
    /// Attachment point as a fraction of the card's width and height from the bottom left (4004).
    pub pivot: [f32; 2],
    /// Card width and height as a fraction of the tree size (4005).
    pub size: [f32; 2],
}

/// The parameters the generator uses.
#[derive(Debug, Clone, PartialEq)]
pub struct Tree {
    /// Bark texture path as authored (2000).
    pub bark_texture: String,
    /// Random seed (2005; the TREE record's SNAM).
    pub seed: u32,
    /// Branch levels, trunk first; the last level describes the leaves.
    pub levels: Vec<Level>,
    pub leaf_maps: Vec<LeafMap>,
    /// Per leaf map: the (u, v) corners of its cell in the combined leaf texture (10002).
    pub leaf_uvs: Vec<[f32; 8]>,
}

impl Tree {
    pub fn parse(data: &[u8]) -> Result<Tree, SptError> {
        Tree::from_entries(&parse_entries(data)?)
    }

    pub fn from_entries(entries: &[Entry]) -> Result<Tree, SptError> {
        if entries.first().and_then(|e| (e.tag == 1000).then(|| e.payload.str()).flatten()) != Some("__IdvSpt_02_") {
            return Err(SptError::BadMagic);
        }
        let bad = |tag: u32, msg: &str| SptError::Bad { tag, msg: msg.into() };
        let mut tree = Tree { bark_texture: String::new(), seed: 0, levels: Vec::new(), leaf_maps: Vec::new(), leaf_uvs: Vec::new() };
        let mut level: Option<LevelBuild> = None;
        let mut leaf: Option<LeafMap> = None;
        for e in entries {
            let p = &e.payload;
            match e.tag {
                2000 => tree.bark_texture = p.str().unwrap_or_default().to_string(),
                2005 => tree.seed = p.u32().unwrap_or(0),
                1016 => level = Some(LevelBuild::default()),
                1017 => {
                    let l = level.take().ok_or(bad(1017, "level end without a start"))?;
                    tree.levels.push(l.finish()?);
                }
                1007 => leaf = Some(LeafMap { texture: String::new(), pivot: [0.5, 0.0], size: [0.0; 2] }),
                1008 => tree.leaf_maps.push(leaf.take().ok_or(bad(1008, "leaf map end without a start"))?),
                10002 => {
                    if let Payload::UvList(v) = p {
                        tree.leaf_uvs = v.clone();
                    }
                }
                t @ 6000..=6017 => {
                    if let Some(l) = level.as_mut() {
                        l.set(t, p)?;
                    }
                }
                t @ 4003..=4005 => {
                    if let Some(m) = leaf.as_mut() {
                        match (t, p) {
                            (4003, Payload::Str(s)) => m.texture = s.clone(),
                            (4004, Payload::Floats(f)) => m.pivot = [f[0], f[1]],
                            (4005, Payload::Floats(f)) => m.size = [f[0], f[1]],
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        if tree.levels.len() < 2 {
            return Err(bad(1014, "fewer than two branch levels"));
        }
        Ok(tree)
    }
}

#[derive(Default)]
struct LevelBuild {
    curves: [Option<Curve>; 8],
    four: [Option<[u8; 4]>; 7],
}

impl LevelBuild {
    fn set(&mut self, tag: u32, p: &Payload) -> Result<(), SptError> {
        match (tag, p) {
            (6000..=6007, Payload::Str(s)) => {
                let c = Curve::parse(s).map_err(|msg| SptError::Bad { tag, msg })?;
                self.curves[(tag - 6000) as usize] = Some(c);
            }
            (6008..=6014, Payload::Four(b)) => self.four[(tag - 6008) as usize] = Some(*b),
            _ => {}
        }
        Ok(())
    }

    fn finish(self) -> Result<Level, SptError> {
        let missing = |tag: u32| SptError::Bad { tag, msg: "missing in a branch level".into() };
        let mut curves = Vec::with_capacity(8);
        for (i, c) in self.curves.into_iter().enumerate() {
            curves.push(c.ok_or(missing(6000 + i as u32))?);
        }
        let four = |i: usize| self.four[i].ok_or(missing(6008 + i as u32));
        let u = |i: usize| four(i).map(u32::from_le_bytes);
        let f = |i: usize| four(i).map(f32::from_le_bytes);
        Ok(Level {
            curves: curves.try_into().unwrap(),
            segments_along: u(0)?,
            segments_around: u(1)?,
            first: f(2)?,
            last: f(3)?,
            frequency: f(4)?,
            texture_repeat: [f(5)?, f(6)?],
        })
    }
}

#[cfg(test)]
mod tests;
