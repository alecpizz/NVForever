//! The weapon wheel's look (docs/design/weapon-wheel.md §7), from the game's own pictures
//! and drawn shapes: no picture of NVForever's own.
//!
//! - The view behind darkens by [`super::DARKEN`] as the wheel eases in.
//! - The matte: the companion wheel's soft black disc
//!   (`interface\companion_wheel\hud_comp_bkg.dds`).
//! - The eight slices and the hub: annular sectors drawn by a small shader
//!   ([`SliceMaterial`], `slice.wesl`), near-black with a dim amber outline, the picked one
//!   amber with a full outline and a bright arc outside the rim.
//! - Each slot's picture: its `SetItemHotkeyIconPath` override when set, else its item's
//!   own (`ICON`, the Pip-Boy's), tinted with the interface colour, else the game's hotkey
//!   type icon for its kind (`interface\icons\typeicons\hot_keys_icon_*.dds`); slot 2 the
//!   Ammo Swap icon. Its number by the hub, its count (ammunition carried, or items) under
//!   the picture.
//! - The hub: the picked slot's kind and name, its figures as bars (DAM, DPS, WG, VAL, CND
//!   of a weapon; DT … of apparel), its ammunition; for slot 2 the equipped weapon's
//!   ammunition choices and what of each is carried.
//! - The game's fonts (`glow_monofonto_large.fnt`, `glow_monofonto_medium.fnt`) in the
//!   interface colour.

use super::{WeaponWheel, DARKEN};
use crate::hotkeys::{Hotkeys, AMMO_SWAP};
use crate::interface::{amber, at, face, plain_font, set_line, Face, Pictures};
use crate::sim::SimState;
use crate::PlaySet;
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::ui_render::prelude::{MaterialNode, UiMaterial};
use bevy::window::PrimaryWindow;
use fnv_sim::forms::ItemKind;
use fnv_sim::FormId;
use nvf_assets::GameData;

/// The wheel's square, interface units, and how far above the middle of the view it
/// stands [design: the owner's picture, the mockups].
pub const SIZE: f32 = 824.0;
const RAISE: f32 = 20.0;
/// The slices' radii, the hub's, the gap between slices (degrees each side) and the lit
/// rim's band outside the slices.
pub const OUTER: f32 = 372.0;
pub const INNER: f32 = 178.0;
const HUB: f32 = 168.0;
const GAP: f32 = 1.6;
const RIM: [f32; 2] = [380.0, 392.0];
// The rim stays inside the square, the hub inside the slices.
const _: () = assert!(RIM[1] < SIZE / 2.0 && HUB < INNER && INNER < OUTER && OUTER < RIM[0]);

/// An item picture's size, and the Ammo Swap icon's.
const ICON: f32 = 132.0;
const SWAP_ICON: f32 = 70.0;
/// The hub's text keeps this far inside its outline; a figure's row, a rule's room, and
/// how far out the figures' labels and values stand at most.
const HUB_PAD: f32 = 12.0;
const ROW: f32 = 22.0;
const RULE_ROOM: f32 = 10.0;
const BAR_HALF: f32 = 112.0;
/// An outline's width.
const LINE: f32 = 2.0;
/// An unpicked slice's picture and count are drawn at this strength; a slot whose item
/// is not carried at half that.
const DIM: f32 = 0.62;
/// The figures' bars are full at these values [design].
const FULL_DAMAGE: f32 = 100.0;
const FULL_DPS: f32 = 200.0;
const FULL_WEIGHT: f32 = 20.0;
const FULL_VALUE: f32 = 2000.0;
const FULL_DT: f32 = 40.0;

const MATTE: &str = "Interface\\companion_wheel\\hud_comp_bkg.dds";
const SWAP: &str = "Interface\\Icons\\typeicons\\hot_keys_icon_ammoswap.dds";
const FONT_1: &str = "textures\\fonts\\glow_monofonto_large.fnt";
const FONT_3: &str = "textures\\fonts\\glow_monofonto_medium.fnt";
const SHADER: &str = "embedded://nvf_play/weapon_wheel/slice.wesl";

