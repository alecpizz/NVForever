//! The static data scripts run against: compiled scripts (decoded once), quests from
//! `QUST`, dialogue and package result scripts, messages, form lists, actor bases, and
//! Editor IDs. Built once per load order from the winning record of every form; field
//! meanings come from the Layer 1 schema (`schemas/fnv`), never from byte offsets here.

use crate::cond::{self, Condition, ParamTypes};
use crate::quest::{Objective, QuestDef, StageEntry, Target};
use fnv_esm::decode::{decode_sub, DecodeCtx};
use fnv_esm::schema::TypeDef;
use fnv_esm::{LoadOrder, Record, Schema};
use fnv_script::unit::{read_record, ReadIssues, ScriptUnit, UnitOwner, VarType};
use fnv_script::{decode, FunctionTable, Program};
use fnv_sim::extract::Issues;
use fnv_sim::{FormId, RefTable};
use rayon::prelude::*;
use serde_json::Value as Json;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, OnceLock};

/// A unit with its decoded program.
#[derive(Debug, Clone)]
pub struct Compiled {
    pub unit: ScriptUnit,
    pub program: Program,
    /// Undecodable pieces (none in the shipped masters).
    pub raw: usize,
    /// [`ScriptUnit::key`], built once in [`Compiled::new`] (#1389).
    key: Arc<str>,
}

impl Compiled {
    /// A unit with its decoded program. Its key is built here, once: the key reads only
    /// the unit's plugin and owner, which nothing changes afterwards [design:
    /// `crate::source_compile::apply` replaces the bytes, references, variables and
    /// program only].
    ///
    /// Known limit: `unit` is a public field, so code that changed its `plugin` or
    /// `owner` after this would leave the cached key stale; no code does [design: checked
    /// for #1389]. Such a change would have to build a new `Compiled`.
    pub fn new(unit: ScriptUnit, program: Program, raw: usize) -> Self {
        let key: Arc<str> = Arc::from(unit.key());
        Compiled { unit, program, raw, key }
    }

    /// `<plugin>|<owner label>` ([`ScriptUnit::key`]).
    pub fn key(&self) -> String {
        self.key.to_string()
    }

    /// The same key, shared: a clone copies no text. Every function call the unit's
    /// script makes carries it in its `functions::Ctx` (#1389).
    pub fn shared_key(&self) -> &Arc<str> {
        &self.key
    }

    pub fn has_block(&self, event: u16) -> bool {
        self.program.blocks.iter().any(|b| b.event == event)
    }

    /// Whether a GameMode block of the script tests that a reference is not in the
    /// player's cell: an If condition with `GetInSameCell … 0 ==` or `GetInSameCell … 1 !=`
    /// (`Game::run_far_frame`'s rule). Such a branch can only do anything while the
    /// actor is somewhere the player is not, so the script was written to run then.
    pub fn tests_away_from_player(&self) -> bool {
        use fnv_script::ir::{Op, Stmt, Token};
        const GET_IN_SAME_CELL: u16 = 0x1020;
        fn away(tokens: &[Token]) -> bool {
            tokens.windows(3).any(|w| match w {
                [Token::Call { call }, Token::Num { value, .. }, Token::Op { op }] if call.opcode == GET_IN_SAME_CELL => (*value == 0.0 && *op == Op::Eq) || (*value == 1.0 && *op == Op::Ne),
                _ => false,
            })
        }
        fn rec(body: &[Stmt]) -> bool {
            body.iter().any(|st| match st {
                Stmt::If { arms, otherwise } => arms.iter().any(|a| away(&a.cond.0) || rec(&a.body)) || otherwise.as_deref().is_some_and(rec),
                Stmt::While { body, .. } | Stmt::ForEach { body, .. } => rec(body),
                _ => false,
            })
        }
        self.program.blocks.iter().filter(|b| b.event == crate::game::event::GAME_MODE).any(|b| rec(&b.body))
    }

    /// Declared type of a local variable.
    pub fn var_type(&self, index: u16) -> Option<&VarType> {
        self.unit.var(index as u32).map(|v| &v.ty)
    }

    /// Index of a local variable by name (case-insensitive).
    pub fn var_index(&self, name: &str) -> Option<u16> {
        self.unit.variables.iter().find(|v| v.name.eq_ignore_ascii_case(name)).map(|v| v.index as u16)
    }

    pub fn var_name(&self, index: u16) -> Option<&str> {
        self.unit.var(index as u32).map(|v| v.name.as_str())
    }
}

/// Index into [`GameData::units`].
pub type UnitId = usize;

#[derive(Debug, Clone, Default)]
pub struct InfoDef {
    pub quest: Option<FormId>,
    /// ANAM: the speaker the line is restricted to.
    pub speaker: Option<FormId>,
    pub conditions: Vec<Condition>,
    /// Result script before `NEXT` (runs at the start of the line) and after it (at the
    /// end) [documented: xEdit "Script (Begin)"/"Script (End)"].
    pub begin: Option<UnitId>,
    pub end: Option<UnitId>,
    /// The topic (DIAL) the line belongs to: the label of its enclosing "topic children"
    /// group [documented: xEdit group types; observed: every INFO of the masters sits in
    /// such a group].
    pub topic: Option<FormId>,
    /// DATA flags: Say Once (an actor says the line only once [documented: GECK wiki
    /// Category:Dialogue, owner export 2026-10-09]) and Goodbye.
    pub say_once: bool,
    pub goodbye: bool,
    /// Characters of each response text (NAM1), in record order: the stand-in for the
    /// length of the voice file (docs/spec-notes/scripts.md §7.7). Same order as
    /// [`InfoDef::responses`].
    pub response_chars: Vec<usize>,
    /// The record's header flags (0x2000 is set on about a quarter of the masters'
    /// lines; its meaning on INFO is not documented, docs/spec-notes/dialogue.md §2).
    pub record_flags: u32,
    /// DATA type code (0 Topic … 7 Radio, [`topic_type`]) [documented: xEdit INFO DATA].
    pub info_type: u8,
    /// DATA next speaker (0 Target, 1 Self, 2 Either) [documented: xEdit wbNextSpeaker]:
    /// who says the next line of a conversation between actors
    /// (`Game::next_conversation_line`).
    pub next_speaker: u8,
    /// DATA flags: the first flag byte in bits 0–7, the second in bits 8–15
    /// ([`info_flag`]) [documented: xEdit INFO DATA; the same combined value the GECK
    /// wiki's GetDialogueInfoFlags page lists, owner export].
    pub flags: u16,
    /// RNAM: the text shown to the player instead of the topic's name
    /// [documented: Category:Dialogue, owner export].
    pub prompt: Option<String>,
    /// The responses (TRDT with its NAM1, NAM2, NAM3, SNAM, LNAM), in record order.
    pub responses: Vec<Response>,
    /// TCLT, in record order: the topics offered after this line ("Choices" in the
    /// editor; xEdit calls the list "Link To") [documented: xEdit INFO; Category:Dialogue].
    pub choices: Vec<FormId>,
    /// TCLF, in record order ("Link From") [documented: xEdit INFO]: the topics after
    /// whose lines this line's topic is offered to the player ([`GameData::linked_from`],
    /// dialogue.md §3.5); a HELLO line with one cannot start a conversation.
    pub link_from: Vec<FormId>,
    /// TCFU, in record order: follow-up lines (INFO) [documented: xEdit INFO "Follow Up"].
    pub follow_ups: Vec<FormId>,
    /// NAME, in record order: topics the line adds [documented: xEdit INFO "Add Topics"].
    pub add_topics: Vec<FormId>,
    /// KNAM: the actor value (AVIF) or perk shown with the choice [documented: xEdit
    /// INFO "ActorValue/Perk"; Category:Dialogue "AV/Perk"].
    pub challenge_form: Option<FormId>,
    /// DNAM: speech challenge difficulty code (0 none, 1 Very Easy … 5 Very Hard)
    /// [documented: xEdit INFO "Speech Challenge"].
    pub challenge_difficulty: Option<u32>,
    /// PNAM "Previous INFO": whether the record has one, and the line it names (none:
    /// the field is 0) [documented: xEdit INFO; Category:Dialogue "Edit Response Order"].
    pub has_previous: bool,
    pub previous: Option<FormId>,
    /// TPIC "Previous Topic" [documented: xEdit INFO]; absent in the ten masters.
    pub previous_topic: Option<FormId>,
}

/// One response of a line (TRDT and the subrecords after it) [documented: xEdit INFO
/// "Responses"; schema `gameplay.json`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Response {
    /// TRDT response number (the order responses are said in).
    pub number: u8,
    /// TRDT emotion type code (0 Neutral, 1 Anger, 2 Disgust, 3 Fear, 4 Sad, 5 Happy,
    /// 6 Surprise, 7 Pained) and value.
    pub emotion: u8,
    pub emotion_value: i32,
    /// TRDT sound (SOUN), played instead of the voice [documented: Category:Dialogue].
    pub sound: Option<FormId>,
    pub use_emotion_animation: bool,
    /// NAM1: the response text.
    pub text: String,
    /// NAM2 "Script Notes" (acting notes).
    pub notes: String,
    /// SNAM / LNAM: speaker and listener idle animations (IDLE).
    pub speaker_idle: Option<FormId>,
    pub listener_idle: Option<FormId>,
}

/// INFO DATA flag bits in [`InfoDef::flags`] [documented: xEdit INFO DATA flag names;
/// meanings: GECK wiki Category:Dialogue, owner export].
pub mod info_flag {
    pub const GOODBYE: u16 = 0x0001;
    pub const RANDOM: u16 = 0x0002;
    pub const SAY_ONCE: u16 = 0x0004;
    pub const RUN_IMMEDIATELY: u16 = 0x0008;
    pub const INFO_REFUSAL: u16 = 0x0010;
    pub const RANDOM_END: u16 = 0x0020;
    pub const RUN_FOR_RUMORS: u16 = 0x0040;
    pub const SPEECH_CHALLENGE: u16 = 0x0080;
    pub const SAY_ONCE_A_DAY: u16 = 0x0100;
    pub const ALWAYS_DARKEN: u16 = 0x0200;
    pub const LOW_INTELLIGENCE: u16 = 0x1000;
    pub const HIGH_INTELLIGENCE: u16 = 0x2000;
    /// Names for listings.
    pub const NAMES: &[(u16, &str)] = &[
        (GOODBYE, "Goodbye"),
        (RANDOM, "Random"),
        (SAY_ONCE, "Say Once"),
        (RUN_IMMEDIATELY, "Run Immediately"),
        (INFO_REFUSAL, "Info Refusal"),
        (RANDOM_END, "Random End"),
        (RUN_FOR_RUMORS, "Run for Rumors"),
        (SPEECH_CHALLENGE, "Speech Challenge"),
        (SAY_ONCE_A_DAY, "Say Once a Day"),
        (ALWAYS_DARKEN, "Always Darken"),
        (0x0400, "Unknown 2"),
        (0x0800, "Unknown 3"),
        (LOW_INTELLIGENCE, "Low Intelligence"),
        (HIGH_INTELLIGENCE, "High Intelligence"),
    ];
}

/// DIAL / INFO DATA type codes [documented: xEdit DIAL DATA].
pub mod topic_type {
    pub const TOPIC: u8 = 0;
    pub const CONVERSATION: u8 = 1;
    pub const COMBAT: u8 = 2;
    pub const PERSUASION: u8 = 3;
    pub const DETECTION: u8 = 4;
    pub const SERVICE: u8 = 5;
    pub const MISCELLANEOUS: u8 = 6;
    pub const RADIO: u8 = 7;
    pub const NAMES: [&str; 8] = ["Topic", "Conversation", "Combat", "Persuasion", "Detection", "Service", "Miscellaneous", "Radio"];
}

impl InfoDef {
    pub fn has(&self, flag: u16) -> bool {
        self.flags & flag != 0
    }

    /// Indices into [`InfoDef::responses`] in the order they are said: by response
    /// number, record order among equal numbers [documented: xEdit "Response number";
    /// hypothesis that the number, not the record order, decides where the two differ,
    /// dialogue.md §2].
    pub fn response_order(&self) -> Vec<usize> {
        let mut ix: Vec<usize> = (0..self.responses.len()).collect();
        ix.sort_by_key(|&i| self.responses[i].number);
        ix
    }
}

/// A dialogue topic (DIAL) [documented: xEdit DIAL; schema `gameplay.json`].
#[derive(Debug, Clone)]
pub struct TopicDef {
    pub form: FormId,
    pub edid: Option<String>,
    /// FULL: the text shown to the player when the line has no prompt
    /// [documented: Category:Dialogue "Prompt"].
    pub name: Option<String>,
    /// DATA type code ([`topic_type`]).
    pub topic_type: u8,
    /// DATA flags: 1 Rumors, 2 Top-level.
    pub flags: u8,
    /// PNAM: priority, 50 when absent [documented: xEdit default].
    pub priority: f32,
    /// QSTI of the winning record, in record order ("Added Quests").
    pub quests: Vec<FormId>,
    /// QSTR ("Removed Quests"); none in the masters.
    pub removed_quests: Vec<FormId>,
    /// INFC / INFX pairs ("Shared Infos") with the QSTI they follow: lines stored under
    /// another topic that count as lines of this one ([`GameData::lines_tried`],
    /// dialogue.md §1.2).
    pub shared_infos: Vec<SharedInfo>,
    /// TDUM "Dumb Response" [documented: xEdit DIAL]; not used.
    pub dumb_response: Option<String>,
    /// Where the topic was first defined: plugin index and record ordinal in that file
    /// (the "record order" tie-break of the offered list).
    pub order: (usize, usize),
}

/// One INFC / INFX pair of a DIAL.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SharedInfo {
    pub quest: Option<FormId>,
    pub info: Option<FormId>,
    pub index: Option<i32>,
}

impl Default for TopicDef {
    fn default() -> Self {
        TopicDef {
            form: FormId(0),
            edid: None,
            name: None,
            topic_type: 0,
            flags: 0,
            priority: 50.0,
            quests: Vec::new(),
            removed_quests: Vec::new(),
            shared_infos: Vec::new(),
            dumb_response: None,
            order: (usize::MAX, 0),
        }
    }
}

impl TopicDef {
    pub const RUMORS: u8 = 1;
    pub const TOP_LEVEL: u8 = 2;

    pub fn top_level(&self) -> bool {
        self.flags & Self::TOP_LEVEL != 0
    }
}

#[derive(Debug, Clone, Default)]
pub struct MessageDef {
    /// FULL: the title of a message box.
    pub title: Option<String>,
    pub text: Option<String>,
    pub buttons: Vec<String>,
    /// CTDA after each ITXT: a button is shown only while its conditions hold
    /// [documented: GECK wiki GetButtonPressed, "certain buttons are hidden with
    /// conditions"; the indices stay those of the record]. One list per button.
    pub button_conditions: Vec<Vec<Condition>>,
    pub message_box: bool,
    /// INAM: the Menu Icon (MICN) drawn with the message when it is not a box; none
    /// means the default icon [documented: GECK wiki, Message; corner-messages.md §3].
    pub icon: Option<FormId>,
    /// TNAM: the seconds a message that is not a box stays on screen [documented: GECK
    /// wiki, Message "Display Time"]; boxes have none [observed].
    pub display_seconds: Option<u32>,
}

/// A note (NOTE): its type and, for a text note, its text [documented: xEdit
/// wbDefinitionsFNV.pas NOTE].
#[derive(Debug, Clone, Default)]
pub struct NoteDef {
    pub name: Option<String>,
    /// DATA: 0 Sound, 1 Text, 2 Image, 3 Voice.
    pub kind: u8,
    /// TNAM of a text note.
    pub text: Option<String>,
    /// XNAM of an image note: a texture path.
    pub texture: Option<String>,
    /// SNAM: a Sound note's sound (SOUN), a Voice note's speaker (NPC_ or CREA)
    /// [documented: xEdit wbDefinitionsFNV.pas NOTE, a union by note type; GECK wiki
    /// GetNoteSound, GetNoteSpeaker]. One value, read by the type ([`NoteDef::sound`],
    /// [`NoteDef::speaker`]), so SetNoteType changes what it is. docs/spec-notes/holotapes.md §1.
    pub snam: Option<FormId>,
    /// TNAM of a Voice note: the topic (DIAL) whose line it plays [documented: xEdit
    /// NOTE TNAM union; GECK wiki GetNoteTopic].
    pub topic: Option<FormId>,
    /// ONAM: the note's quests, in record order [documented: xEdit NOTE ONAM; GECK wiki
    /// GetNoteQuestList, "all quests associated with a Note"; hypothesis: one ONAM per
    /// quest].
    pub quests: Vec<FormId>,
}

impl NoteDef {
    /// A Sound note's sound (SNAM).
    pub fn sound(&self) -> Option<FormId> {
        self.snam.filter(|_| self.kind == 0)
    }

    /// A Voice note's speaker (SNAM).
    pub fn speaker(&self) -> Option<FormId> {
        self.snam.filter(|_| self.kind == 3)
    }
}

/// A terminal (TERM) [documented: xEdit wbDefinitionsFNV.pas TERM; schema
/// `objects.json`; docs/spec-notes/hacking.md §1].
#[derive(Debug, Clone, Default)]
pub struct TerminalDef {
    pub name: Option<String>,
    /// DESC: the welcome text under the header.
    pub welcome: String,
    /// DNAM "Base Hacking Difficulty": 0 Very Easy … 4 Very Hard, 5 Requires Key
    /// (`hacking::Difficulty::from_code`).
    pub difficulty: u8,
    /// DNAM flags ([`terminal_flag`]).
    pub flags: u8,
    /// DNAM "ServerType": 0 is "-Server 1-" … 9 "-Server 10-".
    pub server: u8,
    /// PNAM: the note that holds the password.
    pub password_note: Option<FormId>,
    pub items: Vec<TerminalItem>,
}

impl TerminalDef {
    pub fn has(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }
}

/// TERM DNAM flag bits [documented: xEdit wbDefinitionsFNV.pas TERM].
pub mod terminal_flag {
    pub const LEVELED: u8 = 0x01;
    pub const UNLOCKED: u8 = 0x02;
    pub const ALTERNATE_COLORS: u8 = 0x04;
    pub const HIDE_WELCOME_WITH_IMAGE: u8 = 0x08;
}

/// One menu item of a terminal: ITXT and what follows it up to the next ITXT.
#[derive(Debug, Clone, Default)]
pub struct TerminalItem {
    /// ITXT: the line in the menu.
    pub text: String,
    /// RNAM: the text shown when it is chosen.
    pub result: String,
    /// ANAM: 1 Add Note, 2 Force Redraw.
    pub flags: u8,
    /// INAM: the note it shows (and gives, with Add Note).
    pub note: Option<FormId>,
    /// TNAM: the terminal it opens as a sub menu.
    pub submenu: Option<FormId>,
    /// CTDA: the item is listed only while these hold.
    pub conditions: Vec<Condition>,
    /// The item's result script.
    pub unit: Option<UnitId>,
}

#[derive(Debug, Clone, Default)]
pub struct ActorDef {
    pub sig: [u8; 4],
    /// SNAM entries: faction and rank. The record's own list: with
    /// [`ActorDef::template_factions`] the actor's factions are its template's
    /// ([`GameData::template_source`]).
    pub factions: Vec<(FormId, i8)>,
    /// ACBS template flag "Factions": the template supplies the factions [documented:
    /// xEdit ACBS template flags, bit 2; GECK wiki GetActorTemplateFlag, "Use Factions"].
    pub template_factions: bool,
    pub voice: Option<FormId>,
    pub female: bool,
    pub essential: bool,
    pub race: Option<FormId>,
    pub class: Option<FormId>,
    /// PKID: the AI package list, in record order.
    pub packages: Vec<FormId>,
    /// TPLT: the template actor, and whether the template supplies the AI packages
    /// (ACBS template flag "AI Packages") [documented: xEdit; schema `gameplay.json`].
    pub template: Option<FormId>,
    pub template_packages: bool,
    /// CREA DATA creature type code (0 Animal … 7 Giant) [documented: xEdit
    /// wbCreatureTypeEnum; schema `gameplay.json`].
    pub creature_type: Option<i64>,
    /// AIDT energy level (0–100): low-energy actors spend more of a sandbox sitting and
    /// eating, high-energy ones wandering and idling [documented: GECK wiki, Sandbox
    /// Package].
    pub energy: Option<u8>,
    /// Combat (step C5) [documented: xEdit field names; GECK wiki Stats Tab pages]:
    /// NPC_ DATA Base Health (added to the health derived from Endurance and level) or
    /// CREA DATA Health (the creature's health).
    pub base_health: i64,
    /// DATA attributes, S.P.E.C.I.A.L. in that order (NPC_ and CREA) [documented: xEdit;
    /// observed: every NPC_ and CREA of FalloutNV.esm has them, actor-values.md §2].
    pub attributes: Option<[u8; 7]>,
    /// NPC_ DNAM: the fourteen skill values and skill offsets, Barter to Unarmed (actor
    /// value codes 32–45 in order) [documented: xEdit].
    pub skills: Option<[u8; 14]>,
    pub skill_offsets: Option<[u8; 14]>,
    /// CREA DATA Combat Skill and Stealth Skill [documented: xEdit; GECK wiki Stats Tab -
    /// Creatures].
    pub combat_skill: Option<u8>,
    pub stealth_skill: Option<u8>,
    /// CREA DATA Damage, the creature's own attack damage, and RNAM Attack Reach
    /// [documented: xEdit field names].
    pub attack_damage: Option<i64>,
    pub attack_reach: Option<u8>,
    /// CREA NAM5 Sound Level ("Loud", "Normal", "Silent") [documented: xEdit]: how
    /// loud the creature moves, for detection (docs/design/detection.md §8).
    pub sound_level: Option<String>,
    /// ACBS Fatigue, Speed Multiplier (percent) and Karma [documented: xEdit].
    pub fatigue: i64,
    pub speed_mult: i64,
    pub karma: f64,
    /// ACBS Disposition Base [documented: xEdit]: 35 on the Goodsprings people
    /// [observed], the disposition GetDisposition starts from.
    pub disposition_base: i64,
    /// AIDT Aggression, Confidence, Responsibility, Mood and Assistance as their codes
    /// [documented: xEdit enumerations; GECK wiki Confidence for that scale].
    pub aggression: Option<i64>,
    pub confidence: Option<i64>,
    pub responsibility: Option<i64>,
    pub mood: Option<i64>,
    pub assistance: Option<i64>,
    /// ACBS template flag "AI Data": the template supplies the AIDT values.
    pub template_ai: bool,
    /// AIDT "Aggro Radius Behavior" and "Aggro Radius" (game units) [documented: xEdit
    /// field names; the behaviour: `crate::aggro`].
    pub aggro_radius_behavior: bool,
    pub aggro_radius: u32,
    /// AIDT "Buys/Sells and Services" flags (`crate::services`) [documented: xEdit
    /// wbAIDT, wbServiceFlags; schema `gameplay.json` ServiceFlags].
    pub services: u32,
    /// ACBS "Autocalc Service" (NPC_ only): the class's services stand for the AIDT ones
    /// (`crate::services`) [documented: xEdit ACBS flag bit 23].
    pub autocalc_service: bool,
    /// ACBS template flags "Traits" (race, sex, voice, class), "Model/Animation",
    /// "Inventory" and "Script" [documented: GECK wiki GetActorTemplateFlag, flag IDs 1,
    /// 64, 256 and 512; xEdit ACBS template flags].
    pub template_traits: bool,
    pub template_model: bool,
    pub template_inventory: bool,
    pub template_script: bool,
    /// ZNAM: the combat style (CSTY) [documented: xEdit NPC_ and CREA ZNAM], from the
    /// actor that supplies its traits (template flag "Traits") [documented: GECK wiki
    /// Traits Tab - NPC and Traits Tab - Creatures list Combat Style; Template Data].
    pub combat_style: Option<FormId>,
    /// INAM "Death item": the leveled item list added to the actor's inventory when it
    /// dies, from the actor that supplies its traits (template flag "Traits")
    /// [documented: xEdit NPC_ and CREA INAM, hidden with the Traits flag;
    /// docs/spec-notes/death-items.md].
    pub death_item: Option<FormId>,
    /// NPC_ ACBS "Auto-calc stats".
    pub autocalc: bool,
    /// ACBS "No VATS Melee", "No Blood Spray", "No Blood Decal"; CREA "Allow PC Dialogue"
    /// (docs/spec-notes/actor-flags.md). The template's Base Data does not
    /// carry No VATS Melee [documented: GECK wiki Category:NPC], so it is read off the
    /// actor's own record.
    pub no_vats_melee: bool,
    /// ACBS "Not Pushable" (bit 27), "No Rotating To Head-track" (bit 30, NPC_ and CREA),
    /// and the creatures' "No Head", "No Right Arm", "No Left Arm" (bits 15–17)
    /// [documented: GECK wiki ActorHasBaseFlag flag table] (docs/spec-notes/actor-fields.md
    /// §1, actor-flags.md §3).
    pub not_pushable: bool,
    pub no_head_track: bool,
    pub no_head: bool,
    pub no_right_arm: bool,
    pub no_left_arm: bool,
    pub no_blood_spray: bool,
    pub no_blood_decal: bool,
    pub allow_pc_dialogue: bool,
    /// ACBS level, or the player-level multiplier ×1000 with "PC Level Mult", and the
    /// calculated level's bounds.
    pub level_raw: i64,
    pub pc_level_mult: bool,
    pub calc_min: i64,
    pub calc_max: i64,
    /// ACBS template flag "Stats": the template supplies the stats.
    pub template_stats: bool,
    /// CREA ACBS "Invulnerable" (bit 31) [documented: GECK wiki ActorHasBaseFlag].
    pub invulnerable: bool,
    /// CREA ACBS "Is Ghost" (bit 29): as SetGhost 1 from the start (actor-flags.md §3).
    pub is_ghost: bool,
    /// CREA ACBS "Allow Pickpocket" (bit 28): the player may pickpocket the creature;
    /// NPCs always may (docs/spec-notes/pickpocket.md).
    pub allow_pickpocket: bool,
    /// ACBS flag "No Knockdowns" (NPC_ and CREA): PushActorAway and explosions do not
    /// knock the actor down [documented: GECK wiki PushActorAway notes]. Never from the
    /// template: "Base Data does not include … No Knockdowns" [documented: GECK wiki
    /// Category:NPC, Template Data].
    pub no_knockdowns: bool,
    /// CREA ACBS flag "Immobile": PushActorAway does not work on it [documented: GECK
    /// wiki PushActorAway notes, "flagged as immobile"].
    pub immobile: bool,
    /// CREA ACBS flags "Swims", "Walks", "Flies" and "No Combat in Water" [documented:
    /// GECK wiki Animation Tab: Swims "allows for swimming and moving across water marked
    /// navmesh triangles", Walks "allows for movement", No Combat in Water "will not
    /// perform attacks in water"] (`Game::keeps_out_of_water`,
    /// `Game::no_combat_in_water`).
    pub swims: bool,
    pub walks: bool,
    pub flies: bool,
    pub no_combat_in_water: bool,
    /// CREA ACBS flag "Can't Open Doors" (bit 22; on NPC_ the bit is "Can Be All Races")
    /// [documented: GECK wiki GetActorBaseFlagsHigh, the flag table's Creature column]:
    /// the creature's paths use no door (`fnv_ai`'s path parameters).
    pub cant_open_doors: bool,
    /// ACBS flag "No Low Level Processing": the actor's AI is not updated outside the
    /// loaded area [documented: GECK wiki Category:NPC]. With the template flag "Base
    /// Data" the template's counts, as for Essential and Invulnerable (`Game::base_data_def`).
    pub no_low_level: bool,
    /// The record has a DATA whose Base Health (NPC_) or Health (CREA) is 0: its placed
    /// actors start the game as corpses ([`crate::game::Game::starts_dead`]).
    pub zero_health: bool,
    /// ACBS flag "Respawn": "References of this NPC in the world will be recreated after
    /// three days" [documented: GECK wiki Category:NPC, Configuration]; with the template
    /// flag "Base Data" the template's flag counts ([`ActorDef::template_base`]).
    pub respawn: bool,
    /// ACBS template flag "Base Data": the template supplies the base data, the Respawn
    /// flag among it [documented: GECK wiki Category:NPC, Template Data: "Base Data does
    /// not include the Quest Item, No Knockdowns, No VATS Melee, Is CharGen Face Preset,
    /// or Can Be All Races flags"].
    pub template_base: bool,
    /// `EITM` "Unarmed Attack Effect" and `EAMT` "Unarmed Attack Animation": the magic
    /// item (an ENCH or a Poison SPEL) the actor's own unarmed blows apply, and the
    /// attack animation they must be ("Any", "AttackForwardPower", …) [documented: xEdit
    /// field names, schema `gameplay.json`; GECK wiki GetActorUnarmedEffect].
    pub unarmed_effect: Option<FormId>,
    pub unarmed_anim: Option<String>,
    /// ACBS template flag "Actor Effect List": the template supplies the actor effects,
    /// read here as covering the unarmed attack effect too [hypothesis: EITM and EAMT
    /// sit beside SPLO in the record].
    pub template_spells: bool,
    /// SPLO: the actor effects (abilities, mostly) of the record, in record order; with
    /// [`ActorDef::template_spells`] the template's list counts instead.
    pub spells: Vec<FormId>,
}

