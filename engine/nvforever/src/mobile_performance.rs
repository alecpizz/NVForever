//! Android's explicit performance choices reuse the canonical renderer/settings rows.
//! Manual choices persist; opt-in thermal limits affect only the effective workload.
use crate::frame_pace::{self, FramePace};
use crate::graphics::Graphics;
use crate::upscaler::{UpscaleQuality, UpscalerChoice};
use bevy::prelude::*;
use nvf_play::settings::{self, ChoiceList, Setting, Settings, choice_part};

pub(crate) const PRESET_KEY: &str = "mobile-performance";
#[path = "mobile_performance_policy.rs"]
mod policy;
use policy::{Level, Policy, Sample};

pub(crate) const RESOLUTION_KEY: &str = "mobile-resolution";
const CUSTOM: &str = "Custom";
/// Desktop smoke runs opt in only to the shared settings routes, not Android runtime.
pub(crate) const TEST_ENV: &str = "NVF_PLAY_TEST_ANDROID_PERFORMANCE";
pub fn enabled() -> bool {
    cfg!(target_os = "android") || std::env::var(TEST_ENV).is_ok_and(|v| v == "1")
}

const FSR_UNAVAILABLE: &str =
    "Needs FSR 3.1 support from the current Vulkan renderer; native rendering stays available.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rendering {
    Native,
    Quality,
    Balanced,
    Performance,
}
impl Rendering {
    const ALL: [Self; 4] = [
        Self::Native,
        Self::Quality,
        Self::Balanced,
        Self::Performance,
    ];
    fn name(self) -> &'static str {
        match self {
            Self::Native => "Native (100%)",
            Self::Quality => "67% (FSR)",
            Self::Balanced => "59% (FSR)",
            Self::Performance => "50% (FSR)",
        }
    }
    fn named(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|r| r.name().eq_ignore_ascii_case(name.trim()))
    }
    fn request(self) -> UpscalerChoice {
        match self {
            Self::Native => UpscalerChoice::Off,
            Self::Quality => UpscalerChoice::Fsr(UpscaleQuality::Quality),
            Self::Balanced => UpscalerChoice::Fsr(UpscaleQuality::Balanced),
            Self::Performance => UpscalerChoice::Fsr(UpscaleQuality::Performance),
        }
    }
    fn from_request(request: UpscalerChoice) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.request() == request)
    }
    fn unavailable(self, world: &World) -> Option<String> {
        (self != Self::Native
            && !world
                .get_resource::<crate::fsr::FsrSupport>()
                .is_some_and(|s| s.0))
        .then(|| FSR_UNAVAILABLE.to_string())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Preset {
    Battery,
    Balanced,
    Quality,
}
impl Preset {
    const ALL: [Self; 3] = [Self::Battery, Self::Balanced, Self::Quality];
    fn name(self) -> &'static str {
        match self {
            Self::Battery => "Battery",
            Self::Balanced => "Balanced",
            Self::Quality => "Quality",
        }
    }
    fn named(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|p| p.name().eq_ignore_ascii_case(name.trim()))
    }
    fn cap(self) -> u32 {
        if self == Self::Battery { 30 } else { 60 }
    }
    fn rendering(self) -> Rendering {
        match self {
            Self::Battery => Rendering::Performance,
            Self::Balanced => Rendering::Quality,
            Self::Quality => Rendering::Native,
        }
    }
}

fn rendering_chosen(world: &World) -> String {
    Rendering::from_request(world.resource::<Graphics>().upscaler)
        .map_or(CUSTOM, Rendering::name)
        .to_string()
}
fn matched_preset(cap: u32, requested: UpscalerChoice) -> &'static str {
    Preset::ALL
        .into_iter()
        .find(|p| p.cap() == cap && p.rendering().request() == requested)
        .map_or(CUSTOM, Preset::name)
}
fn preset_chosen(world: &World) -> String {
    let (Some(pace), Some(graphics)) = (
        world.get_resource::<FramePace>(),
        world.get_resource::<Graphics>(),
    ) else {
        return CUSTOM.into();
    };
    matched_preset(pace.cap, graphics.upscaler).to_string()
}
fn log_reading(
    pace: Res<FramePace>,
    graphics: Res<Graphics>,
    mut before: Local<Option<(u32, UpscalerChoice)>>,
) {
    let now = (pace.cap, graphics.upscaler);
    if *before == Some(now) {
        return;
    }
    *before = Some(now);
    info!(
        "mobile performance: {}; frame cap {}; requested world {}",
        matched_preset(now.0, now.1),
        now.0,
        Rendering::from_request(now.1).map_or(CUSTOM, Rendering::name)
    );
}

fn names(mut names: Vec<String>, current: String) -> Vec<String> {
    if current == CUSTOM {
        names.push(current);
    }
    names
}
fn row(world: &World, key: &str) -> Option<usize> {
    world.get_resource::<Settings>()?.index(key)
}

