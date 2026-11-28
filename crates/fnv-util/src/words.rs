//! Translations of NVForever's own words (#1443): the labels, notes and messages it writes
//! itself because the original's are not copied (LEGAL.md). The game's own text comes
//! from the plugins of the install and needs none of this.
//!
//! English is written in the code and is the key: a call site says
//! `words::tr("Accepted.")` and gets that text in the chosen language, or the English when
//! the language has no translation for it (or English is chosen). A language is one file,
//! `<id>.toml`, in a `lang` folder NVForever looks in ([`load`]), UTF-8:
//!
//! ```toml
//! name = "Deutsch"
//!
//! [words]
//! "Accepted." = "Angenommen."
//! "{n} of {len} right" = "{n} von {len} richtig"
//! ```
//!
//! `name` is how the settings page lists the language. A text with `{name}` places is
//! filled by [`trf`]; a translation may put them in another order. The languages are
//! read once at start; the chosen one can change at any time ([`set_language`]), and
//! [`generation`] counts the changes so that text drawn once can be drawn again.

use crate::text::CodePage;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::OnceLock;

/// The language written in the code.
pub const ENGLISH: &str = "en";
const ENGLISH_NAME: &str = "English";

/// One language's words.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    /// The file's name without `.toml` (`de`, `ru`), as NVForever.ini keeps the choice.
    pub id: String,
    /// The language's own name for itself, as the settings page lists it.
    pub name: String,
    words: HashMap<String, &'static str>,
}

#[derive(serde::Deserialize)]
struct File {
    name: String,
    #[serde(default)]
    words: HashMap<String, String>,
}

impl Table {
    /// A language from its file's text; Err says what is wrong. A translation is kept for
    /// the run (the languages are read once, so this is bounded).
    pub fn parse(id: &str, text: &str) -> Result<Table, String> {
        let f: File = toml::from_str(text).map_err(|e| e.to_string())?;
        let name = f.name.trim().to_string();
        if name.is_empty() {
            return Err("no name".into());
        }
        let words = f.words.into_iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| (k, &*Box::leak(v.into_boxed_str()))).collect();
        Ok(Table { id: id.to_string(), name, words })
    }

    /// The translation of `en`, if the table has one.
    pub fn get(&self, en: &str) -> Option<&'static str> {
        self.words.get(en).copied()
    }

    pub fn len(&self) -> usize {
        self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// Every (English, translation) pair, in no order.
    pub fn pairs(&self) -> impl Iterator<Item = (&str, &'static str)> {
        self.words.iter().map(|(k, v)| (k.as_str(), *v))
    }
}

static TABLES: OnceLock<Vec<Table>> = OnceLock::new();
/// 0: English; n: `TABLES[n - 1]`.
static CURRENT: AtomicUsize = AtomicUsize::new(0);
static GENERATION: AtomicU32 = AtomicU32::new(0);

/// The languages in the `lang` folders `dirs`, in that order (an id found again in a
/// later folder is left out); each file that cannot be read with why. `en.toml` is not
/// a language: English is the code's own.
pub fn read_dirs(dirs: &[PathBuf]) -> (Vec<Table>, Vec<String>) {
    let mut tables: Vec<Table> = Vec::new();
    let mut problems = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("toml"))).collect();
        files.sort();
        for path in files {
            let Some(id) = path.file_stem().map(|s| s.to_string_lossy().to_lowercase()) else { continue };
            if id == ENGLISH || tables.iter().any(|t| t.id == id) {
                continue;
            }
            match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|text| Table::parse(&id, &text)) {
                Ok(t) => tables.push(t),
                Err(e) => problems.push(format!("{}: {e}", path.display())),
            }
        }
    }
    (tables, problems)
}

/// Read the languages of `dirs` ([`read_dirs`]) for this run, once (a second call reads
/// nothing); the files that could not be read, with why.
pub fn load(dirs: &[PathBuf]) -> Vec<String> {
    let mut problems = Vec::new();
    TABLES.get_or_init(|| {
        let (tables, p) = read_dirs(dirs);
        problems = p;
        tables
    });
    problems
}

fn tables() -> &'static [Table] {
    TABLES.get().map_or(&[], Vec::as_slice)
}

