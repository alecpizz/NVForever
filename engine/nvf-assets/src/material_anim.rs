//! Sequences that drive a shape's material: its glow (emissive colour, emissive
//! multiplier), its base texture's offset and its alpha (docs/spec-notes/activators.md
//! §2, §6). The jukeboxes' Idle flickers their lights and scrolls the music notes and
//! bubbles; the radios' Open and Close light and darken their dials; the neon signs' and
//! lit windows' Left and Right fade them in and out.
//!
//! A rig whose clips have such tracks carries [`RigMaterials`] on its root: the shapes
//! they drive, each given its own copy of its material when first bound
//! ([`bind_rig_materials`]), so one reference's look does not change every copy of the
//! model. The idle clip's tracks follow the rig's [`Playing`] clock
//! ([`animate_rig_materials`]); other clips are applied by whoever poses them
//! ([`RigMaterials::apply`]).
//!
//! Values [hypothesis, nif.md §6 "Emissive light"]: the drawn emissive is colour ×
//! multiplier, as for a stored material; a track replaces the member it drives, the
//! others keep their stored value. A texture offset track moves the texture by its value
//! (`uv + offset`) [hypothesis: the direction is not documented; the textures tile, so
//! only the direction of the scroll depends on it]. An alpha track replaces the
//! material's alpha [hypothesis]; it shows only on a shape drawn blended or tested (one
//! with an `NiAlphaProperty`). Scale and rotation tracks scale and turn the texture about
//! its centre (0.5, 0.5), then the offset moves it: `uv' = c + R·S·(uv − c) + offset`
//! [hypothesis: nif.xml gives TexDesc a Center and a Transform Method but not the
//! formula; with scale 1 and no rotation it is the offset alone].
//!
//! Actors: the clips an actor's [`BlendPlayer`] plays drive the materials of its parts'
//! shapes by name too (the Securitron screen's flicker, roll and static,
//! docs/spec-notes/effects-animation-audit.md §2): [`actor_material_values`], posed
//! by nvforever's `actor_materials.rs`.

use crate::anim_blend::Animator;
use crate::anim_play::{cycle_time, BlendPlayer, MaterialTarget, MaterialTrack, Playing};
use crate::nif_material::{display_emissive, emissive_in_display, plain_material, MaterialOverride, NifMaterial};
use bevy::ecs::system::SystemParam;
use bevy::math::Affine2;
use bevy::prelude::*;
use std::sync::Arc;

/// One shape of a rig whose material a clip drives.
#[derive(Debug, Clone)]
pub struct MaterialSlot {
    /// The shape's name in the model (what tracks name).
    pub shape: String,
    pub entity: Entity,
    /// The model's shared material until bound, then this reference's own copy.
    pub handle: Handle<NifMaterial>,
    /// Whether `handle` is this reference's own copy (only then is it changed).
    pub own: bool,
    /// Stored emissive colour and multiplier.
    pub emissive: [f32; 3],
    pub emissive_mult: f32,
    /// Stored material alpha.
    pub alpha: f32,
    /// Stored refraction strength (0 unless the shape refracts).
    pub refraction: f32,
    /// What was last written, before the rig's fade, and the fade it was written with.
    last: Option<Look>,
    faded: f32,
    /// An unlit shape's stored base colour (read the first time the slot is posed), and
    /// whether it was last drawn black for a dark idle ([`RigMaterials::apply`]).
    unlit_base: Option<LinearRgba>,
    unlit_dark: bool,
    /// [design, #2685] Members previously driven, for restoring a replaced clip.
    driven: Driven,
}

impl MaterialSlot {
    pub fn new(shape: String, entity: Entity, handle: Handle<NifMaterial>, emissive: [f32; 3], emissive_mult: f32, alpha: f32) -> MaterialSlot {
        MaterialSlot { shape, entity, handle, own: false, emissive, emissive_mult, alpha, refraction: 0.0, last: None, faded: 1.0, unlit_base: None, unlit_dark: false, driven: Driven::default() }
    }
}

/// What the tracks give one shape's material.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub colour: [f32; 3],
    pub mult: f32,
    /// Base texture offset, scale and rotation (radians).
    pub uv: [f32; 2],
    pub scale: [f32; 2],
    pub rotate: f32,
    pub alpha: f32,
    /// Refraction strength (`nif_material`, "Refraction").
    pub refraction: f32,
}