/// The real row routes mark user ownership, persist canonical settings and latch any
/// renderer pipeline change. No mobile shadow copy can override those rows on reload.
fn pick_rendering(world: &mut World, name: &str) -> bool {
    let Some(rendering) = Rendering::named(name) else {
        return false;
    };
    if rendering.unavailable(world).is_some() {
        return false;
    }
    let (Some(upscaler), Some(quality)) = (
        row(world, crate::settings::UPSCALER_KEY),
        row(world, crate::settings::QUALITY_KEY),
    ) else {
        return false;
    };
    let settings = world.resource::<Settings>();
    if settings.disabled_status(world, upscaler).is_some() {
        return false;
    }
    let backend = if rendering == Rendering::Native {
        "Off"
    } else {
        "FSR 3.1"
    };
    if settings
        .choice_unavailable(world, upscaler, backend)
        .is_some()
    {
        return false;
    }
    settings::pick(world, upscaler, backend);
    if let UpscalerChoice::Fsr(q) = rendering.request() {
        settings::pick(world, quality, q.name());
    }
    world.resource::<Graphics>().upscaler == rendering.request()
}
/// A synchronous request to the canonical row, not another persisted cap.
struct FrameLimit {
    row: usize,
    name: Option<String>,
    part: f32,
}
impl FrameLimit {
    fn for_cap(world: &World, cap: u32) -> Option<Self> {
        let index = frame_pace::CAPS.iter().position(|&value| value == cap)?;
        let settings = world.get_resource::<Settings>()?;
        let row = settings.index("frame-cap")?;
        if settings.disabled_status(world, row).is_some() { return None; }
        let name = if let Some(choices) = settings.choices(row) {
            let name = if cap == 0 { "Off".to_string() } else { cap.to_string() };
            if !(choices.names)(world).contains(&name)
                || settings.choice_unavailable(world, row, &name).is_some() { return None; }
            Some(name)
        } else {
            settings.list()[row].slider?;
            None
        };
        Some(Self { row, name, part: choice_part(index, frame_pace::CAPS.len()) })
    }
    fn take(self, world: &mut World) -> bool {
        if let Some(name) = self.name {
            // pick reports an unchanged choice as false; the old slider request
            // succeeded without writing in that case, and the aliases still do.
            if world.resource::<Settings>().choices(self.row)
                .is_some_and(|c| (c.chosen)(world) == name) { return true; }
            settings::pick(world, self.row, &name)
        } else {
            settings::set_slider(world, self.row, self.part).is_some()
        }
    }
}

fn pick_preset(world: &mut World, name: &str) -> bool {
    let Some(preset) = Preset::named(name) else {
        return false;
    };
    let Some(cap) = FrameLimit::for_cap(world, preset.cap()) else {
        return false;
    };
    // Refuse unsupported rendering before changing either part of the preset.
    if !pick_rendering(world, preset.rendering().name()) {
        return false;
    }
    cap.take(world)
}
fn rendering_unavailable(world: &World, name: &str) -> Option<String> {
    Rendering::named(name)?.unavailable(world)
}
fn preset_unavailable(world: &World, name: &str) -> Option<String> {
    Preset::named(name)?.rendering().unavailable(world)
}
fn rendering_label(world: &World, name: &str) -> String {
    if rendering_unavailable(world, name).is_some() && rendering_chosen(world) == name {
        format!("{name} (native fallback)")
    } else {
        name.to_string()
    }
}
fn preset_label(world: &World, name: &str) -> String {
    if preset_unavailable(world, name).is_some() && preset_chosen(world) == name {
        format!("{name} (native fallback)")
    } else {
        name.to_string()
    }
}

