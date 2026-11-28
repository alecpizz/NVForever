//! The registered extender commands no wiki page lists, the rest after the mod lists'
//! (#2326), batch 1: SUP's network services and test commands, the debug and address
//! commands, kept as stated neutral answers (extender-compat.md §5.8); ShowOff's `_OLD`
//! commands, which answer as the current ones; IsJumping_BROKEN; StringContainsOnlyASCII,
//! GetFuncCodeByName, GetFuncNameByCode and IsNewGame. ShowOff's string maps are the VM's
//! (`fnv_script::vm::jip`). Argument shapes and answers from probes in the original game
//! (set rest1 of tools/capture/jip_probes.py and the compile-shape run), in our words in
//! docs/spec-notes/ext-unlisted-rest.md, which every comment here cites.

use crate::game::{Game, RunCtx};
use fnv_script::vm::XVal;
use fnv_sim::FormId;

/// Commands that reach a network service, write the plugin's own log, test the plugin or
/// read the original's memory: NVForever answers 0 and does nothing (§2) [decision:
/// extender-compat.md §5.8, NoOp for the services and tests, refused for RefAddrxData].
pub const NEUTRAL: &[&str] = &[
    // SUP's bulletin board (a web service) [hypothesis: by the names; never probed].
    "bbpostmessage", "bbreturnallmessagestodefaultcell", "bboncellchange", "bbgetinfoforreference", "bbisreferencenote",
    "bbnexusapikeyexist", "bbapikeyexist", "bbgetbbkey", "bbisbbloaded", "bbaremessagesfetched", "bbgetusermessages",
    "bbdeleteusermessage", "bblikemessage", "bbistexteditmenuactive", "bbgettextmenustring", "bbgetuserinfo",
    "bbsettextmenutitle", "bbreportmessage", "bbhidemessage", "bbgetcurrentnotesincell",
    // SUP's update checker and pastebin reader (web services).
    "updatecheckercheckforupdates", "updatecheckerreturnarray", "getsupdatecheckercurrentstage", "getsupdatecheckerdonestage",
    "getpastebinreaderdonestage", "getpastebinreadercurrentstage",
    // Tests, the plugins' own logs, a memory address, a reset of the plugin.
    "suptest", "erdtestfunc", "lunettecmd", "refaddrxdata", "nullargs", "knvsereset", "dumptileinfochilds", "supwritetolog",
];

/// What these commands keep for the session.
#[derive(Debug, Default)]
pub struct Unlisted2State {
    /// The game was started from the New Game menu and no save has been loaded since.
    pub new_game: bool,
    /// SetRaceVoice / SetRaceAgeRace for the session: (race, voice, second) -> form.
    pub race_sets: std::collections::BTreeMap<(FormId, bool, bool), FormId>,
}

/// The current command an `_OLD` one answers as [observed: rest1.old.* and new.* read
/// the same where both compiled: IsMatrix, Dump, ListAddArray] (§4).
fn current_of(name: &str) -> Option<&str> {
    name.strip_suffix("_old")
}

/// The name a command opcode belongs to: the catalogue's own name for it, else the
/// longest of the names the opcode table gives it (an alias is the shorter) (§5).
fn name_of_code(g: &Game, code: u32) -> Option<String> {
    let names = crate::nvse_debug::opcode_names(code);
    let canonical = names.iter().find_map(|n| g.data.table.by_name(n).and_then(|op| g.data.table.get(op)).map(|f| f.name.clone()));
    canonical.or_else(|| names.iter().max_by_key(|n| n.len()).cloned())
}

fn num_arg(args: &[XVal], k: usize) -> Option<f64> {
    match args.get(k)? {
        XVal::Num(n) => Some(*n),
        _ => None,
    }
}

