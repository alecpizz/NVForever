//! The hand report (docs/design/player-reports.md §3.3; #2151): **Report a bug** at the
//! pause menu's bottom left, over the version (`nvf_play::pause::ReportEntry`; owner
//! 2026-11-12), opens a form over the pause menu, for any player, reports on or off.
//!
//! - **What it holds**: a description (required) and a category (bug, crash, differs from
//!   the original, performance: the issue forms' four); ticked at first: this session so far
//!   (its summary and the world snapshot), the run log's last [`LOG_LINES`] lines with the
//!   flight recorder, and a screenshot of the world as it was before the form opened (shown
//!   as a thumbnail); unticked: a save written for the report (never the player's own
//!   files: `<home>\reports\form\`, deleted once sent; at most [`MAX_SAVE`]) and a contact
//!   name, for us to reply on an issue. It shows each part's size and the total before Send.
//!   "This session so far" is there only with reports on (the summary is theirs).
//! - **Consent**: sending is consent for that one report only; `[Reports] Send` stays as it
//!   is. With reports on, the report carries the install ID (so "Delete my reports" covers
//!   it); off, it carries none.
//! - **Where it goes**: as sessions (§8): `NVF_REPORTS_ENDPOINT`; never from a developer or
//!   test run without it; `[Reports] Endpoint`; the project's server only from an official
//!   build (or `NVF_REPORTS_PRODUCTION=1`). Otherwise the form says why and Send is off.
//! - **Keys**: type into the description (Enter: a new line); Tab moves between the
//!   description and the contact name; Ctrl+Enter sends; Esc closes. A click on a category, a
//!   tick box, a field, Send or Cancel takes it. While the form is up the pause menu takes
//!   no key or click (`nvf_play::pause::FormOver`).
//! - **The answer**: the server's reference (`R-…`) is shown; a refusal or no answer says why and keeps the form to try again.
//!
//! The screenshot is taken the way a save takes its picture (`nvf_play::save`): the
//! interface hidden for the frame. Bevy's own font draws the form [our choice: the text is
//! the player's own, in any script].
//!
//! Test aid `NVF_PLAY_TEST_REPORT_FORM=<description>[|save][|contact=<name>][|category=<id>]`:
//! once the form is open, it is filled so and sent.
//!
//! Log lines: `report form: open (…)`, `report form: sending to <url> (…)`, `report form:
//! sent, reference R-…`, `report form: could not send: …`, `report form: closed`.

use super::upload;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use fnv_install::reports::Reports;
use fnv_install::EngineHome;
use nvf_play::interface::{amber, font_units, u};
use nvf_play::pause::{FormOver, FormTextInput, Paused, ReportEntry};
use nvf_play::ui_pointer::{Button as UiButton, PointerState};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// The categories: the server's name and the form's [the issue forms' four, §3.3].
pub const CATEGORIES: [(&str, &str); 4] = [("bug", "Bug"), ("crash", "Crash"), ("differs", "Differs from the original"), ("performance", "Performance")];
/// The run log's last lines sent [§3.3].
pub const LOG_LINES: usize = 2000;
/// The description's length in characters (the server takes 16 KiB of UTF-16).
pub const MAX_DESCRIPTION: usize = 8000;
/// The contact name's length (a GitHub name is at most 39).
pub const MAX_CONTACT: usize = 50;
/// The save's cap [§3.3, §5.1].
pub const MAX_SAVE: usize = 20 * 1024 * 1024;
/// The screenshot's width at most, and the thumbnail's [our choice].
pub const SHOT_WIDTH: u32 = 1280;
/// The screenshot's size at most: halved until it fits [our choice: well under the
/// server's 2 MiB].
pub const MAX_SHOT: usize = 1536 * 1024;
const THUMB_WIDTH: u32 = 320;
/// Frames the screenshot is waited for before the form opens without one.
const SHOT_WAIT_FRAMES: u32 = 10;
/// Seconds the save is waited for before the report goes without it.
const SAVE_WAIT_SECS: f64 = 30.0;

/// The form wants a fresh world snapshot now (mod.rs `refresh_snapshot`).
static SNAPSHOT_NOW: AtomicBool = AtomicBool::new(false);
/// The form is up (the snapshot is kept while it is, reports on or off).
static OPEN: AtomicBool = AtomicBool::new(false);

/// The snapshot is wanted though reports are off: the form is up or about to be.
pub fn wants_snapshot() -> bool {
    OPEN.load(Ordering::Relaxed) || SNAPSHOT_NOW.load(Ordering::Relaxed)
}

/// Take the form's ask for a snapshot this frame.
pub fn snapshot_now() -> bool {
    SNAPSHOT_NOW.swap(false, Ordering::Relaxed)
}

/// The tick boxes, in the form's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tick {
    Session,
    Log,
    Screenshot,
    Save,
    Contact,
}
const TICKS: [Tick; 5] = [Tick::Session, Tick::Log, Tick::Screenshot, Tick::Save, Tick::Contact];

#[derive(Default, Debug)]
enum Phase {
    #[default]
    Closed,
    /// The screenshot was asked for on this frame.
    Shooting(u32),
    Open,
    /// The save is being written to this path (since these real seconds).
    Saving(PathBuf, f64),
    Sending,
    /// Sent (the reference is in the status line and the settings row).
    Sent,
}

