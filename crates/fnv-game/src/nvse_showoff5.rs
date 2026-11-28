//! ShowOff NVSE functions of batch 5 (#1010), the last 37: the level-up menu's pages and
//! points, the pause menu, live explosions and projectiles, the fast-travel checks,
//! Improved Detection, combat actors, VATS and item values, and the player's small
//! states [documented: each page in the wiki export; docs/spec-notes/ext-showoff.md §8].
//! What the engine draws or plays goes through the existing game→engine channels: the
//! level-up menu reads [`LevelUpAsk`], the pause menu [`State::pause_asks`], the shots
//! the tracer and hitscan switches, the blasts register [`LiveExplosion`]s.

use crate::game::{Game, RunCtx};
use fnv_script::vm::XVal;
use fnv_sim::FormId;
use std::collections::HashMap;

fn form(args: &[XVal], k: usize) -> Option<FormId> {
    match args.get(k)? {
        XVal::Ref(0) => None,
        XVal::Ref(r) => Some(FormId(*r)),
        XVal::Num(x) if *x >= 1.0 && x.fract() == 0.0 => Some(FormId(*x as u32)),
        _ => None,
    }
}

fn num(args: &[XVal], k: usize, dflt: f64) -> f64 {
    match args.get(k) {
        Some(XVal::Num(x)) => *x,
        _ => dflt,
    }
}

