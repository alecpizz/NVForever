//! The karma's alignment and title (docs/spec-notes/pipboy-stats.md): for the Pip-Boy's
//! stats page (`nvf-play` pipboy/stats) and GetPlayerKarmaTitle (`crate::nvse_jg9`).

use crate::game::Game;
use std::collections::BTreeMap;

/// Custom per-level karma title overrides parsed from `Data\NVSE\Plugins\TTW.ini`
/// `[Karma Titles]` (`sKarmicTitle<Good|Neutral|Evil><level>`, supporting levels
/// `1..=50`, `1..=60`, and higher) [documented: ROOGNVSE Plugin / TTW NVSE Plugin
/// (Nexus Mods ID 77415, `https://www.nexusmods.com/newvegas/mods/77415`, author
/// Roy Batty / RoyBatterian, versions 3.3.2 / 3.3.3; bundled in Tale of Two Wastelands
/// 3.3.x as `ttw_nvse.dll`) public description ("Karma Title Expansion: Set custom
/// titles for each level of Karma (Good, Neutral, Evil) via INI") and shipped
/// `Data\NVSE\Plugins\TTW.ini` `[Karma Titles]` schema (`sKarmicTitleGood<N>`,
/// `sKarmicTitleNeutral<N>`, `sKarmicTitleEvil<N>`, e.g. levels `1..=60` as also used
/// by Consistent Karma Titles - ESPless, Nexus Mods ID 89822,
/// `https://www.nexusmods.com/newvegas/mods/89822`, author VeryLastKiss); DLL/source
/// not consulted].
///
/// Empty when `TTW.ini` is absent or contains no `[Karma Titles]` entries, leaving
/// the plugins' karma titles unchanged.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TtwKarmaTitles {
    /// Overrides by [`karma_column`] (`0` Good, `1` Neutral, `2` Evil) and 1-based level.
    columns: [BTreeMap<u32, String>; 3],
}

impl TtwKarmaTitles {
    /// Parse `[Karma Titles]` entries from `TTW.ini` text.
    ///
    /// [documented: shipped `Data\NVSE\Plugins\TTW.ini` `[Karma Titles]` section with
    /// `sKarmicTitleGood<N>`, `sKarmicTitleNeutral<N>`, and `sKarmicTitleEvil<N>` keys
    /// for ROOGNVSE / TTW NVSE Plugin 3.3.2 / 3.3.3 (Nexus Mods ID 77415); DLL/source
    /// not consulted]
    /// [hypothesis: ASCII case-insensitive section/key matching, optional leading-zero
    /// level digits, `;`/`#` comment lines, inline `;` comments, and optional surrounding
    /// quotes stripped; blank values ignored].
    pub fn parse(text: &str) -> TtwKarmaTitles {
        let mut out = TtwKarmaTitles::default();
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut in_section = false;
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix('[') {
                let sec = rest.split(']').next().unwrap_or("").trim();
                in_section = sec.eq_ignore_ascii_case("karma titles");
                continue;
            }
            if !in_section {
                continue;
            }
            let Some((raw_key, raw_val)) = trimmed.split_once('=') else { continue };
            let Some((col, level)) = parse_ttw_karma_key(raw_key.trim()) else { continue };
            let val = raw_val.split(';').next().unwrap_or("").trim();
            let val = val.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(val).trim();
            if !val.is_empty() {
                out.columns[col].insert(level, val.to_string());
            }
        }
        out
    }

    /// True when no `[Karma Titles]` overrides are loaded.
    pub fn is_empty(&self) -> bool {
        self.columns.iter().all(BTreeMap::is_empty)
    }

    /// Resolve an override title for `level` and `alignment` (`"Good"`, `"Very Good"`,
    /// `"Neutral"`, `"Evil"`, `"Very Evil"`).
    ///
    /// Returns `None` when no override applies (so callers fall back to the plugins'
    /// settings, [`karma_title`]).
    ///
    /// [design: levels below `1` clamp to `1`; levels above `30` with no exact key clamp
    /// to the highest defined `TTW.ini` level for that alignment when that maximum is at
    /// least `30`, leaving sparse overrides below `30` to the plugins' settings].
    pub fn resolve(&self, level: i64, alignment: &str) -> Option<&str> {
        let map = &self.columns[karma_column(alignment)];
        if map.is_empty() {
            return None;
        }
        let lvl = u32::try_from(level.max(1)).unwrap_or(u32::MAX);
        if let Some(title) = map.get(&lvl) {
            return Some(title.as_str());
        }
        if lvl > 30 {
            if let Some((&max_lvl, title)) = map.iter().next_back() {
                if max_lvl >= 30 && lvl > max_lvl {
                    return Some(title.as_str());
                }
            }
        }
        None
    }
}

fn parse_ttw_karma_key(key: &str) -> Option<(usize, u32)> {
    let lower = key.to_ascii_lowercase();
    let rest = lower.strip_prefix("skarmictitle")?;
    let (col, num_str) = if let Some(n) = rest.strip_prefix("good") {
        (0, n)
    } else if let Some(n) = rest.strip_prefix("neutral") {
        (1, n)
    } else {
        (2, rest.strip_prefix("evil")?)
    };
    if num_str.is_empty() || !num_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let level: u32 = num_str.parse().ok()?;
    (level >= 1).then_some((col, level))
}

