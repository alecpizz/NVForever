//! T12 leftovers: the HUD's texts drawn from tiles while `[UI] TileMenus=HUDMainMenu` is on
//! (`super::vanilla`): the corner messages (`Messages`), the quest and discovery notices
//! (`QuestReminder`), the discovery notices again (`Region_Location`) and the subtitles
//! (`Subtitles`) [design: hud-xml.md §6 step 5, docs/spec-notes/hud.md §14.11].
//!
//! - **What shows and when** stays the hand-built HUD's: the queues, their times and fades
//!   (`crate::corner_messages::CornerMessages`, `crate::quest_messages::QuestMessages`) and
//!   the subtitle selection (`crate::script_hud`). The adapter (`super::HudTiles::notices`)
//!   reads them; [`write`] turns them into code-made copies of the HUD's templates and
//!   their traits, which the tile renderer draws (`super::draw`, `Select::Whole`).
//! - **Only while the switch is on** [design]: with it off nothing here is made or
//!   written, so the tree stays T4's (its `*:N` paths included).
//! - The file's pieces are empty rectangles: the original's code makes their contents
//!   [documented: hud_main_menu.xml, `Messages`, `Subtitles`, `QuestReminder`,
//!   `Region_Location`]. What its copies are under `Messages` and `Subtitles` was read
//!   (hud.md §14.11); under `QuestReminder` none was there before any notice, and
//!   `Region_Location` had none [observed: private run hud_traits run1].
//! - **A discovery is written twice, drawn once** [hypothesis: hud.md §14 maps discoveries
//!   to `Region_Location`; the capture saw no copies there, so their layout is the quest
//!   banner's]: `QuestReminder`'s banner copies carry the picture, as the hand-built HUD
//!   draws it inside that piece's rect (45, 200, 400 x 200, where the owner's screenshot
//!   shows it); `Region_Location`'s label and name copies hold the same strings, places
//!   and alpha with `visible` 0, for the traits scripts read.

use fnv_ui::{MemSource, TileId, Ui, Value};

/// The HUD's menu.
const HUD: &str = "HUDMainMenu";
/// NVForever's own trait on these copies, naming the part each plays, so they are found
/// again without paths (copies of one template share a name) [design].
pub const ROLE: &str = "_NVFHudNotice";

/// A corner message on screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub text: String,
    /// The icon's texture file under the textures folder (the note's, else the default).
    pub icon: String,
    /// How strongly it is drawn, 0 to 1 (`CornerMessages::strength`).
    pub strength: f64,
}

/// A quest line on screen.
#[derive(Clone, Debug, PartialEq)]
pub enum Quest {
    /// "New quest" (and its kinds, "Place found") over the name in capitals;
    /// `discovered` for a map discovery, which `Region_Location` gets too.
    Banner { label: String, name: String, strength: f64, discovered: bool },
    /// Objectives, each with its text (with "COMPLETED: " when done) and whether it is.
    Objectives { list: Vec<(String, bool)>, strength: f64 },
}

/// What the three areas show this frame; `None` for nothing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Notices {
    pub message: Option<Message>,
    pub quest: Option<Quest>,
    /// The subtitles drawn, one text a line.
    pub subtitles: Vec<String>,
}

// ---- Messages [observed: hud.md §14.11, the code-made children `*:0`-`*:2`] -------------

/// The message's text: its place in `Messages` and its depth [observed], wrapped at what is
/// left of the area's width, as the hand-built HUD breaks it [hypothesis:
/// `corner_messages::WRAP`; the copy's `wrapwidth` was not read].
pub const MESSAGE_TEXT: (f64, f64) = (crate::corner_messages::TEXT_X as f64, 5.0);
pub const MESSAGE_TEXT_DEPTH: f64 = 2.0;
pub const MESSAGE_WRAP: f64 = crate::corner_messages::WRAP as f64;
/// The bracket drawn fainter than the icon and the text, as the hand-built HUD's
/// [observed: corner-messages.md §4; `corner_messages::BRACKET_STRENGTH`].
pub const BRACKET_STRENGTH: f64 = crate::corner_messages::BRACKET_STRENGTH as f64;

// ---- QuestReminder [observed: the hand-built HUD's places, quest-messages.md §4] ---------

/// `QuestReminder`'s place [observed: hud.md §14], which the hand-built HUD's places are
/// taken from: its texts start at its x (`quest_messages::TEXT_X`, 45).
const QUEST_PIECE: (f64, f64) = (45.0, 200.0);
/// The label's and the objectives' font, their template's [documented: hud.md §2, the
/// three text templates all in font 7].
const TEXT_FONT: u8 = 7;

/// How far a text tile's first line stands under its top in `font`
/// (`super::draw::text_base`).
fn base(font: u8) -> f64 {
    f64::from(super::draw::text_base(font))
}

fn quest_x(x: f32) -> f64 {
    f64::from(crate::quest_messages::TEXT_X + x) - QUEST_PIECE.0
}

/// The top of a text tile in `font` whose first line stands on the hand-built HUD's
/// `line`, from `QuestReminder`'s top.
fn quest_y(line: f32, font: u8) -> f64 {
    f64::from(line) - base(font) - QUEST_PIECE.1
}

/// `Region_Location`'s place [observed: hud.md §14, 100 x 100 at (45, 728)].
const LOCATION_PIECE: (f64, f64) = (45.0, 728.0);

/// A place from `QuestReminder`'s top left as one from `Region_Location`'s: the discovery's
/// copies there stand where the hand-built HUD draws it [hypothesis: hud.md §14; the
/// capture saw no copies there, so their layout is the quest banner's].
pub fn location_place((x, y): (f64, f64)) -> (f64, f64) {
    (x + QUEST_PIECE.0 - LOCATION_PIECE.0, y + QUEST_PIECE.1 - LOCATION_PIECE.1)
}