fn text(args: &[XVal], k: usize) -> Option<String> {
    match args.get(k) {
        Some(XVal::Str(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

/// How the level-up menu is to open, asked by ShowPerkMenu or ShowSkillMenu; the engine
/// takes it when it opens the menu (`nvf_play::levelup`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LevelUpOpen {
    /// The perks page alone, its Back button hidden (ShowPerkMenu); else the skills page as
    /// the last page (ShowSkillMenu).
    pub perks_only: bool,
    /// The perks or skill points to allot in place of the level's.
    pub count: Option<i32>,
    /// The menu's title in place of `sLevelUpTitleText`'s.
    pub title: Option<String>,
}

/// The level-up menu as scripts see it: what the engine's menu says each frame, and what
/// the scripts ask of it [documented: GetLevelUpMenuCurrentPage, GetLevelUpMenuPoints,
/// SetLevelUpMenuCurrentPage, SetLevelUpMenuCanExitEarly].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LevelUpAsk {
    /// The page shown, 0 skills, 1 perks; `None` while the menu is closed. Written by the
    /// engine.
    pub page: Option<u8>,
    /// Skill points to allot and allotted, perks to take and taken. Written by the engine.
    pub points: [i32; 4],
    /// The menu may be left with points unallotted.
    pub can_exit_early: bool,
    pub open_as: Option<LevelUpOpen>,
    /// A page change asked by SetLevelUpMenuCurrentPage: 0 skills, 1 perks, 2 close.
    pub page_request: Option<u8>,
}

/// A live explosion: a temporary reference of the EXPL base the engine places for each
/// blast ([`Game::explosion_placed`]), gone after its frame.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveExplosion {
    pub source: Option<FormId>,
    pub radius: f32,
    /// `Game::time` it goes at.
    pub until: f64,
}

/// This batch's session state (not saved).
#[derive(Debug, Default)]
pub struct State {
    pub levelup: LevelUpAsk,
    /// ShowPauseMenu calls so far and the last one's page (0 main, 1 load, 2 save, 3
    /// settings, 4 help); the engine opens the pause menu on each new count.
    pub pause_asks: u64,
    pub pause_mode: u8,
    pub explosions: HashMap<FormId, LiveExplosion>,
    /// SetForceDrawHitscanProjectiles: hitscan shots drawn as in VATS.
    pub force_draw_hitscan: bool,
    /// SetProjectileTracerChanceOverride: `Some(false)` never a tracer, `Some(true)` always
    /// one for a projectile with a tracer chance; `None` the chance rolls.
    pub tracer_override: Option<bool>,
    /// HighlightAdditionalReferenceAlt's references, and the one flashing.
    pub highlighted: Vec<FormId>,
    pub flashing: Option<FormId>,
    /// `Game::time` the player last drank from placed water (`Game::drink_from`).
    pub last_drink: Option<f64>,
    /// The player's rotation last frame, and whether it changed (IsPlayerLookingAround).
    pub last_rot: Option<[f32; 3]>,
    pub looking: bool,
}

/// How long the player counts as drinking after a drink from placed water, seconds
/// [hypothesis: the page gives no length; about the drinking animation's].
pub const DRINK_SECONDS: f64 = 1.0;

impl Game<'_> {
    /// A blast of `base` at `at`: its live reference, which GetExplosionRefRadius and
    /// GetExplosionRefSource read until the next frame; `None` when none could be placed.
    /// The engine reads [`Game::explosion_radius`] back before working out who it reaches.
    pub fn explosion_placed(&mut self, base: FormId, source: Option<FormId>, at: [f32; 3], radius: f32) -> Option<FormId> {
        let cell = source.and_then(|s| self.cell_of(s)).or_else(|| self.cell_of(FormId::PLAYER_REF));
        let place_base = self.unclone(base);
        let r = self.world.place(place_base, cell, at, [0.0; 3], 1).ok()?;
        let radius = self.explosion_radius_override(base).unwrap_or(radius);
        let until = self.time + self.cfg.frame_seconds;
        self.showoff.b5.explosions.insert(r, LiveExplosion { source, radius, until });
        Some(r)
    }

    /// A live explosion's radius as a script left it.
    pub fn explosion_radius(&self, r: FormId) -> Option<f32> {
        self.showoff.b5.explosions.get(&r).map(|e| e.radius)
    }

    /// A live explosion's source as a script left it (none: no one's).
    pub fn explosion_source(&self, r: FormId) -> Option<FormId> {
        self.showoff.b5.explosions.get(&r).and_then(|e| e.source)
    }

    /// Each frame (`Game::aux_timer_updates`): live explosions past their frame go, and
    /// whether the player turned since the last frame.
    pub(crate) fn showoff5_tick(&mut self) {
        let now = self.time;
        let gone: Vec<FormId> = self.showoff.b5.explosions.iter().filter(|(_, e)| e.until <= now).map(|(r, _)| *r).collect();
        for r in gone {
            self.showoff.b5.explosions.remove(&r);
            let _ = self.world.set_deleted(r, true);
        }
        let rot = self.world.peek(FormId::PLAYER_REF).map(|s| s.rot);
        self.showoff.b5.looking = rot.is_some() && self.showoff.b5.last_rot.is_some() && rot != self.showoff.b5.last_rot;
        self.showoff.b5.last_rot = rot;
    }

    /// Whether the player could fast travel from here now: the checks the Pip-Boy's map
    /// makes before it asks (`nvf_play::pipboy::data::travel::refusal`), but the marker's:
    /// EnableFastTravel 0, an interior or a world flagged against it
    /// (`crate::fast_travel::place_forbids`), in the air, in combat,
    /// hostiles near, over-encumbered without Long Haul [documented: GetPCCanFastTravel,
    /// "the same check that the game does"]. In the air is the player's IsInAir;
    /// "in combat" the player's combat state, where the map reads the battle music
    /// [hypothesis: the same moments].
    pub(crate) fn can_fast_travel(&mut self, ctx: &RunCtx) -> bool {
        let p = FormId::PLAYER_REF;
        if self.player.fast_travel.travel_disabled {
            return false;
        }
        let cell = self.world.peek(p).and_then(|s| s.cell);
        // The player's Fast Travel From Anywhere lifts it, as on the map [design: docs/design/tweaks.md].
        if !self.tweaks.unsafe_fast_travel && cell.is_some_and(|c| self.place_forbids_travel(Some(c), self.world.placed.world_of(c))) {
            return false;
        }
        if matches!(crate::nvse::call(self, ctx, "isinair", Some(p), &[]), Some(Ok(XVal::Num(x))) if x != 0.0) {
            return false;
        }
        if self.awareness.get(p).is_some_and(|a| a.state.in_combat()) {
            return false;
        }
        let indoors = cell.is_some_and(|c| self.world.placed.world_of(c).is_none());
        let distance = crate::combat::setting(self, ["fHostileActorExteriorDistance", "fHostileActorInteriorDistance"][usize::from(indoors)], [2048.0, 1024.0][usize::from(indoors)]);
        let near: Vec<FormId> = self.package_actors().into_iter().filter(|&a| distance_between(self, p, a).is_some_and(|d| d <= distance)).collect();
        if near.into_iter().any(|a| !self.is_dead(a) && crate::rest::hostile_to_player(self, a)) {
            return false;
        }
        let over = self.world.encumbrance(p).is_ok_and(|e| e.over);
        !(over && !self.player_has_entry(crate::perks::HAS_FAST_TRAVEL_ALWAYS))
    }

    /// Whether the player sees `a` as invisible: Invisibility or Chameleon above 0 and no
    /// Improved Detection [documented: IsActorInvisibleToPlayer].
    fn invisible_to_player(&mut self, a: FormId) -> bool {
        let codes = [crate::functions::actor::av_code(self, "Invisibility").ok(), crate::functions::actor::av_code(self, "Chameleon").ok()];
        let hidden = codes.iter().flatten().any(|&c| crate::functions::actor::actor_value(self, a, c).unwrap_or(0.0) > 0.0);
        hidden && !self.player_has_entry(crate::perks::HAS_IMPROVED_DETECTION)
    }
}

/// The 3-D distance between two references in the same space.
pub(crate) fn distance_between(g: &Game, a: FormId, b: FormId) -> Option<f64> {
    let at = |r: FormId| g.world.peek(r).map(|s| (s.cell, s.pos)).or_else(|| g.world.placed.get(r).map(|p| (p.cell, p.pos)));
    let ((c1, p1), (c2, p2)) = (at(a)?, at(b)?);
    let space = |c: Option<FormId>| c.map(|c| g.world.placed.world_of(c).unwrap_or(c));
    if c1.is_none() || space(c1) != space(c2) {
        return None;
    }
    Some((0..3).map(|i| (f64::from(p1[i]) - f64::from(p2[i])).powi(2)).sum::<f64>().sqrt())
}