/// What the form sends besides its fields, gathered as it opens.
#[derive(Default)]
struct Gathered {
    summary: Option<Vec<u8>>,
    snapshot: Option<Vec<u8>>,
    log: Option<Vec<u8>>,
    log_lines: usize,
    recorder: Option<Vec<u8>>,
    screenshot: Option<Vec<u8>>,
    shot_size: (u32, u32),
}

/// Where the report goes, or why it cannot ([`endpoint`]).
#[derive(Debug, Clone)]
struct Dest(Result<String, String>);

impl Default for Dest {
    fn default() -> Self {
        Dest(Err(String::new()))
    }
}

#[derive(Resource, Default)]
struct Form {
    phase: Phase,
    category: usize,
    description: String,
    contact: String,
    ticks: [bool; 5],
    /// The keys type into the contact name (else the description).
    on_contact: bool,
    hidden: Vec<(Entity, Visibility)>,
    captured: Arc<Mutex<Option<Image>>>,
    thumb: Option<Handle<Image>>,
    parts: Gathered,
    save: Option<Vec<u8>>,
    endpoint: Dest,
    /// The line over the buttons: what happened last.
    status: String,
    result: Arc<Mutex<Option<Result<String, String>>>>,
    opened_at: f64,
    aid_done: bool,
    dirty: bool,
    /// Closed this frame: [`FormOver`] is cleared next frame.
    closing: bool,
}

impl Form {
    fn reset(&mut self) {
        let captured = self.captured.clone();
        let result = self.result.clone();
        *self = Form { captured, result, ticks: [true, true, true, false, false], ..Default::default() };
    }

    fn ticked(&self, t: Tick) -> bool {
        self.ticks[t as usize] && self.available(t)
    }

    fn available(&self, t: Tick) -> bool {
        match t {
            Tick::Session => self.parts.summary.is_some() || self.parts.snapshot.is_some(),
            Tick::Log => self.parts.log.is_some() || self.parts.recorder.is_some(),
            Tick::Screenshot => self.parts.screenshot.is_some(),
            Tick::Save | Tick::Contact => true,
        }
    }

    fn size(&self, t: Tick) -> usize {
        let len = |b: &Option<Vec<u8>>| b.as_ref().map_or(0, Vec::len);
        match t {
            Tick::Session => len(&self.parts.summary) + len(&self.parts.snapshot),
            Tick::Log => len(&self.parts.log) + len(&self.parts.recorder),
            Tick::Screenshot => len(&self.parts.screenshot),
            Tick::Save => len(&self.save),
            Tick::Contact => self.contact.len(),
        }
    }

    fn total(&self) -> usize {
        self.description.len() + TICKS.iter().filter(|t| self.ticked(**t)).map(|t| self.size(*t)).sum::<usize>()
    }

    fn label(&self, t: Tick) -> String {
        let kb = |n: usize| size_text(n);
        match t {
            Tick::Session if self.parts.summary.is_none() => format!("Where you are: the world snapshot (your character, quests, people nearby) ({})", kb(self.size(t))),
            Tick::Session => format!("This session so far: its summary and the world snapshot ({})", kb(self.size(t))),
            Tick::Log => format!("The game's log: the last {} lines and the last minute's events ({})", self.parts.log_lines, kb(self.size(t))),
            Tick::Screenshot => {
                let (w, h) = self.parts.shot_size;
                format!("A screenshot of the game before this form opened, {w}x{h} ({})", kb(self.size(t)))
            }
            Tick::Save => "A save of the game as it is now (written for this report; it holds your character's name)".to_string(),
            Tick::Contact => "Your GitHub name, so we can reply on an issue".to_string(),
        }
    }
}

/// A size as the form shows it.
fn size_text(n: usize) -> String {
    if n >= 1024 * 1024 {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", n.div_ceil(1024))
    }
}

/// Where a hand report goes, or why it cannot go (module notes).
fn endpoint() -> Result<String, String> {
    let trimmed = |s: String| s.trim().trim_end_matches('/').to_string();
    if let Some(e) = std::env::var("NVF_REPORTS_ENDPOINT").ok().filter(|v| !v.trim().is_empty()) {
        return Ok(trimmed(e));
    }
    if let Some(why) = super::forced_off(super::DEVELOPER_DATA.get().copied().unwrap_or(false)) {
        return Err(format!("This run does not send reports ({why}; NVF_REPORTS_ENDPOINT for a test server)"));
    }
    if let Some(e) = EngineHome::user().and_then(|h| Reports::load(&h).endpoint) {
        return Ok(trimmed(e));
    }
    if super::OFFICIAL_BUILD || super::production_allowed() {
        return Ok(upload::DEFAULT_ENDPOINT.to_string());
    }
    Err("This build does not send reports: only official releases send to the project's server".to_string())
}

pub fn add(app: &mut App) {
    // The pause menu's "Report a bug" entry is drawn while the form is there to open.
    app.insert_resource(ReportEntry { shown: true, asked: false })
        .init_resource::<Form>()
        // Only where the pause menu and saves are (a played game): a menu-only or viewer
        // run has neither.
        .add_systems(PreUpdate, clear_form_over.run_if(in_play))
        .add_systems(Update, (open, shoot, keys, clicks, test_aid, progress, draw).chain().run_if(in_play));
}