/// The label ("New quest"), the name (font 8, [observed: quest-messages.md §4]) and the
/// first objective, from `QuestReminder`'s top left: each tile's top its font's line
/// (`super::draw::text_base`) over the hand-built HUD's line, so the tiles draw the texts
/// where it does (#979: the name's top 30 over its line, the label's and the
/// objectives' 24).
pub fn label_place() -> (f64, f64) {
    (quest_x(0.0), quest_y(crate::quest_messages::LABEL_BASE, TEXT_FONT))
}
pub fn title_place() -> (f64, f64) {
    (quest_x(crate::quest_messages::TITLE_X), quest_y(crate::quest_messages::TITLE_BASE, super::draw::font_number(TITLE_FONT)))
}
pub fn list_place() -> (f64, f64) {
    (quest_x(0.0), quest_y(crate::quest_messages::LIST_BASE, TEXT_FONT))
}
pub const TITLE_FONT: f64 = 8.0;
pub const LIST_WRAP: f64 = crate::quest_messages::WRAP as f64;

/// The box before an objective, from its text's top left, both kinds 26 x 26 [observed:
/// quest-messages.md §4-§5, the screenshots' square; the file's templates say x -20, y 3,
/// 26 x 26 filled and 16 x 16 open].
pub fn box_rect() -> [f64; 4] {
    let [x, y, w, h] = crate::quest_messages::BOX;
    [f64::from(x), f64::from(y) + base(TEXT_FONT), f64::from(w), f64::from(h)]
}

// ---- Subtitles [observed: hud.md §14.11, `Subtitles/*:0`] --------------------------------

/// The subtitle text: centre-justified at the area's middle [observed: x 400 of 800; its
/// template by the justification, hypothesis], wrapped at the area's width as the
/// hand-built HUD's [hypothesis: `script_hud::SUBTITLE_WIDTH`; not read].
pub const SUBTITLE_X: f64 = 400.0;
pub const SUBTITLE_WRAP: f64 = crate::script_hud::SUBTITLE_WIDTH as f64;
/// Its string before any subtitle [observed: " ", 13 wide].
pub const SUBTITLE_EMPTY: &str = " ";

fn num(ui: &mut Ui, tile: TileId, name: &str, value: f64) {
    ui.set_trait(tile, name, Value::Num(value));
}

fn string(ui: &mut Ui, tile: TileId, value: &str) {
    ui.set_trait(tile, "string", Value::Str(value.into()));
}

/// This piece's copies playing `role`, in the order they were made.
pub fn copies(ui: &Ui, piece: &str, role: &str) -> Vec<TileId> {
    let Some(parent) = ui.find_tile(&format!("{HUD}/{piece}")) else { return Vec::new() };
    ui.tile(parent).children.iter().copied().filter(|&id| ui.tile(id).live && ui.tile(id).get(ROLE).is_some_and(|v| v.text() == role)).collect()
}

/// A code-made copy of `template` under `piece`, marked with its template (as
/// `fnv_game::ui_hud`'s copies) and its role.
fn make(ui: &mut Ui, piece: &str, template: &str, role: &str) -> Option<TileId> {
    let tile = ui.add_code_tile_from_template(&MemSource::default(), &format!("{HUD}/{piece}"), template, None)?;
    ui.set_trait(tile, "_NVFHudTemplate", Value::Str(template.into()));
    ui.set_trait(tile, ROLE, Value::Str(role.into()));
    Some(tile)
}

/// The copy playing `role`, made when the piece has none.
fn one(ui: &mut Ui, piece: &str, template: &str, role: &str) -> Option<TileId> {
    copies(ui, piece, role).first().copied().or_else(|| make(ui, piece, template, role))
}

/// `n` copies playing `role`, more made as needed (the surplus is the caller's to hide).
fn pool(ui: &mut Ui, piece: &str, template: &str, role: &str, n: usize) -> Vec<TileId> {
    let mut have = copies(ui, piece, role);
    while have.len() < n {
        match make(ui, piece, template, role) {
            Some(t) => have.push(t),
            None => break,
        }
    }
    have
}

/// A text's line count at a wrap width, and the line height.
type LineCount<'a> = dyn Fn(&str, Option<f64>) -> (usize, f64) + 'a;

/// Write the three areas' copies, after T4's writes, before scripts, only while the HUD
/// is a tile menu (`super::write`). `lines` gives a text's line count at a wrap width and
/// the line height, in font 7 (the objectives' places follow the lines before them).
pub fn write(ui: &mut Ui, n: &Notices, lines: &LineCount<'_>) {
    if ui.find_tile(HUD).is_none() {
        return;
    }
    write_message(ui, n.message.as_ref());
    write_quest(ui, n.quest.as_ref(), lines);
    write_location(ui, n.quest.as_ref());
    write_subtitles(ui, &n.subtitles);
}

/// `Messages`: the icon, the text and the bracket, made in that order so the bracket is
/// `*:0`, the text `*:1` and the icon `*:2`, as the original's [observed: hud.md §14.11;
/// fnv-ui counts `*:N` from the last child, §14.8]. All three stay, at alpha 0 with the
/// last text, when no message shows [observed: alpha 0 with a message's text held].
fn write_message(ui: &mut Ui, m: Option<&Message>) {
    if ui.find_tile(&format!("{HUD}/Messages")).is_none() {
        return;
    }
    let icon = one(ui, "Messages", "template_message_icon", "icon");
    let text = one(ui, "Messages", "template_justify_left_text", "text");
    let bracket = one(ui, "Messages", "template_message_bracket", "bracket");
    let strength = m.map_or(0.0, |m| m.strength.clamp(0.0, 1.0));
    if let Some(t) = icon {
        if let Some(m) = m {
            ui.set_trait(t, "filename", Value::Str(m.icon.clone()));
        }
        num(ui, t, "alpha", 255.0 * strength);
    }
    if let Some(t) = text {
        num(ui, t, "x", MESSAGE_TEXT.0);
        num(ui, t, "y", MESSAGE_TEXT.1);
        num(ui, t, "depth", MESSAGE_TEXT_DEPTH);
        num(ui, t, "wrapwidth", MESSAGE_WRAP);
        num(ui, t, "visible", 1.0);
        if let Some(m) = m {
            string(ui, t, m.text.trim());
        }
        num(ui, t, "alpha", 255.0 * strength);
    }
    if let Some(t) = bracket {
        num(ui, t, "alpha", 255.0 * BRACKET_STRENGTH * strength);
    }
}