/// An item's value at its condition: `fItemConditionValueBase`, `…Mult` and `…Exp` over
/// the condition's tens of percent, as the barter menu prices it
/// (`nvf_play::menu::barter::Prices::value_at`; the GECK wiki's defaults 0, 0.0316, 1.5).
/// An item with a health and no condition of its own is at full condition, and the
/// formula holds there too: a fresh 10mm pistol (Value 750) is worth 749, not 750
/// [observed 2026-11-08: showoff5.itemvalue.pistol, 750 · 0.0316 · 10^1.5 = 749.46]; an
/// item with no health keeps its Value.
fn value_at(g: &Game, value: f64, condition: Option<f32>, has_health: bool) -> f64 {
    let Some(c) = condition.or(has_health.then_some(1.0)) else { return value };
    let s = |n: &str, d: f64| crate::combat::setting(g, n, d);
    let (base, mult, exp) = (s("fItemConditionValueBase", 0.0), s("fItemConditionValueMult", 0.0316), s("fItemConditionValueExp", 1.5));
    let tens = f64::from(c.clamp(0.0, 1.0)) * 10.0;
    value * (base + (1.0 - base) * mult * tens.powf(exp))
}

/// The AP setting of a weapon's animation type [documented: the `fActionPointsAttack…`
/// settings, FalloutNV.esm's values as fallbacks; `fnv_combat::vats::shot_ap_cost`].
/// The animation type is DNAM's first field: 0 Hand to Hand, 1 and 2 melee, 3 and 4
/// pistols, 5 to 7 rifles, 8 Handle, 9 Launcher, 10 Grenade Throw, 11 Land Mine, 12 Mine
/// Drop, 13 Thrown [documented: schemas/fnv objects.json WeaponDNAM].
fn anim_ap(g: &Game, anim: i64) -> f64 {
    let (name, d) = match anim {
        0 => ("fActionPointsAttackUnarmed", 22.0),
        1 => ("fActionPointsAttackOneHandMelee", 15.0),
        2 => ("fActionPointsAttackTwoHandMelee", 30.0),
        3 | 4 => ("fActionPointsAttackPistol", 20.0),
        8 => ("fActionPointsAttackHandle", 20.0),
        9 => ("fActionPointsAttackLauncher", 40.0),
        10 | 13 => ("fActionPointsAttackGrenade", 35.0),
        11 | 12 => ("fActionPointsAttackMine", 10.0),
        _ => ("fActionPointsAttackRifle", 25.0),
    };
    crate::combat::setting(g, name, d)
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let lvl_open = g.menu_open(crate::game::menus::LEVEL_UP);
    let v = match name {
        // ---- the level-up menu ----
        // -1 closed, 0 skills, 1 perks [documented: GetLevelUpMenuCurrentPage, alias
        // GetLevelUpMenuPage].
        "getlevelupmenucurrentpage" | "getlevelupmenupage" => XVal::Num(if lvl_open { f64::from(g.showoff.b5.levelup.page.unwrap_or(0)) } else { -1.0 }),
        // `page`: 0 skills, 1 perks, 2 close; 0 when the menu is closed or already on the
        // page [documented: SetLevelUpMenuCurrentPage, alias SetLevelUpMenuPage]. The
        // engine changes the page (a perks page with nothing to take closes the menu).
        "setlevelupmenucurrentpage" | "setlevelupmenupage" => {
            let p = num(args, 0, -1.0);
            if !lvl_open || !(0.0..=2.0).contains(&p) || p.fract() != 0.0 || g.showoff.b5.levelup.page == Some(p as u8) {
                return Some(Ok(zero));
            }
            g.showoff.b5.levelup.page_request = Some(p as u8);
            yes(true)
        }
        // `[checkPerks] [checkAssigned]`: the skill points or perks to allot, or allotted;
        // -1 with the menu closed [documented: GetLevelUpMenuPoints].
        "getlevelupmenupoints" => {
            if !lvl_open {
                return Some(Ok(XVal::Num(-1.0)));
            }
            let i = usize::from(num(args, 0, 0.0) != 0.0) * 2 + usize::from(num(args, 1, 0.0) != 0.0);
            XVal::Num(f64::from(g.showoff.b5.levelup.points[i]))
        }
        // `canExitEarly`: 1 lets the menu be left with points unallotted; 0 does not undo
        // it [documented: SetLevelUpMenuCanExitEarly, "does not currently work"]. 1 when
        // the menu is open.
        "setlevelupmenucanexitearly" => {
            if !lvl_open {
                return Some(Ok(zero));
            }
            if num(args, 0, 0.0) != 0.0 {
                g.showoff.b5.levelup.can_exit_early = true;
            }
            yes(true)
        }
        // `[numPerks] [title]`: the menu on its perks page alone, no level gained; the
        // level's perks by default (1 at a perk level, else 0, which only browses); 0 when
        // no perk is left to take [documented: ShowPerkMenu]. Allowed when no menu is open
        // or when SkillMenu / LevelUpMenu (`menus::LEVEL_UP`) is already open [observed:
        // so_36_show_perk_menu_while_open: 1,1,1]; refused while any other menu is open so
        // `Game::open_menu` does not overwrite another menu without firing its close event
        // [hypothesis: other menus unprobed; design: preserve open-menu lifecycle]. Same-frame
        // GetLevelUpMenuPoints 1 0 reads 0 [observed: so_26_levelup_perk_menu_points:
        // 1,0,0,1,-1].
        "showperkmenu" => {
            let level = g.player_level() as i32;
            let sheet = crate::levelup::PerkSheet::of_player(g, level);
            if !sheet.available.contains(&true) || g.menu.is_some_and(|m| m != crate::game::menus::LEVEL_UP) {
                return Some(Ok(zero));
            }
            let n = num(args, 0, -1.0) as i32;
            let count = if n > -1 { n } else { i32::from(crate::levelup::perk_due(g, level)) * crate::levelup::PERKS_PER_LEVEL as i32 };
            g.showoff.b5.levelup.open_as = Some(LevelUpOpen { perks_only: true, count: Some(count), title: text(args, 1) });
            g.showoff.b5.levelup.page = Some(1);
            g.showoff.b5.levelup.points = [0, 0, 0, 0];
            g.open_menu(crate::game::menus::LEVEL_UP);
            yes(true)
        }
        // `[numSkills] [showDespiteNoPoints] [title]`: the skills page as the last page;
        // the level's points by default; with no points and showDespiteNoPoints 0 it does
        // not open (0) [documented: ShowSkillMenu, alias ShowSkillsMenu].
        "showskillmenu" | "showskillsmenu" => {
            if g.menu.is_some() {
                return Some(Ok(zero));
            }
            let level = g.player_level() as i32;
            let n = num(args, 0, -1.0) as i32;
            let points = if n > -1 { n } else { crate::levelup::points_for(g, level) };
            if points <= 0 && num(args, 1, 1.0) == 0.0 {
                return Some(Ok(zero));
            }
            g.showoff.b5.levelup.open_as = Some(LevelUpOpen { perks_only: false, count: Some(points), title: text(args, 2) });
            // The page and the points read at once, in the same script [observed 2026-11-08:
            // showoff5.levelup.skillmenu, ShowSkillMenu 3 then page 0, points 3, allotted 0,
            // perks 0]; the engine keeps them from its first frame on.
            g.showoff.b5.levelup.page = Some(0);
            g.showoff.b5.levelup.points = [points, 0, 0, 0];
            g.open_menu(crate::game::menus::LEVEL_UP);
            yes(true)
        }
        // `[mode]`: the pause menu, on its main page or 1 load, 3 settings; NVForever's
        // pause menu has no save or help page, so 2 and 4 open the main page [documented:
        // ShowPauseMenu's modes; hypothesis for 2 and 4]. The engine opens it.
        "showpausemenu" => {
            let m = num(args, 0, 0.0);
            g.showoff.b5.pause_mode = if (0.0..=4.0).contains(&m) { m as u8 } else { 0 };
            g.showoff.b5.pause_asks += 1;
            zero
        }
        // ---- live explosions [documented: Get/SetExplosionRefRadius, Get/SetExplosionRefSource] ----
        // -1 for a reference that is no live explosion [observed 2026-11-08:
        // showoff5.explosion.none, the player].
        "getexplosionrefradius" => XVal::Num(subject.and_then(|r| g.explosion_radius(r)).map_or(-1.0, f64::from)),
        "setexplosionrefradius" => match subject.and_then(|r| g.showoff.b5.explosions.get_mut(&r)) {
            Some(e) => {
                e.radius = num(args, 0, 0.0).max(0.0) as f32;
                yes(true)
            }
            None => zero,
        },
        "getexplosionrefsource" => XVal::Ref(subject.and_then(|r| g.explosion_source(r)).map_or(0, |s| s.0)),
        "setexplosionrefsource" => match subject.and_then(|r| g.showoff.b5.explosions.get_mut(&r)) {
            Some(e) => {
                e.source = form(args, 0);
                yes(true)
            }
            None => zero,
        },
        // ---- projectiles ----
        // A shot that hit something and stays [documented: GetProjectileRefIsStuck, the
        // "isStuck" flag; hypothesis: NVForever's shots stop where they hit].
        "getprojectilerefisstuck" => yes(subject.and_then(|r| g.gaps.projs.shots.get(&r)).is_some_and(|s| s.impact.is_some())),
        // `proj [ignoreGravity] [copyData]`: a second projectile following a shot
        // [documented: SpawnTracingProjectile]; the engine has no request to fire a bare
        // projectile, so none is spawned (0) [not built: ext-showoff.md §8 follow-ups].
        "spawntracingprojectile" => XVal::Ref(0),
        // `[forceDraw]`: hitscan shots drawn as in VATS (1) or not (0); -1 or nothing reads;
        // whether they were [documented: SetForceDrawHitscanProjectiles].
        "setforcedrawhitscanprojectiles" => {
            let was = g.showoff.b5.force_draw_hitscan;
            let x = num(args, 0, -1.0);
            if x == 0.0 || x == 1.0 {
                g.showoff.b5.force_draw_hitscan = x == 1.0;
            }
            yes(was)
        }
        // `[chanceOverride]`: -1 the chance rolls, 0 no tracers, 1 always one where the
        // projectile has a chance; -2 or less (the default) changes nothing; the override
        // before [documented: SetProjectileTracerChanceOverride].
        "setprojectiletracerchanceoverride" => {
            let before = match g.showoff.b5.tracer_override {
                None => -1.0,
                Some(false) => 0.0,
                Some(true) => 1.0,
            };
            let x = num(args, 0, -2.0);
            if x == -1.0 {
                g.showoff.b5.tracer_override = None;
            } else if x == 0.0 || x == 1.0 {
                g.showoff.b5.tracer_override = Some(x == 1.0);
            }
            XVal::Num(before)
        }
        // ---- fast travel ----
        // EnableFastTravel 0's override [documented: GetPCHasScriptedFastTravelOverride,
        // "can be granted via EnableFastTravel"; observed: `so_17_fast_travel_override_and_can`
        // (`wave4a_extenders.json`), see docs/spec-notes/ext-showoff.md §6].
        "getpchasscriptedfasttraveloverride" => yes(g.player.fast_travel.travel_disabled),
        "getpccanfasttravel" => yes(g.can_fast_travel(ctx)),
        // ---- detection ----
        "isactorinvisibletoplayer" => match subject {
            Some(a) => yes(g.invisible_to_player(a)),
            None => zero,
        },
        // `[checkInvisibility] [setFlashing]`: kept for the session; the engine's VATS
        // highlight does not draw these yet [documented: HighlightAdditionalReferenceAlt;
        // not built: the drawing]. 0 for an invisible actor when checked.
        "highlightadditionalreferencealt" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            if num(args, 0, 1.0) != 0.0 && g.invisible_to_player(r) {
                return Some(Ok(zero));
            }
            if !g.showoff.b5.highlighted.contains(&r) {
                g.showoff.b5.highlighted.push(r);
            }
            if num(args, 1, 1.0) != 0.0 {
                g.showoff.b5.flashing = Some(r);
            }
            yes(true)
        }
        // `[maxDistance] [flags]`: the actors in combat that are the caller's targets (2) or
        // allies (1), 0 meaning 3; 4 no invisible ones, 8 only those the caller detects
        // [documented: GetNumCombatActorsFromActor]. An ally is another actor in combat
        // that fights one of the caller's targets and would not attack the caller
        // [hypothesis: the page does not define one].
        "getnumcombatactorsfromactor" => {
            let Some(me) = subject else { return Some(Ok(zero)) };
            let max = num(args, 0, 0.0);
            let flags = match num(args, 1, 0.0) as i64 {
                0 => 3,
                f => f,
            };
            let mine: Vec<FormId> = g.awareness.get(me).filter(|a| a.state.in_combat()).map(|a| a.targets.keys().copied().collect()).unwrap_or_default();
            let mut others: Vec<FormId> = g.package_actors().into_iter().collect();
            others.push(FormId::PLAYER_REF);
            others.sort();
            others.dedup();
            let mut n = 0;
            for a in others {
                if a == me || g.is_dead(a) || max > 0.0 && distance_between(g, me, a).is_none_or(|d| d > max) {
                    continue;
                }
                let target = mine.contains(&a);
                let ally = !target && g.awareness.get(a).is_some_and(|x| x.state.in_combat() && x.targets.keys().any(|t| mine.contains(t))) && !g.would_attack(a, me);
                if !(target && flags & 2 != 0 || ally && flags & 1 != 0) {
                    continue;
                }
                if flags & 4 != 0 && g.invisible_to_player(a) && me == FormId::PLAYER_REF {
                    continue;
                }
                if flags & 4 != 0 && me != FormId::PLAYER_REF {
                    let codes = [crate::functions::actor::av_code(g, "Invisibility").ok(), crate::functions::actor::av_code(g, "Chameleon").ok()];
                    if codes.iter().flatten().any(|&c| crate::functions::actor::actor_value(g, a, c).unwrap_or(0.0) > 0.0) {
                        continue;
                    }
                }
                if flags & 8 != 0 && !g.detection.get(me, a).is_some_and(|d| d.band == crate::detection::Band::Detected) {
                    continue;
                }
                n += 1;
            }
            XVal::Num(f64::from(n))
        }
        // ---- animations the engine does not play on request yet ----
        // `[checkIgnoreCrippledLimbs]`: 1 when the actor has a hit location (an OnHit
        // handler's) and, when asked, no Ignore Crippled Limbs [documented:
        // ForceHitStaggerReaction, alias ForceHitReaction]; the stagger is not played [not
        // built: an animation request to the engine].
        "forcehitstaggerreaction" | "forcehitreaction" => {
            let Some(a) = subject else { return Some(Ok(zero)) };
            let hit = g.extra.get(&a).and_then(|e| e.last_hit).is_some_and(|h| h.location >= 0);
            let ignore = num(args, 0, 0.0) != 0.0 && crate::functions::actor::av_code(g, "IgnoreCrippledLimbs").ok().is_some_and(|c| crate::functions::actor::actor_value(g, a, c).unwrap_or(0.0) > 0.0);
            yes(hit && !ignore)
        }
        // The recoil is not played [not built; documented: ForceRecoilAnim, "typically only
        // works for melee/unarmed weapons"]. Returns 1 for a living actor unarmed/melee
        // (`animation_type <= 2` or no equipped weapon) and 0 with a ranged gun equipped
        // [observed 2026-11-08: showoff5.recoil.player 1 unarmed; observed: Fallout: New
        // Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE 184, 2026-11-23/24,
        // `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters,
        // `so_33_force_recoil_unarmed_vs_gun` (unarmed 1, holstered 10mm Pistol 0,
        // ForceHitReaction 0) and `w4d_38_showoff_perk_stagger_fade`
        // (`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3679):
        // `stag(0,0)|rec(1,1,0)` (stagger on Player/Doc Mitchell; recoil unarmed, Baseball
        // Bat, 10mm Pistol); no DLL or source consulted].
        "forcerecoilanim" => {
            let Some(a) = subject else { return Some(Ok(zero)) };
            let actor = a == FormId::PLAYER_REF || matches!(g.world.placed.get(a).map(|p| &p.sig), Some(b"ACHR" | b"ACRE"));
            let melee_or_unarmed = match crate::functions::items::equipped_weapon(g, a) {
                None => true,
                Some(w) => crate::nvse::record_value(g, w, "DNAM", "animation_type")
                    .and_then(|v| v.num().ok())
                    .is_none_or(|t| t <= 2.0),
            };
            yes(actor && !g.is_dead(a) && melee_or_unarmed)
        }
        // `[killer]`: KillActor on every loaded actor hostile to the player [documented:
        // KillAllHostiles; hypothesis: "within range" is the loaded area, as
        // KillAllActors]; essential ones are knocked out, as KillActor does.
        "killallhostiles" => {
            let killer: Vec<String> = form(args, 0).map(|k| vec![format!("{:08X}", k.0)]).unwrap_or_default();
            let all: Vec<FormId> = g.package_actors().into_iter().collect();
            for a in all {
                if a != FormId::PLAYER_REF && !g.is_dead(a) && g.present(a) && crate::rest::hostile_to_player(g, a) {
                    if let Err(e) = crate::functions::console::call_words(g, Some(a), "KillActor", &killer, "KillAllHostiles") {
                        return Some(Err(e));
                    }
                }
            }
            zero
        }
        // ---- values ----
        // `[accountForBarter]` on an item reference: its value at its condition plus the
        // attached weapon mods' IMOD values (which condition does not lower [observed:
        // o26_value, docs/spec-notes/weapon-mods.md §6, `FormStore::mods_value`]); with
        // the flag, on an inventory reference while the barter menu is open, multiplied
        // by the active trade's buy/sell multiplier (`SetBarterPriceMult` override, else
        // live `BarterView::mult`, else the Barter skill GMST fallback) [documented:
        // GetCalculatedItemValue; hypothesis: Calculate Buy Price perks outside a live
        // BarterView are not counted]. -1 for no item.
        "getcalculateditemvalue" => {
            let Some(r) = subject else { return Some(Ok(XVal::Num(-1.0))) };
            let (holder, item, condition, mods) = match g.temp_item(r) {
                Some((c, i)) => {
                    let (cond, mods) = g
                        .world
                        .peek(c)
                        .and_then(|s| s.inventory.as_ref())
                        .and_then(|inv| inv.groups().find(|x| x.0 == i).map(|x| (x.1.condition, x.1.mods)))
                        .unwrap_or((None, 0));
                    (Some(c), i, cond, mods)
                }
                None => match crate::functions::base_of(g, r) {
                    Some(b) => {
                        let (cond, mods) = match g.world.peek(r) {
                            Some(s) => (s.extra.condition, s.extra.mods),
                            None => (g.world.placed.get(r).and_then(|p| p.health), 0),
                        };
                        (None, b, cond, mods)
                    }
                    None => return Some(Ok(XVal::Num(-1.0))),
                },
            };
            let Some(it) = g.world.resolve_item(item) else { return Some(Ok(XVal::Num(-1.0))) };
            let Some(value) = g.forms.item_values.get(&item).copied().or(it.value) else { return Some(Ok(XVal::Num(-1.0))) };
            let has_health = g.world.effective_health(item).is_some_and(|h| h > 0);
            let mut v = value_at(g, f64::from(value), condition, has_health) + f64::from(g.world.forms.mods_value(g.unclone(item), mods));
            if num(args, 0, 0.0) != 0.0 && holder.is_some() && g.menu_open(crate::game::menus::BARTER) {
                let side = usize::from(holder == Some(FormId::PLAYER_REF));
                let mult = g
                    .other
                    .jip12
                    .price_mults[side]
                    .or_else(|| g.other.jip12.barter.as_ref().map(|b| b.mult[side]))
                    .map(f64::from)
                    .unwrap_or_else(|| {
                        let skill = crate::functions::actor::actor_value(g, FormId::PLAYER_REF, crate::actor_values::BARTER).unwrap_or(0.0);
                        let s = |n: &str, d: f64| crate::combat::setting(g, n, d);
                        if side == 1 {
                            (s("fBarterSellBase", 0.45) + skill * s("fBarterSellMult", 0.45) / 100.0).min(1.0)
                        } else {
                            (s("fBarterBuyBase", 1.55) + skill * s("fBarterBuyMult", -0.45) / 100.0).max(1.0)
                        }
                    });
                v *= mult;
            }
            // Rounded to nearest whole number [observed: o26_value Varmint Rifle
            // 74.946 -> 75 and 48.679 -> 49; showoff5.itemvalue.pistol 749.46 -> 749].
            XVal::Num(v.round())
        }
        // `whatAttack [weapon]`: the AP of the player's weapon (fists without one): 0 the
        // weapon's own (its override, else its animation type's setting), 1 a melee
        // weapon's VATS attack AP or an unarmed weapon's fUpperCutAPCost, 2 an unarmed
        // weapon's fCrossAPCost [documented: GetCalculatedAPCost, alias
        // GetCalculatedWeaponAttackAPCost]. AP perks are not counted [hypothesis].
        "getcalculatedapcost" | "getcalculatedweaponattackapcost" => {
            let w = form(args, 1).or_else(|| crate::functions::items::equipped_weapon(g, FormId::PLAYER_REF)).unwrap_or(FormId(0x1F4));
            let field = |g: &Game, s: &str, p: &str| crate::nvse::record_value(g, w, s, p);
            let anim = match field(g, "DNAM", "animation_type") {
                Some(XVal::Num(t)) => t as i64,
                _ => 0,
            };
            // Fists (the player's weapon when none is equipped) cost the unarmed setting,
            // their own override AP 18 aside [observed 2026-11-08: showoff5.ap.fists 22,
            // FalloutNV.esm's fActionPointsAttackUnarmed]; another weapon's override counts
            // [observed: ap.pistol 17, the 10mm pistol's override].
            let fists = w == FormId(0x1F4);
            let own = || {
                let over = !fists && matches!(field(g, "DNAM", "flags2"), Some(XVal::Num(f)) if (f as u64) & (1 << 3) != 0);
                match field(g, "DNAM", "override_action_points") {
                    Some(XVal::Num(ap)) if over => ap,
                    _ => anim_ap(g, anim),
                }
            };
            // 1 and 2 give 0 for the fists and for a gun [observed 2026-11-08: ap.fists and
            // ap.pistol "x 0 0", the unarmed uppercut and cross settings not read]; a melee
            // weapon's 1 is its VATS attack's AP [documented; not observed].
            let melee = anim == 1 || anim == 2;
            let ap = match num(args, 0, 0.0) as i64 {
                1 if melee => match field(g, "VATS", "ap") {
                    Some(XVal::Num(x)) if x > 0.0 => x,
                    _ => 0.0,
                },
                1 | 2 => 0.0,
                _ => own(),
            };
            XVal::Num(ap.round())
        }
        // fVATSMaxEngageDistance [documented: GetVATSMaxEngageDistance; the Tweaks
        // setting is not NVForever's]; 5000 when no plugin sets it [observed 2026-11-08:
        // showoff5.vats.engage 5000 with the base game and DLCs, which do not].
        "getvatsmaxengagedistance" => XVal::Num(crate::combat::setting(g, "fVATSMaxEngageDistance", 5000.0)),
        // Whether VATS could target the reference: enabled and present, an actor alive,
        // another reference by SetVATSTargetable or its destruction data [documented:
        // GetVATSTargetable, "if passing a disabled reference, this function returns
        // false"; `Game::vats_targetable`].
        "getvatstargetable" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            if !g.world.is_enabled(r).unwrap_or(false) || !g.present(r) {
                return Some(Ok(zero));
            }
            // The player counts [observed 2026-11-08: showoff5.vats.target.player 1]; an
            // actor out of the loaded cells does not [observed: vats.target.doc 0, Doc
            // Mitchell in his house while the player stood at the airport test ground;
            // hypothesis: the unloaded cell is the reason].
            if r == FormId::PLAYER_REF {
                return Some(Ok(yes(!g.is_dead(r))));
            }
            let actor = matches!(g.world.placed.get(r).map(|p| &p.sig), Some(b"ACHR" | b"ACRE"));
            yes(if actor { !g.is_dead(r) && crate::functions::refs::in_loaded_cell(g, r) } else { g.vats_targetable(r) })
        }
        // A key's character on the user's keyboard layout [documented: ToANSIChar]: the
        // layout is the operating system's, which no game→engine channel carries, so ""
        // [decision: the owner's 2026-11-08 rule for the user's language; ext-showoff.md
        // §8 follow-ups].
        "toansichar" => XVal::Str(String::new()),
        // GetHitLocation already keeps the last hit's location until the next hit
        // [documented: GetHitLocationLingering, a location that lingers;
        // `functions::actor::get_hit_location`].
        "gethitlocationlingering" => XVal::Num(subject.and_then(|r| g.extra.get(&r)).and_then(|e| e.last_hit).map_or(-1, |h| h.location) as f64),
        // Aiming down a scope that has night vision at night [documented:
        // PlayerHasNightVisionActive; the weapon's flag "Scope has NightVision"]. Night is
        // 20:00 to 6:00 [hypothesis: the page says only "at night"].
        "playerhasnightvisionactive" => {
            let scoped = matches!(crate::nvse::call(g, ctx, "getpcusingscope", None, &[]), Some(Ok(XVal::Num(x))) if x != 0.0);
            let flag = crate::functions::items::equipped_weapon(g, FormId::PLAYER_REF).is_some_and(|w| matches!(crate::nvse::record_value(g, w, "DNAM", "flags2"), Some(XVal::Num(f)) if (f as u64) & (1 << 12) != 0));
            let hour = g.world.global(FormId(0x38)).map_or(12.0, f64::from);
            yes(scoped && flag && !(6.0..20.0).contains(&hour))
        }
        // Within [`DRINK_SECONDS`] of a drink from placed water [documented:
        // PlayerIsDrinkingPlacedWater; `Game::drink_from`].
        "playerisdrinkingplacedwater" => yes(g.showoff.b5.last_drink.is_some_and(|t| g.time - t < DRINK_SECONDS)),
        // A reference under the crosshair within reach, which the prompt names, and no
        // menu open [documented: GetIsActivationPromptShown; hypothesis: the prompt shows
        // for whatever the crosshair finds within reach].
        "getisactivationpromptshown" => yes(g.senses.crosshair_ref.is_some() && g.menu.is_none()),
        // The player's rotation changed since the last frame, and looking is not disabled
        // [documented: IsPlayerLookingAround; hypothesis: any turn counts].
        "isplayerlookingaround" => yes(g.showoff.b5.looking && !g.player.controls_disabled[4]),
        // `ingestible`: the usage monitor's count times its magnitude while it lasts
        // [documented: GetSpellUsageNumEx]. NVForever's monitor forgets the count when it
        // runs out, so after that it reads 0 as GetSpellUsageNum [hypothesis: the decay
        // the page mentions is the monitor's own].
        "getspellusagenumex" => match (subject, form(args, 0)) {
            (Some(a), Some(i)) => XVal::Num(g.spell_usage(a, i)),
            _ => zero,
        },
        // `form [count] [minDistance] [maxDistance] [health]`: placed where the crosshair
        // points, at least minDistance and at most maxDistance along it (nothing hit: the
        // maximum) [documented: PlaceAtReticleAlt, the arguments; hypothesis: how the two
        // distances bound the point]. The new reference.
        "placeatreticlealt" => {
            let Some(f) = form(args, 0) else { return Some(Ok(XVal::Ref(0))) };
            let count = num(args, 1, 1.0).max(1.0) as i32;
            let max = num(args, 3, f64::from(crate::functions::senses::RETICLE_REACH)) as f32;
            let min = (num(args, 2, 0.0) as f32).min(max);
            let Some(ret) = g.senses.reticle else { return Some(Ok(XVal::Ref(0))) };
            let d = ret.hit.unwrap_or(max).clamp(min.max(0.0), max.max(0.0));
            let at = [ret.start[0] + ret.dir[0] * d, ret.start[1] + ret.dir[1] * d, ret.start[2] + ret.dir[2] * d];
            let cell = g.cell_of(FormId::PLAYER_REF);
            match g.world.place(f, cell, at, [0.0; 3], count) {
                Ok(r) => {
                    if let Some(XVal::Num(h)) = args.get(4).filter(|v| matches!(v, XVal::Num(h) if *h >= 0.0)) {
                        if let Ok(st) = g.world.get(r) {
                            st.extra.condition = Some((h / 100.0).min(1.0) as f32);
                        }
                    }
                    XVal::Ref(r.0)
                }
                Err(_) => XVal::Ref(0),
            }
        }
        // `count`: ITR's RefillAmmo on the player [documented: RefillPlayerAmmo,
        // deprecated for RefillAmmo, which does the same for any actor].
        "refillplayerammo" => return crate::nvse::call(g, ctx, "refillammo", Some(FormId::PLAYER_REF), args),
        // `[flags] [typeCode] [target] [exceptions]`: RemoveAllTypedItems with flags 1
        // keep ownership, 2 no messages, 4 the player's quest items too, 8 non-playable
        // biped items too [documented: RemoveAllItemsShowOff's table]. 1 when done, 0
        // without a container [documented].
        "removeallitemsshowoff" => {
            let Some(from) = subject else { return Some(Ok(zero)) };
            let flags = num(args, 0, 0.0) as i64;
            let code = num(args, 1, -1.0) as i32;
            let except = form(args, 3).map(|l| crate::functions::refs::list_members(g, l)).unwrap_or_default();
            let how = crate::functions::items::RemoveAll { to: form(args, 2), retain: flags & 1 != 0, silent: flags & 2 != 0, code, except, quest_items: flags & 4 != 0, unplayable: flags & 8 != 0 };
            match crate::functions::items::remove_all(g, from, &how) {
                Ok(()) => yes(true),
                Err(e) => return Some(Err(e)),
            }
        }
        _ => return None,
    };
    Some(Ok(v))
}