/// The pause menu and the save messages exist (a played game).
fn in_play(over: Option<Res<FormOver>>, saves: Option<Res<Messages<nvf_play::save::SaveRequest>>>, done: Option<Res<Messages<nvf_play::save::SaveDone>>>) -> bool {
    over.is_some() && saves.is_some() && done.is_some()
}

/// The pause menu is up over a running game.
fn in_game(paused: Option<&Paused>, script: Option<&nvf_play::script::ScriptState>) -> bool {
    paused.is_some_and(|p| p.0) && script.is_some_and(|s| s.rt.is_some())
}

fn clear_form_over(mut form: ResMut<Form>, mut over: ResMut<FormOver>, text_input: Option<ResMut<FormTextInput>>) {
    if form.closing {
        form.closing = false;
        over.set_if_neq(FormOver(false));
        if let Some(mut text) = text_input {
            text.set_if_neq(FormTextInput(false));
        }
    }
}

#[derive(Component)]
struct FormRoot;

/// What a click on a node of the form does.
#[derive(Component, Clone, Copy, Debug)]
enum FormAct {
    Category(usize),
    Tick(Tick),
    Description,
    Contact,
    Send,
    Close,
}

/// The root nodes drawn on the window (not the Pip-Boy's screen, drawn by its own camera),
/// hidden for the screenshot as a save hides them (`nvf_play::save`).
type WindowRoots = (With<Node>, Without<ChildOf>, Without<bevy::ui::UiTargetCamera>);

/// "Report a bug" was clicked: hide the interface for a frame and take the screenshot.
#[allow(clippy::too_many_arguments)]
fn open(
    mut commands: Commands,
    mut entry: ResMut<ReportEntry>,
    mut form: ResMut<Form>,
    mut over: ResMut<FormOver>,
    text_input: Option<ResMut<FormTextInput>>,
    frame: Res<bevy::diagnostic::FrameCount>,
    roots: Query<(Entity, &Visibility), WindowRoots>,
    paused: Option<Res<Paused>>,
    script: Option<Res<nvf_play::script::ScriptState>>,
) {
    if !std::mem::take(&mut entry.asked) || !matches!(form.phase, Phase::Closed) {
        return;
    }
    if !in_game(paused.as_deref(), script.as_deref()) {
        info!("report form: only from the pause menu in a game");
        return;
    }
    SNAPSHOT_NOW.store(true, Ordering::Relaxed);
    form.reset();
    over.set_if_neq(FormOver(true));
    if let Some(mut text) = text_input {
        text.set_if_neq(FormTextInput(true));
    }
    OPEN.store(true, Ordering::Relaxed);
    form.hidden = roots.iter().filter(|(_, v)| **v != Visibility::Hidden).map(|(e, v)| (e, *v)).collect();
    for (e, _) in &form.hidden {
        commands.entity(*e).insert(Visibility::Hidden);
    }
    *form.captured.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let slot = form.captured.clone();
    commands.spawn(Screenshot::primary_window()).observe(move |ev: On<ScreenshotCaptured>| {
        *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(ev.image.clone());
    });
    form.phase = Phase::Shooting(frame.0);
}

/// Show the interface again, and once the screenshot is in (or did not come), gather the
/// parts and open the form.
#[allow(clippy::too_many_arguments)]
fn shoot(
    mut commands: Commands,
    mut form: ResMut<Form>,
    frame: Res<bevy::diagnostic::FrameCount>,
    real: Res<Time<Real>>,
    mut images: ResMut<Assets<Image>>,
    collector: Option<ResMut<super::Collector>>,
    script: Option<Res<nvf_play::script::ScriptState>>,
) {
    let Phase::Shooting(asked) = form.phase else { return };
    if frame.0 > asked {
        for (e, v) in std::mem::take(&mut form.hidden) {
            if let Ok(mut ent) = commands.get_entity(e) {
                ent.insert(v);
            }
        }
    }
    let shot = form.captured.lock().unwrap_or_else(|e| e.into_inner()).take();
    if shot.is_none() && frame.0.saturating_sub(asked) <= SHOT_WAIT_FRAMES {
        return;
    }
    let mut parts = Gathered::default();
    if let Some(img) = shot {
        match picture(&img) {
            Some((png, (w, h), thumb)) => {
                parts.screenshot = Some(png);
                parts.shot_size = (w, h);
                form.thumb = thumb.and_then(|(tw, th, rgba)| {
                    let format = bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb;
                    let mut i = nvf_assets::texture::checked_image(tw, th, 1, format, rgba).ok()?;
                    i.asset_usage = bevy::asset::RenderAssetUsages::default();
                    Some(images.add(i))
                });
            }
            None => warn!("report form: the screenshot could not be read; the report goes without it"),
        }
    } else {
        warn!("report form: no screenshot within {SHOT_WAIT_FRAMES} frames; the report goes without it");
    }
    if let Some(mut c) = collector {
        super::bring_up_to_date(&mut c, script.as_deref(), false);
        parts.summary = Some(upload::gzip(&c.summary.to_bytes(super::redact)));
    }
    let snap = super::snapshot::latest();
    if !snap.is_empty() {
        parts.snapshot = Some(upload::gzip(super::redact(&snap).as_bytes()));
    }
    if let Some(text) = crate::crash::run_log_path().and_then(|p| std::fs::read_to_string(p).ok()) {
        let (tail, n) = last_lines(&text, LOG_LINES);
        parts.log_lines = n;
        parts.log = Some(upload::gzip(super::redact(tail).as_bytes()));
    }
    let recorder: String = super::anomaly::recorder_lines().into_iter().map(|l| super::redact(&l) + "\n").collect();
    if !recorder.is_empty() {
        parts.recorder = Some(upload::gzip(recorder.as_bytes()));
    }
    form.parts = parts;
    form.endpoint = Dest(endpoint());
    form.opened_at = real.elapsed_secs_f64();
    form.phase = Phase::Open;
    form.dirty = true;
    let f = &*form;
    let have: Vec<String> = TICKS.iter().filter(|t| f.available(**t) && !matches!(t, Tick::Save | Tick::Contact)).map(|t| format!("{t:?} {}", size_text(f.size(*t)))).collect();
    info!(
        "report form: open ({}; {})",
        if have.is_empty() { "nothing to attach".to_string() } else { have.join(", ") },
        match &f.endpoint.0 {
            Ok(url) => format!("sends to {url}"),
            Err(why) => format!("cannot send: {why}"),
        }
    );
}

