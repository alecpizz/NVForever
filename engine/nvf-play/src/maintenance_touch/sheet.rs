use super::*;

pub(super) struct Geometry {
    panel: Rect,
    pub list: Rect,
    cancel: Rect,
    confirm: Rect,
    shown: usize,
}
impl Geometry {
    #[cfg(test)]
    pub(super) fn center(&self, source: &Source, action: Action) -> Vec2 {
        match action {
            Action::Confirm => self.confirm.center(), Action::Cancel => self.cancel.center(),
            Action::Select(i) => Vec2::new(self.list.center().x, self.list.min.y + (i - source.first()) as f32 * ROW + ROW / 2.0),
            Action::Scroll(_) => self.list.center(),
        }
    }
    pub fn new(c: Context) -> Self {
        let safe = Rect::new(c.safe.left + 8.0, c.safe.top + 8.0, c.logical.x - c.safe.right - 8.0, c.logical.y - c.safe.bottom - 8.0);
        let width = safe.width().min(720.0);
        let x = safe.center().x - width / 2.0;
        let panel = Rect::new(x, safe.min.y, x + width, safe.max.y);
        let shown = ((panel.height() - 160.0) / ROW).floor().max(1.0) as usize;
        let list = Rect::new(x + 8.0, panel.min.y + 100.0, x + width - 8.0, panel.min.y + 100.0 + shown as f32 * ROW);
        let y = panel.max.y - 56.0;
        let middle = panel.center().x;
        Self { panel, list, cancel: Rect::new(x + 8.0, y, middle - 4.0, y + ROW), confirm: Rect::new(middle + 4.0, y, panel.max.x - 8.0, y + ROW), shown }
    }
    pub fn target(&self, at: Vec2, source: &Source) -> Option<Action> {
        if self.cancel.contains(at) { return Some(Action::Cancel); }
        if self.confirm.contains(at) && confirmation(source).1 { return Some(Action::Confirm); }
        if self.list.contains(at) {
            let index = source.first() + ((at.y - self.list.min.y) / ROW) as usize;
            if index < source.rows() { return Some(Action::Select(index)); }
        }
        None
    }
}

fn confirmation(source: &Source) -> (&'static str, bool) {
    match source {
        Source::Repair(s) if s.preview => ("Preview only", false),
        Source::Repair(s) => (if s.selected.checked_sub(1).and_then(|i|s.offer.parts.get(i)).is_some_and(|p|p.kit) { "Repair: use kit" } else { "Repair: consume donor" }, s.selected > 0 && s.offer.parts.get(s.selected - 1).is_some() && s.offer.usable()),
        Source::Mod(s) => match s.selected.checked_sub(1).and_then(|i| s.offer.choices.get(i)) {
            Some(c) => (if c.attached { "Remove mod" } else { "Install mod" }, c.attached || c.count > 0),
            None => ("Select a mod", false),
        },
        Source::Vendor(_, v) => ("Confirm repair", v.rows.get(v.selected).is_some_and(|r| r.ready)),
    }
}
fn heading(source: &Source) -> (String, String) {
    match source {
        Source::Repair(s) => {
            let after = s.selected.checked_sub(1).and_then(|i| s.offer.parts.get(i)).map(|p| s.after(p));
            let change = after.map_or(String::new(), |to| format!(" → {:.0}%", to * 100.0));
            let requirement = if s.preview { "Preview only. No items will be consumed." } else if s.offer.parts.is_empty() { "No compatible donors available." } else if s.offer.parts.iter().any(|p|p.kit) { "Select a donor or kit, then confirm. The selected part is consumed." } else { "Select a donor, then confirm. The donor is consumed." };
            (format!("Repair: {}", s.offer.name), format!("Condition {:.0}%{change} · Repair {:.0}\n{requirement}", s.offer.condition * 100.0, s.skill))
        }
        Source::Mod(s) => {
            let effect = s.selected.checked_sub(1).and_then(|i| s.offer.choices.get(i)).and_then(|c| c.effect.as_deref()).unwrap_or(if s.offer.choices.is_empty() { "No carried or installed compatible modifications." } else { "Select a supported modification, then confirm." });
            (format!("Weapon mods: {}", s.offer.name), effect.to_owned())
        }
        Source::Vendor(_, v) => {
            let detail = v.rows.get(v.selected).map_or("No repairable items".to_owned(), |r| match r.quote {
                fnv_game::repair::Quote::Repair { to, cost } => format!("Condition {:.0}% → {:.0}% · {cost} caps{}", r.condition * 100.0, to * 100.0, if r.ready { "" } else { " · Insufficient caps" }),
                fnv_game::repair::Quote::Beyond => "Beyond this vendor's Repair skill".to_owned(),
            });
            ("Repair services".to_owned(), format!("Repair {} · Carried {} caps\n{detail}", v.skill, v.caps))
        }
    }
}
fn row(source: &Source, index: usize) -> (String, bool) {
    match source {
        Source::Repair(s) => if index == 0 { (format!("{} · {:.0}% (item)", s.offer.name, s.offer.condition * 100.0), s.selected == 0) }
            else { let p = &s.offer.parts[index - 1]; (if p.kit { format!("{} · Repair kit", p.name) } else { format!("{} · {:.0}% donor", p.name, p.condition * 100.0) }, s.selected == index) },
        Source::Mod(s) => if index == 0 { (format!("{} (weapon)", s.offer.name), s.selected == 0) }
            else { let c = &s.offer.choices[index - 1]; (format!("{} · {}", c.name, if c.attached { "Installed".to_owned() } else { format!("{} carried", c.count) }), s.selected == index) },
        Source::Vendor(_, v) => { let r = &v.rows[index]; (format!("{} · {:.0}% · {}", r.label, r.condition * 100.0, r.quote.cost().map_or("Skill required".to_owned(), |n| format!("{n} caps"))), v.selected == index) },
    }
}