/// The folders NVForever looks for languages in: `lang` beside the program (its own
/// translations; a release has it there), then a source checkout's `lang` when the program
/// is a build of one and has none beside it ([`checkout_lang`], #2929), then `lang` in the
/// home (the player's), then `NVF_LANG_DIR` if set.
pub fn dirs(exe_dir: Option<&Path>, home: Option<&Path>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = exe_dir.into_iter().map(|d| d.join("lang")).collect();
    out.extend(exe_dir.and_then(checkout_lang));
    out.extend(home.map(|d| d.join("lang")));
    if let Some(d) = std::env::var_os("NVF_LANG_DIR").filter(|d| !d.is_empty()) {
        out.push(PathBuf::from(d));
    }
    out
}

/// The repository's `lang` for a program built in a source checkout (`target/release`,
/// `engine/target/release`, ...): the nearest ancestor of `exe_dir` holding `engine/Cargo.toml`
/// and a `lang` folder, the way the launcher finds the checkout's game. `None` when `lang`
/// is beside the program (a release) or no ancestor is a checkout.
pub fn checkout_lang(exe_dir: &Path) -> Option<PathBuf> {
    if exe_dir.join("lang").is_dir() {
        return None;
    }
    exe_dir.ancestors().skip(1).find(|a| a.join("engine").join("Cargo.toml").is_file()).map(|root| root.join("lang")).filter(|l| l.is_dir())
}

/// Every language this run can show: English first, then the ones read, as (id, name).
pub fn languages() -> Vec<(&'static str, &'static str)> {
    std::iter::once((ENGLISH, ENGLISH_NAME)).chain(tables().iter().map(|t| (t.id.as_str(), t.name.as_str()))).collect()
}

/// The chosen language's id.
pub fn language() -> &'static str {
    match CURRENT.load(Ordering::Relaxed) {
        0 => ENGLISH,
        n => tables().get(n - 1).map_or(ENGLISH, |t| t.id.as_str()),
    }
}

/// Choose a language by id or by its listed name (case not minded); false: none such.
pub fn set_language(id_or_name: &str) -> bool {
    let want = id_or_name.trim();
    let n = if want.eq_ignore_ascii_case(ENGLISH) || want.eq_ignore_ascii_case(ENGLISH_NAME) {
        0
    } else {
        match tables().iter().position(|t| t.id.eq_ignore_ascii_case(want) || t.name.eq_ignore_ascii_case(want)) {
            Some(i) => i + 1,
            None => return false,
        }
    };
    if CURRENT.swap(n, Ordering::Relaxed) != n {
        GENERATION.fetch_add(1, Ordering::Relaxed);
    }
    true
}

/// The language of the game's text when its code page tells it (#1948): Windows-1251 is
/// Russian, the one Cyrillic release of the game [hypothesis: one Russian install looked
/// at, docs/spec-notes/fonts.md §2.1]. Windows-1250 (Polish or Czech) and 1252 (English,
/// French, German, Italian, Spanish) are each several languages, so they tell none.
/// Code page 936 also leaves the interface language at its default.
pub fn for_code_page(page: CodePage) -> Option<&'static str> {
    match page {
        CodePage::Windows1251 => Some("ru"),
        CodePage::Windows1250 | CodePage::Windows1252 | CodePage::Windows936 => None,
    }
}

/// The code page of the game's text and the language of NVForever's own words for the
/// language Steam installed the game in (a Steamworks language name, lower case, as
/// `fnv_install::steam::manifest_language` reads it) (#1443). It tells the languages the
/// text cannot: Windows-1252 is English, French, German, Italian or Spanish, and 1250 is
/// Polish, Czech or Hungarian [documented: the Windows code pages' scripts]; the words'
/// language is the one of the shipped files (`lang/`), none for English and for 1250's.
/// `None`: a name this build does not know.
pub fn for_steam_language(name: &str) -> Option<(CodePage, Option<&'static str>)> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "english" => (CodePage::Windows1252, None),
        "german" => (CodePage::Windows1252, Some("de")),
        "french" => (CodePage::Windows1252, Some("fr")),
        "italian" => (CodePage::Windows1252, Some("it")),
        "spanish" | "latam" => (CodePage::Windows1252, Some("es")),
        "russian" => (CodePage::Windows1251, Some("ru")),
        "polish" | "czech" | "hungarian" => (CodePage::Windows1250, None),
        _ => return None,
    })
}

