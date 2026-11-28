//! `fnv.voice.coverage/1`: the voice files of an install against the dialogue lines of
//! its load order (docs/spec-notes/voice.md §3 quotes the numbers for the ten masters).

use crate::bank::VoiceBank;
use crate::path::{parse_voice_path, SOUND_EXTENSIONS, VOICE_ROOT};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const COVERAGE_FORMAT: &str = "fnv.voice.coverage/1";

/// The extensions of one file name in one voice folder.
type Exts = Vec<String>;

fn is_sound(ext: &str) -> bool {
    SOUND_EXTENSIONS.contains(&ext)
}

fn bump(m: &mut BTreeMap<String, u64>, k: impl Into<String>) {
    *m.entry(k.into()).or_insert(0) += 1;
}

fn share(part: u64, of: u64) -> Value {
    if of == 0 {
        Value::Null
    } else {
        json!((part as f64 / of as f64 * 1e4).round() / 1e4)
    }
}

/// The summary. With `lengths`, every sound's headers are read too (slow: every voice
/// file is read from its archive).
pub fn coverage(bank: &VoiceBank, lengths: bool) -> Value {
    let ix = &bank.index;
    let vfs = bank.vfs();
    let prefix = format!("{VOICE_ROOT}\\");
    let all = vfs.paths(|p| p.starts_with(&prefix));

    // ---- the files ---------------------------------------------------------------------
    // (plugin folder, stem) → voice folder → extensions.
    let mut by_name: HashMap<(String, String), BTreeMap<String, Exts>> = HashMap::new();
    let mut by_ext: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_source: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_plugin: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let mut voice_folders: BTreeMap<String, HashSet<String>> = BTreeMap::new();
    let mut elsewhere = 0u64;
    let mut odd_names = 0u64;
    for p in &all {
        let Some(f) = parse_voice_path(p) else {
            elsewhere += 1;
            continue;
        };
        bump(&mut by_ext, f.ext);
        bump(&mut by_source, vfs.source_of(p).unwrap_or("?"));
        bump(by_plugin.entry(f.plugin.to_string()).or_default(), f.ext);
        voice_folders.entry(f.plugin.to_string()).or_default().insert(f.voice_type.to_string());
        if f.parsed.is_none() && is_sound(f.ext) {
            odd_names += 1;
        }
        by_name.entry((f.plugin.to_string(), f.stem.to_string())).or_default().entry(f.voice_type.to_string()).or_default().push(f.ext.to_string());
    }
    let mut sounds = 0u64;
    let mut sound_with_lip = 0u64;
    let mut lip_without_sound = 0u64;
    let mut both_ogg_and_wav = 0u64;
    for voices in by_name.values() {
        for exts in voices.values() {
            let has_sound = exts.iter().any(|e| is_sound(e));
            let has_lip = exts.iter().any(|e| e == "lip");
            sounds += has_sound as u64;
            sound_with_lip += (has_sound && has_lip) as u64;
            lip_without_sound += (has_lip && !has_sound) as u64;
            both_ogg_and_wav += SOUND_EXTENSIONS.iter().all(|s| exts.iter().any(|e| e == s)) as u64;
        }
    }
    let voice_names: HashSet<String> = ix.voice_types.values().map(|v| v.to_ascii_lowercase()).collect();
    let unknown_voice_folders: u64 = voice_folders.values().flatten().filter(|v| !voice_names.contains(*v)).count() as u64;

    // ---- the lines ---------------------------------------------------------------------
    #[derive(Default)]
    struct Count {
        lines: u64,
        responses: u64,
        with_sound: u64,
        without_sound: u64,
        without_text: u64,
    }
    let mut total = Count::default();
    let mut per_plugin: BTreeMap<usize, Count> = BTreeMap::new();
    let mut lines_without_responses = 0u64;
    let mut unnamed = 0u64;
    let mut voices_per_response: BTreeMap<u64, u64> = BTreeMap::new();
    let mut claimed: HashSet<(String, String)> = HashSet::new();
    let mut override_total = 0u64;
    let mut override_with_voice = 0u64;
    let mut speaker: BTreeMap<String, u64> = BTreeMap::new();
    let mut missing_by_quest: BTreeMap<String, u64> = BTreeMap::new();
    let mut numbers_in_order = 0u64;
    let mut numbers_other = 0u64;
    let mut sound_paths: Vec<(String, usize)> = Vec::new();
    let mut ids: Vec<u32> = ix.lines.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        let line = &ix.lines[&id];
        let pi = (id >> 24) as usize;
        let plugin = ix.plugin_of(id).unwrap_or("?").to_ascii_lowercase();
        total.lines += 1;
        per_plugin.entry(pi).or_default().lines += 1;
        if line.responses.is_empty() {
            lines_without_responses += 1;
        }
        if line.responses.iter().map(|r| r.number as usize).eq(1..=line.responses.len()) {
            numbers_in_order += 1;
        } else if !line.responses.is_empty() {
            numbers_other += 1;
        }
        let speaker_voice = line.speaker.map(|s| ix.voice_of(s).and_then(|v| ix.voice_types.get(&v)).map(|v| v.to_ascii_lowercase()));
        for (k, r) in line.responses.iter().enumerate() {
            let c = per_plugin.entry(pi).or_default();
            total.responses += 1;
            c.responses += 1;
            if r.chars == 0 {
                total.without_text += 1;
                c.without_text += 1;
            }
            let Some(stem) = ix.stem(id, k) else {
                unnamed += 1;
                total.without_sound += 1;
                c.without_sound += 1;
                continue;
            };
            let key = (plugin.clone(), stem);
            let voices: Vec<&String> =
                by_name.get(&key).map(|v| v.iter().filter(|(_, e)| e.iter().any(|x| is_sound(x))).map(|(v, _)| v).collect()).unwrap_or_default();
            claimed.insert(key.clone());
            if r.sound.is_some() {
                override_total += 1;
                override_with_voice += !voices.is_empty() as u64;
            }
            if voices.is_empty() {
                total.without_sound += 1;
                c.without_sound += 1;
                let quest = line.quest.and_then(|q| ix.edids.get(&q)).cloned().unwrap_or_else(|| "(no quest)".into());
                bump(&mut missing_by_quest, quest);
            } else {
                total.with_sound += 1;
                c.with_sound += 1;
                *voices_per_response.entry(voices.len() as u64).or_insert(0) += 1;
                if lengths {
                    for v in &voices {
                        let exts = &by_name[&key][*v];
                        if let Some(ext) = SOUND_EXTENSIONS.iter().find(|s| exts.iter().any(|e| e == *s)) {
                            sound_paths.push((format!("{VOICE_ROOT}\\{}\\{v}\\{}.{ext}", key.0, key.1), r.chars));
                        }
                    }
                }
            }
            match &speaker_voice {
                None => {}
                Some(None) => bump(&mut speaker, "speaker_has_no_voice_type"),
                Some(Some(v)) if voices.contains(&v) => bump(&mut speaker, "file_in_the_speakers_voice"),
                Some(Some(_)) if !voices.is_empty() => bump(&mut speaker, "file_only_in_another_voice"),
                Some(Some(_)) => bump(&mut speaker, "no_file"),
            }
        }
    }

    // ---- sounds no line names ----------------------------------------------------------
    let line_ids: HashSet<(String, u32)> =
        ix.lines.keys().map(|id| (ix.plugin_of(*id).unwrap_or("?").to_ascii_lowercase(), id & 0x00FF_FFFF)).collect();
    let mut unclaimed: BTreeMap<String, u64> = BTreeMap::new();
    for ((plugin, stem), voices) in &by_name {
        if claimed.contains(&(plugin.clone(), stem.clone())) {
            continue;
        }
        let n = voices.values().filter(|e| e.iter().any(|x| is_sound(x))).count() as u64;
        if n == 0 {
            continue;
        }
        // Rebuild a path to split the stem by the rule.
        let probe = format!("{VOICE_ROOT}\\{plugin}\\v\\{stem}.x");
        let why = match parse_voice_path(&probe).and_then(|f| f.parsed) {
            None => "name_not_by_the_rule",
            Some((_, id, _)) if !line_ids.contains(&(plugin.clone(), id)) => "form_id_is_no_line_of_the_plugin",
            Some(_) => "line_exists_under_another_name_or_response",
        };
        *unclaimed.entry(why.to_string()).or_insert(0) += n;
    }

    // ---- lengths (optional) ------------------------------------------------------------
    let length_report = lengths.then(|| {
        sound_paths.sort();
        sound_paths.dedup();
        let (mut read, mut unreadable, mut seconds) = (0u64, 0u64, 0.0f64);
        let (mut shortest, mut longest) = (f64::INFINITY, 0.0f64);
        let mut rates: BTreeMap<String, u64> = BTreeMap::new();
        let mut channels: BTreeMap<String, u64> = BTreeMap::new();
        // Files far shorter than their text needs (`fnv_game::voice::is_placeholder`):
        // the line keeps its fitted length (#4825).
        let (mut placeholders, mut placeholder_examples) = (0u64, Vec::new());
        for (p, chars) in &sound_paths {
            match bank.info(p) {
                Some(i) => {
                    if fnv_game::voice::is_placeholder(i.seconds, *chars) {
                        placeholders += 1;
                        if placeholder_examples.len() < 20 {
                            placeholder_examples.push(json!({"path": p, "seconds": i.seconds, "chars": chars}));
                        }
                    }
                    read += 1;
                    seconds += i.seconds;
                    shortest = shortest.min(i.seconds);
                    longest = longest.max(i.seconds);
                    bump(&mut rates, i.sample_rate.to_string());
                    bump(&mut channels, i.channels.to_string());
                }
                None => unreadable += 1,
            }
        }
        json!({
            "sounds_read": read,
            "unreadable": unreadable,
            "seconds_total": (seconds * 1e3).round() / 1e3,
            "seconds_shortest": if read > 0 { json!(shortest) } else { Value::Null },
            "seconds_longest": if read > 0 { json!(longest) } else { Value::Null },
            "sample_rates": rates,
            "channels": channels,
            "placeholder_sounds": placeholders,
            "placeholder_examples": placeholder_examples,
        })
    });

    let count = |c: &Count| {
        json!({
            "lines": c.lines,
            "responses": c.responses,
            "responses_with_sound": c.with_sound,
            "responses_without_sound": c.without_sound,
            "responses_without_text": c.without_text,
            "share_with_sound": share(c.with_sound, c.responses),
        })
    };
    let mut plugins = Map::new();
    for (pi, c) in &per_plugin {
        plugins.insert(ix.plugins.get(*pi).cloned().unwrap_or_else(|| format!("plugin {pi}")), count(c));
    }
    let mut top_missing: Vec<(String, u64)> = missing_by_quest.into_iter().collect();
    top_missing.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let quests_missing = top_missing.len();
    top_missing.truncate(25);
    let mut out = json!({
        "format": COVERAGE_FORMAT,
        "load_order": ix.plugins,
        "files": {
            "under_voice_root": all.len(),
            "not_in_a_voice_folder": elsewhere,
            "by_extension": by_ext,
            "by_source": by_source,
            "by_plugin_folder": by_plugin,
            "voice_folders": voice_folders.iter().map(|(k, v)| (k.clone(), json!(v.len()))).collect::<Map<String, Value>>(),
            "voice_folders_not_a_voice_type": unknown_voice_folders,
            "sounds": sounds,
            "sounds_with_lip": sound_with_lip,
            "lip_without_sound": lip_without_sound,
            "sounds_as_both_ogg_and_wav": both_ogg_and_wav,
            "sound_names_not_by_the_rule": odd_names,
            "sounds_no_line_names": unclaimed,
        },
        "voice_types": ix.voice_types.len(),
        "lines": {
            "total": count(&total),
            "by_plugin": plugins,
            "lines_without_responses": lines_without_responses,
            "responses_without_quest_or_topic_name": unnamed,
            "lines_numbered_1_to_n_in_order": numbers_in_order,
            "lines_numbered_otherwise": numbers_other,
            "voices_per_response_with_sound": voices_per_response.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<Map<String, Value>>(),
            "responses_of_lines_with_a_fixed_speaker": speaker,
            "responses_with_a_sound_form": { "total": override_total, "with_a_voice_file": override_with_voice },
            "quests_with_responses_without_sound": quests_missing,
            "most_responses_without_sound": top_missing.into_iter().map(|(q, n)| json!({"quest": q, "responses": n})).collect::<Vec<_>>(),
        },
    });
    if let Some(l) = length_report {
        out["lengths"] = l;
    }
    out
}