/// `QuestReminder`: made at the first notice [observed: none there before one, hud.md
/// §14.11]; directly under the piece at the hand-built HUD's places [hypothesis: which of
/// its rects, `QuestStages` (y 40) or `QuestAdded` (y 80), the original's copies go under is
/// not read; the first objective's top, 39, is next to `QuestStages`' 40]. A discovery is
/// a banner here too [observed: the owner's screenshot "Discovered" at "New quest"'s place
/// and fonts, quest-messages.md].
fn write_quest(ui: &mut Ui, q: Option<&Quest>, lines: &LineCount<'_>) {
    if ui.find_tile(&format!("{HUD}/QuestReminder")).is_none() {
        return;
    }
    let made = !copies(ui, "QuestReminder", "label").is_empty();
    if q.is_none() && !made {
        return;
    }
    let label = one(ui, "QuestReminder", "template_justify_left_text", "label");
    let title = one(ui, "QuestReminder", "template_justify_left_text", "title");
    let (banner, list, strength) = match q {
        Some(Quest::Banner { label, name, strength, .. }) => (Some((label.as_str(), name.as_str())), &[][..], *strength),
        Some(Quest::Objectives { list, strength }) => (None, list.as_slice(), *strength),
        None => (None, &[][..], 0.0),
    };
    let alpha = 255.0 * strength.clamp(0.0, 1.0);
    for (tile, place, font, text) in [(label, label_place(), None, banner.map(|b| b.0.to_string())), (title, title_place(), Some(TITLE_FONT), banner.map(|b| b.1.trim().to_uppercase()))] {
        let Some(t) = tile else { continue };
        num(ui, t, "x", place.0);
        num(ui, t, "y", place.1);
        if let Some(f) = font {
            num(ui, t, "font", f);
        }
        num(ui, t, "visible", 1.0);
        if let Some(s) = &text {
            string(ui, t, s);
        }
        // The name fades as one: a tile is one text, the hand-built HUD's letters come one
        // after another [design].
        num(ui, t, "alpha", if text.is_some() { alpha } else { 0.0 });
    }
    // The objectives, one text tile each, and a box of its kind before it.
    let texts = pool(ui, "QuestReminder", "template_justify_left_text", "objective", list.len());
    let open = pool(ui, "QuestReminder", "template_checkbox", "open_box", list.iter().filter(|o| !o.1).count());
    let done = pool(ui, "QuestReminder", "template_filled_checkbox", "done_box", list.iter().filter(|o| o.1).count());
    let (x, mut y) = list_place();
    let pitch_gap = f64::from(crate::quest_messages::LEADING);
    let (mut next_open, mut next_done) = (0, 0);
    let [bx, by, bw, bh] = box_rect();
    for (i, &t) in texts.iter().enumerate() {
        let Some((text, completed)) = list.get(i) else {
            num(ui, t, "visible", 0.0);
            continue;
        };
        string(ui, t, text);
        num(ui, t, "x", x);
        num(ui, t, "y", y);
        num(ui, t, "wrapwidth", LIST_WRAP);
        num(ui, t, "visible", 1.0);
        num(ui, t, "alpha", alpha);
        let b = if *completed {
            next_done += 1;
            done.get(next_done - 1)
        } else {
            next_open += 1;
            open.get(next_open - 1)
        };
        if let Some(&b) = b {
            for (name, v) in [("x", x + bx), ("y", y + by), ("width", bw), ("height", bh), ("visible", 1.0), ("alpha", alpha)] {
                num(ui, b, name, v);
            }
        }
        // The next objective under this one's lines, the font's size and the hand-built
        // HUD's 2 apart (`quest_messages::LEADING`) [observed: quest-messages.md §4].
        let (count, size) = lines(text, Some(LIST_WRAP));
        y += count.max(1) as f64 * (size + pitch_gap);
    }
    for &b in open.iter().skip(next_open).chain(done.iter().skip(next_done)) {
        num(ui, b, "visible", 0.0);
    }
}

/// `Region_Location`: a discovery's label and name, made at the first discovery, at the
/// quest banner's places moved to this piece's top left and in its fonts [hypothesis: hud.md
/// §14 maps discoveries to `Region_Location`; the capture saw no copies there, so their
/// layout is the quest banner's]. The hand-built HUD's discovery has no bracket
/// (`quest_messages::layout`: the label and the name only), so neither do these. Their
/// strings and alpha follow the queue as `QuestReminder`'s, alpha 0 with the last
/// discovery held while another notice or none shows; `visible` stays 0: the picture is
/// `QuestReminder`'s, drawn inside its rect as the hand-built HUD's, so it is drawn once.
fn write_location(ui: &mut Ui, q: Option<&Quest>) {
    if ui.find_tile(&format!("{HUD}/Region_Location")).is_none() {
        return;
    }
    let found = match q {
        Some(Quest::Banner { label, name, strength, discovered: true }) => Some((label.as_str(), name.as_str(), *strength)),
        _ => None,
    };
    if found.is_none() && copies(ui, "Region_Location", "label").is_empty() {
        return;
    }
    let label = one(ui, "Region_Location", "template_justify_left_text", "label");
    let title = one(ui, "Region_Location", "template_justify_left_text", "title");
    let alpha = found.map_or(0.0, |f| 255.0 * f.2.clamp(0.0, 1.0));
    for (tile, place, font, text) in [(label, label_place(), None, found.map(|f| f.0.to_string())), (title, title_place(), Some(TITLE_FONT), found.map(|f| f.1.trim().to_uppercase()))] {
        let Some(t) = tile else { continue };
        let (x, y) = location_place(place);
        num(ui, t, "x", x);
        num(ui, t, "y", y);
        if let Some(f) = font {
            num(ui, t, "font", f);
        }
        num(ui, t, "visible", 0.0);
        if let Some(s) = &text {
            string(ui, t, s);
        }
        num(ui, t, "alpha", alpha);
    }
}