pub fn register(app: &mut App) {
    app.init_resource::<ThermalSettings>()
        .init_resource::<ThermalRuntime>()
        .init_resource::<Time<Real>>()
        .add_message::<bevy::window::AppLifecycle>()
        .add_message::<bevy::window::WindowFocused>()
        .add_systems(
            Update,
            (
                thermal_update.before(crate::upscaler::follow_request),
                log_reading,
            ),
        );
    let setting = |key, label, note| Setting {
        key,
        group: "Display",
        label,
        note,
        switches: "",
        get: |_| false,
        set: |_, _| {},
        slider: None,
    };
    settings::add_choice_list(
        app,
        setting(
            PRESET_KEY,
            "Mobile preset",
            "Battery: 30 FPS/50% world; Balanced: 60 FPS/67%; Quality: 60 FPS/native. Reduced resolutions need FSR; menus stay sharp and other graphics choices stay unchanged.",
        ),
        ChoiceList {
            names: |w| {
                names(
                    Preset::ALL.iter().map(|p| p.name().to_string()).collect(),
                    preset_chosen(w),
                )
            },
            chosen: preset_chosen,
            pick: pick_preset,
            save: preset_chosen,
            // Persisted canonical rows are the truth, regardless of alias read order.
            load: |_, name| name.eq_ignore_ascii_case(CUSTOM) || Preset::named(name).is_some(),
            ask_keep: false,
        },
    );
    settings::set_choice_unavailable(app, PRESET_KEY, preset_unavailable);
    settings::set_choice_labels(app, PRESET_KEY, preset_label);
    settings::add_choice_list(
        app,
        setting(
            RESOLUTION_KEY,
            "Mobile world resolution",
            "World width and height as a percentage of the display. Smaller choices use FSR and need renderer support. Native uses full resolution. Menus keep the display's resolution.",
        ),
        ChoiceList {
            names: |w| {
                names(
                    Rendering::ALL
                        .iter()
                        .map(|r| r.name().to_string())
                        .collect(),
                    rendering_chosen(w),
                )
            },
            chosen: rendering_chosen,
            pick: pick_rendering,
            save: rendering_chosen,
            load: |_, name| name.eq_ignore_ascii_case(CUSTOM) || Rendering::named(name).is_some(),
            ask_keep: false,
        },
    );
    settings::set_choice_unavailable(app, RESOLUTION_KEY, rendering_unavailable);
    settings::set_choice_labels(app, RESOLUTION_KEY, rendering_label);
    settings::add_choice_list(
        app,
        setting(
            "mobile-fps",
            "Mobile frame limit",
            "30 FPS reduces power use; 60 FPS makes motion smoother. Actual frame rate depends on the scene and phone.",
        ),
        ChoiceList {
            names: |w| names(vec!["30".into(), "60".into()], fps_chosen(w)),
            chosen: fps_chosen,
            pick: pick_fps,
            save: fps_chosen,
            load: |_, value| matches!(value, "30" | "60" | CUSTOM),
            ask_keep: false,
        },
    );
    settings::add_setting(
        app,
        Setting {
            key: "mobile-thermal-auto",
            group: "Display",
            label: "Automatic thermal adjustment",
            note: "While hot, temporarily lower the frame limit and FSR quality, then recover when cool. Saved choices stay.",
            switches: "",
            get: |w| w.resource::<ThermalSettings>().automatic,
            set: |w, automatic| w.resource_mut::<ThermalSettings>().automatic = automatic,
            slider: None,
        },
    );
    settings::add_choice_list(
        app,
        setting(
            "mobile-thermal-status",
            "Thermal adjustment status",
            "Shows the temporary limit. Native rendering stays native; automatic resolution changes apply only to FSR. Phone thermal reports may be incomplete.",
        ),
        ChoiceList {
            names: |w| vec![thermal_status(w)],
            chosen: thermal_status,
            pick: |_, _| false,
            save: |_| "Status".into(),
            load: |_, _| true,
            ask_keep: false,
        },
    );
}

#[derive(Resource, Default)]
struct ThermalSettings {
    automatic: bool,
}

#[derive(Resource)]
pub(crate) struct ThermalRuntime {
    policy: Policy,
    sample: Option<Sample>,
    #[cfg(target_os = "android")]
    last_poll: Option<std::time::Duration>,
    suspended: bool,
    running: bool,
    focused: bool,
}
impl Default for ThermalRuntime {
    fn default() -> Self {
        Self {
            policy: Policy::default(),
            sample: None,
            #[cfg(target_os = "android")]
            last_poll: None,
            suspended: false,
            running: true,
            focused: true,
        }
    }
}
impl ThermalRuntime {
    pub(crate) fn render_request(
        &self,
        requested: UpscalerChoice,
        supported: bool,
    ) -> UpscalerChoice {
        let UpscalerChoice::Fsr(quality) = requested else {
            return requested;
        };
        if !supported {
            return requested;
        }
        let rank = match quality {
            UpscaleQuality::NativeAa => 0,
            UpscaleQuality::Quality => 1,
            UpscaleQuality::Balanced => 2,
            UpscaleQuality::Performance => 3,
            UpscaleQuality::UltraPerformance => 4,
        };
        let quality = match self.policy.fsr_rank(rank) {
            0 => UpscaleQuality::NativeAa,
            1 => UpscaleQuality::Quality,
            2 => UpscaleQuality::Balanced,
            3 => UpscaleQuality::Performance,
            _ => UpscaleQuality::UltraPerformance,
        };
        UpscalerChoice::Fsr(quality)
    }
}

/// The actual limiter uses this without changing the requested/persisted FramePace.
/// Absent on desktop: the original cap is returned exactly.
pub(crate) fn runtime_cap(world: &World, requested: u32) -> u32 {
    world
        .get_resource::<ThermalRuntime>()
        .map_or(requested, |r| r.policy.cap(requested))
}