/// At the start, before NVForever.ini's rows are applied: choose `id` (the language Steam
/// installed the game in, [`for_steam_language`]) when a file of it was read. The id
/// chosen, if one was.
pub fn follow_language(id: &str) -> Option<&'static str> {
    set_language(id).then(language)
}

/// At the start, before NVForever.ini's rows are applied: choose the language of the
/// game's text ([`for_code_page`]) when a file of it was read. A `language=` in the INI,
/// applied after, still wins. The id chosen, if one was.
pub fn follow_code_page(page: CodePage) -> Option<&'static str> {
    let id = for_code_page(page)?;
    set_language(id).then(language)
}

/// Counts the changes of language: text drawn under an older count is out of date.
pub fn generation() -> u32 {
    GENERATION.load(Ordering::Relaxed)
}

/// `en` in the chosen language: its translation, else `en` itself.
pub fn tr(en: &'static str) -> &'static str {
    tr_in(en, current())
}

fn current() -> Option<&'static Table> {
    match CURRENT.load(Ordering::Relaxed) {
        0 => None,
        n => tables().get(n - 1),
    }
}

/// `en` in a given table (`None`: English).
pub fn tr_in(en: &'static str, table: Option<&Table>) -> &'static str {
    table.and_then(|t| t.get(en)).unwrap_or(en)
}

/// [`tr`] of a text made at run time (a choice's name from a list, a slider's value): its
/// translation when the chosen language has a line for it, else `text` itself, so a name
/// with no line (a resolution, a monitor, a language's own name) shows as it is.
pub fn tr_str(text: &str) -> &str {
    str_in(text, current())
}

/// [`tr_str`] in a given table (`None`: English).
pub fn str_in<'a>(text: &'a str, table: Option<&Table>) -> &'a str {
    table.and_then(|t| t.get(text)).unwrap_or(text)
}

/// [`tr`] of a text with `{name}` places, each filled with its value from `with`.
pub fn trf(en: &'static str, with: &[(&str, &dyn std::fmt::Display)]) -> String {
    fill(tr(en), with)
}