impl Look {
    /// A material's stored look: its emissive colour and multiplier and its alpha, the
    /// texture where the model puts it.
    pub fn stored(colour: [f32; 3], mult: f32, alpha: f32) -> Look {
        Look { colour, mult, uv: [0.0; 2], scale: [1.0; 2], rotate: 0.0, alpha, refraction: 0.0 }
    }

    /// Take the value of a track of `target` (`v`: its sample) into the look.
    pub fn take(&mut self, target: MaterialTarget, v: [f32; 3]) {
        match target {
            MaterialTarget::SelfIllum => self.colour = v,
            MaterialTarget::EmissiveMult => self.mult = v[0].max(0.0),
            MaterialTarget::UvOffset(axis) => self.uv[axis] = v[0],
            MaterialTarget::UvScale(axis) => self.scale[axis] = v[0],
            MaterialTarget::UvRotate => self.rotate = v[0],
            MaterialTarget::Alpha => self.alpha = v[0].clamp(0.0, 1.0),
            MaterialTarget::Refraction => self.refraction = v[0].max(0.0),
        }
    }

    /// The base texture's transform (module notes).
    pub fn uv_transform(&self) -> Affine2 {
        let c = Vec2::splat(0.5);
        Affine2::from_translation(c + Vec2::from_array(self.uv))
            * Affine2::from_angle(self.rotate)
            * Affine2::from_scale(Vec2::from_array(self.scale))
            * Affine2::from_translation(-c)
    }
}

/// On a drawn shape of an actor's part (with its `ShapeName`): the look its model
/// stores, which a member no clip drives goes back to.
#[derive(Component, Debug, Clone, Copy)]
pub struct StoredLook(pub Look);

/// The members of a material that tracks drive.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Driven {
    pub emissive: bool,
    pub texture: bool,
    pub alpha: bool,
    pub refraction: bool,
}

impl Driven {
    pub const ALL: Driven = Driven { emissive: true, texture: true, alpha: true, refraction: true };

    pub fn of(target: MaterialTarget) -> Driven {
        match target {
            MaterialTarget::SelfIllum | MaterialTarget::EmissiveMult => Driven { emissive: true, ..default() },
            MaterialTarget::UvOffset(_) | MaterialTarget::UvScale(_) | MaterialTarget::UvRotate => Driven { texture: true, ..default() },
            MaterialTarget::Alpha => Driven { alpha: true, ..default() },
            MaterialTarget::Refraction => Driven { refraction: true, ..default() },
        }
    }

    pub fn or(self, o: Driven) -> Driven {
        Driven { emissive: self.emissive || o.emissive, texture: self.texture || o.texture, alpha: self.alpha || o.alpha, refraction: self.refraction || o.refraction }
    }

    pub fn any(self) -> bool {
        self.emissive || self.texture || self.alpha || self.refraction
    }
}

/// Write the `driven` members of `look` into `m`. With `unlit_glow`, an unlit shape's
/// emissive light is its colour: drawn colour = texture × emissive colour × multiplier
/// [hypothesis, effects-animation-audit.md §2: the Securitron idle's screen glow keys
/// average 0.032 × multiplier 30 ≈ 1 and its screen-change flash reaches 0.99 × 30,
/// while an unlit shape here otherwise draws its texture as it is, so its emissive light
/// would not show]. An additive or multiply-blended shape forms its light in display
/// values, so the value is raised to the display gamma (nif_material
/// `display_emissive`).
pub fn pose_material(m: &mut NifMaterial, look: &Look, driven: Driven, unlit_glow: bool) {
    let Look { colour, mult, alpha, .. } = *look;
    if driven.emissive {
        let value = |c: f32| if emissive_in_display(m.base.alpha_mode) { display_emissive(c * mult) } else { c * mult };
        if unlit_glow && m.base.unlit && !matches!(m.base.alpha_mode, AlphaMode::Multiply) {
            let a = m.base.base_color.alpha();
            m.base.base_color = Color::LinearRgba(LinearRgba::new(value(colour[0]), value(colour[1]), value(colour[2]), a));
        } else {
            m.base.emissive = LinearRgba::rgb(value(colour[0]), value(colour[1]), value(colour[2]));
        }
    }
    if driven.texture {
        m.base.uv_transform = look.uv_transform();
    }
    if driven.alpha {
        m.base.base_color.set_alpha(alpha);
    }
    if driven.refraction && m.extension.refract {
        m.extension.refraction = look.refraction;
    }
}

