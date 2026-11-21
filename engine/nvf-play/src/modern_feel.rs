//! Modern Feel (NVF Features › Modern Feel): the fork owner's "modernize the game feel"
//! experiment (2026-11-20, branch feat/modern-feel). Every part is NVForever's own
//! [design; every constant in the parts an engineering choice, tuned by eye], none of it in
//! the original, and every part on by default (the owner: "keep on by default").
//!
//! One resource, [`ModernFeel`], holds the Global Toggle and a switch per part; the parts
//! read it through its methods (`hit_reactions()` …), which are false while the Global
//! Toggle is off. The parts:
//!
//! - **Hit reactions** (nvforever `hit_react.rs`): the struck bone of a living actor is
//!   pushed on a spring laid over its animation; a killing hit hands over to the ragdoll
//!   with the shot's push.
//! - **Layered gunshots** (`gun_audio`, `crate::gun_audio`): a shot's tail by the place's acoustic space
//!   (outdoors a rolling echo, indoors a short slap), its loudness and brightness by distance.
//! - **Sound occlusion** (`crate::occlusion`): world sounds behind walls are muffled and quieter.
//! - **Camera kick**: each shot kicks and rolls the view by the weapon (damage, rate, weight)
//!   and recovers; heavy shots punch the field of view.
//! - **Shell casings**: spent cases fly from the gun as physical bodies with their sounds.
//! - **Impact effects**: muzzle smoke, barrel heat and debris and sparks by surface.
//! - **Damage direction**: the HUD marks where a hit on the player came from; a kill has its
//!   own confirming sound.
//! - **Grounded footsteps** (nvforever `npc_feel.rs`): actors' walk clips play at the speed they
//!   move, feet stay put in a turn, and standing actors turn in place.
//! - **NPC aim** (nvforever `npc_feel.rs`): a gunman's spine bends so the barrel points at its target.
//! - **Volumetric fog** (nvforever `vol_fog.rs`): light shafts and haze in dusty air from the sun
//!   and lamps.
//! - **Object motion blur** (nvforever `object_blur.rs`): moving things blur by their own
//!   motion, not only the camera's.

use bevy::prelude::*;

/// The switches (all on by default).
#[derive(Resource, Clone, Debug)]
pub struct ModernFeel {
    /// The Global Toggle: off, no part acts (each keeps its own choice).
    pub on: bool,
    pub hit_reactions: bool,
    pub gun_audio: bool,
    pub sound_occlusion: bool,
    pub camera_kick: bool,
    pub shell_casings: bool,
    pub impact_effects: bool,
    pub damage_direction: bool,
    pub grounded_footsteps: bool,
    pub npc_aim: bool,
    pub volumetric_fog: bool,
    pub object_motion_blur: bool,
}

impl Default for ModernFeel {
    fn default() -> Self {
        Self {
            on: true,
            hit_reactions: true,
            gun_audio: true,
            sound_occlusion: true,
            camera_kick: true,
            shell_casings: true,
            impact_effects: true,
            damage_direction: true,
            grounded_footsteps: true,
            npc_aim: true,
            volumetric_fog: true,
            object_motion_blur: true,
        }
    }
}

impl ModernFeel {
    pub fn hit_reactions(&self) -> bool {
        self.on && self.hit_reactions
    }
    pub fn gun_audio(&self) -> bool {
        self.on && self.gun_audio
    }
    pub fn sound_occlusion(&self) -> bool {
        self.on && self.sound_occlusion
    }
    pub fn camera_kick(&self) -> bool {
        self.on && self.camera_kick
    }
    pub fn shell_casings(&self) -> bool {
        self.on && self.shell_casings
    }
    pub fn impact_effects(&self) -> bool {
        self.on && self.impact_effects
    }
    pub fn damage_direction(&self) -> bool {
        self.on && self.damage_direction
    }
    pub fn grounded_footsteps(&self) -> bool {
        self.on && self.grounded_footsteps
    }
    pub fn npc_aim(&self) -> bool {
        self.on && self.npc_aim
    }
    pub fn volumetric_fog(&self) -> bool {
        self.on && self.volumetric_fog
    }
    pub fn object_motion_blur(&self) -> bool {
        self.on && self.object_motion_blur
    }
}

/// The resource and the settings page's rows (NVF Features › Modern Feel).
pub struct ModernFeelPlugin;

