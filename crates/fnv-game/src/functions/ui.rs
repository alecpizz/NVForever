//! Functions that ask something of the Pip-Boy, the terminal and the character-creation
//! menus, and the scripted challenges (docs/design/script-stubs.md §3). The game keeps
//! the request ([`UiRequests`], `crate::terminal::Terminals::back`); the engine's menus
//! take it.

use super::{none, Args, Ctx};
use crate::game::{menus, Game};
use fnv_script::vm::Value;
use fnv_sim::FormId;

/// How a script opened the S.P.E.C.I.A.L. menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialMenu {
    /// AddSPECIALPoints: this many points more to give out, none to take back.
    Add(i32),
    /// SetSPECIALPoints: the attributes add up to this when the menu is left.
    Set(i32),
}

/// What scripts asked of the menus, for the engine to take.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UiRequests {
    /// ForceActiveQuest: the quest to make the Pip-Boy's active quest, and how many
    /// times a quest was forced (the Pip-Boy takes each new count once).
    pub active_quest: Option<FormId>,
    pub active_quest_count: u64,
    /// The rules of the next S.P.E.C.I.A.L. menu (`crate::chargen::SpecialSheet::for_menu`).
    pub special: Option<SpecialMenu>,
    /// AddTagSkills: skills to tag in the next tag-skill menu besides those tagged
    /// (`crate::chargen::TagSheet::for_menu`).
    pub extra_tags: Option<usize>,
    /// SetTagSkills: how many skills the next tag-skill menu has tagged in all, and
    /// whether the skills tagged before show (`crate::chargen::TagSheet::for_menu`).
    pub set_tags: Option<(usize, bool)>,
    /// SetGlobalTimeMultiplier: the game's speed (`None`: never set, 1). The engine
    /// applies it to its clock while no menu holds the game.
    pub time_multiplier: Option<f64>,
    /// SetGlobalTimeMultiplier's keepSoundUnchanged: sounds keep their speed; otherwise
    /// they play at the multiplier too.
    pub time_keeps_sound: bool,
    /// Autosave calls the engine has not taken yet.
    pub autosaves: u32,
    /// SystemSave and ForceSave calls the engine has not taken yet.
    pub system_saves: u32,
    pub force_saves: u32,
    /// The console's camera functions (`super::camera`).
    pub camera: super::camera::CameraRequests,
    /// PlayBink calls the engine's film player has not taken yet, in call order.
    pub films: Vec<FilmRequest>,
    /// The screen splatter functions' requests (`super::misc::trigger_screen_splatter`).
    pub splatter: SplatterRequests,
    /// ResetPipboyManager calls: the Pip-Boy takes each new count once
    /// (`nvf_play::pipboy`).
    pub pipboy_resets: u64,
    /// Engine-synced Pip-Boy transition override for `GetPipBoyMode` (`Some(2)` opening,
    /// `Some(4)` closing, `Some(0)` while covered during startup warming; `None` in resting
    /// `Down`/`Up` phases and headless runs, where `GetPipBoyMode` evaluates live
    /// `Game::menu_open(menus::PIPBOY)` -> `3` / `0`) [design].
    pub pipboy_mode: Option<u8>,
    /// ExitGame calls the engine has not taken yet (`nvf_play::script`).
    pub exit_game: u32,
    /// EnableLoadingMenu 1 this frame ([`enable_loading_menu`]): the engine takes it
    /// after the frame's scripts (`nvf_play::script`).
    pub loading_menu: bool,
    /// TriggerHitShader calls not taken yet: each one's strength (`super::fo3`). Nothing
    /// draws them yet.
    pub hit_shaders: Vec<f32>,
    /// FadeSFX calls not taken yet: each one's seconds (`super::fo3`). Nothing plays by
    /// them yet.
    pub sfx_fades: Vec<f32>,
    /// DisableAllMines calls so far: the engine disables the mines of the player's cell
    /// on each new count (`nvf_play::mines`).
    pub mines_disabled: u64,
    /// KillQuestUpdates calls so far: the HUD drops the quest lines it has not shown yet
    /// on each new count (`nvf_play::quest_messages`).
    pub quest_updates_killed: u64,
    /// ShowOff's ToggleQuestMessages 0: the HUD shows no quest line (added, completed,
    /// failed, objectives) until it is turned on again (`nvf_play::script`).
    pub quest_messages_off: bool,
    /// SUP's CaptureScreenshot and CaptureScreenshotAlt calls the engine has not taken
    /// yet (`crate::nvse_sup5`; `nvf_play::sup_shots`).
    pub screenshots: Vec<crate::image_files::ShotRequest>,
}