/// The last `n` lines of `text` and how many there are.
fn last_lines(text: &str, n: usize) -> (&str, usize) {
    let body = text.trim_end_matches('\n');
    let mut start = body.len();
    let mut count = 0;
    while count < n {
        match body[..start].rfind('\n') {
            Some(i) => {
                start = i;
                count += 1;
            }
            None => {
                start = 0;
                count += 1;
                break;
            }
        }
    }
    let from = if start == 0 { 0 } else { start + 1 };
    (&body[from..], count.min(body.lines().count()))
}

/// The screenshot as a PNG at most [`SHOT_WIDTH`] wide, its size, and a thumbnail (RGBA).
#[allow(clippy::type_complexity)]
fn picture(img: &Image) -> Option<(Vec<u8>, (u32, u32), Option<(u32, u32, Vec<u8>)>)> {
    use bevy::render::render_resource::TextureFormat;
    let bgra = match img.texture_descriptor.format {
        TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb => true,
        TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb => false,
        _ => return None,
    };
    let px = img.data.as_ref()?;
    let mut max_w = SHOT_WIDTH;
    let (png, w, h) = loop {
        let (w, h, rgb) = scaled_rgb(img.width(), img.height(), px, bgra, max_w)?;
        let png = encode_png(&rgb, w, h)?;
        if png.len() <= MAX_SHOT || w <= THUMB_WIDTH {
            break (png, w, h);
        }
        max_w = w / 2;
    };
    let thumb = scaled_rgb(img.width(), img.height(), px, bgra, THUMB_WIDTH).map(|(tw, th, t)| (tw, th, t.as_chunks::<3>().0.iter().flat_map(|[r, g, b]| [*r, *g, *b, 255]).collect()));
    Some((png, (w, h), thumb))
}

/// `px` (`w`×`h`, 4 bytes a pixel, BGRA when `bgra`) as RGB, shrunk by a whole factor (the
/// mean of each block) to at most `max_w` wide.
fn scaled_rgb(w: u32, h: u32, px: &[u8], bgra: bool, max_w: u32) -> Option<(u32, u32, Vec<u8>)> {
    let (w, h) = (w as usize, h as usize);
    if w == 0 || h == 0 || px.len() < w * h * 4 {
        return None;
    }
    let f = w.div_ceil(max_w.max(1) as usize).max(1);
    let (nw, nh) = (w / f, h / f);
    if nw == 0 || nh == 0 {
        return None;
    }
    let mut out = Vec::with_capacity(nw * nh * 3);
    for y in 0..nh {
        for x in 0..nw {
            let mut sum = [0u32; 3];
            for dy in 0..f {
                let row = (y * f + dy) * w;
                for dx in 0..f {
                    let i = (row + x * f + dx) * 4;
                    let (r, b) = if bgra { (px[i + 2], px[i]) } else { (px[i], px[i + 2]) };
                    sum[0] += u32::from(r);
                    sum[1] += u32::from(px[i + 1]);
                    sum[2] += u32::from(b);
                }
            }
            let n = (f * f) as u32;
            out.extend(sum.map(|s| (s / n) as u8));
        }
    }
    Some((nw as u32, nh as u32, out))
}

fn encode_png(rgb: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, w, h);
    e.set_color(png::ColorType::Rgb);
    e.set_depth(png::BitDepth::Eight);
    e.set_compression(png::Compression::High);
    let mut wr = e.write_header().ok()?;
    wr.write_image_data(rgb).ok()?;
    wr.finish().ok()?;
    Some(out)
}

