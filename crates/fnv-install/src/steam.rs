//! The language Steam installed the game in (#1443): read from Steam's install manifest
//! beside the game's folder, read-only.
//!
//! A Steam library keeps each installed app's manifest as `steamapps\appmanifest_<appid>.acf`,
//! the app's files under `steamapps\common\<folder>` [observed: the owner's library,
//! `F:\SteamLibrary\steamapps\appmanifest_22380.acf` beside `common\Fallout New Vegas`].
//! Fallout: New Vegas is app 22380 [observed: the manifest's `"appid" "22380"` and `"name"
//! "Fallout: New Vegas"`]. The manifest is Valve's text key-value format: quoted keys and
//! values, blocks in braces [observed]. Its `"UserConfig"` block holds `"language"`, the
//! language the player chose for the game in Steam (`"english"` on the owner's English
//! install) [observed]; the names are the Steamworks API language names ("german",
//! "french", "polish", …) [documented: Steamworks documentation, Supported Languages].
//! That Steam installs the chosen language's data files is [hypothesis: no localized
//! install looked at, observer check 77].
//!
//! Only reads; a copy of the game outside a Steam library has no manifest and tells none.

use std::path::Path;

/// Fallout: New Vegas's Steam app id [observed].
pub const APP_ID: u32 = 22380;

/// The language in the Steam manifest of the install at `game_root` (the folder holding
/// `Data`), lower case; `None` when the install is not in a Steam library or the manifest
/// names none.
pub fn manifest_language(game_root: &Path) -> Option<String> {
    let steamapps = game_root.parent()?.parent()?;
    let manifest = crate::child_ci(steamapps, &format!("appmanifest_{APP_ID}.acf"))?;
    user_language(&String::from_utf8_lossy(&std::fs::read(manifest).ok()?))
}

/// The `"language"` inside the top block's `"UserConfig"` block of a manifest's text.
pub fn user_language(acf: &str) -> Option<String> {
    let mut depth = 0usize;
    let mut in_user = None;
    let mut last_key: Option<String> = None;
    let tokens = Tokens(acf.chars().peekable());
    for t in tokens {
        match t {
            Token::Open => {
                if in_user.is_none() && last_key.as_deref().is_some_and(|k| k.eq_ignore_ascii_case("UserConfig")) {
                    in_user = Some(depth + 1);
                }
                depth += 1;
                last_key = None;
            }
            Token::Close => {
                if in_user == Some(depth) {
                    in_user = None;
                }
                depth = depth.saturating_sub(1);
                last_key = None;
            }
            Token::Text(s) => match last_key.take() {
                None => last_key = Some(s),
                Some(k) => {
                    if in_user == Some(depth) && k.eq_ignore_ascii_case("language") && !s.trim().is_empty() {
                        return Some(s.trim().to_ascii_lowercase());
                    }
                }
            },
        }
    }
    None
}

enum Token {
    Open,
    Close,
    Text(String),
}

struct Tokens<'a>(std::iter::Peekable<std::str::Chars<'a>>);

impl Iterator for Tokens<'_> {
    type Item = Token;
    fn next(&mut self) -> Option<Token> {
        loop {
            match self.0.next()? {
                '{' => return Some(Token::Open),
                '}' => return Some(Token::Close),
                '"' => {
                    let mut s = String::new();
                    while let Some(c) = self.0.next() {
                        match c {
                            '"' => break,
                            '\\' => {
                                if let Some(e) = self.0.next() {
                                    s.push(e);
                                }
                            }
                            c => s.push(c),
                        }
                    }
                    return Some(Token::Text(s));
                }
                '/' if self.0.peek() == Some(&'/') => {
                    for c in self.0.by_ref() {
                        if c == '\n' {
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of the owner's manifest [observed], shortened: the language is the one in
    /// UserConfig, not a depot's or MountedConfig's.
    const MANIFEST: &str = "\"AppState\"\n{\n\t\"appid\"\t\t\"22380\"\n\t\"name\"\t\t\"Fallout: New Vegas\"\n\t\"InstalledDepots\"\n\t{\n\t\t\"22381\"\n\t\t{\n\t\t\t\"manifest\"\t\t\"1\"\n\t\t\t\"language\"\t\t\"french\"\n\t\t}\n\t}\n\t\"UserConfig\"\n\t{\n\t\t\"language\"\t\t\"German\"\n\t}\n\t\"MountedConfig\"\n\t{\n\t\t\"language\"\t\t\"english\"\n\t}\n}\n";

    #[test]
    fn the_user_config_language_is_read() {
        assert_eq!(user_language(MANIFEST).as_deref(), Some("german"));
        assert_eq!(user_language("\"AppState\" { \"UserConfig\" { } }"), None);
        assert_eq!(user_language(""), None);
    }

    #[test]
    fn the_manifest_is_found_two_folders_up() {
        let dir = std::env::temp_dir().join(format!("nvf-steam-{}", std::process::id()));
        let root = dir.join("steamapps").join("common").join("Fallout New Vegas");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(dir.join("steamapps").join("appmanifest_22380.acf"), MANIFEST).unwrap();
        assert_eq!(manifest_language(&root).as_deref(), Some("german"));
        assert_eq!(manifest_language(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