/// The slice shader's numbers (`slice.wesl`).
#[derive(ShaderType, Clone, Copy, Debug, Default)]
pub struct SliceUniform {
    pub fill: Vec4,
    pub edge: Vec4,
    /// From and to angle (radians clockwise from up), inner and outer radius as parts of
    /// the half size.
    pub shape: Vec4,
    pub line: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct SliceMaterial {
    #[uniform(0)]
    pub u: SliceUniform,
}

impl UiMaterial for SliceMaterial {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
}

/// The drawing's systems; the shader only where assets are served (not in headless tests).
pub fn build(app: &mut App) {
    if app.world().contains_resource::<bevy::asset::io::embedded::EmbeddedAssetRegistry>() {
        bevy::asset::embedded_asset!(app, "slice.wesl");
        app.add_plugins(bevy::ui_render::UiMaterialPlugin::<SliceMaterial>::default());
        app.init_resource::<WheelView>().add_systems(Update, draw.in_set(PlaySet::Ui));
    }
}

/// A slice's shape and colours: `from`/`to` in degrees clockwise from up, radii in
/// interface units.
fn slice_uniform(from: f32, to: f32, inner: f32, outer: f32, fill: Color, edge: Color, line: f32) -> SliceUniform {
    let half = SIZE / 2.0;
    SliceUniform {
        fill: fill.to_linear().to_vec4(),
        edge: edge.to_linear().to_vec4(),
        shape: Vec4::new(from.to_radians(), to.to_radians(), inner / half, outer / half),
        line: Vec4::new(line / half, 0.0, 0.0, 0.0),
    }
}

/// What a slice shows.
#[derive(Debug, Clone, Default, PartialEq)]
struct SliceView {
    icon: Option<String>,
    count: Option<String>,
    /// Nothing in it.
    empty: bool,
    /// Its item is not carried.
    gone: bool,
}

/// What the hub shows.
#[derive(Debug, Clone, Default, PartialEq)]
struct HubView {
    kind: String,
    name: String,
    /// Label, fill 0 to 1, value.
    bars: Vec<(&'static str, f32, String)>,
    /// Ammunition: name and counts, or the choices of slot 2 (name, carried, in use).
    ammo: Option<(String, String)>,
    choices: Vec<(String, u32, bool)>,
    note: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Shown {
    picked: usize,
    slices: Vec<SliceView>,
    hub: HubView,
    width: i32,
}

/// The fonts and pictures, read when first drawn.
struct Kit {
    large: Face,
    medium: Face,
    pictures: Pictures,
}

#[derive(Resource, Default)]
struct WheelView {
    kit: Option<Kit>,
    shown: Option<Shown>,
}

#[derive(Component)]
struct WheelRoot;

#[derive(Component)]
struct Darken;

fn kind_name(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Weapon => "Weapons",
        ItemKind::Armor => "Apparel",
        ItemKind::Aid => "Aid",
        ItemKind::Book => "Books",
        _ => "Items",
    }
}

fn ammo_name(c: &crate::combat::Combat, id: FormId) -> String {
    c.forms.ammo.get(&id).and_then(|a| a.short_name.clone().or_else(|| a.name.clone())).unwrap_or_else(|| id.to_string())
}

/// The slices and the hub for what the slots hold now.
fn shown(hotkeys: &Hotkeys, sim: &mut SimState, combat: Option<&crate::combat::Combat>, script: Option<&crate::script::ScriptState>, keep_chosen: bool, picked: usize, width: i32) -> Shown {
    let count = |sim: &SimState, id: Option<FormId>| crate::combat::player_count(sim, id);
    let in_hand = combat.and_then(|c| c.player_weapon());
    let mut slices = Vec::new();
    for (i, slot) in hotkeys.0.iter().enumerate() {
        if i == AMMO_SWAP {
            slices.push(SliceView { icon: Some(SWAP.into()), ..default() });
            continue;
        }
        let Some(slot) = slot else {
            slices.push(SliceView { empty: true, ..default() });
            continue;
        };
        let id = FormId(slot.item);
        let item = sim.forms.item(id);
        let (carried, _) = crate::hotkeys::carried(&mut sim.world, *slot);
        let weapon = combat.and_then(|c| c.forms.weapons.get(&id));
        let icon = item
            .and_then(|_| crate::hotkeys::icon_override(id, script))
            .or_else(|| item.and_then(|it| it.icon.clone()).filter(|p| !p.trim().is_empty()))
            .or_else(|| item.map(|it| crate::pipboy::hotkey_panel::type_icon(it.kind, weapon.and_then(|w| w.skill.as_deref())).to_string()));
        let n = match (item.map(|i| i.kind), combat, weapon) {
            (Some(ItemKind::Weapon), Some(c), Some(w)) if !c.forms.ammo_choices(w).is_empty() => {
                let ammo = if in_hand == Some(id) { c.player_ammo() } else { c.preferred_ammo_for(id, keep_chosen, |a| count(sim, Some(a)) > 0) };
                Some(count(sim, ammo).to_string())
            }
            (Some(ItemKind::Weapon), _, _) if carried > 1 => Some(carried.to_string()),
            (Some(ItemKind::Aid | ItemKind::Book), _, _) => Some(carried.max(0).to_string()),
            _ => None,
        };
        slices.push(SliceView { icon, count: n, empty: false, gone: carried <= 0 });
    }
    let hub = hub(hotkeys, sim, combat, keep_chosen, picked);
    Shown { picked, slices, hub, width }
}

fn hub(hotkeys: &Hotkeys, sim: &mut SimState, combat: Option<&crate::combat::Combat>, keep_chosen: bool, picked: usize) -> HubView {
    if picked == AMMO_SWAP {
        let mut h = HubView { kind: "Ammo".into(), name: "Ammo Swap".into(), ..default() };
        let w = combat.and_then(|c| Some((c, c.player_weapon()?)));
        if let Some((c, stats)) = w.and_then(|(c, id)| Some((c, c.forms.weapons.get(&id)?))) {
            let now = c.player_ammo();
            h.choices = c.forms.ammo_choices(stats).into_iter().map(|a| (ammo_name(c, a), crate::combat::player_count(sim, Some(a)), Some(a) == now)).collect();
        }
        if h.choices.is_empty() {
            h.note = Some("No ammunition to swap".into());
        }
        return h;
    }
    let Some(slot) = hotkeys.0[picked] else {
        return HubView { name: "Empty".into(), note: Some("Pip-Boy: select an item, press 1, 3-8".into()), ..default() };
    };
    let id = FormId(slot.item);
    let name = crate::menu::model::display_name(sim.forms, id);
    let Some(item) = sim.forms.item(id).cloned() else { return HubView { name, ..default() } };
    let (carried, _) = crate::hotkeys::carried(&mut sim.world, slot);
    let mut h = HubView { kind: kind_name(item.kind).into(), name, ..default() };
    let condition = sim.world.peek(FormId::PLAYER_REF).and_then(|s| s.inventory.as_ref()).and_then(|i| i.condition_in_use(id));
    let bar = |v: f32, full: f32| (v / full).clamp(0.0, 1.0);
    match (item.kind, combat) {
        (ItemKind::Weapon, Some(c)) => {
            if let Some(w) = c.forms.weapons.get(&id).map(|w| w.modded(slot.mods)) {
                if w.base_damage > 0 {
                    let dps = crate::pipboy::items::weapon_dps(&w, condition.unwrap_or(1.0));
                    h.bars.push(("DAM", bar(w.base_damage as f32, FULL_DAMAGE), w.base_damage.to_string()));
                    h.bars.push(("DPS", bar(dps, FULL_DPS), format!("{dps:.0}")));
                }
                let choices = c.forms.ammo_choices(&w);
                if !choices.is_empty() {
                    if c.player_weapon() == Some(id) {
                        let a = c.player_ammo();
                        let (loaded, besides) = c.hud_weapon(sim).ammo.unwrap_or((0, 0));
                        h.ammo = Some((a.map_or_else(String::new, |a| ammo_name(c, a)), format!("{loaded} / {}", loaded + besides)));
                    } else if let Some(a) = c.preferred_ammo_for(id, keep_chosen, |a| crate::combat::player_count(sim, Some(a)) > 0) {
                        h.ammo = Some((ammo_name(c, a), crate::combat::player_count(sim, Some(a)).to_string()));
                    }
                }
            }
        }
        (ItemKind::Armor, Some(c)) => {
            if let Some(a) = c.forms.armor.get(&id) {
                h.bars.push(("DT", bar(a.dt, FULL_DT), format!("{:.0}", a.dt)));
            }
        }
        _ => {}
    }
    if let Some(wg) = item.weight.filter(|w| *w > 0.0) {
        h.bars.push(("WG", bar(wg, FULL_WEIGHT), if wg.fract() == 0.0 { format!("{wg:.0}") } else { format!("{wg:.1}") }));
    }
    if let Some(v) = item.value.filter(|v| *v > 0) {
        h.bars.push(("VAL", bar(v as f32, FULL_VALUE), v.to_string()));
    }
    if matches!(item.kind, ItemKind::Weapon | ItemKind::Armor) {
        if let Some(cnd) = condition {
            h.bars.push(("CND", cnd.clamp(0.0, 1.0), format!("{:.0}%", cnd * 100.0)));
        }
    }
    if carried <= 0 {
        h.note = Some("Not carried".into());
    } else if matches!(item.kind, ItemKind::Aid | ItemKind::Book) {
        h.note = Some(format!("{carried} carried"));
    }
    h
}

/// Where a font's letters stand from a line's top: the tallest glyph's rise.
fn base_of(face: &Face) -> f32 {
    face.font.glyphs.iter().skip(0x20).take(0x5F).filter(|g| g.drawn()).map(|g| g.ascent).fold(0.0, f32::max).max(face.font.size * 0.75)
}

/// One line of text, its top at `top`: centred on `x` (`align` 0), from `x` (-1) or
/// ending at `x` (1).
pub(crate) fn text(p: &mut ChildSpawnerCommands, face: &Face, s: &str, x: f32, top: f32, align: i32, color: Color) {
    let Some(line) = face.font.lines(s.trim(), None).into_iter().next() else { return };
    let left = match align {
        0 => x - line.width / 2.0,
        a if a < 0 => x,
        _ => x - line.width,
    };
    set_line(p, face, &line, left, top + base_of(face), color);
}

#[allow(clippy::too_many_arguments)]
fn picture(p: &mut ChildSpawnerCommands, kit: &mut Kit, data: Option<&GameData>, images: &mut Assets<Image>, bc: bool, path: &str, node: Node, color: Color) {
    if let Some((image, rect)) = kit.pictures.picture(path, data, images, bc) {
        p.spawn((node, ImageNode { rect, color, image_mode: NodeImageMode::Stretch, ..ImageNode::new(image) }));
    }
}

#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    wheel: Res<WeaponWheel>,
    hotkeys: Res<Hotkeys>,
    mut sim: ResMut<SimState>,
    combat: Option<Res<crate::combat::Combat>>,
    script: Option<Res<crate::script::ScriptState>>,
    roots: Query<Entity, With<WheelRoot>>,
    mut darken: Query<(Entity, &mut BackgroundColor), With<Darken>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    (data, formats): (Option<Res<GameData>>, Option<Res<CompressedImageFormatSupport>>),
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<SliceMaterial>>,
    mut view: ResMut<WheelView>,
) {
    // The darkening follows the easing, open or closing.
    let shade = Color::BLACK.with_alpha(DARKEN * wheel.amount());
    match (darken.single_mut(), wheel.amount() > 0.0) {
        (Ok((_, mut bg)), true) => bg.0 = shade,
        (Ok((e, _)), false) => commands.entity(e).despawn(),
        (Err(_), true) => {
            commands.spawn((Darken, Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(shade), GlobalZIndex(104)));
        }
        (Err(_), false) => {}
    }
    let width = windows.single().map_or(1280.0, |w| 960.0 * w.width() / w.height().max(1.0));
    let keep_chosen = tweaks.as_deref().is_some_and(|t| t.keep_chosen_ammo);
    let now = wheel.open.map(|o| shown(&hotkeys, &mut sim, combat.as_deref(), script.as_deref(), keep_chosen, o.picked, width.round() as i32));
    if now == view.shown {
        return;
    }
    view.shown = now.clone();
    for e in &roots {
        commands.entity(e).despawn();
    }
    let Some(s) = now else { return };
    let bc = crate::interface::bc_supported(formats.as_deref());
    let data = data.as_deref();
    let kit = view.kit.get_or_insert_with(|| Kit {
        large: face(FONT_1, plain_font(31.0, 12.0), data, &mut images, bc),
        medium: face(FONT_3, plain_font(25.0, 10.0), data, &mut images, bc),
        pictures: Pictures::load(data),
    });
    let bright = amber(255.0);
    let dark = Color::srgb(0.08, 0.055, 0.025);
    let left = (width - SIZE) / 2.0;
    let top = (960.0 - SIZE) / 2.0 - RAISE;
    let c = SIZE / 2.0;
    let full = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
    let mut slice = |m: &mut ChildSpawnerCommands, u: SliceUniform| {
        m.spawn((at(0.0, 0.0, SIZE, SIZE), MaterialNode(materials.add(SliceMaterial { u }))));
    };
    // At the UI scale the wheel keeps to the view's middle (#1631); the darkening above
    // covers the view.
    commands.spawn((WheelRoot, crate::interface::framed_root(full, crate::interface::MIDDLE), GlobalZIndex(105))).with_children(|r| {
        r.spawn(at(left, top, SIZE, SIZE)).with_children(|m| {
            picture(m, kit, data, &mut images, bc, MATTE, at(0.0, 0.0, SIZE, SIZE), Color::BLACK.with_alpha(0.88));
            for i in 0..s.slices.len() {
                let mid = super::angle_of(i);
                let (from, to) = (mid - 22.5 + GAP, mid + 22.5 - GAP);
                let lit = i == s.picked;
                let (fill, edge) = if lit { (bright.with_alpha(0.27), bright.with_alpha(0.9)) } else { (dark.with_alpha(0.6), bright.with_alpha(0.35)) };
                slice(m, slice_uniform(from, to, INNER, OUTER, fill, edge, if lit { 1.5 * LINE } else { LINE }));
                if lit {
                    slice(m, slice_uniform(from, to, RIM[0], RIM[1], bright, bright, 0.0));
                }
            }
            slice(m, slice_uniform(0.0, 360.0, 0.0, HUB, dark.with_alpha(0.92), bright.with_alpha(0.63), LINE));
            for (i, v) in s.slices.iter().enumerate() {
                let lit = i == s.picked;
                let strength = if lit { 1.0 } else { DIM } * if v.gone { 0.5 } else { 1.0 };
                let mid = super::point(super::angle_of(i), (INNER + OUTER) / 2.0 + 4.0) + Vec2::splat(c);
                let number = super::point(super::angle_of(i), INNER + 24.0) + Vec2::splat(c);
                let size = kit.medium.font.size;
                text(m, &kit.medium, &(i + 1).to_string(), number.x, number.y - size / 2.0, 0, bright.with_alpha(if lit { 1.0 } else { 0.6 }));
                if v.empty {
                    text(m, &kit.medium, "EMPTY", mid.x, mid.y - size / 2.0, 0, bright.with_alpha(0.35));
                    continue;
                }
                if let Some(icon) = &v.icon {
                    let side = if i == AMMO_SWAP { SWAP_ICON } else { ICON };
                    picture(m, kit, data, &mut images, bc, icon, at(mid.x - side / 2.0, mid.y - side / 2.0 - 6.0, side, side), bright.with_alpha(strength));
                }
                if let Some(n) = &v.count {
                    text(m, &kit.medium, n, mid.x, mid.y + 40.0, 0, bright.with_alpha(strength.max(0.68)));
                }
            }
            hub_text(m, kit, &s.hub, c, bright);
        });
        let hint = "Release: use      1-8 or mouse: pick      Esc: cancel";
        text(r, &kit.medium, hint, width / 2.0, top + SIZE - 10.0, 0, bright.with_alpha(0.8));
    });
}

/// One thing the hub draws: `y` its top from the hub's middle, `half` how far out to
/// each side it may reach.
#[derive(Debug, Clone, PartialEq)]
enum Piece {
    Text { big: bool, s: String, y: f32, alpha: f32 },
    Bar { label: &'static str, fill: f32, value: String, y: f32, half: f32 },
    Choice { name: String, n: String, now: bool, y: f32, half: f32 },
    Rule { y: f32, half: f32 },
}

/// A text block before it is placed: its font (the large one may give way to the
/// medium), its words and its lines as set so far.
struct Block {
    big: bool,
    shrink: bool,
    source: String,
    lines: Vec<String>,
    alpha: f32,
}

enum Row {
    Text(Block),
    /// The figures or the ammunition choices, `ROW` each.
    Table(usize),
    Rule,
}

/// `s` cut short with dots until it is no wider than `width`.
fn clip(font: &fnv_font::Font, s: &str, width: f32) -> String {
    if font.width(s) <= width {
        return s.to_string();
    }
    let mut chars: Vec<char> = s.chars().collect();
    let cut = |chars: &[char]| format!("{}...", chars.iter().collect::<String>().trim_end());
    while !chars.is_empty() && font.width(&cut(&chars)) > width {
        chars.pop();
    }
    cut(&chars)
}

/// `s` set in lines no wider than `width`, two at most (the second cut short).
fn wrap(font: &fnv_font::Font, s: &str, width: f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for w in s.split_whitespace() {
        let full = lines.len() == 2;
        match lines.last_mut() {
            Some(l) if full || font.width(&format!("{l} {w}")) <= width => {
                l.push(' ');
                l.push_str(w);
            }
            _ => lines.push(w.to_string()),
        }
    }
    lines.into_iter().map(|l| clip(font, &l, width)).collect()
}

/// Where the hub's text and figures go, every piece inside the circle of radius `r`
/// round the hub's middle: a block too wide where it stands takes the medium font, then
/// a second line, then is cut short; the kind gives way when the whole is too tall.
fn hub_layout(h: &HubView, large: &fnv_font::Font, medium: &fnv_font::Font, r: f32) -> Vec<Piece> {
    let half_at = |y0: f32, y1: f32| {
        let d = y0.abs().max(y1.abs()).min(r);
        (r * r - d * d).sqrt()
    };
    let font = |big: bool| if big { large } else { medium };
    let table = h.bars.len().min(6).max(h.choices.len().min(5));
    let bottom = h.ammo.as_ref().map(|(name, counts)| vec![format!("<  {name}  >"), counts.clone()]).or_else(|| h.note.clone().map(|n| vec![n]));
    let block = |big: bool, s: String, alpha: f32| Row::Text(Block { big, shrink: big, lines: vec![s.clone()], source: s, alpha });
    for with_kind in [true, false] {
        let mut rows = Vec::new();
        if with_kind && !h.kind.is_empty() {
            rows.push(block(false, h.kind.to_uppercase(), 0.75));
        }
        rows.push(block(true, h.name.trim().to_string(), 1.0));
        rows.push(Row::Rule);
        if table > 0 {
            rows.push(Row::Table(table));
            if bottom.is_some() {
                rows.push(Row::Rule);
            }
        }
        for (k, s) in bottom.iter().flatten().enumerate() {
            rows.push(block(false, s.clone(), if h.ammo.is_some() && k == 0 { 1.0 } else { 0.8 }));
        }
        let height = |row: &Row| match row {
            Row::Text(b) => b.lines.len() as f32 * font(b.big).size,
            Row::Table(n) => *n as f32 * ROW,
            Row::Rule => RULE_ROOM,
        };
        // A change only grows the stack, moving blocks out to where the circle is
        // narrower, so a few rounds settle it; what is still too wide is cut below.
        for _ in 0..6 {
            let mut y = -rows.iter().map(height).sum::<f32>() / 2.0;
            let mut changed = false;
            for row in &mut rows {
                let tall = height(row);
                if let Row::Text(b) = row {
                    let room = 2.0 * half_at(y, y + tall);
                    if b.lines.iter().any(|l| font(b.big).width(l) > room) {
                        if b.shrink && b.big {
                            b.big = false;
                        } else {
                            b.lines = wrap(font(b.big), &b.source, room);
                        }
                        changed = true;
                    }
                }
                y += tall;
            }
            if !changed {
                break;
            }
        }
        let total: f32 = rows.iter().map(height).sum();
        if total > 2.0 * r && with_kind && !h.kind.is_empty() {
            continue;
        }
        let mut out = Vec::new();
        let mut y = -total / 2.0;
        for row in &rows {
            let tall = height(row);
            match row {
                Row::Text(b) => {
                    let size = font(b.big).size;
                    for (k, l) in b.lines.iter().enumerate() {
                        let top = y + k as f32 * size;
                        let room = 2.0 * half_at(top, top + size);
                        out.push(Piece::Text { big: b.big, s: clip(font(b.big), l, room), y: top, alpha: b.alpha });
                    }
                }
                Row::Rule => out.push(Piece::Rule { y: y + RULE_ROOM / 2.0, half: half_at(y, y + RULE_ROOM).min(110.0) }),
                Row::Table(_) => {
                    let half = half_at(y, y + tall).min(BAR_HALF);
                    for (k, (label, fill, value)) in h.bars.iter().take(6).enumerate() {
                        out.push(Piece::Bar { label, fill: *fill, value: value.clone(), y: y + k as f32 * ROW, half });
                    }
                    for (k, (name, n, now)) in h.choices.iter().take(5).enumerate() {
                        let n = n.to_string();
                        let room = 2.0 * half - medium.width(&n) - medium.width("> ") - 12.0;
                        out.push(Piece::Choice { name: clip(medium, name, room), n, now: *now, y: y + k as f32 * ROW, half });
                    }
                }
            }
            y += tall;
        }
        return out;
    }
    Vec::new()
}

fn hub_text(m: &mut ChildSpawnerCommands, kit: &Kit, h: &HubView, c: f32, bright: Color) {
    for piece in hub_layout(h, &kit.large.font, &kit.medium.font, HUB - HUB_PAD) {
        match piece {
            Piece::Text { big, s, y, alpha } => text(m, if big { &kit.large } else { &kit.medium }, &s, c, c + y, 0, bright.with_alpha(alpha)),
            Piece::Rule { y, half } => {
                m.spawn((at(c - half, c + y, 2.0 * half, 1.5), BackgroundColor(bright.with_alpha(0.5))));
            }
            Piece::Bar { label, fill, value, y, half } => {
                let y = c + y;
                text(m, &kit.medium, label, c - half, y, -1, bright.with_alpha(0.9));
                let (x0, w) = (c - half + 54.0, 2.0 * half - 138.0);
                m.spawn((Node { border: UiRect::all(Val::Px(1.0)), ..at(x0, y + 6.0, w, 10.0) }, BorderColor::all(bright.with_alpha(0.45))));
                m.spawn((at(x0 + 2.0, y + 8.0, (w - 4.0) * fill, 6.0), BackgroundColor(bright.with_alpha(0.9))));
                text(m, &kit.medium, &value, c + half, y, 1, bright.with_alpha(0.9));
            }
            Piece::Choice { name, n, now, y, half } => {
                let a = if now { 1.0 } else { 0.62 };
                text(m, &kit.medium, &format!("{}{name}", if now { "> " } else { "  " }), c - half, c + y, -1, bright.with_alpha(a));
                text(m, &kit.medium, &n, c + half, c + y, 1, bright.with_alpha(a));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slices_sit_inside_the_square() {
        let u = slice_uniform(-22.5, 22.5, INNER, OUTER, Color::BLACK, Color::WHITE, LINE);
        assert!((u.shape.w - OUTER / (SIZE / 2.0)).abs() < 1e-6);
    }

    /// Every piece's corners inside the circle of radius `r`.
    fn inside(pieces: &[Piece], large: &fnv_font::Font, medium: &fnv_font::Font, r: f32) {
        let ok = |x: f32, y: f32| x * x + y * y <= r * r + 0.5;
        for p in pieces {
            let (w, y, tall) = match p {
                Piece::Text { big, s, y, .. } => {
                    let f = if *big { large } else { medium };
                    (f.width(s) / 2.0, *y, f.size)
                }
                Piece::Bar { y, half, .. } | Piece::Choice { y, half, .. } => (*half, *y, ROW),
                Piece::Rule { y, half } => (*half, *y, 1.5),
            };
            assert!(ok(w, y) && ok(w, y + tall), "{p:?} leaves the hub");
        }
    }

    #[test]
    fn the_hub_keeps_everything_inside() {
        // About as wide as the game's fonts [observed: the owner's pictures].
        let large = crate::interface::plain_font(31.0, 18.0);
        let medium = crate::interface::plain_font(25.0, 15.0);
        let r = HUB - HUB_PAD;
        let bar = |l| (l, 0.5, "100%".to_string());
        let weapon = HubView {
            kind: "Weapons".into(),
            name: "Sturdy Caravan Shotgun".into(),
            bars: vec![bar("DAM"), bar("DPS"), bar("WG"), bar("VAL"), bar("CND")],
            ammo: Some(("20ga".into(), "2 / 40".into())),
            ..default()
        };
        let empty = HubView { name: "Empty".into(), note: Some("Pip-Boy: select an item, press 1, 3-8".into()), ..default() };
        let swap = HubView {
            kind: "Ammo".into(),
            name: "Ammo Swap".into(),
            choices: (0..5).map(|k| (format!("Very Long Ammunition Name {k}"), 1234, k == 0)).collect(),
            ..default()
        };
        let long = HubView {
            kind: "Apparel".into(),
            name: "Lucky Shades of the Very Long Mod Name Collection".into(),
            bars: vec![bar("DT"), bar("WG"), bar("VAL"), bar("CND")],
            note: Some("Not carried".into()),
            ..default()
        };
        for h in [&weapon, &empty, &swap, &long] {
            inside(&hub_layout(h, &large, &medium, r), &large, &medium, r);
        }
        // Nothing is lost when it fits: the weapon keeps its kind, its figures and its
        // whole name over two lines; the empty slot's hint is whole.
        let texts = |h: &HubView| -> Vec<String> {
            hub_layout(h, &large, &medium, r).into_iter().filter_map(|p| if let Piece::Text { s, .. } = p { Some(s) } else { None }).collect()
        };
        assert_eq!(texts(&weapon), ["WEAPONS", "Sturdy Caravan", "Shotgun", "<  20ga  >", "2 / 40"]);
        assert_eq!(hub_layout(&weapon, &large, &medium, r).iter().filter(|p| matches!(p, Piece::Bar { .. })).count(), 5);
        assert!(texts(&empty).iter().all(|l| !l.ends_with("...")), "{:?}", texts(&empty));
    }

    #[test]
    fn hotkey_icon_override_updates_and_restores_weapon_wheel_slice_icon() {
        use fnv_game::game::RunCtx;
        use fnv_sim::forms::{BaseData, BaseForm, FormStore, Item};

        const PISTOL: FormId = FormId(0x0000_434F);
        const AID: FormId = FormId(0x0001_5169);

        let mut forms = FormStore::default();
        let make_item = |kind: ItemKind, icon: Option<&str>| Item {
            kind,
            value: Some(10),
            money: None,
            weight: Some(1.0),
            slots: None,
            projectile: None,
            health: None,
            non_playable: false,
            cant_drop: false,
            armor_class: Default::default(),
            power_armor: false,
            armor_dr: None,
            armor_dt: None,
            weapon_damage: None,
            weight_mods: [0.0; 3],
            drawable: false,
            not_for_npcs: false,
            thrown: false,
            quest_item: false,
            icon: icon.map(str::to_string),
            placed_count: None,
            teaches: None,
            cant_be_taken: false,
            modulates_voice: false,
            weapon_traits: None,
        };
        forms.bases.insert(PISTOL, BaseForm { id: PISTOL, sig: *b"WEAP", edid: Some("TestPistol".into()), name: Some("Test Pistol".into()), script: None, prompt: None, voice_filter: false, data: BaseData::Item(make_item(ItemKind::Weapon, Some("Interface\\Icons\\PipboyImages\\Weapons\\test_pistol.dds"))) });
        forms.bases.insert(AID, BaseForm { id: AID, sig: *b"ALCH", edid: Some("TestAid".into()), name: Some("Test Aid".into()), script: None, prompt: None, voice_filter: false, data: BaseData::Item(make_item(ItemKind::Aid, None)) });

        let forms: &'static FormStore = Box::leak(Box::new(forms));
        let refs: &'static fnv_sim::RefTable = Box::leak(Box::new(fnv_sim::RefTable::default()));
        let mut sim = SimState { forms, refs, world: fnv_sim::World::new(forms, refs, fnv_sim::SimConfig::default()) };
        let mut hotkeys = Hotkeys::default();
        hotkeys.0[0] = Some(crate::hotkeys::Slot { item: PISTOL.0, mods: 0 });
        hotkeys.0[2] = Some(crate::hotkeys::Slot { item: AID.0, mods: 0 });

        let mut script = crate::script::ScriptState::over_no_plugins();

        // Vanilla (no override): slot 1 uses ICON, slot 2 stays Ammo Swap, slot 3 uses type_icon.
        let baseline = shown(&hotkeys, &mut sim, None, Some(&script), false, 0, 1280);
        assert_eq!(baseline.slices[0].icon.as_deref(), Some("Interface\\Icons\\PipboyImages\\Weapons\\test_pistol.dds"));
        assert_eq!(baseline.slices[AMMO_SWAP].icon.as_deref(), Some(SWAP));
        assert_eq!(baseline.slices[2].icon.as_deref(), Some(crate::pipboy::hotkey_panel::type_icon(ItemKind::Aid, None)));
        assert_eq!(shown(&hotkeys, &mut sim, None, None, false, 0, 1280), baseline);

        // SetItemHotkeyIconPath with leading textures/ and forward slashes normalises cleanly.
        let run = |script: &mut crate::script::ScriptState, body: &str| {
            script.rt.as_mut().unwrap().game.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), RunCtx::default()).unwrap();
        };
        run(&mut script, &format!(r#"SetItemHotkeyIconPath {} "Textures/Interface/Icons/CustomWheel.dds""#, PISTOL.0));
        let custom = shown(&hotkeys, &mut sim, None, Some(&script), false, 0, 1280);
        assert_ne!(custom, baseline, "live wheel redraw triggers on override");
        assert_eq!(custom.slices[0].icon.as_deref(), Some("interface\\icons\\customwheel.dds"));
        assert_eq!(custom.slices[AMMO_SWAP].icon.as_deref(), Some(SWAP));
        assert_eq!(custom.slices[2].icon.as_deref(), Some(crate::pipboy::hotkey_panel::type_icon(ItemKind::Aid, None)));

        // Live update replaces the icon immediately.
        run(&mut script, &format!(r#"SetItemHotkeyIconPath {} "\\interface\\icons\\updated_wheel.dds""#, PISTOL.0));
        let updated = shown(&hotkeys, &mut sim, None, Some(&script), false, 0, 1280);
        assert_ne!(updated, custom);
        assert_eq!(updated.slices[0].icon.as_deref(), Some("interface\\icons\\updated_wheel.dds"));

        // Clearing via "" restores the item's default icon.
        run(&mut script, &format!(r#"SetItemHotkeyIconPath {} """#, PISTOL.0));
        let restored = shown(&hotkeys, &mut sim, None, Some(&script), false, 0, 1280);
        assert_eq!(restored, baseline);
    }
}
