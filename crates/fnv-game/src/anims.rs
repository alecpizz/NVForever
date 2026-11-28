//! Animation groups scripts play on objects: `PlayGroup` and `IsAnimPlaying`
//! (docs/spec-notes/activators.md §5).
//!
//! The rules only record what is to be played ([`AnimRequest`], taken from
//! [`ObjectAnims::requests`] by whoever animates objects, and traced as `anim_asked`).
//! Playing it (the model's sequence of the group's name, its timing) is the engine's
//! work, which tells the rules what each reference plays ([`ObjectAnims::set_playing`])
//! so `IsAnimPlaying` can answer ([`ObjectAnims::driven`]). With nothing animating (a
//! headless run), nothing is ever playing [design: as `IsIdlePlaying`, `crate::idles`],
//! and at most [`crate::sound::MAX_PENDING`] requests are kept. Only the last group asked of each
//! reference is saved ([`ObjectAnims::asked`]): a gate a script opened and left open is
//! drawn, and collides, open after a load (issue #1396) [design].
//!
//! `LoopGroup` is not here: no script in the masters calls it and its opcode is not
//! known [observed: docs/spec-notes/script-functions-catalogue.md].

use crate::functions::{none, yes, Args, Ctx};
use crate::game::Game;
use crate::trace::EventKind;
use fnv_script::vm::{ArgValue, Value};
use fnv_sim::FormId;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

/// How a group starts, PlayGroup's InitFlag [documented: GECK wiki PlayGroup].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Start {
    /// 0, "Normal": the current animation finishes its cycle, then the group starts from
    /// its beginning.
    Normal,
    /// 1, "Immediate Start": the current animation stops; the group starts from its
    /// beginning.
    Immediate,
    /// 2, "Immediate Loop": the current animation stops; the group starts at the
    /// beginning of its loop cycle.
    ImmediateLoop,
}

impl Start {
    /// From the flag's value; another value is taken as Normal [design].
    pub fn from_flag(flag: i32) -> Start {
        match flag {
            1 => Start::Immediate,
            2 => Start::ImmediateLoop,
            _ => Start::Normal,
        }
    }

    pub fn flag(self) -> i32 {
        match self {
            Start::Normal => 0,
            Start::Immediate => 1,
            Start::ImmediateLoop => 2,
        }
    }
}

/// An animation group a script asked a reference to play.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnimRequest {
    pub on: FormId,
    /// The group's code (the `anim_group` enumeration of the bytecode).
    pub group: u16,
    /// Its name as shipped source spells it (`Forward`), which is also the name of the
    /// model's sequence [hypothesis: activators.md §5]; `None` for a code without a known
    /// spelling.
    pub name: Option<String>,
    pub start: Start,
}

/// What scripts asked of objects' animations and what the engine reports they play.
#[derive(Debug, Default)]
pub struct ObjectAnims {
    /// For the engine to take.
    pub requests: Vec<AnimRequest>,
    /// The group each reference plays, as the engine last told.
    playing: HashMap<FormId, u16>,
    /// Set by an engine that animates objects: a group asked for counts as playing from
    /// the call on, until the engine says otherwise (so a script testing IsAnimPlaying
    /// right after PlayGroup sees it) [design]. Unset (a headless run): nothing plays.
    pub driven: bool,
    /// The last group and flag traced per reference: a script repeating the same call
    /// every frame is traced once [design, as stubs are].
    traced: HashMap<FormId, (u16, i32)>,
    /// References whose animation state a script reset (`Reset3DState`), for the engine
    /// to stop their groups.
    pub resets: Vec<FormId>,
    /// The last group asked of each reference (code, name), kept until a reset: what an
    /// object shows when it is drawn or given collision again, a group played once at its
    /// end (`nvforever` object_anims.rs, `nvf_play::phys`). Saved [design].
    asked: BTreeMap<FormId, (u16, String)>,
    /// References whose last group asked was set from outside the game's scripts (a co-op
    /// guest told by its host what the host's scripts asked before it joined): the engine
    /// shows it again as on a fresh draw ([`ObjectAnims::show_asked`]). Not saved.
    pub reshow: Vec<FormId>,
    /// The group each new ask replaced in `asked`, while it is the last: put back when
    /// the engine finds the newer one is not in the object's model
    /// ([`ObjectAnims::not_in_model`]). Not saved.
    replaced: HashMap<FormId, (u16, String)>,
}