fn thermal_update(
    time: Res<Time<Real>>,
    settings: Res<ThermalSettings>,
    mut runtime: ResMut<ThermalRuntime>,
    mut lifecycle: MessageReader<bevy::window::AppLifecycle>,
    mut focus: MessageReader<bevy::window::WindowFocused>,
) {
    let mut boundary = false;
    for event in lifecycle.read() {
        boundary = true;
        match event {
            bevy::window::AppLifecycle::Suspended => runtime.running = false,
            bevy::window::AppLifecycle::Running => runtime.running = true,
            _ => {}
        }
    }
    for event in focus.read() {
        boundary = true;
        runtime.focused = event.focused;
    }
    runtime.suspended = !runtime.running || !runtime.focused;
    if boundary {
        runtime.policy = Policy::default();
        runtime.sample = None;
        #[cfg(target_os = "android")]
        {
            runtime.last_poll = None;
        }
    }
    let elapsed = time.elapsed();
    #[cfg(target_os = "android")]
    if !runtime.suspended
        && runtime
            .last_poll
            .is_none_or(|last| elapsed.saturating_sub(last).as_secs() >= 1)
    {
        runtime.sample = crate::android::activity_string("performanceState", None)
            .ok()
            .and_then(|s| Sample::parse(&s));
        runtime.last_poll = Some(elapsed);
    }
    let sample = if runtime.suspended {
        None
    } else {
        runtime.sample
    };
    let previous = runtime.policy.level;
    runtime.policy.update(
        elapsed.as_millis().try_into().unwrap_or(u64::MAX),
        settings.automatic,
        sample,
    );
    if runtime.policy.level != previous {
        info!("mobile thermal adjustment: {:?}", runtime.policy.level);
    }
}