/// `Subtitles`: one centre-justified text, there from the start with " " and hidden
/// [observed: `*:0` x 400, visible 0, alpha 255, " "]; shown with the subtitles' lines, one
/// text a line, and hidden again after them keeping the last [hypothesis].
fn write_subtitles(ui: &mut Ui, subtitles: &[String]) {
    if ui.find_tile(&format!("{HUD}/Subtitles")).is_none() {
        return;
    }
    let fresh = copies(ui, "Subtitles", "text").is_empty();
    let Some(t) = one(ui, "Subtitles", "template_justify_center_text", "text") else { return };
    if fresh {
        string(ui, t, SUBTITLE_EMPTY);
        num(ui, t, "alpha", 255.0);
    }
    num(ui, t, "x", SUBTITLE_X);
    num(ui, t, "wrapwidth", SUBTITLE_WRAP);
    let text: Vec<&str> = subtitles.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    if !text.is_empty() {
        string(ui, t, &text.join("\n"));
    }
    num(ui, t, "visible", f64::from(!text.is_empty()));
}

/// The text copies [`write`] makes, in every area: their widths are written after it
/// ([`write_widths`]).
pub fn texts(ui: &Ui) -> Vec<TileId> {
    [("Messages", "text"), ("QuestReminder", "label"), ("QuestReminder", "title"), ("QuestReminder", "objective"), ("Region_Location", "label"), ("Region_Location", "title"), ("Subtitles", "text")]
        .into_iter()
        .flat_map(|(piece, role)| copies(ui, piece, role))
        .collect()
}

/// Write each text copy's `width` from its string just written, before scripts read it, as
/// the HUD's other text tiles get theirs (`fnv_game::ui_hud::write_text_widths`) [design:
/// ui-tile-tree.md §4.1]. `measure` gives a text's width in a font at a wrap width: a
/// wrapped text is as wide as its widest line [hypothesis: hud.md §14.11 read `Messages/*:1`
/// 281 wide, under the 330 the hand-built HUD wraps at; the width of a copy holding a
/// wrapped string was not read].
///
/// `last` keeps what each copy was last measured from: a copy whose string, font, wrap
/// width and written width are all as they were is not measured again (#1062; seven or
/// more text layouts a frame otherwise, with nothing changed).
pub fn write_widths(ui: &mut Ui, last: &mut WidthsWritten, measure: &dyn Fn(f64, &str, Option<f64>) -> f64) {
    let tiles = texts(ui);
    last.retain(|id, _| tiles.contains(id));
    for tile in tiles {
        let t = ui.tile(tile);
        let Some(string) = t.get("string").map(Value::text) else { continue };
        let font = t.get("font").map_or(0.0, Value::num);
        let wrap = t.get("wrapwidth").map(Value::num).filter(|w| *w > 0.0);
        let width = t.get("width").map(Value::num);
        if let Some(w) = last.get(&tile) {
            if w.string == string && w.font == font && w.wrap == wrap && Some(w.width) == width {
                continue;
            }
        }
        let width = measure(font, &string, wrap);
        ui.set_trait(tile, "width", Value::Num(width));
        last.insert(tile, Written { string, font, wrap, width });
    }
}

/// What a text copy's width was last measured from, and the width written.
#[derive(Clone, Debug, PartialEq)]
pub struct Written {
    string: String,
    font: f64,
    wrap: Option<f64>,
    width: f64,
}

/// [`Written`] by copy, for [`write_widths`].
pub type WidthsWritten = std::collections::HashMap<TileId, Written>;

/// A line for the log when what an area shows changes (smoke reads these).
pub fn describe(n: &Notices) -> [Option<String>; 4] {
    [
        n.message.as_ref().map(|m| format!("tiles: Messages text \"{}\"", m.text.trim().replace('\n', " / "))),
        n.quest.as_ref().map(|q| match q {
            Quest::Banner { label, name, .. } => format!("tiles: QuestReminder banner {label}: {}", name.trim().to_uppercase()),
            Quest::Objectives { list, .. } => format!("tiles: QuestReminder {} objectives: {}", list.len(), list.iter().map(|o| o.0.as_str()).collect::<Vec<_>>().join(" | ")),
        }),
        match &n.quest {
            Some(Quest::Banner { label, name, discovered: true, .. }) => Some(format!("tiles: Region_Location banner {label}: {} (not drawn)", name.trim().to_uppercase())),
            _ => None,
        },
        (!n.subtitles.is_empty()).then(|| format!("tiles: Subtitles text \"{}\"", n.subtitles.join(" / "))),
    ]
}

#[cfg(test)]
mod tests {
    use super::super::draw::{pieces, Look, Select};
    use super::super::layout::Rect as TileRect;
    use super::super::vanilla::kept_pieces;
    use super::*;