/// On the root of a rig whose clips drive materials.
#[derive(Component, Debug, Clone)]
pub struct RigMaterials {
    pub slots: Vec<MaterialSlot>,
    /// The idle clip's material tracks, following the rig's [`Playing`].
    pub idle: Arc<Vec<MaterialTrack>>,
    /// Draw the idle's lights dark (emissive tracks' shapes at black) and keep its
    /// texture offsets where they are: a radio switched off (activators.md §3).
    pub dark: bool,
    /// What the alpha of every slot is multiplied by, 1 to 0: a shot's beam fading out
    /// (nvforever `shot_fx`, docs/spec-notes/combat-rules.md §15); 1 for everything else.
    pub fade: f32,
}

/// The values of `tracks` at `t` for each slot; `dark` blacks out the emissive of
/// every slot an emissive track drives (and leaves offsets and alpha where they are).
/// A member no track drives keeps its stored value; offset and alpha stay where a
/// track last left them. A track with its own timing ([`MaterialTrack::cycle`]) is
/// taken at its own time after `clock` seconds of play instead of at `t`.
pub fn sample(slots: &[MaterialSlot], tracks: &[MaterialTrack], t: f32, clock: f32, dark: bool) -> Vec<Look> {
    slots
        .iter()
        .map(|s| {
            let mut l = Look { refraction: s.refraction, ..Look::stored(s.emissive, s.emissive_mult, s.alpha) };
            if let Some(last) = s.last {
                (l.uv, l.scale, l.rotate, l.alpha) = (last.uv, last.scale, last.rotate, last.alpha);
            }
            for tr in tracks.iter().filter(|tr| tr.shape == s.shape) {
                let t = tr.cycle.map_or(t, |c| c.time(clock));
                match tr.target {
                    MaterialTarget::SelfIllum if dark => l.colour = [0.0; 3],
                    MaterialTarget::EmissiveMult if dark => l.mult = 0.0,
                    _ if dark => {}
                    target => {
                        if let Some(v) = tr.values.at(t) {
                            l.take(target, v);
                        }
                    }
                }
            }
            l
        })
        .collect()
}

impl RigMaterials {
    /// Pose the slots' materials with `tracks` at sequence time `t`, after `clock`
    /// seconds of play (see [`sample`]). With `dark`, an unlit shape any of the tracks
    /// drives is drawn black as well (alpha 0 when blended): the unlit shader ignores
    /// emissive, so the jukebox's light tubes stayed at full brightness while off; the
    /// original's go dark [observed: owner 2026-10-30, #1079; activators.md §3]. The
    /// additive notes and bubbles vanish with it.
    pub fn apply(&mut self, tracks: &[MaterialTrack], t: f32, clock: f32, dark: bool, drawing: &mut RigMaterialDrawing) {
        let values = sample(&self.slots, tracks, t, clock, dark);
        let fade = self.fade.clamp(0.0, 1.0);
        for (slot, v) in self.slots.iter_mut().zip(values) {
            let unlit_dark = dark && tracks.iter().any(|tr| tr.shape == slot.shape);
            // [design, #2685] Preserve the builder's values for untouched members.
            // Retain previous membership so replacing a clip can restore its values.
            let driven = tracks.iter().filter(|tr| tr.shape == slot.shape)
                .fold(slot.driven, |d, tr| d.or(Driven::of(tr.target)));
            if !slot.own {
                continue;
            }
            let Ok((current, override_)) = drawing.shapes.get(slot.entity) else { continue };
            let source = plain_material(&current.0, override_);
            if slot.last == Some(v) && slot.faded == fade && slot.unlit_dark == unlit_dark && slot.driven == driven {
                continue;
            }
            // [design, #2557] Keep animating the bound private plain source. Another
            // owner's replacement (e.g. a fade) keeps ownership until it restores us.
            // Only re-insert `MeshMaterial3d` when an override (e.g. stealth cloak)
            // currently shadows `slot.handle`; re-inserting the already-bound handle on
            // uncloaked meshes dirties `MeshMaterial3d` and triggers unnecessary render
            // specialization every frame [design].
            let publish = current.0 != slot.handle && source == &slot.handle;
            let Some(mut m) = drawing.materials.get_mut(&slot.handle) else { continue };
            let unlit = m.base.unlit && !matches!(m.base.alpha_mode, AlphaMode::Multiply);
            if unlit && slot.unlit_base.is_none() {
                slot.unlit_base = Some(m.base.base_color.to_linear());
            }
            let faded = Driven { alpha: fade < 1.0 || slot.faded < 1.0, ..default() };
            pose_material(&mut m, &Look { alpha: v.alpha * fade, ..v }, driven.or(faded), false);
            if let Some(base) = slot.unlit_base.filter(|_| unlit && (unlit_dark || slot.unlit_dark)) {
                let a = m.base.base_color.alpha();
                m.base.base_color = if unlit_dark {
                    let a = if matches!(m.base.alpha_mode, AlphaMode::Blend) { 0.0 } else { a };
                    Color::LinearRgba(LinearRgba::new(0.0, 0.0, 0.0, a))
                } else {
                    // [design, #2685] Dark mode hid blended alpha even without an
                    // alpha track. Restore it unless animation/fade just posed it.
                    let a = if driven.alpha || faded.alpha { a } else { base.alpha };
                    Color::LinearRgba(LinearRgba { alpha: a, ..base })
                };
            }
            slot.last = Some(v);
            slot.faded = fade;
            slot.unlit_dark = unlit_dark;
            slot.driven = driven;
            // [design, #2557] Publication dirties only this mesh's cloak derivation.
            if publish {
                drawing.commands.entity(slot.entity).insert(MeshMaterial3d(slot.handle.clone()));
            }
        }
    }
}

