//! The character creator's data (docs/spec-notes/character-creator.md): what the player
//! may choose (races, hairs, eyes, beards, hair colours, the face presets) and the named
//! sliders, each a direction in one of the three FaceGen coefficient spaces.
//!
//! **Sliders**: a FaceGen face is 50 symmetric shape values (FGGS), 30 asymmetric ones
//! (FGGA) and 50 texture values (FGTS) [documented: FaceGen SDK manual, "Overview"]. The
//! game's controls file (`facegen\si.ctl`, [`Ctl`]) names directions in those spaces
//! ("Nose - short / long", "Skin Shade Dark / Light") and an age control; a slider reads
//! direction · coefficients (+ the age control's offset) and moves the coefficients
//! along the direction [hypothesis: FaceGen's convention; §3 of the note].
//!
//! A slider reads the face's own values plus its race's (what is drawn, actors.md §4.2,
//! §4.3) and moves the own values only; its range is the authored faces' spread along it
//! ([`spread`]).

use std::collections::{BTreeMap, HashMap};

use fnv_sim::FormId;
use rand_core::RngCore;
use serde::Serialize;

use crate::facegen::Ctl;
use crate::records::{ActorForms, FaceGen};
use crate::resolve::Look;

/// Which coefficients a slider moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, PartialOrd, Ord)]
pub enum Space {
    /// FGGS, 50 symmetric shape values.
    Shape,
    /// FGGA, 30 asymmetric shape values.
    Asymmetry,
    /// FGTS, 50 texture values.
    Skin,
}

impl Space {
    pub fn values(self) -> usize {
        match self {
            Space::Shape | Space::Skin => 50,
            Space::Asymmetry => 30,
        }
    }

    pub fn of(self, fg: &FaceGen) -> &Vec<f64> {
        match self {
            Space::Shape => &fg.fggs,
            Space::Asymmetry => &fg.fgga,
            Space::Skin => &fg.fgts,
        }
    }

    pub fn of_mut(self, fg: &mut FaceGen) -> &mut Vec<f64> {
        match self {
            Space::Shape => &mut fg.fggs,
            Space::Asymmetry => &mut fg.fgga,
            Space::Skin => &mut fg.fgts,
        }
    }
}

/// A FaceGen set with every array at its full length (missing values 0).
pub fn full(fg: &FaceGen) -> FaceGen {
    let pad = |v: &[f64], n: usize| (0..n).map(|i| v.get(i).copied().unwrap_or(0.0)).collect::<Vec<f64>>();
    FaceGen { fggs: pad(&fg.fggs, 50), fgga: pad(&fg.fgga, 30), fgts: pad(&fg.fgts, 50) }
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

// ---- the sliders ----------------------------------------------------------------------------

/// A named slider: one control of the game's controls file, or its age control.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Slider {
    /// The file's label ("Nose - bridge shallow / deep", "Skin Shade Dark / Light").
    pub label: String,
    /// What the label is about ("Nose", "Skin") and which aspect ("bridge", "Shade").
    pub group: String,
    pub aspect: String,
    /// The words at the slider's two ends, the low end first.
    pub low: String,
    pub high: String,
    /// The coefficients it moves: per space a direction (the age control moves shape and
    /// texture together); the reading is the first part's.
    pub parts: Vec<(Space, Vec<f64>)>,
    pub offset: f64,
    /// The range the authored faces span along it (own + race values; [`spread`]).
    pub range: (f64, f64),
}

/// Split a control's label into group, aspect and the two ends [design: the file's
/// labels read "Group - aspect low / high" for shape controls and "Group aspect low /
/// high" for texture controls, observed on `si.ctl`]. The low end has as many words as
/// the high end ("Eyes Dark Brown / Light Blue": group Eyes, ends "Dark Brown", "Light
/// Blue"); a "Very" before it belongs to it.
pub fn split_label(label: &str) -> (String, String, String, String) {
    let (group, rest) = match label.split_once(" - ") {
        Some((g, r)) => (g.trim().to_string(), r.trim().to_string()),
        None => match label.split_once(' ') {
            Some((g, r)) => (g.to_string(), r.trim().to_string()),
            None => (label.to_string(), String::new()),
        },
    };
    let Some((left, right)) = rest.split_once(" / ") else {
        return (group, rest, "−".into(), "+".into());
    };
    let n = right.split_whitespace().count().max(1);
    let words: Vec<&str> = left.split_whitespace().collect();
    let mut cut = words.len().saturating_sub(n);
    if cut > 0 && words[cut - 1].eq_ignore_ascii_case("very") {
        cut -= 1;
    }
    (group, words[..cut].join(" "), words[cut..].join(" "), right.trim().to_string())
}