    /// [design] A synthetic HUD with the four pieces' shapes and the templates' traits the
    /// copies take (hud_main_menu.xml, hudtemplates.xml), at the observed places T4 writes.
    fn hud() -> Ui {
        let src = MemSource::default().with(
            "hud.xml",
            r#"<menu name="HUDMainMenu">
              <rect name="QuestReminder"><x>45</x><y>200</y><width>400</width><height>200</height><locus>&true;</locus>
                <rect name="QuestStages"><y>40</y><width>100</width><height>50</height></rect>
                <template name="template_filled_checkbox"><image name="filled_checkbox"><visible>&false;</visible><alpha>0</alpha>
                  <filename>Interface\Shared\Marker\glow_square_filled_small.dds</filename><width>26</width><height>26</height><x>-20</x><y>3</y></image></template>
                <template name="template_checkbox"><image name="checkbox"><visible>&false;</visible><alpha>0</alpha>
                  <filename>Interface\Shared\Marker\glow_square_small.dds</filename><width>16</width><height>16</height><x>-20</x><y>3</y></image></template>
                <rect name="QuestAdded"><y>80</y><width>100</width><height>150</height></rect></rect>
              <rect name="Messages"><x>25</x><y>55</y><systemcolor>&hudmain;</systemcolor><width>460</width><height>90</height><locus>&true;</locus></rect>
              <rect name="Subtitles"><x>453</x><y>660</y><systemcolor>&hudmain;</systemcolor><width>800</width><height>110</height></rect>
              <rect name="Region_Location"><x>45</x><y>728</y><width>100</width><height>100</height></rect>
              <template name="template_justify_left_text"><text name="justify_left_text"><justify>&left;</justify><font>7</font><wrapwidth>2048</wrapwidth><alpha>0</alpha><visible>&false;</visible></text></template>
              <template name="template_justify_center_text"><text name="justify_center_text"><justify>&center;</justify><font>7</font></text></template>
              <template name="template_message_bracket"><image name="message_bracket"><filename>Interface\icons\MOOD\glow_messages_seperator_left.dds</filename>
                <zoom>&scale;</zoom><y>-20</y><width>470</width><height>192</height><alpha>0</alpha></image></template>
              <template name="template_message_icon"><image name="message_icon"><filename>Interface\Icons\Mood\glow_messages_happy.dds</filename>
                <x>-00</x><y>-25</y><depth>2</depth><zoom>&scale;</zoom><width>160</width><height>160</height><alpha>0</alpha></image></template>
            </menu>"#,
        );
        let mut ui = Ui::new();
        ui.load_menu(&src, "hud.xml").unwrap();
        ui.update();
        ui
    }

    /// Ten units a character, one line in 36 until the wrap width.
    fn lines(text: &str, wrap: Option<f64>) -> (usize, f64) {
        let w = 10.0 * text.chars().count() as f64;
        (wrap.map_or(1, |wrap| (w / wrap).ceil().max(1.0) as usize), 36.0)
    }

    fn plain(_: f64, text: &str, _: Option<f64>) -> (f64, f64) {
        (10.0 * text.chars().count() as f64, 36.0)
    }

    fn f(ui: &Ui, path: &str) -> f64 {
        ui.get_float(&format!("HUDMainMenu/{path}")).unwrap()
    }

    fn discovery(name: &str, strength: f64) -> Notices {
        Notices { quest: Some(Quest::Banner { label: "Place found".into(), name: name.into(), strength, discovered: true }), ..Default::default() }
    }

    fn message(text: &str, strength: f64) -> Notices {
        Notices { message: Some(Message { text: text.into(), icon: "interface\\icons\\message icons\\glow_message_vaultboy_neutral.dds".into(), strength }), ..Default::default() }
    }

    #[test]
    fn the_message_copies_are_made_in_the_observed_order_and_fade_with_the_queue() {
        let mut ui = hud();
        write(&mut ui, &message("You are now Well Rested!", 0.5), &lines);
        // *:0 the bracket, *:1 the text, *:2 the icon, as read in the original (hud.md §14.11).
        assert_eq!(ui.tile(ui.find_tile("HUDMainMenu/Messages/*:0").unwrap()).name, "message_bracket");
        assert_eq!(ui.tile(ui.find_tile("HUDMainMenu/Messages/*:1").unwrap()).name, "justify_left_text");
        assert_eq!(ui.tile(ui.find_tile("HUDMainMenu/Messages/*:2").unwrap()).name, "message_icon");
        assert_eq!((f(&ui, "Messages/*:1/x"), f(&ui, "Messages/*:1/y"), f(&ui, "Messages/*:1/depth")), (130.0, 5.0, 2.0));
        assert_eq!(f(&ui, "Messages/*:1/wrapwidth"), 330.0);
        assert_eq!(ui.get_string("HUDMainMenu/Messages/*:1/string").as_deref(), Some("You are now Well Rested!"));
        // Half way in: the text and icon at half alpha, the bracket fainter.
        assert_eq!((f(&ui, "Messages/*:1/alpha"), f(&ui, "Messages/*:2/alpha")), (127.5, 127.5));
        assert!((f(&ui, "Messages/*:0/alpha") - 127.5 * BRACKET_STRENGTH).abs() < 1e-9);
        assert_eq!(ui.get_string("HUDMainMenu/Messages/*:2/filename").as_deref(), Some("interface\\icons\\message icons\\glow_message_vaultboy_neutral.dds"));
        // Written again: no more copies; gone: alpha 0, the text kept (as the original's).
        write(&mut ui, &message("You are now Well Rested!", 1.0), &lines);
        write(&mut ui, &Notices::default(), &lines);
        assert_eq!(ui.tile(ui.find_tile("HUDMainMenu/Messages").unwrap()).children.len(), 3);
        assert_eq!((f(&ui, "Messages/*:0/alpha"), f(&ui, "Messages/*:1/alpha"), f(&ui, "Messages/*:2/alpha")), (0.0, 0.0, 0.0));
        assert_eq!(ui.get_string("HUDMainMenu/Messages/*:1/string").as_deref(), Some("You are now Well Rested!"));
        // The copies are the code's, not a mod's.
        let menu = ui.menu(HUD).unwrap();
        assert!(ui.mod_added_tiles(menu).is_empty());
    }

