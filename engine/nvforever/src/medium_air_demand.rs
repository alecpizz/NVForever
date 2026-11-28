//! The demand for the light in the air (#3032 stage 3, "light in the air"): an engineering
//! choice of NVForever's own for the enhanced look [design], never a parity item; off by
//! default and in every preset.
//!
//! [`AirWanted`] is the one main-world switch every part of stage 3 reads: the grid and the
//! composite (medium_air_grid.rs, medium_air_compose.rs), and the three narrow consumers
//! outside them (the solid cores follow [`AirDraws`], what the light's gate settled on):
//! - graphics.rs `apply`: the main view stays HDR while it is true (the light is composed in
//!   the linear HDR picture before tonemapping);
//! - terrain_shadows.rs `follow_graphics`: the terrain map's producer runs while it is true
//!   (`TerrainShadowsProducerWanted`), the surfaces' terrain shading does not follow it;
//! - sun_visibility.rs `mark_targets`: the outdoor sun light is marked (`OutdoorSunLight`,
//!   render world `NvfOutdoorSun`, its GPU flag `DIRECTIONAL_LIGHT_FLAGS_NVF_OUTDOOR_SUN_BIT`)
//!   so the grid finds that light, never the first-person arms' shade light.
//!
//! Each consumer reads it as `Option<Res<AirWanted>>`, absent counting as false, so an app
//! without [`AirDemandPlugin`] (tests, a build without stage 3) behaves as with it off.
//!
//! **Who asks** [design]: the settings row Volumetric light (`volumetric-light`,
//! graphics.rs `Graphics::volumetric_light`, also `--graphics-only volumetric-light`) or the
//! debug aid [`AID`] (`NVF_PLAY_DEBUG_MEDIUM_LIGHT=<strength>`, read once at the start, not in
//! release builds: dev_aids.rs; it asks
//! when the value is a finite number above 0). One system, [`follow_demand`], is the only
//! writer of [`AirWanted`]: `row || aid`, on a device that can hold the volume
//! (medium_air_grid.rs `format_supported`, asked once; on another nothing of the light is
//! started, the terrain map's producer included, and one line says why), recomputed when the
//! aid is read or the graphics settings change (so a test that sets [`AirWanted`] itself, with no graphics settings, keeps
//! its value). The strength the light starts from is the aid's value when the aid asks, else
//! the row's slider ([`AirStrengthSetting`], Volumetric light strength, 0 to 200 %); both
//! rows take effect while the game runs (medium_air_light.rs).

use bevy::prelude::*;
use bevy::render::renderer::{RenderAdapter, RenderDevice};

/// The debug aid that asks for the light in the air (its value is the strength).
#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
pub const AID: &str = "NVF_PLAY_DEBUG_MEDIUM_LIGHT";

/// Whether the light in the air is wanted (main world; false by default).
#[derive(Resource, Default, Clone, Copy, PartialEq, Debug)]
pub struct AirWanted(pub bool);

/// Whether the light is drawn as far as its gate goes (main world; false by default):
/// medium_air_light.rs `follow_light` keeps it as its gate has settled (open at once, closed once
/// a reason held 0.5 s), so a closed gate (night without a moon, indoors, under water, a strength of 0) takes
/// with it what is drawn for the light alone (solid_cores.rs).
#[derive(Resource, Default, Clone, Copy, PartialEq, Debug)]
pub struct AirDraws(pub bool);

/// Whether this frame's gate is open (main world; false by default): the answer the frame's
/// composite is drawn by (medium_air_light.rs `follow_light` writes both, in `PostUpdate` before
/// the visibility check), without [`AirDraws`]' delay.
#[derive(Resource, Default, Clone, Copy, PartialEq, Debug)]
pub struct AirOpen(pub bool);

/// Whether this device can hold the volume, once asked (`None` before; [`follow_demand`]).
#[derive(Resource, Default, Clone, Copy, PartialEq, Debug)]
pub(crate) struct AirSupport(pub(crate) Option<bool>);

/// Whether the debug aid [`AID`] asks for the light (read once at the start; only with the
/// developer aids).
#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
#[derive(Resource, Default, Clone, Copy, PartialEq, Debug)]
pub struct AirAid(pub bool);

/// The Volumetric light strength row's value: the light's strength, the one overall scale of
/// its correction (medium_air.rs `applied_scale`), 0..=2 (the slider's 0 to 200 %), 1 by
/// default. Kept apart from `Graphics` so that moving the slider does not run graphics.rs
/// `apply` (which inserts the cameras' components again) [design].
#[derive(Resource, Clone, Copy, PartialEq, Debug)]
pub struct AirStrengthSetting(pub f32);

impl Default for AirStrengthSetting {
    fn default() -> Self {
        AirStrengthSetting(STRENGTH_DEFAULT)
    }
}

/// The row's default strength (100 %) and its most (200 %) [design].
pub const STRENGTH_DEFAULT: f32 = 1.0;
pub const STRENGTH_MOST: f32 = 2.0;