impl Slider {
    fn new(label: &str, parts: Vec<(Space, Vec<f64>)>, offset: f64) -> Slider {
        let (group, aspect, low, high) = split_label(label);
        Slider { label: label.to_string(), group, aspect, low, high, parts, offset, range: (0.0, 0.0) }
    }

    pub fn space(&self) -> Space {
        self.parts[0].0
    }

    /// The reading: direction · (own + race values) + offset [hypothesis: FaceGen's
    /// linear controls read so, docs/spec-notes/character-creator.md §3].
    pub fn value(&self, own: &FaceGen, race: Option<&FaceGen>) -> f64 {
        let (space, dir) = &self.parts[0];
        let r = race.map_or(0.0, |r| dot(space.of(r), dir));
        dot(space.of(own), dir) + r + self.offset
    }

    /// Set the reading to `v`, moving the own values along each part's direction by the
    /// same change of reading.
    pub fn set(&self, own: &mut FaceGen, race: Option<&FaceGen>, v: f64) {
        let by = v - self.value(own, race);
        for (space, dir) in &self.parts {
            let norm = dot(dir, dir);
            if norm < 1e-12 {
                continue;
            }
            let c = space.of_mut(own);
            c.resize(space.values().max(c.len()), 0.0);
            for (x, d) in c.iter_mut().zip(dir) {
                *x += by * d / norm;
            }
        }
    }

    /// The reading as a fraction of the range, 0 to 1 (clamped).
    pub fn fraction(&self, own: &FaceGen, race: Option<&FaceGen>) -> f64 {
        let (lo, hi) = self.range;
        if hi <= lo {
            return 0.5;
        }
        ((self.value(own, race) - lo) / (hi - lo)).clamp(0.0, 1.0)
    }

    pub fn set_fraction(&self, own: &mut FaceGen, race: Option<&FaceGen>, f: f64) {
        let (lo, hi) = self.range;
        self.set(own, race, lo + f.clamp(0.0, 1.0) * (hi - lo));
    }
}

fn widen(dir: &[f32]) -> Vec<f64> {
    dir.iter().map(|&x| f64::from(x)).collect()
}

/// The sliders of a controls file: its age control, then the symmetric shape,
/// asymmetric shape and texture controls in its order. A label the file repeats
/// ("Nose - sellion shallow / deep" twice [observed]) is kept once.
pub fn sliders(ctl: &Ctl) -> Vec<Slider> {
    let mut out = Vec::new();
    if let Some(age) = &ctl.age {
        out.push(Slider::new(AGE_LABEL, vec![(Space::Shape, widen(&age.shape)), (Space::Skin, widen(&age.texture))], f64::from(age.shape_offset)));
    }
    for (space, list) in [(Space::Shape, &ctl.shape), (Space::Asymmetry, &ctl.asymmetry), (Space::Skin, &ctl.texture)] {
        for c in list {
            if out.iter().any(|s: &Slider| s.label == c.label && s.space() == space) {
                continue;
            }
            out.push(Slider::new(&c.label, vec![(space, widen(&c.dir))], 0.0));
        }
    }
    out
}