/// A leveled actor list (LVLN, LVLC) [documented: xEdit; schema `gameplay.json`]: the
/// flag "Calculate from all levels <= player's level" (LVLF) and the entries (LVLO) as
/// (level, actor base or another list), in record order.
#[derive(Debug, Clone, Default)]
pub struct LeveledActorDef {
    /// LVLD Chance none, 0–100 (docs/spec-notes/actor-flags.md §6).
    pub chance_none: u8,
    pub all_levels: bool,
    pub entries: Vec<(u16, FormId)>,
    /// Each entry's count (LVLO), beside `entries`.
    pub counts: Vec<u16>,
    /// LVLF as bits: 1 all levels, 2 each item in count, 4 use all [documented: xEdit
    /// LVLF] (GetLeveledListFlags).
    pub flags: u8,
}

/// A region's areas (REGN WNAM, RPLI/RPLD): each area a polygon of worldspace x, y
/// points (schema `sky.json`).
#[derive(Debug, Clone, Default)]
pub struct RegionDef {
    pub world: Option<FormId>,
    pub areas: Vec<Vec<[f32; 2]>>,
    /// The weather block (RDAT type 3, then RDWT), when the region has one.
    pub weathers: Option<RegionWeathers>,
    /// The map block (RDAT type 4, then RDMP), when the region has one: its name and the
    /// header's priority (schema `sky.json` REGN RDMP).
    pub map: Option<(String, u8)>,
    /// The record's Border Region flag (header bit 6, 0x40) [documented: GECK wiki form
    /// flags table, "Border Region" at 64]: the player is kept inside its areas
    /// (`crate::borders`).
    pub border: bool,
}

/// REGN record flag Border Region [documented: GECK wiki form flags table, 64].
pub const BORDER_REGION: u32 = 0x40;

/// A region's weather block [documented: xEdit wbRegionDataEntries type 3; schema
/// `sky.json` REGN RDAT, RDWT]: its RDAT flags (bit 0 override,
/// `fnv_world::sky::REGION_WEATHER_OVERRIDE`) and priority, and the weathers with their
/// chances and globals.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RegionWeathers {
    pub flags: u8,
    pub priority: u8,
    pub list: Vec<RegionWeather>,
}

/// One RDWT entry: a weather, its chance and the global that gates it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegionWeather {
    pub weather: FormId,
    pub chance: i32,
    pub global: Option<FormId>,
}

impl RegionDef {
    /// Whether a worldspace point lies inside one of the areas (even-odd rule)
    /// [hypothesis: an area is the plain polygon of its points; the edge fall-off
    /// (RPLI) is ignored].
    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.areas.iter().any(|pts| {
            let mut inside = false;
            let n = pts.len();
            for i in 0..n {
                let [xi, yi] = pts[i];
                let [xj, yj] = pts[(i + n - 1) % n];
                if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
                    inside = !inside;
                }
            }
            inside
        })
    }
}

/// A base effect (MGEF DATA) [documented: xEdit MGEF; schema `gameplay.json`].
#[derive(Debug, Clone, Default)]
pub struct MagicEffectDef {
    pub flags: u32,
    pub archetype: i64,
    /// The associated script when the archetype is 1 (Script).
    pub script: Option<FormId>,
    /// DATA "Actor Value" (`None` for the record's "None").
    pub actor_value: Option<u16>,
    /// DATA "Effect Shader": the EFSH played on the target while the effect lasts
    /// (docs/spec-notes/effect-shaders.md §3).
    pub shader: Option<FormId>,
    /// DATA "Resistance Type": the actor value that resists the effect, a percentage of
    /// its magnitude ignored (`None` for "None") [documented: xEdit field; GECK wiki
    /// Stats Tab - Creatures, "Poison Resist: the percentage of poison damage that is
    /// ignored"; docs/spec-notes/hit-effects.md §3].
    pub resistance: Option<u16>,
    /// DATA "Effect Sound" and "Hit Sound" (SOUN) [documented: xEdit MGEF]: played at the
    /// target while the effect lasts and as it starts (docs/spec-notes/audio.md §15.2).
    pub effect_sound: Option<FormId>,
    pub hit_sound: Option<FormId>,
}

impl MagicEffectDef {
    /// MGEF DATA flag bits [documented: xEdit MGEF; GECK wiki GetBaseEffectFlag].
    pub const HOSTILE: u32 = 1;
    pub const RECOVER: u32 = 2;
    pub const DETRIMENTAL: u32 = 4;
    pub const NO_DURATION: u32 = 1 << 7;
    pub const NO_MAGNITUDE: u32 = 1 << 8;
    /// "FX Persist": the effect shader's persistent part shows while the effect lasts
    /// [documented: xEdit MGEF flags; GECK wiki EffectShader].
    pub const FX_PERSIST: u32 = 1 << 10;
    /// "Display Name Only": a Script effect, which shows nothing by default, shows its
    /// name [documented: GECK wiki Base Effect; schema `gameplay.json` MGEF flags bit 13].
    pub const DISPLAY_NAME_ONLY: u32 = 1 << 13;
    /// "No Death Dispel": the effect goes on after its target dies [documented: xEdit
    /// MGEF flags; hypothesis: without it the effect ends at the death,
    /// docs/spec-notes/hit-effects.md §4].
    pub const NO_DEATH_DISPEL: u32 = 1 << 28;

    pub fn has(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }

    /// Whether the interface names this effect where it shows no value: a Script effect
    /// flagged Display Name Only (Psycho's "Damage +25%", Cateye's "Low-Light Vision")
    /// [documented: GECK wiki Base Effect; #3119].
    pub fn shows_name(&self) -> bool {
        self.archetype == ARCHETYPE_SCRIPT && self.has(Self::DISPLAY_NAME_ONLY)
    }
}

/// MGEF archetype "Value Modifier" [documented: xEdit wbArchtypeEnum].
pub const ARCHETYPE_VALUE_MODIFIER: i64 = 0;
/// MGEF archetype "Script" [documented: xEdit wbArchtypeEnum].
pub const ARCHETYPE_SCRIPT: i64 = 1;
/// MGEF archetype "Dispel" [documented: xEdit wbArchtypeEnum].
pub const ARCHETYPE_DISPEL: i64 = 2;
/// MGEF archetype "Paralysis" [documented: xEdit wbArchtypeEnum; GECK wiki
/// GetBaseEffectArchetype].
pub const ARCHETYPE_PARALYSIS: i64 = 24;
/// MGEF archetype "Cure Paralysis" [documented: as Paralysis].
pub const ARCHETYPE_CURE_PARALYSIS: i64 = 30;
/// MGEF archetype "Cure Poison" [documented: as Paralysis].
pub const ARCHETYPE_CURE_POISON: i64 = 32;
/// MGEF archetype "Concussion" [documented: as Paralysis].
pub const ARCHETYPE_CONCUSSION: i64 = 33;
/// MGEF archetype "Turbo" [documented: as Paralysis].
pub const ARCHETYPE_TURBO: i64 = 36;
/// MGEF archetype "Invisibility" [documented: xEdit wbArchtypeEnum].
pub const ARCHETYPE_INVISIBILITY: i64 = 11;
/// MGEF archetype "Chameleon" [documented: xEdit wbArchtypeEnum].
pub const ARCHETYPE_CHAMELEON: i64 = 12;
/// MGEF archetype "Cure Addiction" [documented: xEdit wbArchtypeEnum].
pub const ARCHETYPE_CURE_ADDICTION: i64 = 31;
/// MGEF archetype "Value And Parts" [documented: xEdit wbArchtypeEnum].
pub const ARCHETYPE_VALUE_AND_PARTS: i64 = 34;
/// MGEF archetype "Limb Condition" [documented: xEdit wbArchtypeEnum].
pub const ARCHETYPE_LIMB_CONDITION: i64 = 35;

/// An ingestible's ENIT and ETYP (ALCH) [documented: xEdit ALCH; schema `objects.json`;
/// GECK wiki Ingestible]. docs/spec-notes/aid-items.md §1.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IngestibleDef {
    pub value: i32,
    /// ENIT flags: 2 Food Item, 4 Medicine.
    pub flags: u8,
    /// The actor effect (SPEL of type Addiction) given to an actor that becomes addicted.
    pub withdrawal: Option<FormId>,
    pub addiction_chance: f32,
    /// The sound played when it is used.
    pub consume_sound: Option<FormId>,
    /// ETYP: the equip type code (10 Chems, 11 Stimpak, 12 Food, 13 Alcohol; -1 none).
    pub equip_type: i64,
}

impl IngestibleDef {
    pub const FOOD: u8 = 2;
    pub const MEDICINE: u8 = 4;
    /// ETYP Chems [documented: xEdit wbEquipTypeEnum].
    pub const CHEMS: i64 = 10;
    /// ETYP Stimpak [documented: the same].
    pub const STIMPAK: i64 = 11;
}

/// The places in the DOBJ list the game reads [documented: xEdit wbDefinitionsFNV.pas
/// DOBJ, the list's names in order] (docs/spec-notes/default-objects.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultObject {
    Stimpak = 0,
    PerkParalysis = 5,
    DeathMusic = 11,
    LevelUpMusic = 13,
    /// The player's voice types [documented: xEdit DOBJ "Player Voice (Male)", "(Male
    /// Child)", "(Female)", "(Female Child)"; observed: FalloutNV.esm names PlayerVoiceMale
    /// 0002853B, MaleChild01 00019FCE, PlayerVoiceFemale 00040D42 and FemaleChild01
    /// 00019FD0 there] (`Game::player_voice_type`).
    PlayerVoiceMale = 14,
    PlayerVoiceMaleChild = 15,
    PlayerVoiceFemale = 16,
    PlayerVoiceFemaleChild = 17,
    DrugWearsOffImageSpace = 20,
    /// The explosion of Meltdown's kills [observed: FalloutNV.esm's DOBJ names
    /// MinePlasmaExplosion 0007078C there] (perk-entry-points.md §9).
    MeltdownExplosion = 24,
    /// The unarmed special moves' effects [documented: xEdit DOBJ, "Unarmed Forward PA" …
    /// "Unarmed Counter PA"; observed: FalloutNV.esm names none for forward and counter,
    /// VictoryRifleKnockdownSpell 0001DEA8 for back, HeadCrippleEffect 00067572 for left,
    /// right and crouched] (perk-entry-points.md §11).
    UnarmedForwardPa = 25,
    UnarmedBackwardPa = 26,
    UnarmedLeftPa = 27,
    UnarmedRightPa = 28,
    UnarmedCrouchPa = 29,
    UnarmedCounterPa = 30,
    /// Spotter's and Search and Mark's effect shaders [documented: xEdit DOBJ "Spotter
    /// Effect", "Item Detected Effect"; observed: FalloutNV.esm names fxSpotterShader
    /// 0015FF60 and fxSearchAndMark 0015FF61] (perk-entry-points.md §13).
    SpotterEffect = 31,
    ItemDetectedEffect = 32,
}

impl GameData {
    /// A default object (DOBJ), when the list names one there.
    pub fn default_object(&self, which: DefaultObject) -> Option<FormId> {
        default_in(&self.default_objects, which)
    }
}

impl ActorDef {
    /// A creature that moves (Walks or Flies) without Swims keeps off water triangles
    /// (`Game::keeps_out_of_water`); one with none of the three flags is left as it is.
    pub fn keeps_out_of_water(&self) -> bool {
        (self.walks || self.flies) && !self.swims
    }

    /// Reconstruct the parsed `ACBS` flag bits from this `ActorDef` (used when a
    /// synthetic fixture populates `GameData::actors` without raw `record_fields`).
    pub(crate) fn authored_acbs_flags(&self) -> u32 {
        use crate::actor_values::acbs;
        let npc = &self.sig == b"NPC_";
        let crea = &self.sig == b"CREA";
        let mut bits = 0u32;
        let mut set = |on: bool, bit: u32| {
            if on {
                bits |= 1 << bit;
            }
        };
        set(npc && self.female, acbs::FEMALE);
        set(self.essential, acbs::ESSENTIAL);
        set(self.respawn, acbs::RESPAWN);
        set(npc && self.autocalc, acbs::AUTOCALC);
        set(crea && self.swims, acbs::SWIMS);
        set(crea && self.flies, acbs::FLIES);
        set(crea && self.walks, acbs::WALKS);
        set(self.pc_level_mult, acbs::PC_LEVEL_MULT);
        set(self.no_low_level, acbs::NO_LOW_LEVEL);
        set(self.no_blood_spray, acbs::NO_BLOOD_SPRAY);
        set(self.no_blood_decal, acbs::NO_BLOOD_DECAL);
        set(crea && self.no_head, acbs::NO_HEAD);
        set(crea && self.no_right_arm, acbs::NO_RIGHT_ARM);
        set(crea && self.no_left_arm, acbs::NO_LEFT_ARM);
        set(crea && self.no_combat_in_water, acbs::NO_COMBAT_IN_WATER);
        set(self.no_vats_melee, acbs::NO_VATS_MELEE);
        set(crea && self.allow_pc_dialogue, acbs::ALLOW_PC_DIALOGUE);
        set(crea && self.cant_open_doors, acbs::CANT_OPEN_DOORS);
        set(crea && self.immobile, acbs::IMMOBILE);
        set(npc && self.autocalc_service, acbs::AUTOCALC_SERVICE);
        set(self.no_knockdowns, acbs::NO_KNOCKDOWNS);
        set(self.not_pushable, acbs::NOT_PUSHABLE);
        set(crea && self.allow_pickpocket, acbs::ALLOW_PICKPOCKET);
        set(crea && self.is_ghost, acbs::IS_GHOST);
        set(self.no_head_track, acbs::NO_HEAD_TRACK);
        set(crea && self.invulnerable, acbs::INVULNERABLE);
        bits
    }
}

#[cfg(test)]
mod movement_flag_tests {
    use super::*;

    #[test]
    fn walkers_and_fliers_without_swims_keep_out_of_water() {
        let def = |swims, walks, flies| ActorDef { swims, walks, flies, ..Default::default() };
        // A gecko walks, a cazador flies: neither swims.
        assert!(def(false, true, false).keeps_out_of_water());
        assert!(def(false, false, true).keeps_out_of_water());
        // A dog walks and swims; a creature without movement flags is left alone.
        assert!(!def(true, true, false).keeps_out_of_water());
        assert!(!def(false, false, false).keeps_out_of_water());
    }
}

/// The default object at `which`'s place in a DOBJ list.
fn default_in(list: &[Option<FormId>], which: DefaultObject) -> Option<FormId> {
    list.get(which as usize).copied().flatten()
}

#[cfg(test)]
mod default_object_tests {
    use super::*;

    #[test]
    fn default_objects_by_their_place() {
        let list: Vec<Option<FormId>> = (0..34).map(|i| (i != 19).then_some(FormId(0x100 + i))).collect();
        assert_eq!(default_in(&list, DefaultObject::Stimpak), Some(FormId(0x100)));
        assert_eq!(default_in(&list, DefaultObject::PerkParalysis), Some(FormId(0x105)));
        assert_eq!(default_in(&list, DefaultObject::DeathMusic), Some(FormId(0x10B)));
        assert_eq!(default_in(&list, DefaultObject::LevelUpMusic), Some(FormId(0x10D)));
        assert_eq!(default_in(&list, DefaultObject::PlayerVoiceMale), Some(FormId(0x10E)));
        assert_eq!(default_in(&list, DefaultObject::PlayerVoiceMaleChild), Some(FormId(0x10F)));
        assert_eq!(default_in(&list, DefaultObject::PlayerVoiceFemale), Some(FormId(0x110)));
        assert_eq!(default_in(&list, DefaultObject::PlayerVoiceFemaleChild), Some(FormId(0x111)));
        assert_eq!(default_in(&list, DefaultObject::DrugWearsOffImageSpace), Some(FormId(0x114)));
        assert_eq!(default_in(&list, DefaultObject::MeltdownExplosion), Some(FormId(0x118)));
        assert_eq!(default_in(&[], DefaultObject::DeathMusic), None);
    }
}

/// One effect of a magic item (EFID + EFIT + its CTDA) on ALCH, SPEL, ENCH and INGR
/// [documented: xEdit wbEffects; schema `common.json`].
#[derive(Debug, Clone)]
pub struct EffectItem {
    pub effect: FormId,
    pub magnitude: u32,
    pub area: u32,
    /// Seconds.
    pub duration: u32,
    /// EFIT "Actor Value" (`None` for the record's "None").
    pub actor_value: Option<u16>,
    pub conditions: Vec<Condition>,
}

/// Destructible object data (DEST, then DSTD per stage) [documented: xEdit wbDEST].
#[derive(Debug, Clone, Default)]
pub struct DestructionDef {
    pub health: i32,
    /// DEST flag "VATS Targetable" [documented: xEdit wbDEST].
    pub vats_targetable: bool,
    pub stages: Vec<DestructionStage>,
}

/// One DSTD stage, with its DMDL replacement model when one follows [documented: xEdit
/// wbDEST; meanings: GECK wiki Destructible Object Stage, owner export 2026-10-09].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DestructionStage {
    pub health_percent: u8,
    pub index: u8,
    /// Model damage stage (0–8; `BSDamageStage` nodes).
    pub damage_stage: u8,
    /// 1 cap damage, 2 disable, 4 destroy.
    pub flags: u8,
    pub self_damage_per_second: i32,
    pub explosion: Option<FormId>,
    pub debris: Option<FormId>,
    pub debris_count: i32,
    pub model: Option<String>,
}

impl DestructionStage {
    pub const CAP_DAMAGE: u8 = 1;
    pub const DISABLE: u8 = 2;
    pub const DESTROY: u8 = 4;
}

pub use crate::package::PackageDef;

