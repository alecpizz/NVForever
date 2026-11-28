//! A Mod Organizer 2 merge's files reach the mods' file commands: the host answers for files
//! no read root has on disk (`Roots::list`, `Roots::reader`, `Roots::lookup`) [observed:
//! Nuclear Sunset's MAPMO and Immersive Fast Travel Encounters read their `Config` INI files
//! from their own mod folders; with nothing found the HUD drew black and an error box opened].

use fnv_game::mod_files::{self, set_roots, Roots};
use std::sync::Arc;

const INI: &str = r"config\lime\encounters.ini";

fn merged() {
    set_roots(Roots {
        list: Some(Arc::new(|pre: &str| {
            let all = [INI, r"config\lime\sleepencounters.ini", r"config\other.ini", r"meshes\x.nif"];
            all.iter().filter(|p| p.starts_with(&format!("{pre}\\"))).map(|p| p.to_string()).collect()
        })),
        reader: Some(Arc::new(|p: &str| p.eq_ignore_ascii_case(INI).then(|| b"[Main]\niChance=20\n".to_vec()))),
        lookup: Some(Arc::new(|p: &str| u8::from(p.eq_ignore_ascii_case(INI)))),
        ..Roots::default()
    });
}

#[test]
fn a_merges_files_are_listed_read_and_found() {
    merged();
    assert_eq!(mod_files::files_in(r"Config\Lime"), vec!["encounters.ini".to_string(), "sleepencounters.ini".to_string()]);
    assert_eq!(mod_files::files_under("config"), vec![r"lime\encounters.ini".to_string(), r"lime\sleepencounters.ini".to_string(), "other.ini".to_string()]);
    let entries = mod_files::entries_in("config");
    assert!(entries.contains(&"lime".to_string()) && entries.contains(&"other.ini".to_string()), "{entries:?}");
    assert_eq!(mod_files::read_loose(r"Config/Lime/Encounters.ini").as_deref(), Some(&b"[Main]\niChance=20\n"[..]));
    assert!(mod_files::exists(r"Config\Lime\Encounters.ini"));
}