/// Set each slider's range from authored faces (own + race values): from the 1st to the
/// 99th percentile of their readings, widened by a quarter of that span on each side
/// [design: room past the authored faces, without reaching faces the statistics never
/// saw]; ±3 about the offset when there are fewer than 20 faces.
pub fn spread(sliders: &mut [Slider], faces: &[FaceGen]) {
    for s in sliders {
        let mut v: Vec<f64> = faces.iter().map(|f| s.value(f, None)).collect();
        if v.len() < 20 {
            s.range = (s.offset - 3.0, s.offset + 3.0);
            continue;
        }
        v.sort_by(f64::total_cmp);
        let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
        let (lo, hi) = (at(0.01), at(0.99));
        let pad = (hi - lo).max(0.5) * 0.25;
        s.range = (lo - pad, hi + pad);
        match s.space() {
            // The authored faces have next to no asymmetry (98% none, actors.md §4.2):
            // their spread would leave no room [design: at least ±2 standard deviations].
            Space::Asymmetry => s.range = (s.range.0.min(-ASYMMETRY_REACH), s.range.1.max(ASYMMETRY_REACH)),
            // No younger than an adult [design].
            _ if s.label == AGE_LABEL => s.range.0 = s.range.0.max(MIN_AGE),
            _ => {}
        }
    }
}

/// The least reach of an asymmetry slider either way, standard deviations [design].
pub const ASYMMETRY_REACH: f64 = 2.0;
/// The age slider's own label, and its lowest reading (years, as the control counts
/// them [hypothesis: its offset of 29.7 is the mean age of FaceGen's sample]).
pub const AGE_LABEL: &str = "Age - young / old";
pub const MIN_AGE: f64 = 16.0;

impl Creator {
    /// Raise `look`'s age reading to [`MIN_AGE`] when it is younger, along the age
    /// control's own directions (as the slider moves it). The reading before, when raised.
    pub fn adult(&self, look: &mut Look) -> Option<f64> {
        let sex = usize::from(look.female);
        let age = self.sliders[sex].iter().find(|s| s.label == AGE_LABEL)?;
        let race = self.catalogue.race(look.race).and_then(|r| r.facegen[sex].as_ref());
        let was = age.value(&look.facegen, race);
        if was >= MIN_AGE {
            return None;
        }
        look.facegen = full(&look.facegen);
        age.set(&mut look.facegen, race, MIN_AGE);
        Some(was)
    }
}

/// The controls file [observed: `facegen\si.ctl` in `Fallout - Misc.bsa`].
pub const CTL_PATH: &str = "facegen\\si.ctl";

/// Everything the creator works from: the catalogue, and the sliders with their ranges
/// for each sex (index 0 male, 1 female).
#[derive(Debug, Clone, Default)]
pub struct Creator {
    pub catalogue: Catalogue,
    pub sliders: [Vec<Slider>; 2],
    /// Why the controls file gave no sliders, if it did not.
    pub ctl_problem: Option<String>,
    /// The player record's own look (NPC_ 7), where the creator starts the first time.
    pub player: Option<Look>,
}

impl Creator {
    /// Build from the actor records, reading the controls file with `read` (a path in the
    /// game's archives).
    pub fn build(forms: &ActorForms, read: impl Fn(&str) -> Option<Vec<u8>>) -> Creator {
        let catalogue = Catalogue::build(forms, true);
        let (base, ctl_problem) = match read(CTL_PATH).map(|b| Ctl::parse(&b)) {
            Some(Ok(ctl)) => (sliders(&ctl), None),
            Some(Err(e)) => (Vec::new(), Some(format!("{CTL_PATH}: {e}"))),
            None => (Vec::new(), Some(format!("{CTL_PATH}: not found"))),
        };
        let races: HashMap<FormId, [Option<FaceGen>; 2]> = forms.races.iter().map(|(&id, r)| (id, r.facegen.clone())).collect();
        let per_sex = |female: bool| {
            let mut s = base.clone();
            spread(&mut s, &catalogue.drawn_faces(female, &races));
            s
        };
        let sliders = [per_sex(false), per_sex(true)];
        let player = forms.npcs.get(&FormId::PLAYER_BASE).and_then(|n| {
            Some(Look {
                race: n.race?,
                female: n.female,
                hair: n.hair,
                hair_color: n.hair_color,
                eyes: n.eyes,
                head_parts: n.head_parts.clone(),
                facegen: full(n.facegen.as_ref().unwrap_or(&FaceGen::default())),
            })
        });
        Creator { catalogue, sliders, ctl_problem, player }
    }
}

// ---- what may be chosen -------------------------------------------------------------------

/// A choice in a list: a record and its name.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Choice {
    pub id: FormId,
    pub name: String,
}