impl ObjectAnims {
    /// The engine says `on` plays `group` now (`None`: no group, or a group played once
    /// has ended and its last pose is held).
    pub fn set_playing(&mut self, on: FormId, group: Option<u16>) {
        match group {
            Some(g) => {
                self.playing.insert(on, g);
            }
            None => {
                self.playing.remove(&on);
            }
        }
    }

    /// Forget what `on` plays and was asked to (`Reset3DState`): its requests not taken
    /// yet are dropped and the engine is told to stop its group.
    pub fn reset(&mut self, on: FormId) {
        self.playing.remove(&on);
        self.traced.remove(&on);
        self.asked.remove(&on);
        self.replaced.remove(&on);
        self.requests.retain(|r| r.on != on);
        if !self.resets.contains(&on) {
            if self.resets.len() >= crate::sound::MAX_PENDING {
                self.resets.remove(0);
            }
            self.resets.push(on);
        }
    }

    /// The engine found that `on`'s model has no sequence `name`: a group it cannot show
    /// is not what it shows when drawn again. When `name` is the last group asked, the
    /// one it replaced is the last again. A tripwire's script plays `Forward` and then
    /// activates itself, which asks its "Open" sequence (`Game::set_ref_open`), a name
    /// `tripwire01.nif` does not have: drawn again after a cell load, the sprung wire
    /// showed whole and armed (issue #2420) [design].
    pub fn not_in_model(&mut self, on: FormId, name: &str) {
        if self.asked.get(&on).is_some_and(|(_, n)| n.eq_ignore_ascii_case(name)) {
            match self.replaced.remove(&on) {
                Some(before) => {
                    self.asked.insert(on, before);
                }
                None => {
                    self.asked.remove(&on);
                }
            }
        }
    }

    /// The last group asked of `on` (code, name), until a reset ([`ObjectAnims::reset`]).
    pub fn asked(&self, on: FormId) -> Option<(u16, &str)> {
        self.asked.get(&on).map(|(g, n)| (*g, n.as_str()))
    }

    /// Every reference's last group asked, for the saved game.
    pub fn asked_all(&self) -> &BTreeMap<FormId, (u16, String)> {
        &self.asked
    }

    /// `on`'s last group asked is `group` (`name`), as a co-op host's scripts asked it
    /// before this game joined: kept as if asked here, and shown as a draw shows it (a
    /// group played once at its end, a loop from its start), not played from its start.
    pub fn show_asked(&mut self, on: FormId, group: u16, name: String) {
        self.asked.insert(on, (group, name));
        if !self.reshow.contains(&on) {
            self.reshow.push(on);
        }
    }

    /// The last groups asked, from a saved game.
    pub fn restore_asked(&mut self, asked: BTreeMap<FormId, (u16, String)>) {
        self.asked = asked;
        self.replaced.clear();
    }

    /// The group `on` plays, as last told.
    pub fn playing(&self, on: FormId) -> Option<u16> {
        self.playing.get(&on).copied()
    }
}