/// `text` with every `{name}` of `with` replaced by its value; other braces kept.
pub fn fill(text: &str, with: &[(&str, &dyn std::fmt::Display)]) -> String {
    let mut out = text.to_string();
    for (name, value) in with {
        out = out.replace(&format!("{{{name}}}"), &value.to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DE: &str = "name = \"Deutsch\"\n\n[words]\n\"Accepted.\" = \"Angenommen.\"\n\"{n} of {len} right\" = \"{len}: {n} richtig\"\n\"Empty\" = \"\"\n";

    #[test]
    fn a_table_translates_and_falls_back_to_english() {
        let t = Table::parse("de", DE).unwrap();
        assert_eq!((t.id.as_str(), t.name.as_str(), t.len()), ("de", "Deutsch", 2));
        assert_eq!(tr_in("Accepted.", Some(&t)), "Angenommen.");
        assert_eq!(tr_in("Not accepted", Some(&t)), "Not accepted");
        // An empty translation is no translation.
        assert_eq!(tr_in("Empty", Some(&t)), "Empty");
        assert_eq!(tr_in("Accepted.", None), "Accepted.");
        // A text made at run time, as a choice's name is.
        let made = String::from("Accepted.");
        assert_eq!(str_in(&made, Some(&t)), "Angenommen.");
        assert_eq!(str_in("1920x1080", Some(&t)), "1920x1080");
        assert_eq!(str_in(&made, None), "Accepted.");
        // Places may change order.
        assert_eq!(fill(tr_in("{n} of {len} right", Some(&t)), &[("n", &4), ("len", &7)]), "7: 4 richtig");
        assert_eq!(fill("{n} of {len} right", &[("n", &4), ("len", &7)]), "4 of 7 right");
    }

    #[test]
    fn a_broken_file_says_why() {
        assert!(Table::parse("x", "words = 3").is_err());
        assert!(Table::parse("x", "name = \"  \"").is_err());
        assert!(Table::parse("x", "name = \"X\"\n[words\n").is_err());
        assert!(Table::parse("x", "name = \"X\"").unwrap().is_empty());
    }

    #[test]
    fn folders_are_read_in_order_and_english_is_the_codes_own() {
        let root = std::env::temp_dir().join(format!("nvf-words-{}", std::process::id()));
        let (a, b) = (root.join("a"), root.join("b"));
        for d in [&a, &b] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(a.join("de.toml"), DE).unwrap();
        std::fs::write(a.join("en.toml"), "name = \"Not English\"").unwrap();
        std::fs::write(a.join("notes.txt"), "not a language").unwrap();
        std::fs::write(b.join("DE.toml"), "name = \"Second\"").unwrap();
        std::fs::write(b.join("ru.toml"), "name = \"Русский\"\n[words]\n\"Accepted.\" = \"Принято.\"\n").unwrap();
        std::fs::write(b.join("zz.toml"), "broken").unwrap();
        let (tables, problems) = read_dirs(&[a, b, root.join("missing")]);
        let ids: Vec<(&str, &str)> = tables.iter().map(|t| (t.id.as_str(), t.name.as_str())).collect();
        assert_eq!(ids, [("de", "Deutsch"), ("ru", "Русский")]);
        assert_eq!(tables[1].get("Accepted."), Some("Принято."));
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("zz.toml"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_folders_are_the_programs_then_the_homes() {
        let d = dirs(Some(Path::new("C:/nvf")), Some(Path::new("C:/home")));
        assert_eq!(&d[..2], [PathBuf::from("C:/nvf/lang"), PathBuf::from("C:/home/lang")]);
    }

    /// #2929: a build in a source checkout reads the repository's `lang`; a program with
    /// `lang` beside it (a release) does not look further.
    #[test]
    fn a_checkouts_build_reads_the_repositorys_lang() {
        let root = std::env::temp_dir().join(format!("nvf-words-checkout-{}", std::process::id()));
        let game = root.join("engine").join("target").join("release");
        let launcher = root.join("target").join("release");
        for d in [&game, &launcher, &root.join("lang")] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(root.join("engine").join("Cargo.toml"), "").unwrap();
        for exe in [&game, &launcher] {
            assert_eq!(checkout_lang(exe), Some(root.join("lang")));
            assert_eq!(dirs(Some(exe), None)[..2], [exe.join("lang"), root.join("lang")]);
        }
        std::fs::create_dir_all(game.join("lang")).unwrap();
        assert_eq!(checkout_lang(&game), None);
        let release = std::env::temp_dir().join(format!("nvf-words-release-{}", std::process::id()));
        std::fs::create_dir_all(&release).unwrap();
        assert_eq!(checkout_lang(&release), None);
        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&release).ok();
    }

    #[test]
    fn only_a_code_page_of_one_language_names_it() {
        assert_eq!(for_code_page(CodePage::Windows1251), Some("ru"));
        // Steam's language names tell what the code page cannot (#1443).
        assert_eq!(for_steam_language("english"), Some((CodePage::Windows1252, None)));
        assert_eq!(for_steam_language("German"), Some((CodePage::Windows1252, Some("de"))));
        assert_eq!(for_steam_language("polish"), Some((CodePage::Windows1250, None)));
        assert_eq!(for_steam_language("russian"), Some((CodePage::Windows1251, Some("ru"))));
        assert_eq!(for_steam_language("klingon"), None);
        assert_eq!(for_code_page(CodePage::Windows1250), None);
        assert_eq!(for_code_page(CodePage::Windows1252), None);
        assert_eq!(for_code_page(CodePage::Windows936), None);
    }

    #[test]
    fn without_languages_english_is_all_there_is() {
        // No test loads tables into the run's list, so only English can be chosen.
        assert_eq!(languages()[0], (ENGLISH, "English"));
        assert!(set_language("English") && set_language("en"));
        assert!(!set_language("xx"));
        assert_eq!(language(), ENGLISH);
        // A Russian install's text asks for Russian, but no file of it was read.
        assert_eq!(follow_code_page(CodePage::Windows1251), None);
        assert_eq!(language(), ENGLISH);
        assert_eq!(tr("Accepted."), "Accepted.");
        assert_eq!(trf("{n} of {len} right", &[("n", &1), ("len", &5)]), "1 of 5 right");
    }
}