pub struct GameData {
    pub table: FunctionTable,
    pub param_types: ParamTypes,
    pub load_order: Vec<String>,
    /// The plugins (load-order indices) that hold each form more than one plugin
    /// defines or overrides, in load order (LU's GetFormMods, [`GameData::form_plugins`]).
    pub form_overrides: HashMap<FormId, Vec<u8>>,
    /// Each plugin's TES4 HEDR version, in load order (GetPluginHeaderVersion).
    pub plugin_versions: Vec<f32>,
    /// Each plugin's author and description, TES4 CNAM and SNAM, `None` where the header
    /// has none (SUP's GetModTraitSTR).
    pub plugin_texts: Vec<(Option<String>, Option<String>)>,
    pub units: Vec<Compiled>,
    /// [`Compiled::key`] → unit (the first unit with that key), for finding a saved
    /// lambda's unit on load.
    pub unit_by_key: HashMap<String, UnitId>,
    /// SCPT FormID → unit.
    pub scripts: HashMap<FormId, UnitId>,
    pub quests: BTreeMap<FormId, QuestDef>,
    pub infos: HashMap<FormId, InfoDef>,
    /// The dialogue lines (INFO) of each topic (DIAL), in topic order: the order of the
    /// plugin that first defines each line, then its place in that file, then each line a
    /// later plugin adds moved up behind the line its Previous INFO (PNAM) names
    /// ([`follow_previous`]) [hypothesis: the masters' INFOs carry no PNAM ordering, so file
    /// order stands for the editor's list order; dialogue.md §3.1].
    pub topics: HashMap<FormId, Vec<FormId>>,
    /// How many lines [`follow_previous`] moved out of file order (0 for the ten masters).
    pub lines_moved_by_previous: usize,
    /// The topics with shared lines (INFC / INFX, [`TopicDef::shared_infos`]): their own
    /// lines in stored order, with shared ones put in among the listing quest's own lines
    /// at their Info Index [hypothesis: dialogue.md §1.2], the order
    /// [`GameData::lines_tried`] starts from (dialogue.md §1.2).
    pub shared_lines: HashMap<FormId, Vec<FormId>>,
    /// Link From turned round: for a topic X, the topics with a line whose Link From
    /// names X, in topic record order ([`TopicDef::order`]). A line of X leads to them as
    /// to its choices, in player dialogue, conversations between actors and on the radio
    /// (`Game::links_after`, dialogue.md §3.5). `ANY`, named by 431 links, has no line,
    /// so nothing follows it.
    pub linked_from: HashMap<FormId, Vec<FormId>>,
    /// The lines some line names as a follow-up (TCFU, [`InfoDef::follow_ups`]). When a
    /// topic is said, its lines that are not follow-ups are tried first
    /// (`Game::select_line`, dialogue.md §3.4).
    pub follow_up_targets: HashSet<FormId>,
    /// Every topic (DIAL) from its winning record.
    pub topic_defs: HashMap<FormId, TopicDef>,
    /// The top-level topics of type Topic, in the order they are offered: priority,
    /// highest first, then record order ([`TopicDef::order`]).
    pub top_level: Vec<FormId>,
    /// The special topics GREETING, GOODBYE and SpeechChallengeFailure, by Editor ID
    /// [documented: GECK wiki Topics Tab and Conversation Tab, owner export].
    pub greeting: Option<FormId>,
    pub goodbye: Option<FormId>,
    pub speech_challenge_failure: Option<FormId>,
    /// PACK begin / end / change result scripts.
    pub packages: HashMap<FormId, [Option<UnitId>; 3]>,
    /// Patrol points' result scripts (REFR, after XPPA), run when a patrolling actor
    /// arrives (docs/spec-notes/packages.md §7.4).
    pub patrol_scripts: HashMap<FormId, UnitId>,
    /// Every PACK as typed data ([`crate::package`]).
    pub package_defs: HashMap<FormId, PackageDef>,
    pub messages: HashMap<FormId, MessageDef>,
    /// Terminals (TERM), with their menu items' result scripts.
    pub terminals: HashMap<FormId, TerminalDef>,
    /// Notes (NOTE).
    pub notes: HashMap<FormId, NoteDef>,
    pub form_lists: HashMap<FormId, Vec<FormId>>,
    /// WEAP and ARMO `REPL`: the form list of the items that can repair the item
    /// [documented: xEdit wbREPL, GECK wiki Armor "Repair Item List"]; read raw
    /// (docs/spec-notes/repair.md §5).
    pub repair_lists: HashMap<FormId, FormId>,
    /// MICN (Menu Icon) ICON: the icon's texture file as the record names it, under
    /// the textures folder [documented: GECK wiki, MenuIcon; corner-messages.md §3].
    pub menu_icons: HashMap<FormId, String>,
    pub actors: HashMap<FormId, ActorDef>,
    /// Combat styles (CSTY), the values the combat AI uses ([`CombatStyleDef`]).
    pub combat_styles: HashMap<FormId, CombatStyleDef>,
    /// Leveled actor lists (LVLN, LVLC): what a placed leveled actor or a `TPLT` naming a
    /// list stands for ([`GameData::template_source`]).
    pub leveled_actors: HashMap<FormId, LeveledActorDef>,
    /// SCRI of every winning record that has one (base forms, quests).
    pub form_script: HashMap<FormId, FormId>,
    /// Lower-case Editor ID → FormID (every winning record with an EDID).
    pub edids: HashMap<String, FormId>,
    pub edid_of: HashMap<FormId, String>,
    /// Record header flags of the winning records that have any (GetFlagsLow/High).
    pub form_flags: HashMap<FormId, u32>,
    /// Where each inventory item form was first defined: (plugin, place among its records),
    /// for GetLoadedTypeArray's order.
    pub item_order: HashMap<FormId, (u8, u32)>,
    /// Where each actor base (NPC_, CREA) was first defined: (plugin, place among its
    /// records), for GetFactionMembers' order (`nvse_jg4`).
    pub actor_order: HashMap<FormId, (u8, u32)>,
    /// OBND of the base forms that have one ([`BOUNDS_SIGS`]): min x, y, z, max x, y, z.
    pub object_bounds: HashMap<FormId, [i16; 6]>,
    /// The name the player sees (FULL) of every winning record that has one.
    pub full_names: HashMap<FormId, String>,
    pub sigs: HashMap<FormId, [u8; 4]>,
    /// Lazy cache for [`GameData::forms_of_type`], keyed by form type code, populated on
    /// first query so repeat `GetLoadedTypeArray` calls avoid scanning all `sigs` [design].
    forms_by_type_cache: std::sync::RwLock<HashMap<i32, Vec<FormId>>>,
    /// The decoded subrecords of the record types JIP's trait getters read
    /// ([`FIELD_SIGS`]; jip-records.md), enums as their numbers, decoded on first use.
    pub record_fields: RecordFields,
    /// The game's INI text the extender's INI functions read (GetNumericINISetting …),
    /// later files overriding earlier ones; empty unless the host gives it
    /// (`crate::nvse_settings`).
    pub ini_texts: Vec<String>,
    /// Placed references whose base runs a script, by cell, in FormID order.
    pub scripted_by_cell: BTreeMap<FormId, Vec<FormId>>,
    /// Placed actors whose base has no script of its own but may run one through its
    /// template chain (template flag "Script", any leveled pick), by cell, in FormID
    /// order ([`GameData::script_through_template`]). The lifecycle scheduler reads this
    /// index for OnLoad and OnReset (docs/spec-notes/scripts.md §7.1).
    pub template_scripted_by_cell: BTreeMap<FormId, Vec<FormId>>,
    /// REPU DATA: the reputation's scale (maximum fame or infamy).
    pub reputations: HashMap<FormId, f32>,
    /// REPU FULL: the reputation's name as the player sees it.
    pub reputation_names: HashMap<FormId, String>,
    /// CHAL DATA flags (1 Start Disabled, 2 Recurring, 4 Show Zero Progress).
    pub challenges: HashMap<FormId, u32>,
    /// CHAL DATA threshold: the progress at which the challenge is completed.
    pub challenge_thresholds: HashMap<FormId, u32>,
    /// The whole CHAL record: what counts towards it (`crate::challenges`).
    pub challenge_defs: HashMap<FormId, crate::challenges::ChallengeDef>,
    pub regions: HashMap<FormId, RegionDef>,
    pub magic_effects: HashMap<FormId, MagicEffectDef>,
    /// The effects of every ALCH, SPEL, ENCH and INGR, in record order.
    pub effect_items: HashMap<FormId, Vec<EffectItem>>,
    /// ALCH: what using the item needs besides its effects ([`IngestibleDef`]).
    pub ingestibles: HashMap<FormId, IngestibleDef>,
    /// SPEL SPIT type (0 Actor Effect, 1 Disease, 2 Power, 3 Lesser Power, 4 Ability,
    /// 5 Poison, 10 Addiction) [documented: xEdit wbDefinitionsFNV.pas SPEL].
    pub spell_types: HashMap<FormId, i64>,
    /// SPEL SPIT flags (bit 4 "Area Effect Ignores LOS", `crate::area_effects`)
    /// [documented: xEdit wbDefinitionsFNV.pas SPEL].
    pub spell_flags: HashMap<FormId, u8>,
    /// RACE SPLO: the race's actor effects [hypothesis: as NPC_'s; none in FalloutNV.esm,
    /// observed].
    pub race_spells: HashMap<FormId, Vec<FormId>>,
    /// Voice types (VTYP) whose DNAM leaves "Allow Default Dialog" off: their speakers say
    /// only lines conditioned on them (docs/spec-notes/actor-flags.md §9).
    pub voices_without_default: std::collections::HashSet<FormId>,
    /// ARMO EITM: the object effect (ENCH) an armour piece carries.
    pub armor_enchantments: HashMap<FormId, FormId>,
    /// ENCH ENIT type: 2 Weapon (applied by a hit), 3 Apparel (applied while worn).
    pub enchantment_types: HashMap<FormId, u32>,
    /// Races with the Child flag (RACE DATA).
    pub child_races: HashSet<FormId>,
    /// RACE ONAM and YNAM: the older and the younger race of a race (AgeRace)
    /// [documented: xEdit wbDefinitionsFNV.pas RACE "Older", "Younger"].
    pub race_older: HashMap<FormId, FormId>,
    pub race_younger: HashMap<FormId, FormId>,
    /// FACT XNAM: (other faction, group combat reaction 0 neutral, 1 enemy, 2 ally,
    /// 3 friend).
    pub faction_reactions: HashMap<FormId, Vec<(FormId, i64)>>,
    /// Directional decoded FACT XNAM disposition modifiers, separate from combat.
    pub faction_modifiers: HashMap<FormId, Vec<(FormId, i64)>>,
    /// FACT DATA flags as one number: the first byte's bits (1 Hidden from PC, 2 Evil,
    /// 4 Special Combat) and the second's shifted by 8 (256 Track Crime, 512 Allow Sell)
    /// [documented: xEdit wbDefinitionsFNV.pas FACT, schema `gameplay.json`; the numbering
    /// is the GECK wiki GetFactionFlags'] (docs/spec-notes/crime-karma.md §2).
    pub faction_flags: HashMap<FormId, u16>,
    /// FACT WMI1: the reputation a faction's members count towards [documented: xEdit
    /// wbDefinitionsFNV.pas FACT "Reputation", schema `gameplay.json`; observed:
    /// GoodspringsFaction 00104C6E names RepNVGoodsprings 00104C22].
    pub faction_reputations: HashMap<FormId, FormId>,
    /// Cells with an owner, the Public place flag or the Off Limits form flag
    /// (`crate::crime::CellDef`).
    pub cells: HashMap<FormId, crate::crime::CellDef>,
    /// Whether `Data\NVSE\Plugins\hNVSE.ini` enabled `[Main] bMergeRegionLists=1` at
    /// load time. Read once at load (never re-read after script writes) so script queries
    /// and world consumers always agree [documented: hNVSE 1.2 (Nexus Mods ID 85748,
    /// `https://www.nexusmods.com/newvegas/mods/85748`) public changelog and shipped
    /// `Data\NVSE\Plugins\hNVSE.ini` comments ("Force-merges cell region lists (XCLR
    /// subrecord in xEdit) during loading"); DLL/PDB/source not consulted].
    pub hnvse_merge_region_lists: bool,
    /// Union of `CELL` `XCLR` region FormIDs across load-order overrides for overridden
    /// exterior cells (overriding plugin first, duplicates removed), populated lazily via
    /// [`GameData::enable_hnvse_region_merge`] ONLY when `hNVSE.ini` enables
    /// `[Main] bMergeRegionLists=1` at load time so startup cost is zero when absent or
    /// disabled [observed: hNVSE 120 on 188TradingPost (`0x000DDF1C`) with a test
    /// override ESP; design: skipping interior cells and deleted overrides].
    pub merged_cell_regions: HashMap<FormId, Vec<FormId>>,
    /// XRNK of placed references: the faction rank that owns them (with an XOWN faction).
    pub owner_ranks: HashMap<FormId, i32>,
    pub destructibles: HashMap<FormId, DestructionDef>,
    /// WEAP DNAM skill (actor value code).
    pub weapon_skills: HashMap<FormId, i64>,
    /// WEAP DNAM animation type (the record's own numbering, 0 Hand to Hand … 13
    /// Thrown) [documented: xEdit wbWeaponAnimTypeEnum, schemas/fnv/objects.json].
    pub weapon_anim_types: HashMap<FormId, i64>,
    /// FURN marker flags (MNAM as a little-endian number; 0 when absent): bit 30
    /// furniture to sit on, bit 31 to sleep on [observed: docs/spec-notes/furniture.md
    /// §1].
    pub furniture: HashMap<FormId, u32>,
    /// IDLM: the idle collection of each idle marker base.
    pub idle_markers: HashMap<FormId, crate::package::IdleCollection>,
    /// IDLE: what playing an idle animation needs (`crate::idles`).
    pub idles: HashMap<FormId, IdleDef>,
    /// The idle tree: each IDLE's children (the roots under `None`), siblings in the
    /// order their `previous` links chain them (animation.md §2.1).
    pub idle_children: HashMap<Option<FormId>, Vec<FormId>>,
    /// IMAD: how long each image space modifier plays.
    pub image_modifiers: HashMap<FormId, ImageModifierDef>,
    /// TACT: the record-header flags of every talking activator (radio stations: the
    /// Continuous Broadcast bit, `crate::radio`; docs/spec-notes/radio.md §2).
    pub station_flags: HashMap<FormId, u32>,
    /// TACT VNAM: each talking activator's voice type [documented: xEdit TACT "Voice
    /// Type"; schema `placed.json`], for GetIsVoiceType and the voice gate
    /// (`Game::voice_type_of`; #3060: The Pitt's Ashur intercom, DLC01RaiderMaleColdVoice).
    pub talking_voices: HashMap<FormId, FormId>,
    /// PERK: perks and traits ([`PerkDef`]).
    pub perks: HashMap<FormId, PerkDef>,
    /// AVIF: the name, short name, description and icon of each actor value
    /// ([`GameData::actor_value_info`]).
    pub actor_value_infos: HashMap<FormId, ActorValueInfo>,
    /// RCPE: the recipes (`crate::crafting`).
    pub recipes: HashMap<FormId, crate::crafting::RecipeDef>,
    /// CPTH and CAMS: the camera paths and shots (`crate::camera_paths`).
    pub camera_paths: HashMap<FormId, crate::camera_paths::CameraPathDef>,
    pub camera_shots: HashMap<FormId, crate::camera_paths::CameraShotDef>,
    /// RCCT: the recipe categories and subcategories.
    pub recipe_categories: HashMap<FormId, crate::crafting::RecipeCategoryDef>,
    /// CSNO: the casinos (`crate::casino`).
    pub casinos: HashMap<FormId, crate::casino::CasinoDef>,
    /// CCRD: the Caravan cards (`crate::caravan`).
    pub caravan_cards: HashMap<FormId, crate::caravan::CardDef>,
    /// CDCK: the Caravan decks.
    pub caravan_decks: HashMap<FormId, crate::caravan::DeckDef>,
    /// RADS, DEHY, HUNG, SLPD: the stages of rads and of Hardcore's needs by actor value
    /// code, lowest threshold first (`crate::needs`).
    pub need_stages: HashMap<u16, Vec<NeedStage>>,
    /// WATR XNAM: each water type's actor effect (`crate::radiation`).
    pub water_effects: HashMap<FormId, FormId>,
    /// ACTI WNAM "Water Type": the activators the player drinks from (sinks, toilets,
    /// fountains, water pumps) and the water drunk [documented: xEdit ACTI WNAM; GECK
    /// wiki Activator, "Non scripted activators can serve as drinkable water sources"]
    /// (docs/spec-notes/activators.md §9).
    pub drink_waters: HashMap<FormId, FormId>,
    /// Activators that name a radio station (ACTI RNAM): radio receivers, whose on and
    /// off looks follow the radio rules (`crate::radio`), not their open state.
    pub radio_receivers: HashSet<FormId>,
    /// DOBJ DATA "Default Objects", by their place in the list ([`DefaultObject`])
    /// [documented: xEdit DOBJ] (docs/spec-notes/default-objects.md).
    pub default_objects: Vec<Option<FormId>>,
    /// WATR "Causes Damage" (FNAM bit 0) with its DATA damage: health a second lost in
    /// the water, by water type; only waters with the flag and a damage above 0
    /// (`crate::radiation`, docs/spec-notes/radiation.md §3.1).
    pub water_damage: HashMap<FormId, f64>,
    /// The radiation markers (placed references with XRAD) by the space they are in: a
    /// worldspace, or an interior cell (`crate::radiation`).
    pub radiation_markers: HashMap<FormId, Vec<crate::radiation::RadMarker>>,
    /// CLAS DATA "Buys/Sells and Services" flags by class (`crate::services`)
    /// [documented: xEdit wbDefinitionsFNV.pas CLAS DATA, after the four tag skills and
    /// the class flags].
    pub class_services: HashMap<FormId, u32>,
    pub issues: Issues,
    pub read_issues: ReadIssues,
    /// What the compile of mods' source text made of each unit it picked
    /// (`crate::source_compile`): whether it compiled, and the names it calls that
    /// nothing knows. For the engine's log and `fnvtool ext report`.
    pub source_units: Vec<crate::source_compile::SourceUnit>,
}

/// An idle animation (IDLE) as far as playing it goes: the file it names and how often
/// it repeats. The tree and the conditions are read by `fnv_actor::idle`
/// (docs/spec-notes/animation.md §1.1).
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize)]
pub struct IdleDef {
    /// `MODL` as stored, relative to `meshes\`; only a name ending in `.kf` is an
    /// animation file (other records name a folder: the inner nodes of the tree).
    pub model: Option<String>,
    /// DATA loop minimum and maximum [documented: xEdit IDLE DATA].
    pub loop_min: u8,
    pub loop_max: u8,
    /// The record's conditions; `None` when one of them could not be read (the idle is
    /// then never chosen by its conditions, `Game::idle_allowed`).
    pub conditions: Option<Vec<Condition>>,
    /// ANAM: the parent and the previous sibling in the idle tree [documented: xEdit
    /// IDLE ANAM; animation.md §2.1].
    pub parent: Option<FormId>,
    pub previous: Option<FormId>,
    /// DATA replay delay (i16) [documented: xEdit IDLE DATA].
    pub replay_delay: i16,
    /// DATA flags bit 0 "No attacking": no attacking while the idle plays [documented:
    /// xEdit IDLE DATA; GECK wiki Idle Animations Window]. Absent in the 6-byte DATA.
    pub no_attacking: bool,
    /// DATA "Animation Group Section": the low six bits of byte 0 (7 Special Idle,
    /// 20 Whole Body, 21 Upper Body, ...) [documented: xEdit IDLE DATA, wbIdleAnam;
    /// `fnv_actor::idle::SECTIONS`].
    pub section: u8,
    /// DATA byte 0 bit 0x40, which xEdit labels "Loose Idle" [documented: xEdit
    /// wbIdleAnam]: an idle for scripts to play by name (`LooseNVPHMCYesMan`), not one the
    /// tree's walk gives [hypothesis].
    pub loose: bool,
}

/// The "Upper Body" animation group section ([`IdleDef::section`]) [documented: xEdit
/// wbIdleAnam].
pub const IDLE_SECTION_UPPER_BODY: u8 = 21;

impl IdleDef {
    /// From the decoded `MODL` (the name) and `DATA` (schema `gameplay.json` IDLE:
    /// animation group section, loop minimum and maximum, replay delay, flags
    /// [documented: xEdit wbDefinitionsFNV.pas IDLE DATA]); a missing field reads 0.
    pub fn from_fields(model: Option<&str>, data: Option<&Json>) -> IdleDef {
        let model = model.map(|m| m.trim().to_string()).filter(|s| !s.is_empty());
        let n = |k: &str| data.and_then(|d| d.get(k)).and_then(Json::as_i64).unwrap_or(0);
        let no_attacking = flag_bits(data.and_then(|d| d.get("flags"))) & 1 != 0;
        IdleDef { model, loop_min: n("loop_min") as u8, loop_max: n("loop_max") as u8, conditions: Some(Vec::new()), parent: None, previous: None, replay_delay: n("replay_delay") as i16, no_attacking, section: n("animation_group_section") as u8 & 0x3F, loose: n("animation_group_section") as u8 & 0x40 != 0 }
    }

    /// Whether the record names an animation file.
    pub fn has_file(&self) -> bool {
        self.model.as_deref().is_some_and(|m| m.to_ascii_lowercase().ends_with(".kf"))
    }
}

/// The children of each IDLE (roots under `None`). Siblings follow their `previous`
/// chain: first a sibling whose previous is not in the group, then the one naming it,
/// and so on; siblings outside any chain come last in FormID order [observed: 357 of 364
/// sibling groups form one chain, animation.md §2.1].
pub fn idle_children(idles: &HashMap<FormId, IdleDef>) -> HashMap<Option<FormId>, Vec<FormId>> {
    let mut groups: HashMap<Option<FormId>, Vec<FormId>> = HashMap::new();
    for (id, d) in idles {
        groups.entry(d.parent).or_default().push(*id);
    }
    for list in groups.values_mut() {
        list.sort();
        let members: std::collections::HashSet<FormId> = list.iter().copied().collect();
        let mut after: HashMap<FormId, FormId> = HashMap::new();
        for id in list.iter() {
            if let Some(p) = idles[id].previous.filter(|p| members.contains(p)) {
                after.entry(p).or_insert(*id);
            }
        }
        let mut ordered = Vec::with_capacity(list.len());
        let mut seen = std::collections::HashSet::new();
        for first in list.iter().filter(|id| idles[id].previous.is_none_or(|p| !members.contains(&p))) {
            let mut at = Some(*first);
            while let Some(id) = at.filter(|id| seen.insert(*id)) {
                ordered.push(id);
                at = after.get(&id).copied();
            }
        }
        ordered.extend(list.iter().filter(|id| !seen.contains(id)).copied());
        *list = ordered;
    }
    groups
}

/// A stage of rads or of a Hardcore need (RADS, DEHY, HUNG, SLPD).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NeedStage {
    /// The stage record.
    pub stage: FormId,
    /// The value from which it holds.
    pub threshold: u32,
    /// Its actor effect (SPEL).
    pub spell: FormId,
}

/// A perk or trait (PERK) as the character menus need it.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize)]
pub struct PerkDef {
    /// FULL, DESC, ICON (under the textures folder).
    pub name: String,
    pub description: String,
    pub icon: Option<String>,
    /// DATA [documented: xEdit wbDefinitionsFNV.pas PERK DATA: Trait, Min Level, Ranks,
    /// Playable, Hidden, one byte each; observed: five bytes on every perk of the
    /// masters, `01 01 01 01 00` on each of the sixteen traits, docs/spec-notes/chargen.md
    /// §2].
    pub is_trait: bool,
    pub min_level: u8,
    pub ranks: u8,
    pub playable: bool,
    pub hidden: bool,
    /// The perk's requirements: the CTDA before its first effect; `None` when one of
    /// them could not be read.
    #[serde(skip)]
    pub conditions: Option<Vec<Condition>>,
    /// Its effects (PRKE … PRKF), in record order (`crate::perks`).
    #[serde(skip)]
    pub entries: Vec<PerkEntry>,
}

/// One effect of a perk (PRKE header, then its DATA and parameters, up to PRKF)
/// [documented: xEdit wbDefinitionsFNV.pas PERK, the header's type, rank and priority,
/// `schemas/fnv/gameplay.json`; observed: the DATA and EPFD layouts, docs/spec-notes/
/// aid-items.md §10].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PerkEntry {
    /// The perk rank it belongs to, 0 for the first.
    pub rank: u8,
    pub priority: u8,
    pub effect: PerkEffect,
    /// Its place among the record's effects (PRKE), 0-based: the script unit of an
    /// entry is keyed by it (`fnv_script::unit::UnitOwner::Perk`).
    pub ordinal: u32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum PerkEffect {
    /// Type 1: the spell is the owner's while it has the perk.
    Ability(FormId),
    /// Type 2.
    EntryPoint(EntryPointDef),
    /// Type 0: the quest is set to the stage when the rank is gained (`crate::perks::
    /// gain_rank`) [documented: xEdit wbDefinitionsFNV.pas PERK effect type "Quest +
    /// Stage"; observed: DATA is the quest's FormID then the stage in one byte, three
    /// bytes after it unused, Intense Training's `b2380000 65 cdcdcd` for Generic stage
    /// 101, whose result script reads "triggered by Intense Training perk rank 1"].
    QuestStage { quest: FormId, stage: u8 },
    /// A DATA that could not be read.
    #[default]
    Other,
}

/// An entry point effect: which entry point, which function, its number, and its
/// conditions by tab.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EntryPointDef {
    /// DATA byte 0 [observed: the code identifies the entry point, e.g. 23 on Chem
    /// Resistant alone, aid-items.md §10].
    pub point: u8,
    /// DATA byte 1 [observed: 1 with 0/1 flags (Light Step, Computer Whiz), 2 with
    /// additions (Educated +2, Toughness +3), 3 with factors (Chem Resistant 0.5, Chemist
    /// 2.0): hypothesis Set, Add, Multiply].
    pub function: u8,
    /// EPFD as one float when EPFT is 1 [observed: Chem Resistant `0000003F` = 0.5].
    pub value: Option<f32>,
    /// EPFT, the kind of EPFD [documented: xEdit wbDefinitionsFNV.pas PERK, EPFT
    /// "Entry Point Function Parameter Type"; observed: 1 with one float on every Set,
    /// Add and Multiply entry of the masters, 3 with a leveled list on the functions 8, 4
    /// with a script on the functions 9].
    pub param_type: Option<u8>,
    /// EPFD as read, for the functions whose parameters are not one float (Add Range To
    /// Value, the actor value functions; `crate::perks::resolve`) [hypothesis: their
    /// layout; no entry of the masters has one].
    pub raw: Vec<u8>,
    /// The CTDA with the PRKC tab before them (0 the perk owner); `None` when one could
    /// not be read.
    pub conditions: Option<Vec<(i8, Condition)>>,
    /// EPF2 "Button Label" of an activate choice ("Devour", "Murder") [documented: xEdit
    /// wbDefinitionsFNV.pas PERK; observed: the four entries of the masters with one,
    /// `crate::activate_choices`].
    pub button: Option<String>,
    /// EPF3 "Script Flags" bit 0, Run Immediately [documented: xEdit].
    pub run_immediately: bool,
    /// The entry's script (EPFT 4, SCHR/SCDA after it).
    pub unit: Option<UnitId>,
    /// EPFD as a FormID when EPFT is 3, a leveled item list [observed: the add-ons'
    /// Add Leveled List On Death entries, Them's Good Eating and Certified Tech, name
    /// LVLI records; docs/spec-notes/death-items.md §3].
    pub leveled_list: Option<FormId>,
}

impl PerkDef {
    /// The fields of the raw DATA (see the field notes); a missing byte reads 0.
    pub fn from_data(data: Option<&[u8]>) -> PerkDef {
        let byte = |i: usize| data.and_then(|d| d.get(i)).copied().unwrap_or(0);
        PerkDef { is_trait: byte(0) != 0, min_level: byte(1), ranks: byte(2), playable: byte(3) != 0, hidden: byte(4) != 0, ..Default::default() }
    }
}

/// An actor value's AVIF record: its FULL name, ANAM short name ("PER") [documented:
/// xEdit wbDefinitionsFNV.pas AVIF "Short Name"], DESC and ICON.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize)]
pub struct ActorValueInfo {
    pub name: String,
    pub short_name: String,
    pub description: String,
    pub icon: Option<String>,
}

/// A NUL-terminated string as stored.
fn zstring(b: &[u8]) -> String {
    // In this run's code page (fnv_util::text, #1439).
    fnv_util::text::decode_z(b).trim().to_string()
}

/// The timing of an image space modifier (IMAD DNAM) [documented: xEdit
/// wbDefinitionsFNV.pas IMAD DNAM "Animatable" u32, "Duration" f32;
/// docs/spec-notes/image-modifiers.md §1].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageModifierDef {
    /// The modifier plays over `duration` seconds and then ends; without the flag it
    /// stays until it is removed.
    pub animatable: bool,
    pub duration: f32,
}

pub(crate) fn hexid(v: &Json) -> Option<u32> {
    let s = v.as_str()?;
    u32::from_str_radix(s.strip_prefix("0x")?, 16).ok()
}

fn has_flag(flags: Option<&Json>, name: &str) -> bool {
    flags
        .and_then(|f| f.get("set"))
        .and_then(Json::as_array)
        .is_some_and(|a| a.iter().any(|n| n.as_str() == Some(name)))
}

fn flag_bits(flags: Option<&Json>) -> u32 {
    flags.and_then(|f| f.get("value")).and_then(Json::as_u64).unwrap_or(0) as u32
}

/// A combat style (CSTY) [documented: layout xEdit wbDefinitionsFNV.pas CSTY, schema
/// `ai.json`; meanings GECK wiki Combat Style, owner export]: the values the combat AI
/// uses (docs/design/combat-ai.md §10.4). Percentages as the record has them (0–100).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CombatStyleDef {
    /// Melee decision: base chances to block and to attack; bonuses to the attack chance
    /// when the actor fights hand to hand.
    pub block_chance: f32,
    pub attack_chance: f32,
    pub hand_to_hand_bonus: f32,
    pub power_attack_chance: f32,
    /// Power Attacks: the shares (0–100) of the normal, forward, back, left and right
    /// power attacks [documented: GECK wiki Combat Style; layout xEdit CSTD].
    pub power_attack_dirs: [f32; 5],
    /// Power Attacks: the bonuses to the power attack chance while the opponent recoils
    /// or staggers and while it is unconscious; the bonus to the attack chance while it
    /// is unconscious; flag "Choose Attack using % Chance" (else by movement)
    /// [documented: GECK wiki Combat Style; layout xEdit CSTD].
    pub recoil_stagger_power_bonus: f32,
    pub unconscious_power_bonus: f32,
    pub unconscious_bonus: f32,
    pub power_by_chance: bool,
    /// Melee decision: how long an actor that neither attacks nor blocks holds, seconds.
    pub hold_timer: (f32, f32),
    /// Advanced: the attack skill modifier (`base + mult × skill / 100`) and the attack
    /// chance's multipliers under attack and not.
    pub attack_skill_mod: (f32, f32),
    pub attack_under_attack_mult: f32,
    pub attack_not_under_attack_mult: f32,
    /// Melee decision: the bonus to the attack chance while the opponent recoils or
    /// staggers; Advanced: the block skill modifier (`base + mult × 50 / 100`), the block
    /// chance's multipliers under attack and not, and the attack chance's multiplier while
    /// the opponent blocks [documented: GECK wiki Combat Style].
    pub recoil_stagger_bonus: f32,
    pub block_skill_mod: (f32, f32),
    pub block_under_attack_mult: f32,
    pub block_not_under_attack_mult: f32,
    pub attack_during_block_mult: f32,
    /// Simple: what the actor may fight with: 0 None, 1 Melee Only, 2 Ranged Only.
    pub weapon_restrictions: u32,
    /// Simple: multipliers of the weapon's min and max range in range calculations.
    pub range_mult: (f32, f32),
    /// Simple: multipliers of the weapon's semi-automatic firing delays.
    pub semi_auto_delay_mult: (f32, f32),
    pub max_targeting_fov: f32,
    pub combat_radius: f32,
    /// Flag "Flee Based on Personal Survival".
    pub flee_personal_survival: bool,
    /// Simple, cover: how far cover is looked for; the chance (0–100) to use cover for the
    /// next 10 seconds; how long to stay behind it, to stand out of it looking for targets,
    /// and to fire out of it once they are seen, seconds (min, max) [documented: GECK wiki
    /// Combat Style].
    pub cover_search_radius: f32,
    pub take_cover_chance: f32,
    pub wait_timer: (f32, f32),
    pub wait_to_fire_timer: (f32, f32),
    pub fire_timer: (f32, f32),
    /// Maneuver decision: Dodge % Chance and Left/Right % Chance (0–100); the Dodge L/R,
    /// Forward and Back timers and the Idle Timer, seconds (min, max); Advanced: the
    /// dodge fatigue and encumbered speed modifiers (base, mult) and the dodge
    /// multipliers (while, not) under attack, back under attack, forward while attacking
    /// [documented: GECK wiki Combat Style; docs/design/combat-ai.md §17.1].
    pub dodge_chance: f32,
    pub left_right_chance: f32,
    pub dodge_lr_timer: (f32, f32),
    pub dodge_forward_timer: (f32, f32),
    pub dodge_back_timer: (f32, f32),
    pub idle_timer: (f32, f32),
    pub dodge_fatigue_mod: (f32, f32),
    pub encumbered_speed_mod: (f32, f32),
    pub dodge_mult: (f32, f32),
    pub dodge_back_mult: (f32, f32),
    pub dodge_forward_mult: (f32, f32),
    /// Advanced: the power attack fatigue modifier (base, mult) (§17.3).
    pub power_attack_fatigue_mod: (f32, f32),
    /// Rushing Attack % Chance and Distance Mult [documented: the names only,
    /// GetCombatStyleTraitNumeric; §17.2].
    pub rushing_attack_chance: f32,
    pub rushing_attack_distance_mult: f32,
    /// Flags (GetCombatStyleFlag 1, 4–8): Melee Alert OK, Ignore Threats, Ignore Damaging
    /// Self / Group / Spectators, Cannot Use Stealthboy (§17.6–§17.8).
    pub melee_alert_ok: bool,
    pub ignore_threats: bool,
    pub ignore_damaging_self: bool,
    pub ignore_damaging_group: bool,
    pub ignore_damaging_spectators: bool,
    pub cannot_use_stealthboy: bool,
}