impl AirStrengthSetting {
    /// The slider's part (0..=1) for this strength: strength / 2.
    pub fn part(self) -> f32 {
        (self.0 / STRENGTH_MOST).clamp(0.0, 1.0)
    }

    /// The strength for a slider part (0..=1; NaN the default).
    pub fn from_part(part: f32) -> AirStrengthSetting {
        if part.is_nan() {
            return AirStrengthSetting::default();
        }
        AirStrengthSetting(part.clamp(0.0, 1.0) * STRENGTH_MOST)
    }

    /// The row's value as the page shows it: the strength in percent (0 to 200 %).
    pub fn shown(part: f32) -> String {
        format!("{:.0}%", part * STRENGTH_MOST * 100.0)
    }
}

/// Whether the aid's text `v` asks for the light: a finite number above 0.
#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
pub fn wanted_from_aid(v: Option<&str>) -> bool {
    v.and_then(|s| s.trim().parse::<f32>().ok()).is_some_and(|x| x.is_finite() && x > 0.0)
}

/// The demand: the row or the aid (`row || aid`).
pub fn wanted(row: bool, aid: bool) -> bool {
    row || aid
}

/// The demand a device allows: `asked`, unless the device cannot hold the volume (`supported`,
/// asked only for an asked light; `None`: no device to ask, as in tests).
pub fn granted(asked: bool, supported: impl FnOnce() -> Option<bool>) -> bool {
    asked && supported().unwrap_or(true)
}

/// Registers [`AirWanted`] and [`AirStrengthSetting`], with the developer aids [`AirAid`] (the aid
/// read once at the start), and follows the row (added by `MediumAirPlugin`).
pub struct AirDemandPlugin;

impl Plugin for AirDemandPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AirWanted>()
            .init_resource::<AirDraws>()
            .init_resource::<AirOpen>()
            .init_resource::<AirSupport>()
            .init_resource::<AirStrengthSetting>()
            .add_systems(PreUpdate, follow_demand);
        #[cfg(any(test, debug_assertions, feature = "dev-aids"))]
        app.init_resource::<AirAid>().add_systems(Startup, read_aid);
    }
}

/// Set [`AirAid`] from the aid; one log line when it asks for the light, none otherwise.
#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
fn read_aid(mut aid: ResMut<AirAid>) {
    let on = wanted_from_aid(crate::dev_aids::var(AID).as_deref());
    if on {
        info!("volumetric medium: light in the air wanted (debug aid {AID})");
    }
    // Written once even when unchanged, so `follow_demand` sees it as changed and decides.
    *aid = AirAid(on);
}

