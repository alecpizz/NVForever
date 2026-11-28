//! Original touch presentation of the existing gameplay hotkeys.
use bevy::{
    ecs::system::SystemParam,
    input::touch::{TouchInput, TouchPhase},
    prelude::*,
};
use nvf_play::{
    bindings::ControlInput,
    hotkeys::{Hotkeys, AMMO_SWAP, SLOTS},
    menu::{model::Mode, ActiveMenu},
    pipboy::{touch_items::{TouchItems, OverlayCapture}, Frame, Page, Phase, PipBoy},
    settings::{self, ChoiceList, Setting, Slider},
};
use std::collections::HashSet;
#[path = "hotbar_gesture.rs"]
mod gesture;
use gesture::{Box2, Gesture, Outcome, Target};
#[path = "hotbar_layout.rs"]
mod packing;

#[derive(Resource, Clone, PartialEq)]
pub(super) struct Preferences {
    enabled: bool,
    count: usize,
    scale: f32,
    tray: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            enabled: false,
            count: 4,
            scale: 1.,
            tray: true,
        }
    }
}
impl Preferences {
    pub(super) fn tray_available(&self) -> bool { self.enabled && self.tray }
}
#[derive(Clone, Debug)]
struct Placement {
    center: Vec2,
    pins: [bool; SLOTS],
    extra:serde_json::Value,
}
impl PartialEq for Placement {fn eq(&self,other:&Self)->bool {self.encode()==other.encode()}}
impl Default for Placement {
    fn default() -> Self {
        Self {
            center: Vec2::new(0.5, 0.82),
            pins: [false; SLOTS],
            extra:serde_json::json!({}),
        }
    }
}
impl Placement {
    fn decode(value: &serde_json::Value) -> Option<Self> {
        if value.get("version")?.as_u64()? != 1 {
            return None;
        }
        let center = value.get("center")?.as_array()?;
        if center.len() != 2 {
            return None;
        }
        let x = center[0].as_f64()? as f32;
        let y = center[1].as_f64()? as f32;
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let mut out = Self {
            center: Vec2::new(x, y).clamp(Vec2::splat(0.05), Vec2::splat(0.95)),
            ..default()
        };
        out.extra=value.clone();
        let pins = value.get("pins")?.as_array()?;
        if pins.len() != SLOTS {
            return None;
        }
        for (target, value) in out.pins.iter_mut().zip(pins) {
            *target = value.as_bool()?;
        }
        out.pins[AMMO_SWAP] = false;
        Some(out)
    }
    fn encode(&self) -> serde_json::Value {
        let mut out=if self.extra.is_object(){self.extra.clone()}else{serde_json::json!({})};
        out["version"]=serde_json::json!(1);out["center"]=serde_json::json!([self.center.x,self.center.y]);out["pins"]=serde_json::json!(self.pins);out
    }
    fn save(&self) {
        let Some(home) = std::env::var_os(fnv_install::HOME_ENV) else {
            return;
        };
        let path = std::path::PathBuf::from(home).join("touch-hotbar.json");
        let result = self.save_to(&path);
        if result.is_err() {
            warn!("touch hotbar placement could not be saved");
        }
    }
    fn save_to(&self,path:&std::path::Path)->Result<(),String> {
        use std::io::Write;
            fnv_install::guard_write(path, None).map_err(|e| e.to_string())?;
            let mut encoded=self.encode();
            if path.exists() {
                if std::fs::metadata(path).map_err(|e|e.to_string())?.len()>8192 {return Err("Existing tray pin settings exceed bounds; kept unchanged".into())}
                let mut old:serde_json::Value=serde_json::from_slice(&std::fs::read(path).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
                Self::decode(&old).ok_or("Unknown tray pin settings; kept unchanged")?;
                for key in ["version","center","pins"] {old[key]=encoded[key].clone();}
                encoded=old;
            }
            let bytes=serde_json::to_vec_pretty(&encoded).map_err(|e|e.to_string())?;
            if bytes.len()>8192 {return Err("Tray pin settings exceed bounds".into())}
            let tmp = path.with_extension("json.tmp");
            fnv_install::guard_write(&tmp, None).map_err(|e| e.to_string())?;
            let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|e| e.to_string())?;
            drop(file);
            std::fs::rename(tmp, path).map_err(|e| e.to_string())
    }
}
/// The input session is independent of the opt-in bar and its fitted geometry.
#[derive(Clone, Copy, PartialEq, Eq)]
struct MenuTouchSession {
    page: Page,
    tab: usize,
    frame: Frame,
}
#[derive(Clone, Copy)]
struct BindingOrigin {id:u64,slot:usize,released:bool}
#[derive(Resource, Default)]
struct Bar {
    placement: Placement,
    active_tray:Option<super::layout::Placement>,
    safe_area:nvf_play::touch_safe_area::SafeInsets,
    preview:bool,
    gesture: Gesture,
    ignored: HashSet<u64>,
    context: Option<usize>,
    outcomes: Vec<Outcome>,
    tray_open: bool,
    moving: bool,
    move_id: Option<u64>,
    move_original: Option<Vec2>,
    safe_bottom: f32,
    size: Vec2,
    mode: u8,
    menu_touch: Option<MenuTouchSession>,
    fitting_key: Option<FittingKey>,
    resolved: Option<packing::Resolved>,
    occupied: Vec<packing::Occupied>,
    capture_drain: u8,
    request_opening: bool,
    pending_binding: Option<usize>,
    binding_origin:Option<BindingOrigin>,
    binding_slot: Option<usize>,
    message: String,
}
impl Bar {
    fn observe_hold(&mut self, now: u64) {
        if let Some(outcome) = self.gesture.tick(now) {
            if self.outcomes.is_empty() {
                if let Outcome::Context(slot)=outcome {
                    if self.mode==2&&slot!=AMMO_SWAP {
                        self.binding_origin=self.gesture.owner().map(|id|BindingOrigin{id,slot,released:false});
                    }
                }
                self.outcomes.push(outcome);
            }
            self.capture_drain = 2;
        }
    }
    fn retire_binding_request(&mut self) {
        let retiring=self.binding_origin.is_some()||self.pending_binding.is_some();
        if let Some(origin)=self.binding_origin.take() {
            self.outcomes.retain(|outcome|!matches!(outcome,
                Outcome::Context(slot)|Outcome::Tap(Target::Slot(slot)) if *slot==origin.slot));
        }
        self.pending_binding=None;
        if retiring {self.capture_drain=2;}
    }
    fn cancel_binding_owner(&mut self,id:u64) {
        if self.binding_origin.is_some_and(|origin|origin.id==id) {self.retire_binding_request();}
    }
    fn captures_actions(&self) -> bool {
        self.context.is_some()
            || self.binding_origin.is_some_and(|origin|!origin.released)
            || self.moving
            || self.tray_open
            || self.capture_drain > 0
            || self.outcomes.iter().any(|o| {
                matches!(
                    o,
                    Outcome::Context(_) | Outcome::Tap(Target::Context(_) | Target::Tray)
                )
            })
    }
    fn cancel(&mut self) {
        self.mode = 0;
        self.menu_touch = None;
        self.gesture.cancel();
        self.ignored.clear();
        self.outcomes.clear();
        self.context = None;
        self.tray_open = false;
        self.move_id = None;
        if let Some(original) = self.move_original.take() {
            self.placement.center = original;
        }
        self.moving = false;
        self.capture_drain = 0;
        self.request_opening = false;
        self.pending_binding = None;
        self.binding_origin = None;
        self.binding_slot = None;
    }
}
// Shared by early raw admission, late dispatch and the final arm handoff.
// A quantity prompt keeps ordinary item/prompt ingress; only binding retires.
fn enforce_binding_context(bar:&mut Bar,items:&mut TouchItems,pip:Option<&PipBoy>,active:Option<&ActiveMenu>)->bool {
    let menu=active.and_then(|a|a.0.as_ref()).filter(|m|m.mode==Mode::Inventory);
    let reason=if menu.is_some_and(|m|m.quantity.is_some()) {
        Some("Finish the quantity prompt before binding a slot.")
    } else if menu.is_none() || pip.is_none_or(|p|p.phase!=Phase::Up||p.page!=Page::Items) {
        Some("Open ITEMS to bind a slot, then tap the slot again.")
    } else {None};
    if let Some(reason)=reason {
        let pending=bar.binding_origin.is_some()||bar.pending_binding.is_some()||items.binding_slot().is_some();
        bar.retire_binding_request();
        if items.binding_slot().is_some(){items.cancel_binding();}
        bar.binding_slot=None;
        if pending {items.message=reason.into();}
        false
    } else {true}
}
#[derive(Clone, PartialEq)]
struct FittingKey {
    size: Vec2,
    safe: f32,
    count: usize,
    scale: f32,
    tray: bool,
    center: Vec2,
    occupied: Vec<packing::Occupied>,
    safe_area:nvf_play::touch_safe_area::SafeInsets,
    profile_epoch:u64,
}
#[derive(Resource, Default)]
pub(super) struct Occlusion {
    rects: Vec<Rect>,
    capturing: bool,
}
impl Occlusion {
    pub fn blocks(&self, rect: Rect) -> bool {
        self.capturing
            || self.rects.iter().any(|r| {
                let overlap = r.intersect(rect);
                overlap.width() > 0. && overlap.height() > 0.
            })
    }
}

/// Parent inventory gestures can request the tray without taking the Pip-Boy swipe.
#[derive(Message, Clone, Copy)]
pub(crate) struct UtilityTrayRequest(pub bool);