    #[test]
    fn the_renderer_draws_the_message_where_the_hand_built_hud_does() {
        let mut ui = hud();
        write(&mut ui, &message("Bomb disarmed.", 1.0), &lines);
        ui.update();
        let menu = ui.menu(HUD).unwrap();
        let (drawn, _) = pieces(&ui, menu, &plain, Select::Whole { kept: &kept_pieces(&ui), strip: None }, false);
        let by = |name: &str| drawn.iter().find(|p| ui.tile(p.tile).name == name).unwrap_or_else(|| panic!("{name} not drawn"));
        // The area at (25, 55): the text at 130 in (corner_messages::TEXT_X), the bracket
        // and icon at the templates' places, the icon and text in front.
        let text = by("justify_left_text");
        assert_eq!((text.rect.left, text.rect.top), (155.0, 60.0));
        assert_eq!(text.look, Look::Text { string: "Bomb disarmed.".into(), font: 7, wrap: Some(330.0), alignment: super::super::layout::Alignment::Left });
        assert_eq!(by("message_bracket").rect, TileRect { left: 25.0, top: 35.0, width: 470.0, height: 192.0 });
        assert_eq!(by("message_icon").rect, TileRect { left: 25.0, top: 30.0, width: 160.0, height: 160.0 });
        let order: Vec<_> = drawn.iter().map(|p| ui.tile(p.tile).name.clone()).collect();
        assert!(order.iter().position(|n| n == "message_bracket") < order.iter().position(|n| n == "message_icon"));
        // Nothing showing: nothing drawn.
        write(&mut ui, &Notices::default(), &lines);
        let (drawn, _) = pieces(&ui, menu, &plain, Select::Whole { kept: &kept_pieces(&ui), strip: None }, false);
        assert!(!drawn.iter().any(|p| ui.path_of(p.tile).starts_with("HUDMainMenu/Messages/")));
    }

    #[test]
    fn a_quest_banner_and_its_objectives_take_the_hand_built_places() {
        let mut ui = hud();
        // Nothing before the first notice: QuestReminder's *:0 stays its file's QuestAdded.
        write(&mut ui, &Notices::default(), &lines);
        assert_eq!(ui.tile(ui.find_tile("HUDMainMenu/QuestReminder/*:0").unwrap()).name, "QuestAdded");
        let banner = Quest::Banner { label: "New quest".into(), name: "Back in the Saddle".into(), strength: 1.0, discovered: false };
        write(&mut ui, &Notices { quest: Some(banner), ..Default::default() }, &lines);
        let label = copies(&ui, "QuestReminder", "label")[0];
        let title = copies(&ui, "QuestReminder", "title")[0];
        // "New quest" on 247, the name in font 8 on 311, from x 45 (quest-messages.md §4):
        // the label's top 24 over its line (font 7), the name's 30 (font 8).
        fn at(ui: &Ui, t: TileId) -> (f64, f64) {
            (ui.tile(t).get("x").unwrap().num(), ui.tile(t).get("y").unwrap().num())
        }
        assert_eq!((at(&ui, label), at(&ui, title)), ((0.0, 23.0), (-1.5, 81.0)));
        assert_eq!(ui.tile(title).get("font").unwrap().num(), 8.0);
        assert_eq!(ui.tile(title).get("string").unwrap().text(), "BACK IN THE SADDLE");
        assert_eq!(ui.tile(label).get("alpha").unwrap().num(), 255.0);
        // Then the objectives: the banner goes, each objective under the lines before it,
        // with its box; a completed one's box filled.
        let list = vec![("COMPLETED: Talk to Sunny Smiles in the Prospector Saloon and meet her".to_string(), true), ("Meet Sunny.".to_string(), false)];
        write(&mut ui, &Notices { quest: Some(Quest::Objectives { list, strength: 0.5 }), ..Default::default() }, &lines);
        assert_eq!((ui.tile(label).get("alpha").unwrap().num(), ui.tile(title).get("alpha").unwrap().num()), (0.0, 0.0));
        let texts = copies(&ui, "QuestReminder", "objective");
        assert_eq!(texts.len(), 2);
        // The first is 69 characters: two lines at 570, 38 a line.
        assert_eq!((at(&ui, texts[0]), at(&ui, texts[1])), ((0.0, 39.0), (0.0, 39.0 + 2.0 * 38.0)));
        assert_eq!(ui.tile(texts[0]).get("wrapwidth").unwrap().num(), 570.0);
        assert_eq!(ui.tile(texts[1]).get("alpha").unwrap().num(), 127.5);
        let (done, open) = (copies(&ui, "QuestReminder", "done_box"), copies(&ui, "QuestReminder", "open_box"));
        assert_eq!((done.len(), open.len()), (1, 1));
        let rect = |t: TileId| ["x", "y", "width", "height"].map(|n| ui.tile(t).get(n).unwrap().num());
        assert_eq!(rect(done[0]), [-16.0, 42.0, 26.0, 26.0]);
        assert_eq!(rect(open[0]), [-16.0, 42.0 + 76.0, 26.0, 26.0]);
        assert_eq!(ui.tile(open[0]).get("filename").unwrap().text(), "Interface\\Shared\\Marker\\glow_square_small.dds");
        // One objective later: the second text and its box hidden, not removed.
        write(&mut ui, &Notices { quest: Some(Quest::Objectives { list: vec![("Meet Sunny.".into(), false)], strength: 1.0 }), ..Default::default() }, &lines);
        assert_eq!(copies(&ui, "QuestReminder", "objective").len(), 2);
        assert_eq!((ui.tile(texts[0]).get("string").unwrap().text(), ui.tile(texts[1]).get("visible").unwrap().num()), ("Meet Sunny.".into(), 0.0));
        assert_eq!((ui.tile(open[0]).get("visible").unwrap().num(), ui.tile(done[0]).get("visible").unwrap().num()), (1.0, 0.0));
        // Drawn: the objective at QuestReminder's place plus its own, its box before it.
        ui.update();
        let menu = ui.menu(HUD).unwrap();
        let (drawn, _) = pieces(&ui, menu, &plain, Select::Whole { kept: &kept_pieces(&ui), strip: None }, false);
        let text = drawn.iter().find(|p| p.tile == texts[0]).unwrap();
        assert_eq!((text.rect.left, text.rect.top), (45.0, 239.0));
        let square = drawn.iter().find(|p| p.tile == open[0]).unwrap();
        assert_eq!(square.rect, TileRect { left: 29.0, top: 242.0, width: 26.0, height: 26.0 });
        assert!(!drawn.iter().any(|p| p.tile == texts[1] || p.tile == done[0]));
        // Gone: everything at alpha 0 or hidden, nothing drawn.
        write(&mut ui, &Notices::default(), &lines);
        let (drawn, _) = pieces(&ui, menu, &plain, Select::Whole { kept: &kept_pieces(&ui), strip: None }, false);
        assert!(!drawn.iter().any(|p| ui.path_of(p.tile).starts_with("HUDMainMenu/QuestReminder/")));
    }

