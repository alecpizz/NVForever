//! A new game's start, a row of the Game folder page's NVForever.ini card (#2945, #4313): the
//! start quest and the intro film the game reads from NVForever.ini's `[General]`
//! (`sCharGenQuest`, `sIntroMovie`; docs/design/install-layout.md, nvf-play game_start.rs),
//! typed in so a start mod's values need no hand edit of the file. A TTW on/off switch writes Tale of Two Wastelands'
//! values or the base game's (docs/spec-notes/new-game-start.md).

use fnv_install::{ini_get, ini_set};

/// The section and keys the game reads (nvf-play game_start.rs `SECTION`, `CHARGEN_KEY`,
/// `INTRO_KEY`).
pub const SECTION: &str = "General";
pub const QUEST_KEY: &str = "sCharGenQuest";
pub const INTRO_KEY: &str = "sIntroMovie";

/// The quest a new game starts when nothing else is set.
///
/// FormID `00102037`, editor ID `VCG00`, named "Welcome To Fabulous New Vegas".
/// [documented: Fallout Wiki, Fallout: New Vegas quest IDs, the `00102037` row and the
/// VCG00 stage table]
/// [documented: GECK Wiki, INI Settings, `SCharGenQuest` default `00102037`]
/// [documented: `Fallout_default.ini` in the player's game folder,
/// `SCharGenQuest=00102037`]
/// `001FFFF8` is not this quest. That value is Tale of Two Wastelands' start, and
/// FalloutNV.esm has no form there [observed: `fnvtool esm decode`, as recorded at
/// `not_a_quest` in `engine/nvf-play/src/game_start.rs`].
pub const DEFAULT_QUEST: &str = "00102037";

/// The intro film a new game plays from `Data\Video` when the film is not turned off.
///
/// It is the base game's own file, in the game folder the player provides; NVForever
/// ships no film. A Steam install's `Data\Video` holds `FNVIntro.bik` and no other file,
/// put there with the game [observed: a Steam install's `Data\Video`, 2026-11-23]. A mod
/// can replace it with its own `Video\FNVIntro.bik`, and the game then plays that file
/// (code: `open_film` in `engine/nvf-play/src/intro.rs` reads the loose file that wins
/// among the data's layers, through `Vfs::loose_path`).
/// The game's own `Fallout_default.ini` names `sIntroMovie=Fallout INTRO Vsk.bik`, which
/// that folder does not have [documented: `Fallout_default.ini`].
/// NVForever's default is `FNVIntro.bik` by owner decision [owner, 2026-10-15]; the other
/// file is Fallout 3's film (`engine/nvf-play/src/game_start.rs` module doc). Without the key
/// NVForever plays `video\fnvintro.bik` (code: `DEFAULT_INTRO` in
/// `engine/nvf-play/src/game_start.rs`).
pub const DEFAULT_FILM: &str = "FNVIntro.bik";

/// Tale of Two Wastelands' master. The card's TTW switch is always shown; while the start
/// quest is TTW's and this plugin is off or not in the load order, the card warns
/// ([`ttw_missing`]).
pub const TTW_PLUGIN: &str = "TaleOfTwoWastelands.esm";
/// TTW's start quest, as its installation guide writes it (`SCharGenQuest=001FFFF8`, `SIntroMovie=`).
pub const TTW_QUEST: &str = "001FFFF8";

/// The card's fields.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct Fields {
    /// `sCharGenQuest` (a FormID or an Editor ID); empty: no line, the New Vegas opening.
    pub quest: String,
    /// `sIntroMovie` (a file in `Data\Video`); empty: no line, the New Vegas film.
    pub film: String,
    /// `sIntroMovie=` with nothing after it: no film at all.
    pub no_film: bool,
}

fn value(text: &str, key: &str) -> Option<String> {
    ini_get(text, SECTION, key).map(|v| {
        let v = v.trim();
        v.strip_prefix('"').and_then(|r| r.strip_suffix('"')).unwrap_or(v).to_string()
    })
}

impl Fields {
    /// The opening a fresh game uses: quest `00102037` and `FNVIntro.bik`.
    pub fn original() -> Fields {
        Fields { quest: DEFAULT_QUEST.into(), film: DEFAULT_FILM.into(), no_film: false }
    }