fn fps_chosen(world: &World) -> String {
    match world.resource::<FramePace>().cap {
        30 => "30",
        60 => "60",
        _ => CUSTOM,
    }
    .into()
}
fn pick_fps(world: &mut World, value: &str) -> bool {
    let cap = match value {
        "30" => 30,
        "60" => 60,
        _ => return false,
    };
    FrameLimit::for_cap(world, cap).is_some_and(|request| request.take(world))
}
fn thermal_status(world: &World) -> String {
    let runtime = world.resource::<ThermalRuntime>();
    let settings = world.resource::<ThermalSettings>();
    if !settings.automatic {
        return "Off - manual settings".into();
    }
    if runtime.suspended {
        return "Paused - manual settings".into();
    }
    if !runtime.sample.is_some_and(Sample::usable) {
        return "Feedback unavailable - manual settings".into();
    }
    let cap = runtime_cap(world, world.resource::<FramePace>().cap);
    match runtime.policy.level {
        Level::Manual => "Monitoring - selected settings".into(),
        level => format!(
            "{} - {cap} FPS; world {}",
            if level == Level::Hot { "Hot" } else { "Warm" },
            Rendering::from_request(crate::upscaler::effective(world))
                .map_or(CUSTOM, Rendering::name)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame_pace::DisplaySync;
    use crate::graphics::{self, MsaaSetting};
    use nvf_play::settings::{GivenSwitches, SettingsFile};

    fn app(supported: bool) -> App {
        let mut app = App::new();
        app.insert_resource(Graphics::preset(graphics::Preset::Vanilla))
            .insert_resource(MsaaSetting(Msaa::Sample8))
            .insert_resource(FramePace {
                sync: DisplaySync::Off,
                cap: 0,
            })
            .insert_resource(crate::fsr::FsrSupport(supported))
            .insert_resource(crate::frame_gen::FrameGeneration {
                kind: crate::frame_gen::FrameGenKind::Off,
            })
            .init_resource::<crate::frame_gen::DlssgFrames>()
            .insert_resource(crate::anti_lag::LatencyReduction {
                on: false,
                by_frame_generation: false,
            })
            .init_resource::<GivenSwitches>();
        crate::settings::register_mobile_test_rows(&mut app);
        register(&mut app);
        app
    }
    fn pick(app: &mut App, key: &str, name: &str) -> bool {
        let i = row(app.world(), key).unwrap();
        settings::pick(app.world_mut(), i, name)
    }
    fn set_cap(app: &mut App, cap: u32) {
        let row = row(app.world(), "frame-cap").unwrap();
        if let Some(choices) = app.world().resource::<Settings>().choices(row) {
            let name = if cap == 0 { "Off".to_string() } else { cap.to_string() };
            assert!((choices.names)(app.world()).contains(&name));
            if (choices.chosen)(app.world()) != name {
                assert!(settings::pick(app.world_mut(), row, &name));
            }
        } else {
            let i = frame_pace::CAPS.iter().position(|&v| v == cap).unwrap();
            assert!(
                settings::set_slider(app.world_mut(), row, choice_part(i, frame_pace::CAPS.len()))
                    .is_some()
            );
        }
    }
    #[test]
    fn mobile_presets_use_real_rows_and_individual_edits_become_custom() {
        let mut app = app(true);
        let bloom = app.world().resource::<Graphics>().bloom;
        assert!(pick(&mut app, PRESET_KEY, "Battery"));
        assert_eq!(app.world().resource::<FramePace>().cap, 30);
        assert_eq!(
            app.world().resource::<Graphics>().upscaler,
            UpscalerChoice::Fsr(UpscaleQuality::Performance)
        );
        assert_eq!(preset_chosen(app.world()), "Battery");
        assert_eq!(
            app.world().resource::<FramePace>().sync,
            DisplaySync::Off,
            "a preset does not replace explicit display sync"
        );
        assert_eq!(
            app.world().resource::<MsaaSetting>().0,
            Msaa::Sample8,
            "AA preference remains available when native rendering returns"
        );
        assert_eq!(app.world().resource::<Graphics>().bloom, bloom);
        set_cap(&mut app, 60);
        assert_eq!(preset_chosen(app.world()), CUSTOM);
        assert!(pick(&mut app, RESOLUTION_KEY, "67% (FSR)"));
        assert_eq!(preset_chosen(app.world()), "Balanced");
        assert!(pick(&mut app, RESOLUTION_KEY, "59% (FSR)"));
        assert_eq!(preset_chosen(app.world()), CUSTOM);
        assert!(pick(&mut app, PRESET_KEY, "Quality"));
        assert_eq!(
            app.world().resource::<Graphics>().upscaler,
            UpscalerChoice::Off
        );
        assert_eq!(app.world().resource::<FramePace>().cap, 60);
    }
    #[test]
    fn unsupported_fsr_refuses_the_whole_preset_before_cap_or_render_changes() {
        let mut app = app(false);
        set_cap(&mut app, 120);
        let preset = row(app.world(), PRESET_KEY).unwrap();
        assert!(
            app.world()
                .resource::<Settings>()
                .choice_unavailable(app.world(), preset, "Battery")
                .is_some()
        );
        assert!(!pick(&mut app, PRESET_KEY, "Battery"));
        assert!(!pick_rendering(app.world_mut(), "50% (FSR)"));
        assert_eq!(app.world().resource::<FramePace>().cap, 120);
        assert_eq!(
            app.world().resource::<Graphics>().upscaler,
            UpscalerChoice::Off
        );
        assert!(
            pick(&mut app, PRESET_KEY, "Quality"),
            "native rendering is still a usable choice"
        );
        assert_eq!(app.world().resource::<FramePace>().cap, 60);
    }
    #[test]
    fn registration_and_saved_alias_names_never_enforce_over_custom_preferences() {
        let mut app = app(true);
        set_cap(&mut app, 120);
        assert!(pick(&mut app, RESOLUTION_KEY, "59% (FSR)"));
        let before = app.world().resource::<Graphics>().upscaler;
        // The same callbacks the shared INI reader calls; an old aggregate name is metadata.
        let settings = app.world().resource::<Settings>();
        let preset = settings
            .choices(settings.index(PRESET_KEY).unwrap())
            .unwrap();
        let rendering = settings
            .choices(settings.index(RESOLUTION_KEY).unwrap())
            .unwrap();
        assert!((preset.load)(app.world_mut(), "Battery"));
        assert!((rendering.load)(app.world_mut(), "50% (FSR)"));
        assert_eq!(app.world().resource::<FramePace>().cap, 120);
        assert_eq!(app.world().resource::<Graphics>().upscaler, before);
        assert_eq!(preset_chosen(app.world()), CUSTOM);
    }
    #[test]
    fn unsupported_stored_request_is_named_as_native_fallback_without_rewriting_it() {
        let mut app = app(false);
        app.world_mut().resource_mut::<Graphics>().upscaler =
            UpscalerChoice::Fsr(UpscaleQuality::Performance);
        app.world_mut().resource_mut::<FramePace>().cap = 30;
        let settings = app.world().resource::<Settings>();
        let r = settings.index(RESOLUTION_KEY).unwrap();
        let p = settings.index(PRESET_KEY).unwrap();
        assert!(
            settings
                .choice_label(app.world(), r, "50% (FSR)")
                .contains("native fallback")
        );
        assert!(
            settings
                .choice_label(app.world(), p, "Battery")
                .contains("native fallback")
        );
        assert_eq!(
            app.world().resource::<Graphics>().upscaler,
            UpscalerChoice::Fsr(UpscaleQuality::Performance)
        );
    }
    #[test]
    fn unknown_or_custom_choice_does_not_mutate_user_settings() {
        let mut app = app(true);
        for name in ["Ludicrous", "Custom", ""] {
            assert!(!pick_preset(app.world_mut(), name));
            assert!(!pick_rendering(app.world_mut(), name));
        }
        assert_eq!(app.world().resource::<FramePace>().cap, 0);
        assert_eq!(
            app.world().resource::<Graphics>().upscaler,
            UpscalerChoice::Off
        );
    }
    #[test]
    fn canonical_preferences_roundtrip_through_ini_despite_stale_mobile_names() {
        let dir =
            std::env::temp_dir().join(format!("nvf-mobile-performance-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("NVForever.ini");
        let mut first = app(true);
        first.insert_resource(SettingsFile(Some(path.clone())));
        assert!(pick(&mut first, PRESET_KEY, "Battery"));
        let saved = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            fnv_install::ini_get(&saved, "Settings", "upscaler"),
            Some("FSR 3.1".into())
        );
        assert_eq!(
            fnv_install::ini_get(&saved, "Settings", "upscaler-quality"),
            Some("Performance".into())
        );
        // Reload only these canonical and aggregate rows. Other settings have global
        // library selectors unrelated to this test and must not be replayed here.
        let focused = format!(
            "[Settings]\n{}",
            [
                "upscaler",
                "upscaler-quality",
                "frame-cap",
                PRESET_KEY,
                RESOLUTION_KEY
            ]
            .into_iter()
            .map(|key| format!(
                "{key}={}\n",
                fnv_install::ini_get(&saved, "Settings", key).unwrap()
            ))
            .collect::<String>()
        );
        std::fs::write(&path, &focused).unwrap();
        let mut loaded = app(true);
        loaded.insert_resource(SettingsFile(Some(path.clone())));
        settings::apply_stored(loaded.world_mut());
        assert_eq!(preset_chosen(loaded.world()), "Battery");
        // A person changes canonical values; aggregate names still say Battery/50%.
        let mut changed = fnv_install::ini_set(&focused, "Settings", "upscaler", "Off");
        let index = frame_pace::CAPS.iter().position(|&v| v == 60).unwrap();
        changed = fnv_install::ini_set(
            &changed,
            "Settings",
            "frame-cap",
            &choice_part(index, frame_pace::CAPS.len()).to_string(),
        );
        std::fs::write(&path, changed).unwrap();
        let mut edited = app(false);
        edited.insert_resource(SettingsFile(Some(path.clone())));
        settings::apply_stored(edited.world_mut());
        assert_eq!(
            edited.world().resource::<Graphics>().upscaler,
            UpscalerChoice::Off
        );
        assert_eq!(edited.world().resource::<FramePace>().cap, 60);
        assert_eq!(preset_chosen(edited.world()), "Quality");
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    fn feedback(app: &mut App, milliseconds: u64, status: i32, headroom: i32) {
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_millis(milliseconds));
        app.world_mut().resource_mut::<ThermalRuntime>().sample = Some(Sample {
            active: true,
            status,
            headroom_milli: headroom,
            age_ms: 0,
        });
        app.update();
    }
    fn automatic(app: &mut App) {
        let row = row(app.world(), "mobile-thermal-auto").unwrap();
        assert_eq!(settings::flip(app.world_mut(), row), Some(true));
    }
    #[test]
    fn combined_app_thermal_adjustment_changes_real_upscaler_status_but_not_requested_settings() {
        let mut app = app(true);
        app.add_systems(Update, crate::upscaler::follow_request);
        assert!(pick(&mut app, PRESET_KEY, "Balanced"));
        feedback(&mut app, 0, 3, 1100);
        assert_eq!(
            runtime_cap(app.world(), 60),
            60,
            "no consent: no automatic limit"
        );
        assert_eq!(
            app.world()
                .resource::<crate::upscaler::UpscalerStatus>()
                .effective(),
            UpscalerChoice::Fsr(UpscaleQuality::Quality)
        );
        automatic(&mut app);
        feedback(&mut app, 1, 3, 1100);
        assert_eq!(runtime_cap(app.world(), 60), 30);
        assert_eq!(
            app.world()
                .resource::<crate::upscaler::UpscalerStatus>()
                .effective(),
            UpscalerChoice::Fsr(UpscaleQuality::Performance)
        );
        assert_eq!(app.world().resource::<FramePace>().cap, 60);
        assert_eq!(
            app.world().resource::<Graphics>().upscaler,
            UpscalerChoice::Fsr(UpscaleQuality::Quality)
        );
        assert_eq!(preset_chosen(app.world()), "Balanced");
        assert_eq!(
            app.world()
                .resource::<ThermalRuntime>()
                .render_request(UpscalerChoice::Off, true),
            UpscalerChoice::Off,
            "native never automatically enables FSR"
        );
        let dlss = UpscalerChoice::Dlss(UpscaleQuality::Quality);
        assert_eq!(
            app.world()
                .resource::<ThermalRuntime>()
                .render_request(dlss, true),
            dlss,
            "desktop backend choices are preserved"
        );
        app.world_mut().resource_mut::<crate::fsr::FsrSupport>().0 = false;
        app.update();
        assert_eq!(
            app.world()
                .resource::<crate::upscaler::UpscalerStatus>()
                .effective(),
            UpscalerChoice::Off,
            "unsupported FSR uses the existing native fallback"
        );
        assert_eq!(
            runtime_cap(app.world(), 60),
            30,
            "FPS adjustment remains usable without FSR"
        );
        app.world_mut().resource_mut::<crate::fsr::FsrSupport>().0 = true;
        assert!(pick(&mut app, "mobile-fps", "30"));
        assert!(pick(&mut app, RESOLUTION_KEY, "50% (FSR)"));
        feedback(&mut app, 1, 3, 1100);
        feedback(&mut app, 1, 0, 600);
        for _ in 0..60 {
            feedback(&mut app, 1000, 0, 600);
        }
        feedback(&mut app, 1, 0, 600);
        for _ in 0..60 {
            feedback(&mut app, 1000, 0, 600);
        }
        assert_eq!(
            app.world().resource::<ThermalRuntime>().policy.level,
            Level::Manual
        );
        assert_eq!(
            app.world().resource::<FramePace>().cap,
            30,
            "recovery honors the latest manual selection"
        );
        assert_eq!(
            app.world().resource::<Graphics>().upscaler,
            UpscalerChoice::Fsr(UpscaleQuality::Performance)
        );
    }
    #[test]
    fn combined_app_lifecycle_clears_override_and_focus_cannot_resume_suspended_activity() {
        let mut app = app(true);
        automatic(&mut app);
        set_cap(&mut app, 60);
        feedback(&mut app, 0, 3, -1);
        assert_eq!(runtime_cap(app.world(), 60), 30);
        app.world_mut()
            .write_message(bevy::window::AppLifecycle::Suspended);
        app.world_mut().write_message(bevy::window::WindowFocused {
            window: Entity::PLACEHOLDER,
            focused: true,
        });
        app.update();
        assert_eq!(runtime_cap(app.world(), 60), 60);
        assert!(app.world().resource::<ThermalRuntime>().suspended);
        app.world_mut()
            .write_message(bevy::window::AppLifecycle::Running);
        app.update();
        assert_eq!(
            runtime_cap(app.world(), 60),
            60,
            "resume needs new feedback"
        );
        feedback(&mut app, 1, 2, -1);
        feedback(&mut app, 4999, 2, -1);
        assert_eq!(
            runtime_cap(app.world(), 60),
            60,
            "warm hysteresis restarts after lifecycle boundary"
        );
        feedback(&mut app, 1, 2, -1);
        assert_eq!(runtime_cap(app.world(), 60), 45);
    }
    #[test]
    fn consent_roundtrip_keeps_canonical_settings_during_hot_feedback() {
        let dir =
            std::env::temp_dir().join(format!("nvf-thermal-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("NVForever.ini");
        let mut app = app(true);
        app.insert_resource(SettingsFile(Some(path.clone())));
        assert!(pick(&mut app, PRESET_KEY, "Balanced"));
        automatic(&mut app);
        feedback(&mut app, 0, 3, 1100);
        // Actual manual edits write settings; reselecting 60 would intentionally be a no-op.
        // The final write must persist the latest baseline, not the hot override.
        assert!(pick(&mut app, "mobile-fps", "30"));
        assert!(pick(&mut app, "mobile-fps", "60"));
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            fnv_install::ini_get(&text, "Settings", "mobile-thermal-auto").as_deref(),
            Some("1")
        );
        assert_eq!(
            fnv_install::ini_get(&text, "Settings", "mobile-performance").as_deref(),
            Some("Balanced")
        );
        let mut loaded = self::app(false);
        loaded.insert_resource(SettingsFile(Some(path.clone())));
        settings::apply_stored(loaded.world_mut());
        assert!(loaded.world().resource::<ThermalSettings>().automatic);
        assert_eq!(loaded.world().resource::<FramePace>().cap, 60);
        assert_eq!(
            loaded.world().resource::<Graphics>().upscaler,
            UpscalerChoice::Fsr(UpscaleQuality::Quality)
        );
        assert_eq!(
            runtime_cap(loaded.world(), 60),
            60,
            "no old thermal level is persisted"
        );
        assert_eq!(
            loaded
                .world()
                .resource::<ThermalRuntime>()
                .render_request(UpscalerChoice::Off, true),
            UpscalerChoice::Off
        );
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn canonical_frame_limit_positive_precedes_mobile_list_aliases_and_same_cap_noop() {
        let mut app = app(true);
        let cap = row(app.world(), "frame-cap").unwrap();
        let choices = app.world().resource::<Settings>().choices(cap)
            .expect("the integrated Main provider is the actual named frame-limit list");
        assert_eq!((choices.names)(app.world()), ["Off", "30", "60", "90", "120", "144"]);
        assert!(settings::pick(app.world_mut(), cap, "30"), "canonical provider positive first");
        assert_eq!(app.world().resource::<FramePace>().cap, 30);
        assert!(pick(&mut app, "mobile-fps", "60"));
        assert_eq!(app.world().resource::<FramePace>().cap, 60);
        assert!(pick_fps(app.world_mut(), "60"), "an unchanged canonical cap succeeds without another write");
        assert!(pick_preset(app.world_mut(), "Quality"), "native preset also accepts the unchanged named cap");
        assert_eq!(preset_chosen(app.world()), "Quality");
        assert_eq!(app.world().resource::<Graphics>().upscaler, UpscalerChoice::Off);
        assert_eq!(app.world().resource::<FramePace>().sync, DisplaySync::Off);
        assert_eq!(app.world().resource::<MsaaSetting>().0, Msaa::Sample8);
    }

    #[test]
    fn canonical_frame_limit_disabled_and_unavailable_refuse_before_preset_render_changes() {
        let mut app = app(true);
        settings::set_disabled_status(&mut app, "frame-cap", |_| Some("Managed for this test"));
        assert!(!pick_preset(app.world_mut(), "Battery"));
        assert!(!pick_fps(app.world_mut(), "60"));
        assert_eq!(app.world().resource::<FramePace>().cap, 0);
        assert_eq!(app.world().resource::<Graphics>().upscaler, UpscalerChoice::Off);
        let mut app = self::app(true);
        settings::set_choice_unavailable(&mut app, "frame-cap", |_, name| (name == "30").then(|| "Managed for this test".into()));
        assert!(!pick_preset(app.world_mut(), "Battery"));
        assert!(!pick_fps(app.world_mut(), "30"));
        assert_eq!(app.world().resource::<FramePace>().cap, 0);
        assert_eq!(app.world().resource::<Graphics>().upscaler, UpscalerChoice::Off);
        assert!(pick_fps(app.world_mut(), "60"));
        assert_eq!(app.world().resource::<FramePace>().cap, 60);
        assert!(!pick_fps(app.world_mut(), "Custom"));
        assert!(!pick_fps(app.world_mut(), "not a limit"));
        assert_eq!(app.world().resource::<FramePace>().cap, 60);
    }

    #[test]
    fn canonical_old_numeric_limit_loads_and_named_limit_saves_beside_stale_mobile_aliases() {
        struct File(PathBuf);
        impl Drop for File {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
                let _ = std::fs::remove_dir(self.0.parent().unwrap());
            }
        }
        use std::path::PathBuf;
        let dir = std::env::temp_dir().join(format!("nvf-mobile-list-limit-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let file = File(dir.join("NVForever.ini"));
        let input = "[Settings]\nframe-cap=0.2\nmobile-performance=Battery\nmobile-resolution=50% (FSR)\n[Unrelated]\nkeep=yes\n";
        std::fs::write(&file.0, input).unwrap();
        let mut loaded = app(false);
        loaded.insert_resource(SettingsFile(Some(file.0.clone())));
        settings::apply_stored(loaded.world_mut());
        assert_eq!(loaded.world().resource::<FramePace>().cap, 30);
        assert_eq!(loaded.world().resource::<Graphics>().upscaler, UpscalerChoice::Off,
            "stale mobile aggregate names are metadata, not commands during restore");
        assert_eq!(std::fs::read_to_string(&file.0).unwrap(), input, "reading never rewrites the old profile");
        assert!(pick(&mut loaded, "mobile-fps", "60"));
        let text = std::fs::read_to_string(&file.0).unwrap();
        assert_eq!(fnv_install::ini_get(&text, "Settings", "frame-cap").as_deref(), Some("60"));
        assert_eq!(fnv_install::ini_get(&text, "Unrelated", "keep").as_deref(), Some("yes"));
        let mut fresh = app(false);
        fresh.insert_resource(SettingsFile(Some(file.0.clone())));
        settings::apply_stored(fresh.world_mut());
        assert_eq!(fresh.world().resource::<FramePace>().cap, 60);
        assert_eq!(fresh.world().resource::<Graphics>().upscaler, UpscalerChoice::Off);
    }

    #[test]
    fn mobile_registered_notes_stay_within_the_two_hundred_character_budget() {
        let app = app(true);
        let settings = app.world().resource::<Settings>();
        for key in [PRESET_KEY, RESOLUTION_KEY, "mobile-fps", "mobile-thermal-auto", "mobile-thermal-status"] {
            let setting = &settings.list()[settings.index(key).unwrap()];
            assert!(settings::note_over_budget(setting.note).is_none(), "{key}: {:?}", settings::note_over_budget(setting.note));
            assert!(setting.note.chars().count() <= 200, "{key}: {} chars", setting.note.chars().count());
        }
        let auto = &settings.list()[settings.index("mobile-thermal-auto").unwrap()];
        assert!(auto.note.chars().count() <= 120, "mobile-thermal-auto should fit cleanly within two lines: {} chars", auto.note.chars().count());
    }
}