    #[test]
    fn a_discovery_is_written_under_region_location_too_and_drawn_once() {
        let mut ui = hud();
        // Another quest notice first: nothing under Region_Location.
        let added = Quest::Banner { label: "New quest".into(), name: "Back in the Saddle".into(), strength: 1.0, discovered: false };
        write(&mut ui, &Notices { quest: Some(added.clone()), ..Default::default() }, &lines);
        assert!(copies(&ui, "Region_Location", "label").is_empty());
        write(&mut ui, &discovery("Goodsprings", 0.5), &lines);
        let label = copies(&ui, "Region_Location", "label")[0];
        let title = copies(&ui, "Region_Location", "title")[0];
        fn traits(ui: &Ui, t: TileId) -> [f64; 4] {
            ["x", "y", "alpha", "visible"].map(|n| ui.tile(t).get(n).unwrap().num())
        }
        // The quest banner's places from Region_Location's (45, 728): the label's top on
        // 223, the name's on 281, as QuestReminder's (45, 200) + (0, 23) and (-1.5, 81).
        assert_eq!(traits(&ui, label), [0.0, -505.0, 127.5, 0.0]);
        assert_eq!(traits(&ui, title), [-1.5, -447.0, 127.5, 0.0]);
        assert_eq!(ui.tile(label).get("string").unwrap().text(), "Place found");
        assert_eq!((ui.tile(title).get("string").unwrap().text(), ui.tile(title).get("font").unwrap().num()), ("GOODSPRINGS".into(), 8.0));
        // QuestReminder's banner holds the same and is the one shown.
        let shown = copies(&ui, "QuestReminder", "label")[0];
        assert_eq!((ui.tile(shown).get("string").unwrap().text(), ui.tile(shown).get("visible").unwrap().num()), ("Place found".into(), 1.0));
        // Drawn once: at the hand-built HUD's place, from QuestReminder; nothing from
        // Region_Location.
        ui.update();
        let menu = ui.menu(HUD).unwrap();
        let (drawn, _) = pieces(&ui, menu, &plain, Select::Whole { kept: &kept_pieces(&ui), strip: None }, false);
        let said: Vec<_> = drawn.iter().filter(|p| matches!(&p.look, Look::Text { string, .. } if string == "Place found")).collect();
        assert_eq!(said.len(), 1);
        assert_eq!((said[0].tile, said[0].rect.left, said[0].rect.top), (shown, 45.0, 223.0));
        assert!(!drawn.iter().any(|p| ui.path_of(p.tile).starts_with("HUDMainMenu/Region_Location/")));
        // Another notice, then none: alpha 0, the discovery held, no more copies.
        write(&mut ui, &Notices { quest: Some(added), ..Default::default() }, &lines);
        assert_eq!((traits(&ui, label)[2], traits(&ui, title)[2]), (0.0, 0.0));
        assert_eq!(ui.tile(title).get("string").unwrap().text(), "GOODSPRINGS");
        write(&mut ui, &Notices::default(), &lines);
        assert_eq!(traits(&ui, label)[2], 0.0);
        assert_eq!(ui.tile(ui.find_tile("HUDMainMenu/Region_Location").unwrap()).children.len(), 2);
        let menu = ui.menu(HUD).unwrap();
        assert!(ui.mod_added_tiles(menu).is_empty());
    }

    #[test]
    fn subtitles_are_one_centred_text_there_from_the_start() {
        let mut ui = hud();
        write(&mut ui, &Notices::default(), &lines);
        // As read in the original before any line: " ", hidden, alpha 255, at x 400.
        assert_eq!(ui.get_string("HUDMainMenu/Subtitles/*:0/string").as_deref(), Some(" "));
        assert_eq!((f(&ui, "Subtitles/*:0/x"), f(&ui, "Subtitles/*:0/visible"), f(&ui, "Subtitles/*:0/alpha")), (400.0, 0.0, 255.0));
        let lines_said = Notices { subtitles: vec!["What in tarnation?".into(), " Hi. ".into()], ..Default::default() };
        write(&mut ui, &lines_said, &lines);
        assert_eq!(ui.get_string("HUDMainMenu/Subtitles/*:0/string").as_deref(), Some("What in tarnation?\nHi."));
        assert_eq!((f(&ui, "Subtitles/*:0/visible"), f(&ui, "Subtitles/*:0/wrapwidth")), (1.0, 800.0));
        ui.update();
        let menu = ui.menu(HUD).unwrap();
        let (drawn, _) = pieces(&ui, menu, &plain, Select::Whole { kept: &kept_pieces(&ui), strip: None }, false);
        let text = drawn.iter().find(|p| ui.path_of(p.tile).starts_with("HUDMainMenu/Subtitles/")).unwrap();
        // Centred on the area's middle (453 + 400), its top the area's.
        assert_eq!((text.rect.left + text.rect.width / 2.0, text.rect.top), (853.0, 660.0));
        // After the line: hidden, the text kept.
        write(&mut ui, &Notices::default(), &lines);
        assert_eq!(f(&ui, "Subtitles/*:0/visible"), 0.0);
        assert_eq!(ui.get_string("HUDMainMenu/Subtitles/*:0/string").as_deref(), Some("What in tarnation?\nHi."));
        assert_eq!(ui.tile(ui.find_tile("HUDMainMenu/Subtitles").unwrap()).children.len(), 1);
    }