/// Everything read from one winning record. Large (3,848 bytes [design: `size_of`,
/// 2026-11-06]), so it travels boxed: each move of one is that many bytes copied, and
/// [`reads_nothing`] spares most records one altogether.
#[derive(Debug, Default)]
struct Read {
    edid: Option<String>,
    /// FULL: the name the player sees.
    full: Option<String>,
    script: Option<FormId>,
    quest: Option<QuestDef>,
    info: Option<InfoDef>,
    topic: Option<TopicDef>,
    message: Option<MessageDef>,
    terminal: Option<TerminalDef>,
    note: Option<NoteDef>,
    form_list: Option<Vec<FormId>>,
    menu_icon: Option<String>,
    actor: Option<ActorDef>,
    combat_style: Option<CombatStyleDef>,
    leveled_actor: Option<LeveledActorDef>,
    package: Option<PackageDef>,
    reputation: Option<f32>,
    reputation_name: Option<String>,
    /// CHAL DATA, SNAM, XNAM, FULL and DESC.
    challenge: Option<crate::challenges::ChallengeDef>,
    region: Option<RegionDef>,
    magic_effect: Option<MagicEffectDef>,
    effects: Option<Vec<EffectItem>>,
    ingestible: Option<IngestibleDef>,
    spell_type: Option<i64>,
    spell_flags: Option<u8>,
    /// RACE SPLO list; ARMO EITM; ENCH ENIT type (`GameData::race_spells` …).
    race_spells: Option<Vec<FormId>>,
    /// VTYP DNAM bit 0, "Allow Default Dialog"; `None` without a DNAM.
    voice_default: Option<bool>,
    object_effect: Option<FormId>,
    repair_list: Option<FormId>,
    enchantment_type: Option<u32>,
    child_race: bool,
    race_older: Option<FormId>,
    race_younger: Option<FormId>,
    faction: Option<Vec<(FormId, i64)>>,
    faction_modifiers: Option<Vec<(FormId, i64)>>,
    faction_flags: Option<u16>,
    faction_reputation: Option<FormId>,
    cell: Option<crate::crime::CellDef>,
    owner_rank: Option<i32>,
    destruction: Option<DestructionDef>,
    weapon_skill: Option<i64>,
    weapon_anim_type: Option<i64>,
    furniture: Option<u32>,
    idle_marker: Option<crate::package::IdleCollection>,
    idle: Option<IdleDef>,
    image_modifier: Option<ImageModifierDef>,
    perk: Option<PerkDef>,
    av_info: Option<ActorValueInfo>,
    recipe: Option<crate::crafting::RecipeDef>,
    camera_path: Option<crate::camera_paths::CameraPathDef>,
    camera_shot: Option<crate::camera_paths::CameraShotDef>,
    recipe_category: Option<crate::crafting::RecipeCategoryDef>,
    casino: Option<crate::casino::CasinoDef>,
    class_services: Option<u32>,
    caravan_card: Option<crate::caravan::CardDef>,
    caravan_deck: Option<crate::caravan::DeckDef>,
    need_stage: Option<(u16, u32, FormId)>,
    water_effect: Option<FormId>,
    /// An ACTI's WNAM: the water drunk from it.
    drink_water: Option<FormId>,
    /// An ACTI with an RNAM (a radio station): a radio receiver.
    radio_receiver: bool,
    /// DOBJ DATA: the default objects, by their place in the list (null: `None`).
    default_objects: Option<Vec<Option<FormId>>>,
    /// A WATR's damage a second when it has "Causes Damage", else 0 (`Some` on every WATR).
    water_damage: Option<u16>,
    units: Vec<ScriptUnit>,
    issues: Issues,
    read_issues: ReadIssues,
}

const UNIT_SIGS: &[&[u8; 4]] = &[b"SCPT", b"QUST", b"INFO", b"PACK", b"TERM", b"PERK"];

/// Winning records read at a time while [`GameData::build`] takes them in [engineering
/// choice: a few tens of MB of reads, enough work per slice for every core].
const READ_SLICE: usize = 16_384;

/// Whether [`read_one`] finds script units in the record: a [`UNIT_SIGS`] type, or a
/// patrol point's result script (REFR with XPPA).
fn carries_units(r: &Record) -> bool {
    UNIT_SIGS.contains(&&r.header.sig) || (&r.header.sig == b"REFR" && has_any_sub(r, &[*b"XPPA"]))
}

/// [`carries_units`] as it was first written, for [`GameData::build_the_long_way`].
fn carries_units_copying(r: &Record) -> bool {
    UNIT_SIGS.contains(&&r.header.sig) || (&r.header.sig == b"REFR" && r.subrecords().is_ok_and(|subs| subs.iter().any(|s| &s.sig == b"XPPA")))
}

/// The first OBND of a readable record, if it holds six values.
fn bounds_of(r: &Record) -> Option<[i16; 6]> {
    let mut first: Option<Option<[i16; 6]>> = None;
    r.visit_subrecords(|sig, data| {
        if sig == b"OBND" && first.is_none() {
            let v = data.as_chunks::<2>().0;
            first = Some((v.len() >= 6).then(|| std::array::from_fn(|i| i16::from_le_bytes(v[i]))));
        }
    })
    .ok()?;
    first?
}

/// [`bounds_of`] as it was first written, for [`GameData::build_the_long_way`].
fn bounds_of_copying(r: &Record) -> Option<[i16; 6]> {
    let s = r.subrecords().ok()?.into_iter().find(|s| &s.sig == b"OBND")?;
    let v: Vec<i16> = s.data.as_chunks::<2>().0.iter().take(6).map(|b| i16::from_le_bytes(*b)).collect();
    <[i16; 6]>::try_from(v).ok()
}

/// Whether the record is readable and has one of `subs`; an unreadable record has none, as
/// [`Record::subrecords`] gives it none.
fn has_any_sub(r: &Record, subs: &[[u8; 4]]) -> bool {
    let mut found = false;
    r.visit_subrecords(|sig, _| found |= subs.contains(sig)).is_ok() && found
}

/// The most common record types, none of which [`read_one`] treats by type except for the
/// [`BULK_READ_SUBS`] it looks for. Of 628,464 winning records of the base game with its
/// add-ons, 419,409 are REFR and 42,467 LAND [observed: count, 2026-11-06].
const BULK_SIGS: [[u8; 4]; 6] = [*b"REFR", *b"ACHR", *b"ACRE", *b"PGRE", *b"LAND", *b"NAVM"];

/// All that [`read_one`] looks for in a record of the [`BULK_SIGS`] types: the name fields
/// and script every record may have, a destructible's data, a patrol point's script and an
/// owner's rank.
const BULK_READ_SUBS: [[u8; 4]; 6] = [*b"EDID", *b"FULL", *b"SCRI", *b"DEST", *b"XPPA", *b"XRNK"];

/// Whether [`read_one`] would find nothing in the record, so it need not run: a readable
/// record of a [`BULK_SIGS`] type with none of the [`BULK_READ_SUBS`]. A new read of such a
/// type in [`read_one`] must add its subrecord to the list; a test gives [`read_one`] every
/// subrecord the schema knows for these types, so it catches one left out.
fn reads_nothing(r: &Record) -> bool {
    if !BULK_SIGS.contains(&r.header.sig) {
        return false;
    }
    let mut found = false;
    r.visit_subrecords(|sig, _| found |= BULK_READ_SUBS.contains(sig)).is_ok() && !found
}

/// The base form types whose OBND [`GameData::object_bounds`] keeps (GetObjectDimensions).
const BOUNDS_SIGS: [[u8; 4]; 20] = [
    *b"NPC_", *b"CREA", *b"CONT", *b"ACTI", *b"STAT", *b"MSTT", *b"FURN", *b"DOOR", *b"LIGH", *b"TERM", *b"MISC", *b"WEAP", *b"ARMO", *b"ALCH", *b"AMMO",
    *b"BOOK", *b"KEYM", *b"NOTE", *b"TACT", *b"IDLM",
];

/// The record types whose fields [`GameData::record_fields`] keeps.
pub const FIELD_SIGS: [[u8; 4]; 56] = [
    *b"PROJ", *b"EXPL", *b"AMMO", *b"WEAP", *b"ARMO", *b"LSCR", *b"NOTE", *b"PERK", *b"MESG", *b"TERM", *b"IMOD", *b"CHAL", *b"BOOK", *b"BPTD", *b"MISC", *b"ALCH", *b"KEYM",
    *b"NPC_", *b"CREA", *b"FACT", *b"RACE", *b"HAIR", *b"EYES", *b"SOUN", *b"WTHR", *b"IMGS", *b"IMAD", *b"AMEF", *b"CLMT", *b"CELL", *b"WRLD", *b"IPDS", *b"IPCT", *b"IDLE", *b"ACTI",
    // JIP batch 4 (`crate::nvse_jip4`).
    *b"CLAS", *b"CSTY", *b"LIGH", *b"GRAS", *b"TXST", *b"WATR",
    // JohnnyGuitar batch 1 (`crate::nvse_jg1`).
    *b"MUSC", *b"CONT", *b"LGTM",
    // JIP batch 5 (`crate::nvse_jip5`).
    *b"LTEX",
    // JohnnyGuitar batch 3 (`crate::nvse_jg3`).
    *b"EFSH", *b"MSET",
    // JIP batch 8 (`crate::nvse_jip8`): GetEffectFlag / SetEffectFlag read SPIT and ENIT flags.
    *b"SPEL", *b"ENCH",
    // Lutana batch 3 (`crate::nvse_lu3`): the Anim Object's idle, the loading screen type.
    *b"ANIO", *b"LSCT",
    // GetModelPath also reads these world objects' authored MODL. Keep the winning
    // record handles here; their fields are still decoded only when a getter asks.
    *b"STAT", *b"MSTT", *b"FURN", *b"DOOR", *b"TACT",
];

/// The winning records of the [`FIELD_SIGS`] types, each decoded by
/// [`numeric_fields`] the first time a getter asks for it and kept from then on (#1093:
/// decoding all ~68 000 up front cost 322 MB of the base game's and DLCs' GameData heap).
/// A record is a cheap handle on the plugin's mapped bytes, which live for the session.
/// A record's decoded subrecords, in record order.
pub type Fields = Vec<([u8; 4], Json)>;

pub struct RecordFields {
    schema: Schema,
    records: HashMap<FormId, (Record, OnceLock<Fields>)>,
}

impl RecordFields {
    fn new(schema: &Schema, records: impl IntoIterator<Item = (FormId, Record)>) -> RecordFields {
        RecordFields { schema: schema.clone(), records: records.into_iter().map(|(f, r)| (f, (r, OnceLock::new()))).collect() }
    }

    /// The decoded subrecords of `form`, in record order; `None` when it is not a
    /// winning record of a [`FIELD_SIGS`] type.
    pub fn get(&self, form: &FormId) -> Option<&Fields> {
        let (r, fields) = self.records.get(form)?;
        Some(fields.get_or_init(|| numeric_fields(&self.schema, r)))
    }

    /// How many records there are (decoded or not).
    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// The subrecords kept of an actor record, which are many: what xNVSE's actor-base
/// getters read.
///
/// CSCR (a creature's sounds template) and NIFZ (its model list): JIP batch 6
/// (`crate::nvse_jip6`). NAM4 (the impact material): JIP batch 7 (`crate::nvse_jip7`).
/// INAM (the death item), TPLT (the template), SPLO (the actor effects): JIP batch 8
/// (`crate::nvse_jip8`). FGGS, FGGA and FGTS (the FaceGen values): JohnnyGuitar batch 10
/// (`crate::nvse_jg10`).
const ACTOR_SUBS: [[u8; 4]; 25] = [
    *b"ACBS", *b"HNAM", *b"LNAM", *b"ENAM", *b"HCLR", *b"NAM6", *b"NAM7", *b"RNAM", *b"PNAM", *b"KFFZ", *b"VTCK", *b"DATA", *b"MODL", *b"TNAM", *b"BNAM", *b"WNAM", *b"CSCR", *b"NIFZ", *b"NAM4", *b"INAM", *b"TPLT", *b"SPLO",
    *b"FGGS", *b"FGGA", *b"FGTS",
];

/// A record's subrecords decoded by the schema, with each enum as its number (the
/// decoder names them) so a script getter can return it.
fn numeric_fields(schema: &Schema, r: &Record) -> Vec<([u8; 4], Json)> {
    let mut ctx = DecodeCtx { schema, on_formid: None };
    let mut out = Vec::new();
    let actor = matches!(&r.header.sig, b"NPC_" | b"CREA");
    for s in r.subrecords().unwrap_or_default() {
        if actor && !ACTOR_SUBS.contains(&s.sig) {
            continue;
        }
        let Some(mut v) = decode_sub(&r.header.sig, &s, &mut ctx).value else { continue };
        if let Some(def) = schema.lookup(&r.header.sig, &s.sig) {
            enums_to_numbers(schema, &def.ty, &mut v);
        }
        out.push((s.sig, v));
    }
    out
}

fn enums_to_numbers(schema: &Schema, t: &TypeDef, v: &mut Json) {
    match (schema.resolve(t), v) {
        (TypeDef::Enum { values, .. }, v @ Json::String(_)) => {
            let name = v.as_str().unwrap_or_default().to_string();
            if let Some((n, _)) = values.iter().find(|(_, s)| *s == name) {
                *v = Json::from(*n);
            }
        }
        (TypeDef::Struct(fields), Json::Object(o)) => {
            for f in fields {
                if let Some(x) = o.get_mut(&f.name) {
                    enums_to_numbers(schema, &f.ty, x);
                }
            }
        }
        (TypeDef::Array { elem, .. }, Json::Array(a)) => {
            for x in a {
                enums_to_numbers(schema, elem, x);
            }
        }
        _ => {}
    }
}

impl GameData {
    /// The name (BPTN) of the part of body-part data `form` whose type (BPND part_type)
    /// is `part`; "" for a part without a name, `None` when there is no such part. A
    /// part's subrecords start with BPTN or BPNN [documented: xEdit BPTD, schema
    /// combat.json].
    pub fn body_part_name(&self, form: FormId, part: i64) -> Option<String> {
        let subs = self.record_fields.get(&form)?;
        let mut name = String::new();
        let mut named = false;
        for (sig, v) in subs {
            match sig {
                b"BPTN" => {
                    name = v.as_str().unwrap_or_default().to_string();
                    named = true;
                }
                b"BPNN" if !named => name.clear(),
                b"BPND" => {
                    if v.get("part_type").and_then(Json::as_i64) == Some(part) {
                        return Some(name);
                    }
                    named = false;
                }
                _ => {}
            }
        }
        None
    }

    /// A faction's ranks (FACT RNAM, then its MNAM / FNAM titles), in record order.
    pub fn faction_ranks(&self, faction: FormId) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        for (sig, v) in self.record_fields.get(&faction).into_iter().flatten() {
            let text = || v.as_str().unwrap_or_default().to_string();
            match sig {
                b"RNAM" => out.push(Default::default()),
                b"MNAM" => out.last_mut().into_iter().for_each(|r| r.0 = text()),
                b"FNAM" => out.last_mut().into_iter().for_each(|r| r.1 = text()),
                _ => {}
            }
        }
        out
    }

    /// Every `sub` of a record in [`GameData::record_fields`], in record order (a
    /// repeated subrecord: AMMO RCIL, WEAP SNAM).
    pub fn record_all(&self, form: FormId, sub: &str) -> Vec<&Json> {
        self.record_fields.get(&form).into_iter().flatten().filter(|(s, _)| s == sub.as_bytes()).map(|(_, v)| v).collect()
    }

    /// A decoded field of a record in [`GameData::record_fields`]: subrecord `sub`, then
    /// `path` (field names split by `.`; empty for the whole subrecord). A flags field
    /// gives its number.
    pub fn record_field(&self, form: FormId, sub: &str, path: &str) -> Option<&Json> {
        let subs = self.record_fields.get(&form)?;
        let mut v = &subs.iter().find(|(s, _)| s == sub.as_bytes())?.1;
        for k in path.split('.').filter(|k| !k.is_empty()) {
            v = v.get(k)?;
        }
        Some(v.get("value").filter(|_| v.get("set").is_some()).unwrap_or(v))
    }
}

/// The time of each step of [`GameData::build`], one log line a step (`script build: …`,
/// #1732). The steps run one after another, so the lines add up to the build's time.
struct StepClock {
    last: std::time::Instant,
}

impl StepClock {
    fn new() -> StepClock {
        StepClock { last: std::time::Instant::now() }
    }

    /// Log the time since the last lap as `step`'s.
    fn lap(&mut self, step: &str) {
        let now = std::time::Instant::now();
        tracing::info!("script build: {step}: {:.0} ms", now.duration_since(self.last).as_secs_f64() * 1e3);
        self.last = now;
    }
}

impl GameData {
    /// Build from a load order. `functions` is the parsed `functions.json` (for the
    /// condition parameter types); `table` the function table built from it.
    pub fn build(lo: &LoadOrder, schema: &Schema, table: FunctionTable, functions: &Json, refs: &RefTable) -> GameData {
        Self::build_with(lo, schema, table, functions, refs, false)
    }

    /// Enable `hNVSE.ini` `[Main] bMergeRegionLists=1` merging and compute
    /// [`GameData::merged_cell_regions`] for overridden exterior `CELL`s.
    ///
    /// Called only at load time when `hNVSE.ini` enables merging, so unmodded vanilla
    /// and disabled loads pay zero startup cost [design].
    pub fn enable_hnvse_region_merge(&mut self, lo: &LoadOrder, schema: &Schema) {
        self.hnvse_merge_region_lists = true;
        self.merged_cell_regions = lo
            .index
            .iter()
            .filter_map(|(&g, defs)| {
                if defs.len() <= 1 {
                    return None;
                }
                let last = defs.last()?;
                let win = lo.records[last.plugin as usize][last.ordinal as usize];
                if &win.header.sig != b"CELL" {
                    return None;
                }
                let regions = fnv_world::audio::cell_regions_merged(lo, schema, g, true);
                Some((FormId(g), regions.into_iter().map(FormId).collect()))
            })
            .collect();
    }

    /// Read `Data\NVSE\Plugins\hNVSE.ini` bytes once at load time and, only if
    /// `[Main] bMergeRegionLists=1` is enabled, populate [`GameData::merged_cell_regions`].
    pub fn load_hnvse_region_merge(&mut self, lo: &LoadOrder, schema: &Schema, ini_bytes: Option<&[u8]>) -> bool {
        let enabled = ini_bytes
            .is_some_and(|bytes| fnv_world::audio::parse_hnvse_merge_region_lists(&String::from_utf8_lossy(bytes)));
        if enabled {
            self.enable_hnvse_region_merge(lo, schema);
        }
        enabled
    }

    /// [`GameData::build`] without its shortcuts, for a test to compare with it
    /// (tests/build_shortcuts.rs, #1732): every record is read and its subrecords copied, the
    /// source compile runs and is put in before the records are read, and units found by
    /// label are checked against those found by owner. Not for the game: it is slower.
    #[doc(hidden)]
    pub fn build_the_long_way(lo: &LoadOrder, schema: &Schema, table: FunctionTable, functions: &Json, refs: &RefTable) -> GameData {
        Self::build_with(lo, schema, table, functions, refs, true)
    }