#[derive(Component)]
pub(super) struct Sheet;
#[cfg(test)]
#[derive(Component)]
pub(super) struct SheetRow(pub usize);
#[cfg(test)]
#[derive(Component)]
pub(super) struct SheetButton(pub Action);
#[derive(Resource, Default)]
pub(super) struct Drawn(Option<(u64, bool)>);

fn node(rect: Rect, scale: f32) -> Node {
    Node { position_type: PositionType::Absolute, left: px(rect.min.x / scale), top: px(rect.min.y / scale), width: px(rect.width() / scale), height: px(rect.height() / scale), overflow: Overflow::clip(), ..default() }
}
fn label(parent: &mut ChildSpawnerCommands, rect: Rect, text: String, size: f32, scale: f32, color: Color) {
    parent.spawn((node(rect, scale), Text::new(text), TextFont { font_size: FontSize::Px(size / scale), ..default() }, TextColor(color), Pickable::IGNORE));
}

pub(super) fn draw(mut commands: Commands, mut state: ResMut<State>, mut drawn: ResMut<Drawn>, old: Query<Entity, With<Sheet>>) {
    let key = (state.generation, state.visible());
    if drawn.0 == Some(key) { return; }
    drawn.0 = Some(key);
    for entity in &old { commands.entity(entity).despawn(); }
    if !state.visible() { return; }
    let (Some(context), Some(source)) = (state.context, state.source.as_deref()) else { return };
    let g = Geometry::new(context);
    let scale = context.ui_scale;
    let bright = Color::srgb(1.0, 0.84, 0.40);
    let dim = Color::srgb(0.62, 0.56, 0.40);
    let (title, detail) = heading(source);
    commands.spawn((Sheet, UiTargetCamera(context.camera), node(Rect::from_corners(Vec2::ZERO, context.logical), scale), GlobalZIndex(900), BackgroundColor(Color::srgba(0.015, 0.02, 0.015, 0.96)), Pickable::IGNORE)).with_children(|p| {
        label(p, Rect::new(g.panel.min.x + 12.0, g.panel.min.y + 4.0, g.panel.max.x - 12.0, g.panel.min.y + 32.0), title, 22.0, scale, bright);
        label(p, Rect::new(g.panel.min.x + 12.0, g.panel.min.y + 34.0, g.panel.max.x - 12.0, g.list.min.y - 2.0), detail, 16.0, scale, bright);
        for (offset, index) in (source.first()..source.rows()).take(g.shown).enumerate() {
            let y = g.list.min.y + offset as f32 * ROW;
            let rect = Rect::new(g.list.min.x, y, g.list.max.x, y + ROW);
            let (text, selected) = row(source, index);
            let _row = p.spawn((node(rect, scale), BackgroundColor(if selected { Color::srgb(0.20, 0.18, 0.10) } else { Color::srgb(0.065, 0.075, 0.055) }), Pickable::IGNORE)).id();
            #[cfg(test)]
            p.commands().entity(_row).insert(SheetRow(index));
            label(p, Rect::new(rect.min.x + 8.0, rect.min.y + 10.0, rect.max.x - 8.0, rect.max.y), text, 18.0, scale, bright);
        }
        let (confirm, enabled) = confirmation(source);
        for (r, text, color, _action) in [(g.cancel, "Cancel / Back", bright, Action::Cancel), (g.confirm, confirm, if enabled { bright } else { dim }, Action::Confirm)] {
            let _button = p.spawn((node(r, scale), BackgroundColor(Color::srgb(0.12, 0.13, 0.085)), Pickable::IGNORE)).id();
            #[cfg(test)]
            p.commands().entity(_button).insert(SheetButton(_action));
            label(p, Rect::new(r.min.x + 8.0, r.min.y + 12.0, r.max.x - 8.0, r.max.y), text.to_owned(), 17.0, scale, color);
        }
    });
    state.painted = Some(state.generation);
}