/// The karma's alignment and its picture from the four `Karma*` globals (very evil, evil,
/// good, very good) [documented: the globals `KarmaVeryEvil`, `KarmaEvil`, `KarmaGood`,
/// `KarmaVeryGood` in FalloutNV.esm; hypothesis: a karma at or beyond a threshold has
/// that alignment, the words are the globals' and the pictures are
/// `karma icons\karma_evil|bad|neutral|good|saintly.dds` in that order].
pub fn karma_alignment(karma: f64, very_evil: f64, evil: f64, good: f64, very_good: f64) -> (&'static str, &'static str) {
    if karma >= very_good {
        ("Very Good", "karma_saintly")
    } else if karma >= good {
        ("Good", "karma_good")
    } else if karma <= very_evil {
        ("Very Evil", "karma_evil")
    } else if karma <= evil {
        ("Evil", "karma_bad")
    } else {
        ("Neutral", "karma_neutral")
    }
}

/// The karma title's column for an alignment ([`karma_alignment`]'s word): good for Good
/// and Very Good, evil for Evil and Very Evil [hypothesis: three columns over five
/// alignments].
pub fn karma_column(alignment: &str) -> usize {
    match alignment {
        "Good" | "Very Good" => 0,
        "Evil" | "Very Evil" => 2,
        _ => 1,
    }
}

/// The game setting holding the karma title at a level (1 to 30) for an alignment, under
/// either spelling: FalloutNV.esm writes levels below 10 with two digits
/// (`sKarmicTitleNeutral01`) [observed: `fnvtool esm inventory`]; one digit is accepted too.
fn karma_title_at(level: usize, column: usize, setting: &impl Fn(&str) -> Option<String>) -> Option<String> {
    let column = ["Good", "Neutral", "Evil"][column];
    [format!("sKarmicTitle{column}{level:02}"), format!("sKarmicTitle{column}{level}")].iter().find_map(|name| setting(name).filter(|s| !s.is_empty()))
}

/// The karma title the plugins' settings give at a level for an alignment: the setting
/// `sKarmicTitle<Good|Neutral|Evil><level>` (levels above 30 take 30's, below 1 level
/// 1's). FalloutNV.esm holds 53 of the 90 (levels 1, 2, 9, 11, 13, 15 and 20-30 in every
/// column, with 8, 14 and 28 in some) [observed: `fnvtool esm inventory`, docs/spec-notes/
/// pipboy-stats.md]; a level without one takes the nearest level below that has one in
/// the same column, else the nearest above [design: the original's built-in titles for
/// the missing levels are not in the data and are not transcribed, LEGAL.md]. `None` when
/// the plugins hold no title in the column.
pub fn karma_title_setting(level: i64, alignment: &str, setting: impl Fn(&str) -> Option<String>) -> Option<String> {
    let level = level.clamp(1, 30) as usize;
    let column = karma_column(alignment);
    (1..=level).rev().chain(level + 1..=30).find_map(|l| karma_title_at(l, column, &setting))
}

/// The karma title at a level for an alignment ([`karma_title_setting`]); with no setting
/// in the column (a load order without the titles), the alignment word itself [design].
pub fn karma_title(level: i64, alignment: &str, setting: impl Fn(&str) -> Option<String>) -> String {
    karma_title_setting(level, alignment, setting).unwrap_or_else(|| alignment.to_string())
}

/// [`karma_title`] with optional `Data\NVSE\Plugins\TTW.ini` `[Karma Titles]` overrides
/// checked first. When `ttw` is empty, behaves identically to [`karma_title`].
pub fn karma_title_with_ttw(level: i64, alignment: &str, ttw: &TtwKarmaTitles, setting: impl Fn(&str) -> Option<String>) -> String {
    if let Some(title) = ttw.resolve(level, alignment) {
        return title.to_string();
    }
    karma_title(level, alignment, setting)
}