/// [design, #2557] Shared plain-source ownership and publication for rig animation.
#[derive(SystemParam)]
pub struct RigMaterialDrawing<'w, 's> {
    commands: Commands<'w, 's>,
    shapes: Query<'w, 's, (&'static MeshMaterial3d<NifMaterial>, Option<&'static MaterialOverride>)>,
    materials: ResMut<'w, Assets<NifMaterial>>,
}

/// On a mesh wearing a see-through copy of its material that is given back when it ends
/// (a reference's fade, `nvf_play::fade`): the material given back, and the copy worn. A
/// rig that binds its shape's material meanwhile copies `source`, not the copy, and puts
/// its own copy here, so the fade goes on from it and gives it back (#3919: the Hidden
/// Valley storm, enabled with a fade, copied the fade's copy and was given back the model's
/// material, its scroll going on in a material nothing drew).
#[derive(Component, Debug, Clone)]
pub struct HeldCopy {
    pub source: Handle<NifMaterial>,
    pub copy: Handle<NifMaterial>,
}

/// Give each newly drawn rig's driven shapes their own copy of their material. A shape
/// wearing a [`HeldCopy`] keeps it: the rig's own copy is made from its source and becomes
/// that source.
pub fn bind_rig_materials(
    mut rigs: Query<&mut RigMaterials, Added<RigMaterials>>,
    mut held: Query<&mut HeldCopy>,
    mut drawing: RigMaterialDrawing,
) {
    for mut rig in &mut rigs {
        for slot in &mut rig.slots {
            let Ok((current, override_)) = drawing.shapes.get(slot.entity) else { continue };
            let mut held = held.get_mut(slot.entity).ok().filter(|h| h.copy == current.0);
            let from = held.as_ref().map_or_else(|| plain_material(&current.0, override_).clone(), |h| h.source.clone());
            let Some(copy) = drawing.materials.get(&from).cloned() else { continue };
            let own = drawing.materials.add(copy);
            match held.as_mut() {
                Some(h) => h.source = own.clone(),
                None => {
                    drawing.commands.entity(slot.entity).insert(MeshMaterial3d(own.clone()));
                }
            }
            slot.handle = own;
            slot.own = true;
        }
    }
}