/// The keys while the form is open.
fn keys((mut typed, switching): (MessageReader<KeyboardInput>, Option<Res<nvf_play::pad::SwitchingInput>>), held: nvf_play::pad::ActionButtons<KeyCode>, mut form: ResMut<Form>, mut commands: Commands, mut saves: MessageWriter<nvf_play::save::SaveRequest>, real: Res<Time<Real>>) {
    let held = held.snapshot();
    let events: Vec<KeyboardInput> = typed.read().filter(|k| k.state.is_pressed() && switching.as_ref().is_none_or(|s| s.allows(k))).cloned().collect();
    let ctrl = held.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let other_mods = held.any_pressed([KeyCode::AltLeft, KeyCode::AltRight, KeyCode::SuperLeft, KeyCode::SuperRight]);
    for k in events {
        match (&form.phase, &k.logical_key) {
            (Phase::Open | Phase::Sent, Key::Escape) => {
                close(&mut form, &mut commands);
                return;
            }
            (Phase::Open, Key::Enter) if ctrl => {
                send(&mut form, &mut saves, real.elapsed_secs_f64());
            }
            (Phase::Open, Key::Tab) => {
                form.on_contact = !form.on_contact && form.ticks[Tick::Contact as usize];
                form.dirty = true;
            }
            (Phase::Open, Key::Backspace) => {
                let f = &mut *form;
                if f.on_contact { f.contact.pop() } else { f.description.pop() };
                f.dirty = true;
            }
            (Phase::Open, Key::Enter) if !form.on_contact => {
                if form.description.chars().count() < MAX_DESCRIPTION {
                    form.description.push('\n');
                    form.dirty = true;
                }
            }
            (Phase::Open, _) if !ctrl && !other_mods => {
                let Some(t) = k.text.as_deref().filter(|t| !t.is_empty() && t.chars().all(|c| !c.is_control())) else { continue };
                let f = &mut *form;
                let (field, cap) = if f.on_contact { (&mut f.contact, MAX_CONTACT) } else { (&mut f.description, MAX_DESCRIPTION) };
                for c in t.chars() {
                    if field.chars().count() < cap {
                        field.push(c);
                    }
                }
                f.dirty = true;
            }
            _ => {}
        }
    }
}

/// A click on the form.
fn clicks(pressed: Query<(&PointerState, &FormAct), Changed<PointerState>>, mut form: ResMut<Form>, mut commands: Commands, mut saves: MessageWriter<nvf_play::save::SaveRequest>, real: Res<Time<Real>>) {
    let Some(act) = pressed.iter().find(|(s, _)| **s == PointerState::Pressed).map(|(_, a)| *a) else { return };
    match (act, &form.phase) {
        (FormAct::Close, Phase::Open | Phase::Sent) => close(&mut form, &mut commands),
        (FormAct::Send, Phase::Open) => send(&mut form, &mut saves, real.elapsed_secs_f64()),
        (FormAct::Category(i), Phase::Open) => form.category = i,
        (FormAct::Tick(t), Phase::Open) if form.available(t) => {
            form.ticks[t as usize] = !form.ticks[t as usize];
            if t == Tick::Contact {
                form.on_contact = form.ticks[t as usize];
            }
        }
        (FormAct::Description, Phase::Open) => form.on_contact = false,
        (FormAct::Contact, Phase::Open) => form.on_contact = true,
        _ => return,
    }
    form.dirty = true;
}

/// The test aid's filling and sending (module notes).
fn test_aid(mut form: ResMut<Form>, real: Res<Time<Real>>, mut saves: MessageWriter<nvf_play::save::SaveRequest>) {
    if form.aid_done || !matches!(form.phase, Phase::Open) || real.elapsed_secs_f64() - form.opened_at < 1.0 {
        return;
    }
    let Ok(v) = std::env::var("NVF_PLAY_TEST_REPORT_FORM") else { return };
    form.aid_done = true;
    let mut items = v.split('|');
    form.description = items.next().unwrap_or_default().to_string();
    for item in items {
        match item.trim() {
            "save" => form.ticks[Tick::Save as usize] = true,
            i if i.starts_with("contact=") => {
                form.ticks[Tick::Contact as usize] = true;
                form.contact = i["contact=".len()..].to_string();
            }
            i if i.starts_with("category=") => {
                if let Some(n) = CATEGORIES.iter().position(|(id, _)| *id == &i["category=".len()..]) {
                    form.category = n;
                }
            }
            other => warn!("test aid NVF_PLAY_TEST_REPORT_FORM: {other:?} is not save, contact=<name> or category=<id>"),
        }
    }
    info!("test aid NVF_PLAY_TEST_REPORT_FORM: the form filled and sent");
    send(&mut form, &mut saves, real.elapsed_secs_f64());
}

fn close(form: &mut Form, commands: &mut Commands) {
    if !matches!(form.phase, Phase::Open | Phase::Sent) {
        return;
    }
    info!("report form: closed");
    form.phase = Phase::Closed;
    form.closing = true;
    form.dirty = true;
    OPEN.store(false, Ordering::Relaxed);
    commands.queue(|w: &mut World| {
        let roots: Vec<Entity> = w.query_filtered::<Entity, With<FormRoot>>().iter(w).collect();
        for e in roots {
            w.despawn(e);
        }
    });
}

/// Send was chosen: write the save first if it is ticked, else send now.
fn send(form: &mut Form, saves: &mut MessageWriter<nvf_play::save::SaveRequest>, now: f64) {
    form.dirty = true;
    if form.description.trim().is_empty() {
        form.status = "Write what went wrong first.".into();
        return;
    }
    if let Err(why) = &form.endpoint.0 {
        form.status = why.clone();
        return;
    }
    form.save = None;
    if form.ticked(Tick::Save) {
        let dir = EngineHome::user().map(|h| h.reports().join("form"));
        let made = dir.as_ref().is_some_and(|d| fnv_install::guard_write(d, None).is_ok() && std::fs::create_dir_all(d).is_ok());
        if let (Some(dir), true) = (dir, made) {
            let stamp = crate::crash::stamp(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()));
            let path = dir.join(format!("report-{stamp}-{}.{}", std::process::id(), fnv_save::EXTENSION));
            saves.write(nvf_play::save::SaveRequest { name: path.to_string_lossy().into_owned(), kind: fnv_save::SaveKind::Manual });
            form.status = "Writing the save...".into();
            form.phase = Phase::Saving(path, now);
            return;
        }
        form.status = "The save could not be written; sending without it.".into();
    }
    start_sending(form);
}

