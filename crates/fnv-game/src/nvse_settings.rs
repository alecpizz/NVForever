//! xNVSE's INI functions (docs/handoff/xnvse-remaining.md §3, "settings and INI"):
//! GetNumericINISetting, GetStringIniSetting and their setters. A setting is named
//! "SettingName:CategoryName", the category being the INI section [documented:
//! GetNumericINISetting; GetStringIniSetting]. The values come from the game's INI text
//! the host gives (`GameData::ini_texts`, later files overriding earlier ones); the
//! setters change them for the session only: NVForever never writes the game's INI
//! files (install-layout.md) [hypothesis: the original's setters change memory, not the
//! files; observed: nvse-sixth left the owner's INI files as they were].

use crate::game::Game;
use fnv_script::vm::XVal;
use std::collections::HashMap;

/// The INI settings, by lower-case "name:category", read on first use.
#[derive(Debug, Default, Clone)]
pub struct IniState {
    values: Option<HashMap<String, String>>,
}

/// Every `key=value` of INI text under its section, as lower-case "key:section".
fn parse(text: &str, into: &mut HashMap<String, String>) {
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            section = rest.trim_end_matches(']').trim().to_ascii_lowercase();
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            into.insert(format!("{}:{section}", k.trim().to_ascii_lowercase()), v.trim().to_string());
        }
    }
}

impl IniState {
    /// The files are read again on the next use, so the session's changes go
    /// (con_RefreshINI).
    pub(crate) fn reload(&mut self) {
        self.values = None;
    }

    fn values(&mut self, texts: &[String]) -> &mut HashMap<String, String> {
        self.values.get_or_insert_with(|| {
            let mut m = HashMap::new();
            for t in texts {
                parse(t, &mut m);
            }
            m
        })
    }
}

/// The setting a name argument names: lower case, with its category. A name without
/// ":" names nothing [observed: nvse-sixth, "uGridsToLoad" gives -1].
fn key(args: &[XVal]) -> Option<String> {
    match args.first()? {
        XVal::Str(s) if s.contains(':') => Some(s.to_ascii_lowercase()),
        _ => None,
    }
}

impl Game<'_> {
    /// A numeric setting of the game's INI text by lower-case "name:category", as
    /// GetNumericINISetting reads it; `None` when no file the host gave has it.
    pub fn ini_number(&mut self, key: &str) -> Option<f64> {
        let texts = &self.data.ini_texts;
        self.ini.values(texts).get(key).and_then(|s| s.parse::<f64>().ok())
    }
}

/// The game's display size settings, `iSize W` and `iSize H` under `[Display]`: the
/// original sizes its window from them, so in the original they are the window's size
/// [observed: jip9 cursor.*, the original's pointer sat at the centre of a 1280 by 720
/// window on an install whose FalloutPrefs.ini says iSize W=1280, iSize H=720].
/// NVForever sizes its window from its own settings, not the game's INI, so these two
/// read the window's size in pixels as the engine last told it, above any INI text
/// [design: scripts compare them with GetCursorPos's pixels; an INI that is absent (no
/// `--mo2`) or names another size would put the window's edge in the wrong place].
/// `None` for any other setting, or before the engine tells a size (headless runs keep
/// the INI's value, else none).
fn live_display_size(g: &Game, key: &str) -> Option<f64> {
    let (w, h) = g.other.jip9.window.filter(|(w, h)| *w > 0.0 && *h > 0.0)?;
    match key {
        "isize w:display" => Some(w),
        "isize h:display" => Some(h),
        _ => None,
    }
}

/// A setting's text: the live display size for `iSize W`/`iSize H`
/// ([`live_display_size`]), else the INI text's value.
fn setting_text(g: &mut Game, texts: &[String], key: &str) -> Option<String> {
    if let Some(v) = live_display_size(g, key) {
        return Some(number_text(v));
    }
    g.ini.values(texts).get(key).cloned()
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, name: &str, args: &[XVal]) -> Option<Result<XVal, String>> {
    let texts = g.data.ini_texts.clone();
    let v = match name {
        // A missing setting is -1 [observed: nvse-sixth, ini.missing; names ignore case,
        // ini.case].
        // (GetNumericINISetting and GetNumericIniSetting are one name in lower case.)
        "getnumericinisetting" => {
            let v = key(args).and_then(|k| setting_text(g, &texts, &k));
            XVal::Num(v.and_then(|s| s.parse::<f64>().ok()).unwrap_or(-1.0))
        }
        // A missing string setting gives no value: the call fails [observed: nvse-sixth,
        // ini.string.missing "script call failed"].
        "getstringinisetting" => match key(args).and_then(|k| setting_text(g, &texts, &k)) {
            Some(s) => XVal::Str(s),
            None => return Some(Err("GetStringIniSetting: no such INI setting".into())),
        },
        // 1 when the setting exists and was changed, 0 otherwise [documented:
        // SetNumericINISetting; observed: nvse-sixth, ini.set.numeric 1, missing 0].
        "setnumericinisetting" => {
            let value = match args.get(1) {
                Some(XVal::Num(x)) => *x,
                _ => return Some(Ok(XVal::Num(0.0))),
            };
            let ok = match key(args) {
                Some(k) => match g.ini.values(&texts).get_mut(&k) {
                    Some(slot) => {
                        *slot = crate::nvse_settings::number_text(value);
                        true
                    }
                    None => false,
                },
                None => false,
            };
            XVal::Num(f64::from(u8::from(ok)))
        }
        // Sets an existing setting [documented: SetStringIniSetting; observed: nvse-sixth,
        // ini.set.string]; a missing one is left alone [hypothesis].
        "setstringinisetting" => {
            if let (Some(k), Some(XVal::Str(new))) = (key(args), args.get(1)) {
                if let Some(slot) = g.ini.values(&texts).get_mut(&k) {
                    *slot = new.clone();
                }
            }
            XVal::Num(0.0)
        }
        _ => return None,
    };
    Some(Ok(v))
}

/// A number as INI text: whole numbers without a fraction.
pub(crate) fn number_text(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 { format!("{}", x as i64) } else { format!("{x}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sections_and_keys_without_case() {
        let mut m = HashMap::new();
        parse("[General]\nuGridsToLoad=5\n; comment\nsLanguage=ENGLISH\n[Display]\nfDefaultFOV=75.0000\n", &mut m);
        parse("[General]\nuGridsToLoad=7\n", &mut m);
        assert_eq!(m.get("ugridstoload:general").map(String::as_str), Some("7"));
        assert_eq!(m.get("slanguage:general").map(String::as_str), Some("ENGLISH"));
        assert_eq!(m.get("fdefaultfov:display").map(String::as_str), Some("75.0000"));
        assert_eq!(key(&[XVal::Str("uGridsToLoad".into())]), None);
        assert_eq!(number_text(80.0), "80");
    }
}