/// A film a script asked for (`PlayBink`), for the engine's film player
/// (`nvf_play::intro`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilmRequest {
    /// The file under the game's `Data` folder, lower case with `\` separators:
    /// `video\` and the script's name with `..` resolved ([`film_path`]).
    pub path: String,
    /// Esc ends it early.
    pub interruptible: bool,
    /// The game's world sounds are silent under it.
    pub mute_audio: bool,
    /// The game's music pauses under it.
    pub pause_music: bool,
    /// Fitted inside the screen with black bars (else stretched over the whole screen).
    pub letterboxed: bool,
}

/// The film file a PlayBink name stands for: the path is relative to `Data\Video\`
/// [documented: GECK wiki PlayBink, "The path to the filename is relative to
/// Fallout3\Data\Video\", its `..\sound\videos\` example]; `None` for an empty name or
/// one that leaves the `Data` folder.
pub fn film_path(name: &str) -> Option<String> {
    let mut parts: Vec<String> = vec!["video".into()];
    for p in name.trim().split(['\\', '/']) {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p.to_ascii_lowercase()),
        }
    }
    let path = parts.join("\\");
    (!path.is_empty() && path != "video").then_some(path)
}

/// One TriggerScreenSplatter call (docs/spec-notes/screen-splatter.md §2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplatterTrigger {
    /// How many splats it puts on the screen.
    pub count: u32,
    /// Seconds each lasts; `None`: the game setting `fBloodSplatterDuration`.
    pub duration: Option<f32>,
    /// Factors on the splats' size and opacity.
    pub size: f32,
    pub opacity: f32,
}

/// What the screen splatter functions asked of the engine (`nvf_play::screen_splatter`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SplatterRequests {
    /// TriggerScreenSplatter calls not taken yet.
    pub triggers: Vec<SplatterTrigger>,
    /// ClearScreenSplatter calls so far (the engine clears on each new count).
    pub clears: u64,
    /// SetScreenSplatterFade: the share of a splat's time after which it starts to
    /// fade, and whether splats fade (`None`: never called, the game settings).
    pub fade: Option<(f32, bool)>,
}

/// `ResetPipboyManager`: called after the player is given a Pip-Boy (Doc Mitchell's line
/// in VCG01) and after a DLC adds its radio stations and map markers. It
/// "(re-)initializes" the Pip-Boy's menus and puts its look back to the default, its
/// light off [documented: GECK wiki ResetPipboyManager, owner export]. NVForever's
/// Pip-Boy builds its pages afresh each time it opens, so the engine
/// (`nvf_play::pipboy`, [`UiRequests::pipboy_resets`]) shows the default page and tabs
/// next time and turns the Pip-Boy's light (NVForever's flashlight) off [hypothesis:
/// what "reset" leaves of the pages chosen].
pub fn reset_pipboy_manager(g: &mut Game, _ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    g.ui.pipboy_resets += 1;
    none()
}

/// `ShowTutorialMenu message`: the message in the tutorial menu ("VDSG Manual"),
/// [`Game::show_tutorial`].
pub fn show_tutorial_menu(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let m = a.form(0)?;
    if !g.data.messages.contains_key(&m) {
        return Err(format!("{} is not a message", g.data.name(m)));
    }
    g.show_tutorial(m);
    none()
}