impl Game<'_> {
    /// Queue `r` for the engine and trace it. Fires JIP's `SetOnPlayGroupEventHandler`
    /// (`playgroup`) for real animation group codes unless a flag-0 request repeats the
    /// group already playing (`ObjectAnims::playing`).
    pub fn ask_anim(&mut self, r: AnimRequest) {
        let (on, group, start) = (r.on, r.group, r.start);
        let already_playing = self.anims.playing(on) == Some(group);
        if self.anims.traced.insert(r.on, (r.group, r.start.flag())) != Some((r.group, r.start.flag())) {
            let group = r.name.clone().unwrap_or_else(|| format!("code {}", r.group));
            self.emit(EventKind::AnimAsked { on: r.on, group, flag: r.start.flag() });
        }
        if self.anims.driven {
            self.anims.playing.insert(r.on, r.group);
        }
        if let Some(name) = &r.name {
            if let Some(before) = self.anims.asked.insert(r.on, (r.group, name.clone())).filter(|(_, n)| n != name) {
                self.anims.replaced.insert(r.on, before);
            }
        }
        // A co-op host's: its guests' copies play it too [design].
        if let Some(out) = self.coop_effects.as_mut().filter(|o| o.len() < crate::sound::MAX_PENDING) {
            out.push(crate::sound::CoopEffect::Anim { on: r.on, group: r.group, flag: r.start.flag() });
        }
        let q = &mut self.anims.requests;
        if q.len() >= crate::sound::MAX_PENDING {
            q.remove(0);
        }
        q.push(r);
        if !(start == Start::Normal && already_playing) {
            self.fire_play_group(on, group);
        }
    }

    /// The spelling of animation group `code` in shipped source.
    pub fn anim_group_name(&self, code: u16) -> Option<String> {
        self.data.table.enum_name("anim_group", code).map(str::to_string)
    }

    /// The code of the animation group spelt `name`, when the table has it.
    pub fn anim_group_code(&self, name: &str) -> Option<u16> {
        self.data.table.enums.get("anim_group")?.iter().find(|(_, n)| n.eq_ignore_ascii_case(name)).map(|(c, _)| *c)
    }

    /// Open (true) or close the reference `r`: its open flag (GetOpenState) and, for an
    /// activator that is not a radio receiver, its "Open" or "Close" sequence, asked as
    /// PlayGroup asks one, so the engine plays it once and holds its end, its collision
    /// follows and it is shown so when drawn again (activators.md §5). A model without
    /// the sequence plays nothing. Returns whether the state changed.
    ///
    /// [hypothesis: an activator with Open and Close sequences opens and closes like a
    /// door; observed in the data: Fallout 3's Overseer terminal opens the tunnel with
    /// `SetOpenState 1` on the desk activator `VRmOverseerDesk01` (Open 10 s with the
    /// sound key OBJOverseerDeskOpen), and the vault door control pod opens the vault
    /// door `VaultGearDoor` (Open 25 s) by activating it, whose script then runs its
    /// default activation; issues #2231, #2232.]
    pub fn set_ref_open(&mut self, r: FormId, open: bool) -> Result<bool, String> {
        let was = std::mem::replace(&mut self.world.get(r).map_err(|e| e.to_string())?.open, open);
        if was == open {
            return Ok(false);
        }
        let base = crate::functions::base_of(self, r);
        let animated = base.is_some_and(|b| self.data.sig(b).as_ref() == Some(b"ACTI") && !self.data.radio_receivers.contains(&b));
        if animated {
            let name = if open { "Open" } else { "Close" };
            // A code of its own when the table has none for the name [design: the
            // engine plays by name; IsAnimPlaying compares codes].
            let group = self.anim_group_code(name).unwrap_or(if open { OPEN_GROUP } else { CLOSE_GROUP });
            tracing::info!("activator: {r} {} (its {name} sequence)", if open { "opens" } else { "closes" });
            self.ask_anim(AnimRequest { on: r, group, name: Some(name.to_string()), start: Start::Immediate });
        }
        Ok(true)
    }
}

/// Group codes for an activator's Open and Close sequences when the bytecode table has
/// no code spelt so ([`Game::set_ref_open`]) [design: out of the range of the table's
/// codes].
pub const OPEN_GROUP: u16 = 0xFF00;
pub const CLOSE_GROUP: u16 = 0xFF01;

/// The group parameter: a bare code (as compiled), or a number.
fn group_code(a: &Args, i: usize) -> Option<u16> {
    match a.get(i)? {
        ArgValue::Enum(c) => Some(*c),
        ArgValue::Value(v) => Some(v.num() as u16),
        _ => None,
    }
}