    /// The fields as the INI's text has them.
    pub fn read(text: &str) -> Fields {
        let film = value(text, INTRO_KEY);
        let quest = value(text, QUEST_KEY).filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT_QUEST.into());
        match film {
            None => Fields { quest, film: DEFAULT_FILM.into(), no_film: false },
            Some(f) if f.is_empty() => Fields { quest, film: DEFAULT_FILM.into(), no_film: true },
            Some(f) => Fields { quest, film: f, no_film: false },
        }
    }

    /// `text` with these fields written: an empty field takes its line out (the game's
    /// default), "no film" writes `sIntroMovie=`; every other line is kept.
    pub fn write(&self, text: &str) -> String {
        let quest = self.quest.trim();
        let t = if quest.is_empty() { ini_remove(text, SECTION, QUEST_KEY) } else { ini_set(text, SECTION, QUEST_KEY, quest) };
        let film = self.film.trim();
        if self.no_film {
            ini_set(&t, SECTION, INTRO_KEY, "")
        } else if film.is_empty() {
            ini_remove(&t, SECTION, INTRO_KEY)
        } else {
            ini_set(&t, SECTION, INTRO_KEY, film)
        }
    }

    /// Whether these are neither TTW's start nor the base game's: another mod's, set through
    /// the card's Custom… window (#4288).
    pub fn is_custom(&self) -> bool {
        !self.is_ttw() && *self != Fields::original()
    }

    /// Whether these are Tale of Two Wastelands' start: its quest and no film.
    pub fn is_ttw(&self) -> bool {
        self.quest.trim().eq_ignore_ascii_case(TTW_QUEST) && self.no_film
    }

    /// Tale of Two Wastelands' start. TTW off is [`Fields::original`], the base game. The film
    /// field keeps the base game's name, as [`Fields::read`] fills it, so the switch's value
    /// and a saved file read back are equal; "No intro film" writes `sIntroMovie=`.
    pub fn ttw() -> Fields {
        Fields { quest: TTW_QUEST.into(), film: DEFAULT_FILM.into(), no_film: true }
    }
}

/// The card's warning when the fields name TTW's start quest but TTW's master is off
/// (`Some(false)`) or not in the load order at all (`None`, TTW removed with its lines
/// left in, #3229): New Game then starts the New Vegas opening instead.
pub fn ttw_missing(fields: &Fields, ttw_active: Option<bool>) -> Option<String> {
    if !fields.quest.trim().eq_ignore_ascii_case(TTW_QUEST) || ttw_active == Some(true) {
        return None;
    }
    let state = if ttw_active.is_some() { "is off in the load order" } else { "is not in the load order" };
    Some(format!("{TTW_PLUGIN} {state}: New Game starts the New Vegas opening instead. Turn it on, or turn TTW off."))
}

