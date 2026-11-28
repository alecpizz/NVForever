//! Logical window pixels are the single authority for both painted and hit rectangles.
use super::*;

const ROW: f32 = 48.0;
// Two 15px text lines at Bevy's default 1.2 line height plus 10px padding.
const HEADER: f32 = 48.0;
#[derive(Clone, Copy)]
pub(super) struct Panel {
    pub area: Rect,
    pub rows: usize,
    lists: [Rect; 2],
    categories: [Rect; 2],
    info: Rect,
    footer: Rect,
    quantity: Rect,
    pub meter: Option<Rect>,
}
fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect::from_corners(Vec2::new(x, y), Vec2::new(x + width, y + height))
}
impl Panel {
    pub fn of(f: Frame) -> Option<Self> {
        if !f.size.is_finite() || !f.ui_scale.is_finite() || f.ui_scale <= 0.0
            || !f.density.is_finite() || f.density <= 0.0 || !(f.size / f.ui_scale).is_finite()
            || !(48.0 / f.ui_scale).is_finite() { return None; }
        let s = f.safe;
        crate::touch_safe_area::SafeInsets::from_array([s.left, s.top, s.right, s.bottom])?;
        let span = f.size - Vec2::new(s.left + s.right + 16.0, s.top + s.bottom + 16.0);
        // Rect::from_corners sorts inverted corners, so reject signed available
        // space before construction instead of turning invalid insets into UI.
        if span.x < 480.0 || span.y < 240.0 { return None; }
        let area = rect(s.left + 8.0, s.top + 8.0, span.x, span.y);
        let rows = (((area.height() - HEADER - ROW * 3.0) / ROW).floor() as usize).clamp(1, 8);
        let width = (area.width() - 8.0) * 0.5;
        let left = area.min.x;
        let right = left + width + 8.0;
        let top = area.min.y + HEADER;
        let lists = [rect(right, top + ROW, width, rows as f32 * ROW),
            rect(left, top + ROW, width, rows as f32 * ROW)];
        let categories = [rect(right, top, width, ROW), rect(left, top, width, ROW)];
        let footer = rect(left, area.max.y - ROW, area.width(), ROW);
        let info = rect(left, footer.min.y - ROW, area.width(), ROW);
        let qwidth = area.width().min(560.0);
        let quantity = rect(area.center().x - qwidth * 0.5, area.center().y - 120.0, qwidth, 240.0);
        let meter = rect(quantity.min.x + ROW * 2.0, quantity.min.y + ROW,
            qwidth - ROW * 4.0, ROW);
        Some(Self { area, rows, lists, categories, info, footer, quantity, meter: Some(meter) })
    }
    pub fn hits(self, m: &MenuModel, first: [usize; 2]) -> Vec<Hit> {
        let mut hits = Vec::new();
        if m.quantity.is_some() {
            let q = self.quantity;
            let y = q.min.y + ROW;
            for (x, target) in [(q.min.x, Target::Min), (q.min.x + ROW, Target::Step(-1)),
                (q.max.x - ROW * 2.0, Target::Step(1)), (q.max.x - ROW, Target::Max)] {
                hits.push(Hit { rect: rect(x, y, ROW, ROW), target });
            }
            hits.push(Hit { rect: self.meter.unwrap(), target: Target::Meter });
            for (i, target) in [Target::ConfirmAmount, Target::CancelAmount].into_iter().enumerate() {
                hits.push(Hit { rect: rect(q.min.x + i as f32 * q.width() * 0.5,
                    q.max.y - ROW, q.width() * 0.5, ROW), target });
            }
            return hits;
        }
        for side in [Side::Player, Side::Container] {
            let i = side as usize;
            let c = self.categories[i];
            hits.push(Hit { rect: rect(c.min.x, c.min.y, ROW, ROW), target: Target::Category(side, -1) });
            hits.push(Hit { rect: rect(c.max.x - ROW, c.min.y, ROW, ROW), target: Target::Category(side, 1) });
            for n in first[i]..(first[i] + self.rows).min(m.list(side).rows.len()) {
                if let Some(token) = RowToken::at(m, side, n) {
                    hits.push(Hit { rect: rect(self.lists[i].min.x, self.lists[i].min.y + (n - first[i]) as f32 * ROW,
                        self.lists[i].width(), ROW), target: Target::Row(token) });
                }
            }
        }
        for (i, target) in [Target::One, Target::All, Target::Accept, Target::Close].into_iter().enumerate() {
            hits.push(Hit { rect: rect(self.footer.min.x + i as f32 * self.footer.width() / 4.0,
                self.footer.min.y, self.footer.width() / 4.0, ROW), target });
        }
        hits
    }
}