/// A race the player may take, with its lists per sex (index 0 male, 1 female).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RaceChoice {
    pub id: FormId,
    pub name: String,
    pub edid: String,
    pub hairs: [Vec<Choice>; 2],
    pub eyes: [Vec<Choice>; 2],
    pub default_hair: [Option<FormId>; 2],
    /// The race's older variant (ONAM), drawn with an aged head texture (actors.md §1.1).
    pub older: Option<FormId>,
    /// The race FaceGen baseline per sex (actors.md §4.2).
    pub facegen: [Option<FaceGen>; 2],
}

/// A head part kind, by its model's file name [design: HDPT records carry no kind;
/// observed: the model names, §2.4 of the note].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PartKind {
    FacialHair,
    Brows,
    Other,
}

pub fn part_kind(model: &str) -> PartKind {
    let file = model.rsplit(['\\', '/']).next().unwrap_or(model).to_ascii_lowercase();
    if file.contains("eyebrow") || file.starts_with("brow") {
        PartKind::Brows
    } else if ["beard", "mustache", "moustache", "goatee", "sideburn", "stubble", "chops", "soulpatch"].iter().any(|k| file.contains(k)) {
        PartKind::FacialHair
    } else {
        PartKind::Other
    }
}

/// Everything the creator offers.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Catalogue {
    /// Playable adult races, by name.
    pub races: Vec<RaceChoice>,
    /// Beards and moustaches (HDPT): playable ones for the player.
    pub facial_hair: Vec<Choice>,
    /// Eyebrows (HDPT).
    pub brows: Vec<Choice>,
    /// Hair colours the authored faces use, most common first (r, g, b; how many).
    pub hair_colours: Vec<([u8; 3], usize)>,
    /// The authored faces with their race and sex, for the spread of the sliders, the
    /// random faces and the "look like" starts: (NPC, race, female, own values).
    #[serde(skip)]
    pub faces: Vec<AuthoredFace>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuthoredFace {
    pub npc: FormId,
    /// One of the race menu's starting faces (NPC_ "Is CharGen Face Preset").
    pub preset: bool,
    pub name: Option<String>,
    pub race: FormId,
    pub female: bool,
    pub facegen: FaceGen,
    pub hair: Option<FormId>,
    pub hair_color: Option<[u8; 3]>,
    pub eyes: Option<FormId>,
    pub head_parts: Vec<FormId>,
}

fn named(id: FormId, name: Option<&String>, edid: Option<&String>) -> Choice {
    Choice { id, name: name.or(edid).cloned().unwrap_or_else(|| id.to_string()) }
}

/// Quantise a colour to 16 levels a channel, for counting the authored hair colours.
fn bucket(c: [u8; 3]) -> [u8; 3] {
    c.map(|x| (x / 16) * 16 + 8)
}

