//! SUP NVSE, batch 1 (#1008): rounding, points and angles, numbers in words, the
//! Cyrillic test, the actor finders and the topic getters [documented: each page in the
//! wiki export; docs/spec-notes/ext-sup1.md], checked in the original with the sup1 probe
//! set (supNVSE in the probe game, 2026-11-08) where marked [observed].

use crate::game::{Game, RunCtx};
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;
use rand_core::RngCore;

fn form(args: &[XVal], k: usize) -> Option<FormId> {
    match args.get(k)? {
        XVal::Ref(0) => None,
        XVal::Ref(r) => Some(FormId(*r)),
        XVal::Num(x) if *x >= 1.0 && x.fract() == 0.0 => Some(FormId(*x as u32)),
        _ => None,
    }
}

fn num(args: &[XVal], k: usize, dflt: f64) -> f64 {
    match args.get(k) {
        Some(XVal::Num(x)) => *x,
        _ => dflt,
    }
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

fn list(g: &mut Game, items: Vec<XVal>) -> XVal {
    let entries = items.into_iter().enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
    XVal::Array(g.ext.new_array(ArrayKind::List, entries))
}

/// The x86 conversion of a number to a 32-bit integer, cut toward zero: a NaN or a value
/// outside the range gives i32::MIN (cvttsd2si's "integer indefinite") [observed: sup1
/// roundalt.places, 37.123456789 at 9 places cut reads -2.14748359, INT_MIN / 10^9].
fn to_i32(x: f64) -> i32 {
    if x.is_nan() || x >= 2_147_483_648.0 || x <= -2_147_483_649.0 {
        i32::MIN
    } else {
        x.trunc() as i32
    }
}

/// RoundAlt: `x` to `places` decimals [documented: the page's examples, 37.55 at 1 cut is
/// 37.5, 37.66 at 1 rounded is 37.7]. As the original computes it [observed: sup1
/// roundalt.page, .places, .negative, .halves]: the scale 10^places as a 32-bit integer
/// (no clamp to 0-4: 5 places keep 37.12346; a negative count makes it 0, so the result is
/// 0/0, a NaN), the product kept as a 32-bit float, 0.5 added when `round` is set, cut
/// toward zero to an integer (so -2.5 rounds to -2 and -37.66 to -37), divided back and
/// returned as a 32-bit float (37.6 reads 37.599998).
pub fn round_to(x: f64, places: f64, round: bool) -> f64 {
    let m = to_i32(10f64.powi(to_i32(places).clamp(-64, 64)));
    let mut v = (x * f64::from(m)) as f32;
    if round {
        v += 0.5;
    }
    let r = f64::from(to_i32(f64::from(v)));
    // 0/0 on x86 is the negative "default" NaN.
    if m == 0 && r == 0.0 {
        return -f64::NAN;
    }
    f64::from((r / f64::from(m)) as f32)
}

/// Degrees from a facing `angle_z` (degrees, clockwise from +Y) at (x, y) to (tx, ty),
/// as GetHeadingAngle (`functions::objects::get_heading_angle`)
/// [documented: GetHeadingAngleBetweenPoints, "equivalent to GetHeadingAngle"].
pub fn heading_between(x: f64, y: f64, angle_z: f64, tx: f64, ty: f64) -> f64 {
    let d = (tx - x).atan2(ty - y).to_degrees() - angle_z;
    // Script-supplied: a non-finite angle gives 0.
    if !d.is_finite() {
        return 0.0;
    }
    // One turn added or taken off, once: due behind is -180, and a facing of 765 leaves
    // -405 [observed: sup1 heading.points.s, heading.points.n.765, heading.points.n.m270].
    if d >= 180.0 {
        d - 360.0
    } else if d < -180.0 {
        d + 360.0
    } else {
        d
    }
}

/// A whole number in English words, without "and" or hyphens [documented: NumberToWords,
/// 332323 is "three hundred thirty two thousand three hundred twenty three", 10020 is
/// "ten thousand twenty"; observed: sup1 words.0 "zero", words.m15 "negative fifteen"].
pub fn number_to_words(n: i64) -> String {
    const ONES: [&str; 20] = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen",
        "eighteen", "nineteen",
    ];
    const TENS: [&str; 10] = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];
    const SCALES: [(u64, &str); 6] = [
        (1_000_000_000_000_000_000, "quintillion"),
        (1_000_000_000_000_000, "quadrillion"),
        (1_000_000_000_000, "trillion"),
        (1_000_000_000, "billion"),
        (1_000_000, "million"),
        (1_000, "thousand"),
    ];
    fn below_1000(n: u64, out: &mut Vec<&'static str>) {
        if n >= 100 {
            out.push(ONES[(n / 100) as usize]);
            out.push("hundred");
        }
        let r = n % 100;
        if r >= 20 {
            out.push(TENS[(r / 10) as usize]);
            if !r.is_multiple_of(10) {
                out.push(ONES[(r % 10) as usize]);
            }
        } else if r > 0 {
            out.push(ONES[r as usize]);
        }
    }
    if n == 0 {
        return "zero".into();
    }
    let mut out: Vec<&'static str> = Vec::new();
    if n < 0 {
        out.push("negative");
    }
    let mut rest = n.unsigned_abs();
    for (size, name) in SCALES {
        if rest >= size {
            below_1000(rest / size, &mut out);
            out.push(name);
            rest %= size;
        }
    }
    below_1000(rest, &mut out);
    out.join(" ")
}