impl Game<'_> {
    /// Loaded `Data\NVSE\Plugins\TTW.ini` `[Karma Titles]` overrides (empty when absent).
    pub fn ttw_karma_titles(&self) -> &TtwKarmaTitles {
        self.mod_files.ttw_karma_titles()
    }

    /// The player's karma alignment word ([`karma_alignment`]) and karma title
    /// ([`karma_title_with_ttw`]) now, from the player's Karma, level and the four
    /// `Karma*` globals; `None` without those globals.
    pub fn player_karma(&mut self) -> Option<(&'static str, String)> {
        let limits = ["KarmaVeryEvil", "KarmaEvil", "KarmaGood", "KarmaVeryGood"].map(|n| self.world.forms.global_by_edid(n).map(|v| f64::from(v.1)));
        let [Some(ve), Some(e), Some(go), Some(vg)] = limits else { return None };
        let karma = crate::functions::actor::actor_value(self, fnv_sim::FormId::PLAYER_REF, crate::actor_values::KARMA).ok()?;
        let (word, _) = karma_alignment(karma, ve, e, go, vg);
        let level = self.player_level();
        let title = karma_title_with_ttw(level as i64, word, self.mod_files.ttw_karma_titles(), |n| match self.world.setting_value(n)? {
            fnv_sim::settings::Setting::Str(s) => Some(s.clone()),
            _ => None,
        });
        Some((word, title))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_ttw_ini_leaves_the_settings() {
        let empty = TtwKarmaTitles::default();
        assert!(empty.is_empty());
        let esm = |n: &str| (n == "sKarmicTitleGood01").then(|| "PluginGood1".to_string());
        assert_eq!(karma_title_with_ttw(1, "Good", &empty, esm), "PluginGood1");
        assert_eq!(karma_title_with_ttw(1, "Neutral", &empty, esm), "Neutral");
        assert_eq!(karma_title_with_ttw(45, "Very Evil", &empty, |_| None), "Very Evil");
    }

    #[test]
    fn ttw_ini_parses_levels_1_through_50_across_all_alignments() {
        let mut ini = String::from("\u{feff}; Synthetic test INI\n[Other]\nsKarmicTitleGood1=Ignored\n\n[Karma Titles]\n");
        for lvl in 1..=50 {
            ini.push_str(&format!("sKarmicTitleGood{lvl} = GoodTier{lvl}\n"));
            ini.push_str(&format!("sKarmicTitleNeutral{lvl} = NeutralTier{lvl}\n"));
            ini.push_str(&format!("sKarmicTitleEvil{lvl} = EvilTier{lvl} ; inline comment\n"));
        }
        let ttw = TtwKarmaTitles::parse(&ini);
        assert!(!ttw.is_empty());
        assert_eq!(karma_title_with_ttw(0, "Good", &ttw, |_| None), "GoodTier1");
        assert_eq!(karma_title_with_ttw(1, "Good", &ttw, |_| None), "GoodTier1");
        assert_eq!(karma_title_with_ttw(1, "Very Good", &ttw, |_| None), "GoodTier1");
        assert_eq!(karma_title_with_ttw(25, "Neutral", &ttw, |_| None), "NeutralTier25");
        assert_eq!(karma_title_with_ttw(31, "Evil", &ttw, |_| None), "EvilTier31");
        assert_eq!(karma_title_with_ttw(50, "Very Evil", &ttw, |_| None), "EvilTier50");
        assert_eq!(karma_title_with_ttw(50, "Good", &ttw, |_| None), "GoodTier50");
        // Above the highest defined tier (>= 30), clamps to the top defined tier.
        assert_eq!(karma_title_with_ttw(65, "Neutral", &ttw, |_| None), "NeutralTier50");
    }

    #[test]
    fn sparse_ttw_overrides_fall_back_to_the_settings() {
        let ini = "[karma titles]\nsKarmicTitleGood01=\"Custom First\"\nsKarmicTitleEvil42=CustomFortyTwo\nsKarmicTitleNeutral2=\n";
        let ttw = TtwKarmaTitles::parse(ini);
        let esm = |n: &str| (n == "sKarmicTitleNeutral2").then(|| "PluginRankTwo".to_string());
        assert_eq!(karma_title_with_ttw(1, "Good", &ttw, esm), "Custom First");
        assert_eq!(karma_title_with_ttw(2, "Neutral", &ttw, esm), "PluginRankTwo");
        assert_eq!(karma_title_with_ttw(10, "Good", &ttw, esm), "Good");
        assert_eq!(karma_title_with_ttw(42, "Evil", &ttw, esm), "CustomFortyTwo");
        assert_eq!(karma_title_with_ttw(50, "Evil", &ttw, esm), "CustomFortyTwo");
        assert_eq!(karma_title_with_ttw(45, "Good", &ttw, esm), "Good");
    }

    #[test]
    fn a_level_without_a_setting_takes_the_nearest_one_in_its_column() {
        let esm = |n: &str| match n {
            "sKarmicTitleGood02" => Some("Two".to_string()),
            "sKarmicTitleGood09" => Some("Nine".to_string()),
            "sKarmicTitleEvil30" => Some("Thirty".to_string()),
            "sKarmicTitleNeutral1" => Some("One digit".to_string()),
            _ => None,
        };
        assert_eq!(karma_title(2, "Good", esm), "Two");
        assert_eq!(karma_title(5, "Very Good", esm), "Two", "the nearest below");
        assert_eq!(karma_title(1, "Good", esm), "Two", "none below: the nearest above");
        assert_eq!(karma_title(12, "Good", esm), "Nine");
        assert_eq!(karma_title(3, "Evil", esm), "Thirty");
        assert_eq!(karma_title(64, "Evil", esm), "Thirty");
        assert_eq!(karma_title(4, "Neutral", esm), "One digit");
        assert_eq!(karma_title_setting(4, "Neutral", |_| None), None);
        assert_eq!(karma_title(4, "Neutral", |_| None), "Neutral");
    }
}