impl Plugin for ModernFeelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ModernFeel>();
        type F = ModernFeel;
        let row = |key, label, note, get: fn(&World) -> bool, set: fn(&mut World, bool)| crate::settings::Setting {
            key,
            group: crate::pause::GROUP_MENUS,
            label,
            note,
            switches: "",
            get,
            set,
            slider: None,
        };
        for s in [
            row(
                "modern-feel",
                "Global Toggle",
                "On: the switches below modernize how fights look, sound and feel. Off: none of them acts, as in the original; each keeps its own choice.",
                |w| w.resource::<F>().on,
                |w, on| w.resource_mut::<F>().on = on,
            ),
            row(
                "feel-hit-reactions",
                "Hit reactions",
                "On: a hit jolts the part of the body it strikes on top of what the actor is doing, and a killing shot throws the body with it.",
                |w| w.resource::<F>().hit_reactions,
                |w, on| w.resource_mut::<F>().hit_reactions = on,
            ),
            row(
                "feel-camera-kick",
                "Camera kick",
                "On: each shot kicks and rolls the view by the weapon's power and weight, then recovers; heavy shots punch the field of view.",
                |w| w.resource::<F>().camera_kick,
                |w, on| w.resource_mut::<F>().camera_kick = on,
            ),
            row(
                "feel-damage-direction",
                "Damage direction",
                "On: the HUD marks the direction a hit on you came from, and a kill plays a short confirming sound.",
                |w| w.resource::<F>().damage_direction,
                |w, on| w.resource_mut::<F>().damage_direction = on,
            ),
            row(
                "feel-gun-audio",
                "Layered gunshots",
                "On: shots echo by the place: a rolling tail outdoors, a short slap indoors; far shots are duller and quieter.",
                |w| w.resource::<F>().gun_audio,
                |w, on| w.resource_mut::<F>().gun_audio = on,
            ),
            row(
                "feel-sound-occlusion",
                "Sound occlusion",
                "On: sounds behind walls and floors are muffled and quieter.",
                |w| w.resource::<F>().sound_occlusion,
                |w, on| w.resource_mut::<F>().sound_occlusion = on,
            ),
            row(
                "feel-shell-casings",
                "Shell casings",
                "On: spent cases fly from the gun, bounce and clink on the ground.",
                |w| w.resource::<F>().shell_casings,
                |w, on| w.resource_mut::<F>().shell_casings = on,
            ),
            row(
                "feel-impact-effects",
                "Impact effects",
                "On: muzzle smoke after shots and dust, chips and sparks where bullets strike, by the surface.",
                |w| w.resource::<F>().impact_effects,
                |w, on| w.resource_mut::<F>().impact_effects = on,
            ),
            row(
                "feel-grounded-footsteps",
                "Grounded footsteps",
                "On: people and creatures step at the speed they move, keep their feet planted in turns and turn in place.",
                |w| w.resource::<F>().grounded_footsteps,
                |w, on| w.resource_mut::<F>().grounded_footsteps = on,
            ),
            row(
                "feel-npc-aim",
                "NPC aim",
                "On: armed people bend their spine and arms so their weapons point at what they shoot.",
                |w| w.resource::<F>().npc_aim,
                |w, on| w.resource_mut::<F>().npc_aim = on,
            ),
            row(
                "feel-volumetric-fog",
                "Volumetric fog",
                "On: sunlight and lamps light the haze and dust in the air, with shafts through gaps.",
                |w| w.resource::<F>().volumetric_fog,
                |w, on| w.resource_mut::<F>().volumetric_fog = on,
            ),
            row(
                "feel-motion-blur",
                "Object motion blur",
                "On: moving things blur by their own motion as well as the camera's.",
                |w| w.resource::<F>().object_motion_blur,
                |w, on| w.resource_mut::<F>().object_motion_blur = on,
            ),
        ] {
            crate::settings::add_setting(app, s);
        }
        app.add_systems(Update, log_switches);
    }
}

/// "modern feel: …" in the log when a switch changes (and once at start), for smoke runs.
fn log_switches(feel: Res<ModernFeel>, mut last: Local<Option<String>>) {
    if !feel.is_changed() && last.is_some() {
        return;
    }
    let f = &*feel;
    let line = format!(
        "modern feel: on {} hit-reactions {} camera-kick {} damage-direction {} gun-audio {} occlusion {} casings {} impacts {} footsteps {} npc-aim {} volumetric-fog {} motion-blur {}",
        f.on,
        f.hit_reactions(),
        f.camera_kick(),
        f.damage_direction(),
        f.gun_audio(),
        f.sound_occlusion(),
        f.shell_casings(),
        f.impact_effects(),
        f.grounded_footsteps(),
        f.npc_aim(),
        f.volumetric_fog(),
        f.object_motion_blur()
    );
    if last.as_deref() != Some(line.as_str()) {
        info!("{line}");
        *last = Some(line);
    }
}