/// The one writer of [`AirWanted`]: `row || aid` whenever the aid was read or the graphics
/// settings changed since this system last ran (absent graphics settings count as the row
/// off), as far as the device allows ([`granted`]); nothing otherwise. One log line when the row
/// asks for the light or stops asking, and one when the device cannot draw it.
pub(crate) fn follow_demand(
    #[cfg(any(test, debug_assertions, feature = "dev-aids"))]
    aid: Res<AirAid>,
    graphics: Option<Res<crate::graphics::Graphics>>,
    device: Option<Res<RenderDevice>>,
    adapter: Option<Res<RenderAdapter>>,
    mut support: Option<ResMut<AirSupport>>,
    mut wanted_now: ResMut<AirWanted>,
    mut row_was: Local<Option<bool>>,
) {
    let graphics_changed = graphics.as_ref().is_some_and(|g| g.is_changed());
    #[cfg(any(test, debug_assertions, feature = "dev-aids"))]
    let (aid_changed, aid) = (aid.is_changed(), aid.0);
    #[cfg(not(any(test, debug_assertions, feature = "dev-aids")))]
    let (aid_changed, aid) = (false, false);
    if !aid_changed && !graphics_changed {
        return;
    }
    let row = graphics.is_some_and(|g| g.volumetric_light);
    if row_was.is_some_and(|was| was != row) || (row_was.is_none() && row) {
        info!("volumetric medium: light in the air {} (the Volumetric light row)", if row { "wanted" } else { "no longer wanted by the row" });
    }
    *row_was = Some(row);
    let next = AirWanted(granted(wanted(row, aid), || {
        let support = support.as_mut()?;
        if support.0.is_none() {
            let (Some(device), Some(adapter)) = (device, adapter) else { return None };
            let answer = crate::medium_air_grid::format_supported(&adapter, &device.limits());
            if let Err(reason) = &answer {
                warn!("volumetric medium: light in the air not drawn: {reason}");
            }
            support.0 = Some(answer.is_ok());
        }
        support.0
    }));
    wanted_now.set_if_neq(next);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::{Graphics, Preset};

    #[test]
    fn the_aid_asks_for_the_light_only_with_a_finite_strength_above_zero() {
        for on in ["1", "0.5", " 2 ", "1e3"] {
            assert!(wanted_from_aid(Some(on)), "{on:?}");
        }
        for off in [None, Some(""), Some("0"), Some("-1"), Some("off"), Some("NaN"), Some("inf"), Some("-0")] {
            assert!(!wanted_from_aid(off), "{off:?}");
        }
    }

    #[test]
    fn the_plugin_registers_the_demand_off_by_default() {
        let mut app = App::new();
        app.add_plugins(AirDemandPlugin);
        assert_eq!(*app.world().resource::<AirWanted>(), AirWanted(false));
        assert_eq!(AirWanted::default(), AirWanted(false));
        assert_eq!(*app.world().resource::<AirStrengthSetting>(), AirStrengthSetting(1.0));
    }

    /// The demand is the row or the aid: from the row, from the aid, from both, from neither.
    #[test]
    fn the_light_is_wanted_by_the_row_or_the_aid() {
        assert!(!wanted(false, false));
        assert!(wanted(true, false), "the row alone");
        assert!(wanted(false, true), "the aid alone");
        assert!(wanted(true, true), "both");
        // Through the system: the aid stands in for the environment (set before the first
        // run), the row is the graphics settings' switch, switched while the app runs.
        for aid in [false, true] {
            let mut app = App::new();
            app.insert_resource(Graphics::preset(Preset::Vanilla)).insert_resource(AirAid(aid)).init_resource::<AirWanted>().add_systems(Update, follow_demand);
            app.update();
            assert_eq!(app.world().resource::<AirWanted>().0, aid, "aid {aid}, row off");
            app.world_mut().resource_mut::<Graphics>().volumetric_light = true;
            app.update();
            assert!(app.world().resource::<AirWanted>().0, "aid {aid}, row on");
            app.world_mut().resource_mut::<Graphics>().volumetric_light = false;
            app.update();
            assert_eq!(app.world().resource::<AirWanted>().0, aid, "aid {aid}, row off again");
        }
    }

    /// A device that cannot hold the volume is not asked for the light, so nothing that follows
    /// the demand starts (the HDR view, the terrain map's producer, the sun's mark, the grid); it is
    /// not asked while the light is not.
    #[test]
    fn a_device_without_the_volume_gets_no_demand() {
        assert!(!granted(false, || panic!("no light asked for: the device is not either")));
        assert!(granted(true, || Some(true)) && granted(true, || None) && !granted(true, || Some(false)));
        // Through the system: the row or the aid on a device that answered no.
        for (aid, supported) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut app = App::new();
            app.insert_resource(Graphics::preset(Preset::Vanilla))
                .insert_resource(AirAid(aid))
                .insert_resource(AirSupport(Some(supported)))
                .init_resource::<AirWanted>()
                .add_systems(Update, follow_demand);
            app.update();
            assert_eq!(app.world().resource::<AirWanted>().0, aid && supported);
            for row in [true, false, true] {
                app.world_mut().resource_mut::<Graphics>().volumetric_light = row;
                app.update();
                assert_eq!(app.world().resource::<AirWanted>().0, (row || aid) && supported, "row {row}, aid {aid}, supported {supported}");
            }
        }
        use crate::medium_air_grid::{volume_format, AIR_USAGES};
        use bevy::render::render_resource::TextureUsages;
        assert!(volume_format(AIR_USAGES, true, 1).is_ok());
        assert!(volume_format(AIR_USAGES, true, 0).unwrap_err().contains("no storage textures"));
        assert!(volume_format(AIR_USAGES, false, 4).is_err() && volume_format(TextureUsages::TEXTURE_BINDING, true, 4).is_err());
    }

    /// With no graphics settings and an aid already decided, the system leaves a value set by
    /// hand alone (the grid's tests set the demand themselves after the first update).
    #[test]
    fn a_demand_set_by_hand_stays_without_graphics_settings() {
        let mut app = App::new();
        app.add_plugins(AirDemandPlugin);
        app.update();
        app.world_mut().insert_resource(AirWanted(true));
        app.update();
        app.update();
        assert!(app.world().resource::<AirWanted>().0);
    }

    /// The slider's part is strength / 2: 0, 100 % (the default) and 200 % at its ends and
    /// middle; on a step of 10 % each step is a twentieth of the slider.
    #[test]
    fn the_strength_row_maps_its_part_to_0_to_200_percent() {
        assert_eq!(AirStrengthSetting::default().part(), 0.5);
        assert_eq!(AirStrengthSetting::from_part(0.0).0, 0.0);
        assert_eq!(AirStrengthSetting::from_part(0.5).0, 1.0);
        assert_eq!(AirStrengthSetting::from_part(1.0).0, 2.0);
        assert_eq!(AirStrengthSetting::from_part(2.0).0, 2.0);
        assert_eq!(AirStrengthSetting::from_part(f32::NAN), AirStrengthSetting::default());
        assert_eq!(AirStrengthSetting(3.0).part(), 1.0);
        assert_eq!(AirStrengthSetting::shown(0.5), "100%");
        assert_eq!(AirStrengthSetting::shown(0.05), "10%");
        assert_eq!(AirStrengthSetting::shown(1.0), "200%");
        assert_eq!(AirStrengthSetting::shown(0.0), "0%");
    }
}