    fn build_with(lo: &LoadOrder, schema: &Schema, table: FunctionTable, functions: &Json, refs: &RefTable, long_way: bool) -> GameData {
        let build_start = std::time::Instant::now();
        let mut clock = StepClock::new();
        let param_types = ParamTypes::from_catalogue(functions);
        clock.lap("condition parameter types");
        let winners: Vec<(u32, usize, &Record)> = lo
            .index
            .iter()
            .filter_map(|(&g, defs)| {
                let d = defs.last()?;
                Some((g, d.plugin as usize, lo.records[d.plugin as usize][d.ordinal as usize]))
            })
            .collect();
        // The records with script units are read first (their units are decoded before the
        // rest is taken in); every other record is read a slice at a time as the loop below
        // takes it in. A `Read` is about 3.8 KB: read all at once, the winning records held
        // some GB until the build ended, at every start and in both runs of a load while
        // playing (#1357). Most of them hold nothing and take no memory ([`reads_nothing`]).
        let mut winners = winners;
        // Each form is listed once, so any sort gives this one order.
        winners.par_sort_unstable_by_key(|w| w.0);
        clock.lap(&format!("{} winning records listed and sorted", winners.len()));
        let mut unit_reads: Vec<(u32, Box<Read>)> = winners
            .par_iter()
            .filter(|w| if long_way { carries_units_copying(w.2) } else { carries_units(w.2) })
            .map(|&(g, pi, r)| (g, Box::new(read_one(lo, schema, &param_types, g, pi, r))))
            .collect();
        clock.lap(&format!("{} records with script units read", unit_reads.len()));
        let item_order: HashMap<FormId, (u8, u32)> = lo
            .index
            .iter()
            .filter_map(|(&g, defs)| {
                let first = defs.first()?;
                let r = lo.records[first.plugin as usize][first.ordinal as usize];
                crate::functions::items::form_type_code(&r.header.sig).map(|_| (FormId(g), (first.plugin, first.ordinal)))
            })
            .collect();
        clock.lap("item order");
        let actor_order: HashMap<FormId, (u8, u32)> = lo
            .index
            .iter()
            .filter_map(|(&g, defs)| {
                let first = defs.first()?;
                let r = lo.records[first.plugin as usize][first.ordinal as usize];
                matches!(&r.header.sig, b"NPC_" | b"CREA").then_some((FormId(g), (first.plugin, first.ordinal)))
            })
            .collect();
        let object_bounds: HashMap<FormId, [i16; 6]> = winners
            .par_iter()
            .filter(|(_, _, r)| BOUNDS_SIGS.contains(&r.header.sig))
            .filter_map(|&(g, _, r)| Some((FormId(g), if long_way { bounds_of_copying(r) } else { bounds_of(r) }?)))
            .collect();
        clock.lap("object bounds");
        let form_flags: HashMap<FormId, u32> = winners.iter().filter(|(_, _, r)| r.header.flags != 0).map(|&(g, _, r)| (FormId(g), r.header.flags)).collect();
        let record_fields = RecordFields::new(schema, winners.iter().filter(|(_, _, r)| FIELD_SIGS.contains(&r.header.sig)).map(|&(g, _, r)| (FormId(g), r.clone())));
        let station_flags: HashMap<FormId, u32> = winners.iter().filter(|w| &w.2.header.sig == b"TACT").map(|w| (FormId(w.0), w.2.header.flags)).collect();
        let talking_voices: HashMap<FormId, FormId> = winners
            .iter()
            .filter(|w| &w.2.header.sig == b"TACT")
            .filter_map(|&(g, pi, r)| {
                let mut local = None;
                r.visit_subrecords(|sig, data| {
                    if sig == b"VNAM" && local.is_none() {
                        local = data.get(0..4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
                    }
                })
                .ok()?;
                Some((FormId(g), FormId(lo.to_global(pi, local.filter(|&l| l != 0)?)?)))
            })
            .collect();
        clock.lap("form flags, record field handles");
        let radiation_markers = crate::radiation::markers(refs);
        clock.lap("radiation markers");
        let mut d = GameData {
            table,
            param_types: param_types.clone(),
            load_order: lo.plugins.iter().map(|p| p.name.clone()).collect(),
            form_overrides: lo.index.iter().filter(|(_, d)| d.len() > 1).map(|(&id, d)| (FormId(id), d.iter().map(|x| x.plugin).collect())).collect(),
            plugin_versions: lo.plugins.iter().map(|p| p.header.version).collect(),
            plugin_texts: lo.plugins.iter().map(|p| (p.header.author.clone(), p.header.description.clone())).collect(),
            units: Vec::new(),
            unit_by_key: HashMap::new(),
            scripts: HashMap::new(),
            quests: BTreeMap::new(),
            infos: HashMap::new(),
            topics: HashMap::new(),
            lines_moved_by_previous: 0,
            shared_lines: HashMap::new(),
            linked_from: HashMap::new(),
            follow_up_targets: HashSet::new(),
            topic_defs: HashMap::new(),
            top_level: Vec::new(),
            greeting: None,
            goodbye: None,
            speech_challenge_failure: None,
            packages: HashMap::new(),
            patrol_scripts: HashMap::new(),
            package_defs: HashMap::new(),
            messages: HashMap::new(),
            terminals: HashMap::new(),
            notes: HashMap::new(),
            form_lists: HashMap::new(),
            repair_lists: HashMap::new(),
            menu_icons: HashMap::new(),
            actors: HashMap::new(),
            combat_styles: HashMap::new(),
            leveled_actors: HashMap::new(),
            form_script: HashMap::new(),
            edids: HashMap::new(),
            edid_of: HashMap::new(),
            item_order,
            actor_order,
            object_bounds,
            form_flags,
            full_names: HashMap::new(),
            sigs: HashMap::new(),
            forms_by_type_cache: std::sync::RwLock::new(HashMap::new()),
            ini_texts: Vec::new(),
            record_fields,
            scripted_by_cell: BTreeMap::new(),
            template_scripted_by_cell: BTreeMap::new(),
            reputations: HashMap::new(),
            reputation_names: HashMap::new(),
            challenges: HashMap::new(),
            challenge_thresholds: HashMap::new(),
            challenge_defs: HashMap::new(),
            regions: HashMap::new(),
            magic_effects: HashMap::new(),
            effect_items: HashMap::new(),
            ingestibles: HashMap::new(),
            spell_types: HashMap::new(),
            spell_flags: HashMap::new(),
            race_spells: HashMap::new(),
            voices_without_default: Default::default(),
            armor_enchantments: HashMap::new(),
            enchantment_types: HashMap::new(),
            child_races: HashSet::new(),
            race_older: HashMap::new(),
            race_younger: HashMap::new(),
            faction_reactions: HashMap::new(),
            faction_modifiers: HashMap::new(),
            faction_flags: HashMap::new(),
            faction_reputations: HashMap::new(),
            cells: HashMap::new(),
            hnvse_merge_region_lists: false,
            merged_cell_regions: HashMap::new(),
            owner_ranks: HashMap::new(),
            destructibles: HashMap::new(),
            weapon_skills: HashMap::new(),
            weapon_anim_types: HashMap::new(),
            furniture: HashMap::new(),
            idle_markers: HashMap::new(),
            idles: HashMap::new(),
            idle_children: HashMap::new(),
            image_modifiers: HashMap::new(),
            station_flags,
            talking_voices,
            perks: HashMap::new(),
            actor_value_infos: HashMap::new(),
            recipes: HashMap::new(),
            camera_paths: HashMap::new(),
            camera_shots: HashMap::new(),
            recipe_categories: HashMap::new(),
            casinos: HashMap::new(),
            caravan_cards: HashMap::new(),
            caravan_decks: HashMap::new(),
            need_stages: HashMap::new(),
            water_effects: HashMap::new(),
            drink_waters: HashMap::new(),
            radio_receivers: HashSet::new(),
            default_objects: Vec::new(),
            water_damage: HashMap::new(),
            radiation_markers,
            class_services: HashMap::new(),
            issues: Issues::default(),
            read_issues: ReadIssues::default(),
            source_units: Vec::new(),
        };
        // Decode every unit once.
        let mut all_units: Vec<ScriptUnit> = Vec::new();
        for (_, r) in &mut unit_reads {
            all_units.append(&mut r.units);
        }
        let unit_count = all_units.len();
        let compiled: Vec<Compiled> = all_units
            .into_par_iter()
            .map(|unit| {
                let dec = decode(&unit.bytes, &d.table);
                let raw = dec.report.raw_total();
                Compiled::new(unit, dec.program, raw)
            })
            .collect();
        clock.lap(&format!("{unit_count} units decoded"));
        // By owner, not by [`UnitOwner::label`]: two owners have one label only when they are
        // equal [design: the label prints every field], so a lookup finds the same unit
        // without writing out a label for every record.
        let mut unit_of: HashMap<UnitOwner, UnitId> = HashMap::with_capacity(compiled.len());
        d.unit_by_key.reserve(compiled.len());
        let mut units: Vec<Compiled> = Vec::with_capacity(compiled.len());
        for c in compiled {
            unit_of.insert(c.unit.owner.clone(), units.len());
            d.unit_by_key.entry(c.key()).or_insert(units.len());
            units.push(c);
        }
        if long_way {
            let by_label: HashMap<String, UnitId> = unit_of.iter().map(|(o, &u)| (o.label(), u)).collect();
            assert_eq!(by_label.len(), unit_of.len(), "two owners with one label");
            for (i, c) in units.iter().enumerate() {
                assert_eq!(by_label.get(&c.unit.owner.label()), unit_of.get(&c.unit.owner), "unit {i}");
            }
        }
        clock.lap("units indexed");
        d.sigs.reserve(winners.len());
        // The units picked for it compile while the records are read: the loop below reads
        // of a unit only what `source_compile::apply` leaves alone.
        let table = std::mem::take(&mut d.table);
        if long_way {
            let recompiled = crate::source_compile::compile_picked(lo, schema, &table, &units);
            d.source_units.extend(crate::source_compile::apply(&mut units, recompiled, &mut d.issues));
        }
        let recompiled = std::thread::scope(|scope| {
            let compile = scope.spawn(|| if long_way { crate::source_compile::Recompiled::none() } else { crate::source_compile::compile_picked(lo, schema, &table, &units) });
            // In form order, as one list read all at once was.
            let mut unit_reads: HashMap<u32, Box<Read>> = unit_reads.into_iter().collect();
            // The reads run a slice at a time inside the loop below: their time is summed here,
            // and the rest of the loop's is the taking in.
            let read_time = std::cell::Cell::new(std::time::Duration::ZERO);
            let move_time = std::cell::Cell::new(std::time::Duration::ZERO);
            let reads = winners.chunks(READ_SLICE).flat_map(|slice| {
                let read_start = std::time::Instant::now();
                let read: Vec<Option<Box<Read>>> = slice
                    .par_iter()
                    .map(|&(g, pi, r)| {
                        if unit_reads.contains_key(&g) {
                            return None;
                        }
                        (long_way || !reads_nothing(r)).then(|| Box::new(read_one(lo, schema, &param_types, g, pi, r)))
                    })
                    .collect();
                read_time.set(read_time.get() + read_start.elapsed());
                let move_start = std::time::Instant::now();
                // `None`: nothing read ([`reads_nothing`]).
                let taken: Vec<(FormId, [u8; 4], Option<Box<Read>>)> =
                    slice.iter().zip(read).map(|(&(g, _, r), read)| (FormId(g), r.header.sig, read.or_else(|| unit_reads.remove(&g)))).collect();
                move_time.set(move_time.get() + move_start.elapsed());
                taken
            });
            for (id, sig, r) in reads {
                d.sigs.insert(id, sig);
                let Some(r) = r else { continue };
                if let Some(e) = r.edid {
                    d.edids.entry(e.to_ascii_lowercase()).or_insert(id);
                    d.edid_of.insert(id, e);
                }
                if let Some(n) = r.full {
                    d.full_names.insert(id, n);
                }
                if let Some(s) = r.script {
                    d.form_script.insert(id, s);
                }
                d.issues.merge(r.issues);
                d.read_issues.merge(r.read_issues);
                match &sig {
                    b"SCPT" => {
                        // SCHR flag "Enabled" clear: the script does not run [documented: xEdit
                        // SCHR flags; hypothesis: what a disabled script does, logic-fields.md
                        // §2]. One master script has it clear (l38upgradeGunRack01SCRIPT).
                        if let Some(&u) = unit_of.get(&UnitOwner::Script { form: id.0 }).filter(|&&u| units[u].unit.header.flags & 1 != 0) {
                            d.scripts.insert(id, u);
                        }
                    }
                    b"QUST" => {
                        if let Some(mut q) = r.quest {
                            for (stage, entries) in q.stages.iter_mut() {
                                for (n, e) in entries.iter_mut().enumerate() {
                                    e.unit = unit_of
                                        .get(&UnitOwner::QuestStage { form: id.0, stage: *stage as i64, entry: n as u32 })
                                        .copied()
                                        .filter(|&u| units[u].unit.has_scda);
                                }
                            }
                            d.quests.insert(id, q);
                        }
                    }
                    b"INFO" => {
                        let mut info = r.info.unwrap_or_default();
                        let unit = |end| {
                            unit_of.get(&UnitOwner::Info { form: id.0, end }).copied().filter(|&u| units[u].unit.has_scda)
                        };
                        info.begin = unit(false);
                        info.end = unit(true);
                        d.infos.insert(id, info);
                    }
                    b"REFR" => {
                        if let Some(&u) = unit_of.get(&UnitOwner::Patrol { form: id.0 }).filter(|&&u| units[u].unit.has_scda) {
                            d.patrol_scripts.insert(id, u);
                        }
                    }
                    b"PACK" => {
                        use fnv_script::unit::PackageEvent as P;
                        let unit = |event| {
                            unit_of.get(&UnitOwner::Package { form: id.0, event }).copied().filter(|&u| units[u].unit.has_scda)
                        };
                        let p = [unit(P::Begin), unit(P::End), unit(P::Change)];
                        if p.iter().any(Option::is_some) {
                            d.packages.insert(id, p);
                        }
                    }
                    b"TERM" => {
                        if let Some(mut t) = r.terminal {
                            for (n, item) in t.items.iter_mut().enumerate() {
                                item.unit = unit_of
                                    .get(&UnitOwner::Terminal { form: id.0, item: n as u32 })
                                    .copied()
                                    .filter(|&u| units[u].unit.has_scda);
                            }
                            d.terminals.insert(id, t);
                        }
                    }
                    _ => {}
                }
                if let Some(mut t) = r.topic {
                    t.form = id;
                    d.topic_defs.insert(id, t);
                }
                if let Some(n) = r.note {
                    d.notes.insert(id, n);
                }
                if let Some(m) = r.message {
                    d.messages.insert(id, m);
                }
                if let Some(l) = r.form_list {
                    d.form_lists.insert(id, l);
                }
                if let Some(i) = r.menu_icon {
                    d.menu_icons.insert(id, i);
                }
                if let Some(a) = r.actor {
                    d.actors.insert(id, a);
                }
                if let Some(c) = r.combat_style {
                    d.combat_styles.insert(id, c);
                }
                if let Some(l) = r.leveled_actor {
                    d.leveled_actors.insert(id, l);
                }
                if let Some(p) = r.package {
                    d.package_defs.insert(id, p);
                }
                if let Some(v) = r.reputation {
                    d.reputations.insert(id, v);
                }
                if let Some(v) = r.reputation_name {
                    d.reputation_names.insert(id, v);
                }
                if let Some(mut c) = r.challenge {
                    c.form = id;
                    d.challenges.insert(id, c.flags);
                    d.challenge_thresholds.insert(id, c.threshold);
                    d.challenge_defs.insert(id, c);
                }
                if let Some(v) = r.region {
                    d.regions.insert(id, v);
                }
                if let Some(v) = r.magic_effect {
                    d.magic_effects.insert(id, v);
                }
                if let Some(v) = r.effects {
                    d.effect_items.insert(id, v);
                }
                if let Some(v) = r.ingestible {
                    d.ingestibles.insert(id, v);
                }
                if let Some(v) = r.spell_type {
                    d.spell_types.insert(id, v);
                }
                if let Some(v) = r.spell_flags.filter(|&v| v != 0) {
                    d.spell_flags.insert(id, v);
                }
                if let Some(v) = r.race_spells.filter(|v| !v.is_empty()) {
                    d.race_spells.insert(id, v);
                }
                if r.voice_default == Some(false) {
                    d.voices_without_default.insert(id);
                }
                if let Some(v) = r.object_effect {
                    d.armor_enchantments.insert(id, v);
                }
                if let Some(v) = r.repair_list {
                    d.repair_lists.insert(id, v);
                }
                if let Some(v) = r.enchantment_type {
                    d.enchantment_types.insert(id, v);
                }
                if r.child_race {
                    d.child_races.insert(id);
                }
                if let Some(v) = r.race_older {
                    d.race_older.insert(id, v);
                }
                if let Some(v) = r.race_younger {
                    d.race_younger.insert(id, v);
                }
                if let Some(v) = r.faction {
                    d.faction_reactions.insert(id, v);
                }
                if let Some(v) = r.faction_modifiers {
                    d.faction_modifiers.insert(id, v);
                }
                if let Some(v) = r.destruction {
                    d.destructibles.insert(id, v);
                }
                if let Some(v) = r.weapon_skill {
                    d.weapon_skills.insert(id, v);
                }
                if let Some(v) = r.weapon_anim_type {
                    d.weapon_anim_types.insert(id, v);
                }
                if let Some(v) = r.furniture {
                    d.furniture.insert(id, v);
                }
                if let Some(v) = r.idle_marker {
                    d.idle_markers.insert(id, v);
                }
                if let Some(v) = r.idle {
                    d.idles.insert(id, v);
                }
                if let Some(v) = r.image_modifier {
                    d.image_modifiers.insert(id, v);
                }
                if let Some(mut v) = r.perk {
                    // An entry's script (an activate choice's, `crate::activate_choices`).
                    for e in &mut v.entries {
                        if let PerkEffect::EntryPoint(ep) = &mut e.effect {
                            ep.unit = unit_of.get(&UnitOwner::Perk { form: id.0, entry: e.ordinal }).copied().filter(|&u| units[u].unit.has_scda);
                        }
                    }
                    d.perks.insert(id, v);
                }
                if let Some(v) = r.av_info {
                    d.actor_value_infos.insert(id, v);
                }
                if let Some(v) = r.recipe {
                    d.recipes.insert(id, v);
                }
                if let Some(v) = r.camera_path {
                    d.camera_paths.insert(id, v);
                }
                if let Some(v) = r.camera_shot {
                    d.camera_shots.insert(id, v);
                }
                if let Some(v) = r.recipe_category {
                    d.recipe_categories.insert(id, v);
                }
                if let Some(v) = r.casino {
                    d.casinos.insert(id, v);
                }
                if let Some(v) = r.class_services {
                    d.class_services.insert(id, v);
                }
                if let Some(v) = r.caravan_card {
                    d.caravan_cards.insert(id, v);
                }
                if let Some(v) = r.caravan_deck {
                    d.caravan_decks.insert(id, v);
                }
                if let Some(v) = r.faction_flags {
                    d.faction_flags.insert(id, v);
                }
                if let Some(v) = r.faction_reputation {
                    d.faction_reputations.insert(id, v);
                }
                if let Some(v) = r.cell.filter(crate::crime::CellDef::matters) {
                    d.cells.insert(id, v);
                }
                if let Some(v) = r.owner_rank {
                    d.owner_ranks.insert(id, v);
                }
                if let Some((code, threshold, spell)) = r.need_stage {
                    d.need_stages.entry(code).or_default().push(NeedStage { stage: id, threshold, spell });
                }
                if let Some(spell) = r.water_effect {
                    d.water_effects.insert(id, spell);
                }
                if let Some(water) = r.drink_water {
                    d.drink_waters.insert(id, water);
                }
                if r.radio_receiver {
                    d.radio_receivers.insert(id);
                }
                if let Some(list) = r.default_objects {
                    d.default_objects = list;
                }
                let damage = r.water_damage;
                match damage {
                    Some(n) if n > 0 => {
                        d.water_damage.insert(id, n as f64);
                    }
                    Some(_) => {
                        d.water_damage.remove(&id);
                    }
                    None => {}
                }
            }
            {
                let read_ms = read_time.get().as_secs_f64() * 1e3;
                let all_ms = clock.last.elapsed().as_secs_f64() * 1e3;
                tracing::info!("script build: other records read: {read_ms:.0} ms (a slice at a time, {READ_SLICE} records)");
                let move_ms = move_time.get().as_secs_f64() * 1e3;
                tracing::info!("script build: records moved out of the slices: {move_ms:.0} ms");
                tracing::info!("script build: records taken in: {:.0} ms", all_ms - read_ms - move_ms);
                clock.last = std::time::Instant::now();
            }
            let recompiled = compile.join().unwrap_or_else(|p| std::panic::resume_unwind(p));
            clock.lap("waited for the source compile");
            recompiled
        });
        d.table = table;
        d.source_units.extend(crate::source_compile::apply(&mut units, recompiled, &mut d.issues));
        d.units = units;
        clock.lap("source compile results put in");
        for stages in d.need_stages.values_mut() {
            stages.sort_by_key(|s| (s.threshold, s.stage));
        }
        d.idle_children = idle_children(&d.idles);
        clock.lap("need stages, idle children");
        let (mut lines, order, first_plugin) = topic_lines(lo);
        clock.lap("dialogue lines listed");
        for list in lines.values_mut() {
            d.lines_moved_by_previous += follow_previous(list, &first_plugin, &d.infos);
        }
        d.topics = lines;
        for (topic, lines) in &d.topics {
            for i in lines {
                if let Some(info) = d.infos.get_mut(i) {
                    info.topic = Some(*topic);
                }
            }
        }
        for (id, t) in d.topic_defs.iter_mut() {
            t.order = order.get(id).copied().unwrap_or((usize::MAX, 0));
        }
        d.shared_lines = shared_lines(&d.topics, &d.topic_defs, &d.infos);
        d.linked_from = linked_from(&d.topic_defs, &d.infos);
        d.follow_up_targets = d.infos.values().flat_map(|i| i.follow_ups.iter().copied()).collect();
        let mut top: Vec<&TopicDef> = d.topic_defs.values().filter(|t| t.top_level() && t.topic_type == topic_type::TOPIC).collect();
        top.sort_by(|a, b| b.priority.total_cmp(&a.priority).then(a.order.cmp(&b.order)));
        d.top_level = top.into_iter().map(|t| t.form).collect();
        d.greeting = d.by_edid("GREETING").filter(|t| d.topic_defs.contains_key(t));
        d.goodbye = d.by_edid("GOODBYE").filter(|t| d.topic_defs.contains_key(t));
        d.speech_challenge_failure = d.by_edid(crate::dialogue::SPEECH_CHALLENGE_FAILURE).filter(|t| d.topic_defs.contains_key(t));
        clock.lap("topics ordered and linked");
        // An actor whose base takes its script from a template (template flag "Script")
        // runs the template's script, its leveled pick's through a list [documented: GECK
        // wiki Template Data, "Use Script: Uses the Script of the ActorBase";
        // `Game::ref_script`]. Those whose base has no script of its own are kept apart:
        // they get OnLoad and OnReset, so the leveled spawns' placeholder
        // `VCrILikeOblivionChancedSpawns` kills itself as its cell loads
        // (docs/spec-notes/dead-on-spawn.md §4.1, #3228).
        let mut through_template: HashMap<FormId, bool> = HashMap::new();
        for (id, p) in &refs.refs {
            let Some(cell) = p.cell else { continue };
            if d.script_of_base(p.base).is_some() {
                d.scripted_by_cell.entry(cell).or_default().push(*id);
            } else if *through_template.entry(p.base).or_insert_with(|| d.script_through_template(p.base)) {
                d.template_scripted_by_cell.entry(cell).or_default().push(*id);
            }
        }
        clock.lap("scripted references by cell");
        tracing::info!("script build: GameData::build: {:.0} ms", build_start.elapsed().as_secs_f64() * 1e3);
        d
    }

    pub fn unit(&self, id: UnitId) -> &Compiled {
        &self.units[id]
    }

    /// The SCPT a base form or quest runs, if it is a compiled script.
    pub fn script_of_base(&self, base: FormId) -> Option<FormId> {
        let s = *self.form_script.get(&base)?;
        self.scripts.contains_key(&s).then_some(s)
    }

    /// Whether a reference placed with `base`, which has no script of its own, may run
    /// one through its template chain: `base` is a leveled actor list placed directly, or
    /// an actor with the template flag "Script", and an actor its chain can reach with
    /// that flag through any leveled pick has a script (an end of the chain, or one on
    /// the way that is kept when its list yields no entry;
    /// [`GameData::template_chain_any_pick`]). `Game::ref_script` reads the actual pick.
    pub fn script_through_template(&self, base: FormId) -> bool {
        let templated = self.actors.get(&base).is_some_and(|a| a.template_script && a.template.is_some());
        if !templated && !self.leveled_actors.contains_key(&base) {
            return false;
        }
        self.template_chain_any_pick(base, |a| a.template_script).into_iter().any(|s| s != base && self.script_of_base(s).is_some())
    }

    pub fn script(&self, script: FormId) -> Option<&Compiled> {
        self.scripts.get(&script).map(|&u| &self.units[u])
    }

    /// The file names of the plugins that hold `form`, defining it or overriding it, in
    /// load order; none for a form of no plugin (made in play).
    pub fn form_plugins(&self, form: FormId) -> Vec<String> {
        let idx: Vec<u8> = match self.form_overrides.get(&form) {
            Some(v) => v.clone(),
            None if self.sig(form).is_some() => vec![(form.0 >> 24) as u8],
            None => Vec::new(),
        };
        idx.into_iter().filter_map(|i| self.load_order.get(usize::from(i)).cloned()).collect()
    }

    /// Whether `form` is overridden by at least one plugin after its defining plugin in
    /// the load order.
    pub fn is_form_overridden(&self, form: FormId) -> bool {
        self.form_overrides.get(&form).is_some_and(|v| v.len() > 1)
    }

    /// Whether `form` is overridden by a plugin loaded after `caller_plugin`
    /// (`IsFormOverridden` [documented: GECK wiki `IsFormOverridden`, "overridden by a
    /// plugin lower in the load order than the calling script"]).
    ///
    /// Returns `false` when `caller_plugin` is empty or not in `load_order` (runtime
    /// scripts such as console, `CompileScript`, or Script Runner have no later plugin in
    /// the load order [observed: `jg_01_form_override_index_edge`, `GetFormOverrideIndex`
    /// `5` with `IsFormOverridden` `0`]).
    pub fn is_form_overridden_after(&self, form: FormId, caller_plugin: &str) -> bool {
        if caller_plugin.is_empty() {
            return false;
        }
        let Some(caller_idx) = self.load_order.iter().position(|l| l.eq_ignore_ascii_case(caller_plugin)) else {
            return false;
        };
        self.form_overrides.get(&form).is_some_and(|v| v.len() > 1 && v.last().copied().is_some_and(|win| usize::from(win) > caller_idx))
    }

    /// The load-order index of the winning (last) plugin that defines or overrides
    /// `form` (`GetFormOverrideIndex`), or `None` when `form` is not known from any
    /// plugin in `GameData`.
    pub fn form_override_index(&self, form: FormId) -> Option<u8> {
        if let Some(last) = self.form_overrides.get(&form).and_then(|v| v.last().copied()) {
            return Some(last);
        }
        if (form.0 >> 24) < 0xFE && self.sig(form).is_some() {
            return Some((form.0 >> 24) as u8);
        }
        None
    }

    pub fn by_edid(&self, edid: &str) -> Option<FormId> {
        self.edids.get(&edid.to_ascii_lowercase()).copied()
    }

    /// The AVIF record of a S.P.E.C.I.A.L. or skill code. The masters' AVIF FormIDs run
    /// in code order: 0x3E8–0x3EE for Strength to Luck, 0x4B0–0x4BD for Barter to
    /// Unarmed [observed: each record's name is its code's, Guns under the Editor ID
    /// AVSmallGuns and Survival under AVThrowing, docs/spec-notes/chargen.md §2].
    pub fn actor_value_info(&self, code: u16) -> Option<&ActorValueInfo> {
        use crate::actor_values::{BARTER, SKILLS, SPECIALS, STRENGTH};
        let id = if SPECIALS.contains(&code) {
            0x3E8 + u32::from(code - STRENGTH)
        } else if SKILLS.contains(&code) {
            0x4B0 + u32::from(code - BARTER)
        } else {
            return None;
        };
        self.actor_value_infos.get(&FormId(id))
    }

    pub fn name(&self, id: FormId) -> String {
        match self.edid_of.get(&id) {
            Some(e) => e.clone(),
            None => id.to_string(),
        }
    }

    /// The loaded forms whose type has that form type code
    /// (`functions::items::form_type_code`), in the game's order: the forms below 0x800
    /// first, highest first, then each plugin's records in file order, in load order;
    /// forms without a place in a file last [observed: glta-order, GasTrapDummy 1F6 then
    /// Fists 1F4, then FalloutNV.esm's WEAP records as the file has them, the last
    /// plugin's last; hypothesis: the low forms' order].
    pub fn forms_of_type(&self, code: i32) -> Vec<FormId> {
        if let Ok(cache) = self.forms_by_type_cache.read() {
            if let Some(hit) = cache.get(&code) {
                return hit.clone();
            }
        }
        let mut out: Vec<FormId> = self.sigs.iter().filter(|(_, s)| crate::functions::items::form_type_code(s) == Some(code)).map(|(&f, _)| f).collect();
        out.sort_by_key(|f| {
            if f.0 < 0x800 {
                (0u8, u8::MAX - (f.0 >> 8) as u8, u64::from(u32::MAX - f.0))
            } else {
                match self.item_order.get(f) {
                    Some(&(p, o)) => (1, p, u64::from(o)),
                    None => (2, 0, u64::from(f.0)),
                }
            }
        });
        if let Ok(mut cache) = self.forms_by_type_cache.write() {
            cache.insert(code, out.clone());
        }
        out
    }

    pub fn sig(&self, id: FormId) -> Option<[u8; 4]> {
        self.sigs.get(&id).copied()
    }

    pub fn is_quest(&self, id: FormId) -> bool {
        self.quests.contains_key(&id)
    }

    /// The AI package list of `reference`, placed with `base` (NPC_, CREA, or a leveled
    /// actor list): the `PKID` list of the "AI Packages" source of its template chain,
    /// through leveled lists with the same pick as the drawn actor
    /// ([`GameData::template_source`]; [`crate::game::Game::package_list`]). For a base
    /// alone, pass the base as the reference.
    pub fn base_package_list(&self, base: FormId, reference: FormId) -> Vec<FormId> {
        self.base_package_list_at(base, reference, fnv_sim::zones::LevelAt::plain(crate::templates::LEVELED_ACTOR_LEVEL))
    }

    /// [`GameData::base_package_list`] with leveled lists read at `at` (the reference's
    /// level in a game).
    pub fn base_package_list_at(&self, base: FormId, reference: FormId, at: fnv_sim::zones::LevelAt) -> Vec<FormId> {
        self.template_source_at(base, reference, at, |d| d.template_packages).map(|(_, d)| d.packages.clone()).unwrap_or_default()
    }

    /// The priority of the quest a line belongs to (0 without a quest).
    pub fn line_priority(&self, info: FormId) -> u8 {
        self.infos.get(&info).and_then(|d| d.quest).and_then(|q| self.quests.get(&q)).map_or(0, |q| q.priority)
    }

    /// A topic's lines in the order they are tried: lines of higher-priority quests
    /// first, and within one priority the stored order ([`GameData::topics`]); a stable
    /// sort, so lines of one quest stay top to bottom [documented: GECK wiki Quest Data
    /// Tab, priority; owner statement 2026-10-10 for equal priorities and for the order
    /// inside a quest; docs/spec-notes/dialogue.md §4].
    pub fn lines_by_priority(&self, topic: FormId) -> Vec<FormId> {
        let mut lines: Vec<FormId> = self.topics.get(&topic).cloned().unwrap_or_default();
        lines.sort_by_key(|i| std::cmp::Reverse(self.line_priority(*i)));
        lines
    }

    /// The lines an actor can say for `topic`, in the order they are tried: its own lines
    /// and the lines it shares from other topics (INFC / INFX, [`GameData::shared_lines`])
    /// [observed: Red Lucy's choice "What's this place?" (VMS38VMS38RedLucyTopic047) has
    /// no line of its own and shares the answer of the topic of the same name; of the 257
    /// player lines whose choices all lack lines of their own, 250 get a choice with
    /// lines this way, dialogue.md §1.2], sorted as [`GameData::lines_by_priority`].
    pub fn lines_tried(&self, topic: FormId) -> Vec<FormId> {
        let Some(merged) = self.shared_lines.get(&topic) else { return self.lines_by_priority(topic) };
        let mut lines = merged.clone();
        lines.sort_by_key(|i| std::cmp::Reverse(self.line_priority(*i)));
        lines
    }

    /// The text the player sees for a topic whose line would be `info`: the line's
    /// prompt, else the topic's name, else nothing [documented: GECK wiki
    /// Category:Dialogue, "Prompt"].
    pub fn choice_text(&self, topic: FormId, info: FormId) -> &str {
        match self.infos.get(&info).and_then(|d| d.prompt.as_deref()) {
            Some(p) => p,
            None => self.topic_defs.get(&topic).and_then(|t| t.name.as_deref()).unwrap_or(""),
        }
    }
}

/// [`GameData::shared_lines`]: for each topic whose INFC / INFX pairs name a line, its
/// own lines in stored order with each shared line put in among the own lines of the
/// quest that lists it (the QSTI the pair follows) [hypothesis: the Info Index is the
/// line's place in that quest's lines of the topic, dialogue.md §1.2].
///
/// [design] Each listing quest's list is its own lines of the topic, in stored order,
/// with its shared lines inserted at their Info Index (pairs in index order; a negative,
/// missing or past-the-end index puts the line last). In the topic's list a shared line
/// then goes just before the quest's own line that follows it in that list, or right
/// after the quest's last own line when none follows; lines that meet at one place keep
/// index order, then QSTI order. The own lines never move, so equal priorities keep the
/// stored order as in [`GameData::lines_by_priority`] (dialogue.md §4). A quest with no
/// own line in the topic has its shared lines appended after the own lines, quests in
/// QSTI order. A line already in the topic is not added twice (the first pair in index
/// order wins); missing INFOs are skipped. A topic with a valid pair under no QSTI keeps
/// the topic-wide merge (the index as a place in the whole list). Maps here are lookups
/// only: the order comes from the stored lines, the QSTI list and sorted keys.
fn shared_lines(topics: &HashMap<FormId, Vec<FormId>>, defs: &HashMap<FormId, TopicDef>, infos: &HashMap<FormId, InfoDef>) -> HashMap<FormId, Vec<FormId>> {
    let mut out = HashMap::new();
    for (id, t) in defs {
        let mut pairs: Vec<_> = t.shared_infos.iter().filter(|s| s.info.is_some_and(|i| infos.contains_key(&i))).collect();
        if pairs.is_empty() {
            continue;
        }
        let index_order = |s: &SharedInfo| s.index.filter(|&x| x >= 0).unwrap_or(i32::MAX);
        pairs.sort_by_key(|s| index_order(s));
        if pairs.iter().any(|s| s.quest.is_none()) {
            // No listing quest can be assigned without guessing. Keep the entire
            // malformed topic's old merge, including equal-index insertion order.
            let mut lines = topics.get(id).cloned().unwrap_or_default();
            for s in pairs {
                let i = s.info.unwrap();
                if !lines.contains(&i) {
                    let at = s.index.and_then(|x| usize::try_from(x).ok()).map_or(lines.len(), |x| x.min(lines.len()));
                    lines.insert(at, i);
                }
            }
            out.insert(*id, lines);
            continue;
        }
        let own = topics.get(id).cloned().unwrap_or_default();
        let mut seen: HashSet<FormId> = own.iter().copied().collect();
        // Where each quest's own lines stand in the topic's stored list.
        let mut own_at: HashMap<FormId, Vec<usize>> = HashMap::new();
        for (at, i) in own.iter().enumerate() {
            if let Some(quest) = infos.get(i).and_then(|d| d.quest) {
                own_at.entry(quest).or_default().push(at);
            }
        }
        // Each listing quest's list: None for one of its own lines, Some for a shared
        // line with its index order.
        let mut quest_lines: HashMap<FormId, Vec<Option<(FormId, i32)>>> = HashMap::new();
        for s in pairs {
            let i = s.info.unwrap(); // Filtered above.
            if !seen.insert(i) {
                continue;
            }
            let quest = s.quest.unwrap(); // Unscoped topics use the fallback above.
            let lines = quest_lines.entry(quest).or_insert_with(|| vec![None; own_at.get(&quest).map_or(0, Vec::len)]);
            let at = usize::try_from(s.index.unwrap_or(-1)).map_or(lines.len(), |x| x.min(lines.len()));
            lines.insert(at, Some((i, index_order(s))));
        }
        // Shared lines among the own lines, keyed (place in `own`, index order, QSTI
        // rank, place in the quest's list); shared lines of quests with no own line.
        let mut placed: Vec<((usize, i32, usize, usize), FormId)> = Vec::new();
        let mut appended = Vec::new();
        for (rank, quest) in t.quests.iter().enumerate() {
            let Some(lines) = quest_lines.remove(quest) else { continue };
            let Some(at) = own_at.get(quest) else {
                appended.extend(lines.into_iter().flatten().map(|(i, _)| i));
                continue;
            };
            let mut own_before = 0;
            for (pos, line) in lines.into_iter().enumerate() {
                match line {
                    None => own_before += 1,
                    Some((i, index)) => {
                        let place = at.get(own_before).copied().unwrap_or(at[at.len() - 1] + 1);
                        placed.push(((place, index, rank, pos), i));
                    }
                }
            }
        }
        placed.sort_by_key(|&(key, _)| key);
        let mut placed = placed.into_iter().peekable();
        let mut lines = Vec::with_capacity(own.len() + placed.len() + appended.len());
        for (at, i) in own.into_iter().enumerate() {
            while let Some((_, shared)) = placed.next_if(|&((place, ..), _)| place <= at) {
                lines.push(shared);
            }
            lines.push(i);
        }
        lines.extend(placed.map(|(_, shared)| shared));
        lines.extend(appended);
        out.insert(*id, lines);
    }
    out
}

/// [`GameData::linked_from`]: each line with a Link From list puts its topic under
/// every topic the list names, once, in the topics' record order.
fn linked_from(defs: &HashMap<FormId, TopicDef>, infos: &HashMap<FormId, InfoDef>) -> HashMap<FormId, Vec<FormId>> {
    let mut out: HashMap<FormId, Vec<FormId>> = HashMap::new();
    for i in infos.values().filter(|i| !i.link_from.is_empty()) {
        let Some(t) = i.topic else { continue };
        for &x in &i.link_from {
            let list = out.entry(x).or_default();
            if !list.contains(&t) {
                list.push(t);
            }
        }
    }
    let order = |t: &FormId| defs.get(t).map_or((usize::MAX, 0), |d| d.order);
    for list in out.values_mut() {
        list.sort_by_key(|t| (order(t), t.0));
    }
    out
}

/// Every topic's dialogue lines in file order (first-defining plugin, then place in that
/// file), where each topic (DIAL) is first defined (plugin, record ordinal), and the
/// plugin that first defines each line.
type TopicLines = (HashMap<FormId, Vec<FormId>>, HashMap<FormId, (usize, usize)>, HashMap<FormId, usize>);

/// Puts the lines a later plugin adds to a topic where their Previous INFO (PNAM) says:
/// walking the file order, a line goes straight behind the line its winning PNAM names
/// when that line is already placed and is first defined by an earlier plugin, or was
/// itself moved this way (so a run of added lines chained by PNAM moves together). Every
/// other line keeps its file-order place: lines of one plugin, overrides (they keep the
/// place of their first definition), PNAM 0, and PNAM naming a line not in the topic
/// [documented: PNAM is the line above in the editor's list, Category:Dialogue "Edit
/// Response Order"; observed: within the add-ons file order agrees with PNAM in 5,677 of
/// 5,686 lines, dialogue.md §3.1; hypothesis: how the game orders two added lines naming
/// one previous line (the later one in file order goes first here, as each is put
/// straight behind it), a PNAM 0 line from a later plugin (left at the end), and broken
/// chains (left in file order)].
/// Returns how many lines it moved.
pub fn follow_previous(list: &mut Vec<FormId>, first_plugin: &HashMap<FormId, usize>, infos: &HashMap<FormId, InfoDef>) -> usize {
    let plugin = |i: &FormId| first_plugin.get(i).copied().unwrap_or(usize::MAX);
    let mut out: Vec<FormId> = Vec::with_capacity(list.len());
    let mut moved: std::collections::HashSet<FormId> = std::collections::HashSet::new();
    for &line in list.iter() {
        let at = infos
            .get(&line)
            .and_then(|i| i.previous)
            .filter(|p| *p != line && (plugin(p) < plugin(&line) || moved.contains(p)))
            .and_then(|p| out.iter().position(|x| *x == p));
        match at {
            Some(k) => {
                out.insert(k + 1, line);
                moved.insert(line);
            }
            None => out.push(line),
        }
    }
    *list = out;
    moved.len()
}

fn topic_lines(lo: &LoadOrder) -> TopicLines {
    use fnv_esm::raw::{group_type, visit_records};
    // Line -> (topic, plugin, ordinal) of its first definition.
    let mut first: HashMap<u32, (u32, usize, usize)> = HashMap::new();
    let mut dial: HashMap<FormId, (usize, usize)> = HashMap::new();
    for (pi, p) in lo.plugins.iter().enumerate() {
        let mut ord = 0usize;
        visit_records(p.nodes.get(1..).unwrap_or_default(), &mut |r, groups| {
            ord += 1;
            if &r.header.sig == b"DIAL" {
                if let Some(t) = lo.to_global(pi, r.header.form_id) {
                    dial.entry(FormId(t)).or_insert((pi, ord));
                }
            }
            if &r.header.sig != b"INFO" {
                return;
            }
            let Some(g) = groups.iter().rev().find(|g| g.group_type == group_type::TOPIC_CHILDREN) else { return };
            let (Some(info), Some(topic)) = (lo.to_global(pi, r.header.form_id), lo.to_global(pi, g.label_u32())) else { return };
            first.entry(info).or_insert((topic, pi, ord));
        });
    }
    let mut by_topic: HashMap<FormId, Vec<(usize, usize, FormId)>> = HashMap::new();
    let mut first_plugin: HashMap<FormId, usize> = HashMap::with_capacity(first.len());
    for (info, (topic, pi, ord)) in first {
        by_topic.entry(FormId(topic)).or_default().push((pi, ord, FormId(info)));
        first_plugin.insert(FormId(info), pi);
    }
    let lines = by_topic
        .into_iter()
        .map(|(t, mut v)| {
            v.sort();
            (t, v.into_iter().map(|x| x.2).collect())
        })
        .collect();
    (lines, dial, first_plugin)
}

fn read_one(lo: &LoadOrder, schema: &Schema, types: &ParamTypes, g: u32, pi: usize, r: &Record) -> Read {
    let mut out = Read::default();
    let sig = r.header.sig;
    let subs = match r.subrecords() {
        Ok(s) => s,
        Err(e) => {
            out.issues.note("record unreadable", || format!("0x{g:08X}: {e}"));
            return out;
        }
    };
    let map = |l: u32| lo.to_global(pi, l);
    let mut ctx = DecodeCtx { schema, on_formid: None };
    let mut val = |s: &fnv_esm::Subrecord| decode_sub(&sig, s, &mut ctx).value;
    let wanted = matches!(
        &sig,
        b"QUST"
            | b"INFO"
            | b"DIAL"
            | b"MESG"
            | b"TERM"
            | b"NOTE"
            | b"MICN"
            | b"FLST"
            | b"NPC_"
            | b"CREA"
            | b"LVLN"
            | b"LVLC"
            | b"PACK"
            | b"REPU"
            | b"CHAL"
            | b"REGN"
            | b"MGEF"
            | b"ALCH"
            | b"SPEL"
            | b"ENCH"
            | b"INGR"
            | b"RACE"
            | b"FACT"
            | b"WEAP"
            | b"IDLM"
            | b"IDLE"
            | b"PERK"
            | b"AVIF"
            | b"CSTY"
            | b"VTYP"
    ) || subs.iter().any(|s| &s.sig == b"DEST");
    // Abilities and worn enchantments (docs/spec-notes/aid-items.md §13), read raw: the
    // schema has no ARMO EITM or RACE SPLO. ENCH ENIT's first u32 is the type, 2 Weapon,
    // 3 Apparel [documented: schema `combat.json` EnchantmentData]; ARMO EITM names the
    // armour's object effect [documented: GECK wiki ActorValue, "object effects found on
    // armor"; observed: 186 ARMO of FalloutNV.esm carry a 4-byte EITM naming an ENCH of
    // type Apparel]; RACE SPLO is read as NPC_'s [hypothesis: FalloutNV.esm's races carry
    // none, observed].
    let word = |b: &[u8]| <[u8; 4]>::try_from(b.get(..4)?).ok().map(u32::from_le_bytes);
    match &sig {
        b"RACE" => out.race_spells = Some(subs.iter().filter(|s| &s.sig == b"SPLO").filter_map(|s| word(&s.data).and_then(map)).map(FormId).collect()),
        b"ARMO" => out.object_effect = subs.iter().find(|s| &s.sig == b"EITM").and_then(|s| word(&s.data)).filter(|&l| l != 0).and_then(map).map(FormId),
        b"ENCH" => out.enchantment_type = subs.iter().find(|s| &s.sig == b"ENIT").and_then(|s| word(&s.data)),
        // VTYP DNAM flags: bit 0 Allow Default Dialog, bit 1 Female [documented: xEdit
        // VTYP; observed: FalloutNV.esm's adult voices have bit 0, its unique and super
        // mutant voices not, every female voice bit 1]. Read raw: the schema has no VTYP.
        b"VTYP" => out.voice_default = subs.iter().find(|s| &s.sig == b"DNAM").and_then(|s| s.data.first()).map(|b| b & 1 != 0),
        _ => {}
    }
    if matches!(&sig, b"WEAP" | b"ARMO") {
        out.repair_list = subs.iter().find(|s| &s.sig == b"REPL").and_then(|s| word(&s.data)).filter(|&l| l != 0).and_then(map).map(FormId);
    }
    if &sig == b"FURN" {
        let flags = subs.iter().find(|s| &s.sig == b"MNAM").and_then(|s| <[u8; 4]>::try_from(s.data.as_slice()).ok()).map(u32::from_le_bytes);
        out.furniture = Some(flags.unwrap_or(0));
    }
    // A stage of a need: the value from which it holds, and its actor effect (DATA,
    // schema type NeedStage [documented: xEdit RADS/DEHY/HUNG/SLPD]; observed: 200, 400 …
    // 1000 and a SPEL named for that number, Water1: 200, Dehydration200;
    // docs/spec-notes/aid-items.md §11).
    let need = match &sig {
        b"RADS" => Some(crate::aid::RAD_LEVEL),
        b"DEHY" => Some(crate::aid::DEHYDRATION),
        b"HUNG" => Some(crate::aid::HUNGER),
        b"SLPD" => Some(crate::aid::SLEEP_DEPRIVATION),
        _ => None,
    };
    if let Some(code) = need {
        let data = subs.iter().find(|s| &s.sig == b"DATA").and_then(&mut val);
        let threshold = data.as_ref().and_then(|v| v.get("trigger_threshold")).and_then(Json::as_u64);
        let spell = data.as_ref().and_then(|v| v.get("actor_effect")).and_then(hexid).and_then(map);
        if let (Some(threshold), Some(spell)) = (threshold, spell) {
            out.need_stage = Some((code, threshold as u32, FormId(spell)));
        }
    }
    // A water type's actor effect: XNAM, a SPEL [documented: xEdit wbDefinitionsFNV.pas
    // WATR "Actor Effect"; observed: FalloutNV.esm's waters name WaterHeal1Purified …
    // WaterHeal5Terrible, the effects of drinking them]. docs/spec-notes/radiation.md §3.
    if &sig == b"ACTI" {
        out.drink_water = subs.iter().find(|s| &s.sig == b"WNAM").and_then(|s| word(&s.data)).filter(|&l| l != 0).and_then(map).map(FormId);
        // RNAM: the radio station a receiver plays [documented: xEdit ACTI RNAM;
        // `fnv_world::radio`].
        out.radio_receiver = subs.iter().find(|s| &s.sig == b"RNAM").and_then(|s| word(&s.data)).is_some_and(|l| l != 0);
    }
    if &sig == b"DOBJ" {
        let data = subs.iter().find(|s| &s.sig == b"DATA").map(|s| s.data.as_slice()).unwrap_or_default();
        out.default_objects = Some(data.as_chunks::<4>().0.iter().map(|c| Some(u32::from_le_bytes(*c)).filter(|&l| l != 0).and_then(map).map(FormId)).collect());
    }
    if &sig == b"WATR" {
        out.water_effect = subs.iter().find(|s| &s.sig == b"XNAM").and_then(|s| word(&s.data)).filter(|&l| l != 0).and_then(map).map(FormId);
        // "Causes Damage" is FNAM bit 0; the damage is DATA's u16, the whole of a 2-byte
        // DATA or the last two bytes of the old 186-byte form [documented: xEdit
        // wbDefinitionsFNV.pas WATR FNAM, DATA; GECK wiki WaterType "Causes n damage per
        // second"]. No master's water has either [observed]. radiation.md §3.1.
        let flag = subs.iter().find(|s| &s.sig == b"FNAM").and_then(|s| s.data.first()).is_some_and(|f| f & 1 != 0);
        let damage = subs.iter().find(|s| &s.sig == b"DATA").and_then(|s| s.data.len().checked_sub(2).and_then(|o| s.data.get(o..o + 2))).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
        out.water_damage = Some(if flag { damage } else { 0 });
    }
    // Recipes and their categories (`crate::crafting`): the raw layout; a recipe's
    // conditions are the common CTDA, read as any record's.
    if &sig == b"RCPE" {
        let mut ctx = DecodeCtx { schema, on_formid: None };
        let issues = &mut out.issues;
        let mut cond = |s: &fnv_esm::Subrecord| {
            let v = decode_sub(&sig, s, &mut ctx).value.or_else(|| decode_sub(b"PACK", s, &mut ctx).value)?;
            match cond::parse(&v, types, &map) {
                Ok(c) => Some(c),
                Err(e) => {
                    issues.note("condition unreadable", || format!("0x{g:08X}: {e}"));
                    None
                }
            }
        };
        let recipe = crate::crafting::RecipeDef::from_raw(FormId(g), &subs, &map, &mut cond);
        out.recipe = Some(recipe);
    }
    // Camera paths and shots (`crate::camera_paths`): a path's conditions are the common
    // CTDA, read as a recipe's.
    if &sig == b"CPTH" {
        let mut ctx = DecodeCtx { schema, on_formid: None };
        let issues = &mut out.issues;
        let mut cond = |s: &fnv_esm::Subrecord| {
            let v = decode_sub(&sig, s, &mut ctx).value.or_else(|| decode_sub(b"PACK", s, &mut ctx).value)?;
            match cond::parse(&v, types, &map) {
                Ok(c) => Some(c),
                Err(e) => {
                    issues.note("condition unreadable", || format!("0x{g:08X}: {e}"));
                    None
                }
            }
        };
        let zoom = |v: &Json| field_code(schema, b"CPTH", b"DATA", None, v);
        out.camera_path = Some(crate::camera_paths::CameraPathDef::from_fields(&subs, &mut val, &zoom, &map, &mut cond));
    }
    if &sig == b"CAMS" {
        out.camera_shot = crate::camera_paths::CameraShotDef::from_raw(&subs, &map);
    }
    if &sig == b"RCCT" {
        out.recipe_category = Some(crate::crafting::RecipeCategoryDef::from_raw(&subs));
    }
    // CLAS DATA "services" (schema `gameplay.json` ServiceFlags) [documented: xEdit
    // wbDefinitionsFNV.pas CLAS].
    if &sig == b"CLAS" {
        out.class_services = subs.iter().find(|s| &s.sig == b"DATA").and_then(&mut val).and_then(|v| v.get("services").map(|f| flag_bits(Some(f))));
    }
    if &sig == b"CSNO" {
        out.casino = Some(crate::casino::CasinoDef::from_raw(&subs, &map));
    }
    if &sig == b"CCRD" {
        out.caravan_card = crate::caravan::CardDef::from_raw(&subs);
    }
    if &sig == b"CDCK" {
        out.caravan_deck = Some(crate::caravan::DeckDef::from_raw(&subs, &map));
    }
    if &sig == b"IMAD" {
        let word = |o: usize| subs.iter().find(|s| &s.sig == b"DNAM").and_then(|s| <[u8; 4]>::try_from(s.data.get(o..o + 4)?).ok());
        if let (Some(a), Some(d)) = (word(0), word(4)) {
            out.image_modifier = Some(ImageModifierDef { animatable: u32::from_le_bytes(a) != 0, duration: f32::from_le_bytes(d) });
        }
    }
    for s in &subs {
        match &s.sig {
            b"EDID" if out.edid.is_none() => out.edid = val(s).and_then(|v| v.as_str().map(str::to_string)),
            b"FULL" if out.full.is_none() => out.full = Some(fnv_util::text::decode_z(&s.data)).filter(|n| !n.is_empty()),
            b"SCRI" if out.script.is_none() => out.script = val(s).as_ref().and_then(hexid).and_then(map).map(FormId),
            _ => {}
        }
    }
    // Ownership and crime (`crate::crime`): read from the raw subrecords, as the schema
    // lays them out [documented: xEdit wbDefinitionsFNV.pas CELL DATA flags, XOWN, XRNK;
    // FACT DATA; schemas `world.json`, `common.json`, `gameplay.json`].
    match &sig {
        b"CELL" => out.cell = Some(crate::crime::CellDef::from_raw(&subs, r.header.flags, &map)),
        b"REFR" | b"ACHR" | b"ACRE" | b"PGRE" => {
            out.owner_rank = subs.iter().find(|s| &s.sig == b"XRNK").and_then(|s| <[u8; 4]>::try_from(s.data.as_slice()).ok()).map(i32::from_le_bytes);
        }
        b"FACT" => {
            let data = subs.iter().find(|s| &s.sig == b"DATA").map(|s| s.data.as_slice()).unwrap_or_default();
            out.faction_flags = Some(u16::from(data.first().copied().unwrap_or(0)) | (u16::from(data.get(1).copied().unwrap_or(0)) << 8));
            out.faction_reputation = subs
                .iter()
                .find(|s| &s.sig == b"WMI1")
                .and_then(|s| <[u8; 4]>::try_from(s.data.as_slice()).ok())
                .map(u32::from_le_bytes)
                .filter(|&l| l != 0)
                .and_then(map)
                .map(FormId);
        }
        _ => {}
    }
    // A patrol point's result script (REFR with XPPA) is a unit too.
    if UNIT_SIGS.contains(&&sig) || (&sig == b"REFR" && subs.iter().any(|s| &s.sig == b"XPPA")) {
        let plugin = &lo.plugins[pi].name;
        let to_global = |l: u32| lo.to_global(pi, l);
        out.units = read_record(r, plugin, &to_global, schema, &mut out.read_issues);
    }
    if !wanted {
        return out;
    }
    let decoded: Vec<(&[u8; 4], Option<Json>)> = subs.iter().map(|s| (&s.sig, val(s))).collect();
    if let Some((_, Some(h))) = decoded.iter().find(|(s, _)| *s == b"DEST") {
        let mut dd = DestructionDef { health: h.get("health").and_then(Json::as_i64).unwrap_or(0) as i32, vats_targetable: flag_bits(h.get("flags")) & 1 != 0, stages: Vec::new() };
        for (sig, v) in decoded.iter() {
            match *sig {
                b"DSTD" => {
                    let Some(v) = v else { continue };
                    let n = |k: &str| v.get(k).and_then(Json::as_u64).unwrap_or(0) as u8;
                    let i = |k: &str| v.get(k).and_then(Json::as_i64).unwrap_or(0) as i32;
                    // FormID 0 is "none".
                    let form = |k: &str| v.get(k).and_then(hexid).filter(|&x| x != 0).and_then(map).map(FormId);
                    dd.stages.push(DestructionStage {
                        health_percent: n("health_percent"),
                        index: n("index"),
                        damage_stage: n("damage_stage"),
                        flags: flag_bits(v.get("flags")) as u8,
                        self_damage_per_second: i("self_damage_per_second"),
                        explosion: form("explosion"),
                        debris: form("debris"),
                        debris_count: i("debris_count"),
                        model: None,
                    });
                }
                // DMDL follows its stage's DSTD.
                b"DMDL" => {
                    if let (Some(st), Some(Json::String(m))) = (dd.stages.last_mut(), v) {
                        st.model = Some(m.trim().to_string()).filter(|m| !m.is_empty());
                    }
                }
                _ => {}
            }
        }
        out.destruction = Some(dd);
    }
    let fid = |v: &Option<Json>| v.as_ref().and_then(hexid).and_then(map).map(FormId);
    let cond = |v: &Option<Json>, issues: &mut Issues| -> Option<Condition> {
        match v.as_ref().map(|v| cond::parse(v, types, &map)) {
            Some(Ok(c)) => Some(c),
            Some(Err(e)) => {
                issues.note("condition unreadable", || format!("0x{g:08X}: {e}"));
                None
            }
            None => {
                issues.note("condition not decoded", || format!("0x{g:08X}"));
                None
            }
        }
    };
    match &sig {
        b"QUST" => {
            let mut q = QuestDef { form: FormId(g), edid: out.edid.clone(), script: out.script, ..Default::default() };
            // Where the next CTDA belongs.
            enum At {
                Quest,
                Entry,
                Target,
            }
            let mut at = At::Quest;
            let mut stage: Option<i32> = None;
            let mut objective: Option<i32> = None;
            for (s, v) in &decoded {
                match *s {
                    b"FULL" => q.name = v.as_ref().and_then(|v| v.as_str().map(str::to_string)),
                    b"DATA" => {
                        if let Some(v) = v {
                            q.flags = flag_bits(v.get("flags")) as u8;
                            q.priority = v.get("priority").and_then(Json::as_u64).unwrap_or(0) as u8;
                            q.delay = v.get("quest_delay").and_then(Json::as_f64).map(|d| d as f32);
                        }
                    }
                    b"INDX" => {
                        stage = v.as_ref().and_then(Json::as_i64).map(|x| x as i32);
                        objective = None;
                        if let Some(st) = stage {
                            q.stages.entry(st).or_default();
                        }
                        at = At::Quest;
                    }
                    b"QSDT" => {
                        if let Some(st) = stage {
                            let flags = flag_bits(v.as_ref()) as u8;
                            q.stages.entry(st).or_default().push(StageEntry { flags, ..Default::default() });
                            at = At::Entry;
                        }
                    }
                    b"CNAM" => {
                        if let Some(e) = stage.and_then(|st| q.stages.get_mut(&st)).and_then(|e| e.last_mut()) {
                            e.log = v.as_ref().and_then(|v| v.as_str().map(str::to_string));
                        }
                    }
                    b"NAM0" => {
                        let next = fid(v);
                        match stage.and_then(|st| q.stages.get_mut(&st)).and_then(|e| e.last_mut()) {
                            Some(e) => e.next_quest = next,
                            None => q.next_quest = next,
                        }
                    }
                    b"QOBJ" => {
                        let i = v.as_ref().and_then(Json::as_i64).map(|x| x as i32);
                        objective = i;
                        stage = None;
                        if let Some(i) = i {
                            q.objectives.insert(i, Objective { index: i, ..Default::default() });
                            q.objective_order.push(i);
                        }
                        at = At::Quest;
                    }
                    b"NNAM" => {
                        if let Some(o) = objective.and_then(|i| q.objectives.get_mut(&i)) {
                            o.text = v.as_ref().and_then(|v| v.as_str().map(str::to_string));
                        }
                    }
                    b"QSTA" => {
                        if let Some(o) = objective.and_then(|i| q.objectives.get_mut(&i)) {
                            let t = v.as_ref().and_then(|v| v.get("target")).and_then(hexid).and_then(map).map(FormId);
                            let ignores_locks = flag_bits(v.as_ref().and_then(|v| v.get("flags"))) & 1 != 0;
                            o.targets.push(Target { target: t, ignores_locks, conditions: Vec::new() });
                            at = At::Target;
                        }
                    }
                    b"CTDA" => {
                        let Some(c) = cond(v, &mut out.issues) else { continue };
                        match at {
                            At::Quest => q.conditions.push(c),
                            At::Entry => {
                                if let Some(e) = stage.and_then(|st| q.stages.get_mut(&st)).and_then(|e| e.last_mut()) {
                                    e.conditions.push(c);
                                }
                            }
                            At::Target => {
                                if let Some(t) =
                                    objective.and_then(|i| q.objectives.get_mut(&i)).and_then(|o| o.targets.last_mut())
                                {
                                    t.conditions.push(c);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            out.quest = Some(q);
        }
        b"INFO" => {
            let mut info = InfoDef { record_flags: r.header.flags, ..Default::default() };
            let mut after_next = false;
            let text = |v: &Option<Json>| v.as_ref().and_then(Json::as_str).unwrap_or_default().to_string();
            // FormID 0 is "none".
            let link = |v: &Option<Json>| v.as_ref().and_then(hexid).filter(|&x| x != 0).and_then(map).map(FormId);
            for (s, v) in &decoded {
                match *s {
                    b"QSTI" => info.quest = fid(v),
                    b"ANAM" => info.speaker = link(v),
                    b"DATA" => {
                        let Some(v) = v else { continue };
                        let code = |k: &str| v.get(k).and_then(|t| field_code(schema, &sig, b"DATA", Some(k), t)).unwrap_or(0) as u8;
                        info.info_type = code("type");
                        info.next_speaker = code("next_speaker");
                        info.flags = (flag_bits(v.get("flags1")) as u16 & 0xFF) | ((flag_bits(v.get("flags2")) as u16 & 0xFF) << 8);
                        info.say_once = info.has(info_flag::SAY_ONCE);
                        info.goodbye = info.has(info_flag::GOODBYE);
                    }
                    b"TPIC" => info.previous_topic = link(v),
                    b"PNAM" => {
                        info.has_previous = true;
                        info.previous = link(v);
                    }
                    b"NAME" => info.add_topics.extend(link(v)),
                    b"TRDT" => {
                        let mut resp = Response::default();
                        if let Some(v) = v {
                            resp.number = v.get("response_number").and_then(Json::as_u64).unwrap_or(0) as u8;
                            resp.emotion =
                                v.get("emotion_type").and_then(|t| field_code(schema, &sig, b"TRDT", Some("emotion_type"), t)).unwrap_or(0) as u8;
                            resp.emotion_value = v.get("emotion_value").and_then(Json::as_i64).unwrap_or(0) as i32;
                            resp.sound = v.get("sound").and_then(hexid).filter(|&x| x != 0).and_then(map).map(FormId);
                            resp.use_emotion_animation = match v.get("use_emotion_animation") {
                                Some(Json::Bool(b)) => *b,
                                Some(x) => x.as_u64().unwrap_or(0) != 0,
                                None => false,
                            };
                        }
                        info.responses.push(resp);
                        info.response_chars.push(0);
                    }
                    b"NAM1" => {
                        let t = text(v);
                        // A text without its TRDT (none in the masters) is a response of
                        // its own.
                        if info.responses.len() != info.response_chars.len() || info.responses.last().is_none_or(|x| !x.text.is_empty()) {
                            info.responses.push(Response::default());
                            info.response_chars.push(0);
                        }
                        if let (Some(resp), Some(n)) = (info.responses.last_mut(), info.response_chars.last_mut()) {
                            *n = t.chars().count();
                            resp.text = t;
                        }
                    }
                    b"NAM2" => {
                        if let Some(resp) = info.responses.last_mut() {
                            resp.notes = text(v);
                        }
                    }
                    b"SNAM" => {
                        if let Some(resp) = info.responses.last_mut() {
                            resp.speaker_idle = link(v);
                        }
                    }
                    b"LNAM" => {
                        if let Some(resp) = info.responses.last_mut() {
                            resp.listener_idle = link(v);
                        }
                    }
                    b"TCLT" => info.choices.extend(link(v)),
                    b"TCLF" => info.link_from.extend(link(v)),
                    b"TCFU" => info.follow_ups.extend(link(v)),
                    b"RNAM" => info.prompt = Some(text(v)).filter(|t| !t.is_empty()),
                    b"KNAM" => info.challenge_form = link(v),
                    b"DNAM" => {
                        info.challenge_difficulty =
                            v.as_ref().and_then(|t| field_code(schema, &sig, b"DNAM", None, t)).map(|c| c as u32);
                    }
                    b"NEXT" => after_next = true,
                    b"CTDA" if !after_next => {
                        if let Some(c) = cond(v, &mut out.issues) {
                            info.conditions.push(c);
                        }
                    }
                    _ => {}
                }
            }
            out.info = Some(info);
        }
        b"DIAL" => {
            let mut t = TopicDef { form: FormId(g), edid: out.edid.clone(), ..Default::default() };
            let mut quest: Option<FormId> = None;
            for (s, v) in &decoded {
                match *s {
                    b"FULL" => t.name = v.as_ref().and_then(|v| v.as_str().map(str::to_string)),
                    b"PNAM" => t.priority = v.as_ref().and_then(Json::as_f64).unwrap_or(50.0) as f32,
                    b"TDUM" => t.dumb_response = v.as_ref().and_then(|v| v.as_str().map(str::to_string)),
                    b"DATA" => {
                        if let Some(v) = v {
                            t.topic_type = v.get("type").and_then(|x| field_code(schema, &sig, b"DATA", Some("type"), x)).unwrap_or(0) as u8;
                            t.flags = flag_bits(v.get("flags")) as u8;
                        }
                    }
                    b"QSTI" => {
                        quest = fid(v);
                        t.quests.extend(quest);
                    }
                    b"QSTR" => t.removed_quests.extend(fid(v)),
                    b"INFC" => t.shared_infos.push(SharedInfo { quest, info: fid(v).filter(|f| f.0 != 0), index: None }),
                    b"INFX" => {
                        let x = v.as_ref().and_then(Json::as_i64).map(|x| x as i32);
                        match t.shared_infos.last_mut() {
                            Some(si) if si.index.is_none() => si.index = x,
                            _ => t.shared_infos.push(SharedInfo { quest, info: None, index: x }),
                        }
                    }
                    _ => {}
                }
            }
            out.topic = Some(t);
        }
        b"MESG" => {
            let mut m = MessageDef::default();
            for (s, v) in &decoded {
                match *s {
                    b"FULL" => m.title = v.as_ref().and_then(|v| v.as_str().map(str::to_string)).filter(|t| !t.trim().is_empty()),
                    b"DESC" => m.text = v.as_ref().and_then(|v| v.as_str().map(str::to_string)),
                    b"ITXT" => {
                        m.buttons.push(v.as_ref().and_then(|v| v.as_str()).unwrap_or_default().to_string());
                        m.button_conditions.push(Vec::new());
                    }
                    // A CTDA belongs to the button before it [observed: GSChemistrySetMsg
                    // has a Science condition after each of its two craft buttons].
                    b"CTDA" => match m.button_conditions.last_mut() {
                        Some(c) => c.extend(cond(v, &mut out.issues)),
                        None => out.issues.note("message condition before any button", || format!("0x{g:08X}")),
                    },
                    b"DNAM" => m.message_box = has_flag(v.as_ref(), "Message Box"),
                    b"INAM" => m.icon = fid(v).filter(|f| f.0 != 0),
                    b"TNAM" => m.display_seconds = v.as_ref().and_then(Json::as_u64).map(|x| x as u32),
                    _ => {}
                }
            }
            out.message = Some(m);
        }
        b"NOTE" => {
            let mut n = NoteDef::default();
            for (sub, v) in &decoded {
                match *sub {
                    b"FULL" => n.name = v.as_ref().and_then(|v| v.as_str()).map(str::to_string),
                    b"DATA" => {
                        n.kind = match v.as_ref() {
                            Some(Json::String(k)) => ["Sound", "Text", "Image", "Voice"].iter().position(|x| x == k).unwrap_or(0) as u8,
                            Some(x) => x.as_u64().unwrap_or(0) as u8,
                            None => 0,
                        }
                    }
                    b"XNAM" => n.texture = v.as_ref().and_then(|v| v.as_str()).map(str::to_string).filter(|t| !t.is_empty()),
                    _ => {}
                }
            }
            let form_of = |sig: &[u8; 4]| {
                let s = subs.iter().find(|s| &s.sig == sig)?;
                let b = s.data.get(..4)?;
                map(u32::from_le_bytes([b[0], b[1], b[2], b[3]])).filter(|f| *f != 0).map(FormId)
            };
            n.quests = subs
                .iter()
                .filter(|s| &s.sig == b"ONAM")
                .filter_map(|s| s.data.get(..4).and_then(|b| map(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))).filter(|f| *f != 0).map(FormId))
                .collect();
            n.snam = form_of(b"SNAM");
            match n.kind {
                1 => n.text = subs.iter().find(|s| &s.sig == b"TNAM").map(|s| fnv_util::text::decode_z(&s.data)),
                3 => n.topic = form_of(b"TNAM"),
                _ => {}
            }
            out.note = Some(n);
        }
        b"TERM" => {
            let mut t = TerminalDef::default();
            let s = |v: &Option<Json>| v.as_ref().and_then(|v| v.as_str()).unwrap_or_default().to_string();
            for (sub, v) in &decoded {
                match *sub {
                    b"FULL" => t.name = Some(s(v)).filter(|n| !n.trim().is_empty()),
                    b"DESC" => t.welcome = s(v),
                    b"PNAM" => t.password_note = fid(v).filter(|f| f.0 != 0),
                    b"DNAM" => {
                        if let Some(v) = v {
                            t.difficulty = match v.get("base_hacking_difficulty") {
                                Some(Json::String(name)) => {
                                    ["Very Easy", "Easy", "Average", "Hard", "Very Hard"].iter().position(|n| n == name).unwrap_or(5) as u8
                                }
                                Some(n) => n.as_u64().unwrap_or(5) as u8,
                                None => 5,
                            };
                            t.flags = flag_bits(v.get("flags")) as u8;
                            t.server = v.get("server_type").and_then(Json::as_u64).unwrap_or(0) as u8;
                        }
                    }
                    b"ITXT" => t.items.push(TerminalItem { text: s(v), ..Default::default() }),
                    b"RNAM" | b"ANAM" | b"INAM" | b"TNAM" | b"CTDA" => {
                        let Some(item) = t.items.last_mut() else { continue };
                        match *sub {
                            b"RNAM" => item.result = s(v),
                            b"ANAM" => item.flags = flag_bits(v.as_ref()) as u8,
                            b"INAM" => item.note = fid(v).filter(|f| f.0 != 0),
                            b"TNAM" => item.submenu = fid(v).filter(|f| f.0 != 0),
                            _ => item.conditions.extend(cond(v, &mut out.issues)),
                        }
                    }
                    _ => {}
                }
            }
            out.terminal = Some(t);
        }
        b"CSTY" => {
            let get = |sig: &[u8; 4]| decoded.iter().find(|(s, _)| *s == sig).and_then(|(_, v)| v.as_ref());
            let (std, adv, simple) = (get(b"CSTD"), get(b"CSAD"), get(b"CSSD"));
            let n = |v: Option<&Json>, k: &str, d: f32| v.and_then(|v| v.get(k)).and_then(Json::as_f64).map_or(d, |x| x as f32);
            out.combat_style = Some(CombatStyleDef {
                block_chance: n(std, "block_chance", 0.0),
                attack_chance: n(std, "attack_chance", 100.0),
                hand_to_hand_bonus: n(std, "hand_to_hand_bonus_to_attack", 0.0),
                power_attack_chance: n(std, "power_attack_chance", 0.0),
                power_attack_dirs: ["normal", "forward", "back", "left", "right"].map(|d| n(std, &format!("power_attack_{d}"), 0.0)),
                recoil_stagger_power_bonus: n(std, "recoil_stagger_bonus_to_power", 0.0),
                unconscious_power_bonus: n(std, "unconscious_bonus_to_power_attack", 0.0),
                unconscious_bonus: n(std, "unconscious_bonus_to_attack", 0.0),
                power_by_chance: has_flag(std.and_then(|v| v.get("flags")), "Choose Attack using % Chance"),
                hold_timer: (n(std, "hold_timer_min", 0.0), n(std, "hold_timer_max", 0.0)),
                attack_skill_mod: (n(adv, "attack_skill_modifier_base", 0.0), n(adv, "attack_skill_modifier_mult", 0.0)),
                attack_under_attack_mult: n(adv, "attack_while_under_attack_mult", 1.0),
                attack_not_under_attack_mult: n(adv, "attack_not_under_attack_mult", 1.0),
                recoil_stagger_bonus: n(std, "recoil_stagger_bonus_to_attack", 0.0),
                block_skill_mod: (n(adv, "block_skill_modifier_base", 0.0), n(adv, "block_skill_modifier_mult", 0.0)),
                block_under_attack_mult: n(adv, "block_while_under_attack_mult", 1.0),
                block_not_under_attack_mult: n(adv, "block_not_under_attack_mult", 1.0),
                attack_during_block_mult: n(adv, "attack_during_block_mult", 1.0),
                weapon_restrictions: match simple.and_then(|v| v.get("weapon_restrictions")) {
                    Some(Json::String(s)) => ["None", "Melee Only", "Ranged Only"].iter().position(|x| x == s).unwrap_or(0) as u32,
                    Some(v) => v.as_u64().unwrap_or(0) as u32,
                    None => 0,
                },
                range_mult: (n(simple, "ranged_weapon_range_mult_min", 1.0), n(simple, "ranged_weapon_range_mult_max", 1.0)),
                semi_auto_delay_mult: (n(simple, "semi_auto_firing_delay_mult_min", 1.0), n(simple, "semi_auto_firing_delay_mult_max", 1.0)),
                max_targeting_fov: n(simple, "max_targeting_fov", 0.0),
                combat_radius: n(simple, "combat_radius", 0.0),
                flee_personal_survival: has_flag(std.and_then(|v| v.get("flags")), "Flee Based on Personal Survival"),
                cover_search_radius: n(simple, "cover_search_radius", 0.0),
                take_cover_chance: n(simple, "take_cover_chance", 0.0),
                wait_timer: (n(simple, "wait_timer_min", 0.0), n(simple, "wait_timer_max", 0.0)),
                wait_to_fire_timer: (n(simple, "wait_to_fire_timer_min", 0.0), n(simple, "wait_to_fire_timer_max", 0.0)),
                fire_timer: (n(simple, "fire_timer_min", 0.0), n(simple, "fire_timer_max", 0.0)),
                dodge_chance: n(std, "dodge_chance", 0.0),
                left_right_chance: n(std, "left_right_chance", 0.0),
                dodge_lr_timer: (n(std, "dodge_lr_timer_min", 0.0), n(std, "dodge_lr_timer_max", 0.0)),
                dodge_forward_timer: (n(std, "dodge_forward_timer_min", 0.0), n(std, "dodge_forward_timer_max", 0.0)),
                dodge_back_timer: (n(std, "dodge_back_timer_min", 0.0), n(std, "dodge_back_timer_max", 0.0)),
                idle_timer: (n(std, "idle_timer_min", 0.0), n(std, "idle_timer_max", 0.0)),
                dodge_fatigue_mod: (n(adv, "dodge_fatigue_mod_base", 0.0), n(adv, "dodge_fatigue_mod_mult", 0.0)),
                encumbered_speed_mod: (n(adv, "encumbered_speed_mod_base", 0.0), n(adv, "encumbered_speed_mod_mult", 0.0)),
                dodge_mult: (n(adv, "dodge_while_under_attack_mult", 1.0), n(adv, "dodge_not_under_attack_mult", 1.0)),
                dodge_back_mult: (n(adv, "dodge_back_while_under_attack_mult", 1.0), n(adv, "dodge_back_not_under_attack_mult", 1.0)),
                dodge_forward_mult: (n(adv, "dodge_forward_while_attacking_mult", 1.0), n(adv, "dodge_forward_not_attacking_mult", 1.0)),
                power_attack_fatigue_mod: (n(adv, "power_attack_fatigue_mod_base", 0.0), n(adv, "power_attack_fatigue_mod_mult", 0.0)),
                rushing_attack_chance: n(std, "rushing_attack_chance", 0.0),
                rushing_attack_distance_mult: n(std, "rushing_attack_distance_mult", 0.0),
                melee_alert_ok: has_flag(std.and_then(|v| v.get("flags")), "Melee Alert OK"),
                ignore_threats: has_flag(std.and_then(|v| v.get("flags")), "Ignore Threats"),
                ignore_damaging_self: has_flag(std.and_then(|v| v.get("flags")), "Ignore Damaging Self"),
                ignore_damaging_group: has_flag(std.and_then(|v| v.get("flags")), "Ignore Damaging Group"),
                ignore_damaging_spectators: has_flag(std.and_then(|v| v.get("flags")), "Ignore Damaging Spectators"),
                cannot_use_stealthboy: has_flag(std.and_then(|v| v.get("flags")), "Cannot Use Stealthboy"),
            });
        }
        b"MICN" => {
            out.menu_icon = decoded.iter().find(|(s, _)| *s == b"ICON").and_then(|(_, v)| v.as_ref()?.as_str().map(str::to_string));
        }
        b"FLST" => {
            out.form_list = Some(decoded.iter().filter(|(s, _)| *s == b"LNAM").filter_map(|(_, v)| fid(v)).collect());
        }
        b"NPC_" | b"CREA" => {
            let mut a = ActorDef { sig, ..Default::default() };
            for (s, v) in &decoded {
                match *s {
                    b"SNAM" => {
                        if let Some(f) = v.as_ref().and_then(|v| v.get("faction")).and_then(hexid).and_then(map) {
                            let rank = v.as_ref().and_then(|v| v.get("rank")).and_then(Json::as_i64).unwrap_or(0) as i8;
                            a.factions.push((FormId(f), rank));
                        }
                    }
                    b"VTCK" => a.voice = fid(v),
                    b"PKID" => a.packages.extend(fid(v)),
                    b"SPLO" => a.spells.extend(fid(v).filter(|f| f.0 != 0)),
                    b"TPLT" => a.template = fid(v),
                    b"EITM" => a.unarmed_effect = fid(v).filter(|f| f.0 != 0),
                    b"EAMT" => a.unarmed_anim = v.as_ref().and_then(Json::as_str).map(str::to_string),
                    b"ZNAM" => a.combat_style = fid(v).filter(|f| f.0 != 0),
                    b"INAM" => a.death_item = fid(v).filter(|f| f.0 != 0),
                    b"RNAM" if &sig == b"NPC_" => a.race = fid(v),
                    b"NAM5" if &sig == b"CREA" => a.sound_level = v.as_ref().and_then(Json::as_str).map(str::to_string),
                    b"RNAM" if &sig == b"CREA" => a.attack_reach = v.as_ref().and_then(Json::as_u64).map(|n| n.min(255) as u8),
                    b"CNAM" if &sig == b"NPC_" => a.class = fid(v),
                    b"DATA" if &sig == b"CREA" => {
                        a.creature_type =
                            v.as_ref().and_then(|v| v.get("type")).and_then(|t| field_code(schema, &sig, b"DATA", Some("type"), t));
                        a.base_health = v.as_ref().and_then(|v| v.get("health")).and_then(Json::as_i64).unwrap_or(0);
                        a.zero_health = v.as_ref().and_then(|v| v.get("health")).and_then(Json::as_i64) == Some(0);
                        a.attributes = v.as_ref().and_then(|v| v.get("attributes")).and_then(attributes);
                        let skill = |k: &str| v.as_ref().and_then(|v| v.get(k)).and_then(Json::as_u64).map(|n| n.min(255) as u8);
                        (a.combat_skill, a.stealth_skill) = (skill("combat_skill"), skill("stealth_skill"));
                        a.attack_damage = v.as_ref().and_then(|v| v.get("damage")).and_then(Json::as_i64);
                    }
                    b"DATA" if &sig == b"NPC_" => {
                        a.base_health = v.as_ref().and_then(|v| v.get("base_health")).and_then(Json::as_i64).unwrap_or(0);
                        a.zero_health = v.as_ref().and_then(|v| v.get("base_health")).and_then(Json::as_i64) == Some(0);
                        a.attributes = v.as_ref().and_then(|v| v.get("attributes")).and_then(attributes);
                    }
                    b"DNAM" if &sig == b"NPC_" => {
                        a.skills = v.as_ref().and_then(|v| v.get("skill_values")).and_then(skills);
                        a.skill_offsets = v.as_ref().and_then(|v| v.get("skill_offsets")).and_then(skills);
                    }
                    b"AIDT" => {
                        a.energy = v.as_ref().and_then(|v| v.get("energy_level")).and_then(Json::as_u64).map(|e| e.min(255) as u8);
                        let code = |k: &str| v.as_ref().and_then(|v| v.get(k)).and_then(|t| field_code(schema, &sig, b"AIDT", Some(k), t));
                        (a.aggression, a.confidence, a.mood, a.assistance) = (code("aggression"), code("confidence"), code("mood"), code("assistance"));
                        a.responsibility = v.as_ref().and_then(|v| v.get("responsibility")).and_then(Json::as_i64);
                        a.aggro_radius_behavior = v.as_ref().and_then(|v| v.get("aggro_radius_behavior")).is_some_and(|b| b.as_str() == Some("True") || b.as_bool() == Some(true) || b.as_u64().is_some_and(|n| n != 0));
                        a.aggro_radius = v.as_ref().and_then(|v| v.get("aggro_radius")).and_then(Json::as_u64).map_or(0, |n| n.min(u64::from(u32::MAX)) as u32);
                        a.services = flag_bits(v.as_ref().and_then(|v| v.get("services")));
                    }
                    b"ACBS" => {
                        let f = v.as_ref().and_then(|v| v.get("flags"));
                        a.female = has_flag(f, "Female");
                        a.essential = has_flag(f, "Essential");
                        a.invulnerable = &sig == b"CREA" && has_flag(f, "Invulnerable");
                        a.is_ghost = &sig == b"CREA" && has_flag(f, "Is Ghost");
                        a.allow_pickpocket = &sig == b"CREA" && has_flag(f, "Allow Pickpocket");
                        a.no_knockdowns = has_flag(f, "No Knockdowns");
                        a.immobile = &sig == b"CREA" && has_flag(f, "Immobile");
                        a.swims = &sig == b"CREA" && has_flag(f, "Swims");
                        a.walks = &sig == b"CREA" && has_flag(f, "Walks");
                        a.flies = &sig == b"CREA" && has_flag(f, "Flies");
                        a.no_combat_in_water = &sig == b"CREA" && has_flag(f, "No Combat in Water");
                        a.cant_open_doors = &sig == b"CREA" && has_flag(f, "Can't Open Doors");
                        a.no_low_level = has_flag(f, "No Low Level Processing");
                        a.respawn = has_flag(f, "Respawn");
                        a.pc_level_mult = has_flag(f, "PC Level Mult");
                        a.autocalc = &sig == b"NPC_" && has_flag(f, "Auto-calc stats");
                        a.autocalc_service = &sig == b"NPC_" && has_flag(f, "Autocalc Service");
                        a.no_vats_melee = has_flag(f, "No VATS Melee");
                        a.not_pushable = has_flag(f, "Not Pushable");
                        a.no_head_track = has_flag(f, "No Rotating To Head-track");
                        a.no_head = has_flag(f, "No Head");
                        a.no_right_arm = has_flag(f, "No Right Arm");
                        a.no_left_arm = has_flag(f, "No Left Arm");
                        a.no_blood_spray = has_flag(f, "No Blood Spray");
                        a.no_blood_decal = has_flag(f, "No Blood Decal");
                        a.allow_pc_dialogue = &sig == b"CREA" && has_flag(f, "Allow PC Dialogue");
                        let n = |k: &str| v.as_ref().and_then(|v| v.get(k)).and_then(Json::as_i64).unwrap_or(0);
                        (a.level_raw, a.calc_min, a.calc_max) = (n("level_or_level_mult_raw"), n("calc_min"), n("calc_max"));
                        (a.fatigue, a.speed_mult) = (n("fatigue"), n("speed_multiplier"));
                        a.karma = v.as_ref().and_then(|v| v.get("karma")).and_then(Json::as_f64).unwrap_or(0.0);
                        a.disposition_base = n("disposition_base");
                        let tf = v.as_ref().and_then(|v| v.get("template_flags"));
                        a.template_packages = has_flag(tf, "AI Packages");
                        a.template_stats = has_flag(tf, "Stats");
                        a.template_ai = has_flag(tf, "AI Data");
                        a.template_factions = has_flag(tf, "Factions");
                        a.template_base = has_flag(tf, "Base Data");
                        a.template_traits = has_flag(tf, "Traits");
                        a.template_model = has_flag(tf, "Model/Animation");
                        a.template_inventory = has_flag(tf, "Inventory");
                        a.template_script = has_flag(tf, "Script");
                        a.template_spells = has_flag(tf, "Actor Effect List");
                    }
                    _ => {}
                }
            }
            out.actor = Some(a);
        }
        b"LVLN" | b"LVLC" => {
            let mut l = LeveledActorDef::default();
            for (s, v) in &decoded {
                match *s {
                    b"LVLF" => {
                        l.all_levels = has_flag(v.as_ref(), "Calculate from all levels <= player's level");
                        let bits = [("Calculate from all levels <= player's level", 1u8), ("Calculate for each item in count", 2), ("Use All", 4)];
                        l.flags = bits.iter().filter(|(n, _)| has_flag(v.as_ref(), n)).map(|(_, b)| b).sum();
                    }
                    b"LVLD" => l.chance_none = v.as_ref().and_then(Json::as_u64).unwrap_or(0).min(100) as u8,
                    b"LVLO" => {
                        let level = v.as_ref().and_then(|v| v.get("level")).and_then(Json::as_u64).unwrap_or(0).min(u64::from(u16::MAX)) as u16;
                        if let Some(f) = v.as_ref().and_then(|v| v.get("reference")).and_then(hexid).and_then(map) {
                            l.entries.push((level, FormId(f)));
                            l.counts.push(v.as_ref().and_then(|v| v.get("count")).and_then(Json::as_u64).unwrap_or(1).min(u64::from(u16::MAX)) as u16);
                        }
                    }
                    _ => {}
                }
            }
            out.leveled_actor = Some(l);
        }
        b"PACK" => {
            let code = |sub: &[u8; 4], field: &str, v: &Json| field_code(schema, &sig, sub, Some(field), v);
            let ctx = crate::package::ReadCtx { code: &code, map: &map };
            let mut issues = Issues::default();
            out.package = Some(crate::package::read(&decoded, &ctx, &mut |v| cond(v, &mut issues)));
            out.issues.merge(issues);
        }
        b"REPU" => {
            out.reputation = decoded.iter().find(|(s, _)| *s == b"DATA").and_then(|(_, v)| v.as_ref()?.as_f64()).map(|x| x as f32);
            out.reputation_name = decoded.iter().find(|(s, _)| *s == b"FULL").and_then(|(_, v)| v.as_ref()?.as_str().map(str::to_string)).filter(|n| !n.trim().is_empty());
        }
        b"CHAL" => {
            let sub = |sig: &[u8; 4]| decoded.iter().find(|(s, _)| *s == sig).and_then(|(_, v)| v.as_ref());
            let text = |sig: &[u8; 4]| sub(sig).and_then(|v| v.as_str()).map(str::to_string).filter(|t| !t.trim().is_empty());
            let link = |sig: &[u8; 4]| sub(sig).and_then(hexid).and_then(map).map(FormId).filter(|f| f.0 != 0);
            out.challenge = sub(b"DATA").map(|v| {
                let n = |k: &str| v.get(k).and_then(Json::as_u64).unwrap_or(0) as u32;
                // value1/value2 are two bytes, value3 four (xEdit); read little-endian.
                let bytes = |k: &str| v.get(k).and_then(Json::as_array).map_or(0, |a| a.iter().rev().fold(0u32, |acc, b| (acc << 8) | b.as_u64().unwrap_or(0) as u32));
                crate::challenges::ChallengeDef {
                    form: FormId(0),
                    kind: v.get("type").and_then(|t| field_code(schema, &sig, b"DATA", Some("type"), t)).unwrap_or(-1),
                    threshold: n("threshold"),
                    flags: flag_bits(v.get("flags")),
                    interval: n("interval"),
                    value1: bytes("value1"),
                    value2: bytes("value2"),
                    value3: bytes("value3"),
                    form1: link(b"SNAM"),
                    form2: link(b"XNAM"),
                    name: text(b"FULL"),
                    description: text(b"DESC"),
                }
            });
        }
        b"REGN" => {
            let mut rg = RegionDef { border: r.header.flags & BORDER_REGION != 0, ..Default::default() };
            let num = |v: Option<&Json>| v.and_then(|x| x.as_f64().or_else(|| x.get("value")?.as_f64())).unwrap_or(0.0);
            let mut header: Option<&Json> = None;
            for (s, v) in &decoded {
                match *s {
                    b"RDAT" => header = v.as_ref(),
                    // A weather list follows a header of type 3 [documented: xEdit
                    // wbRegionDataEntries].
                    b"RDWT" if num(header.and_then(|h| h.get("type"))) == 3.0 => {
                        let h = header;
                        let list = v
                            .as_ref()
                            .and_then(Json::as_array)
                            .into_iter()
                            .flatten()
                            .filter_map(|w| {
                                let form = |k: &str| w.get(k).and_then(hexid).filter(|&x| x != 0).and_then(map).map(FormId);
                                Some(RegionWeather { weather: form("weather")?, chance: num(w.get("chance")) as i32, global: form("global") })
                            })
                            .collect();
                        rg.weathers = Some(RegionWeathers { flags: num(h.and_then(|h| h.get("flags"))) as u8, priority: num(h.and_then(|h| h.get("priority"))) as u8, list });
                    }
                    // A map name follows a header of type 4 [observed: VMapGoodspringsRegion].
                    b"RDMP" if num(header.and_then(|h| h.get("type"))) == 4.0 => {
                        let name = v.as_ref().and_then(Json::as_str).unwrap_or_default().to_string();
                        rg.map = Some((name, num(header.and_then(|h| h.get("priority"))) as u8));
                    }
                    b"WNAM" => rg.world = fid(v),
                    b"RPLI" => rg.areas.push(Vec::new()),
                    b"RPLD" => {
                        let pts: Vec<[f32; 2]> = v
                            .as_ref()
                            .and_then(Json::as_array)
                            .into_iter()
                            .flatten()
                            .filter_map(|p| Some([p.get("x")?.as_f64()? as f32, p.get("y")?.as_f64()? as f32]))
                            .collect();
                        match rg.areas.last_mut() {
                            Some(a) if a.is_empty() => *a = pts,
                            _ => rg.areas.push(pts),
                        }
                    }
                    _ => {}
                }
            }
            rg.areas.retain(|a| a.len() >= 3);
            out.region = Some(rg);
        }
        b"MGEF" => {
            if let Some((_, Some(v))) = decoded.iter().find(|(s, _)| *s == b"DATA") {
                let archetype = v.get("archetype").and_then(|t| field_code(schema, &sig, b"DATA", Some("archetype"), t)).unwrap_or(-1);
                let item = v.get("associated_item").and_then(hexid).filter(|&x| x != 0).and_then(map).map(FormId);
                let actor_value = v.get("actor_value").and_then(|t| field_code(schema, &sig, b"DATA", Some("actor_value"), t));
                let resistance = v.get("resistance_type").and_then(|t| field_code(schema, &sig, b"DATA", Some("resistance_type"), t));
                out.magic_effect = Some(MagicEffectDef {
                    flags: flag_bits(v.get("flags")),
                    archetype,
                    script: item.filter(|_| archetype == ARCHETYPE_SCRIPT),
                    actor_value: actor_value.and_then(|c| u16::try_from(c).ok()),
                    shader: v.get("effect_shader").and_then(hexid).filter(|&x| x != 0).and_then(map).map(FormId),
                    resistance: resistance.and_then(|c| u16::try_from(c).ok()),
                    effect_sound: v.get("effect_sound").and_then(hexid).filter(|&x| x != 0).and_then(map).map(FormId),
                    hit_sound: v.get("hit_sound").and_then(hexid).filter(|&x| x != 0).and_then(map).map(FormId),
                });
            }
        }
        b"ALCH" | b"SPEL" | b"ENCH" | b"INGR" => {
            let mut list: Vec<EffectItem> = Vec::new();
            for (s, v) in &decoded {
                match *s {
                    b"EFID" => list.extend(fid(v).map(|effect| EffectItem { effect, magnitude: 0, area: 0, duration: 0, actor_value: None, conditions: Vec::new() })),
                    b"EFIT" => {
                        if let (Some(e), Some(v)) = (list.last_mut(), v.as_ref()) {
                            let n = |k: &str| v.get(k).and_then(Json::as_u64).unwrap_or(0) as u32;
                            e.magnitude = n("magnitude");
                            e.area = n("area");
                            e.duration = n("duration");
                            e.actor_value = v
                                .get("actor_value")
                                .and_then(|t| field_code(schema, &sig, b"EFIT", Some("actor_value"), t))
                                .and_then(|c| u16::try_from(c).ok());
                        }
                    }
                    b"ENIT" if &sig == b"ALCH" => {
                        if let Some(v) = v.as_ref() {
                            let form = |k: &str| v.get(k).and_then(hexid).filter(|&x| x != 0).and_then(map).map(FormId);
                            let etyp = decoded.iter().find(|(s, _)| *s == b"ETYP").and_then(|(_, t)| t.as_ref());
                            out.ingestible = Some(IngestibleDef {
                                value: v.get("value").and_then(Json::as_i64).unwrap_or(0) as i32,
                                flags: flag_bits(v.get("flags")) as u8,
                                withdrawal: form("withdrawal_effect"),
                                addiction_chance: v.get("addiction_chance").and_then(Json::as_f64).unwrap_or(0.0) as f32,
                                consume_sound: form("sound_consume"),
                                equip_type: etyp.and_then(|t| field_code(schema, &sig, b"ETYP", None, t)).unwrap_or(-1),
                            });
                        }
                    }
                    b"SPIT" if &sig == b"SPEL" => {
                        out.spell_type = v.as_ref().and_then(|v| v.get("type")).and_then(|t| field_code(schema, &sig, b"SPIT", Some("type"), t));
                        out.spell_flags = v.as_ref().map(|v| flag_bits(v.get("flags")) as u8);
                    }
                    b"CTDA" => {
                        if let Some(c) = cond(v, &mut out.issues) {
                            if let Some(e) = list.last_mut() {
                                e.conditions.push(c);
                            }
                        }
                    }
                    _ => {}
                }
            }
            out.effects = Some(list);
        }
        b"RACE" => {
            out.child_race = decoded
                .iter()
                .find(|(s, _)| *s == b"DATA")
                .is_some_and(|(_, v)| has_flag(v.as_ref().and_then(|v| v.get("flags")), "Child"));
            let link = |sub: &[u8; 4]| decoded.iter().find(|(s, _)| *s == sub).and_then(|(_, v)| fid(v)).filter(|f| f.0 != 0);
            out.race_older = link(b"ONAM");
            out.race_younger = link(b"YNAM");
        }
        b"FACT" => {
            out.faction_modifiers = Some(decoded.iter().filter(|(s, _)| *s == b"XNAM").filter_map(|(_, v)| {
                let v = v.as_ref()?;
                let f = v.get("faction").and_then(hexid).and_then(map)?;
                Some((FormId(f), v.get("modifier")?.as_i64()?))
            }).collect());
            out.faction = Some(
                decoded
                    .iter()
                    .filter(|(s, _)| *s == b"XNAM")
                    .filter_map(|(_, v)| {
                        let v = v.as_ref()?;
                        let f = v.get("faction").and_then(hexid).and_then(map)?;
                        let r = field_code(schema, &sig, b"XNAM", Some("group_combat_reaction"), v.get("group_combat_reaction")?)?;
                        Some((FormId(f), r))
                    })
                    .collect(),
            );
        }
        b"PERK" => {
            // The perk's own fields come before its first effect (PRKE); the CTDA and
            // DATA after it belong to the effects [documented: xEdit wbDefinitionsFNV.pas
            // PERK record order].
            let head = decoded.iter().position(|(s, _)| *s == b"PRKE").unwrap_or(decoded.len());
            let raw_data = subs[..head].iter().find(|s| &s.sig == b"DATA").map(|s| s.data.as_slice());
            let mut unread = Issues::default();
            let conds: Vec<Option<Condition>> = decoded[..head].iter().filter(|(s, _)| *s == b"CTDA").map(|(_, v)| cond(v, &mut unread)).collect();
            let text = |sig: &[u8; 4]| decoded[..head].iter().find(|(s, _)| *s == sig).and_then(|(_, v)| v.as_ref()?.as_str().map(str::to_string));
            let mut p = PerkDef::from_data(raw_data);
            p.name = text(b"FULL").unwrap_or_default();
            p.description = text(b"DESC").unwrap_or_default();
            p.icon = text(b"ICON").filter(|i| !i.trim().is_empty());
            p.conditions = conds.into_iter().collect();
            let mut entry: Option<PerkEntry> = None;
            let mut ordinal: u32 = 0;
            let mut tab: i8 = 0;
            let mut epft: Option<u8> = None;
            for (i, s) in subs.iter().enumerate().skip(head) {
                match &s.sig {
                    b"PRKE" => {
                        let b = |k: usize| s.data.get(k).copied().unwrap_or(0);
                        let effect = match b(0) {
                            0 => PerkEffect::QuestStage { quest: FormId(0), stage: 0 },
                            1 => PerkEffect::Ability(FormId(0)),
                            2 => PerkEffect::EntryPoint(EntryPointDef { conditions: Some(Vec::new()), ..Default::default() }),
                            _ => PerkEffect::Other,
                        };
                        entry = Some(PerkEntry { rank: b(1), priority: b(2), effect, ordinal });
                        ordinal += 1;
                        tab = 0;
                        epft = None;
                    }
                    b"DATA" => {
                        let Some(e) = entry.as_mut() else { continue };
                        e.effect = match &e.effect {
                            PerkEffect::Ability(_) => match <[u8; 4]>::try_from(s.data.as_slice()).ok().map(u32::from_le_bytes).and_then(map) {
                                Some(g) => PerkEffect::Ability(FormId(g)),
                                None => PerkEffect::Other,
                            },
                            PerkEffect::QuestStage { .. } if s.data.len() >= 5 => {
                                let id = u32::from_le_bytes([s.data[0], s.data[1], s.data[2], s.data[3]]);
                                match map(id) {
                                    Some(q) => PerkEffect::QuestStage { quest: FormId(q), stage: s.data[4] },
                                    None => PerkEffect::Other,
                                }
                            }
                            PerkEffect::EntryPoint(ep) if s.data.len() >= 2 => {
                                PerkEffect::EntryPoint(EntryPointDef { point: s.data[0], function: s.data[1], ..ep.clone() })
                            }
                            _ => PerkEffect::Other,
                        };
                    }
                    b"PRKC" => tab = s.data.first().map_or(0, |&b| b as i8),
                    b"CTDA" => {
                        if let Some(PerkEffect::EntryPoint(ep)) = entry.as_mut().map(|e| &mut e.effect) {
                            let c = cond(&decoded[i].1, &mut unread);
                            match (ep.conditions.as_mut(), c) {
                                (Some(list), Some(c)) => list.push((tab, c)),
                                _ => ep.conditions = None,
                            }
                        }
                    }
                    b"EPFT" => epft = s.data.first().copied(),
                    b"EPF2" => {
                        if let Some(PerkEffect::EntryPoint(ep)) = entry.as_mut().map(|e| &mut e.effect) {
                            ep.button = Some(zstring(&s.data)).filter(|b| !b.is_empty());
                        }
                    }
                    b"EPF3" => {
                        if let Some(PerkEffect::EntryPoint(ep)) = entry.as_mut().map(|e| &mut e.effect) {
                            ep.run_immediately = s.data.first().is_some_and(|b| b & 1 != 0);
                        }
                    }
                    b"EPFD" => {
                        if let Some(PerkEffect::EntryPoint(ep)) = entry.as_mut().map(|e| &mut e.effect) {
                            ep.param_type = epft;
                            ep.raw = s.data.clone();
                            if epft == Some(1) {
                                ep.value = <[u8; 4]>::try_from(s.data.as_slice()).ok().map(f32::from_le_bytes);
                            }
                            if epft == Some(3) {
                                ep.leveled_list = <[u8; 4]>::try_from(s.data.as_slice()).ok().map(u32::from_le_bytes).filter(|&l| l != 0).and_then(map).map(FormId);
                            }
                        }
                    }
                    b"PRKF" => p.entries.extend(entry.take()),
                    _ => {}
                }
            }
            out.perk = Some(p);
        }
        b"AVIF" => {
            let text = |sig: &[u8; 4]| decoded.iter().find(|(s, _)| *s == sig).and_then(|(_, v)| v.as_ref()?.as_str().map(str::to_string));
            let anam = subs.iter().find(|s| &s.sig == b"ANAM").map(|s| zstring(&s.data));
            out.av_info = Some(ActorValueInfo {
                name: text(b"FULL").unwrap_or_default(),
                short_name: anam.unwrap_or_default(),
                description: text(b"DESC").unwrap_or_default(),
                icon: text(b"ICON").filter(|i| !i.trim().is_empty()),
            });
        }
        b"IDLE" => {
            // A condition that cannot be read is no issue of the script data (some idle
            // conditions use functions scripts never call, animation.md §1.2): the idle
            // is only marked as one that cannot be judged.
            let mut unread = Issues::default();
            let conds: Vec<Option<Condition>> = decoded.iter().filter(|(s, _)| *s == b"CTDA").map(|(_, v)| cond(v, &mut unread)).collect();
            let field = |sub: &[u8; 4]| decoded.iter().find(|(s, _)| *s == sub).and_then(|(_, v)| v.as_ref());
            let mut i = IdleDef::from_fields(field(b"MODL").and_then(Json::as_str), field(b"DATA"));
            i.conditions = conds.into_iter().collect();
            // ANAM: parent and previous IDLE, file-local FormIDs (null: none).
            let link = |k: &str| field(b"ANAM").and_then(|a| a.get(k)).and_then(hexid).filter(|l| *l != 0).and_then(map).map(FormId);
            i.parent = link("parent");
            i.previous = link("previous");
            out.idle = Some(i);
        }
        b"IDLM" => {
            let code = |sub: &[u8; 4], field: &str, v: &Json| field_code(schema, &sig, sub, Some(field), v);
            let ctx = crate::package::ReadCtx { code: &code, map: &map };
            out.idle_marker = Some(crate::package::read(&decoded, &ctx, &mut |_| None).idles.unwrap_or_default());
        }
        b"WEAP" => {
            let dnam = decoded.iter().find(|(s, _)| *s == b"DNAM").and_then(|(_, v)| v.as_ref());
            let code = |field: &str| dnam?.get(field).and_then(|t| field_code(schema, &sig, b"DNAM", Some(field), t));
            out.weapon_skill = code("skill");
            out.weapon_anim_type = code("animation_type");
        }
        _ => {}
    }
    out
}

fn resolve<'s>(schema: &'s Schema, t: &'s TypeDef) -> &'s TypeDef {
    match t {
        TypeDef::Named(n) => schema.types.get(n).map(|t| resolve(schema, t)).unwrap_or(t),
        _ => t,
    }
}

/// The integer code of a decoded enumeration value (the decoder gives the value's
/// name, or the number when the schema has no name for it), from the schema layout of
/// `sig`/`sub` (and the struct field `field`).
/// DATA attributes: S.P.E.C.I.A.L. in record order.
fn attributes(v: &Json) -> Option<[u8; 7]> {
    let mut out = [0u8; 7];
    for (o, name) in out.iter_mut().zip(fnv_sim::forms::SPECIAL) {
        *o = v.get(name)?.as_u64()?.min(255) as u8;
    }
    Some(out)
}

/// One of NPC_ DNAM's two skill arrays, Barter to Unarmed.
fn skills(v: &Json) -> Option<[u8; 14]> {
    let mut out = [0u8; 14];
    for (o, name) in out.iter_mut().zip(crate::actor_values::SKILL_FIELDS) {
        *o = v.get(name)?.as_u64()?.min(255) as u8;
    }
    Some(out)
}

pub(crate) fn field_code(schema: &Schema, sig: &[u8; 4], sub: &[u8; 4], field: Option<&str>, v: &Json) -> Option<i64> {
    if let Some(n) = v.as_i64() {
        return Some(n);
    }
    let name = v.as_str()?;
    let def = schema.records.get(sig).and_then(|r| r.subrecords.get(sub)).or_else(|| schema.common.get(sub))?;
    let mut t = resolve(schema, &def.ty);
    if let Some(f) = field {
        let TypeDef::Struct(fields) = t else { return None };
        t = resolve(schema, &fields.iter().find(|x| x.name == f)?.ty);
    }
    match t {
        TypeDef::Enum { values, .. } => values.iter().find(|(_, n)| n == name).map(|(k, _)| *k),
        _ => None,
    }
}


#[cfg(test)]
mod unit_key_tests {
    use super::*;
    use fnv_script::unit::{Header, PackageEvent};
    use fnv_script::ScriptKind;

    fn unit(plugin: &str, owner: UnitOwner) -> ScriptUnit {
        ScriptUnit { plugin: plugin.into(), owner, kind: ScriptKind::Object, header: Header::default(), variables: Vec::new(), refs: Vec::new(), has_scda: false, bytes: Vec::new(), source: None }
    }

    #[test]
    fn the_built_key_is_the_units_key_for_every_owner() {
        let owners = [
            UnitOwner::Script { form: 0x0001_0F2A },
            UnitOwner::QuestStage { form: 0x0010_4C1C, stage: 10, entry: 0 },
            UnitOwner::Info { form: 0x0200_0801, end: true },
            UnitOwner::Info { form: 0x0200_0801, end: false },
            UnitOwner::Package { form: 0x5, event: PackageEvent::Begin },
            UnitOwner::Package { form: 0x5, event: PackageEvent::End },
            UnitOwner::Package { form: 0x5, event: PackageEvent::Change },
            UnitOwner::Terminal { form: 0x7, item: 3 },
            UnitOwner::Perk { form: 0x9, entry: 1 },
            UnitOwner::Patrol { form: 0xA },
            UnitOwner::Other { sig: "NOTE".into(), form: 0xB, ordinal: 2 },
        ];
        for owner in owners {
            let u = unit("FalloutNV.esm", owner);
            let c = Compiled::new(u.clone(), Program::default(), 0);
            assert_eq!(c.key(), u.key());
            assert_eq!(&**c.shared_key(), u.key().as_str());
            // A clone of the unit shares the text.
            assert!(Arc::ptr_eq(c.shared_key(), c.clone().shared_key()));
        }
        let c = Compiled::new(unit("FalloutNV.esm", UnitOwner::QuestStage { form: 0x0010_4C1C, stage: 10, entry: 0 }), Program::default(), 0);
        assert_eq!(c.key(), "FalloutNV.esm|QUST 0x00104C1C stage 10 entry 0");
    }
}

#[cfg(test)]
mod reads_nothing_tests {
    use super::*;
    use fnv_esm::raw::{write_subrecords, Subrecord};
    use fnv_esm::Plugin;

    fn sub(sig: [u8; 4], data: &[u8]) -> Subrecord {
        Subrecord { sig, data: data.to_vec(), extended: false }
    }

    fn record(sig: &[u8; 4], fid: u32, payload: &[u8]) -> Vec<u8> {
        [&sig[..], &(payload.len() as u32).to_le_bytes(), &0u32.to_le_bytes(), &fid.to_le_bytes(), &[0; 4], &15u16.to_le_bytes(), &[0; 2], payload].concat()
    }

    /// A plugin of these records, as it is parsed from a file. No game data.
    fn plugin(records: &[Vec<u8>]) -> Plugin {
        let hedr = [1.34f32.to_le_bytes(), 0i32.to_le_bytes(), 0x800u32.to_le_bytes()].concat();
        let tes4 = record(b"TES4", 0, &write_subrecords(&[sub(*b"HEDR", &hedr)]));
        let body = records.concat();
        let group = [&b"GRUP"[..], &((24 + body.len()) as u32).to_le_bytes(), b"REFR", &0i32.to_le_bytes(), &[0; 8], &body].concat();
        Plugin::parse(&[tes4, group].concat(), "T.esm".into()).unwrap()
    }

    /// Each record's [`reads_nothing`], [`carries_units`] and [`read_one`] (as text), checked
    /// by `f`.
    fn each(records: &[Vec<u8>], mut f: impl FnMut(&Record, String)) {
        let plugins = [plugin(records)];
        let lo = LoadOrder::new(&plugins).unwrap();
        let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
        for r in &lo.records[0] {
            let read = read_one(&lo, &schema, &ParamTypes::default(), r.header.form_id, 0, r);
            f(r, format!("{read:?}"));
        }
    }

    #[test]
    fn a_skipped_record_is_one_read_one_finds_nothing_in() {
        // Every subrecord the schema knows for each type, but those read_one looks for.
        let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
        let records: Vec<Vec<u8>> = BULK_SIGS
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let mut sigs: Vec<[u8; 4]> = schema.common.keys().copied().collect();
                sigs.extend(schema.records.get(s).into_iter().flat_map(|r| r.subrecords.keys().copied()));
                sigs.retain(|x| !BULK_READ_SUBS.contains(x));
                sigs.sort_unstable();
                sigs.dedup();
                assert!(sigs.len() > 10, "{sigs:?}");
                let subs: Vec<Subrecord> = sigs.into_iter().map(|x| sub(x, &[0; 64])).collect();
                record(s, 0x800 + i as u32, &write_subrecords(&subs))
            })
            .collect();
        let mut seen = 0;
        each(&records, |r, read| {
            assert!(reads_nothing(r), "{}", r.sig_str());
            assert_eq!(read, format!("{:?}", Read::default()), "{}", r.sig_str());
            seen += 1;
        });
        assert_eq!(seen, BULK_SIGS.len());
    }

    #[test]
    fn a_subrecord_read_one_looks_for_keeps_the_record() {
        for s in BULK_READ_SUBS {
            let payload = write_subrecords(&[sub(*b"DATA", &[0; 24]), sub(s, &[0; 4])]);
            let records: Vec<Vec<u8>> = BULK_SIGS.iter().enumerate().map(|(i, b)| record(b, 0x800 + i as u32, &payload)).collect();
            each(&records, |r, _| assert!(!reads_nothing(r), "{} {}", r.sig_str(), fnv_util::tag_str(&s)));
        }
        // Another record type is always read, and so is one that cannot be read (its issue
        // is noted).
        let other = record(b"STAT", 0x800, &write_subrecords(&[sub(*b"DATA", &[0; 4])]));
        let bad = record(b"REFR", 0x801, b"DATA\xFF\xFF");
        each(&[other, bad], |r, read| {
            assert!(!reads_nothing(r), "{}", r.sig_str());
            if r.header.form_id == 0x801 {
                assert!(read.contains("record unreadable"), "{read}");
            }
        });
    }

    #[test]
    fn a_patrol_point_carries_units_when_its_record_is_readable() {
        let patrol = record(b"REFR", 0x800, &write_subrecords(&[sub(*b"XPPA", &[])]));
        let plain = record(b"REFR", 0x801, &write_subrecords(&[sub(*b"NAME", &[1, 0, 0, 0])]));
        // XPPA, then a subrecord cut short.
        let cut = record(b"REFR", 0x802, &[&write_subrecords(&[sub(*b"XPPA", &[])])[..], b"DATA\xFF\xFF"].concat());
        each(&[patrol, plain, cut], |r, _| assert_eq!(carries_units(r), r.header.form_id == 0x800, "{:X}", r.header.form_id));
    }

    #[test]
    fn owners_and_labels_tell_the_same_units_apart() {
        use fnv_script::unit::PackageEvent as P;
        let owners = [
            UnitOwner::Script { form: 1 },
            UnitOwner::QuestStage { form: 1, stage: 1, entry: 0 },
            UnitOwner::QuestStage { form: 1, stage: 1, entry: 1 },
            UnitOwner::QuestStage { form: 1, stage: -1, entry: 1 },
            UnitOwner::Info { form: 1, end: false },
            UnitOwner::Info { form: 1, end: true },
            UnitOwner::Package { form: 1, event: P::Begin },
            UnitOwner::Package { form: 1, event: P::End },
            UnitOwner::Package { form: 1, event: P::Change },
            UnitOwner::Terminal { form: 1, item: 0 },
            UnitOwner::Perk { form: 1, entry: 0 },
            UnitOwner::Patrol { form: 1 },
            UnitOwner::Other { sig: "SCPT".into(), form: 1, ordinal: 0 },
            UnitOwner::Other { sig: "REFR".into(), form: 1, ordinal: 0 },
        ];
        let labels: HashSet<String> = owners.iter().map(UnitOwner::label).collect();
        assert_eq!(labels.len(), owners.len());
    }
}
