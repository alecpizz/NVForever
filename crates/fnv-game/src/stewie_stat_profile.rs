//! Live NVForever critical-rule values exposed through Stewie queries.
use crate::Game;

impl Game<'_> {
    /// Bind application settings before startup/restore scripts. Queries read the
    /// same current fields as combat, with no second copy or saved authority.
    pub fn bind_stewie_stat_settings(&mut self, settings: &crate::tweaks::Tweaks) {
        self.tweaks = settings.clone();
        self.small.stewie_stat_settings_loaded = true;
    }

    pub(crate) fn loaded_stewie_stat_value(&self, key: &str, section: &str) -> Option<f64> {
        if !self.small.stewie_stat_settings_loaded {
            return None;
        }
        if section.eq_ignore_ascii_case("Tweaks")
            && key.eq_ignore_ascii_case("bCritChanceIgnoresFireRate")
        {
            return Some(f64::from(u8::from(self.tweaks.crit_ignores_fire_rate)));
        }
        if section.eq_ignore_ascii_case("Critical Hits")
            && key.eq_ignore_ascii_case("fCriticalDamageMult")
        {
            return Some(self.tweaks.crit_damage_scale);
        }
        // There is no separate loaded scale-enable intent in the engine slider.
        // Do not infer that native switch from an effective multiplier of one.
        None
    }
}
#[cfg(test)]
mod loaded_tests {
    use super::*;
    use crate::{game::GameConfig, GameData};
    use fnv_esm::{LoadOrder, Plugin, Schema};
    use fnv_script::{vm::XVal, FunctionTable};
    use fnv_sim::{FormStore, RefTable, SimConfig, World};

    #[test]
    fn loaded_aliases_read_current_consumer_fields_without_claiming_unrepresented_switches() {
        let bytes = crate::common::tes4();
        let plugins = [Plugin::parse(&bytes, "OwnedLoadedSettings.esm".into()).unwrap()];
        let lo = LoadOrder::new(&plugins).unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
        let schema = Schema::load_dir(root.join("fnv")).unwrap();
        let path = root.join("script/functions.json");
        let catalogue = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let forms = FormStore::build(&lo, &schema);
        let refs = RefTable::build(&lo, &schema);
        let data = GameData::build(
            &lo,
            &schema,
            FunctionTable::load(&path).unwrap(),
            &catalogue,
            &refs,
        );
        let mut game = Game::new(
            &data,
            World::new(&forms, &refs, SimConfig::default()),
            GameConfig::default(),
        );
        assert_eq!(
            game.loaded_stewie_stat_value("bCritChanceIgnoresFireRate", "Tweaks"),
            None,
            "headless/default games retain the original query route"
        );
        let settings = crate::tweaks::Tweaks {
            crit_ignores_fire_rate: true,
            crit_damage_scale: 1.5,
            ..Default::default()
        };
        game.bind_stewie_stat_settings(&settings);
        let query = |g: &mut Game<'_>, name: &str, key: &str| {
            g.run_query(
                &format!("Begin Function {{}}\nSetFunctionValue ({name} \"{key}\")\nEnd\n"),
                Default::default(),
            )
            .unwrap()
        };
        assert_eq!(
            query(
                &mut game,
                "StewVal",
                "bCritChanceIgnoresFireRate:tWEAKs"
            ),
            XVal::Num(1.0)
        );
        // Leading, trailing, and around-colon whitespace are NOT trimmed
        // [observed: w4h_08_stewie_get_value_whitespace].
        for padded in [
            " bCritChanceIgnoresFireRate:Tweaks",
            "bCritChanceIgnoresFireRate:Tweaks ",
            "bCritChanceIgnoresFireRate : Tweaks",
            " bCritChanceIgnoresFireRate : tWEAKs ",
        ] {
            assert_eq!(query(&mut game, "StewVal", padded), XVal::Num(0.0), "padded={padded:?}");
        }
        assert_eq!(
            query(
                &mut game,
                "StewieGetValue",
                "fCriticalDamageMult:Critical Hits"
            ),
            XVal::Num(1.5)
        );
        // Existing frame sync changes these fields directly; queries must not keep
        // a duplicate startup copy of their former values.
        game.tweaks.crit_ignores_fire_rate = false;
        game.tweaks.crit_damage_scale = 0.0;
        assert_eq!(
            query(
                &mut game,
                "StewieGetValue",
                "bCritChanceIgnoresFireRate:Tweaks"
            ),
            XVal::Num(0.0)
        );
        assert_eq!(
            query(&mut game, "StewVal", "fCriticalDamageMult:Critical Hits"),
            XVal::Num(0.0)
        );
        for (key, section) in [
            ("bScaleCriticalDamage", "Tweaks"),
            ("bMenuSearch", "Tweaks"),
            ("bCritChanceIgnoresFireRate", "Other"),
        ] {
            assert_eq!(game.loaded_stewie_stat_value(key, section), None);
        }
        assert_eq!(query(&mut game, "StewVal", "malformed"), XVal::Num(0.0));
        let snapshot = game.snapshot();
        let mut restored = Game::restore(
            &data,
            World::new(&forms, &refs, SimConfig::default()),
            GameConfig::default(),
            &snapshot,
        )
        .unwrap();
        assert_eq!(
            restored.loaded_stewie_stat_value("fCriticalDamageMult", "Critical Hits"),
            None,
            "loaded application authority is not save-baked"
        );
        restored.bind_stewie_stat_settings(&settings);
        assert_eq!(
            query(
                &mut restored,
                "StewVal",
                "fCriticalDamageMult:Critical Hits"
            ),
            XVal::Num(1.5)
        );
    }
}