#[derive(Component)]
pub(super) struct Sheet;
fn bounded(text: &str, max: usize) -> String { text.chars().take(max).collect() }
fn label(m: &MenuModel, target: Target) -> String {
    match target {
        Target::Row(t) => {
            let r = &m.list(t.side).rows[t.index];
            format!("{}{}  ×{}   {} caps", if r.bartered { "Basket: " } else { "" },
                bounded(&r.name, 72), r.count, r.price.map_or_else(|| "?".into(), super::super::barter::price_text))
        }
        Target::Category(_, by) => if by < 0 { "‹" } else { "›" }.into(),
        Target::One => "Choose amount".into(), Target::All => "Whole stack".into(),
        Target::Accept => "Accept trade".into(), Target::Close => "Cancel / leave".into(),
        Target::Step(n) => if n < 0 { "−" } else { "+" }.into(),
        Target::Min => "1".into(), Target::Max => "Max".into(), Target::Meter => {
            let q = m.quantity.unwrap(); format!("{} / {}", q.amount, q.max)
        }
        Target::ConfirmAmount => "Confirm amount".into(), Target::CancelAmount => "Cancel amount".into(),
    }
}
fn paint(parent: &mut ChildSpawnerCommands, r: Rect, scale: f32, text: String, selected: bool) {
    parent.spawn((Node {
        position_type: PositionType::Absolute, left: px(r.min.x / scale), top: px(r.min.y / scale),
        width: px(r.width() / scale), height: px(r.height() / scale),
        padding: UiRect::all(px(5.0 / scale)), overflow: Overflow::clip(),
        align_items: AlignItems::Center, ..default()
    }, BackgroundColor(if selected { Color::srgb(0.14, 0.32, 0.24) } else { Color::srgb(0.035, 0.075, 0.06) })))
        .with_children(|p| { p.spawn((Text::new(text), TextFont { font_size: FontSize::Px(15.0 / scale), ..default() },
            TextColor(Color::srgb(0.8, 1.0, 0.87)), TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter))); });
}
pub(super) fn draw(mut commands: Commands, state: Res<TouchBarter>, active: Option<Res<ActiveMenu>>,
    roots: Query<Entity, With<Sheet>>, mut last: Local<Option<u64>>) {
    if *last == Some(state.revision) && !active.as_ref().is_some_and(|a| a.is_changed()) { return; }
    *last = Some(state.revision);
    for root in &roots { commands.entity(root).despawn(); }
    let Some((f, m, p)) = state.frame.filter(|_| state.visible())
        .and_then(|f| Some((f, active.as_ref()?.0.as_ref()?, Panel::of(f)?))) else { return; };
    let Some(t) = m.trade.as_ref().filter(|_| session(m) == state.identity.map(|i| i.session)) else { return; };
    // Explicit target camera supplied by the parent: never change the implicit UI camera.
    commands.spawn((Sheet, UiTargetCamera(f.camera), GlobalZIndex(3000),
        Node { position_type: PositionType::Absolute, width: percent(100.0), height: percent(100.0),
            overflow: Overflow::clip(), ..default() }, BackgroundColor(Color::srgb(0.015, 0.03, 0.025))))
        .with_children(|parent| {
            let balance = if t.balance < 0 { format!("Pay {}", t.balance.saturating_neg()) }
                else { format!("Receive {}", if m.full_barter_total { t.balance } else { t.paid() }) };
            // Reuse the ordinary menu's current/after-caps presentation. This
            // changes only the figure shown; actual acceptance still uses paid().
            let caps = if m.barter_caps_change {
                format!("Your {} · Merchant {}",
                    super::super::look::half_figure(m, Side::Player).unwrap_or_default(),
                    super::super::look::half_figure(m, Side::Container).unwrap_or_default())
            } else { format!("Your caps {} · Merchant caps {}", t.purse[Side::Player as usize], t.purse[Side::Container as usize]) };
            let warning = if t.balance < 0 && !t.affordable() { " — insufficient caps" }
                else if t.paid() != t.balance { " — merchant caps limit" } else { "" };
            paint(parent, rect(p.area.min.x, p.area.min.y, p.area.width(), HEADER), f.ui_scale,
                format!("{caps} · {balance}{warning} · Basket buy {} / sell {}",
                    t.offered[0].values().map(|n| i64::from(*n)).sum::<i64>(),
                    t.offered[1].values().map(|n| i64::from(*n)).sum::<i64>()), false);
            if let Some(q) = m.quantity {
                let name = &m.list(q.side).rows[q.row].name;
                paint(parent, rect(p.quantity.min.x, p.quantity.min.y, p.quantity.width(), ROW), f.ui_scale,
                    format!("How many? {}", bounded(name, 80)), true);
                paint(parent, rect(p.quantity.min.x, p.quantity.min.y + ROW * 2.0, p.quantity.width(), ROW), f.ui_scale,
                    "Drag the amount bar or use − / +; confirmation only stages the basket.".into(), false);
            } else {
                for side in [Side::Player, Side::Container] {
                    let c = p.categories[side as usize];
                    paint(parent, rect(c.min.x + ROW, c.min.y, c.width() - ROW * 2.0, ROW), f.ui_scale,
                        format!("{} · {}", if side == Side::Player { "Yours" } else { "Merchant" }, m.category[side as usize].title()), false);
                }
                let status = m.status.as_deref().map(|s| bounded(s, 160)).unwrap_or_else(||
                    "Tap a row to select. Drag either list to browse. Choose amount adds to the basket; Accept trades.".into());
                paint(parent, p.info, f.ui_scale, status, false);
            }
            for hit in p.hits(m, state.first) {
                let selected = match hit.target { Target::Row(t) => m.focus == t.side && m.selected[t.side as usize] == t.index, _ => false };
                paint(parent, hit.rect, f.ui_scale, label(m, hit.target), selected);
            }
        });
}