    #[test]
    fn a_rebuilt_tree_gets_its_copies_again_and_a_tree_without_the_pieces_none() {
        let mut ui = hud();
        write(&mut ui, &message("a", 1.0), &lines);
        // A new tree (a load): made again.
        let mut ui = hud();
        assert!(copies(&ui, "Messages", "text").is_empty());
        write(&mut ui, &message("b", 1.0), &lines);
        assert_eq!(copies(&ui, "Messages", "text").len(), 1);
        // A message makes nothing under Region_Location; a discovery makes its label and
        // name there, once however often it is written.
        let region = ui.find_tile("HUDMainMenu/Region_Location").unwrap();
        assert!(ui.tile(region).children.is_empty());
        write(&mut ui, &discovery("Goodsprings", 1.0), &lines);
        write(&mut ui, &discovery("Goodsprings", 1.0), &lines);
        assert_eq!((copies(&ui, "Region_Location", "label").len(), copies(&ui, "Region_Location", "title").len()), (1, 1));
        assert_eq!(ui.tile(region).children.len(), 2);
        // No HUD: nothing, quietly.
        let mut none = Ui::new();
        write(&mut none, &message("c", 1.0), &lines);
        assert!(none.find_tile("HUDMainMenu").is_none());
    }

    /// The review of #952: the copies' widths were never written, unlike the HUD's other
    /// text tiles'. Each text copy gets one from its string, its font and its wrap width.
    #[test]
    fn every_text_copy_gets_its_width_from_its_string_font_and_wrap() {
        let mut ui = hud();
        let list = vec![("x".repeat(69), false), ("Meet Sunny.".to_string(), false)];
        write(&mut ui, &message("Bomb disarmed.", 1.0), &lines);
        write(&mut ui, &Notices { quest: Some(Quest::Objectives { list, strength: 1.0 }), subtitles: vec!["Hi.".into()], ..message("Bomb disarmed.", 1.0) }, &lines);
        // The measure echoes what it was given: ten units a character, held to the wrap,
        // plus the font as a hundred.
        let measure = |font: f64, text: &str, wrap: Option<f64>| (10.0 * text.chars().count() as f64).min(wrap.unwrap_or(f64::MAX)) + 100.0 * font;
        write_widths(&mut ui, &mut WidthsWritten::new(), &measure);
        let width = |t: TileId| ui.tile(t).get("width").map(Value::num);
        assert_eq!(width(copies(&ui, "Messages", "text")[0]), Some(140.0 + 700.0));
        let objectives = copies(&ui, "QuestReminder", "objective");
        assert_eq!((width(objectives[0]), width(objectives[1])), (Some(570.0 + 700.0), Some(110.0 + 700.0)));
        // The subtitle in its template's font, wrapped at 800.
        assert_eq!(width(copies(&ui, "Subtitles", "text")[0]), Some(30.0 + 700.0));
        // The message, the banner's label and name (made with the objectives, no string yet,
        // so no width), the two objectives and the subtitle.
        assert_eq!(texts(&ui).len(), 1 + 2 + 2 + 1);
        assert!(copies(&ui, "QuestReminder", "label").iter().all(|&t| width(t).is_none()));
        // Pictures are not texts: the boxes, the icon and the bracket get no width written.
        assert!(copies(&ui, "QuestReminder", "open_box").iter().all(|&b| ui.tile(b).get("width").map(Value::num) == Some(26.0)));
    }

    /// #1062: a copy is measured again only when its string, font or wrap width changed,
    /// or its width was written over; unchanged copies keep theirs without a layout.
    #[test]
    fn widths_are_measured_only_when_a_text_changed() {
        use std::cell::Cell;
        let mut ui = hud();
        write(&mut ui, &Notices { subtitles: vec!["Hi.".into()], ..message("Bomb disarmed.", 1.0) }, &lines);
        let calls = Cell::new(0);
        let measure = |_: f64, text: &str, _: Option<f64>| {
            calls.set(calls.get() + 1);
            10.0 * text.chars().count() as f64
        };
        let mut last = WidthsWritten::new();
        write_widths(&mut ui, &mut last, &measure);
        assert_eq!(calls.get(), 2);
        // Nothing changed: no layout.
        write(&mut ui, &Notices { subtitles: vec!["Hi.".into()], ..message("Bomb disarmed.", 1.0) }, &lines);
        write_widths(&mut ui, &mut last, &measure);
        assert_eq!(calls.get(), 2);
        // A new subtitle: only it is measured.
        write(&mut ui, &Notices { subtitles: vec!["Hello.".into()], ..message("Bomb disarmed.", 1.0) }, &lines);
        write_widths(&mut ui, &mut last, &measure);
        assert_eq!(calls.get(), 3);
        let subtitle = copies(&ui, "Subtitles", "text")[0];
        assert_eq!(ui.tile(subtitle).get("width").map(Value::num), Some(60.0));
        // A width written over (a script, a reloaded tree): measured again.
        ui.set_trait(subtitle, "width", Value::Num(1.0));
        write_widths(&mut ui, &mut last, &measure);
        assert_eq!((calls.get(), ui.tile(subtitle).get("width").map(Value::num)), (4, Some(60.0)));
        // A changed font too.
        let text = copies(&ui, "Messages", "text")[0];
        ui.set_trait(text, "font", Value::Num(3.0));
        write_widths(&mut ui, &mut last, &measure);
        assert_eq!(calls.get(), 5);
    }

    #[test]
    fn the_log_names_what_each_area_shows() {
        let n = Notices {
            message: Some(Message { text: "Two\nlines".into(), icon: String::new(), strength: 1.0 }),
            quest: Some(Quest::Banner { label: "Place found".into(), name: "Goodsprings".into(), strength: 1.0, discovered: true }),
            subtitles: vec!["Hi.".into()],
        };
        assert_eq!(
            describe(&n),
            [
                Some("tiles: Messages text \"Two / lines\"".to_string()),
                Some("tiles: QuestReminder banner Place found: GOODSPRINGS".to_string()),
                Some("tiles: Region_Location banner Place found: GOODSPRINGS (not drawn)".to_string()),
                Some("tiles: Subtitles text \"Hi.\"".to_string()),
            ]
        );
        // Another banner: QuestReminder's line only.
        let added = Notices { quest: Some(Quest::Banner { label: "New quest".into(), name: "Back in the Saddle".into(), strength: 1.0, discovered: false }), ..Default::default() };
        assert_eq!(describe(&added)[2], None);
        assert_eq!(describe(&Notices::default()), [None, None, None, None]);
    }
}