/// The actors the finders look through: those in the loaded cells, not the player
/// (and not `besides`), the dead only with `dead`, the player's teammates only with
/// `teammates`, within `distance` of `at` when it is above 0 [documented: FindClosestActor
/// and the others' notes: the loaded interior or the exterior cells around the player's,
/// excluding the player], with their distance.
fn candidates(g: &Game, at: [f64; 3], besides: Option<FormId>, dead: bool, teammates: bool, distance: f64) -> Vec<(FormId, f64)> {
    let mut out = Vec::new();
    let mut cells: Vec<FormId> = g.loaded_cells.iter().copied().collect();
    cells.sort();
    for c in cells {
        for r in g.refs_in_cell(c, false) {
            if r == FormId::PLAYER_REF || Some(r) == besides || !g.present(r) {
                continue;
            }
            let base = crate::functions::base_of(g, r);
            if !matches!(base.and_then(|b| g.data.sig(b)).as_ref(), Some(b"NPC_") | Some(b"CREA")) {
                continue;
            }
            if (!dead && g.is_dead(r)) || (!teammates && g.is_teammate(r)) {
                continue;
            }
            let Some(s) = g.world.peek(r) else { continue };
            let d = ((f64::from(s.pos[0]) - at[0]).powi(2) + (f64::from(s.pos[1]) - at[1]).powi(2) + (f64::from(s.pos[2]) - at[2]).powi(2)).sqrt();
            if distance > 0.0 && d > distance {
                continue;
            }
            out.push((r, d));
        }
    }
    out
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let v = match name {
        // `x`: to the nearest whole number [documented: Round, 37.4 is 37, 37.8 is 38;
        // hypothesis: a half away from zero].
        "round" => XVal::Num(num(args, 0, 0.0).round()),
        "roundalt" => XVal::Num(round_to(num(args, 0, 0.0), num(args, 1, 0.0), num(args, 2, 0.0) != 0.0)),
        // `[x]`: 1 for a NaN; with no number, a NaN [documented: IsNumberNAN's two forms].
        "isnumbernan" => match args.first() {
            Some(XVal::Num(x)) => yes(x.is_nan()),
            _ => XVal::Num(f64::NAN),
        },
        // `x1 y1 z1 x2 y2 z2` [documented: GetDistanceBetweenPoints].
        "getdistancebetweenpoints" => {
            let p: Vec<f64> = (0..6).map(|k| num(args, k, 0.0)).collect();
            XVal::Num(((p[3] - p[0]).powi(2) + (p[4] - p[1]).powi(2) + (p[5] - p[2]).powi(2)).sqrt())
        }
        // `x y angleZ tx ty` [documented: GetHeadingAngleBetweenPoints]; `x y angleZ
        // ref`, the reference's position [documented: GetHeadingAngleAlt].
        "getheadinganglebetweenpoints" => XVal::Num(heading_between(num(args, 0, 0.0), num(args, 1, 0.0), num(args, 2, 0.0), num(args, 3, 0.0), num(args, 4, 0.0))),
        "getheadinganglealt" => {
            let Some(to) = form(args, 3).and_then(|r| g.world.get(r).ok()).map(|s| s.pos) else { return Some(Ok(zero)) };
            XVal::Num(heading_between(num(args, 0, 0.0), num(args, 1, 0.0), num(args, 2, 0.0), f64::from(to[0]), f64::from(to[1])))
        }
        "numbertowords" => XVal::Str(number_to_words(num(args, 0, 0.0).trunc() as i64)),
        // `text`: 1 when it holds a Cyrillic letter (U+0400-U+04FF) [documented:
        // StringContainsCyrillic; hypothesis: the block].
        "stringcontainscyrillic" => yes(match args.first() {
            Some(XVal::Str(s)) => s.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c)),
            _ => false,
        }),
        // Always -1 [documented: GetSUPVersion's note].
        "getsupversion" => XVal::Num(-1.0),
        // Carrying more than the capacity, as ShowOff's GetIsPlayerOverencumbered
        // [documented: IsPlayerOverencumbered points there].
        "isplayeroverencumbered" => {
            let w = g.world.carried_weight(FormId::PLAYER_REF).unwrap_or(0.0);
            let cap = crate::functions::actor::actor_value(g, FormId::PLAYER_REF, crate::actor_values::CARRY_WEIGHT).unwrap_or(0.0);
            yes(w > cap)
        }
        // `x y z [dead] [teammates] [distance]`, or on a reference `[dead] [teammates]
        // [distance]`: the closest of the candidates, or a random one; none (0) when
        // there is none [documented: FindClosestActor, FindRandomActor and the FromRef
        // forms; hypothesis: the calling reference is not its own find].
        "findclosestactor" | "findrandomactor" | "findclosestactorfromref" | "findrandomactorfromref" => {
            let from_ref = name.ends_with("fromref");
            let (at, k) = if from_ref {
                let Some(p) = subject.and_then(|r| g.world.get(r).ok()).map(|s| s.pos) else { return Some(Ok(XVal::Ref(0))) };
                ([f64::from(p[0]), f64::from(p[1]), f64::from(p[2])], 0)
            } else {
                ([num(args, 0, 0.0), num(args, 1, 0.0), num(args, 2, 0.0)], 3)
            };
            let found = candidates(g, at, subject.filter(|_| from_ref), num(args, k, 0.0) != 0.0, num(args, k + 1, 0.0) != 0.0, num(args, k + 2, 0.0));
            let pick = if name.starts_with("findclosest") {
                found.iter().min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0))).map(|p| p.0)
            } else if found.is_empty() {
                None
            } else {
                Some(found[g.rng.next_u32() as usize % found.len()].0)
            };
            XVal::Ref(pick.map_or(0, |r| r.0))
        }
        // `topic`: the topic's type, 0 Topic … 7 Radio (DATA) [documented: TopicGetType].
        "topicgettype" => XVal::Num(form(args, 0).and_then(|t| g.data.topic_defs.get(&t)).map_or(0.0, |t| f64::from(t.topic_type))),
        // `topic [quest]`: the topic's lines, in topic order [documented:
        // TopicGetAllTopicInfos]. With a quest the original gives the lines of the topic's
        // first quest (its first QSTI) whichever quest is named, and none for a form that is
        // not a quest [observed: sup1 infos.greeting.quest, 25 lines of 139B8C, GREETING's
        // first quest, for VFreeformGoodsprings; infos.goodbye.quest, GOODBYE's first
        // quest's one line; infos.creatures.otherquest, VCG02 still gives 104C66's line;
        // infos.creatures.notquest, the stimpak gives none].
        "topicgetalltopicinfos" => {
            let topic = form(args, 0);
            let quest = match form(args, 1) {
                None => None,
                Some(q) if g.data.sig(q) == Some(*b"QUST") => Some(topic.and_then(|t| g.data.topic_defs.get(&t)).and_then(|d| d.quests.first().copied())),
                Some(_) => return Some(Ok(list(g, Vec::new()))),
            };
            let infos: Vec<XVal> = topic
                .and_then(|t| g.data.topics.get(&t))
                .into_iter()
                .flatten()
                .filter(|i| quest.is_none_or(|q| q.is_some() && g.data.infos.get(i).is_some_and(|d| d.quest == q)))
                .map(|i| XVal::Ref(i.0))
                .collect();
            list(g, infos)
        }
        // `info`: its choices (TCLT) [documented: TopicInfoGetAllChoices, …ChoiceCount];
        // `info topic`: 1 when the topic is one [documented: TopicInfoHasChoice].
        "topicinfogetallchoices" => {
            let choices: Vec<XVal> = form(args, 0).and_then(|i| g.dialogue_view().info(&i).map(|d| d.choices.iter().map(|c| XVal::Ref(c.0)).collect())).unwrap_or_default();
            list(g, choices)
        }
        // A line without choices, or a form that is not a line, gives -1 for both
        // [observed: sup1 count.bye, count.notinfo, has.bye].
        "topicinfogetchoicecount" => {
            let n = form(args, 0).and_then(|i| g.dialogue_view().info(&i).map(|d| d.choices.len())).unwrap_or(0);
            XVal::Num(if n == 0 { -1.0 } else { n as f64 })
        }
        "topicinfohaschoice" => {
            let t = form(args, 1);
            let choices: Vec<FormId> = form(args, 0).and_then(|i| g.dialogue_view().info(&i).map(|d| d.choices.clone())).unwrap_or_default();
            if choices.is_empty() {
                XVal::Num(-1.0)
            } else {
                yes(t.is_some_and(|t| choices.contains(&t)))
            }
        }
        _ => return None,
    };
    let _ = ctx;
    Some(Ok(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pages_examples() {
        // [documented: RoundAlt; observed: sup1 roundalt.page, 32-bit float results].
        let f = |x: f64| f64::from(x as f32);
        assert_eq!(round_to(37.55, 0.0, false), 37.0);
        assert_eq!(round_to(37.55, 1.0, false), 37.5);
        assert_eq!(round_to(37.6, 1.0, false), f(37.6));
        assert_eq!(round_to(37.66, 1.0, true), f(37.7));
        assert_eq!(round_to(37.665454, 3.0, true), f(37.665));
        assert_eq!(round_to(37.665654, 3.0, true), f(37.666));
        // [observed: sup1 roundalt.places, .negative, .halves].
        assert_eq!(round_to(37.123456789, 5.0, true), f(37.12346));
        assert_eq!(round_to(37.123456789, 9.0, false), f(-2.147483648));
        assert!(round_to(37.55, -1.0, true).is_nan());
        assert_eq!(round_to(-37.55, 1.0, true), -37.5);
        assert_eq!(round_to(-37.66, 0.0, true), -37.0);
        assert_eq!(round_to(-2.5, 0.0, true), -2.0);
        assert_eq!(round_to(0.125, 2.0, true), f(0.13));
        // [documented: NumberToWords; observed: sup1 words.m15].
        assert_eq!(number_to_words(10), "ten");
        assert_eq!(number_to_words(332323), "three hundred thirty two thousand three hundred twenty three");
        assert_eq!(number_to_words(10020), "ten thousand twenty");
        assert_eq!(number_to_words(0), "zero");
        assert_eq!(number_to_words(-1_000_015), "negative one million fifteen");
        // Facing north (0) at the origin: east is 90, south -180, west -90 [observed:
        // sup1 heading.points.*].
        assert_eq!(heading_between(0.0, 0.0, 0.0, 10.0, 0.0), 90.0);
        assert_eq!(heading_between(0.0, 0.0, 0.0, 0.0, -10.0), -180.0);
        assert_eq!(heading_between(0.0, 0.0, 90.0, 0.0, 10.0), -90.0);
        assert_eq!(heading_between(0.0, 0.0, -270.0, 0.0, 10.0), -90.0);
        // One turn only: 765 leaves -405 [observed: sup1 heading.points.n.765].
        assert_eq!(heading_between(0.0, 0.0, 765.0, 0.0, 10.0), -405.0);
        assert_eq!(heading_between(0.0, 0.0, f64::INFINITY, 0.0, 10.0), 0.0);
        assert_eq!(heading_between(0.0, 0.0, f64::NAN, 0.0, 10.0), 0.0);
        assert!(heading_between(0.0, 0.0, 1e30, 0.0, 10.0).is_finite());
    }
}