/// `ref.PlayGroup group flag`: play an animation group on the calling reference
/// [documented: GECK wiki PlayGroup].
pub fn play_group(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let on = ctx.subject()?;
    let group = group_code(a, 0).ok_or("parameter 1 is not an animation group")?;
    let start = Start::from_flag(a.num_or(1, 0.0) as i32);
    let name = g.anim_group_name(group);
    g.ask_anim(AnimRequest { on, group, name, start });
    none()
}

/// The group code a PlayAnimSequence request carries: a sequence named by text, not an
/// animation group [design: IsAnimPlaying with a group never matches it].
pub const SEQUENCE: u16 = u16::MAX;

/// JIP's `ref.PlayAnimSequence sequence [nodeName]`: plays the named sequence of the
/// reference's model at once [documented: GECK wiki PlayAnimSequence], through the same
/// request PlayGroup makes, so the engine plays the sequence of that name
/// (`nvforever` object_anims.rs). The node argument is not used: the engine plays the
/// model's sequences by name wherever their controller manager is [hypothesis: the shipped
/// models keep one manager]. On an actor the page allows a `.kf` path; the engine's actor
/// groups do not load one [not built]. A door, drawn by the engine's door system, does not
/// play it either (its Open and Close sequences play only as it opens and closes) [not
/// built].
pub(crate) fn call_ext(g: &mut Game, name: &str, subject: Option<FormId>, args: &[fnv_script::vm::XVal]) -> Option<Result<fnv_script::vm::XVal, String>> {
    if name != "playanimsequence" {
        return None;
    }
    let seq = match args.first() {
        Some(fnv_script::vm::XVal::Str(s)) if !s.is_empty() => s.clone(),
        _ => return Some(Ok(fnv_script::vm::XVal::Num(0.0))),
    };
    let Some(on) = subject else { return Some(Ok(fnv_script::vm::XVal::Num(0.0))) };
    g.ask_anim(AnimRequest { on, group: SEQUENCE, name: Some(seq), start: Start::Immediate });
    Some(Ok(fnv_script::vm::XVal::Num(0.0)))
}

/// `ref.IsAnimPlaying [group]`: 1 while the calling reference plays an animation; with a
/// group, only that group [documented: GECK wiki IsAnimPlaying, which says "the sequence
/// type for that Animation Group"; we match the group itself, hypothesis: shipped
/// scripts test `IsAnimPlaying Forward` and `IsAnimPlaying Backward` as different
/// states, activators.md §5]. What plays is what the engine reports: a group asked by
/// PlayGroup, while it runs (a group played once until its end, a looping one until
/// another replaces it); on an actor without one, the movement group its body plays
/// (`Idle` standing, `Forward`, `FastLeft` …, `crate::anim_state`) [observed: Sunny
/// Smiles standing read `IsAnimPlaying Idle` 1, function probes 2026-10-17; hypothesis:
/// the walking and running groups count the same way]. An idle an object plays by
/// itself is not counted [hypothesis].
pub fn is_anim_playing(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let on = ctx.subject()?;
    if let Some(playing) = g.anims.playing(on) {
        return yes(group_code(a, 0).is_none_or(|want| playing == want));
    }
    let Some(body) = g.anim_state(on).and_then(|s| s.group) else { return none() };
    yes(match group_code(a, 0) {
        Some(want) => g.anim_group_name(want).is_some_and(|n| n.eq_ignore_ascii_case(body)),
        None => true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_and_reports() {
        assert_eq!(Start::from_flag(0), Start::Normal);
        assert_eq!(Start::from_flag(1), Start::Immediate);
        assert_eq!(Start::from_flag(2), Start::ImmediateLoop);
        assert_eq!(Start::from_flag(7), Start::Normal);
        assert_eq!(Start::ImmediateLoop.flag(), 2);
        let mut a = ObjectAnims::default();
        assert_eq!(a.playing(FormId(5)), None);
        a.set_playing(FormId(5), Some(3));
        assert_eq!(a.playing(FormId(5)), Some(3));
        a.set_playing(FormId(5), None);
        assert_eq!(a.playing(FormId(5)), None);
    }
}