/// A race's older or younger race (ONAM / YNAM) or its male or female voice (VTCK).
fn race_field(g: &Game, race: FormId, voice: bool, second: bool) -> Option<FormId> {
    let subs = g.data.record_fields.get(&race)?;
    let hex = |v: &serde_json::Value| v.as_str().and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok()).filter(|&f| f != 0).map(FormId);
    if voice {
        let v = subs.iter().find(|(s, _)| s == b"VTCK")?;
        return hex(&v.1[if second { "female" } else { "male" }]);
    }
    let sig: &[u8; 4] = if second { b"YNAM" } else { b"ONAM" };
    subs.iter().find(|(s, _)| s == sig).and_then(|(_, v)| hex(v))
}

fn form_arg(args: &[XVal], k: usize) -> Option<FormId> {
    match args.get(k)? {
        XVal::Ref(0) => None,
        XVal::Ref(r) => Some(FormId(*r)),
        _ => None,
    }
}

/// Whether `name` matches a wildcard pattern (`*` any run, `?` one character), without
/// regard to case.
fn wild(pattern: &str, name: &str) -> bool {
    fn go(p: &[char], n: &[char]) -> bool {
        match (p.first(), n.first()) {
            (None, None) => true,
            (Some('*'), _) => go(&p[1..], n) || (!n.is_empty() && go(p, &n[1..])),
            (Some('?'), Some(_)) => go(&p[1..], &n[1..]),
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b) && go(&p[1..], &n[1..]),
            _ => false,
        }
    }
    let (p, n): (Vec<char>, Vec<char>) = (pattern.chars().collect(), name.chars().collect());
    go(&p, &n)
}