#[derive(Clone, Debug, PartialEq)]
struct Geometry {
    slots: Vec<(usize, Rect)>,
    tray: Option<Rect>,
    context: Vec<Rect>,
    bounds: Rect,
}
fn usable(size: Vec2, bar: &Bar) -> Option<Vec2> {
    let s=bar.safe_area;
    if !size.is_finite() || !bar.safe_bottom.is_finite() || [s.left,s.top,s.right,s.bottom].into_iter().any(|v|!v.is_finite() || v<0.0) {
        return None;
    }
    let size = Vec2::new(size.x-s.left-s.right, size.y-s.top-s.bottom.max(bar.safe_bottom.max(0.))-64.);
    (size.min_element() >= 96.).then_some(size)
}
fn rect2(r: Rect) -> packing::Rect2 {
    packing::Rect2 {
        x: r.min.x,
        y: r.min.y,
        w: r.width(),
        h: r.height(),
    }
}
fn bevy_rect(r: packing::Rect2) -> Rect {
    Rect::from_corners(Vec2::new(r.x, r.y), Vec2::new(r.x + r.w, r.y + r.h))
}
fn fit(size: Vec2, prefs: &Preferences, bar: &Bar) -> Option<packing::Resolved> {
    let raw_size=size;
    let size = usable(size, bar)?;
    let origin=Vec2::new(bar.safe_area.left,bar.safe_area.top);
    let (anchor,scale)=bar.active_tray.as_ref().map_or((bar.placement.center,prefs.scale),|t|((t.center*raw_size-origin)/size,t.scale));
    let occupied:Vec<_>=bar.occupied.iter().map(|s|packing::Occupied {rect:packing::Rect2{x:s.rect.x-origin.x,y:s.rect.y-origin.y,..s.rect},radius:s.radius}).collect();
    let mut resolved=packing::fit(
        size.to_array(),
        anchor.to_array(),
        (size.min_element() * 0.11 * scale).clamp(48., 96.),
        prefs.count + usize::from(prefs.tray),
        &occupied,
    )?;
    for cell in &mut resolved.cells {cell.x+=origin.x;cell.y+=origin.y;}
    resolved.panel.x+=origin.x;resolved.panel.y+=origin.y;
    Some(resolved)
}
fn geometry(size: Vec2, prefs: &Preferences, bar: &Bar) -> Option<Geometry> {
    let raw_size = size;
    let size = usable(size, bar)?;
    let origin=Vec2::new(bar.safe_area.left,bar.safe_area.top);
    let fitted = if bar.fitting_key.is_some() {
        bar.resolved.clone()?
    } else {
        fit(raw_size, prefs, bar)?
    };
    let mut slots = fitted
        .cells
        .iter()
        .take(prefs.count)
        .enumerate()
        .map(|(i, r)| (i, bevy_rect(*r)))
        .collect::<Vec<_>>();
    let tray = if prefs.tray {
        fitted.cells.get(prefs.count).copied().map(bevy_rect)
    } else {
        None
    };
    if prefs.tray && bar.tray_open {
        let pinned = bar
            .placement
            .pins
            .iter()
            .enumerate()
            .filter(|(_, on)| **on)
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        if !pinned.is_empty() {
            // Expanded pins are an explicit captured UI layer; retain the base bar
            // as occupied so duplicate slot views cannot cover each other.
            let occupied = fitted
                .cells
                .iter()
                .map(|r| packing::Occupied {
                    rect: packing::Rect2{x:r.x-origin.x,y:r.y-origin.y,..*r},
                    radius: 0.,
                })
                .collect::<Vec<_>>();
            if let Some(pin_layout) = packing::fit(
                size.to_array(),
                [0.5, 0.3],
                fitted.side,
                pinned.len(),
                &occupied,
            ) {
                slots.extend(
                    pinned
                        .into_iter()
                        .zip(pin_layout.cells)
                        .map(|(i, r)| (i, bevy_rect(packing::Rect2{x:r.x+origin.x,y:r.y+origin.y,..r}))),
                );
            }
        }
    }
    let bounds = bevy_rect(fitted.panel);
    let context = if bar.context.is_some() || bar.binding_slot.is_some() {
        let count = 4;
        let width = (size.x * 0.42).clamp(220., 360.).min(size.x - 16.);
        let height = 48.;
        let top = (bounds.min.y - height * count as f32 - 12.).clamp(origin.y+4., (origin.y+size.y - height * count as f32 - 4.).max(origin.y+4.));
        (0..count)
            .map(|i| {
                Rect::from_corners(
                    Vec2::new(origin.x+(size.x - width) * 0.5, top + i as f32 * height),
                    Vec2::new(origin.x+(size.x + width) * 0.5, top + (i + 1) as f32 * height),
                )
            })
            .collect()
    } else {
        Vec::new()
    };
    Some(Geometry {
        slots,
        tray,
        context,
        bounds,
    })
}
fn box2(r: Rect) -> Box2 {
    Box2 {
        min: r.min.to_array(),
        max: r.max.to_array(),
    }
}

#[derive(SystemParam)]
pub(super) struct Bridge<'w, 's> {
    bar: ResMut<'w, Bar>,
    prefs: Res<'w, Preferences>,
    items: ResMut<'w, TouchItems>,
    active: Option<Res<'w, ActiveMenu>>,
    pip: Option<Res<'w, PipBoy>>,
    fresh: Option<Res<'w, nvf_play::menu::FreshMenu>>,
    paused: Option<Res<'w, nvf_play::pause::Paused>>,
    script: Option<Res<'w, nvf_play::script::ScriptState>>,
    time: Res<'w, Time<Real>>,
    safe: Option<Res<'w, super::drawer::SafeBottom>>,
    occlusion: ResMut<'w, Occlusion>,
    capture: ResMut<'w, OverlayCapture>,
    requests: MessageReader<'w, 's, UtilityTrayRequest>,
    edit:ResMut<'w,super::tray_editor::Requests>,
    preview:Res<'w,super::tray_editor::PreviewMode>,
    safe_area:Option<Res<'w,nvf_play::touch_safe_area::SafeInsets>>,
}
impl Bridge<'_, '_> {
    pub fn cancel(&mut self) {
        self.requests.clear();
        self.cancel_owned_ui();
    }
    fn cancel_owned_ui(&mut self) {
        self.bar.cancel();
        self.edit.0=false;
        if self.preview.0 {self.bar.mode=3;self.bar.preview=true;}
        self.items.cancel();
        self.publish_capture();
        self.occlusion.rects.clear();
    }
    pub fn captures_actions(&self) -> bool {
        self.bar.captures_actions()
    }
    pub fn take_editor_request(&mut self)->bool {std::mem::take(&mut self.edit.0)}
    fn publish_capture(&mut self) {
        publish_capture(
            &self.bar,
            &mut self.items,
            &mut self.capture,
            &mut self.occlusion,
        );
    }
    pub fn apply_tray_request(&mut self, open: bool) -> bool {
        if self.bar.preview {return false}
        if open && (self.bar.mode == 0 || !self.prefs.tray_available()) { return false; }
        if open {
            // Retire canonical binding state before the queued-open batch owns
            // releases. Dismissal must never resurrect an old slot request.
            self.bar.retire_binding_request();
            self.bar.binding_slot=None;
            if self.items.binding_slot().is_some(){self.items.cancel_binding();}
            else{self.items.cancel();}
        }
        if self.bar.tray_open != open {
            if open {
                // This request owns the opening batch, including old held releases.
                // A contact in that batch cannot select or dismiss the new UI.
                self.bar.gesture.cancel();
                self.bar.outcomes.clear();
                self.bar.ignored.clear();
                self.bar.context = None;
                self.bar.move_id = None;
                if let Some(original) = self.bar.move_original.take() {
                    self.bar.placement.center = original;
                    // prepare may have fitted the live drag before this request.
                    // Geometry must resolve the restored anchor in this same frame.
                    self.bar.fitting_key = None;
                    self.bar.resolved = None;
                }
                self.bar.moving = false;
                self.bar.request_opening = true;
            }
            self.bar.tray_open = open;
            self.bar.capture_drain = 2;
        }
        self.publish_capture();
        true
    }
    pub fn blocks(&self, rect: Rect) -> bool {
        self.occlusion.blocks(rect)
    }
    pub fn items_pending(&self) -> bool { self.items.pending_contact() }
    pub fn prepare(
        &mut self,
        size: Vec2,
        allowed: bool,
        playing: bool,
        config: &super::layout::Config,
        gyro_enabled: bool,
        extra: &[(Rect, f32)],
    ) {
        let preview=self.preview.0 && allowed;
        let playing=playing || preview;
        self.bar.active_tray=config.tray_supported.then(||config.tray.clone());
        self.bar.preview=preview;
        self.bar.safe_area=self.safe_area.as_deref().copied().unwrap_or_default();
        self.bar.request_opening = false;
        enforce_binding_context(&mut self.bar,&mut self.items,self.pip.as_deref(),self.active.as_deref());
        self.bar.binding_slot = self.items.binding_slot();
        let inventory = self
            .active
            .as_ref()
            .and_then(|a| a.0.as_ref())
            .is_some_and(|m| m.mode == Mode::Inventory)
            && self.pip.as_ref().is_some_and(|p| p.phase == Phase::Up);
        let allowed = allowed && !self.paused.as_ref().is_some_and(|p| p.0)
            && !(inventory && self.script.as_ref().and_then(|s| s.rt.as_ref())
                .is_some_and(|rt| nvf_play::tiles::stats_menu::modal_over(rt.game.menu)));
        let menu_touch = self.pip.as_ref().filter(|_| {
            allowed && !playing && inventory && size.is_finite() && size.min_element() >= 64.
        }).map(|pip| MenuTouchSession { page: pip.page, tab: pip.tab(), frame: pip.frame });
        let mode = if preview {3} else if !allowed || !self.prefs.enabled || !config.tray.visible {
            0
        } else if playing {
            1
        } else if inventory {
            2
        } else {
            0
        };
        let safe = self.safe.as_ref().map_or(0., |s| s.0).max(0.);
        let mut occupied = Vec::new();
        if size.is_finite() && size.min_element() >= 96. {
            for (i, b) in super::BUTTONS.iter().enumerate() {
                if b.playing != playing
                    || !config.buttons[i].visible
                    || (i == super::gyro::BUTTON && !gyro_enabled)
                {
                    continue;
                }
                let t = super::layout::Target::Button(i);
                let g = config.appearance.geometry(config.diameter(t, size));
                occupied.push(packing::Occupied {
                    rect: rect2(Rect::from_center_size(
                        config.center(t, size),
                        Vec2::new(g.width, g.height),
                    )),
                    radius: g.radius,
                });
            }
            if playing {
                let r = config.diameter(super::layout::Target::Stick, size) * 0.65;
                occupied.push(packing::Occupied {
                    rect: rect2(Rect::from_center_size(
                        config.center(super::layout::Target::Stick, size),
                        Vec2::splat(r * 2.),
                    )),
                    radius: r,
                });
                occupied.push(packing::Occupied {
                    rect: rect2(Rect::from_center_size(size * 0.5, Vec2::splat(48.))),
                    radius: 0.,
                });
            }
            occupied.extend(extra.iter().map(|(r, radius)| packing::Occupied {
                rect: rect2(*r),
                radius: *radius,
            }));
        }
        let key = FittingKey {
            size,
            safe,
            count: self.prefs.count,
            scale: self.bar.active_tray.as_ref().map_or(self.prefs.scale,|t|t.scale),
            tray: self.prefs.tray,
            center: self.bar.active_tray.as_ref().map_or(self.bar.placement.center,|t|t.center),
            occupied: occupied.clone(),
            safe_area:self.bar.safe_area,
            profile_epoch:config.profile_epoch,
        };
        let context_changed = menu_touch != self.bar.menu_touch
            || self.bar.size != size
            || self.bar.safe_bottom != safe
            || self.fresh.as_ref().is_some_and(|f| f.0);
        if mode != self.bar.mode || context_changed {
            if self.bar.mode == 0 && mode == 1 && !context_changed {
                // The preview publisher retires mode3 after explicit editor
                // Cancel. A new gameplay request queued after that Cancel is
                // fresh; reconcile owned UI without discarding that request.
                self.cancel_owned_ui();
            } else { self.cancel(); }
        }
        if self.bar.fitting_key.as_ref() != Some(&key) {
            self.bar.retire_binding_request();
            self.bar.gesture.cancel();
            self.bar.outcomes.clear();
            self.items.cancel();
            self.bar.size = size;
            self.bar.safe_bottom = safe;
            self.bar.occupied = occupied;
            self.bar.resolved = fit(size, &self.prefs, &self.bar);
            self.bar.fitting_key = Some(key);
        }
        self.bar.mode = mode;
        self.bar.menu_touch = menu_touch;
        self.bar.size = size;
        self.bar.safe_bottom = safe;
        // Single request reader: use this frame's admitted mode before the parent
        // snapshots capture or routes generic touch. The drawer calls the same
        // method directly, so it needs no competing writer/reader in this system.
        if let Some(open) = self.requests.read().last().map(|request| request.0) {
            self.apply_tray_request(open);
        }
        if mode != 0 {
            let now = self
                .time
                .elapsed()
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX);
            self.bar.observe_hold(now);
        }
        self.bar.capture_drain =
            if self.bar.context.is_some() || self.bar.moving || self.bar.tray_open {
                2
            } else {
                self.bar.capture_drain.saturating_sub(1)
            };
        self.occlusion.rects = if mode == 0 {
            Vec::new()
        } else {
            geometry(size, &self.prefs, &self.bar).map_or_else(Vec::new, |g| {
                g.slots.into_iter().map(|(_, r)| r).chain(g.tray).collect()
            })
        };
        self.publish_capture();
    }
    pub fn event(&mut self, event: &TouchInput, unclaimed: bool) -> bool {
        self.publish_capture();
        if self.bar.preview {return true}
        if self.bar.request_opening { return true; }
        let consumed = self.route_event(event, unclaimed);
        self.publish_capture();
        consumed
    }
    fn route_event(&mut self, event: &TouchInput, unclaimed: bool) -> bool {
        let now = self
            .time
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX);
        if self.bar.menu_touch.is_some() && !self.captures_actions() && self.items.owns(event.id) {
            self.items.event(event, now);
            if event.phase == TouchPhase::Ended {
                // Assignment is an opt-in visible bar operation. Basic navigation
                // keeps its ordinary release even with no bar or fitted slots.
                let target = (self.bar.mode == 2).then(|| geometry(self.bar.size, &self.prefs, &self.bar))
                    .flatten().and_then(|g| {
                        g.slots.into_iter().find(|(_, r)| r.contains(event.position)).map(|(i, _)| i)
                    });
                self.items.release_over(target);
            }
            return true;
        }
        if self.bar.mode == 0 {
            return self.menu_start(event, unclaimed, now);
        }
        if self.bar.ignored.contains(&event.id) {
            if matches!(event.phase, TouchPhase::Ended | TouchPhase::Canceled) {
                self.bar.ignored.remove(&event.id);
            }
            return true;
        }
        if self.bar.move_id == Some(event.id) {
            if event.phase == TouchPhase::Moved && event.position.is_finite() {
                let size = Vec2::new(
                    self.bar.size.x,
                    (self.bar.size.y - self.bar.safe_bottom - 64.).max(1.),
                );
                self.bar.placement.center =
                    (event.position / size).clamp(Vec2::splat(0.05), Vec2::splat(0.95));
            }
            if matches!(event.phase, TouchPhase::Ended | TouchPhase::Canceled) {
                self.bar.move_id = None;
                self.bar.moving = false;
                self.bar.capture_drain = 2;
                if event.phase == TouchPhase::Ended && event.position.is_finite() {
                    self.bar.move_original = None;
                    self.bar.placement.save();
                } else if let Some(original) = self.bar.move_original.take() {
                    self.bar.placement.center = original;
                }
            }
            return true;
        }
        let Some(g) = geometry(self.bar.size, &self.prefs, &self.bar) else {
            return self.menu_start(event, unclaimed, now);
        };
        if self.bar.gesture.owns(event.id) {
            let size = self.bar.size.to_array();
            self.bar
                .gesture
                .moved(event.id, event.position.to_array(), size);
            if event.phase != TouchPhase::Canceled {
                self.bar.observe_hold(now);
            }
            let valid=self.bar.gesture.valid_release(event.id,event.position.to_array(),size);
            if event.phase==TouchPhase::Canceled||!valid {
                self.bar.cancel_binding_owner(event.id);
            } else if event.phase==TouchPhase::Ended {
                if let Some(origin)=self.bar.binding_origin.as_mut().filter(|o|o.id==event.id) {
                    origin.released=true;
                    self.bar.capture_drain=2;
                }
            }
            match event.phase {
                TouchPhase::Moved => {
                    self.bar
                        .gesture
                        .moved(event.id, event.position.to_array(), size)
                }
                TouchPhase::Ended | TouchPhase::Canceled => {
                    if let Some(o) = self.bar.gesture.end(
                        event.id,
                        event.position.to_array(),
                        size,
                        now,
                        event.phase == TouchPhase::Canceled,
                    ) {
                        if matches!(
                            o,
                            Outcome::Context(_) | Outcome::Tap(Target::Context(_) | Target::Tray)
                        ) {
                            self.bar.capture_drain = 2;
                        }
                        if self.bar.outcomes.is_empty() {
                            if let Outcome::Tap(Target::Slot(slot))=o {
                                if self.bar.mode==2&&slot!=AMMO_SWAP {
                                    self.bar.binding_origin=Some(BindingOrigin{id:event.id,slot,released:true});
                                }
                            }
                            self.bar.outcomes.push(o);
                        }
                    }
                }
                _ => {}
            }
            return true;
        }
        if event.phase != TouchPhase::Started {
            return false;
        }
        if self.bar.moving && g.bounds.contains(event.position) {
            self.bar.move_id = Some(event.id);
            self.bar.move_original = Some(self.bar.placement.center);
            return true;
        }
        let target = g
            .context
            .iter()
            .enumerate()
            .find(|(_, r)| r.contains(event.position))
            .map(|(i, r)| (Target::Context(i), *r))
            .or_else(|| {
                g.tray
                    .filter(|r| r.contains(event.position))
                    .map(|r| (Target::Tray, r))
            })
            .or_else(|| {
                g.slots
                    .iter()
                    .find(|(_, r)| r.contains(event.position))
                    .map(|(i, r)| (Target::Slot(*i), *r))
            });
        if let Some((target, rect)) = target {
            if !unclaimed && !self.captures_actions() {
                return false;
            }
            if self.bar.gesture.busy() || self.items.busy() {
                if self.bar.ignored.len() < 16 {
                    self.bar.ignored.insert(event.id);
                }
            } else {
                // A newer owned UI press supersedes a released request that has
                // not yet passed its drain. Active binding has no pending origin.
                self.bar.retire_binding_request();
                let size = self.bar.size.to_array();
                self.bar.gesture.start(
                    event.id,
                    target,
                    box2(rect),
                    event.position.to_array(),
                    size,
                    now,
                );
            }
            return true;
        }
        if self.bar.context.is_some() {
            self.bar.context = None;
            self.bar.capture_drain = 2;
            return true;
        }
        if self.bar.tray_open {
            self.bar.tray_open = false;
            self.bar.capture_drain = 2;
            return true;
        }
        if self.bar.moving {
            self.bar.moving = false;
            self.bar.capture_drain = 2;
            return true;
        }
        self.menu_start(event, unclaimed, now)
    }
    fn menu_start(&mut self, event: &TouchInput, unclaimed: bool, now: u64) -> bool {
        if self.bar.menu_touch.is_some() && event.phase == TouchPhase::Started
            && unclaimed && !self.captures_actions() && !self.items.busy()
        {
            return self.items.event(event, now);
        }
        false
    }
}