impl Catalogue {
    /// Build from the actor records. `playable_parts_only`: offer only head parts whose
    /// DATA says Playable (else every beard and brow model in the masters).
    pub fn build(forms: &ActorForms, playable_parts_only: bool) -> Catalogue {
        let hair_name = |id: &FormId| forms.hairs.get(id).map(|h| named(*id, h.name.as_ref(), h.edid.as_ref()));
        let eye_name = |id: &FormId| forms.eyes.get(id).map(|e| named(*id, e.name.as_ref(), e.edid.as_ref()));
        let mut races: Vec<RaceChoice> = forms
            .races
            .iter()
            .filter(|(_, r)| r.playable && !r.child)
            .map(|(&id, r)| {
                let per_sex = |female: bool| {
                    let hairs = r.hairs.iter().filter(|h| forms.hairs.get(h).is_some_and(|x| x.choosable.for_sex(female) && x.model.is_some())).filter_map(hair_name).collect();
                    let eyes = r.eyes.iter().filter(|e| forms.eyes.get(e).is_some_and(|x| x.choosable.for_sex(female))).filter_map(eye_name).collect();
                    (hairs, eyes)
                };
                let (mh, me) = per_sex(false);
                let (fh, fe) = per_sex(true);
                RaceChoice {
                    id,
                    name: r.name.clone().or_else(|| r.edid.clone()).unwrap_or_default(),
                    edid: r.edid.clone().unwrap_or_default(),
                    hairs: [mh, fh],
                    eyes: [me, fe],
                    default_hair: r.default_hair,
                    older: r.older,
                    facegen: r.facegen.clone(),
                }
            })
            .collect();
        races.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        let mut facial_hair = Vec::new();
        let mut brows = Vec::new();
        let mut parts: Vec<_> = forms.head_parts.iter().collect();
        parts.sort_by_key(|(id, _)| **id);
        for (&id, h) in parts {
            // A head part without a model (the "BeardMix" beards) is drawn as the parts it
            // lists (HNAM) [observed: BeardMix6a "Ronin" lists three beards]: it is of their
            // kind when they all are. Starting faces wear them (CGPresetHispanicM03), so the
            // Facial hair row must know them or it cannot take them off.
            let kind = match h.model.as_deref() {
                Some(model) => part_kind(model),
                None => {
                    let kinds: Vec<PartKind> = h.extra.iter().filter_map(|e| forms.head_parts.get(e)?.model.as_deref().map(part_kind)).collect();
                    match kinds.first() {
                        Some(&k) if kinds.len() == h.extra.len() && kinds.iter().all(|x| *x == k) => k,
                        _ => continue,
                    }
                }
            };
            // The brows are not playable, yet the player's own record wears one
            // (EyebrowM [observed]): they are offered all the same [design], but not the
            // old-age ones (their Editor IDs say Old).
            let old = h.edid.as_deref().is_some_and(|e| e.to_ascii_lowercase().contains("old"));
            if playable_parts_only && !h.playable && (kind != PartKind::Brows || old) {
                continue;
            }
            match kind {
                PartKind::FacialHair => facial_hair.push(named(id, h.name.as_ref(), h.edid.as_ref())),
                PartKind::Brows => brows.push(named(id, h.name.as_ref(), h.edid.as_ref())),
                PartKind::Other => {}
            }
        }
        let mut counts: BTreeMap<[u8; 3], (usize, [u64; 3])> = BTreeMap::new();
        let mut faces = Vec::new();
        for n in forms.npcs.values() {
            if let Some(c) = n.hair_color {
                let e = counts.entry(bucket(c)).or_default();
                e.0 += 1;
                for (sum, &x) in e.1.iter_mut().zip(&c) {
                    *sum += u64::from(x);
                }
            }
            let (Some(race), Some(fg)) = (n.race, n.facegen.as_ref()) else { continue };
            if fg.fggs.len() != 50 || fg.fgts.len() != 50 || !forms.races.get(&race).is_some_and(|r| !r.child) {
                continue;
            }
            faces.push(AuthoredFace {
                npc: n.id,
                preset: n.chargen_preset,
                name: n.name.clone(),
                race,
                female: n.female,
                facegen: full(fg),
                hair: n.hair,
                hair_color: n.hair_color,
                eyes: n.eyes,
                head_parts: n.head_parts.clone(),
            });
        }
        let mut hair_colours: Vec<([u8; 3], usize)> = counts.into_values().map(|(n, sum)| (sum.map(|s| (s / n as u64) as u8), n)).collect();
        hair_colours.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        Catalogue { races, facial_hair, brows, hair_colours, faces }
    }

    pub fn race(&self, id: FormId) -> Option<&RaceChoice> {
        self.races.iter().find(|r| r.id == id)
    }

    /// The authored faces of a race and sex (all races of that sex when the race has
    /// fewer than 10).
    pub fn faces_of(&self, race: FormId, female: bool) -> Vec<&AuthoredFace> {
        let own: Vec<&AuthoredFace> = self.faces.iter().filter(|f| f.race == race && f.female == female).collect();
        if own.len() >= 10 {
            return own;
        }
        self.faces.iter().filter(|f| f.female == female).collect()
    }

    /// The starting faces of a race and sex, in record order.
    pub fn presets(&self, race: FormId, female: bool) -> Vec<&AuthoredFace> {
        self.faces.iter().filter(|f| f.preset && f.race == race && f.female == female).collect()
    }