/// GetDirectoryFiles's list for a pattern under the game folder (§7).
fn directory_files(pattern: &str) -> Vec<String> {
    let (dir, last) = pattern.rsplit_once('\\').unwrap_or(("", pattern));
    if last.is_empty() {
        return Vec::new();
    }
    let data_rel = |d: &str| -> Option<String> {
        let low = d.to_ascii_lowercase();
        if low == "data" {
            Some(String::new())
        } else {
            low.strip_prefix("data\\").map(|_| d[5..].to_string())
        }
    };
    if !last.contains(['*', '?']) {
        // The folder or file itself, when there is one.
        let whole = data_rel(pattern);
        let there = whole.is_some_and(|w| crate::mod_files::exists(&w) || !crate::mod_files::entries_in(&w).is_empty());
        return if there { vec![pattern.to_string()] } else { Vec::new() };
    }
    let Some(rel) = data_rel(dir) else { return Vec::new() };
    let mut names = crate::mod_files::entries_in(&rel);
    names.extend([".".to_string(), "..".to_string()]);
    let mut out: Vec<String> = names.into_iter().filter(|n| wild(last, n)).collect();
    out.sort_by_key(|n| std::cmp::Reverse(n.to_ascii_lowercase()));
    out.into_iter().map(|n| format!("{dir}\\{n}")).collect()
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    if NEUTRAL.contains(&name) {
        return Some(Ok(XVal::Num(0.0)));
    }
    if let Some(current) = current_of(name) {
        return crate::nvse::call(g, ctx, current, subject, args);
    }
    let v = match name {
        // Tracks airborne/falling state (`IsInAir`): 0 when grounded indoors or outdoors,
        // 1 while airborne/falling, 0 after landing [observed: Fallout: New Vegas 1.4.0.525
        // + xNVSE 6.4.9 + ShowOffNVSE 184, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`)
        // and `Goodsprings` (`0x000DAEBB`), 10 official base masters,
        // `w4c_a01_isjumping_indoor`, `w4c_a13_isjumping_deep`, `w4c_a01b_isjumping_bare`] (§4).
        "isjumping_broken" => return crate::nvse::call(g, ctx, "isinair", subject, &[]),
        // 1 when every character is below 128 [observed: rest1.ascii.plain 1,
        // ascii.high 0] (§5).
        "stringcontainsonlyascii" => match args.first() {
            Some(XVal::Str(s)) => XVal::Num(f64::from(u8::from(s.is_ascii()))),
            _ => XVal::Num(0.0),
        },
        // A command's opcode by name or alias, any case; -1 for none [observed:
        // rest1.funccode.*: GetAV and GetActorValue 4110, AttachLight 10361] (§5).
        "getfunccodebyname" => {
            let n = match args.first() {
                Some(XVal::Str(s)) => s.clone(),
                _ => String::new(),
            };
            XVal::Num(crate::nvse_debug::opcode_of(&n).map_or(-1.0, f64::from))
        }
        // The command's name; no such opcode fails the call [observed: rest1.funcname.*:
        // 4110 GetActorValue, 10361 AttachLight, 1 and 65000 "script call failed"] (§5).
        "getfuncnamebycode" => {
            let code = match args.first() {
                Some(XVal::Num(x)) if *x >= 0.0 => *x as u32,
                _ => return Some(Err("GetFuncNameByCode: no such command".into())),
            };
            match name_of_code(g, code) {
                Some(n) => XVal::Str(n),
                None => return Some(Err(format!("GetFuncNameByCode: no command {code}"))),
            }
        }
        // ---- kNVSE (§7) ----
        // The actor's current animation file for sequence type 0, else "" [observed:
        // knv1.pathbytype.0 and knv2.pathbytype.0 Doc's mtidle.kf, knv1.pathbytype.player.0
        // the player's first-person idle; types 1-3 ""; hypothesis: type 0 is the body's clip].
        "getanimpathbysequencetype" => {
            let Some(a) = subject else { return Some(Err("GetAnimPathBySequenceType: no actor".into())) };
            let t = args.first().and_then(|v| if let XVal::Num(n) = v { Some(*n) } else { None }).unwrap_or(0.0);
            let clip = if t == 0.0 { g.playing_sequences(a).into_iter().next() } else { None };
            XVal::Str(clip.unwrap_or_default())
        }
        // The answers kNVSE gave for vanilla animations, even the one playing [observed:
        // knv2, knv3: weight 0, hand type -1, additive 0, queued 0, priority -1, text key
        // time -1; hypothesis: they read kNVSE's own overrides, which NVForever has none of
        // for vanilla clips]. When a script explicitly set a weight or additive flag for the
        // actor and sequence path, return that session value [design].
        "getanimweight" => {
            let a = subject.unwrap_or(FormId::PLAYER_REF);
            let p = match args.first() {
                Some(XVal::Str(s)) => s.replace('/', "\\").to_ascii_lowercase(),
                _ => String::new(),
            };
            let first = a == FormId::PLAYER_REF && g.senses.first_person;
            let w = g
                .anim_paths
                .sequences
                .get(&(a, first, p.clone()))
                .or_else(|| (a == FormId::PLAYER_REF).then(|| g.anim_paths.sequences.get(&(a, !first, p.clone()))).flatten())
                .map(|s| f64::from(s.weight))
                .or_else(|| g.knvse.weights.get(&(a, p)).copied())
                .unwrap_or(0.0);
            XVal::Num(w)
        }
        "isanimadditive" => {
            let a = subject.unwrap_or(FormId::PLAYER_REF);
            let p = match args.first() {
                Some(XVal::Str(s)) => s.clone(),
                _ => String::new(),
            };
            let first = a == FormId::PLAYER_REF && g.senses.first_person;
            let on = g.sequence_additive(a, first, &p).is_some()
                || (a == FormId::PLAYER_REF && g.sequence_additive(a, !first, &p).is_some())
                || g.knvse.additive.contains_key(&(a, p.replace('/', "\\").to_ascii_lowercase()));
            XVal::Num(f64::from(u8::from(on)))
        }
        "isanimqueued" => XVal::Num(0.0),
        "getanimhandtype" | "getaniminterppriority" | "getanimtextkeytime" => XVal::Num(-1.0),
        // Setters that answered 0 and changed nothing a getter showed [observed: knv2.setprio
        // then prio.after -1, copyprio, removetextkey, queue then queued.after 0].
        "setaniminterppriority" | "copyanimpriorities" | "removeanimtextkey" | "queuenextanim" | "setanimadditiveinterpsweightmult"
        | "registercustomanimgroup" | "ejectweapon" => XVal::Num(0.0),
        // 1 for an existing animation file of the game and 0 for a missing file
        // [observed: knv2.cycletype, destframe; `b3_knv_02_anim_cycle_dest_frame`
        // (`wave4b_extenders.json`) and `knv_02_set_anim_cycle_dest_missing_file`
        // (`wave4a_extenders.json`), see docs/spec-notes/ext-unlisted-rest.md §7].
        "setanimcycletype" | "setanimdestframe" => {
            let p = match args.first() {
                Some(XVal::Str(s)) => s.clone(),
                _ => String::new(),
            };
            XVal::Num(f64::from(u8::from(crate::nvse_anim::file_exists(&p))))
        }
        // [observed: knv2r2 erase.player 1]
        "erasecurrentanimaction" => XVal::Num(1.0),
        // "" outside an animation's own script [observed: knv1.thisanim.*, thisanimdir].
        "thisanim" | "thisanimdir" => XVal::Str(String::new()),
        // No form [decision: the original crashed with a real path and with none,
        // knv2r0, knv2r1].
        "createidleanimform" => XVal::Ref(0),
        // `group flag`: plays as PlayGroup and answers 1 [observed: knv3.playgroupalt.*].
        "playgroupalt" => {
            let Some(on) = subject else { return Some(Ok(XVal::Num(0.0))) };
            let code = match args.first() {
                Some(XVal::Num(n)) => Some(*n as u16),
                Some(XVal::Str(s)) => g.data.table.enums.get("anim_group").and_then(|m| m.iter().find(|(_, n)| n.eq_ignore_ascii_case(s)).map(|(c, _)| *c)),
                _ => None,
            };
            let Some(group) = code else { return Some(Ok(XVal::Num(0.0))) };
            let flag = args.get(1).and_then(|v| if let XVal::Num(n) = v { Some(*n as i32) } else { None }).unwrap_or(0);
            let name = g.anim_group_name(group);
            g.ask_anim(crate::anims::AnimRequest { on, group, name, start: crate::anims::Start::from_flag(flag) });
            XVal::Num(1.0)
        }
        // `pattern [n]`: the paths a wildcard pattern matches, folders and the `.` and
        // `..` entries too, in reverse order of name; the second argument changed nothing;
        // a pattern with no wildcard gives the folder itself [observed: knv3.dir.*,
        // knv2.dir.*]. NVForever lists Data through its read roots [decision].
        "getdirectoryfiles" => {
            let pat = match args.first() {
                Some(XVal::Str(s)) => s.replace('/', "\\"),
                _ => String::new(),
            };
            let found = directory_files(&pat);
            let entries = found.into_iter().enumerate().map(|(i, p)| (fnv_script::vm::XKey::Num(i as f64), XVal::Str(p))).collect();
            XVal::Array(g.ext.new_array(fnv_script::vm::ArrayKind::List, entries))
        }
        // ---- batch 4: ShowOff, SUP, One111, ITR, JohnnyGuitar (§8) ----
        // The reference's position in the world [observed: rest4.worldoffset.vals, a placed
        // Doc at 37198.9 -117368.7 6056.0 outside the airport; hypothesis: the offset of a
        // worldspace is none in the probed one].
        "getworldoffsetposarray" => {
            let p = subject.and_then(|r| g.ref_pos_rot(r)).map_or([0.0; 3], |x| x.0);
            let entries = p.iter().enumerate().map(|(i, v)| (fnv_script::vm::XKey::Num(i as f64), XVal::Num(f64::from(*v)))).collect();
            XVal::Array(g.ext.new_array(fnv_script::vm::ArrayKind::List, entries))
        }
        // `info`: the dialogue line's quest; none for a topic [observed: rest4.topicquest
        // 0008D243, topicquest.dial 00000000].
        "topicgettopicinfoquest" => XVal::Ref(form_arg(args, 0).and_then(|f| g.data.infos.get(&f)).and_then(|i| i.quest).map_or(0, |q| q.0)),
        // `casino index`: the deck's back texture, the casino's ICO2 entries in order
        // [observed: rest4.casinodeck.0 "terminals\NV_PlayingCards\DeckB\cardB_Back.dds"
        // for the Atomic Wrangler, whose ICO2 entries all name it].
        "getcasinodecktexture" => {
            let at = args.get(1).and_then(|v| if let XVal::Num(n) = v { Some(*n as usize) } else { None }).unwrap_or(0);
            let tex = form_arg(args, 0).and_then(|c| {
                g.data.record_fields.get(&c).and_then(|subs| subs.iter().filter(|(s, _)| s == b"ICO2").nth(at).and_then(|(_, v)| v.as_str().map(str::to_string)))
            });
            XVal::Str(tex.unwrap_or_default())
        }
        // Lists with nothing in them here [observed: rest4.eligibleperks, pipboyradio,
        // comparenif: empty arrays; nifids.id no array].
        "geteligibleperksex" | "getpipboyradiovoiceentrydata" | "comparenifblocks" | "getnifchildrenidsbyname" => {
            XVal::Array(g.ext.new_array(fnv_script::vm::ArrayKind::List, Vec::new()))
        }
        "getnifchildrenidsbyid" => XVal::Array(0),
        // A string map whose keys the probe did not read [observed: rest4.cover.info.doc,
        // five keys; NVForever gives it empty].
        "getactorcoverinfo" => XVal::Array(g.ext.new_array(fnv_script::vm::ArrayKind::StringMap, Vec::new())),
        // No form outside a SayTo [observed: rest4.sayto.*].
        "sayto_getspeakingactor" | "sayto_getquest" => XVal::Ref(0),
        "getnifstringbyid" | "getcameramovement" => XVal::Str(String::new()),
        // The answers the original gave (§8), with what each is not known to change.
        "getactorcoverstate" | "getweapadditionalfloat" | "hudbarsetforcehide" => XVal::Num(-1.0),
        "getextraaccuracypenaltymult" | "setextraaccuracypenaltymult" | "removeextraaccuracypenaltymult" | "getcamerashotpath"
        | "setcamerashotpath" | "setmusictypepath" | "setmedialocationcontrolleroverride" | "clearmedialocationcontrolleroverride" => XVal::Num(1.0),
        // Not probed: 1 as JohnnyGuitar's other setters answered, and GetCasinoDeckTexture
        // keeps reading the casino's own deck [hypothesis, §8].
        "setcasinodecktexture" => XVal::Num(1.0),
        // `to from`: NPC base `to` takes `from`'s look for the session, every reference
        // of it [documented: CopyFaceGenFrom, the reference form; the name's "Base" and
        // ShowOff's `to from` order, hypothesis]: race, sex, face, skin tone, hair, eyes
        // and head parts, drawn by the engine (`nvforever` actors.rs), and GetIsRace and
        // GetIsSex read them. 1 as the original answered [observed: §8]; the same NPC
        // twice, or not two NPCs, changes nothing.
        "basecopyfacegenfrom" => {
            let (to, from) = (form_arg(args, 0), form_arg(args, 1));
            if let (Some(to), Some(from)) = (to, from) {
                let npc = |f: FormId| g.data.sig(f).is_some_and(|s| &s == b"NPC_");
                if to != from && npc(to) && npc(from) {
                    g.face_from_base.insert(to, from);
                }
            }
            XVal::Num(1.0)
        }
        "getcurrentdialogue" | "setdismemberedlimbvelocity" | "spawntempparticle" | "getswitchnodeindex" | "setweapadditionalfloat"
        | "setweapadditionalflag" | "setondamagecalcflag" | "addperkentry" | "istemporaryreference" | "getlevelupmenuunspentpoints"
        | "setlevelupmenupoints" | "getnoequipshowoff" | "setnoequipshowoff" | "getliveeffectbasetrait" | "setliveeffectmagnitudemodifier"
        | "showingoffdisable" | "showingoffenable" | "messageexaltshowoff" | "setfactioncombatreactiontemp" | "getniftypebyid"
        | "supgetconditionvarfloat" | "hudbargetpos" | "getcalculatedposframe" | "getnifblocktranslationtovars" | "topicreplacetopicinfo"
        | "getpackedplayerfov" | "jglegacyworldtoscreen" | "setblocktransform" | "setdialogresponseoverridevalues" => XVal::Num(0.0),
        // ---- batch 5: JIP (§9) ----
        // `race which`: 0 the race's older race (ONAM), 1 its younger (YNAM); a script's
        // SetRaceAgeRace first [observed: rest5.raceagerace.*, Caucasian 0 CaucasianOld,
        // 1 CaucasianChild; CaucasianOld 0 none].
        "getraceagerace" | "getracevoice" => {
            let (Some(race), Some(which)) = (form_arg(args, 0), num_arg(args, 1)) else { return Some(Ok(XVal::Ref(0))) };
            let key = (race, name == "getracevoice", which != 0.0);
            let got = g.other.unlisted2.race_sets.get(&key).copied().or_else(|| race_field(g, race, key.1, key.2));
            XVal::Ref(got.map_or(0, |f| f.0))
        }
        // `race which form`: answers 0 [observed: rest5.racevoice.set, raceagerace.set];
        // the getters read it back for the session [hypothesis].
        "setraceagerace" | "setracevoice" => {
            if let (Some(race), Some(which), Some(f)) = (form_arg(args, 0), num_arg(args, 1), form_arg(args, 2)) {
                g.other.unlisted2.race_sets.insert((race, name == "setracevoice", which != 0.0), f);
            }
            XVal::Num(0.0)
        }
        // The actor's equipped armour [observed: rest5.equippedarmor.*, 3 for the player,
        // 1 for Doc; hypothesis: the original's inventory references are the base forms here].
        "getequippedarmorrefs" => {
            let worn = subject.and_then(|a| g.world.inventory_mut(a).ok().map(|inv| inv.equipped())).unwrap_or_default();
            let armour: Vec<FormId> = worn.into_iter().filter(|&f| g.data.sig(f) == Some(*b"ARMO")).collect();
            let entries = armour.into_iter().enumerate().map(|(i, f)| (fnv_script::vm::XKey::Num(i as f64), XVal::Ref(f.0))).collect();
            XVal::Array(g.ext.new_array(fnv_script::vm::ArrayKind::List, entries))
        }
        // `array index`: the element; 0 for no array [observed: rest5.arrayvalue 20 of
        // [10 20 30] at 1, arrayvalue.ref 0].
        "getarrayvalue" => {
            let (Some(XVal::Array(id)), Some(i)) = (args.first(), num_arg(args, 1)) else { return Some(Ok(XVal::Num(0.0))) };
            let v = g.ext.array(*id).and_then(|a| a.entries.iter().find(|(k, _)| matches!(k, fnv_script::vm::XKey::Num(x) if *x == i)).map(|(_, v)| v.clone()));
            v.unwrap_or(XVal::Num(0.0))
        }
        // `sound [container]`: the container's base takes the sound; answers 0 [observed:
        // rest5.opensound.set "0 00076E94", closesound.set; cmd2.opensound.set from a
        // reference], through SetContainerSound (`crate::nvse_jg1`).
        "setcontaineropensound" | "setcontainerclosesound" => {
            let container = form_arg(args, 1).or(subject).map_or(0, |f| f.0);
            let which = if name == "setcontaineropensound" { 0.0 } else { 1.0 };
            let sound = form_arg(args, 0).map_or(0, |f| f.0);
            let _ = crate::nvse::call(g, ctx, "setcontainersound", None, &[XVal::Ref(container), XVal::Num(which), XVal::Ref(sound)]);
            XVal::Num(0.0)
        }
        // Active action/interaction state on the calling reference (0-arg only): 0 for
        // Player, freshly spawned NPCs before their first AI tick, XMarkerHeading
        // `0x00104C0D`, and unactivated unscripted non-actor refs; 1 for activated refs
        // (`ctx.action_ref` / `action_refs`), Doc Mitchell `0x00104C0F` (sitting in
        // furniture `0x001059B0`), spawned NPCs after 1 AI tick, and front door
        // `0x00103E61` [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN
        // 57.30, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters,
        // isolated clean-room staging copy (read-only `PrintToConsole` probe; zero writes
        // to game install or Saves), `w4c_a03_has_action_ref`,
        // `w4c_a03c_has_action_ref_doc_real`, `w4c_a03d_has_action_ref_doors_actors`,
        // `w4c_a03b_has_action_ref_1arg` (`wave4c_extenders.json`, #3679), and
        // `w4d_02_has_action_ref_triggers` (`tools/capture/nvse/catalogs/wave4d_extenders.json`,
        // #3802 / PR #3803): `PROBE|w4d_02_has_action_ref_triggers|front(1,1)|doc(1,1)|docMk104C0D(0,0)|unscr(001062AC:0,1,1)|spawn(0,1)|cont(001062C6:0,1,0)`;
        // zero DLL, PDB, or third-party source consulted].
        // Generalizing those observations to (a) any non-player actor with an active AI
        // `current_package` ("active AI package") and (b) any `DOOR` base with an attached
        // script ("scripted door"), as well as exact clearing rules on container menu close
        // (`cont(001062C6:0,1,0)`), cell unload, or unprobed reference types, remain
        // [hypothesis].
        "hasactionref" => {
            let has = subject.is_some_and(|r| {
                (ctx.action_ref.is_some() && ctx.self_ref == Some(r))
                    || g.action_refs.contains_key(&r)
                    || g.furniture_use(r).is_some()
                    || (r != FormId::PLAYER_REF
                        && g.extra.get(&r).is_some_and(|e| e.current_package.is_some()))
                    || crate::functions::base_of(g, r)
                        .is_some_and(|b| g.data.sig(b) == Some(*b"DOOR") && g.data.script_of_base(b).is_some())
            });
            XVal::Num(f64::from(u8::from(has)))
        }
        // [observed: rest5.questtargetschanged 1, toggle.hiteffects 1]
        "getquesttargetschanged" | "togglehiteffects" => XVal::Num(1.0),
        "getobjectiveteleportlinks" => XVal::Array(0),
        "getcollisionnodes" => XVal::Array(g.ext.new_array(fnv_script::vm::ArrayKind::List, Vec::new())),
        "getbodypartvatstarget" => XVal::Str(String::new()),
        // The answers the original gave, effects not seen (§9).
        "getscriptblockdisabled" | "setscriptblockdisabled" | "calculateshotspersec" | "isattackqueued" | "getrecipemenunumsubcategories"
        | "setwatertrait" | "toggledepthclear" | "togglenomovementcombat" | "togglenozposreset" | "togglenodecollision"
        | "suppressquestmessages" | "unlockrecipemenuquantity" | "setitemfilter" | "settexturetransformkey" | "clearmodelalttextures"
        | "initrockitlauncher" | "enableimprovedrecipemenu" | "setwindspeedmult" | "swapobjectlod" | "scrollmousewheel"
        | "inititemfilter" | "initminimap" | "updateminimap" | "reloadcharcontroller" | "reloadcloudtextures"
        | "resetclouds" | "executescript" | "showlevelupmenuex" | "ccctaskpackageflags" | "gettranslatedpos" | "getworldmapposmults"
        | "transformworldtolocal" => XVal::Num(0.0),
        // 1 in a fresh New Game session and 0 in a loaded save [observed: rest1.isnewgame 0
        // in a loaded save; `sup_12_is_new_game` (`wave4a_extenders.json`), see
        // docs/spec-notes/ext-unlisted-rest.md §5]; clearing from 1 to 0 when a save is
        // loaded mid-session ("until a save is loaded") remains [hypothesis] (§5).
        "isnewgame" => XVal::Num(f64::from(u8::from(g.other.unlisted2.new_game))),
        _ => return None,
    };
    Some(Ok(v))
}