/// Pose the materials of rigs playing their idle clip at its time (or dark), and fade
/// those that fade.
pub fn animate_rig_materials(mut rigs: Query<(&Playing, &mut RigMaterials)>, mut drawing: RigMaterialDrawing) {
    for (p, mut rig) in &mut rigs {
        if rig.idle.is_empty() && rig.fade >= 1.0 {
            continue;
        }
        let idle = rig.idle.clone();
        let dark = rig.dark;
        rig.apply(&idle, p.sequence_time(), p.time - p.start, dark, &mut drawing);
    }
}

/// The values the playing layers of an actor give the materials of its shapes: for each
/// (shape, member) the layer with the most weight that drives it [design: a material is
/// not blended as bones are; the strongest clip's value is the one drawn], and of layers
/// as strong the track of the higher priority (a robot's `Talking` at 85 over its idle's
/// 0 on the chest light, `talk_clips.rs` in nvforever; docs/spec-notes/lipsync.md §6.1),
/// and of those as strong and as high the layer played over the others (the later slot,
/// as an idle's over the base idle's: Mr. House's `DynamicIdle_YesMan.kf` and his
/// `mtidle.kf` both give his Yes Man screen priority 20, and the idle's alpha 1 shows it,
/// #1542) [design].
/// `out` gets (shape, target, value, weight) and is cleared first; shapes are named as
/// the clips name them.
pub fn actor_material_values<'a>(player: &'a BlendPlayer, out: &mut Vec<(&'a str, MaterialTarget, [f32; 3], f32)>) {
    out.clear();
    // The priority of each entry of `out`, in its order.
    let mut priorities: Vec<i32> = Vec::new();
    for layer in player.layers.iter().flatten().filter(|l| l.weight > 0.0) {
        let bc = &layer.clip;
        if bc.clip.materials.is_empty() {
            continue;
        }
        let time = cycle_time(bc.time_at(layer.phase), bc.clip.start, bc.clip.stop, bc.clip.cycle_type);
        for tr in &bc.clip.materials {
            let Some(v) = tr.values.at(tr.cycle.map_or(time, |c| c.time(time - bc.clip.start))) else { continue };
            match out.iter().position(|(s, t, ..)| *t == tr.target && s.eq_ignore_ascii_case(&tr.shape)) {
                Some(k) if (out[k].3, priorities[k]) > (layer.weight, tr.priority) => {}
                Some(k) => {
                    out[k] = (tr.shape.as_str(), tr.target, v, layer.weight);
                    priorities[k] = tr.priority;
                }
                None => {
                    out.push((tr.shape.as_str(), tr.target, v, layer.weight));
                    priorities.push(tr.priority);
                }
            }
        }
    }
}

/// [`actor_material_values`] for the layers of an [`Animator`] (the player's rigs): a
/// layer's strength is its weight times its fade.
pub fn animator_material_values<'a>(animator: &'a Animator, out: &mut Vec<(&'a str, MaterialTarget, [f32; 3], f32)>) {
    out.clear();
    for layer in &animator.layers {
        let w = layer.weight * layer.ease;
        if w <= 0.0 || layer.clip.materials.is_empty() {
            continue;
        }
        let time = layer.sample_time(animator.phase);
        for tr in &layer.clip.materials {
            let Some(v) = tr.values.at(tr.cycle.map_or(time, |c| c.time(time - layer.clip.start))) else { continue };
            match out.iter_mut().find(|(s, t, ..)| *t == tr.target && s.eq_ignore_ascii_case(&tr.shape)) {
                Some(e) if e.3 >= w => {}
                Some(e) => *e = (tr.shape.as_str(), tr.target, v, w),
                None => out.push((tr.shape.as_str(), tr.target, v, w)),
            }
        }
    }
}