fn start_sending(form: &mut Form) {
    let Ok(url) = form.endpoint.0.clone() else { return };
    let identity = super::active().map(super::identity);
    let mut fields: Vec<(&'static str, String)> = vec![
        ("description", super::redact(&form.description)),
        ("category", CATEGORIES[form.category].0.to_string()),
        ("build", env!("CARGO_PKG_VERSION").to_string()),
    ];
    if form.ticked(Tick::Contact) && !form.contact.trim().is_empty() {
        fields.push(("contact", form.contact.trim().to_string()));
    }
    if let Some((id, _)) = &identity {
        fields.push(("install_id", id.clone()));
    }
    let mut files: Vec<(&'static str, &'static str, &'static str, Vec<u8>)> = Vec::new();
    let p = &form.parts;
    if form.ticked(Tick::Session) {
        if let Some(b) = &p.summary {
            files.push(("summary", "summary.json.gz", "application/gzip", b.clone()));
        }
        if let Some(b) = &p.snapshot {
            files.push(("snapshot", "snapshot.json.gz", "application/gzip", b.clone()));
        }
    }
    if form.ticked(Tick::Log) {
        if let Some(b) = &p.log {
            files.push(("log", "log.txt.gz", "application/gzip", b.clone()));
        }
        if let Some(b) = &p.recorder {
            files.push(("recorder", "recorder.jsonl.gz", "application/gzip", b.clone()));
        }
    }
    if form.ticked(Tick::Screenshot) {
        if let Some(b) = &p.screenshot {
            files.push(("screenshot", "screenshot.png", "image/png", b.clone()));
        }
    }
    if let Some(b) = form.save.take() {
        files.push(("save", "report.nvfsave", "application/octet-stream", b));
    }
    let names: Vec<String> = files.iter().map(|(n, _, _, b)| format!("{n} {}", size_text(b.len()))).collect();
    info!(
        "report form: sending to {url} ({}, description {} characters{}{})",
        CATEGORIES[form.category].0,
        form.description.chars().count(),
        if fields.iter().any(|(n, _)| *n == "contact") { ", contact" } else { "" },
        if names.is_empty() { String::new() } else { format!(", {}", names.join(", ")) }
    );
    form.status = "Sending...".into();
    form.phase = Phase::Sending;
    let out = form.result.clone();
    *out.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let spawned = std::thread::Builder::new().name("nvf-report-form".into()).spawn(move || {
        let mut parts: Vec<(&str, &str, &str, &[u8])> = fields.iter().map(|(n, v)| (*n, "", "", v.as_bytes())).collect();
        parts.extend(files.iter().map(|(n, f, t, b)| (*n, *f, *t, b.as_slice())));
        let (body, ctype) = upload::multipart(&parts);
        let answer = upload::http::post_for(&format!("{url}/v1/report"), &body, &ctype, identity.as_ref().map(|(_, t)| t.as_str()));
        *out.lock().unwrap_or_else(|e| e.into_inner()) = Some(answer_of(answer));
    });
    if spawned.is_err() {
        *form.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(Err("could not start sending".into()));
    }
}

/// The server's answer to a hand report: its reference, or what to tell the player (§5.1).
fn answer_of(answer: Option<(u16, Vec<u8>)>) -> Result<String, String> {
    let Some((status, body)) = answer else { return Err("No answer from the server (offline?). Try again later.".into()) };
    let text = String::from_utf8_lossy(&body);
    match status {
        200..=299 => serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v.get("ref").and_then(|r| r.as_str()).map(str::to_string))
            .ok_or_else(|| format!("The server took it but sent no reference ({status})")),
        413 => Err("Too large for the server: untick the save or the screenshot.".into()),
        429 => Err("The server takes 5 reports a day from one PC. Try again tomorrow.".into()),
        503 => Err("Reports are switched off on the server for now.".into()),
        403 => Err("This build can no longer send reports; update NVForever.".into()),
        _ => Err(format!("The server refused it ({status}): {}", text.chars().take(200).collect::<String>())),
    }
}

