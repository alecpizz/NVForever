//! Developer aids that stay out of release builds (docs/design/graphics-enhanced.md §3.23; the
//! launcher's `update::test_aids` is the same idea). They exist in tests, in debug builds and in a
//! build with the `dev-aids` feature:
//!
//! ```text
//! cargo build --release -p nvforever --features dev-aids
//! ```
//!
//! An aid's variable is read through [`var`] (or `var_os`), which looks at the environment only when
//! [`ON`]; in a release build it is `None` without a look, so the aid keeps its default. Code only an
//! aid uses (a tune file's follower, `--preview-weather`) is compiled under the same condition,
//! `#[cfg(any(test, debug_assertions, feature = "dev-aids"))]`.
//!
//! The debug switches older than this module are read in every build, as is `NVF_PLAY_TEST_*`
//! (nvf-play's test_aid.rs) but for `NVF_PLAY_TEST_SHOT_DIFF`, which nvf-play compiles in under the
//! same condition (its own `dev-aids` feature, which this crate's turns on).

/// Whether this build has the developer aids.
pub const ON: bool = cfg!(any(test, debug_assertions, feature = "dev-aids"));

/// An aid's variable as text; `None` in a build without the aids.
#[inline]
pub fn var(name: &str) -> Option<String> {
    if ON { std::env::var(name).ok() } else { None }
}

/// An aid's variable as the system gives it (a path).
#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
pub fn var_os(name: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(name)
}

#[cfg(test)]
mod tests {
    /// The volumetric medium's and its sources' aids, and the made bounce light volume's, come
    /// through this module alone.
    #[test]
    fn the_volumetric_aids_read_the_environment_here_only() {
        let read = ["env", "::var"].concat();
        for (name, text) in [
            ("medium.rs", include_str!("medium.rs")),
            ("medium_air.rs", include_str!("medium_air.rs")),
            ("medium_air_compose.rs", include_str!("medium_air_compose.rs")),
            ("medium_air_demand.rs", include_str!("medium_air_demand.rs")),
            ("medium_air_grid.rs", include_str!("medium_air_grid.rs")),
            ("medium_air_light.rs", include_str!("medium_air_light.rs")),
            ("rtgi_volume/made.rs", include_str!("rtgi_volume/made.rs")),
            ("solid_cores.rs", include_str!("solid_cores.rs")),
            ("volumetric_clouds.rs", include_str!("volumetric_clouds.rs")),
            ("volumetric_clouds_light.rs", include_str!("volumetric_clouds_light.rs")),
            ("volumetric_clouds_lightvol.rs", include_str!("volumetric_clouds_lightvol.rs")),
            ("volumetric_clouds_tune.rs", include_str!("volumetric_clouds_tune.rs")),
            ("volumetric_clouds_wisp.rs", include_str!("volumetric_clouds_wisp.rs")),
        ] {
            assert!(!text.contains(&read), "{name}: use crate::dev_aids::var");
        }
        assert!(!include_str!("sky.rs").contains(&[&read, "(\"NVF_PLAY_DEBUG_FAIR_SUN"].concat()));
        assert!(!include_str!("cloud_shadows.rs").contains(&[&read, "(\"NVF_PLAY_DEBUG_CLOUD_SHADOWS_SOURCE"].concat()));
    }
}