/// `AutoDisplayObjectives 0/1`: turns the automatic display of the active quest's
/// objectives on or off, for while the Pip-Boy cannot be used [documented: GECK wiki
/// AutoDisplayObjectives, owner export]. VCG01 turns it on before the player has a
/// Pip-Boy and off at its end [observed in the masters]. Kept in
/// `PlayerState::auto_objectives` (saved); the HUD's quest lines keep the objectives on
/// screen while it is on (`nvf_play::quest_messages`) [hypothesis: what the display
/// shows].
pub fn auto_display_objectives(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    g.player.auto_objectives = a.num_or(0, 0.0) != 0.0;
    none()
}

/// `ExitGame`: plays the credits and returns to the main menu [documented: GECK wiki
/// ExitGame, owner export]. The engine takes [`UiRequests::exit_game`] at the end of the
/// frame (`nvf_play::script`): back to the main menu as the pause menu's Quit does; the
/// credits are not built. The one caller is the ending's fade-out (SCPT 0x0017A17D)
/// [observed in the masters].
pub fn exit_game(g: &mut Game, _ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    g.ui.exit_game += 1;
    none()
}

/// `IncrementScriptedChallenge challenge`: one step towards the challenge's threshold
/// [documented: GECK wiki IncrementScriptedChallenge]. At the threshold (CHAL DATA) it
/// is completed (GetChallengeCompleted); a recurring one then starts again from 0, a
/// non-recurring one stays at its threshold [documented: GECK wiki GetChallengeProgress
/// (a ShowOff function), "will return the max Threshold value" / "will return to 0"].
/// A challenge flagged Start Disabled does not move until UnlockChallenge [hypothesis:
/// what "Start Disabled" means for progress]. The step, the completion, the reward
/// script and the notices are `Game::challenge_step` (`crate::challenges`).
pub fn increment_scripted_challenge(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let c = a.form(0)?;
    if !g.data.challenges.contains_key(&c) {
        return Err(format!("{} is not a challenge", g.data.name(c)));
    }
    g.challenge_step(c, 1.0);
    none()
}

/// `Autosave`: the game saves over the current autosave slot [documented: GECK wiki
/// Autosave]. The engine takes the request at the end of the frame (`nvf_play::save`).
pub fn autosave(g: &mut Game, _ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    g.ui.autosaves += 1;
    Ok(Value::ZERO)
}

/// `EnableLoadingMenu flag`: "forces a loading screen to appear for a few seconds",
/// one that "does not use any loading screens specific to the current location"
/// [documented: GECK wiki EnableLoadingMenu]. Both shipped calls are `EnableLoadingMenu
/// 1` in a door activator's OnActivate just before `player.MoveTo` a marker (SCPT
/// 0x00168A8B and 0x00141144, the way in and out of one place) [observed: decompiled
/// scripts, 2026-10-19]; the move brings its own loading screen here, so the engine
/// makes that screen one for anywhere (`nvf_play::loading::Loading::generic_next`).
/// A call with no move after it shows nothing (not built; no shipped call does so).
/// `EnableLoadingMenu 0` asks for nothing [hypothesis: the parameter reads as a switch].
pub fn enable_loading_menu(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    if a.num_or(0, 1.0) != 0.0 {
        g.ui.loading_menu = true;
    }
    none()
}

/// `SystemSave`: the one system save, overwritten by each later call; listed for loading,
/// not offered to save over [documented: GECK wiki SystemSave]. The engine writes it to
/// its own slot (`nvf_play::save`).
pub fn system_save(g: &mut Game, _ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    g.ui.system_saves += 1;
    none()
}

/// `ForceSave`: a new save in a new slot, as if the player saved by hand, with the limits
/// of saving by hand [documented: GECK wiki ForceSave, "it will fail if called while a
/// message box is being displayed"]. Taken by the engine as a pause-menu save
/// (`nvf_play::save`).
pub fn force_save(g: &mut Game, _ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    g.ui.force_saves += 1;
    none()
}