/// The save written, the answer come.
fn progress(mut form: ResMut<Form>, mut done: MessageReader<nvf_play::save::SaveDone>, real: Res<Time<Real>>) {
    match &form.phase {
        Phase::Saving(path, since) => {
            let path = path.clone();
            let mut finished = None;
            for d in done.read() {
                if d.path == path {
                    finished = Some(d.result.clone());
                }
            }
            let timed_out = real.elapsed_secs_f64() - since > SAVE_WAIT_SECS;
            if finished.is_none() && !timed_out {
                return;
            }
            match finished {
                Some(Ok(())) => match std::fs::read(&path) {
                    Ok(b) if b.len() <= MAX_SAVE => {
                        info!("report form: save written for the report ({})", size_text(b.len()));
                        form.save = Some(b);
                    }
                    Ok(b) => {
                        warn!("report form: the save is {} (over {}); sent without it", size_text(b.len()), size_text(MAX_SAVE));
                        form.status = "The save is too large to send; sending without it.".into();
                    }
                    Err(e) => warn!("report form: the save could not be read: {e}"),
                },
                Some(Err(e)) => {
                    warn!("report form: no save for the report: {e}");
                    form.status = format!("No save ({e}); sending without it.");
                }
                None => warn!("report form: the save did not come within {SAVE_WAIT_SECS} s; sent without it"),
            }
            let _ = std::fs::remove_file(&path);
            start_sending(&mut form);
            form.dirty = true;
        }
        Phase::Sending => {
            let Some(r) = form.result.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
            match r {
                Ok(reference) => {
                    info!("report form: sent, reference {reference}");
                    form.status = format!("Sent. Thank you! Your reference is {reference}; quote it if you open an issue.");
                    form.phase = Phase::Sent;
                }
                Err(why) => {
                    warn!("report form: could not send: {why}");
                    form.status = why;
                    form.phase = Phase::Open;
                }
            }
            form.dirty = true;
        }
        _ => {}
    }
}

const PANEL_WIDTH: f32 = 1000.0;
const TEXT: f32 = 17.0;

fn text_bundle(s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (Text::new(s), TextFont { font_size: font_units(size), ..default() }, TextColor(color))
}