fn publish_capture(
    bar: &Bar,
    items: &mut TouchItems,
    capture: &mut OverlayCapture,
    occlusion: &mut Occlusion,
) {
    // Binding's three explicit buttons consume their own raw contacts. Keep the
    // item surface admitted so its next guarded tap can propose an item.
    let captured = bar.captures_actions() && items.binding_slot().is_none();
    capture.0 = captured;
    occlusion.capturing = captured;
    if captured {
        items.cancel();
    }
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<bevy::picking::pointer::PointerMap>()
        .init_resource::<Occlusion>()
        .init_resource::<Preferences>()
        .init_resource::<super::tray_editor::Requests>()
        .init_resource::<super::tray_editor::PreviewMode>()
        .init_resource::<Bar>()
        .init_resource::<TouchItems>()
        .init_resource::<OverlayCapture>()
        .init_resource::<bevy::picking::pointer::PointerMap>()
        .add_message::<UtilityTrayRequest>()
        .add_message::<nvf_play::weapon_wheel::TouchHotkey>()
        .add_systems(Startup, load)
        .add_systems(PreUpdate,migrate_tray.after(super::drawer::PublishSafeBottom).before(super::tray_editor::PublishMode).before(super::TouchInputSystems))
        .add_systems(PreUpdate,publish_preview.in_set(PreviewPublication).after(super::TouchInputSystems).after(nvf_play::pad::InputSwitchSystems::Switch).before(nvf_play::pipboy::TouchPointerSet).before(bevy::ui::UiSystems::Focus))
        .add_systems(
            Update,
            dispatch
                .before(draw)
                .before(nvf_play::PlaySet::Input)
                .before(nvf_play::pipboy::PointerSet)
                .run_if(resource_exists::<nvf_play::sim::SimState>)
                .run_if(resource_exists::<Hotkeys>)
                .run_if(resource_exists::<nvf_play::bindings::Bindings>),
        )
        .add_systems(
            Update,
            (assign, draw)
                .chain()
                .after(nvf_play::pipboy::PointerSet)
                .run_if(resource_exists::<nvf_play::sim::SimState>)
                .run_if(resource_exists::<Hotkeys>),
        );
    let row = |key, label, note, get, set| Setting {
        key,
        group: "Controls",
        label,
        note,
        switches: "",
        get,
        set,
        slider: None,
    };
    settings::add_setting(
        app,
        row(
            "touch-hotbar",
            "Touch hotbar",
            "Show quick slots on screen. Tap to use; hold for safe slot options. Item assignments follow the game's saved hotkeys.",
            |w| w.resource::<Preferences>().enabled,
            |w, on| w.resource_mut::<Preferences>().enabled = on,
        ),
    );
    settings::add_setting(
        app,
        row(
            "touch-hotbar-tray",
            "Pinned consumable tray",
            "Show a tray button for pinned Aid slots. Holding a slot offers Pin; no item is consumed by a hold.",
            |w| w.resource::<Preferences>().tray,
            |w, on| w.resource_mut::<Preferences>().tray = on,
        ),
    );
    settings::add_choice_list(
        app,
        row(
            "touch-hotbar-slots",
            "Visible quick slots",
            "Choose four, six or eight. Slot 2 remains Ammo Swap. Hidden slots remain assigned.",
            |_| false,
            |_, _| {},
        ),
        ChoiceList {
            names: |_| vec!["4".into(), "6".into(), "8".into()],
            chosen: |w| w.resource::<Preferences>().count.to_string(),
            pick: |w, v| {
                let n = match v {
                    "4" => 4,
                    "6" => 6,
                    "8" => 8,
                    _ => return false,
                };
                w.resource_mut::<Preferences>().count = n;
                true
            },
            save: |w| w.resource::<Preferences>().count.to_string(),
            load: |w, v| {
                let n = match v {
                    "4" => 4,
                    "6" => 6,
                    "8" => 8,
                    _ => return false,
                };
                w.resource_mut::<Preferences>().count = n;
                true
            },
            ask_keep: false,
        },
    );
    settings::add_setting(
        app,
        Setting::slider(
            "touch-hotbar-size",
            "Controls",
            "Quick slot size",
            "Choose a preferred size; slots wrap or shrink to fit around controls, with a 48-pixel minimum. Hold a slot and choose Move bar to reposition.",
            Slider::choice(
                13,
                |w| {
                    settings::choice_part(
                        ((current_scale(w) * 100.0 - 60.0) / 10.0)
                            .round()
                            .clamp(0.0, 12.0) as usize,
                        13,
                    )
                },
                |w, v| {
                    set_scale(w,(60 + settings::choice_index(v, 13) * 10) as f32 / 100.0)
                },
                |v| format!("{}%", 60 + settings::choice_index(v, 13) * 10),
            ),
        ),
    );
}
fn current_scale(world:&World)->f32 {
    if world.get_resource::<super::tray_editor::Ready>().is_some_and(|r|r.0) {
        if let Some(scale)=world.get_resource::<super::Layout>().and_then(super::Layout::committed_tray_scale) {return scale}
    }
    world.resource::<Preferences>().scale
}
fn set_scale(world:&mut World,scale:f32) {
    let mut accepted=true;
    if world.get_resource::<super::tray_editor::Ready>().is_some_and(|r|r.0) {
        if let Some(mut layout)=world.get_resource_mut::<super::Layout>() {
            if layout.editor.is_some() {layout.message="Use Smaller/Larger in the preview, then Save or Cancel before changing the size setting.".into();return}
            if !layout.config.tray_supported {layout.message="This tray uses a newer version; kept unchanged".into();return}
            if layout.config.tray_explicit && layout.config.tray_supported {
                let before=layout.config.tray.scale;layout.config.tray.scale=scale;
                if !layout.tray_editing() {
                    if let Err(error)=layout.save() {layout.config.tray.scale=before;layout.message=format!("Could not save tray size: {error}");accepted=false;}
                }
            }
        }
    }
    if accepted {world.resource_mut::<Preferences>().scale=scale;}
}
fn migrate_tray(layout:Option<ResMut<super::Layout>>,bar:Res<Bar>,prefs:Res<Preferences>,windows:Query<&Window,With<bevy::window::PrimaryWindow>>,safe:Option<Res<super::drawer::SafeBottom>>) {
    let Some(mut layout)=layout else {return};
    if layout.config.tray_explicit || !layout.config.tray_supported || layout.editor.is_some() {return}
    let Ok(window)=windows.single() else {return};
    let size=Vec2::new(window.width(),window.height());
    if !size.is_finite() || size.min_element()<96.0 {return}
    let height=(size.y-safe.as_ref().map_or(0.0,|s|s.0).max(0.0)-64.0).max(1.0);
    layout.config.tray.center=Vec2::new(bar.placement.center.x,bar.placement.center.y*height/size.y);
    layout.config.tray.scale=prefs.scale;
}
#[derive(SystemSet,Debug,Clone,Copy,PartialEq,Eq,Hash)]
pub(super) struct PreviewPublication;
type PreviewMirrors<'w> = (ResMut<'w,TouchItems>,ResMut<'w,OverlayCapture>,ResMut<'w,Occlusion>);
type PreviewAdmission<'w> = (Option<Res<'w,super::TouchState>>,Option<Res<'w,nvf_play::bindings::Bindings>>,Option<Res<'w,nvf_play::pad::TouchDeviceSwitch>>);
fn publish_preview(layout:Option<ResMut<super::Layout>>,mut bar:ResMut<Bar>,prefs:Res<Preferences>,modal:Option<Res<super::modal::Capture>>,platform:Option<Res<nvf_play::pad::PlatformInputGate>>, (state,bindings,switch):PreviewAdmission<'_>, (mut items,mut capture,mut occlusion):PreviewMirrors<'_>) {
    let Some(mut layout)=layout else {return};
    if !layout.tray_editing() {
        layout.tray_preview=None;
        if bar.mode==3 || bar.preview {
            bar.cancel();bar.preview=false;items.cancel();occlusion.rects.clear();
            publish_capture(&bar,&mut items,&mut capture,&mut occlusion);
        }
        return;
    }
    if !layout.tray_admitted || modal.as_deref().is_some_and(super::modal::Capture::blocking)
        || platform.as_deref().is_some_and(|g|!g.allows_actions())
        || !super::tray_editor::preview_allowed(state.as_deref(),bindings.as_deref(),switch.as_deref()) {
        layout.cancel_editor_drag();layout.tray_preview=None;
        // prepare may have selected an ordinary mode while preview was blocked.
        // Never publish that live bar through the open editor transaction.
        bar.cancel();bar.preview=false;items.cancel();occlusion.rects.clear();
        publish_capture(&bar,&mut items,&mut capture,&mut occlusion);
        return;
    }
    bar.mode=3;bar.preview=true;
    if bar.active_tray.as_ref()!=Some(&layout.config.tray) {
        bar.active_tray=Some(layout.config.tray.clone());
        if let Some(key)=bar.fitting_key.as_mut() {key.center=layout.config.tray.center;key.scale=layout.config.tray.scale;}
        bar.resolved=fit(bar.size,&prefs,&bar);
    }
    layout.tray_preview=geometry(bar.size,&prefs,&bar).map(|g|super::tray_editor::Preview {bounds:g.bounds,slots:g.slots.into_iter().map(|(_,r)|r).chain(g.tray).collect()});
    // Drawn preview, parent exclusions and the next core pointer route share the
    // final fitted result, including same-frame movement/resize.
    occlusion.rects=layout.tray_preview.as_ref().map_or_else(Vec::new,|p|p.slots.clone());
    items.cancel();publish_capture(&bar,&mut items,&mut capture,&mut occlusion);
}
fn load(mut commands: Commands, mut bar: ResMut<Bar>) {
    commands.spawn(nvf_play::pipboy::touch_items::POINTER);
    if !cfg!(target_os = "android") {
        return;
    }
    let Some(home) = std::env::var_os(fnv_install::HOME_ENV) else {
        return;
    };
    let path = std::path::PathBuf::from(home).join("touch-hotbar.json");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() <= 8192) {
        if let Some(p) = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .and_then(|v| Placement::decode(&v))
        {
            bar.placement = p;
        }
    }
}
type DispatchScene<'w> = (
    Option<Res<'w,ActiveMenu>>,Option<Res<'w,nvf_play::pause::Paused>>,
    Option<Res<'w,nvf_play::script::ScriptState>>,Option<Res<'w,PipBoy>>,
);
type DispatchInputState<'w> = (
    Option<Res<'w,nvf_play::pad::TouchDeviceSwitch>>,Res<'w,Time<Real>>,
    Option<Res<'w,nvf_play::pipboy::touch_scroll::PhysicalPriority>>,
);
fn dispatch(
    input: ControlInput,
    (active,paused,script,pip): DispatchScene<'_>,
    (mut bar, mut items,mut edit): (ResMut<Bar>, ResMut<TouchItems>,ResMut<super::tray_editor::Requests>),
    (mut capture, mut occlusion): (ResMut<OverlayCapture>, ResMut<Occlusion>),
    (mut hotkeys, mut sim): (ResMut<Hotkeys>, ResMut<nvf_play::sim::SimState>),
    (touch, time, physical): DispatchInputState<'_>,
    mut out: MessageWriter<nvf_play::weapon_wheel::TouchHotkey>,
) {
    if bar.preview {bar.gesture.cancel();bar.outcomes.clear();items.cancel();publish_capture(&bar,&mut items,&mut capture,&mut occlusion);return}
    if input.controller()
        || input.suppressed()
        || paused.as_ref().is_some_and(|p| p.0)
        || (bar.mode == 2 && script.as_ref().and_then(|s| s.rt.as_ref())
            .is_some_and(|rt| nvf_play::tiles::stats_menu::modal_over(rt.game.menu)))
        || touch.as_ref().is_some_and(|t| !t.allows_actions())
    {
        bar.cancel();
        items.cancel();
        publish_capture(&bar, &mut items, &mut capture, &mut occlusion);
        return;
    }
    let now = time.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
    items.update_hold(now);
    bar.observe_hold(now);
    enforce_binding_context(&mut bar,&mut items,pip.as_deref(),active.as_deref());
    if physical.as_ref().is_some_and(|p|p.active)
        && (bar.binding_origin.is_some()||bar.pending_binding.is_some()||items.binding_slot().is_some()) {
        bar.retire_binding_request();
        if items.binding_slot().is_some(){items.cancel_binding();}
    }
    for outcome in std::mem::take(&mut bar.outcomes) {
        match outcome {
            Outcome::Context(slot) => {
                if bar.mode == 2 && slot != AMMO_SWAP {
                    bar.pending_binding = Some(slot);
                } else {
                    bar.context = Some(slot);
                }
                bar.capture_drain = 2;
            }
            Outcome::Tap(Target::Slot(slot)) => {
                if bar.mode == 1 && slot < SLOTS {
                    out.write(nvf_play::weapon_wheel::TouchHotkey(slot));
                } else if bar.mode == 2 && slot != AMMO_SWAP {
                    bar.pending_binding = Some(slot);
                    bar.capture_drain = 2;
                }
            }
            Outcome::Tap(Target::Tray) => {
                bar.tray_open = !bar.tray_open;
                bar.capture_drain = 2;
            }
            Outcome::Tap(Target::Context(action)) => {
                bar.capture_drain = 2;
                if items.binding_slot().is_some() {
                    match action {
                        0 => { if let Some(active) = active.as_deref() { items.confirm_binding(active, &mut hotkeys,&mut sim.world); } }
                        1 => items.choose_binding_again(),
                        2 => items.cancel_binding(),
                        _ => {
                            if let Some(slot) = items.binding_slot().filter(|s| *s != AMMO_SWAP) {
                                hotkeys.0[slot] = None;
                                bar.placement.pins[slot] = false;
                                bar.placement.save();
                                items.cancel_binding();
                                items.message = "Slot cleared.".into();
                            }
                        }
                    }
                    continue;
                }
                let Some(slot) = bar.context.take() else {
                    continue;
                };
                match action {
                    0 => {
                        let aid = hotkeys.0[slot]
                            .and_then(|s| sim.forms.item(fnv_sim::FormId(s.item)))
                            .is_some_and(|i| i.kind == fnv_sim::forms::ItemKind::Aid);
                        if slot != AMMO_SWAP && aid {
                            bar.placement.pins[slot] = !bar.placement.pins[slot];
                            bar.placement.save();
                        } else {
                            bar.message = "Only Aid slots can be pinned.".into();
                        }
                    }
                    1 => {
                        if slot != AMMO_SWAP {
                            hotkeys.0[slot] = None;
                            bar.placement.pins[slot] = false;
                            bar.placement.save();
                            bar.message = "Slot cleared.".into();
                        } else {
                            bar.message = "Slot 2 is reserved for Ammo Swap.".into();
                        }
                    }
                    2 => {
                        edit.0=true;
                        bar.message = "Opening the tray layout editor.".into();
                    }
                    _ => {}
                }
            }
        }
    }
    publish_capture(&bar, &mut items, &mut capture, &mut occlusion);
}
fn assign(
    mut sim: Option<ResMut<nvf_play::sim::SimState>>,
    (active,pip): (Option<Res<ActiveMenu>>,Option<Res<PipBoy>>),
    mut items: ResMut<TouchItems>,
    mut hotkeys: ResMut<Hotkeys>,
    mut bar: ResMut<Bar>,
) {
    let binding_allowed=enforce_binding_context(&mut bar,&mut items,pip.as_deref(),active.as_deref());
    if bar.mode != 2 || !binding_allowed {
        bar.retire_binding_request();
        if items.binding_slot().is_some() { items.cancel_binding(); }
    } else if !bar.captures_actions() && !bar.gesture.busy()
        && bar.binding_origin.is_some_and(|o|o.released&&Some(o.slot)==bar.pending_binding) {
        if let Some(slot) = bar.pending_binding.take() {
            bar.binding_origin=None;
            items.arm_binding(slot);
        }
    }
    if let Some((active, sim)) = active.zip(sim.as_deref_mut()) {
        items.assign_pending(&active, &mut hotkeys, &mut sim.world);
    }
    bar.binding_slot = items.binding_slot();
}