/// `ForceActiveQuest quest`: the quest becomes the active quest [documented: GECK wiki
/// ForceActiveQuest]: the Pip-Boy's Quests list marks it and its notes and map markers
/// follow (`nvf_play::pipboy` takes [`UiRequests::active_quest`]).
pub fn force_active_quest(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let q = a.form(0)?;
    if !g.data.quests.contains_key(&q) {
        return Err(format!("{} is not a quest", g.data.name(q)));
    }
    g.ui.active_quest = Some(q);
    g.ui.active_quest_count += 1;
    none()
}

/// `ForceTerminalBack`: called from a terminal menu item's result script, the terminal
/// goes back one level, or closes from its top level [documented: GECK wiki
/// ForceTerminalBack]. Handed to the engine's terminal by `crate::terminal::choose`.
pub fn force_terminal_back(g: &mut Game, _ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    g.terminals.back = true;
    none()
}

/// `AddSPECIALPoints n`: opens the S.P.E.C.I.A.L. menu with n more points to give out;
/// the attributes cannot go below what they were [documented: GECK wiki
/// AddSPECIALPoints, used by the Intense Training perk]. The menu is the character
/// menu's attributes page, "ATTRIBUTES: 0/1 Points Assigned" [observed: the owner's
/// screenshots after Intense Training, docs/spec-notes/levelup.md §8; documented:
/// char_gen_menu.xml's attribute template and `_OnlyAdd`; the catalogue, "Opens the
/// character-generation Attributes page"], the menu the tag-skill page is (1048)
/// [hypothesis: its MenuMode code is not captured].
pub fn add_special_points(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    g.ui.special = Some(SpecialMenu::Add(a.int(0)?));
    g.open_menu(menus::TAG_SKILLS);
    none()
}

/// `SetSPECIALPoints n`: opens the S.P.E.C.I.A.L. menu, which may be left once the
/// attributes add up to n [documented: GECK wiki SetSPECIALPoints].
pub fn set_special_points(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let mut n = a.int(0)?;
    // The character revision keeps the attributes (`crate::revision`): the menu opens on
    // them with the points they add up to, so the player moves points around, on the
    // character menu's attributes page as Intense Training's, not on the Vigor Tester
    // [design: owner's playtest, #1031].
    let sum: i32 = crate::chargen::SpecialSheet::of_player(g).values.iter().sum();
    let revision = crate::revision::keeps(ctx, FormId::PLAYER_REF, || format!("the S.P.E.C.I.A.L. ({sum} points, not {n}) on the attributes page"));
    if revision {
        n = n.max(sum);
    }
    g.ui.special = Some(SpecialMenu::Set(n));
    // Always the attributes page: the Vigor Tester is ShowLoveTesterMenuParams's menu
    // alone, New Vegas's call at Doc Mitchell's [design: owner 2026-11-02, Tale of Two
    // Wastelands' character rebuild at Vault 101's exit (SetSPECIALPoints 40) showed the
    // Vigor Tester; one function, one menu, instead of telling the calls apart].
    g.open_menu(menus::TAG_SKILLS);
    none()
}

/// `ShowSPECIALBookMenuParams n` (`ssbmp`): Fallout 3's S.P.E.C.I.A.L. book, a menu to
/// distribute or redistribute the points up to `n` in all [documented: GECK wiki
/// ShowSPECIALBookMenuParams]. NVForever has no book menu: it opens the attributes page
/// as SetSPECIALPoints does [design: owner 2026-11-02, "the intense training special
/// menu"; Tale of Two Wastelands' CG01SpecialBookSCRIPT calls `ssbmp 40`].
pub fn show_special_book_menu(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    g.ui.special = Some(SpecialMenu::Set(a.int(0)?));
    g.open_menu(menus::TAG_SKILLS);
    none()
}

/// `AddTagSkills n`: opens the tag-skill menu to tag n more skills; the skills tagged
/// before cannot be untagged [documented: GECK wiki AddTagSkills; its note that only
/// four tags take effect is not applied: the Tag! perk's own rule is the perk system's].
pub fn add_tag_skills(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    g.ui.extra_tags = Some(a.int(0)?.max(0) as usize);
    g.open_menu(menus::TAG_SKILLS);
    none()
}