    /// Own + race values of the authored faces of one sex, for [`spread`].
    pub fn drawn_faces(&self, female: bool, races: &HashMap<FormId, [Option<FaceGen>; 2]>) -> Vec<FaceGen> {
        let sex = usize::from(female);
        self.faces
            .iter()
            .filter(|f| f.female == female)
            .map(|f| {
                let r = races.get(&f.race).and_then(|r| r[sex].as_ref()).map(full).unwrap_or_else(|| full(&FaceGen::default()));
                add(&f.facegen, &r)
            })
            .collect()
    }
}

/// Two FaceGen sets added value by value (full length).
pub fn add(a: &FaceGen, b: &FaceGen) -> FaceGen {
    let (a, b) = (full(a), full(b));
    let s = |x: &[f64], y: &[f64]| x.iter().zip(y).map(|(p, q)| p + q).collect();
    FaceGen { fggs: s(&a.fggs, &b.fggs), fgga: s(&a.fgga, &b.fgga), fgts: s(&a.fgts, &b.fgts) }
}

/// `a` + t × (`b` − `a`), value by value.
pub fn blend(a: &FaceGen, b: &FaceGen, t: f64) -> FaceGen {
    let (a, b) = (full(a), full(b));
    let m = |x: &[f64], y: &[f64]| x.iter().zip(y).map(|(p, q)| p + t * (q - p)).collect();
    FaceGen { fggs: m(&a.fggs, &b.fggs), fgga: m(&a.fgga, &b.fgga), fgts: m(&a.fgts, &b.fgts) }
}

// ---- the look being edited ----------------------------------------------------------------

/// A whole number below `n` (0 when `n` is 0).
fn below(rng: &mut impl RngCore, n: usize) -> usize {
    if n == 0 {
        0
    } else {
        (rng.next_u64() % n as u64) as usize
    }
}