#[derive(Component)]
struct Visual;
#[derive(Clone, PartialEq)]
struct Picture {
    mode: u8,
    size: Vec2,
    geometry: Geometry,
    entries: Vec<(usize, String, i32, bool)>,
    context: Option<usize>,
    binding: Option<(usize, bool, String)>,
    pinned: [bool; SLOTS],
    message: String,
}
#[derive(Default)]
struct DrawCache {
    picture: Option<Picture>,
    no_room: bool,
}
fn draw(
    mut commands: Commands,
    (prefs, bar, items): (Res<Preferences>, Res<Bar>, Res<TouchItems>),
    hotkeys: Res<Hotkeys>,
    mut sim: ResMut<nvf_play::sim::SimState>,
    roots: Query<Entity, With<Visual>>,
    mut previous: Local<DrawCache>,
) {
    if bar.mode == 0 || (!prefs.enabled && !bar.preview) {
        for e in &roots {
            commands.entity(e).despawn();
        }
        *previous = DrawCache::default();
        return;
    }
    let Some(g) = geometry(bar.size, &prefs, &bar) else {
        if !previous.no_room {
            for e in &roots {
                commands.entity(e).despawn();
            }
            let width = (bar.size.x - 16.).clamp(48., 480.);
            tile(
                &mut commands,
                Rect::from_corners(
                    Vec2::new((bar.size.x - width) * 0.5, 60.),
                    Vec2::new((bar.size.x + width) * 0.5, 108.),
                ),
                "Hotbar needs space. Move/hide controls in the touch editor.",
                None,
                false,
            );
            previous.picture = None;
            previous.no_room = true;
        }
        return;
    };
    previous.no_room = false;
    let mut entries = Vec::new();
    for (slot, _) in &g.slots {
        let (name, count, aid) = if *slot == AMMO_SWAP {
            ("Ammo Swap".into(), 1, false)
        } else if let Some(s) = hotkeys.0[*slot] {
            let name = sim
                .name(fnv_sim::FormId(s.item))
                .unwrap_or("Unknown")
                .chars()
                .take(24)
                .collect();
            let aid = sim
                .forms
                .item(fnv_sim::FormId(s.item))
                .is_some_and(|i| i.kind == fnv_sim::forms::ItemKind::Aid);
            (name, nvf_play::hotkeys::carried(&mut sim.world, s).0, aid)
        } else {
            ("Empty".into(), 0, false)
        };
        entries.push((*slot, name, count, aid));
    }
    let picture = Picture {
        mode: bar.mode,
        size: bar.size,
        geometry: g.clone(),
        entries: entries.clone(),
        context: bar.context,
        binding: items.binding_slot().map(|slot| (slot, items.binding_ready(), items.binding_label().to_owned())),
        pinned: bar.placement.pins,
        message: if bar.tray_open && !bar.placement.pins.iter().any(|p| *p) {
            "No pins yet. Hold an Aid slot to pin it.".into()
        } else if items.message.is_empty() {
            bar.message.clone()
        } else {
            items.message.clone()
        },
    };
    if previous.picture.as_ref() == Some(&picture) {
        return;
    }
    previous.picture = Some(picture.clone());
    for e in &roots {
        commands.entity(e).despawn();
    }
    for ((slot, r), (_, name, count, aid)) in g.slots.iter().zip(entries) {
        tile(
            &mut commands,
            *r,
            &format!(
                "{}{} {}{}",
                if items.binding_slot() == Some(*slot) { "[BIND] " } else { "" },
                slot + 1,
                name,
                if count > 1 {
                    format!(" ({count})")
                } else {
                    String::new()
                }
            ),
            Some(if aid { "aid" } else { "quickwheel" }),
            count > 0,
        );
    }
    if let Some(r) = g.tray {
        tile(&mut commands, r, "Pins", Some("quickwheel"), true);
    }
    if !picture.message.is_empty() {
        let width = (bar.size.x * 0.6).clamp(160., 480.).min(bar.size.x - 8.);
        let top = (g.bounds.min.y - 38.).max(4.);
        tile(
            &mut commands,
            Rect::from_corners(
                Vec2::new((bar.size.x - width) * 0.5, top),
                Vec2::new((bar.size.x + width) * 0.5, top + 32.),
            ),
            &picture.message,
            None,
            true,
        );
    }
    if items.binding_slot().is_some() {
        for (index, (r, label)) in g.context.into_iter().zip(["Confirm binding", "Choose another item", "Cancel binding", "Clear slot"]).enumerate() {
            tile(&mut commands, r, label, None, index != 0 || items.binding_ready());
        }
    } else if let Some(slot) = bar.context {
        let pin = if bar.placement.pins[slot] {
            "Unpin consumable"
        } else {
            "Pin consumable"
        };
        for (r, label) in g
            .context
            .into_iter()
            .zip([pin, "Clear slot", "Move bar", "Close"])
        {
            tile(&mut commands, r, label, None, true);
        }
    }
}
fn tile(commands: &mut Commands, r: Rect, label: &str, icon: Option<&str>, enabled: bool) {
    commands
        .spawn((
            Visual,
            Node {
                position_type: PositionType::Absolute,
                left: px(r.min.x),
                top: px(r.min.y),
                width: px(r.width()),
                height: px(r.height()),
                border: UiRect::all(px(1.5)),
                border_radius: BorderRadius::all(px(8.)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.06, 0.07, 0.88)),
            BorderColor::all(Color::srgba(
                0.7,
                0.9,
                0.85,
                if enabled { 0.9 } else { 0.35 },
            )),
            GlobalZIndex(2200),
            Pickable::IGNORE,
        ))
        .with_children(|p| {
            if let Some(index) = icon.and_then(super::icons::semantic_index) {
                p.spawn((
                    ImageNode::default(),
                    super::icons::TouchIcon(index),
                    Node {
                        width: px(r.height() * 0.48),
                        height: px(r.height() * 0.48),
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
            }
            p.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(12.),
                    ..default()
                },
                TextColor(Color::WHITE),
                Pickable::IGNORE,
            ));
        });
}

