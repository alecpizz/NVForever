//! Cosmetic presentation of stable touch actions [design: android-adaptive-actions.md].
use crate::{
    combat::{Combat, WeaponAnim},
    control::PlayerControl,
    PlaySet, PlayState,
};
use bevy::prelude::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AimRole {
    #[default]
    Aim,
    Block,
    Scope,
}
impl AimRole {
    pub fn label(self, adaptive: bool) -> &'static str {
        match (adaptive, self) {
            (true, Self::Block) => "Block",
            (true, Self::Scope) => "Scope",
            _ => "Aim",
        }
    }
    pub fn glyph(self, adaptive: bool) -> &'static str {
        if adaptive && self == Self::Block {
            "block"
        } else {
            "aim"
        }
    }
}
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AimPresentation(pub AimRole);
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdaptiveTouchActions(pub bool);
impl Default for AdaptiveTouchActions {
    fn default() -> Self {
        Self(true)
    }
}

/// Use the same current weapon classification and ready/drawn gates as gameplay.
pub fn aim_role(
    combat: Option<&Combat>,
    control: Option<&PlayerControl>,
    anim: Option<&WeaponAnim>,
    playing: bool,
) -> AimRole {
    if !playing || !control.is_some_and(|c| c.weapon_out) || !anim.is_some_and(|a| a.ready) {
        return AimRole::Aim;
    }
    let Some(combat) = combat else {
        return AimRole::Aim;
    };
    if combat.player_weapon_blocks() {
        AimRole::Block
    } else if combat.player_weapon_scoped() {
        AimRole::Scope
    } else {
        AimRole::Aim
    }
}
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PublishPresentation;
pub struct TouchActionPresentationPlugin;
impl Plugin for TouchActionPresentationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AimPresentation>()
            .init_resource::<AdaptiveTouchActions>()
            .add_systems(
                Update,
                publish.in_set(PlaySet::Ui).in_set(PublishPresentation),
            );
        crate::settings::add_setting(app,crate::settings::Setting {
            key:"adaptive-touch-actions",group:"Touch",label:"Adaptive touch actions",
            note:"On: Aim becomes Block for ready drawn melee weapons or fists, or Scope for a ready drawn scoped gun; off: keep Aim. Only text and icons change; bindings, actions and positions stay the same.",
            switches: "",
            get:|w|w.resource::<AdaptiveTouchActions>().0,set:|w,on|w.resource_mut::<AdaptiveTouchActions>().0=on,slider:None,
        });
    }
}
fn publish(
    combat: Option<Res<Combat>>,
    control: Option<Res<PlayerControl>>,
    anim: Option<Res<WeaponAnim>>,
    state: Option<Res<State<PlayState>>>,
    paused: Option<Res<crate::pause::Paused>>,
    mut presentation: ResMut<AimPresentation>,
) {
    let role = aim_role(
        combat.as_deref(),
        control.as_deref(),
        anim.as_deref(),
        state.is_some_and(|s| *s.get() == PlayState::Playing) && !paused.is_some_and(|p| p.0),
    );
    if presentation.0 != role {
        presentation.0 = role;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn static_preference_and_unknown_context_keep_the_legacy_aim_presentation() {
        for role in [AimRole::Aim, AimRole::Block, AimRole::Scope] {
            assert_eq!(role.label(false), "Aim");
            assert_eq!(role.glyph(false), "aim");
        }
        assert_eq!(AimRole::Block.label(true), "Block");
        assert_eq!(AimRole::Scope.label(true), "Scope");
        assert_eq!(
            aim_role(
                None,
                Some(&PlayerControl {
                    weapon_out: true,
                    ..default()
                }),
                Some(&WeaponAnim {
                    ready: true,
                    ..default()
                }),
                true
            ),
            AimRole::Aim
        );
    }
    #[test]
    fn the_static_preference_uses_the_normal_persisted_settings_row() {
        let path = std::env::temp_dir().join(format!(
            "nvf-adaptive-touch-{}-{}.ini",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut app = App::new();
        app.add_plugins(TouchActionPresentationPlugin)
            .insert_resource(crate::settings::SettingsFile(Some(path.clone())));
        crate::settings::apply_stored(app.world_mut());
        let index = app
            .world()
            .resource::<crate::settings::Settings>()
            .index("adaptive-touch-actions")
            .unwrap();
        assert_eq!(crate::settings::flip(app.world_mut(), index), Some(false));
        let mut loaded = App::new();
        loaded
            .add_plugins(TouchActionPresentationPlugin)
            .insert_resource(crate::settings::SettingsFile(Some(path.clone())));
        crate::settings::apply_stored(loaded.world_mut());
        assert!(!loaded.world().resource::<AdaptiveTouchActions>().0);
        std::fs::remove_file(path).unwrap();
    }
}