#[cfg(test)]
#[path = "material_anim_publish_tests.rs"]
mod material_anim_publish_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use fnv_nif::anim::{Channel, Key, KeyKind, Keys, ValueTrack};

    fn keys<const N: usize>(pts: &[(f32, [f32; N])]) -> Keys<N> {
        Keys { kind: KeyKind::Linear, keys: pts.iter().map(|&(time, value)| Key { time, value, forward: [0.0; N], backward: [0.0; N], tbc: [0.0; 3] }).collect() }
    }

    fn slot(name: &str) -> MaterialSlot {
        MaterialSlot::new(name.into(), Entity::PLACEHOLDER, Handle::default(), [0.86; 3], 1.0, 0.0)
    }

    #[test]
    fn tracks_replace_what_they_drive() {
        // The saloon jukebox's body light: emissive 0.86 stored, the Idle dims it to
        // 0.349 at 2 s; the music notes scroll from 2 to 0 in 8 s.
        let glow = MaterialTrack {
            shape: "JukeBox01:0".into(),
            target: MaterialTarget::SelfIllum,
            values: Arc::new(ValueTrack::Point3(Channel::Keys(keys(&[(0.0, [0.863; 3]), (2.0, [0.349; 3])])))),
            cycle: None,
            priority: 0,
        };
        let notes = MaterialTrack {
            shape: "Music".into(),
            target: MaterialTarget::UvOffset(0),
            values: Arc::new(ValueTrack::Float(Channel::Keys(keys(&[(0.0, [2.0]), (8.0, [0.0])])))),
            cycle: None,
            priority: 0,
        };
        let slots = [slot("JukeBox01:0"), slot("Music"), slot("Other")];
        let tracks = [glow, notes];
        let v = sample(&slots, &tracks, 2.0, 2.0, false);
        assert!((v[0].colour[0] - 0.349).abs() < 1e-5 && v[0].mult == 1.0 && v[0].uv == [0.0, 0.0]);
        assert!((v[1].uv[0] - 1.5).abs() < 1e-5 && v[1].colour == [0.86; 3]);
        assert_eq!(v[2], Look::stored([0.86; 3], 1.0, 0.0));
        // Dark: the light goes out, the notes stay where they were.
        let d = sample(&slots, &tracks, 2.0, 2.0, true);
        assert_eq!(d[0].colour, [0.0; 3]);
        assert_eq!(d[1].colour, [0.86; 3]);
        assert_eq!(d[2].colour, [0.86; 3]);
    }

    #[test]
    fn an_alpha_track_fades_the_shape() {
        // `windowem01.nif`: the window stores alpha 0; Left keys 1 (lit), Right 0.
        let left = MaterialTrack {
            shape: "windowEm01:0".into(),
            target: MaterialTarget::Alpha,
            values: Arc::new(ValueTrack::Float(Channel::Keys(keys(&[(0.0, [1.0]), (0.033, [1.0])])))),
            cycle: None,
            priority: 0,
        };
        let slots = [slot("windowEm01:0")];
        assert_eq!(sample(&slots, &[], 0.0, 0.0, false)[0].alpha, 0.0);
        assert_eq!(sample(&slots, std::slice::from_ref(&left), 0.01, 0.01, false)[0].alpha, 1.0);
    }

    #[test]
    fn dark_blended_material_restores_alpha_on_reactivation() {
        // [design, #2685] Synthetic builder alpha differs from the stored look;
        // an undriven alpha must survive the dark override without normalization.
        for target in [MaterialTarget::UvOffset(0), MaterialTarget::SelfIllum] {
            for animated_alpha in [false, true] {
                for (off_fade, on_fade) in [(1.0, 1.0), (0.5, 0.5), (0.5, 1.0), (1.0, 0.5)] {
                    let track = |target, value| MaterialTrack {
                        shape: "SyntheticGlow".into(), target,
                        values: Arc::new(ValueTrack::Float(Channel::Static(Some([value])))),
                        cycle: None, priority: 0,
                    };
                    let mut tracks = vec![track(target, 0.4)];
                    if animated_alpha {
                        tracks.push(track(MaterialTarget::Alpha, 0.25));
                    }
                    let mut app = App::new();
                    app.init_resource::<Assets<NifMaterial>>().add_systems(Update, (
                        bind_rig_materials,
                        |mut rigs: Query<&mut RigMaterials>, mut drawing: RigMaterialDrawing| {
                            for mut rig in &mut rigs {
                                let tracks = rig.idle.clone();
                                let dark = rig.dark;
                                rig.apply(&tracks, 1.0, 1.0, dark, &mut drawing);
                            }
                        },
                    ).chain());
                    let mut material = NifMaterial::default();
                    material.base.unlit = true;
                    material.base.alpha_mode = AlphaMode::Blend;
                    material.base.base_color = Color::linear_rgba(0.2, 0.3, 0.4, 0.6);
                    let original = material.base.base_color.to_linear();
                    let shared = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(material);
                    let entity = app.world_mut().spawn(MeshMaterial3d(shared.clone())).id();
                    app.world_mut().entity_mut(entity).insert(RigMaterials {
                        slots: vec![MaterialSlot::new("SyntheticGlow".into(), entity, shared.clone(), [0.0; 3], 1.0, 0.8)],
                        idle: Arc::new(tracks), dark: false, fade: off_fade,
                    });
                    let drawn = |app: &App| {
                        let handle = &app.world().get::<MeshMaterial3d<NifMaterial>>(entity).unwrap().0;
                        assert_ne!(handle, &shared);
                        app.world().resource::<Assets<NifMaterial>>().get(handle).unwrap().base.base_color.to_linear()
                    };
                    app.update();
                    let pre_dark = if animated_alpha { 0.25 } else if off_fade < 1.0 { 0.8 } else { 0.6 };
                    assert_eq!(drawn(&app).alpha, pre_dark * off_fade);
                    app.world_mut().get_mut::<RigMaterials>(entity).unwrap().dark = true;
                    for _ in 0..2 {
                        app.update();
                        assert_eq!(drawn(&app).alpha, 0.0);
                    }
                    {
                        let mut rig = app.world_mut().get_mut::<RigMaterials>(entity).unwrap();
                        rig.dark = false;
                        rig.fade = on_fade;
                    }
                    let restored = if animated_alpha { 0.25 } else if off_fade < 1.0 || on_fade < 1.0 { 0.8 } else { 0.6 };
                    for _ in 0..2 {
                        app.update();
                        assert_eq!(drawn(&app), LinearRgba { alpha: restored * on_fade, ..original });
                    }
                    assert_eq!(app.world().resource::<Assets<NifMaterial>>().get(&shared).unwrap().base.base_color.to_linear(), original);
                }
            }
        }
    }

    #[test]
    fn scale_and_rotation_turn_the_texture_about_its_centre() {
        let mut l = Look::stored([0.0; 3], 1.0, 1.0);
        assert!(l.uv_transform().abs_diff_eq(Affine2::IDENTITY, 1e-6));
        // An offset alone moves the texture as before.
        l.take(MaterialTarget::UvOffset(1), [0.25; 3]);
        assert!(l.uv_transform().abs_diff_eq(Affine2::from_translation(Vec2::new(0.0, 0.25)), 1e-6));
        // Scale 2 about the centre: the centre stays, a corner goes twice as far.
        let mut l = Look::stored([0.0; 3], 1.0, 1.0);
        l.take(MaterialTarget::UvScale(0), [2.0; 3]);
        l.take(MaterialTarget::UvScale(1), [2.0; 3]);
        assert!(l.uv_transform().transform_point2(Vec2::splat(0.5)).abs_diff_eq(Vec2::splat(0.5), 1e-6));
        assert!(l.uv_transform().transform_point2(Vec2::ZERO).abs_diff_eq(Vec2::splat(-0.5), 1e-6));
        // A quarter turn about the centre.
        let mut l = Look::stored([0.0; 3], 1.0, 1.0);
        l.take(MaterialTarget::UvRotate, [std::f32::consts::FRAC_PI_2; 3]);
        assert!(l.uv_transform().transform_point2(Vec2::new(1.0, 0.5)).abs_diff_eq(Vec2::new(0.5, 1.0), 1e-5));
    }

    #[test]
    fn an_unlit_shape_glows_by_its_emissive_value() {
        // The Securitron screen: unlit, emissive 0.032 × 30 in the idle.
        let mut m = NifMaterial::default();
        m.base.unlit = true;
        let mut l = Look::stored([0.032; 3], 30.0, 1.0);
        pose_material(&mut m, &l, Driven { emissive: true, ..default() }, true);
        assert!((m.base.base_color.to_linear().red - 0.96).abs() < 1e-5);
        assert_eq!(m.base.emissive, LinearRgba::BLACK);
        // An additive shape forms it in display values: 0.96 raised to the display gamma.
        let mut added = NifMaterial::default();
        added.base.unlit = true;
        added.base.alpha_mode = AlphaMode::Add;
        pose_material(&mut added, &l, Driven { emissive: true, ..default() }, true);
        assert!((added.base.base_color.to_linear().red - 0.96f32.powf(2.2)).abs() < 1e-5);
        // Without the rule, and on a lit shape, it is emissive light.
        l.colour = [0.1; 3];
        let mut lit = NifMaterial::default();
        pose_material(&mut lit, &l, Driven { emissive: true, ..default() }, true);
        assert!((lit.base.emissive.red - 3.0).abs() < 1e-5);
        // Members not driven stay.
        let before = lit.base.uv_transform;
        pose_material(&mut lit, &l, Driven { alpha: true, ..default() }, true);
        assert_eq!(lit.base.uv_transform, before);
    }

    #[test]
    fn a_controller_of_its_own_keeps_its_own_cycle() {
        // `fxsmokewisps01.nif`: the wisps' texture scrolls along V from 0 to 1 in 36.67 s
        // and along U in 44.1 s, each looping by itself.
        let track = |axis: usize, stop: f32| MaterialTrack {
            shape: "WispsGeo:0".into(),
            target: MaterialTarget::UvOffset(axis),
            values: Arc::new(ValueTrack::Float(Channel::Keys(keys(&[(0.0, [0.0]), (stop, [1.0])])))),
            cycle: Some(crate::anim_play::OwnCycle { start: 0.0, stop, frequency: 1.0, phase: 0.0, cycle_type: 0 }),
            priority: 0,
        };
        let tracks = [track(1, 36.666668), track(0, 44.1)];
        let clip = crate::anim_play::Clip::from_controllers(tracks.to_vec(), Vec::new(), Vec::new());
        assert!((clip.stop - 44.1).abs() < 1e-4);
        let slots = [slot("WispsGeo:0")];
        // 50 s of play: V is 13.33 s into its second cycle, U 5.9 s into its second,
        // whatever the clip's own (wrapped) time.
        let v = sample(&slots, &tracks, 5.9, 50.0, false)[0].uv;
        assert!((v[1] - 13.333332 / 36.666668).abs() < 1e-4, "{v:?}");
        assert!((v[0] - 5.9 / 44.1).abs() < 1e-4, "{v:?}");
    }

    #[test]
    fn of_layers_as_strong_the_higher_priority_shows() {
        // The Securitron's chest light `##VoiceBox_talk:0`: its idle holds it at 0 with
        // priority 0, `Talking` lights it with priority 85 (lipsync.md §6.1).
        use crate::anim_play::{BoundClip, Layer};
        let clip = |v: f32, priority: i32| {
            let track = MaterialTrack {
                shape: "##VoiceBox_talk:0".into(),
                target: MaterialTarget::SelfIllum,
                values: Arc::new(ValueTrack::Point3(Channel::Keys(keys(&[(0.0, [v; 3]), (1.0, [v; 3])])))),
                cycle: None,
                priority,
            };
            let mut c = crate::anim_play::Clip::from_controllers(vec![track], Vec::new(), Vec::new());
            c.materials[0].cycle = None;
            Arc::new(BoundClip::bind(Arc::new(c), |_| None))
        };
        let (idle, talk) = (clip(0.0, 0), clip(0.15, 85));
        let value = |a: &Arc<BoundClip>, wa: f32, b: &Arc<BoundClip>, wb: f32| {
            let mut player = BlendPlayer::new(Vec::new(), Arc::new(Vec::new()));
            player.layers[0] = Some(Layer { clip: a.clone(), phase: 0.5, weight: wa });
            player.layers[1] = Some(Layer { clip: b.clone(), phase: 0.5, weight: wb });
            let mut out = Vec::new();
            actor_material_values(&player, &mut out);
            assert_eq!(out.len(), 1);
            out[0].2[0]
        };
        assert!((value(&idle, 1.0, &talk, 1.0) - 0.15).abs() < 1e-6);
        assert!((value(&talk, 1.0, &idle, 1.0) - 0.15).abs() < 1e-6);
        // A stronger layer still wins over a higher priority.
        assert_eq!(value(&idle, 1.0, &talk, 0.5), 0.0);
        // As strong and of the same priority: the layer played over the other (the
        // later slot) shows, as an idle over the base idle (Mr. House's screen, #1542).
        let (base, over) = (clip(0.0, 20), clip(1.0, 20));
        assert_eq!(value(&base, 1.0, &over, 1.0), 1.0);
        assert_eq!(value(&over, 1.0, &base, 1.0), 0.0);
    }
}