#[cfg(test)]
pub(super) fn test_slot_center(world: &World, index: usize) -> Vec2 {
    let bar = world.resource::<Bar>();
    let prefs = world.resource::<Preferences>();
    geometry(bar.size, prefs, bar)
        .unwrap()
        .slots
        .into_iter()
        .find(|(i, _)| *i == index)
        .unwrap()
        .1
        .center()
}
#[cfg(test)]
pub(super) fn test_context_open(world: &World) -> bool {
    world.resource::<Bar>().context.is_some()
}
#[cfg(test)]
pub(super) fn reset_collision_fixture(world: &mut World) {
    *world.resource_mut::<Bar>() = Bar::default();
}
#[cfg(test)]
pub(super) fn tray_open_collision_fixture(world: &World) -> bool {
    world.resource::<Bar>().tray_open
}

#[cfg(test)]
pub(super) fn test_anchor(world: &World) -> Vec2 {
    let bar=world.resource::<Bar>();
    bar.active_tray.as_ref().map_or(bar.placement.center,|tray|tray.center)
}
#[cfg(test)]
pub(super) fn test_occlusion(world:&World)->(bool,Vec<Rect>) {
    let occlusion=world.resource::<Occlusion>();(occlusion.capturing,occlusion.rects.clone())
}
#[cfg(test)]
pub(super) fn test_ui_owners_retired(world: &World) -> bool {
    let bar = world.resource::<Bar>();
    !bar.moving && bar.move_id.is_none() && bar.move_original.is_none()
        && bar.context.is_none() && bar.ignored.is_empty() && !bar.gesture.busy()
        && world.get_resource::<super::Layout>().is_none_or(|layout|!layout.tray_editing())
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("hotbar/tray_layout_tests.rs");
    #[test]
    fn installing_the_custom_pointer_initializes_its_map_without_replacing_existing_entries() {
        use bevy::picking::pointer::{PointerId, PointerMap};
        for preexisting in [false, true] {
            let mut app = App::new();
            app.add_plugins(MinimalPlugins);
            let other = PointerId::Custom(bevy::asset::uuid::Uuid::from_u128(23));
            let other_entity = preexisting.then(|| {
                app.init_resource::<PointerMap>();
                app.world_mut().spawn(other).id()
            });
            install(&mut app);
            app.update();
            let map = app.world().resource::<PointerMap>();
            let entity = map.get_entity(nvf_play::pipboy::touch_items::POINTER).unwrap();
            assert_eq!(app.world().get::<PointerId>(entity), Some(&nvf_play::pipboy::touch_items::POINTER));
            assert_eq!(map.get_entity(other), other_entity);
            let world = app.world_mut();
            let mut ids = world.query::<&PointerId>();
            assert_eq!(ids.iter(world).filter(|id| **id == nvf_play::pipboy::touch_items::POINTER).count(), 1);
        }
    }
    #[test]
    fn placement_roundtrips_bounded_geometry_and_reserved_pin() {
        let mut p = Placement { center: Vec2::new(0.3, 0.7), ..Placement::default() };
        p.pins[2] = true;
        assert_eq!(Placement::decode(&p.encode()), Some(p));
        let bad = serde_json::json!({"version":1,"center":[999,-50],"pins":[true,true,false,false,false,false,false,false]});
        let clamped = Placement::decode(&bad).unwrap();
        assert_eq!(clamped.center, Vec2::new(0.95, 0.05));
        assert!(!clamped.pins[AMMO_SWAP]);
        for value in [
            serde_json::json!({"version":8}),
            serde_json::json!({"version":1,"center":[0.5,0.8],"pins":[]}),
        ] {
            assert!(Placement::decode(&value).is_none());
        }
    }
    #[test]
    fn geometry_resizes_visible_and_hit_bounds_together_without_overlapping_tray() {
        let mut bar = Bar { tray_open: true, ..Bar::default() };
        bar.placement.pins = [true; SLOTS];
        for size in [Vec2::new(1280., 720.), Vec2::new(640., 360.)] {
            for count in [4, 6, 8] {
                let small = geometry(
                    size,
                    &Preferences {
                        count,
                        scale: 0.6,
                        ..default()
                    },
                    &bar,
                )
                .unwrap();
                let large = geometry(
                    size,
                    &Preferences {
                        count,
                        scale: 1.8,
                        ..default()
                    },
                    &bar,
                )
                .unwrap();
                assert!(large.slots[0].1.width() >= small.slots[0].1.width());
                for g in [&small, &large] {
                    for (_, r) in &g.slots {
                        assert!(r.width() >= 48. && r.height() >= 48.);
                        assert!(box2(*r).contains(r.center().to_array()));
                        assert!(
                            r.min.x >= 0.
                                && r.min.y >= 0.
                                && r.max.x <= size.x
                                && r.max.y <= size.y
                        );
                    }
                    for a in 0..g.slots.len() {
                        for b in a + 1..g.slots.len() {
                            let x = g.slots[a].1.intersect(g.slots[b].1);
                            assert!(x.width() <= 0. || x.height() <= 0.);
                        }
                    }
                }
            }
        }
        for bad in [Vec2::ZERO, Vec2::splat(f32::NAN), Vec2::new(10., 100.)] {
            assert!(geometry(bad, &Preferences::default(), &bar).is_none());
        }
    }
    #[test]
    fn real_ui_nodes_use_the_same_slot_rectangles_as_touch_admission() {
        use bevy::ecs::system::RunSystemOnce;
        let forms = Box::leak(Box::new(fnv_sim::FormStore::default()));
        let refs = Box::leak(Box::new(fnv_sim::RefTable::default()));
        let mut world = World::new();
        let prefs = Preferences {
            enabled: true,
            count: 6,
            ..default()
        };
        let bar = Bar {
            mode: 1,
            size: Vec2::new(1000., 600.),
            ..default()
        };
        let expected = geometry(bar.size, &prefs, &bar).unwrap();
        world.insert_resource(prefs);
        world.insert_resource(bar);
        world.init_resource::<TouchItems>();
        world.init_resource::<Hotkeys>();
        world.insert_resource(nvf_play::sim::SimState {
            forms,
            refs,
            world: fnv_sim::World::new(forms, refs, fnv_sim::SimConfig::default()),
        });
        world.run_system_once(draw).unwrap();
        let nodes = world
            .query_filtered::<&Node, With<Visual>>()
            .iter(&world)
            .collect::<Vec<_>>();
        assert_eq!(nodes.len(), 7, "six slots plus tray");
        for (_, r) in &expected.slots {
            assert!(nodes.iter().any(|n| n.left == px(r.min.x)
                && n.top == px(r.min.y)
                && n.width == px(r.width())
                && n.height == px(r.height())));
        }
    }
    #[test]
    fn size_default_and_every_choice_roundtrip_actual_ini_without_scale_drift() {
        fn settings_app(path: &std::path::Path) -> App {
            let mut app = App::new();
            install(&mut app);
            app.insert_resource(settings::SettingsFile(Some(path.to_path_buf())));
            app
        }
        let dir = std::env::temp_dir().join(format!("nvf-hotbar-size-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("settings.ini");
        std::fs::write(
            &path,
            "[Other]
untouched=keep me
",
        )
        .unwrap();
        let mut first = settings_app(&path);
        let toggle = first
            .world()
            .resource::<settings::Settings>()
            .index("touch-hotbar")
            .unwrap();
        assert_eq!(settings::flip(first.world_mut(), toggle), Some(true));
        let mut loaded = settings_app(&path);
        settings::apply_stored(loaded.world_mut());
        assert_eq!(
            loaded.world().resource::<Preferences>().scale,
            1.0,
            "default100% survives an unrelated preference write"
        );
        for (index, expected) in [
            0.6_f32, 0.7, 0.8, 0.9, 1.0, 1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 1.7, 1.8,
        ]
        .into_iter()
        .enumerate()
        {
            let row = first
                .world()
                .resource::<settings::Settings>()
                .index("touch-hotbar-size")
                .unwrap();
            assert!(
                settings::set_slider(first.world_mut(), row, settings::choice_part(index, 13))
                    .is_some()
            );
            assert_eq!(first.world().resource::<Preferences>().scale, expected);
            let mut back = settings_app(&path);
            settings::apply_stored(back.world_mut());
            assert_eq!(
                back.world().resource::<Preferences>().scale,
                expected,
                "choice{index} drifted through INI rounding"
            );
            let row = back
                .world()
                .resource::<settings::Settings>()
                .index("touch-hotbar-size")
                .unwrap();
            let slider = back.world().resource::<settings::Settings>().list()[row]
                .slider
                .unwrap();
            assert_eq!(
                slider.text((slider.get)(back.world())),
                format!("{:.0}%", expected * 100.0)
            );
            let text = std::fs::read_to_string(&path).unwrap();
            assert_eq!(
                fnv_install::ini_get(&text, "Other", "untouched").as_deref(),
                Some("keep me")
            );
        }
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
    #[derive(Resource, Default)]
    struct ItemCaptureReceipt {
        admitted: bool,
        projected: bool,
        canceled: bool,
    }
    fn capture_fixture_route(mut bridge: Bridge, mut touches: MessageReader<TouchInput>) {
        bridge.prepare(
            Vec2::new(1000., 1000.),
            true,
            true,
            &super::super::layout::Config::default(),
            false,
            &[],
        );
        for event in touches.read() {
            bridge.event(event, true);
        }
    }
    fn capture_fixture_pointer(
        admission: nvf_play::pipboy::touch_items::Admission,
        mut items: ResMut<TouchItems>,
        mut receipt: ResMut<ItemCaptureReceipt>,
    ) {
        receipt.admitted = admission.allows();
        receipt.canceled = items.take_cancel();
        items.project(Some);
        receipt.projected = items.take_projected().is_some();
    }
    fn item_capture_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::input::InputPlugin));
        app.insert_resource(nvf_play::bindings::Bindings::default());
        let forms = Box::leak(Box::new(fnv_sim::FormStore::default()));
        let refs = Box::leak(Box::new(fnv_sim::RefTable::default()));
        app.insert_resource(nvf_play::sim::SimState {
            forms,
            refs,
            world: fnv_sim::World::new(forms, refs, fnv_sim::SimConfig::default()),
        });
        app.init_resource::<Hotkeys>()
            .init_resource::<ItemCaptureReceipt>();
        install(&mut app);
        app.add_systems(PreUpdate, capture_fixture_route.in_set(super::super::TouchInputSystems).before(bevy::input::InputSystems))
            .add_systems(PreUpdate, capture_fixture_pointer.in_set(nvf_play::pipboy::TouchPointerSet).after(bevy::input::InputSystems));
        app.world_mut().resource_mut::<Preferences>().enabled = true;
        app.update();
        app
    }
    fn item_event(id: u64, phase: TouchPhase, point: Vec2) -> TouchInput {
        TouchInput {
            id,
            phase,
            position: point,
            window: Entity::PLACEHOLDER,
            force: None,
        }
    }
    #[test]
    fn actual_tray_request_and_idle_hold_cancel_already_projected_item_before_picking() {
        for hold in [false, true] {
            let mut app = item_capture_app();
            if hold {
                let slot = test_slot_center(app.world(), 0);
                app.world_mut()
                    .write_message(item_event(91, TouchPhase::Started, slot));
                app.update();
            }
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyW);
            {
                let mut items = app.world_mut().resource_mut::<TouchItems>();
                assert!(items.event(&item_event(90, TouchPhase::Started, Vec2::splat(200.)), 0));
                items.release_over(Some(0));
                items.project(Some); // Real projected press existed before overlay admission.
            }
            if hold {
                app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                    std::time::Duration::from_millis(500),
                ));
            } else {
                app.world_mut().write_message(UtilityTrayRequest(true));
            }
            app.update();
            assert!(app.world().resource::<Bar>().captures_actions());
            let receipt = app.world().resource::<ItemCaptureReceipt>();
            assert!(
                !receipt.admitted && !receipt.projected && receipt.canceled,
                "hold={hold}: old press survived actual capture before picking"
            );
            assert!(!app.world().resource::<TouchItems>().owns(90));
            assert!(app
                .world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyW));
            assert!(app
                .world()
                .resource::<Hotkeys>()
                .0
                .iter()
                .all(Option::is_none));
            // Old release cannot re-enter the item route during the captured UI.
            app.world_mut()
                .write_message(item_event(90, TouchPhase::Ended, Vec2::splat(200.)));
            app.update();
            assert!(!app.world().resource::<ItemCaptureReceipt>().projected);
            assert!(!app.world().resource::<TouchItems>().busy());
        }
    }
    #[test]
    fn disabled_bar_rejects_tray_request_without_hidden_pointer_capture() {
        let mut app = item_capture_app();
        app.world_mut().resource_mut::<Preferences>().enabled = false;
        app.world_mut().write_message(UtilityTrayRequest(true));
        app.update();
        assert!(!app.world().resource::<Bar>().tray_open);
        assert!(!app.world().resource::<OverlayCapture>().0);
        assert!(app.world().resource::<ItemCaptureReceipt>().admitted);
    }
}