/// Draw the form again when it changed.
fn draw(mut commands: Commands, mut form: ResMut<Form>, roots: Query<Entity, With<FormRoot>>) {
    if !std::mem::take(&mut form.dirty) {
        return;
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
    if matches!(form.phase, Phase::Closed | Phase::Shooting(_)) {
        return;
    }
    let f = &*form;
    let (bright, dim, faint) = (amber(255.0), amber(170.0), amber(90.0));
    let open = matches!(f.phase, Phase::Open);
    let full = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() };
    commands.spawn((FormRoot, full, nvf_play::pause::form_shade(), GlobalZIndex(nvf_play::pause::FORM_Z))).with_children(|root| {
        let panel = Node {
            width: u(PANEL_WIDTH),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(u(18.0)),
            row_gap: u(8.0),
            border: UiRect::all(Val::Px(2.0)),
            ..default()
        };
        root.spawn((panel, nvf_play::pause::form_panel(), BorderColor::all(dim))).with_children(|p| {
            p.spawn(text_bundle("Send a report", TEXT * 1.6, bright));
            p.spawn(text_bundle("What went wrong? What did you do, what happened, and what did you expect?", TEXT, dim));
            // The description.
            let shown = tail_chars(&f.description, 1200);
            let caret = if open && !f.on_contact { "_" } else { "" };
            let (body, color) = if f.description.is_empty() && !(open && !f.on_contact) { ("Click here and type".to_string(), faint) } else { (format!("{shown}{caret}"), bright) };
            p.spawn((
                UiButton,
                FormAct::Description,
                Node { min_height: u(130.0), max_height: u(300.0), overflow: Overflow::clip(), padding: UiRect::all(u(8.0)), border: UiRect::all(Val::Px(1.0)), ..default() },
                BorderColor::all(if open && !f.on_contact { bright } else { faint }),
            ))
            .with_children(|d| {
                d.spawn((text_bundle(body, TEXT, color), Node { width: Val::Percent(100.0), ..default() }));
            });
            // The category.
            p.spawn(Node { flex_direction: FlexDirection::Row, column_gap: u(10.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
                r.spawn(text_bundle("Category:", TEXT, dim));
                for (i, (_, name)) in CATEGORIES.iter().enumerate() {
                    let chosen = i == f.category;
                    r.spawn((
                        UiButton,
                        FormAct::Category(i),
                        Node { padding: UiRect::axes(u(8.0), u(3.0)), border: UiRect::all(Val::Px(1.0)), ..default() },
                        BorderColor::all(if chosen { bright } else { faint }),
                        BackgroundColor(if chosen { amber(40.0) } else { Color::NONE }),
                    ))
                    .with_children(|b| {
                        b.spawn(text_bundle(*name, TEXT, if chosen { bright } else { dim }));
                    });
                }
            });
            // The tick boxes.
            for t in TICKS {
                if !f.available(t) {
                    continue;
                }
                p.spawn(Node { flex_direction: FlexDirection::Column, row_gap: u(4.0), ..default() }).with_children(|row| {
                    row.spawn((UiButton, FormAct::Tick(t), Node { flex_direction: FlexDirection::Row, column_gap: u(8.0), ..default() })).with_children(|b| {
                        b.spawn(text_bundle(if f.ticked(t) { "[x]" } else { "[ ]" }, TEXT, bright));
                        b.spawn(text_bundle(f.label(t), TEXT, if f.ticked(t) { bright } else { dim }));
                    });
                    if t == Tick::Screenshot && f.ticked(t) {
                        if let Some(h) = &f.thumb {
                            row.spawn((ImageNode::new(h.clone()), Node { width: u(240.0), height: u(135.0), margin: UiRect::left(u(34.0)), ..default() }));
                        }
                    }
                    if t == Tick::Contact && f.ticked(t) {
                        let caret = if open && f.on_contact { "_" } else { "" };
                        row.spawn((
                            UiButton,
                            FormAct::Contact,
                            Node { margin: UiRect::left(u(34.0)), width: u(360.0), padding: UiRect::all(u(4.0)), border: UiRect::all(Val::Px(1.0)), ..default() },
                            BorderColor::all(if open && f.on_contact { bright } else { faint }),
                        ))
                        .with_children(|c| {
                            c.spawn(text_bundle(format!("{}{caret}", f.contact), TEXT, bright));
                        });
                    }
                });
            }
            p.spawn(text_bundle(
                "Sending this is your consent for this report only; it does not turn reports on. The report is kept private and used only to fix bugs.",
                TEXT * 0.85,
                dim,
            ));
            let total = format!("To send: about {}{}", size_text(f.total()), if f.ticked(Tick::Save) && f.save.is_none() { " and the save" } else { "" });
            p.spawn(text_bundle(total, TEXT, dim));
            if let Err(why) = &f.endpoint.0 {
                p.spawn(text_bundle(why.clone(), TEXT, bright));
            }
            if !f.status.is_empty() {
                p.spawn(text_bundle(f.status.clone(), TEXT, bright));
            }
            // The buttons.
            p.spawn(Node { flex_direction: FlexDirection::Row, column_gap: u(16.0), justify_content: JustifyContent::FlexEnd, ..default() }).with_children(|r| {
                let mut button = |act: FormAct, label: &str, on: bool| {
                    r.spawn((
                        UiButton,
                        act,
                        Node { padding: UiRect::axes(u(14.0), u(5.0)), border: UiRect::all(Val::Px(1.0)), ..default() },
                        BorderColor::all(if on { bright } else { faint }),
                        BackgroundColor(if on { amber(40.0) } else { Color::NONE }),
                    ))
                    .with_children(|b| {
                        b.spawn(text_bundle(label, TEXT, if on { bright } else { faint }));
                    });
                };
                match f.phase {
                    Phase::Sent => button(FormAct::Close, "Close  Esc", true),
                    Phase::Open => {
                        button(FormAct::Send, "Send  Ctrl+Enter", f.endpoint.0.is_ok() && !f.description.trim().is_empty());
                        button(FormAct::Close, "Cancel  Esc", true);
                    }
                    _ => button(FormAct::Close, "Sending...", false),
                }
            });
        });
    });
}

/// The last `n` characters of `s` (a long description shows its end, where the caret is).
fn tail_chars(s: &str, n: usize) -> std::borrow::Cow<'_, str> {
    let count = s.chars().count();
    if count <= n {
        return s.into();
    }
    let skip = s.char_indices().nth(count - n).map_or(0, |(i, _)| i);
    format!("...{}", &s[skip..]).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_escape_keeps_the_report_open_until_a_fresh_escape() {
        use bevy::input::ButtonState;
        use nvf_play::bindings::{Bindings, InputDevice};
        let mut bindings = Bindings::default();
        bindings.device = InputDevice::Controller;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::input::InputPlugin, nvf_play::pad::InputSwitchPlugin))
            .insert_resource(bindings)
            .insert_resource(Form { phase: Phase::Open, ..default() })
            .add_message::<nvf_play::save::SaveRequest>().add_systems(Update, keys);
        let escape = KeyboardInput { key_code: KeyCode::Escape, logical_key: Key::Escape, state: ButtonState::Pressed, text: None, repeat: false, window: Entity::PLACEHOLDER };
        app.world_mut().write_message(escape.clone());
        app.update();
        assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
        assert!(matches!(app.world().resource::<Form>().phase, Phase::Open));
        app.world_mut().write_message(KeyboardInput { state: ButtonState::Released, ..escape.clone() });
        app.update();
        app.world_mut().write_message(escape);
        app.update();
        assert!(matches!(app.world().resource::<Form>().phase, Phase::Closed));
    }

    #[test]
    fn the_last_lines_of_a_log() {
        assert_eq!(last_lines("a\nb\nc\n", 2), ("b\nc", 2));
        assert_eq!(last_lines("a\nb\nc", 5), ("a\nb\nc", 3));
        assert_eq!(last_lines("", 3).0, "");
    }

    #[test]
    fn a_screenshot_is_shrunk_by_whole_blocks_and_swapped_to_rgb() {
        // 4x2 BGRA: blue pixels.
        let px: Vec<u8> = (0..8).flat_map(|_| [255, 0, 0, 255]).collect();
        let (w, h, rgb) = scaled_rgb(4, 2, &px, true, 2).unwrap();
        assert_eq!((w, h), (2, 1));
        assert_eq!(rgb, vec![0, 0, 255, 0, 0, 255]);
        assert!(encode_png(&rgb, w, h).is_some_and(|p| p.starts_with(b"\x89PNG")));
    }

    #[test]
    fn the_server_answer_becomes_a_reference_or_a_reason() {
        assert_eq!(answer_of(Some((202, br#"{"id":"x","ref":"R-K7Q2M9TB"}"#.to_vec()))), Ok("R-K7Q2M9TB".to_string()));
        assert!(answer_of(Some((429, Vec::new()))).unwrap_err().contains("5 reports a day"));
        assert!(answer_of(Some((413, Vec::new()))).unwrap_err().contains("untick"));
        assert!(answer_of(None).unwrap_err().contains("No answer"));
        assert!(answer_of(Some((400, b"description required".to_vec()))).unwrap_err().contains("description required"));
    }

    #[test]
    fn a_long_description_shows_its_end() {
        assert_eq!(tail_chars("abc", 5), "abc");
        assert_eq!(tail_chars("abcdef", 3), "...def");
    }
}