/// `text` without the `key=` lines of `[section]` (names without case); every other line kept.
fn ini_remove(text: &str, section: &str, key: &str) -> String {
    let mut in_section = false;
    let mut out: Vec<&str> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            in_section = t[1..t.len() - 1].trim().eq_ignore_ascii_case(section);
        } else if in_section && !t.starts_with(';') && !t.starts_with('#') && t.split_once('=').is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case(key)) {
            continue;
        }
        out.push(line);
    }
    let mut s = out.join("\n");
    if text.ends_with('\n') {
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keys_are_the_games() {
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../engine/nvf-play/src/game_start.rs")).unwrap();
        for line in ["pub const SECTION: &str = \"General\";", "pub const CHARGEN_KEY: &str = \"sCharGenQuest\";", "pub const INTRO_KEY: &str = \"sIntroMovie\";"] {
            assert!(src.contains(line), "game_start.rs no longer has {line}");
        }
        // The game's own test reads TTW's lines as its quest and no film.
        assert!(src.contains("SCharGenQuest=001FFFF8\\nSIntroMovie=\\n"));
    }

    #[test]
    fn ttws_quest_without_ttw_is_flagged() {
        assert_eq!(ttw_missing(&Fields::ttw(), Some(true)), None);
        assert!(ttw_missing(&Fields::ttw(), Some(false)).unwrap().contains("is off in the load order"));
        assert!(ttw_missing(&Fields::ttw(), None).unwrap().contains("is not in the load order"));
        // The quest alone, with the default film, is still TTW's.
        let quest_only = Fields { quest: "001fffF8".into(), ..Fields::default() };
        assert!(ttw_missing(&quest_only, None).is_some());
        assert_eq!(ttw_missing(&Fields::default(), None), None);
    }

    #[test]
    fn read_and_write() {
        let ini = "[Game]\nPath=F:\\FNV\n\n[Settings]\ngrass=1\n";
        assert_eq!(Fields::read(ini), Fields::original());
        let on = Fields { quest: "001FFFF8".into(), film: String::new(), no_film: true }.write(ini);
        assert_eq!(ini_get(&on, "General", "sCharGenQuest").as_deref(), Some("001FFFF8"));
        assert_eq!(ini_get(&on, "General", "sIntroMovie").as_deref(), Some(""));
        assert_eq!(Fields::read(&on), Fields { quest: "001FFFF8".into(), film: DEFAULT_FILM.into(), no_film: true });
        // Emptied: both lines go, every other line stays.
        let off = Fields::default().write(&on);
        assert_eq!(ini_get(&off, "General", "sCharGenQuest"), None);
        assert_eq!(ini_get(&off, "General", "sIntroMovie"), None);
        assert_eq!(ini_get(&off, "Settings", "grass").as_deref(), Some("1"));
        assert_eq!(ini_get(&off, "Game", "Path").as_deref(), Some("F:\\FNV"));
        // Another mod's start and film, typed with spaces around.
        let other = Fields { quest: " MyStart ".into(), film: "Mine.bik ".into(), no_film: false }.write(ini);
        assert_eq!(Fields::read(&other), Fields { quest: "MyStart".into(), film: "Mine.bik".into(), no_film: false });
        // "No film" wins over a typed film; lines copied in by hand, any case, quoted, count.
        let none = Fields { quest: String::new(), film: "Mine.bik".into(), no_film: true }.write(ini);
        assert_eq!(ini_get(&none, "General", "sIntroMovie").as_deref(), Some(""));
        let quoted = Fields::read("[general]\nSCharGenQuest=\"001fffF8\"\nSIntroMovie=\n");
        assert_eq!(quoted.quest, "001fffF8");
        assert!(quoted.no_film);
        // A comment naming the key is not a key.
        assert!(Fields::default().write("[General]\n; sCharGenQuest=old\nsCharGenQuest=001FFFF8\n").contains("; sCharGenQuest=old"));
    }

    #[test]
    fn ttw_off_restores_the_original_opening() {
        let ini = "[General]\nSCharGenQuest=001FFFF8\nSIntroMovie=\n";
        let off = Fields::original();
        assert_eq!(off.quest, "00102037");
        assert_eq!(off.film, "FNVIntro.bik");
        assert!(!off.no_film);
        let out = off.write(ini);
        assert_eq!(ini_get(&out, "General", "sCharGenQuest").as_deref(), Some("00102037"));
        assert_eq!(ini_get(&out, "General", "sIntroMovie").as_deref(), Some("FNVIntro.bik"));
        assert_eq!(Fields::read(&out), off);
    }

    #[test]
    fn ttw_start_is_its_quest_and_no_film() {
        let fields = Fields::ttw();
        assert!(fields.is_ttw());
        assert!(!Fields::original().is_ttw());
        let out = fields.write("[General]\nsCharGenQuest=00102037\nsIntroMovie=FNVIntro.bik\n");
        assert_eq!(ini_get(&out, "General", "sCharGenQuest").as_deref(), Some("001FFFF8"));
        assert_eq!(ini_get(&out, "General", "sIntroMovie").as_deref(), Some(""));
        assert_eq!(Fields::read(&out), fields);
        let back = Fields::original().write(&out);
        assert_eq!(ini_get(&back, "General", "sCharGenQuest").as_deref(), Some("00102037"));
        assert_eq!(ini_get(&back, "General", "sIntroMovie").as_deref(), Some("FNVIntro.bik"));
    }

    #[test]
    fn a_start_that_is_neither_ttws_nor_the_base_games_is_custom() {
        assert!(!Fields::ttw().is_custom());
        assert!(!Fields::original().is_custom());
        assert!(!Fields::read("").is_custom());
        assert!(Fields { quest: "MyStart".into(), ..Fields::original() }.is_custom());
        assert!(Fields { film: "Mine.bik".into(), ..Fields::original() }.is_custom());
        assert!(Fields { no_film: true, ..Fields::original() }.is_custom());
    }
}