#[cfg(test)]
mod binding_release_tests {
    use super::*;
    use bevy::input::{InputPlugin,mouse::MouseButtonInput,ButtonState};
    use bevy::state::app::StatesPlugin;
    use bevy::window::{PrimaryWindow,WindowResolution};

    // Full wrapper raw reader -> Bridge -> actual Gesture -> dispatch -> assign.
    // There is no second test raw reader, injected Outcome or fake hardware hold.
    fn app()->(App,Entity) {
        let mut app=App::new();
        app.add_plugins((MinimalPlugins,InputPlugin,StatesPlugin))
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_millis(100)))
            .insert_state(nvf_play::PlayState::Menu)
            .init_resource::<nvf_play::bindings::Bindings>()
            .init_resource::<Hotkeys>()
            .init_resource::<nvf_play::pipboy::PipBoyAid>()
            .init_resource::<nvf_play::pipboy::PointerMoved>()
            .add_message::<bevy::window::WindowFocused>()
            .add_message::<bevy::window::AppLifecycle>();
        let mut forms=fnv_sim::FormStore::default();
        let player=fnv_sim::FormId::PLAYER_BASE;
        forms.bases.insert(player,fnv_sim::forms::BaseForm{id:player,sig:*b"NPC_",edid:None,
            name:Some("Owned binding fixture".into()),script:None,prompt:None,voice_filter:false,
            data:fnv_sim::forms::BaseData::Actor(fnv_sim::forms::ActorBase{entries:vec![],attributes:None})});
        let item=fnv_sim::FormId(0xF203);
        forms.bases.insert(item,fnv_sim::forms::BaseForm{id:item,sig:*b"ALCH",edid:None,name:Some("Owned quantity stack".into()),
            script:None,prompt:None,voice_filter:false,data:fnv_sim::forms::BaseData::Item(fnv_sim::forms::Item{
                kind:fnv_sim::forms::ItemKind::Aid,value:None,money:None,weight:None,slots:None,projectile:None,
                health:None,non_playable:false,cant_drop:false,armor_class:default(),power_armor:false,
                armor_dr:None,armor_dt:None,weapon_damage:None,weight_mods:[0.;3],drawable:false,not_for_npcs:false,
                thrown:false,quest_item:false,icon:None,placed_count:None,teaches:None,cant_be_taken:false,
                modulates_voice:false,weapon_traits:None})});
        let forms=Box::leak(Box::new(forms));let refs=Box::leak(Box::new(fnv_sim::RefTable::default()));
        let mut sim=nvf_play::sim::SimState{forms,refs,world:fnv_sim::World::new(forms,refs,fnv_sim::SimConfig::default())};
        sim.world.add_item(fnv_sim::FormId::PLAYER_REF,item,10).unwrap();
        let model=nvf_play::menu::model::MenuModel::open_inventory(&mut sim.world).unwrap();
        app.insert_resource(sim).insert_resource(ActiveMenu(Some(model)));
        let mut pip=PipBoy::default();pip.phase=Phase::Up;pip.page=Page::Items;
        app.insert_resource(pip);
        let camera=app.world_mut().spawn_empty().id();
        app.insert_resource(nvf_play::pipboy::PipBoyScreen{image:Handle::default(),camera,on_model:false,pointer:None,scale:1.});
        let window=app.world_mut().spawn((Window{resolution:WindowResolution::new(1000,600),..default()},PrimaryWindow)).id();
        app.add_plugins((super::super::TouchControlsPlugin,nvf_play::pipboy::touch_scroll::TouchPointerPlugin));
        app.insert_resource(settings::SettingsFile(None));
        app.update();
        *app.world_mut().resource_mut::<Bar>()=Bar::default();
        app.world_mut().resource_mut::<Preferences>().enabled=true;
        app.update();
        assert_eq!(app.world().resource::<Bar>().mode,2);
        (app,window)
    }
    fn edge(app:&mut App,window:Entity,id:u64,phase:TouchPhase,p:Vec2) {
        app.world_mut().write_message(TouchInput{window,id,phase,position:p,force:None});
    }
    fn idle(app:&mut App,frames:usize){for _ in 0..frames{app.update();}}
    fn assert_unarmed(app:&App){
        assert_eq!(app.world().resource::<TouchItems>().binding_slot(),None);
        assert!(app.world().resource::<Hotkeys>().0.iter().all(Option::is_none));
    }
    fn fresh_tap(app:&mut App,window:Entity,id:u64){
        let p=test_slot_center(app.world(),0);
        edge(app,window,id,TouchPhase::Started,p);app.update();
        edge(app,window,id,TouchPhase::Ended,p);app.update();
        assert_unarmed(app);
        idle(app,4);
        assert_eq!(app.world().resource::<TouchItems>().binding_slot(),Some(0),"a genuinely released fresh slot tap still arms");
        assert!(app.world().resource::<Hotkeys>().0.iter().all(Option::is_none));
    }
    #[test]
    fn actual_raw_inventory_slot_hold_waits_for_original_valid_release_and_post_release_drain(){
        let (mut app,w)=app();let p=test_slot_center(app.world(),0);
        edge(&mut app,w,610,TouchPhase::Started,p);app.update();
        for _ in 0..10{
            app.update();assert_unarmed(&app);
            if app.world().resource::<Bar>().pending_binding.is_some(){
                assert!(app.world().resource::<Bar>().captures_actions(),"pending held owner must also exclude underlying item use");
                assert!(app.world().resource::<OverlayCapture>().0);
            }
        }
        assert_eq!(app.world().resource::<Bar>().pending_binding,Some(0));
        // An actual second raw contact cannot start the underlying item pointer
        // while the original slot is still held, even after many drain frames.
        let size=Vec2::new(1000.,600.);
        let background={let layout=&app.world().resource::<super::super::layout::Layout>().config;
            let bar=app.world().resource::<Bar>();let prefs=app.world().resource::<Preferences>();
            let g=geometry(size,prefs,bar).unwrap();
            (100..900).step_by(40).flat_map(|x|(100..500).step_by(40).map(move|y|Vec2::new(x as f32,y as f32)))
                .find(|point|!g.slots.iter().any(|(_,r)|r.contains(*point))&&!g.tray.is_some_and(|r|r.contains(*point))
                    &&!g.context.iter().any(|r|r.contains(*point))
                    &&layout.hit_filtered(*point,size,false,false,|_|true).is_none()
                    &&![false,true].iter().any(|help|super::super::layout::utility_rect(*help,size).contains(*point))).unwrap()};
        edge(&mut app,w,611,TouchPhase::Started,background);app.update();
        edge(&mut app,w,611,TouchPhase::Ended,background);app.update();
        assert_eq!(app.world().resource::<TouchItems>().contact_id(),None);
        let touch_actions=app.world_mut().resource_mut::<bevy::ecs::message::Messages<bevy::picking::pointer::PointerInput>>()
            .drain().filter(|event|event.pointer_id==nvf_play::pipboy::touch_items::POINTER
                &&matches!(event.action,bevy::picking::pointer::PointerAction::Press(_)|bevy::picking::pointer::PointerAction::Release(_))).count();
        assert_eq!(touch_actions,0,"no native/handbuilt item click can be produced beneath the captured hold");
        edge(&mut app,w,610,TouchPhase::Ended,p);app.update();assert_unarmed(&app);
        assert!(!app.world().resource::<Bar>().gesture.busy());
        idle(&mut app,4);
        assert_eq!(app.world().resource::<TouchItems>().binding_slot(),Some(0));
        assert!(app.world().resource::<Hotkeys>().0.iter().all(Option::is_none));
        assert!(app.world().resource::<bevy::ecs::message::Messages<nvf_play::weapon_wheel::TouchHotkey>>().is_empty());
    }
    #[test]
    fn actual_raw_pending_slot_cancellation_drag_resize_and_tick_cancel_never_replay_binding(){
        for reason in ["cancel","drag","fit","tick_cancel"] {
            let (mut app,w)=app();let p=test_slot_center(app.world(),0);
            edge(&mut app,w,620,TouchPhase::Started,p);app.update();
            idle(&mut app,if reason=="tick_cancel"{4}else{6});
            assert_unarmed(&app);
            if reason!="tick_cancel"{assert_eq!(app.world().resource::<Bar>().pending_binding,Some(0));}
            match reason {
                "cancel"|"tick_cancel"=>edge(&mut app,w,620,TouchPhase::Canceled,p),
                "drag"=>{
                    edge(&mut app,w,620,TouchPhase::Moved,p+Vec2::new(30.,0.));
                    edge(&mut app,w,620,TouchPhase::Moved,p);
                    edge(&mut app,w,620,TouchPhase::Ended,p);
                },
                _=>{app.world_mut().resource_mut::<Preferences>().scale=1.1;edge(&mut app,w,620,TouchPhase::Ended,p);},
            }
            app.update();idle(&mut app,5);
            assert_unarmed(&app);
            assert!(app.world().resource::<Bar>().pending_binding.is_none(),"{reason}: pending request retired");
            assert!(app.world().resource::<Bar>().binding_origin.is_none(),"{reason}: no stale origin");
            assert!(app.world().resource::<Bar>().outcomes.is_empty());
            fresh_tap(&mut app,w,621);
        }
    }
    #[test]
    fn actual_mouse_intent_cancels_pending_binding_without_clearing_physical_button(){
        let (mut app,w)=app();let p=test_slot_center(app.world(),0);
        edge(&mut app,w,630,TouchPhase::Started,p);app.update();idle(&mut app,6);
        assert_eq!(app.world().resource::<Bar>().pending_binding,Some(0));
        app.world_mut().write_message(MouseButtonInput{window:w,button:MouseButton::Middle,state:ButtonState::Pressed});
        app.update();
        assert_unarmed(&app);
        assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Middle));
        edge(&mut app,w,630,TouchPhase::Ended,p);app.update();idle(&mut app,4);
        assert_unarmed(&app);assert!(app.world().resource::<Bar>().pending_binding.is_none());
        app.world_mut().write_message(MouseButtonInput{window:w,button:MouseButton::Middle,state:ButtonState::Released});
        idle(&mut app,3);
        fresh_tap(&mut app,w,631);
    }
    #[test]
    fn actual_raw_stats_and_data_slots_refuse_binding_with_items_feedback_then_fresh_items_tap_works(){
        for page in [Page::Stats,Page::Data] {
            for held in [false,true] {
                let (mut app,w)=app();app.world_mut().resource_mut::<PipBoy>().page=page;app.update();
                let p=test_slot_center(app.world(),0);
                edge(&mut app,w,640,TouchPhase::Started,p);app.update();
                if held {idle(&mut app,7);}
                edge(&mut app,w,640,TouchPhase::Ended,p);app.update();idle(&mut app,4);
                assert_unarmed(&app);
                assert!(app.world().resource::<Bar>().pending_binding.is_none());
                assert!(app.world().resource::<Bar>().binding_origin.is_none());
                assert!(app.world().resource::<TouchItems>().message.contains("Open ITEMS"));
                app.world_mut().resource_mut::<PipBoy>().page=Page::Items;app.update();
                fresh_tap(&mut app,w,641);
            }
        }
    }
    fn quantity(active:&mut ActiveMenu,sim:&mut nvf_play::sim::SimState) {
        let model=active.0.as_mut().unwrap();
        let result=model.apply(&mut sim.world,nvf_play::menu::model::Command::Drop);
        assert!(result.used.is_none());
        assert!(model.quantity.is_some(),"the real native stack-drop rule must open the prompt");
    }
    fn open_quantity(app:&mut App) {
        app.world_mut().resource_scope(|world,mut active:Mut<ActiveMenu>| {
            let mut sim=world.resource_mut::<nvf_play::sim::SimState>();quantity(&mut active,&mut sim);
        });
    }
    #[derive(Resource)]struct LateQuantity(bool);
    fn late_quantity(mut flag:ResMut<LateQuantity>,mut active:ResMut<ActiveMenu>,mut sim:ResMut<nvf_play::sim::SimState>){
        if std::mem::take(&mut flag.0){quantity(&mut active,&mut sim);}
    }
    #[derive(Resource,Default)]
    struct PromptPointerReleases(usize);
    fn record_prompt_pointer_releases(
        mut events:MessageReader<bevy::picking::pointer::PointerInput>,
        mut seen:ResMut<PromptPointerReleases>,
    ) {
        seen.0+=events.read().filter(|event|event.pointer_id==nvf_play::pipboy::touch_items::POINTER
            &&matches!(event.action,bevy::picking::pointer::PointerAction::Release(_))).count();
    }
    fn install_flat_prompt_projection(app:&mut App) {
        use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
        use bevy::asset::{AssetApp, AssetPlugin};
        use bevy::camera::{ComputedCameraValues, RenderTargetInfo, visibility::VisibilityPlugin};
        use bevy::ui::{ComputedUiRenderTargetInfo, ui_layout_system, ui_surface::UiSurface, update::propagate_ui_target_cameras};
        app.add_plugins((AssetPlugin::default(),bevy::text::TextPlugin,bevy::transform::TransformPlugin))
            .init_asset::<Mesh>().init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>()
            .add_plugins(VisibilityPlugin)
            .add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate))
            .add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate))
            .init_resource::<UiSurface>().init_resource::<UiScale>()
            .add_systems(PostUpdate,(propagate_ui_target_cameras,ui_layout_system).chain())
            .configure_sets(PostUpdate,PropagateSet::<ComputedUiTargetCamera>::default().after(propagate_ui_target_cameras).before(ui_layout_system))
            .configure_sets(PostUpdate,PropagateSet::<ComputedUiRenderTargetInfo>::default().after(propagate_ui_target_cameras).before(ui_layout_system));
        let camera=app.world().resource::<nvf_play::pipboy::PipBoyScreen>().camera;
        app.world_mut().entity_mut(camera).insert((Camera2d,Camera {computed:ComputedCameraValues {
            target_info:Some(RenderTargetInfo {physical_size:UVec2::new(1000,600),scale_factor:1.0}),..default()},..default()}));
        app.init_resource::<PromptPointerReleases>()
            .add_systems(PreUpdate,record_prompt_pointer_releases.after(nvf_play::pipboy::TouchPointerSet))
            .add_systems(Update,nvf_play::pipboy::flat_screen).add_systems(Last,nvf_play::pipboy::flat_pointer);
        idle(app,4);
    }
    fn free_menu_point(app:&App)->Vec2 {
        let size=Vec2::new(1000.,600.);let config=&app.world().resource::<super::super::layout::Layout>().config;
        let bar=app.world().resource::<Bar>();let prefs=app.world().resource::<Preferences>();let g=geometry(size,prefs,bar).unwrap();
        (100..900).step_by(40).flat_map(|x|(100..500).step_by(40).map(move|y|Vec2::new(x as f32,y as f32)))
            .find(|point|!g.slots.iter().any(|(_,r)|r.contains(*point))&&!g.tray.is_some_and(|r|r.contains(*point))
                &&!g.context.iter().any(|r|r.contains(*point))&&config.hit_filtered(*point,size,false,false,|_|true).is_none()
                &&![false,true].iter().any(|help|super::super::layout::utility_rect(*help,size).contains(*point))).unwrap()
    }
    fn free_prompt_point(app:&mut App)->Vec2 {
        let bounds={
            let world=app.world_mut();
            let mut nodes=world.query_filtered::<(&ComputedNode,&UiGlobalTransform),With<nvf_play::pipboy::FlatScreen>>();
            let (node,at)=nodes.single(world).unwrap();
            assert!(node.size().is_finite() && node.size().min_element()>0.0);
            Rect::from_center_size(at.translation,node.size())
        };
        let size=Vec2::new(1000.,600.);let config=&app.world().resource::<super::super::layout::Layout>().config;
        let bar=app.world().resource::<Bar>();let prefs=app.world().resource::<Preferences>();let g=geometry(size,prefs,bar).unwrap();
        (100..900).step_by(40).flat_map(|x|(100..500).step_by(40).map(move|y|Vec2::new(x as f32,y as f32)))
            .find(|point|bounds.contains(*point)&&!g.slots.iter().any(|(_,r)|r.contains(*point))&&!g.tray.is_some_and(|r|r.contains(*point))
                &&!g.context.iter().any(|r|r.contains(*point))&&config.hit_filtered(*point,size,false,false,|_|true).is_none()
                &&![false,true].iter().any(|help|super::super::layout::utility_rect(*help,size).contains(*point))).unwrap()
    }
    #[test]
    fn actual_quantity_interrupts_active_pending_and_late_binding_but_fresh_raw_prompt_pointer_survives(){
        for state in ["active","pending","late"] {
            let (mut app,w)=app();let p=test_slot_center(app.world(),0);
            // Use actual flat projection and the real PreUpdate pointer driver.
            install_flat_prompt_projection(&mut app);
            app.world_mut().write_message(bevy::input::keyboard::KeyboardInput{window:w,key_code:KeyCode::F12,
                logical_key:bevy::input::keyboard::Key::F12,state:ButtonState::Pressed,text:None,repeat:false});
            app.update();
            let release_baseline=app.world().resource::<PromptPointerReleases>().0;
            if state=="active" {fresh_tap(&mut app,w,650);open_quantity(&mut app);}
            else {
                edge(&mut app,w,650,TouchPhase::Started,p);app.update();
                if state=="pending" {idle(&mut app,6);assert_eq!(app.world().resource::<Bar>().pending_binding,Some(0));open_quantity(&mut app);}
                else {edge(&mut app,w,650,TouchPhase::Ended,p);
                    app.insert_resource(LateQuantity(true)).add_systems(Update,late_quantity.before(dispatch));}
            }
            app.update();idle(&mut app,3);
            assert_unarmed(&app);
            assert!(app.world().resource::<Bar>().pending_binding.is_none());
            assert!(app.world().resource::<Bar>().binding_origin.is_none());
            assert!(app.world().resource::<ActiveMenu>().0.as_ref().unwrap().quantity.is_some());
            assert!(!app.world().resource::<OverlayCapture>().0,"quantity must not become a permanent overlay capture");
            assert!(app.world().resource::<Bar>().menu_touch.is_some(),"ordinary prompt ingress stays admitted");
            edge(&mut app,w,650,TouchPhase::Ended,p);app.update();idle(&mut app,3);
            let old_releases=app.world().resource::<PromptPointerReleases>().0-release_baseline;
            assert_eq!(old_releases,0,"interrupted binding cannot release into the prompt");
            let fresh_baseline=app.world().resource::<PromptPointerReleases>().0;
            let point=free_prompt_point(&mut app);
            edge(&mut app,w,651,TouchPhase::Started,point);app.update();
            assert!(app.world().resource::<TouchItems>().owns(651));
            app.update();edge(&mut app,w,651,TouchPhase::Ended,point);app.update();idle(&mut app,3);
            let releases=app.world().resource::<PromptPointerReleases>().0-fresh_baseline;
            assert_eq!(releases,1,"fresh ordinary prompt contact reaches the actual projected pointer boundary");
            assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12));
            assert_eq!(app.world_mut().resource_mut::<nvf_play::sim::SimState>().world.item_count(fnv_sim::FormId::PLAYER_REF,fnv_sim::FormId(0xF203)).unwrap(),10);
            assert_unarmed(&app);
        }
    }

    #[test]
    fn actual_mode2_queued_tray_open_retires_held_released_and_active_binding_without_replay_or_lost_pins(){
        for state in ["held","released","active"] {
            let (mut app,w)=app();
            app.world_mut().write_message(bevy::input::keyboard::KeyboardInput{window:w,key_code:KeyCode::F12,
                logical_key:bevy::input::keyboard::Key::F12,state:ButtonState::Pressed,text:None,repeat:false});
            app.update();
            let assigned=nvf_play::hotkeys::Slot{item:0xF203,mods:0};
            app.world_mut().resource_mut::<Hotkeys>().assign(0,assigned,Some(fnv_sim::forms::ItemKind::Aid)).unwrap();
            app.world_mut().resource_mut::<Bar>().placement.pins[0]=true;
            let saved_hotkeys=app.world().resource::<Hotkeys>().0;
            let saved_pins=app.world().resource::<Bar>().placement.pins;
            let p=test_slot_center(app.world(),0);
            edge(&mut app,w,810,TouchPhase::Started,p);app.update();
            if state=="held" {idle(&mut app,6);assert_eq!(app.world().resource::<Bar>().pending_binding,Some(0));}
            else {
                edge(&mut app,w,810,TouchPhase::Ended,p);app.update();
                if state=="active" {idle(&mut app,4);assert_eq!(app.world().resource::<TouchItems>().binding_slot(),Some(0));}
                else {assert_eq!(app.world().resource::<Bar>().pending_binding,Some(0));}
            }
            // The real request reader must retire binding before this old raw
            // release reaches the opening batch, not after Update dispatch.
            app.world_mut().write_message(UtilityTrayRequest(true));
            edge(&mut app,w,810,TouchPhase::Ended,p);app.update();
            assert!(app.world().resource::<Bar>().tray_open);
            assert!(app.world().resource::<Bar>().pending_binding.is_none());
            assert!(app.world().resource::<Bar>().binding_origin.is_none());
            assert!(app.world().resource::<Bar>().binding_slot.is_none());
            assert_eq!(app.world().resource::<TouchItems>().binding_slot(),None);
            assert!(app.world().resource::<OverlayCapture>().0);
            assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12));
            assert_eq!(app.world().resource::<Hotkeys>().0,saved_hotkeys);
            assert_eq!(app.world().resource::<Bar>().placement.pins,saved_pins);
            let outside=free_menu_point(&app);
            edge(&mut app,w,811,TouchPhase::Started,outside);app.update();
            assert!(!app.world().resource::<Bar>().tray_open);
            edge(&mut app,w,811,TouchPhase::Ended,outside);app.update();
            app.update(); // the known post-close drain reaches neutral here
            assert!(!app.world().resource::<OverlayCapture>().0);
            assert!(!app.world().resource::<super::super::TouchState>().ui_capture);
            assert!(app.world().resource::<Bar>().pending_binding.is_none());
            assert!(app.world().resource::<Bar>().binding_origin.is_none());
            assert_eq!(app.world().resource::<TouchItems>().binding_slot(),None);
            idle(&mut app,2);
            assert_eq!(app.world().resource::<TouchItems>().binding_slot(),None,"{state}: dismissal cannot revive a canceled binding");
            let fresh=test_slot_center(app.world(),0);
            edge(&mut app,w,812,TouchPhase::Started,fresh);app.update();
            edge(&mut app,w,812,TouchPhase::Ended,fresh);app.update();idle(&mut app,4);
            assert_eq!(app.world().resource::<TouchItems>().binding_slot(),Some(0),"fresh slot controls remain usable");
            assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12));
            assert_eq!(app.world().resource::<Hotkeys>().0,saved_hotkeys);
            assert_eq!(app.world().resource::<Bar>().placement.pins,saved_pins);
            assert!(app.world().resource::<bevy::ecs::message::Messages<nvf_play::weapon_wheel::TouchHotkey>>().is_empty());
            assert_eq!(app.world_mut().resource_mut::<nvf_play::sim::SimState>().world.item_count(fnv_sim::FormId::PLAYER_REF,fnv_sim::FormId(0xF203)).unwrap(),10);
        }
    }

    #[test]
    fn queued_tray_requests_crossing_actual_menu_sessions_are_retired_then_fresh_requests_work() {
        for cause in ["page","tab","frame"] {
            let (mut app,_)=app();
            app.world_mut().write_message(UtilityTrayRequest(true));
            {
                let mut pip=app.world_mut().resource_mut::<PipBoy>();
                match cause {
                    "page"=>pip.page=Page::Stats,
                    "tab"=>pip.tabs[Page::Items.index()]=1,
                    _=>pip.frame=nvf_play::pipboy::Frame::Screen,
                }
            }
            app.update();
            assert!(!app.world().resource::<Bar>().tray_open,"{cause}: old session request cannot cross the change");
            assert_unarmed(&app);
            app.world_mut().write_message(UtilityTrayRequest(true));
            app.update();
            assert!(app.world().resource::<Bar>().tray_open,"{cause}: fresh stable-session request still opens");
            assert_unarmed(&app);
        }
    }

}