/// A number between `lo` and `hi`.
fn between(rng: &mut impl RngCore, lo: f64, hi: f64) -> f64 {
    lo + (hi - lo) * (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

/// What Randomise changes [design].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Randomise {
    pub face: bool,
    pub hair: bool,
    pub race: bool,
}

/// A random face: two authored faces of the race and sex blended at a random point
/// between them [design: a blend stays among plausible faces; two give variety], with the
/// hair, beard, brows, eyes and hair colour of one of them when `what.hair` and the race
/// allows them.
pub fn random_look(cat: &Catalogue, look: &Look, what: Randomise, rng: &mut impl RngCore) -> Look {
    let mut out = look.clone();
    if what.race && !cat.races.is_empty() {
        out.race = cat.races[below(rng, cat.races.len())].id;
    }
    let faces = cat.faces_of(out.race, out.female);
    if faces.len() >= 2 {
        let a = faces[below(rng, faces.len())];
        let b = faces[below(rng, faces.len())];
        if what.face {
            out.facegen = blend(&a.facegen, &b.facegen, between(rng, 0.25, 0.75));
        }
        if what.hair {
            let sex = usize::from(out.female);
            let race = cat.race(out.race);
            let allowed_hair = |h: Option<FormId>| h.is_some_and(|h| race.is_none_or(|r| r.hairs[sex].iter().any(|c| c.id == h)));
            out.hair = if allowed_hair(a.hair) {
                a.hair
            } else {
                race.and_then(|r| r.hairs[sex].get(below(rng, r.hairs[sex].len()))).map(|c| c.id).or(out.hair)
            };
            out.hair_color = a.hair_color.or(out.hair_color);
            if let Some(r) = race.filter(|r| !r.eyes[sex].is_empty()) {
                out.eyes = Some(r.eyes[sex][below(rng, r.eyes[sex].len())].id);
            }
            let known: Vec<FormId> = cat.facial_hair.iter().chain(&cat.brows).map(|c| c.id).collect();
            out.head_parts = a.head_parts.iter().copied().filter(|p| known.contains(p)).collect();
        }
    }
    out
}

/// The head parts of `look` of one kind replaced by `part` (none: removed).
pub fn set_part(look: &mut Look, kind: &[Choice], part: Option<FormId>) {
    look.head_parts.retain(|p| !kind.iter().any(|c| c.id == *p));
    if let Some(p) = part {
        look.head_parts.push(p);
    }
}

/// The head part of `look` of one kind, if any.
pub fn part_of(look: &Look, kind: &[Choice]) -> Option<FormId> {
    look.head_parts.iter().copied().find(|p| kind.iter().any(|c| c.id == *p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facegen::{Control, OffsetControl};

    fn ctl() -> Ctl {
        let unit = |m: usize, n: usize| (0..n).map(|i| if i == m { 1.0 } else { 0.0 }).collect::<Vec<f32>>();
        Ctl {
            geometry_basis: 0,
            texture_basis: 0,
            shape: vec![
                Control { label: "Nose - short / long".into(), dir: unit(0, 50) },
                Control { label: "Nose - bridge shallow / deep".into(), dir: unit(1, 50) },
                Control { label: "Nose - short / long".into(), dir: unit(2, 50) },
            ],
            asymmetry: vec![Control { label: "Nose - tip transverse shift".into(), dir: unit(0, 30) }],
            texture: vec![Control { label: "Skin Shade Dark / Light".into(), dir: unit(0, 50) }],
            texture_asymmetry: vec![],
            age: Some(OffsetControl { shape: (0..50).map(|i| if i < 2 { 2.0 } else { 0.0 }).collect(), shape_offset: 30.0, texture: unit(5, 50), texture_offset: 32.0 }),
            gender: None,
        }
    }

    #[test]
    fn labels_split_into_group_aspect_and_ends() {
        let s = |l: &str| split_label(l);
        assert_eq!(s("Nose - bridge shallow / deep"), ("Nose".into(), "bridge".into(), "shallow".into(), "deep".into()));
        assert_eq!(s("Chin - small / large"), ("Chin".into(), "".into(), "small".into(), "large".into()));
        assert_eq!(s("Skin Shade Dark / Light"), ("Skin".into(), "Shade".into(), "Dark".into(), "Light".into()));
        assert_eq!(s("Eyes Dark Brown / Light Blue"), ("Eyes".into(), "".into(), "Dark Brown".into(), "Light Blue".into()));
        assert_eq!(s("Eyebrows Very Thin / Thick"), ("Eyebrows".into(), "".into(), "Very Thin".into(), "Thick".into()));
        assert_eq!(s("Face - brow-nose-chin ratio").3, "+");
    }

    #[test]
    fn the_controls_become_sliders() {
        let sl = sliders(&ctl());
        let labels: Vec<&str> = sl.iter().map(|s| s.label.as_str()).collect();
        // Age first; the repeated label once.
        assert_eq!(labels, vec!["Age - young / old", "Nose - short / long", "Nose - bridge shallow / deep", "Nose - tip transverse shift", "Skin Shade Dark / Light"]);
        assert_eq!(sl[3].space(), Space::Asymmetry);
        assert_eq!(sl[4].space(), Space::Skin);
    }

    #[test]
    fn a_slider_reads_own_and_race_values_and_moves_the_own_ones() {
        let sl = sliders(&ctl());
        let nose = &sl[1];
        let mut own = full(&FaceGen::default());
        let race = FaceGen { fggs: vec![0.5; 50], ..full(&FaceGen::default()) };
        assert!((nose.value(&own, Some(&race)) - 0.5).abs() < 1e-9);
        nose.set(&mut own, Some(&race), 2.0);
        assert!((nose.value(&own, Some(&race)) - 2.0).abs() < 1e-9);
        assert!((own.fggs[0] - 1.5).abs() < 1e-9 && own.fggs[1] == 0.0);
        assert_eq!(race.fggs[0], 0.5);
        // Age: read from the shape part with its offset; moves shape and texture alike.
        let age = &sl[0];
        let mut own = full(&FaceGen::default());
        assert!((age.value(&own, None) - 30.0).abs() < 1e-9);
        age.set(&mut own, None, 38.0);
        assert!((age.value(&own, None) - 38.0).abs() < 1e-9);
        // 8 years along (2, 2) of length² 8: two units each; the texture part (a unit
        // direction) moves 8.
        assert!((own.fggs[0] - 2.0).abs() < 1e-9 && (own.fggs[1] - 2.0).abs() < 1e-9 && (own.fgts[5] - 8.0).abs() < 1e-9);
    }

    #[test]
    fn a_slider_range_comes_from_the_authored_faces() {
        let mut sl = sliders(&ctl());
        let faces: Vec<FaceGen> = (0..101).map(|i| FaceGen { fggs: (0..50).map(|m| if m == 0 { i as f64 / 50.0 - 1.0 } else { 0.0 }).collect(), ..full(&FaceGen::default()) }).collect();
        spread(&mut sl, &faces);
        let (lo, hi) = sl[1].range;
        assert!(lo < -0.9 && hi > 0.9 && hi - lo < 3.0, "{lo} {hi}");
        let mut own = full(&FaceGen::default());
        sl[1].set_fraction(&mut own, None, 1.0);
        assert!((sl[1].value(&own, None) - hi).abs() < 1e-9);
        assert!((sl[1].fraction(&own, None) - 1.0).abs() < 1e-9);
        // Asymmetry keeps room although no face has any; age stays adult.
        assert!(sl[3].range.0 <= -ASYMMETRY_REACH && sl[3].range.1 >= ASYMMETRY_REACH);
        assert!(sl[0].range.0 >= MIN_AGE);
        spread(&mut sl, &faces[..5]);
        assert_eq!(sl[0].range, (27.0, 33.0));
    }

    #[test]
    fn head_parts_are_sorted_by_their_models_name() {
        assert_eq!(part_kind(r"Characters\Hair\BeardMustacheCurly.NIF"), PartKind::FacialHair);
        assert_eq!(part_kind(r"characters\hair\eyebrowm.nif"), PartKind::Brows);
        assert_eq!(part_kind(r"characters\hair\eyepatch.nif"), PartKind::Other);
    }

    #[test]
    fn one_kind_of_part_is_replaced() {
        let beards = vec![Choice { id: FormId(1), name: "a".into() }, Choice { id: FormId(2), name: "b".into() }];
        let mut look = Look { race: FormId(9), female: false, hair: None, hair_color: None, eyes: None, head_parts: vec![FormId(1), FormId(7)], facegen: FaceGen::default() };
        set_part(&mut look, &beards, Some(FormId(2)));
        assert_eq!(look.head_parts, vec![FormId(7), FormId(2)]);
        assert_eq!(part_of(&look, &beards), Some(FormId(2)));
        set_part(&mut look, &beards, None);
        assert_eq!(look.head_parts, vec![FormId(7)]);
    }

    #[test]
    fn a_blend_lies_between() {
        let a = FaceGen { fggs: vec![0.0; 50], fgga: vec![2.0; 30], fgts: vec![1.0; 50] };
        let b = FaceGen { fggs: vec![4.0; 50], fgga: vec![0.0; 30], fgts: vec![1.0; 50] };
        let m = blend(&a, &b, 0.25);
        assert_eq!((m.fggs[3], m.fgga[3], m.fgts[3]), (1.0, 1.5, 1.0));
    }

    #[test]
    fn a_beard_mix_is_offered_as_facial_hair() {
        let part = |model: Option<&str>, extra: Vec<u32>, name: &str| crate::records::HeadPart {
            model: model.map(str::to_string),
            extra: extra.into_iter().map(FormId).collect(),
            edid: Some(name.into()),
            name: Some(name.into()),
            playable: true,
        };
        let mut forms = ActorForms::default();
        forms.head_parts.insert(FormId(1), part(Some("Characters\\Hair\\BeardMustache.nif"), vec![], "Swashbuckler"));
        forms.head_parts.insert(FormId(2), part(Some("Characters\\Hair\\BeardGoatee.nif"), vec![], "Goatee"));
        forms.head_parts.insert(FormId(3), part(None, vec![1, 2], "Ronin"));
        // A mix naming a part that is not a beard, or one not loaded, is left out.
        forms.head_parts.insert(FormId(4), part(Some("Characters\\Hair\\EyePatch.nif"), vec![], "Patch"));
        forms.head_parts.insert(FormId(5), part(None, vec![1, 4], "Odd"));
        forms.head_parts.insert(FormId(6), part(None, vec![1, 99], "Missing"));
        let cat = Catalogue::build(&forms, true);
        let names: Vec<&str> = cat.facial_hair.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["Swashbuckler", "Goatee", "Ronin"]);
    }
}
