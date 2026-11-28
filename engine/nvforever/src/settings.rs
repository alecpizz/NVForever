//! The renderer's switches on the pause menu's settings page (`nvf_play::settings`, drawn
//! by `nvf_play::pause`; `--play` only): the on/off choices of the command line that can
//! change while the game runs. Each reads and writes the state its module uses, so a
//! switch starts as the command line left it, then as `NVForever.ini` stored it (kept by
//! `nvf_play::settings` like every switch of the page), except that in a test run a
//! switch given on the command line keeps its state (`given_switches`, #1382).
//!
//! Left on the command line: what is fixed when the game starts (`--no-npc-packages`,
//! `--no-actors`, FaceGen, weapons, `--no-lodgen`), most numbers (`--radius`; the volumes
//! are the Sound tab, `nvf_play::audio_out::register_settings`)
//! and the test aids. The draw distances are sliders here, each a part of the
//! command line's distance (`draw_distance.rs`), and the field of view a slider of whole
//! degrees; a row of 7 or fewer values (the frame-rate cap, the shadow distance, the
//! anti-aliasing, shadow-filter and shadow-resolution choices among them) is a list of its
//! choices [design: the menu overhaul], whose INI value is the choice's name, an older
//! slider's part still loading.

use crate::draw_distance::{self, DrawClass, DrawDistances};
use crate::fly::FlyCam;
use crate::frame_pace::{self, DisplaySync, FramePace};
use crate::graphics::{self, Graphics};
use crate::grass::GrassConfig;
use crate::foot_ik::FootIkSettings;
use crate::head_look::HeadLookSettings;
use crate::lips::LipState;
use crate::lod::LodState;
use crate::nav::NavState;
use crate::perf_overlay::{FpsCounter, Low};
use crate::player_body::PlayerBody;
use crate::sky::SkyState;
use crate::ssao::{AoLevel, AO_CHOICES};
use crate::upscaler::{DlssModel, UpscaleQuality, Upscaler, UpscalerChoice};
use crate::volumetric_clouds::{CloudLevel, CLOUD_CHOICES};
use crate::window_mode;
use crate::world::Streamer;
use crate::{Args, Overlay};
use bevy::prelude::*;
use nvf_play::pause::GROUP_MENUS;
use nvf_play::settings::{add_choice_list, add_setting, choice_index, ChoiceList, Setting, Slider};

// The groups the rows register with, one short word each; which tab and heading the pause
// menu shows each row under is nvf_play::settings_layout's table, not the group (#1953).
const GROUP_WORLD: &str = "World";
const GROUP_PICTURE: &str = "Display";
const GROUP_DEVELOPER: &str = "Debug";
const GROUP_GRAPHICS: &str = "Graphics";

/// The settings switches this run's command line gave (`nvf_play::settings::GivenSwitches`):
/// the `--name` of each argument, without the `--size` a run taking over a load is given
/// (the window's size then, handoff.rs). They win over the INI in a test run: a timed,
/// measured or pictured run, whose results must be those of its switches (#1382).
pub(crate) fn given_switches(args: &Args, line: &[String]) -> nvf_play::settings::GivenSwitches {
    let handoff = line.iter().any(|a| a == "--handoff" || a.starts_with("--handoff="));
    let given = line
        .iter()
        .filter(|a| a.starts_with("--"))
        .map(|a| a.split('=').next().unwrap_or(a).to_string())
        .filter(|a| !(handoff && a == "--size"))
        .collect();
    let win = args.run_seconds.is_some() || args.log_frame_times.is_some() || args.screenshot.is_some() || args.timelapse.is_some() || args.frame_json.is_some();
    nvf_play::settings::GivenSwitches { given, win }
}

/// List the renderer's switches: those whose module is part of this run.
pub fn register(app: &mut App) {
    let given = given_switches(app.world().resource::<Args>(), &crate::command_line());
    app.insert_resource(given);
    let has_lips = app.world().contains_resource::<LipState>();
    let has_heads = app.world().contains_resource::<HeadLookSettings>();
    let has_feet = app.world().contains_resource::<FootIkSettings>();
    let has_nav = app.world().contains_resource::<NavState>();
    let has_body = app.world().contains_resource::<PlayerBody>();
    if has_lips {
        add_setting(
            app,
            Setting {
                key: "lip-sync",
                group: GROUP_MENUS,
                label: "Lip sync",
                note: "On: a speaker's face moves with the line. Off: a face that is moving eases back to rest.",
                switches: "--no-lip",
                get: |w| w.resource::<LipState>().enabled,
                set: |w, on| w.resource_mut::<LipState>().enabled = on,
                slider: None,
            },
        );
    }
    if has_heads {
        add_setting(
            app,
            Setting {
                key: "head-tracking",
                group: GROUP_WORLD,
                label: "Head tracking",
                note: "On: actors turn their heads to the player and to others near them. Off: heads ease back to their animation.",
                switches: "--no-head-tracking",
                get: |w| w.resource::<HeadLookSettings>().enabled,
                set: |w, on| w.resource_mut::<HeadLookSettings>().enabled = on,
                slider: None,
            },
        );
    }
    if has_feet {
        add_setting(
            app,
            Setting {
                key: "foot-ik",
                group: GROUP_WORLD,
                label: "Feet follow the ground",
                note: "On: on slopes and steps actors bend their knees and set each foot on the ground under it. Off: the legs play the animation as it is.",
                switches: "--no-foot-ik",
                get: |w| w.resource::<FootIkSettings>().enabled,
                set: |w, on| w.resource_mut::<FootIkSettings>().enabled = on,
                slider: None,
            },
        );
    }
    add_setting(
        app,
        Setting {
            key: "sky-clock",
            group: GROUP_WORLD,
            label: "Sky follows the game clock",
            note: "On: the sky shows the game's time of day as it passes. Off: the sky keeps its hour while the game's clock runs on ([ and ] still set it).",
            switches: "--still-sky",
            get: |w| !w.resource::<Args>().still_sky,
            set: |w, on| w.resource_mut::<Args>().still_sky = !on,
            slider: None,
        },
    );
    // The resolution first, then full screen [owner 2026-10-18].
    register_window_display(app);
    if has_body {
        add_setting(
            app,
            Setting {
                key: "first-person-body",
                group: GROUP_PICTURE,
                label: "Body in first person",
                note: "On: in first person you see your own body and legs when you look down. Off: only the arms and the weapon are drawn, as in the original.",
                switches: "--first-person-body",
                get: |w| w.resource::<PlayerBody>().first_person_body,
                set: |w, on| w.resource_mut::<PlayerBody>().first_person_body = on,
                slider: None,
            },
        );
        // Advanced Movement's visual switches (NVF Features › Visual Only; nvf_play `player.rs`
        // keeps the rest) [design: the owner, 2026-11-15]. A saved `weapon-sway` or `head-bob`
        // (the rows they took over from) is read as theirs.
        add_setting(
            app,
            Setting {
                key: "weapon-inertia",
                group: GROUP_PICTURE,
                label: "Weapon Inertia",
                note: "On: in first person the arms and weapon lag behind turns and steps, bob, dip, breathe and recoil, heavier weapons more. Off: they stay still.",
                switches: "--weapon-sway --no-weapon-sway",
                get: |w| w.resource::<PlayerBody>().weapon_sway,
                set: |w, on| w.resource_mut::<PlayerBody>().weapon_sway = on,
                slider: None,
            },
        );
        nvf_play::settings::set_older_key(app, "weapon-inertia", "weapon-sway");
        nvf_play::settings::set_older_split(app, "weapon-sway", |v| vec![("weapon-inertia", v.to_string())]);
        add_setting(
            app,
            Setting {
                key: "camera-motion",
                group: GROUP_PICTURE,
                label: "Camera Motion",
                note: "On: in first person the view bobs with each step, breathes, gets out of breath after running and jolts when you are hit. Off: it stays still.",
                switches: "--head-bob --no-head-bob",
                get: |w| w.resource::<PlayerBody>().head_bob,
                set: |w, on| w.resource_mut::<PlayerBody>().head_bob = on,
                slider: None,
            },
        );
        nvf_play::settings::set_older_key(app, "camera-motion", "head-bob");
        nvf_play::settings::set_older_split(app, "head-bob", |v| vec![("camera-motion", v.to_string())]);
        add_setting(
            app,
            Setting {
                key: "weapon-lower",
                group: GROUP_PICTURE,
                label: "Lower weapon near walls",
                note: "On: in first person a drawn weapon lowers at a wall or a friend and comes back up when you aim or fire. Off: it stays up, as in the original.",
                switches: "--no-weapon-lower",
                get: |w| w.resource::<PlayerBody>().weapon_lower,
                set: |w, on| w.resource_mut::<PlayerBody>().weapon_lower = on,
                slider: None,
            },
        );
        add_setting(
            app,
            Setting {
                key: "true-iron-sights",
                group: GROUP_MENUS,
                label: "True iron sights",
                note: "On: aiming a gun lines its sights up with your eye and hides the crosshair. Off: the gun is held low with the crosshair, as in the original.",
                switches: "--no-true-iron-sights",
                get: |w| w.resource::<PlayerBody>().true_iron_sights,
                set: |w, on| w.resource_mut::<PlayerBody>().true_iron_sights = on,
                slider: None,
            },
        );
        add_setting(
            app,
            Setting {
                key: "scope-3d",
                group: GROUP_MENUS,
                label: "3D scopes (test)",
                note: "On: a scoped gun stays on screen and shows the zoomed view in its lens. Off: the full-screen scope picture, as in the original.",
                switches: "--scope-3d",
                get: |w| w.resource::<nvf_play::aim_zoom::Scope3d>().on,
                set: |w, on| w.resource_mut::<nvf_play::aim_zoom::Scope3d>().on = on,
                slider: None,
            },
        );
    }
    {
        use crate::shadow_rate::{set_sun_shadow_choice, sun_shadow_choice, SUN_SHADOW_CHOICES};
        // A list of its three choices [design: the menu overhaul, owner 2026-11-15: 7 or
        // fewer values are a list]; the INI's older slider part still loads.
        add_choice_list(
            app,
            Setting {
                key: "sun-shadows",
                group: GROUP_PICTURE,
                label: "Sun shadows",
                note: "The sun's shadows: Off, Performance (cheaper outdoors; moving shadows update at half the frame rate) or Quality. The original casts none from the world.",
                // `--shadow-distance` as the note named it ("out to"), not a part of it: kept so a
                // test run's switches keep the row as before [design].
                switches: "--shadow-distance, --sun-shadows, --no-sun-shadows",
                get: |_| false,
                set: |_, _| {},
                slider: None,
            },
            ChoiceList {
                names: |_| SUN_SHADOW_CHOICES.iter().map(|c| (*c).to_string()).collect(),
                chosen: |w| SUN_SHADOW_CHOICES[sun_shadow_choice(w).min(SUN_SHADOW_CHOICES.len() - 1)].to_string(),
                pick: take_sun_shadows,
                save: |w| SUN_SHADOW_CHOICES[sun_shadow_choice(w).min(SUN_SHADOW_CHOICES.len() - 1)].to_string(),
                load: take_sun_shadows,
                ask_keep: false,
            },
        );
        fn take_sun_shadows(w: &mut World, text: &str) -> bool {
            let names: Vec<String> = SUN_SHADOW_CHOICES.iter().map(|c| (*c).to_string()).collect();
            match nvf_play::settings::choice_of(text, &names, |v| choice_index(v, SUN_SHADOW_CHOICES.len())) {
                Some(i) => {
                    set_sun_shadow_choice(w, i);
                    true
                }
                None => false,
            }
        }
    }
    add_setting(
        app,
        Setting {
            key: "grass",
            group: GROUP_PICTURE,
            label: "Grass",
            note: "On: grass grows on the landscape; the cells around you get theirs over the next frames. Off: it is hidden and none grows on new cells.",
            switches: "--no-grass",
            get: |w| w.resource::<GrassConfig>().enabled,
            set: |w, on| w.resource_mut::<GrassConfig>().enabled = on,
            slider: None,
        },
    );
    add_setting(
        app,
        Setting {
            key: "distant-lod",
            group: GROUP_PICTURE,
            label: "Distant terrain and objects",
            note: "On: distant land, buildings and trees are drawn beyond the loaded cells (outdoors). Off: the world ends at the loaded cells.",
            switches: "--no-lod",
            get: |w| !w.resource::<Args>().no_lod,
            set: set_lod,
            slider: None,
        },
    );
    // How the landscape is smoothed (#2149, #2676): Off, 2 x 2, 4 x 4, 4 x 4 near; read when
    // the game starts, so the row keeps the next start's choice (nvf_play::smooth_terrain).
    // The INI's older on/off still reads (4 x 4 and Off).
    add_choice_list(
        app,
        Setting {
            key: nvf_play::smooth_terrain::KEY,
            group: GROUP_GRAPHICS,
            label: "Smooth terrain",
            note: nvf_play::smooth_terrain::NOTE,
            switches: "--smooth-terrain, --no-smooth-terrain",
            get: |_| false,
            set: |_, _| {},
            slider: None,
        },
        ChoiceList {
            names: |_| nvf_play::smooth_terrain::Smoothing::ALL.iter().map(|s| s.name().to_string()).collect(),
            chosen: |_| nvf_play::smooth_terrain::setting().name().to_string(),
            pick: pick_smoothing,
            save: |_| nvf_play::smooth_terrain::setting().stored().to_string(),
            load: pick_smoothing,
            ask_keep: false,
        },
    );
    nvf_play::settings::set_choice_labels(app, nvf_play::smooth_terrain::KEY, |_, name| fnv_util::words::tr_str(name).to_string());
    add_setting(
        app,
        Setting {
            key: "grade",
            group: GROUP_PICTURE,
            label: "Colour grade",
            note: "On: the picture is graded by the worldspace's, the weather's and the cell's image space. Off: the picture as lit, for comparisons.",
            switches: "--no-grade",
            get: |w| w.resource::<SkyState>().grade,
            set: |w, on| w.resource_mut::<SkyState>().grade = on,
            slider: None,
        },
    );
    register_display(app);
    register_distances(app);
    register_graphics(app);
    if crate::mobile_performance::enabled() {
        crate::mobile_performance::register(app);
    }
    // The Quality and Enhanced effects rows over the rows above (#1966).
    crate::quality::register(app, GROUP_GRAPHICS);
    // Renderer edits are latched by the shared settings setters, including Defaults.
    // Draw-distance streaming has no resume readiness contract yet (#1820) [design].
    register_pipeline_waits(app);
    // The volumes after the picture's rows (shown on the Audio tab: nvf_play::settings_layout).
    nvf_play::audio_out::register_settings(app);
    add_setting(
        app,
        Setting {
            key: "overlay",
            group: GROUP_DEVELOPER,
            label: "Text overlay",
            note: "On: the position, loading counts and key help are written over the picture. Off by default in the game.",
            switches: "--overlay, --no-overlay",
            get: |w| !w.resource::<Args>().no_overlay,
            set: set_overlay,
            slider: None,
        },
    );
    add_setting(
        app,
        Setting {
            key: "hit-markers",
            group: GROUP_DEVELOPER,
            label: "Shot markers",
            note: "On: a cross is drawn at each impact and a line from the muzzle along each hitscan shot.",
            switches: "--hit-markers",
            get: |w| w.resource::<nvf_play::combat::HitMarks>().show,
            set: |w, on| w.resource_mut::<nvf_play::combat::HitMarks>().show = on,
            slider: None,
        },
    );
    add_setting(
        app,
        Setting {
            key: "god-mode",
            group: GROUP_DEVELOPER,
            label: "God mode",
            note: "On: the player takes no damage from hits and cannot be killed by them.",
            switches: "--god",
            get: |_| nvf_play::combat::god_mode(),
            set: |_, on| nvf_play::combat::set_god_mode(on),
            slider: None,
        },
    );
    if has_nav {
        add_setting(
            app,
            Setting {
                key: "navmesh",
                group: GROUP_DEVELOPER,
                label: "Draw the navmesh",
                note: "On: the navmesh near the camera and the walking NPCs' paths are drawn.",
                switches: "--show-navmesh",
                get: |w| w.resource::<NavState>().show,
                set: |w, on| w.resource_mut::<NavState>().show = on,
                slider: None,
            },
        );
    }
    register_terrain_wireframe(app);
}

/// The Debug row Terrain wireframe (#2149, `crate::terrain_wireframe`): live, off by
/// default, beside the navmesh's whether or not there is a navmesh.
fn register_terrain_wireframe(app: &mut App) {
    use crate::terrain_wireframe::TerrainWireframe;
    if !app.world().contains_resource::<TerrainWireframe>() {
        return;
    }
    add_setting(
        app,
        Setting {
            key: crate::terrain_wireframe::KEY,
            group: GROUP_DEVELOPER,
            label: "Terrain wireframe",
            note: crate::terrain_wireframe::NOTE,
            switches: crate::terrain_wireframe::SWITCHES,
            get: |w| w.resource::<TerrainWireframe>().on,
            set: |w, on| w.resource_mut::<TerrainWireframe>().on = on,
            slider: None,
        },
    );
}

/// The Frame rate counter switch's note.
pub(crate) const FPS_COUNTER_NOTE: &str = "On: a panel of performance readings at the top right; the Counter rows below choose what it shows. Not in the original.";

/// The Frame rate counter switch's command-line switch (`--fps`): it decides with the INI
/// whether the frame split takes GPU timestamps from the start
/// (`perf_overlay::split::timestamps_wanted`), as `nvf_play::settings::apply_stored` reads it.
pub(crate) const FPS_COUNTER_SWITCHES: &str = "--fps";

/// The upscaling row's note (#2242), NVForever's own words [design].
pub(crate) const FPS_UPSCALING_NOTE: &str = "The upscaler drawing now: its name and mode, and the sizes the world is drawn at and rebuilt to. Off and the window's size while nothing upscales.";

/// The frame split row's note.
pub(crate) const FPS_SPLIT_NOTE: &str = "What a frame took on the main thread, the render thread (its wait for the window in brackets) and the GPU. The GPU part needs this row on at the start.";

/// The frame split row's command-line switches: none, so the INI's state always applies.
pub(crate) const FPS_SPLIT_SWITCHES: &str = "";

/// The Tree distance row's note [design: our own words, #3016].
const TREE_DISTANCE_NOTE: &str = "How far from you trees beyond the loaded cells are drawn as flat pictures, at most as far as distant objects are drawn. Farther runs slower.";

/// The field of view's least and greatest degrees on the settings page, in steps of 5.
const FOV_MIN: f32 = 50.0;
const FOV_MAX: f32 = 110.0;
const FOV_CHOICES: usize = 13;

/// The viewmodel field of view's least and greatest degrees, in steps of 5 (the original's
/// 55, `player_body::FIRST_PERSON_FOV`, among them).
const VIEWMODEL_FOV_MIN: f32 = 30.0;
const VIEWMODEL_FOV_MAX: f32 = 90.0;
const VIEWMODEL_FOV_CHOICES: usize = 13;

/// The field of view (`--fov`) and under it the arms' and weapon's (`--viewmodel-fov`), the
/// frame rate counter (the performance overlay, `perf_overlay/`: its switch and a switch per
/// row) and how fast frames are shown (`frame_pace.rs`; not while `--log-frame-times` measures, which wants vsync off).
fn register_display(app: &mut App) {
    add_setting(
        app,
        Setting::slider(
            "field-of-view",
            GROUP_PICTURE,
            "Field of view",
            "The game's field of view in degrees across a 4:3 picture (75 as the original's default); wider screens see more at the sides.",
            Slider::choice(FOV_CHOICES, |w| fov_part(w.resource::<Args>().fov), set_fov, |v| format!("{:.0}", fov_degrees(v))),
        )
        .with_switches("--fov"),
    );
    add_setting(
        app,
        Setting::slider(
            "viewmodel-field-of-view",
            GROUP_PICTURE,
            "Viewmodel field of view",
            "The angle the first-person arms and weapon are drawn with, in degrees (55 as the original's default): lower draws them bigger and closer.",
            Slider::choice(VIEWMODEL_FOV_CHOICES, |w| viewmodel_fov_part(w.resource::<Args>().viewmodel_fov), set_viewmodel_fov, |v| format!("{:.0}", viewmodel_fov_degrees(v))),
        )
        .with_switches("--viewmodel-fov"),
    );
    // The HUD's size (#1631), kept by nvf-play's interface and fixed for the run at start.
    nvf_play::interface::register_hud_scale(app, GROUP_PICTURE);
    nvf_play::menu_scale::register(app, GROUP_PICTURE);
    #[cfg(target_os = "android")]
    {
        nvf_play::menu_readability::register(app, GROUP_PICTURE);
        nvf_play::hud_readability::register(app, GROUP_PICTURE);
    }
    add_setting(
        app,
        Setting {
            key: "fps-counter",
            group: GROUP_PICTURE,
            label: "Frame rate counter",
            note: FPS_COUNTER_NOTE,
            switches: FPS_COUNTER_SWITCHES,
            get: |w| w.resource::<FpsCounter>().show,
            set: |w, on| w.resource_mut::<FpsCounter>().show = on,
            slider: None,
        },
    );
    register_counter_rows(app);
    // The physics rate (nvf_play::phys_rate) after the renderer's own Display rows, so their
    // order in the registry and the INI is as it was before the row.
    nvf_play::phys_rate::register_setting(app);
    if app.world().resource::<Args>().log_frame_times.is_some() {
        return;
    }
    add_choice_list(
        app,
        Setting {
            key: "display-sync",
            group: GROUP_PICTURE,
            label: "Display sync",
            note: "Vertical sync: frames wait for the display (no tearing). Variable refresh: for a G-Sync or FreeSync screen. Off: frames show at once (may tear).",
            switches: "",
            get: |_| false,
            set: |_, _| {},
            slider: None,
        },
        ChoiceList {
            names: |_| DisplaySync::ALL.iter().map(|d| d.name().to_string()).collect(),
            chosen: |w| w.resource::<FramePace>().sync.name().to_string(),
            pick: set_display_sync,
            save: |w| w.resource::<FramePace>().sync.stored().to_string(),
            load: set_display_sync,
            ask_keep: false,
        },
    );
    app.add_systems(PostStartup, read_old_vsync.after(nvf_play::settings::apply_stored));
    register_frame_cap(app);
}

fn register_frame_cap(app: &mut App) {
    // A list of the limits [design: the menu overhaul]; the INI's older slider part loads.
    add_choice_list(
        app,
        Setting {
            key: "frame-cap",
            group: GROUP_PICTURE,
            label: "Frame rate limit",
            note: "At most this many frames a second (Off: no limit of its own). A lower limit keeps the computer cooler and quieter.",
            switches: "",
            get: |_| false,
            set: |_, _| {},
            slider: None,
        },
        ChoiceList { names: |_| frame_cap_names(), chosen: frame_cap_chosen, pick: take_frame_cap, save: frame_cap_chosen, load: take_frame_cap, ask_keep: false },
    );
}

/// The Frame Rate Limit row's choices: Off, then each limit.
fn frame_cap_names() -> Vec<String> {
    frame_pace::CAPS.iter().map(|&c| if c == 0 { "Off".to_string() } else { c.to_string() }).collect()
}

fn frame_cap_chosen(w: &World) -> String {
    let cap = w.resource::<FramePace>().cap;
    frame_cap_names()[frame_pace::CAPS.iter().position(|&c| c == cap).unwrap_or(0)].clone()
}

/// The limit `text` names (or an older slider's part stands for).
fn take_frame_cap(w: &mut World, text: &str) -> bool {
    match nvf_play::settings::choice_of(text, &frame_cap_names(), |v| choice_index(v, frame_pace::CAPS.len())) {
        Some(i) => {
            w.resource_mut::<FramePace>().cap = frame_pace::CAPS[i];
            true
        }
        None => false,
    }
}

/// The frame rate counter's rows, listed under its switch (issue #1868; the
/// defaults are the issue's, `perf_overlay::Rows`). Each note says what the row reads and
/// where from.
fn register_counter_rows(app: &mut App) {
    let row = |key: &'static str, label: &'static str, note: &'static str, get: fn(&World) -> bool, set: fn(&mut World, bool)| Setting { key, group: GROUP_PICTURE, label, note, switches: "", get, set, slider: None };
    add_setting(
        app,
        row(
            "fps-counter-fps",
            "Counter: frame rate",
            "Frames a second, the mean frame time and the 1% low or the slowest frame (the row below chooses), from the time between frames.",
            |w| w.resource::<FpsCounter>().rows.fps,
            |w, on| w.resource_mut::<FpsCounter>().rows.fps = on,
        ),
    );
    add_choice_list(
        app,
        Setting {
            key: "fps-counter-low",
            group: GROUP_PICTURE,
            label: "Counter: last figure",
            note: "The frame rate row's last figure, over 10 seconds: 1% low (the mean of the slowest hundredth of frames) or the slowest frame.",
            switches: "",
            get: |_| false,
            set: |_, _| {},
            slider: None,
        },
        ChoiceList {
            names: |_| Low::ALL.iter().map(|l| l.name().to_string()).collect(),
            chosen: |w| w.resource::<FpsCounter>().rows.low.name().to_string(),
            pick: set_counter_low,
            save: |w| w.resource::<FpsCounter>().rows.low.stored().to_string(),
            load: set_counter_low,
            ask_keep: false,
        },
    );
    let rows = [
        row(
            "fps-counter-graph",
            "Counter: frame graph",
            "The frame times of the last 5 seconds as a line under the frame rate, the newest at the right, a faint line at 60 Hz (16.7 ms).",
            |w| w.resource::<FpsCounter>().rows.graph,
            |w, on| w.resource_mut::<FpsCounter>().rows.graph = on,
        ),
        row(
            "fps-counter-split",
            "Counter: frame split",
            FPS_SPLIT_NOTE,
            |w| w.resource::<FpsCounter>().rows.split,
            |w, on| w.resource_mut::<FpsCounter>().rows.split = on,
        ),
        // [design, #2249] Availability and scope follow perf_overlay/usage.rs.
        row(
            "fps-counter-cpu",
            "Counter: CPU use",
            "Machine and game CPU use, read once a second. Game use is cores kept busy: 1.0 is one full core.",
            |w| w.resource::<FpsCounter>().rows.cpu,
            |w, on| w.resource_mut::<FpsCounter>().rows.cpu = on,
        ),
        row(
            "fps-counter-gpu",
            "Counter: GPU use",
            "GPU use, read once a second: on Windows the busiest engine across programs; on Linux the whole device's, or the game's own where the driver tells.",
            |w| w.resource::<FpsCounter>().rows.gpu,
            |w, on| w.resource_mut::<FpsCounter>().rows.gpu = on,
        ),
        row(
            "fps-counter-vram",
            "Counter: GPU memory",
            "The graphics memory the game holds against the driver's budget, read every 2 seconds. Marked ! while near it the game keeps fewer unused assets.",
            |w| w.resource::<FpsCounter>().rows.vram,
            |w, on| w.resource_mut::<FpsCounter>().rows.vram = on,
        ),
        row(
            "fps-counter-ram",
            "Counter: RAM",
            "The game's RAM and the machine's RAM in use. Marked ! while under a tenth is free and the game keeps fewer unused assets. Windows only.",
            |w| w.resource::<FpsCounter>().rows.ram,
            |w, on| w.resource_mut::<FpsCounter>().rows.ram = on,
        ),
        row(
            "fps-counter-loaded",
            "Counter: loaded",
            "What the game holds now: the cells in the physics world, placed objects with a model, actors with a skeleton and engine entities.",
            |w| w.resource::<FpsCounter>().rows.loaded,
            |w, on| w.resource_mut::<FpsCounter>().rows.loaded = on,
        ),
        // [design, #2242] What draws is the effective choice (`upscaler::effective_temporal`),
        // its sizes what the render world drew (`upscaler::UpscaleDrawn`).
        row(
            "fps-counter-upscaling",
            "Counter: upscaling",
            FPS_UPSCALING_NOTE,
            |w| w.resource::<FpsCounter>().rows.upscale,
            |w, on| w.resource_mut::<FpsCounter>().rows.upscale = on,
        ),
    ];
    for r in rows {
        add_setting(app, r);
    }
}

/// The frame rate row's last figure of this name (or INI text, `Low::parse`); false: not
/// one.
fn set_counter_low(world: &mut World, name: &str) -> bool {
    let Some(low) = Low::parse(name) else { return false };
    world.resource_mut::<FpsCounter>().rows.low = low;
    true
}

/// `[Settings] <key>` of the INI this run reads (`SettingsFile::for_run`), for what has to
/// be known when the app is built, before the INI is applied (`apply_stored`, PostStartup):
/// the CPU placement (`cpu_placement.rs`), the frame split's GPU timestamps
/// (`perf_overlay::split`). None without the INI or the key.
pub(crate) fn stored_at_start(key: &str) -> Option<String> {
    fnv_install::ini_get(&settings_text_at_start()?, nvf_play::settings::SECTION, key)
}

/// The text of the INI this run reads (`SettingsFile::for_run`), as [`stored_at_start`] reads
/// it: for the upscaler's rows, read before the renderer exists (`UpscalerIni`). None without
/// the INI.
pub(crate) fn settings_text_at_start() -> Option<String> {
    // Unattended: a test aid drives this run (as nvf_play's test_aid::Unattended).
    let unattended = std::env::vars_os().any(|(k, _)| k.to_str().is_some_and(nvf_play::drives_run));
    let path = nvf_play::settings::SettingsFile::for_run(unattended).0?;
    std::fs::read_to_string(path).ok()
}

/// The display sync of this name (or INI text, `DisplaySync::parse`); false: not one.
fn set_display_sync(world: &mut World, name: &str) -> bool {
    let Some(d) = DisplaySync::parse(name) else { return false };
    world.resource_mut::<FramePace>().sync = d;
    true
}

/// An INI written before the display-sync row had the `vsync` switch: read it when there is
/// no `display-sync` (after `nvf_play::settings::apply_stored`, so it counts as stored).
fn read_old_vsync(world: &mut World) {
    if world.resource::<Args>().log_frame_times.is_some() {
        return;
    }
    let Some(path) = world.get_resource::<nvf_play::settings::SettingsFile>().and_then(|f| f.0.clone()) else { return };
    let Ok(text) = std::fs::read_to_string(&path) else { return };
    let section = nvf_play::settings::SECTION;
    if fnv_install::ini_get(&text, section, "display-sync").is_some() {
        return;
    }
    let Some(old) = fnv_install::ini_get(&text, section, "vsync") else { return };
    match DisplaySync::parse(&old) {
        Some(d) => {
            world.resource_mut::<FramePace>().sync = d;
            info!("settings: display-sync {} from the old vsync={} in {}", d.name(), old.trim(), path.display());
        }
        None => warn!("settings: [{section}] vsync={old} in {} is not 1 or 0; left as it is", path.display()),
    }
}

/// Whole degrees from `min` to `max` for a slider part.
fn slider_degrees(v: f32, min: f32, max: f32) -> f32 {
    (min + v.clamp(0.0, 1.0) * (max - min)).round()
}

/// The slider part for `deg` degrees of `min` to `max`, as `Slider::fit` keeps parts.
fn degrees_part(deg: f32, min: f32, max: f32) -> f32 {
    ((deg - min) / (max - min) * 1000.0).round().clamp(0.0, 1000.0) / 1000.0
}

/// The field of view's degrees for a slider part, whole.
fn fov_degrees(v: f32) -> f32 {
    slider_degrees(v, FOV_MIN, FOV_MAX)
}

/// The slider part for `deg` degrees, as `Slider::fit` keeps parts.
fn fov_part(deg: f32) -> f32 {
    degrees_part(deg, FOV_MIN, FOV_MAX)
}

/// A new field of view: kept in `Args` and put on the camera, where it takes the place of
/// a console `fov`'s world angle, so scripts no longer read that one (GetCameraFOV 0).
fn set_fov(world: &mut World, v: f32) {
    let deg = fov_degrees(v);
    world.resource_mut::<Args>().fov = deg;
    if let Some(rt) = world.get_resource_mut::<nvf_play::script::ScriptState>().and_then(|s| s.into_inner().rt.as_mut()) {
        rt.game.ui.camera.fov_set.0 = None;
    }
    let mut cams = world.query_filtered::<&mut Projection, With<FlyCam>>();
    for mut p in cams.iter_mut(world) {
        if let Projection::Perspective(p) = &mut *p {
            p.fov = nvf_play::free_cam::vertical_fov(deg);
        }
    }
}

/// The viewmodel field of view's degrees for a slider part, whole.
fn viewmodel_fov_degrees(v: f32) -> f32 {
    slider_degrees(v, VIEWMODEL_FOV_MIN, VIEWMODEL_FOV_MAX)
}

/// The slider part for a viewmodel field of view of `deg` degrees, as `Slider::fit` keeps parts.
fn viewmodel_fov_part(deg: f32) -> f32 {
    degrees_part(deg, VIEWMODEL_FOV_MIN, VIEWMODEL_FOV_MAX)
}

/// A new viewmodel field of view: kept in `Args`, where `player_body` reads it each frame.
fn set_viewmodel_fov(world: &mut World, v: f32) {
    world.resource_mut::<Args>().viewmodel_fov = viewmodel_fov_degrees(v);
}

/// The two fields of view as the player set them (`--fov`, `--viewmodel-fov` and their
/// sliders), told to the scripts (GetCameraFOV, GetPlayerCamFOV) before they run: at start,
/// after a slider's change and for each new game state. A console `fov` still wins there,
/// as on the screen.
pub fn script_fov(args: Res<Args>, script: Option<ResMut<nvf_play::script::ScriptState>>) {
    let Some(mut script) = script else { return };
    let told = (Some(args.fov), Some(args.viewmodel_fov));
    // Compared first, so an unchanged frame leaves the script state unchanged for Bevy.
    if script.rt.as_ref().is_some_and(|rt| rt.game.ui.camera.settings_fov != told) {
        if let Some(rt) = script.rt.as_mut() {
            rt.game.ui.camera.settings_fov = told;
            info!("settings: scripts read the field of view {:.0}, the viewmodel's {:.0}", args.fov, args.viewmodel_fov);
        }
    }
}

/// The draw-distance sliders (`draw_distance.rs`): each a part of its full distance, the
/// full one drawing as without them; the Tree distance's full one is the distant LOD's far
/// limit and it starts at `--tree-distance`'s part of that (#3016).
fn register_distances(app: &mut App) {
    if !app.world().contains_resource::<DrawDistances>() {
        return;
    }
    let slider = |get: fn(&World) -> f32, set: fn(&mut World, f32)| Slider { get, set, min: draw_distance::MIN, step: draw_distance::STEP, shown: None };
    let sliders = [
        Setting::slider(
            "object-distance",
            GROUP_PICTURE,
            "Object distance",
            "How far from you objects of the loaded cells (buildings, rocks, furniture, clutter) are drawn. Lower draws fewer and runs faster.",
            slider(|w| w.resource::<DrawDistances>().objects, |w, v| draw_distance::set_part(w, DrawClass::Object, v)),
        ),
        Setting::slider(
            "actor-distance",
            GROUP_PICTURE,
            "Actor distance",
            "How far from you people and creatures are drawn, as a part of the farthest the loaded cells reach.",
            slider(|w| w.resource::<DrawDistances>().actors, |w, v| draw_distance::set_part(w, DrawClass::Actor, v)),
        ),
        Setting::slider(
            "item-distance",
            GROUP_PICTURE,
            "Item distance",
            "How far from you items lying in the world are drawn, as a part of the farthest the loaded cells reach.",
            slider(|w| w.resource::<DrawDistances>().items, |w, v| draw_distance::set_part(w, DrawClass::Item, v)),
        ),
        Setting::slider(
            "grass-distance",
            GROUP_PICTURE,
            "Grass distance",
            "How far from you grass is drawn, as a part of the farthest grass distance and its fade range.",
            slider(|w| w.resource::<DrawDistances>().grass, draw_distance::set_grass),
        ),
        // A part of the distant LOD's far limit, shown as the distance it gives (#3016).
        Setting::slider(
            "tree-distance",
            GROUP_PICTURE,
            "Tree distance",
            TREE_DISTANCE_NOTE,
            Slider {
                get: |w| w.resource::<DrawDistances>().trees,
                set: draw_distance::set_trees,
                min: draw_distance::TREE_STEP,
                step: draw_distance::TREE_STEP,
                shown: Some(draw_distance::tree_distance_text),
            },
        )
        // Its start is the flag's distance, so a given flag wins over the INI in a test run.
        .with_switches("--tree-distance"),
    ];
    for s in sliders {
        add_setting(app, s);
    }
    add_choice_list(
        app,
        Setting {
            key: "shadow-distance",
            group: GROUP_PICTURE,
            label: "Shadow distance",
            note: "How far from you the sun casts shadows, as a part of the farthest shadow distance; 0%: none.",
            switches: "",
            get: |_| false,
            set: |_, _| {},
            slider: None,
        },
        ChoiceList { names: |_| shadow_distance_names(), chosen: shadow_distance_chosen, pick: take_shadow_distance, save: shadow_distance_chosen, load: take_shadow_distance, ask_keep: false },
    );
}

/// The Upscaler row's key and the Upscaling mode row's: two rows for the one request
/// (`Graphics::upscaler`) since #2198's play-test, where the one dropdown of eleven choices
/// was too long for its box [design: the user's, 2026-11-12]. The second row was labelled
/// Upscaling quality at first; renamed to keep it apart from the Quality preset row (the
/// user's, 2026-11-12), its key kept.
pub(crate) const UPSCALER_KEY: &str = "upscaler";
pub(crate) const QUALITY_KEY: &str = "upscaler-quality";

/// The one row the two took over from, under the name it had when FSR 3.1 was its only
/// upscaler: still read from an INI that lacks `upscaler` (`set_older_key`) and still taken by
/// the test aids (`set_list_alias`), by its eleven names (`graphics::FSR_UPSCALING_CHOICES`)
/// and the early slider build's numbers.
pub(crate) const LEGACY_KEY: &str = "fsr-upscaling";

/// The two rows' notes.
pub(crate) const UPSCALER_NOTE: &str = "FSR 3.1 or NVIDIA DLSS rebuilds the world at display resolution, at the mode below. DLSS needs an NVIDIA RTX card and its files in the dlss folder.";
pub(crate) const QUALITY_NOTE: &str = "The share of the screen the world is drawn at; the upscaler rebuilds the rest. Native AA: full size. Quality to Ultra Performance: ever faster.";

/// The two rows' command-line switches: each row names all three, so a test run that gives
/// any of them keeps both rows at the command line's choice over the INI
/// (`nvf_play::settings::GivenSwitches`), at the start ([`upscaler_after_ini`]) as at
/// `PostStartup`.
pub(crate) const UPSCALER_SWITCHES: &str = "--upscaler, --upscaler-quality, --fsr";

/// The DLSS model row's key, right under Upscaling mode (#2198, the user's request): which
/// of NVIDIA's DLSS models DLSS uses (`Graphics::dlss_model`).
pub(crate) const MODEL_KEY: &str = "dlss-model";

/// The DLSS model row's note: each choice in NVForever's own words, in the row's order, from
/// what Streamline's header says of the presets [documented: `sl_dlss.h:46-65`] and the
/// defaults NGX reported in the runs at 1280x720 [observed, 2026-11-12:
/// docs/spec-notes/streamline.md, "DLSS presets"]. The settings page has no note per choice,
/// so the row's note holds them all; it says the letters are what NVIDIA calls presets.
pub(crate) const MODEL_NOTE: &str = "Which DLSS model rebuilds the picture (NVIDIA calls them presets). Default, recommended: NVIDIA picks per mode. Only while DLSS draws.";

/// The DLSS model row's command-line switch.
pub(crate) const MODEL_SWITCHES: &str = "--dlss-model";

/// The mesh terrain blend's row (#2661, mesh_terrain_blend.rs).
pub(crate) const MESH_BLEND_KEY: &str = "mesh-terrain-blend";
/// Its note, in NVForever's own words.
pub(crate) const MESH_BLEND_NOTE: &str = "On: softens where outdoor roads, rocks and cliffs meet the landscape, giving them the ground's colour and light there. Not in the original.";

/// The engine-level improvements of `graphics.rs` (`--graphics enhanced` starts with all
/// of them on, `vanilla` with none).
fn register_graphics(app: &mut App) {
    // Key, label, note, command-line switches, get, set.
    type Switch = (&'static str, &'static str, &'static str, &'static str, fn(&World) -> bool, fn(&mut World, bool));
    // Two rows for the one temporal upscaler request (#2198): which upscaler, then its
    // quality; both keep `Graphics::upscaler`. The quality row stands dimmed while no upscaler
    // is chosen. The older single row's INI key and test-aid key still work.
    add_choice_list(
        app,
        Setting { key: UPSCALER_KEY, group: GROUP_GRAPHICS, label: "Upscaler", note: UPSCALER_NOTE, switches: UPSCALER_SWITCHES, get: |_| false, set: |_, _| {}, slider: None },
        ChoiceList {
            names: upscaler_names,
            chosen: upscaler_chosen,
            pick: pick_upscaler,
            save: upscaler_chosen,
            load: load_upscaler,
            ask_keep: false,
        },
    );
    nvf_play::settings::set_choice_labels(app, UPSCALER_KEY, upscaler_label);
    nvf_play::settings::set_choice_unavailable(app, UPSCALER_KEY, upscaler_unavailable);
    // Download DLSS… opens its dialog: nothing of the row to keep or store (#2250).
    nvf_play::settings::set_choice_action(app, UPSCALER_KEY, crate::dlss_download::is_choice);
    nvf_play::settings::set_older_key(app, UPSCALER_KEY, LEGACY_KEY);
    nvf_play::settings::set_older_split(app, LEGACY_KEY, split_legacy);
    add_choice_list(
        app,
        Setting { key: QUALITY_KEY, group: GROUP_GRAPHICS, label: "Upscaling mode", note: QUALITY_NOTE, switches: UPSCALER_SWITCHES, get: |_| false, set: |_, _| {}, slider: None },
        ChoiceList {
            names: |_| UpscaleQuality::ALL.iter().map(|q| q.name().to_string()).collect(),
            chosen: quality_chosen,
            pick: load_quality,
            save: quality_chosen,
            load: load_quality,
            ask_keep: false,
        },
    );
    nvf_play::settings::set_choice_labels(app, QUALITY_KEY, quality_label);
    // The upscaler's own rows show only for the upscaler chosen (the Upscaler row's choice),
    // Anti-Aliasing only without one [design: owner 2026-11-15, the menu overhaul].
    // The DLSS model, under the mode: dimmed while DLSS is not what draws.
    add_choice_list(
        app,
        Setting { key: MODEL_KEY, group: GROUP_GRAPHICS, label: "DLSS model", note: MODEL_NOTE, switches: MODEL_SWITCHES, get: |_| false, set: |_, _| {}, slider: None },
        ChoiceList {
            names: |_| DlssModel::ALL.iter().map(|m| m.name().to_string()).collect(),
            chosen: model_chosen,
            pick: pick_model,
            save: model_chosen,
            load: load_model,
            ask_keep: false,
        },
    );
    nvf_play::settings::set_choice_labels(app, MODEL_KEY, |_, name| fnv_util::words::tr_str(name).to_string());
    nvf_play::settings::set_disabled_status(app, MODEL_KEY, model_dimmed);
    nvf_play::settings::set_list_alias(
        app,
        LEGACY_KEY,
        nvf_play::settings::ListAlias { rows: &[UPSCALER_KEY, QUALITY_KEY], chosen: legacy_chosen, pick: pick_legacy, unavailable: legacy_unavailable },
    );
    // The FSR library, under the DLSS model: dimmed while FSR is not what draws.
    add_choice_list(
        app,
        Setting {
            key: "fsr-library",
            group: GROUP_GRAPHICS,
            label: "FSR library",
            note: "What FSR upscales with: NVForever's built-in FSR 3.1, or a FidelityFX library in the game's folder (for tools that replace the upscaler).",
            switches: "",
            get: |_| false,
            set: |_, _| {},
            slider: None,
        },
        ChoiceList {
            names: |_| fsr_library_names().to_vec(),
            chosen: |_| fsr_library_names()[usize::from(crate::ffx_api::enabled())].clone(),
            pick: |_, name| pick_fsr_library(name),
            save: |_| (if crate::ffx_api::enabled() { FSR_LIBRARY_FOLDER } else { FSR_LIBRARY_BUILT_IN }).to_string(),
            load: |_, value| pick_fsr_library(value),
            ask_keep: false,
        },
    );
    nvf_play::settings::set_disabled_status(app, "fsr-library", fsr_library_dimmed);
    // Lists [design: the menu overhaul: 7 or fewer values]; the INI's older slider part loads.
    add_choice_list(
        app,
        Setting {
            key: "anti-aliasing",
            group: GROUP_GRAPHICS,
            label: "Anti-aliasing",
            note: "How edges are smoothed: multisampling 2x, 4x or 8x (dearer at each step) or FXAA, SMAA or TAA afterwards. Shown while no upscaler is chosen.",
            switches: "--msaa, --graphics-only",
            get: |_| false,
            set: |_, _| {},
            slider: None,
        },
        ChoiceList {
            names: |_| graphics::AA_CHOICES.iter().map(|c| c.0.to_string()).collect(),
            chosen: |w| graphics::AA_CHOICES[graphics::aa_choice(w).min(graphics::AA_CHOICES.len() - 1)].0.to_string(),
            pick: take_aa,
            save: |w| graphics::AA_CHOICES[graphics::aa_choice(w).min(graphics::AA_CHOICES.len() - 1)].0.to_string(),
            load: take_aa,
            ask_keep: false,
        },
    );
    add_choice_list(
        app,
        Setting {
            key: "shadow-filter",
            group: GROUP_GRAPHICS,
            label: "Shadow filtering",
            note: "How the sun's shadow edges are softened: Fast (harder, cheapest), Smooth (as vanilla) or Temporal (grainy unless Anti-aliasing is TAA).",
            switches: "",
            get: |_| false,
            set: |_, _| {},
            slider: None,
        },
        ChoiceList { names: |_| graphics::SHADOW_FILTERS.iter().map(|f| f.0.to_string()).collect(), chosen: shadow_filter_chosen, pick: take_shadow_filter, save: shadow_filter_chosen, load: take_shadow_filter, ask_keep: false },
    );
    // The full-detail scenery ring (#2701, full_detail.rs): Off, 7 x 7 or 9 x 9, switched
    // in play (the ring is cleared and streamed again).
    add_choice_list(
        app,
        Setting {
            key: FULL_DETAIL_KEY,
            group: GROUP_GRAPHICS,
            label: "Full-detail scenery",
            note: "Draws buildings, rocks and cliffs out to 7 x 7 or 9 x 9 cells as full models, not distant forms. Costs GPU time. Not in the original.",
            switches: "--graphics-only",
            get: |_| false,
            set: |_, _| {},
            slider: None,
        },
        ChoiceList { names: |_| full_detail_names(), chosen: full_detail_chosen, pick: take_full_detail, save: full_detail_chosen, load: take_full_detail, ask_keep: false },
    );
    // Distant objects (#3014, distant_objects.rs): Off, 8 cells or 12 cells, switched in
    // play (cells out of a smaller reach fade out, new ones stream in).
    add_choice_list(
        app,
        Setting {
            key: DISTANT_OBJECTS_KEY,
            group: GROUP_GRAPHICS,
            label: "Distant objects",
            note: "Draws the rocks, shacks, pylons and signs the distant LOD leaves out, up to 8 or 12 cells away. Costs GPU time. Not in the original.",
            switches: "--graphics-only",
            get: |_| false,
            set: |_, _| {},
            slider: None,
        },
        ChoiceList { names: |_| distant_objects_names(), chosen: distant_objects_chosen, pick: take_distant_objects, save: distant_objects_chosen, load: take_distant_objects, ask_keep: false },
    );
    // The sun's shadow maps' side, 2048 (the default) or 4096, changed in play (#2843); saved
    // as its name, the branch's older slider part (0 or 1) still loads.
    add_choice_list(
        app,
        Setting {
            key: "shadow-resolution",
            group: GROUP_GRAPHICS,
            label: "Shadow resolution",
            note: "Size of the sun's and the flashlight's shadow maps: 4096 draws sharper shadows than 2048. It takes more video memory and a little frame rate.",
            switches: "",
            get: |_| false,
            set: |_, _| {},
            slider: None,
        },
        ChoiceList {
            names: |_| graphics::SHADOW_RESOLUTIONS.iter().map(|r| r.0.to_string()).collect(),
            chosen: shadow_resolution_chosen,
            pick: take_shadow_resolution,
            save: shadow_resolution_chosen,
            load: take_shadow_resolution,
            ask_keep: false,
        },
    );
    // Shadows of placed lights (light_shadows.rs; graphics-enhanced.md §3.17): Off,
    // Performance or Quality as the Sun shadows row, saved by name, off by default [design].
    add_choice_list(
        app,
        Setting {
            key: "light-shadows",
            group: GROUP_GRAPHICS,
            label: "ALCS (experimental)",
            note: LIGHT_SHADOWS_NOTE,
            switches: "--light-shadows",
            get: |_| false,
            set: |_, _| {},
            slider: None,
        },
        ChoiceList {
            names: |_| crate::light_shadows::CHOICES.iter().map(|c| c.to_string()).collect(),
            chosen: light_shadows_chosen,
            pick: take_light_shadows,
            save: light_shadows_chosen,
            load: take_light_shadows,
            ask_keep: false,
        },
    );
    nvf_play::settings::set_disabled_status(app, "anti-aliasing", |w| crate::upscaler::temporal_active(w).then_some("Off (upscaling active)"));
    // Frame generation: Off, FSR 3.1 or DLSS (#2317), each choice dimmed with its reason; the
    // INI's old on/off still reads. Then DLSS's frames per real frame.
    add_choice_list(
        app,
        Setting { key: FRAME_GENERATION_KEY, group: GROUP_GRAPHICS, label: "Frame generation", note: FRAME_GENERATION_NOTE, switches: FRAME_GENERATION_SWITCHES, get: |_| false, set: |_, _| {}, slider: None },
        ChoiceList {
            names: |_| crate::frame_gen::FrameGenKind::ALL.iter().map(|k| k.name().to_string()).collect(),
            chosen: frame_generation_chosen,
            pick: pick_frame_generation,
            save: frame_generation_saved,
            load: pick_frame_generation,
            ask_keep: false,
        },
    );
    nvf_play::settings::set_choice_labels(app, FRAME_GENERATION_KEY, |_, name| fnv_util::words::tr_str(name).to_string());
    nvf_play::settings::set_choice_unavailable(app, FRAME_GENERATION_KEY, frame_generation_unavailable);
    add_choice_list(
        app,
        Setting { key: FRAMES_KEY, group: GROUP_GRAPHICS, label: "Generated frames", note: FRAMES_NOTE, switches: "", get: |_| false, set: |_, _| {}, slider: None },
        ChoiceList { names: |_| FRAMES_CHOICES.iter().map(|c| (*c).to_string()).collect(), chosen: frames_chosen, pick: pick_frames, save: frames_chosen, load: pick_frames, ask_keep: false },
    );
    nvf_play::settings::set_choice_unavailable(app, FRAMES_KEY, frames_unavailable);
    nvf_play::settings::set_disabled_status(app, FRAMES_KEY, frames_dimmed);
    // Only DLSS makes more than one frame (FSR 3.1: 2x) [design: owner 2026-11-15].
    nvf_play::settings::set_note_now(app, FRAMES_KEY, frames_note_now);
    add_setting(
        app,
        Setting {
            key: "latency-reduction",
            group: GROUP_GRAPHICS,
            label: "Latency reduction",
            note: "The graphics driver keeps the game from preparing frames ahead (AMD Anti-Lag, NVIDIA low latency), meant to show what you do sooner. Not in the original.",
            switches: "--latency-reduction",
            get: |w| w.resource::<crate::anti_lag::LatencyReduction>().on,
            set: |w, on| w.resource_mut::<crate::anti_lag::LatencyReduction>().on = on,
            slider: None,
        },
    );
    nvf_play::settings::set_disabled_status(app, "latency-reduction", |w| (!crate::anti_lag::available(w)).then_some("Not on this GPU"));
    // Ambient occlusion: a list of its three choices like Sun shadows' [design, #3075:
    // \"AO's row to match ALC shadows' Off/Performance/Quality"]; the INI's older
    // on/off (1/0, true/false, on/off, yes/no) still loads, as Quality and Off.
    add_choice_list(
        app,
        Setting { key: AO_KEY, group: GROUP_GRAPHICS, label: "Ambient occlusion", note: AO_NOTE, switches: "--graphics, --ambient-occlusion", get: |_| false, set: |_, _| {}, slider: None },
        ChoiceList { names: |_| AO_CHOICES.iter().map(|c| (*c).to_string()).collect(), chosen: ao_chosen, pick: take_ao, save: ao_chosen, load: take_ao, ask_keep: false },
    );
    // Volumetric clouds: a list of Off and its levels [design, 2026-11-23: 7
    // or fewer values]; the INI's older on/off still loads, on as Normal.
    add_choice_list(
        app,
        Setting { key: CLOUDS_KEY, group: GROUP_GRAPHICS, label: "Volumetric clouds (experimental)", note: CLOUDS_NOTE, switches: "--graphics-only", get: |_| false, set: |_, _| {}, slider: None },
        ChoiceList { names: |_| CLOUD_CHOICES.iter().map(|c| (*c).to_string()).collect(), chosen: clouds_chosen, pick: take_clouds, save: clouds_chosen, load: take_clouds, ask_keep: false },
    );
    let switches: [Switch; 26] = [
        (
            "fine-shadows",
            "Longer sun shadows",
            "On: the sun's shadows reach half as far again as the shadow distance. Costs a little frame rate.",
            "--shadow-distance, --graphics",
            |w| w.resource::<Graphics>().fine_shadows,
            |w, on| w.resource_mut::<Graphics>().fine_shadows = on,
        ),
        (
            "cloud-shadows",
            "Cloud shadows (experimental)",
            CLOUD_SHADOWS_NOTE,
            "--graphics, --graphics-only",
            |w| w.resource::<Graphics>().cloud_shadows,
            |w, on| w.resource_mut::<Graphics>().cloud_shadows = on,
        ),
        (
            "terrain-shadows",
            "Terrain shadows (experimental)",
            TERRAIN_SHADOWS_NOTE,
            "--graphics, --graphics-only",
            |w| w.resource::<Graphics>().terrain_shadows,
            |w, on| w.resource_mut::<Graphics>().terrain_shadows = on,
        ),
        (
            "bloom",
            "Bloom",
            "On: light brighter than a sunlit white wall (the sun, lamps, windows) glows into its surroundings, on top of the game's own image space.",
            "--graphics",
            |w| w.resource::<Graphics>().bloom,
            |w, on| w.resource_mut::<Graphics>().bloom = on,
        ),
        (
            "glow",
            "Light glow (HDR)",
            "On: neon, lamps, beams and fire glow softly, as the original's HDR glow does, as much as the place's image space asks. Bloom adds to it.",
            "--graphics-only",
            |w| w.resource::<Graphics>().glow,
            |w, on| w.resource_mut::<Graphics>().glow = on,
        ),
        (
            "eye-adaptation",
            "Eye adaptation",
            "On: dark views brighten after a moment, as the eye gets used to the dark, by the place's image space. Needs the light glow.",
            "--graphics-only",
            |w| w.resource::<Graphics>().eye_adaptation,
            |w, on| w.resource_mut::<Graphics>().eye_adaptation = on,
        ),
        (
            "neon-glow",
            "Neon glow",
            "On: where the image space asks for a strong glow (the Strip at night) neon burns with a wide halo, as in the original. Needs the light glow.",
            "--graphics-only",
            |w| w.resource::<Graphics>().neon_glow,
            |w, on| w.resource_mut::<Graphics>().neon_glow = on,
        ),
        (
            "sharpen",
            "Sharpening",
            "On: the final picture is sharpened where it has little contrast (contrast-adaptive sharpening).",
            "--graphics",
            |w| w.resource::<Graphics>().sharpen,
            |w, on| w.resource_mut::<Graphics>().sharpen = on,
        ),
        (
            "anisotropic",
            "Anisotropic filtering 16x",
            "On: textures seen at a slant stay sharp further (16x instead of 8x). For textures loaded from now on (the cells loaded next).",
            "--graphics",
            |w| w.resource::<Graphics>().anisotropic,
            |w, on| {
                w.resource_mut::<Graphics>().anisotropic = on;
                nvf_assets::texture::set_anisotropy(if on { 16 } else { 8 });
            },
        ),
        // #2981: read once at the start (main.rs, nvf_assets `authored::set_enabled`); the
        // row keeps the next start's choice, so no pipeline waits for it.
        (
            crate::graphics::AUTHORED_MATERIALS_KEY,
            "Authored materials",
            AUTHORED_MATERIALS_NOTE,
            AUTHORED_MATERIALS_SWITCHES,
            |w| w.resource::<Graphics>().authored_materials,
            |w, on| w.resource_mut::<Graphics>().authored_materials = on,
        ),
        // #2981 reflections: read once at the start like Authored materials (main.rs,
        // nvf_assets `reflection_cube::set_enabled`); the row keeps the next start's choice.
        (
            crate::graphics::AUTHORED_REFLECTIONS_KEY,
            "Reflections on authored materials",
            AUTHORED_REFLECTIONS_NOTE,
            AUTHORED_MATERIALS_SWITCHES,
            |w| w.resource::<Graphics>().authored_reflections,
            |w, on| w.resource_mut::<Graphics>().authored_reflections = on,
        ),
        (
            "terrain-height-blend",
            "Terrain height blending",
            "On: where ground textures meet, the new one shows through its brighter stones and clumps first instead of an even fade. Fully painted ground is unchanged.",
            "--graphics, --graphics-only",
            |w| w.resource::<Graphics>().terrain_height_blend,
            |w, on| w.resource_mut::<Graphics>().terrain_height_blend = on,
        ),
        (
            "terrain-anti-tiling",
            "Terrain anti-tiling",
            "On: ground textures stop repeating as a visible grid, for some GPU time. Off: each repeats every 256 units, as in the original.",
            "--graphics, --graphics-only",
            |w| w.resource::<Graphics>().terrain_anti_tiling,
            |w, on| w.resource_mut::<Graphics>().terrain_anti_tiling = on,
        ),
        // #3322: needs Authored materials (nvf_settings_layout NEEDS, for the next start) and
        // the run's state of it (`parallax_status`), whose height maps it draws; switched
        // while the game runs (terrain.rs `sync_parallax`).
        (
            "terrain-parallax",
            "Terrain parallax",
            "On: ground textures a texture pack gives height maps for (textures\\pbr\\) show depth near you, for some GPU time. Without such a pack nothing changes.",
            "--graphics-only",
            |w| w.resource::<Graphics>().terrain_parallax,
            |w, on| w.resource_mut::<Graphics>().terrain_parallax = on,
        ),
        (
            "light-shafts",
            "Sun shafts",
            "On: sunlight streams past what stands in front of the sun, stronger the hazier the weather's fog. Not in the original.",
            "--graphics",
            |w| w.resource::<Graphics>().light_shafts,
            |w, on| w.resource_mut::<Graphics>().light_shafts = on,
        ),
        (
            "heat-haze",
            "Heat haze (experimental)",
            "On: the far ground near the horizon shimmers slightly on hot, clear days, by the sun and the weather. A prototype. Not in the original.",
            "--graphics-only",
            |w| w.resource::<Graphics>().heat_haze,
            |w, on| w.resource_mut::<Graphics>().heat_haze = on,
        ),
        (
            "desert-haze",
            "Desert haze (experimental)",
            "On: a thin dust haze thickens with distance and toward the ground, warmer toward the sun, as far as the weather's fog allows. Not in the original.",
            "--graphics, --graphics-only",
            |w| w.resource::<Graphics>().desert_haze,
            |w, on| w.resource_mut::<Graphics>().desert_haze = on,
        ),
        (
            VOLUMETRIC_LIGHT_KEY,
            "Volumetric light (experimental)",
            VOLUMETRIC_LIGHT_NOTE,
            "--graphics-only",
            |w| w.resource::<Graphics>().volumetric_light,
            |w, on| w.resource_mut::<Graphics>().volumetric_light = on,
        ),
        (
            "aerial-perspective",
            "Aerial perspective (experimental)",
            "On: far land takes on the sky's colours with distance, bluer at noon, warmer at dusk, brighter toward the sun, inside the weather's fog. Not in the original.",
            "--graphics, --graphics-only",
            |w| w.resource::<Graphics>().aerial_perspective,
            |w, on| w.resource_mut::<Graphics>().aerial_perspective = on,
        ),
        (
            "night-sky-detail",
            "Night sky detail (experimental)",
            "On: stars twinkle a little, more near the horizon; the moon gets a glow and lights the clouds facing it, wider in hazy weather. Not in the original.",
            "--graphics, --graphics-only",
            |w| w.resource::<Graphics>().night_sky_detail,
            |w, on| w.resource_mut::<Graphics>().night_sky_detail = on,
        ),
        (
            "physical-pbr",
            "Physical lighting (experimental)",
            PHYSICAL_PBR_NOTE,
            "--graphics-only",
            |w| w.resource::<Graphics>().physical_pbr,
            |w, on| w.resource_mut::<Graphics>().physical_pbr = on,
        ),
        (
            "distant-lights",
            "Distant lights (experimental)",
            "On: at night, lamps in the places beyond the loaded area show as small glows, so towns can be seen from afar. They light nothing. Not in the original.",
            "--graphics, --graphics-only",
            |w| w.resource::<Graphics>().distant_lights,
            |w, on| w.resource_mut::<Graphics>().distant_lights = on,
        ),
        (
            "water-reflections",
            "Screen-space reflections",
            "On: water the mirrored reflection does not reach (another height, interiors) reflects what is on the screen instead of only the sky.",
            "--graphics",
            |w| w.resource::<Graphics>().water_reflections,
            |w, on| w.resource_mut::<Graphics>().water_reflections = on,
        ),
        (
            "water-effects",
            "Enhanced water effects",
            "Sunlight shifts across shallow beds. Deeper water looks murkier. Selected rivers flow, shorelines lap, and shots and blasts disturb the surface.",
            "--graphics, --graphics-only",
            |w| w.resource::<Graphics>().water_effects,
            |w, on| w.resource_mut::<Graphics>().water_effects = on,
        ),
        (
            "water-mirror",
            "Mirrored reflections",
            "On: water mirrors the world above it, as in the original, drawing the scene a second time. Off: it reflects the sky.",
            "--graphics-only",
            |w| w.resource::<Graphics>().water_mirror,
            |w, on| w.resource_mut::<Graphics>().water_mirror = on,
        ),
        // #2661: off in both looks and not part of `enhanced`; switched while the game runs
        // (mesh_terrain_blend.rs `follow_setting`); the test aid NVF_PLAY_TEST_MESH_BLEND
        // overrides it.
        (
            MESH_BLEND_KEY,
            "Blend meshes into ground",
            MESH_BLEND_NOTE,
            "--graphics-only",
            |w| w.resource::<Graphics>().mesh_terrain_blend,
            |w, on| w.resource_mut::<Graphics>().mesh_terrain_blend = on,
        ),
    ];
    for (key, label, note, switches, get, set) in switches {
        add_setting(app, Setting { key, group: GROUP_GRAPHICS, label, note, switches, get, set, slider: None });
    }
    // Read once at the start like Authored materials; the row keeps the next start's choice
    // (gpu_occlusion.rs).
    crate::gpu_occlusion::add_row(app, GROUP_GRAPHICS);
    nvf_play::settings::set_needs_in_game(app, "terrain-parallax", |w| parallax_status(nvf_assets::authored::enabled(), w.get_resource::<Graphics>().is_some_and(|g| g.authored_materials)));
    // #3032 stage 3: the light in the air's strength, 0 to 200 % in steps of 10 (greyed while
    // its switch is off: `settings_layout::NEEDS`); Sun shafts are greyed while the light is
    // wanted at a strength above 0 (they yield to it where it draws, shafts.rs): the game's page
    // alone, for the launcher's shared rules know no slider's value.
    nvf_play::settings::set_needs_in_game(app, "light-shafts", |w| {
        use crate::medium_air_demand::{AirStrengthSetting, AirWanted};
        #[cfg(any(test, debug_assertions, feature = "dev-aids"))]
        let aid = w.get_resource::<crate::medium_air_demand::AirAid>().is_some_and(|a| a.0);
        #[cfg(not(any(test, debug_assertions, feature = "dev-aids")))]
        let aid = false;
        shafts_status(w.get_resource::<AirWanted>().is_some_and(|a| a.0), aid, w.get_resource::<AirStrengthSetting>().copied().unwrap_or_default().0)
    });
    add_setting(app, Setting::slider(VOLUMETRIC_LIGHT_STRENGTH_KEY, GROUP_GRAPHICS, "Volumetric light strength", VOLUMETRIC_LIGHT_STRENGTH_NOTE, air_strength_slider()));
}

/// Why the Sun shafts row cannot apply: the light in the air is wanted (so drawn on this device)
/// at a strength above 0 (the debug aid's always is); at 0 the shafts draw.
fn shafts_status(wanted: bool, aid: bool, strength: f32) -> Option<&'static str> {
    (wanted && (aid || strength > 0.0)).then_some("Replaced by Volumetric Light.")
}

/// The Volumetric light row's key (#3032 stage 3; graphics.rs `Graphics::volumetric_light`).
pub(crate) const VOLUMETRIC_LIGHT_KEY: &str = "volumetric-light";
/// Its strength's slider (medium_air_demand.rs `AirStrengthSetting`).
pub(crate) const VOLUMETRIC_LIGHT_STRENGTH_KEY: &str = "volumetric-light-strength";

/// The Volumetric light row's note: what it does and what it looks best with (its rays are
/// cut by the Cloud shadows image and the Terrain shadows map, medium_air_grid.rs) [design].
pub(crate) const VOLUMETRIC_LIGHT_NOTE: &str = "On: sunlight and moonlight show in the air as rays and a glow toward a low sun. An addition of NVForever's.";

/// The strength slider's note [design].
pub(crate) const VOLUMETRIC_LIGHT_STRENGTH_NOTE: &str = "How strongly the light in the air brightens lit air and darkens shaded air: 100% is the look as tuned, 0% none.";

/// The strength's slider: the part strength / 2 (0..=1) in steps of 0.05 (10 %), shown as
/// 0 to 200 %; read from and written to `AirStrengthSetting` (made when it is not there yet:
/// the INI is read before every plugin's resource may exist) [design].
fn air_strength_slider() -> Slider {
    use crate::medium_air_demand::AirStrengthSetting;
    Slider {
        get: |w| w.get_resource::<AirStrengthSetting>().copied().unwrap_or_default().part(),
        set: |w, v| {
            let next = AirStrengthSetting::from_part(v);
            match w.get_resource_mut::<AirStrengthSetting>() {
                Some(mut s) => {
                    s.set_if_neq(next);
                }
                None => w.insert_resource(next),
            }
        },
        min: 0.0,
        step: 0.05,
        shown: Some(AirStrengthSetting::shown),
    }
}

/// The Terrain parallax row's availability in the game, by the run's Authored materials
/// state (`session`, nvf_assets `authored::enabled`, fixed at the start), not the row's
/// saved value (`row`, which the launcher's NEEDS rule follows for the next start): free
/// while the run has authored materials, even with the row turned off for the next start
/// (the height maps stay loaded); greyed with a restart note while the row is on but the run
/// started without them; the NEEDS words while both are off [design, #3322 review].
pub(crate) fn parallax_status(session: bool, row: bool) -> Option<&'static str> {
    match (session, row) {
        (true, _) => None,
        (false, true) => Some(PARALLAX_RESTART),
        (false, false) => Some(PARALLAX_NEEDS),
    }
}

/// The NEEDS words of the Terrain parallax row (nvf_settings_layout `NEEDS`).
pub(crate) const PARALLAX_NEEDS: &str = "Needs Authored Materials.";

/// What the Terrain parallax row says while Authored materials waits for the next start.
pub(crate) const PARALLAX_RESTART: &str = "After a restart (Authored Materials)";

/// The Authored materials row's note (#2981): it takes effect at the next start, as the run's
/// state is fixed before the first material (main.rs).
pub(crate) const AUTHORED_MATERIALS_NOTE: &str = "On: a material pack's maps (textures\\pbr\\) shade its surfaces. Off: the game's own values. Takes effect at the next start.";

/// The command-line switches the Authored materials row weighs against the INI at the start
/// (`authored_after_ini`).
pub(crate) const AUTHORED_MATERIALS_SWITCHES: &str = "--graphics-only";

/// The run's Authored materials state (#2981), decided before the first material: the
/// command line's (`--graphics-only no-authored-materials`, else on), then the INI's stored
/// row by the settings' own precedence (`nvf_play::settings::switch_after_ini`, as
/// `apply_stored` decides at PostStartup), so the saved choice of the last run applies from
/// the start. `given`: the switches a `--play` run was given (`None`: no settings page, the
/// INI is not read).
pub(crate) fn authored_after_ini(command_line: bool, given: Option<&nvf_play::settings::GivenSwitches>) -> bool {
    let Some(given) = given else { return command_line };
    let stored = stored_at_start(crate::graphics::AUTHORED_MATERIALS_KEY).and_then(|v| nvf_play::settings::parse_switch(&v));
    nvf_play::settings::switch_after_ini(command_line, stored, given, AUTHORED_MATERIALS_SWITCHES)
}

/// The Physical lighting row's note (graphics-enhanced.md §3.25).
const PHYSICAL_PBR_NOTE: &str = "On: light, sky, fog and exposure follow physical rules instead of the original's display values. Still being tuned. Takes effect at the next start. Not in the original.";

/// The run's Physical PBR lighting state, decided before the first shader hook and material
/// by [`authored_after_ini`]'s rule: the command line's (`--graphics-only physical-pbr`),
/// then the INI's stored row. In a build with the developer aids (dev_aids.rs),
/// `NVF_PHYSICAL_PBR=1` or `0` overrides both, for comparing.
pub(crate) fn physical_pbr_after_ini(command_line: bool, given: Option<&nvf_play::settings::GivenSwitches>) -> bool {
    if let Some(v) = crate::dev_aids::var("NVF_PHYSICAL_PBR") {
        return v.trim() == "1";
    }
    let Some(given) = given else { return command_line };
    let stored = stored_at_start("physical-pbr").and_then(|v| nvf_play::settings::parse_switch(&v));
    nvf_play::settings::switch_after_ini(command_line, stored, given, "--graphics-only")
}

/// The Reflections on authored materials row's note (#2981 reflections): it needs a pack and
/// Authored materials, and takes effect at the next start (main.rs).
pub(crate) const AUTHORED_REFLECTIONS_NOTE: &str = "On: pack materials reflect the sky outside and nearby lights inside. Needs Authored materials on. Takes effect at the next start.";

/// The run's Reflections on authored materials state (#2981 reflections), decided before the
/// first NIF pipeline by [`authored_after_ini`]'s rule: the command line's
/// (`--graphics-only no-authored-reflections`, else on), then the INI's stored row.
pub(crate) fn reflections_after_ini(command_line: bool, given: Option<&nvf_play::settings::GivenSwitches>) -> bool {
    let Some(given) = given else { return command_line };
    let stored = stored_at_start(crate::graphics::AUTHORED_REFLECTIONS_KEY).and_then(|v| nvf_play::settings::parse_switch(&v));
    nvf_play::settings::switch_after_ini(command_line, stored, given, AUTHORED_MATERIALS_SWITCHES)
}

/// The Ambient occlusion row's key and note (ssao.rs `AoLevel`).
const AO_KEY: &str = "ambient-occlusion";
const AO_NOTE: &str = "Creases, corners and the ground under things darken by how enclosed they are: Off, Performance (cheaper, a little grainier) or Quality. Not in the original.";

/// The Ambient occlusion row's choice now: Off, or the level it runs at.
fn ao_chosen(world: &World) -> String {
    let g = world.resource::<Graphics>();
    if g.ambient_occlusion { g.ambient_occlusion_level.name() } else { AO_CHOICES[0] }.to_string()
}

/// The page's, the INI's and the test aids' Ambient occlusion: one of [`AO_CHOICES`] (any
/// case), the row's older on/off as Quality and Off, or an older slider's number (0 Off, 1
/// Quality); false otherwise.
fn take_ao(world: &mut World, text: &str) -> bool {
    let names: Vec<String> = AO_CHOICES.iter().map(|c| (*c).to_string()).collect();
    let choice = nvf_play::settings::choice_of(text, &names, |v| choice_index(v, AO_CHOICES.len()))
        .or_else(|| nvf_play::settings::parse_switch(text).map(|on| if on { AO_CHOICES.len() - 1 } else { 0 }));
    let Some(choice) = choice else { return false };
    let mut g = world.resource_mut::<Graphics>();
    let level = if choice == 1 { AoLevel::Performance } else { AoLevel::Quality };
    // Off keeps the level for the next On, as Sun shadows keeps its mode.
    if choice == 0 {
        if g.ambient_occlusion {
            g.ambient_occlusion = false;
        }
    } else if !g.ambient_occlusion || g.ambient_occlusion_level != level {
        g.ambient_occlusion = true;
        g.ambient_occlusion_level = level;
    }
    true
}

/// The Volumetric clouds row's key and note (volumetric_clouds.rs `CloudLevel`).
const CLOUDS_KEY: &str = "volumetric-clouds";
const CLOUDS_NOTE: &str = "The weather's clouds drawn with depth and lit by the sun: Off, or Performance, Normal or High. A prototype. Not in the original.";

/// The Volumetric clouds row's choice now: Off, or the level the clouds are drawn at.
fn clouds_chosen(world: &World) -> String {
    let g = world.resource::<Graphics>();
    if g.volumetric_clouds { g.volumetric_clouds_level.name() } else { CLOUD_CHOICES[0] }.to_string()
}

/// The page's, the INI's and the test aids' Volumetric clouds: one of [`CLOUD_CHOICES`] (any
/// case), or the row's older on/off as Normal and Off (it was never a slider, so no other
/// number is read); false otherwise.
fn take_clouds(world: &mut World, text: &str) -> bool {
    let level = match text.trim() {
        t if t.eq_ignore_ascii_case(CloudLevel::Performance.name()) => Some(CloudLevel::Performance),
        t if t.eq_ignore_ascii_case(CloudLevel::Normal.name()) => Some(CloudLevel::Normal),
        t if t.eq_ignore_ascii_case(CloudLevel::High.name()) => Some(CloudLevel::High),
        t => match nvf_play::settings::parse_switch(t) {
            Some(on) => on.then_some(CloudLevel::Normal),
            None => return false,
        },
    };
    let mut g = world.resource_mut::<Graphics>();
    // Off keeps the level, as the Ambient occlusion row does.
    let want = (level.is_some(), level.unwrap_or(g.volumetric_clouds_level));
    if (g.volumetric_clouds, g.volumetric_clouds_level) != want {
        (g.volumetric_clouds, g.volumetric_clouds_level) = want;
    }
    true
}

/// The Cloud shadows row's note (#2670; docs/design/sun-visibility.md): the label says the
/// feature is experimental (as 3D scopes' says "(test)"), the note that its look is still
/// being tuned (the relative rule, §7a, and the calibration, §8).
const CLOUD_SHADOWS_NOTE: &str = "On: the weather's clouds cast soft shadows that drift over the land, only where the cover is heavier than around it. Still being tuned. Not in the original.";

/// The Terrain shadows row's note (#2671; docs/design/terrain-shadows.md): experimental
/// like Cloud shadows, its softness and bias still being tuned.
const TERRAIN_SHADOWS_NOTE: &str = "On: at a low sun, hills and mountains shade the land beyond the reach of the sun's shadows. Still being tuned. Not in the original.";

fn register_pipeline_waits(app: &mut App) {
    // Cloud shadows and Terrain shadows switch the world views' material pipelines (the sun
    // visibility's view key bits, sun_visibility.rs), so Continue waits for them like the
    // others.
    for key in [
        "anti-aliasing", "shadow-filter", "upscaler", "upscaler-quality", "ambient-occlusion",
        "fine-shadows", "bloom", "glow", "eye-adaptation", "neon-glow", "sharpen",
        "light-shafts", "water-reflections", "water-effects", "water-mirror", "sun-shadows", "grade",
        "terrain-height-blend", "terrain-anti-tiling", "terrain-parallax", "cloud-shadows", "terrain-shadows", "heat-haze", CLOUDS_KEY, VOLUMETRIC_LIGHT_KEY, MESH_BLEND_KEY, "distant-lights",
        // ALC shadows' first point and spot light shadow views specialize their passes.
        "light-shadows",
    ] {
        nvf_play::settings::wait_for_pipelines(app, key);
    }
}

/// The DLSS model row's choice: the model's name (`DlssModel::name`).
fn model_chosen(world: &World) -> String {
    world.resource::<Graphics>().dlss_model.name().to_string()
}

/// The page's and the test aids' pick: one of the five names (any case); false otherwise.
fn pick_model(world: &mut World, name: &str) -> bool {
    let Some(model) = DlssModel::named(name) else { return false };
    world.resource_mut::<Graphics>().dlss_model = model;
    true
}

/// The FSR library row's two choices; the second is shown with the library found.
const FSR_LIBRARY_BUILT_IN: &str = "Built-in";
const FSR_LIBRARY_FOLDER: &str = "Game folder";

/// The choices as shown: the built-in upscaler, and the folder's library by name
/// (`ffx_api::found`: the one loaded, the one that failed, or none).
fn fsr_library_names() -> [String; 2] {
    [FSR_LIBRARY_BUILT_IN.to_string(), format!("{FSR_LIBRARY_FOLDER} ({})", crate::ffx_api::found())]
}

/// Takes a choice by its name, with or without the library named after it.
fn pick_fsr_library(name: &str) -> bool {
    let name = name.trim().to_ascii_lowercase();
    let folder = if name == FSR_LIBRARY_BUILT_IN.to_ascii_lowercase() {
        false
    } else if name.starts_with(&FSR_LIBRARY_FOLDER.to_ascii_lowercase()) {
        true
    } else {
        return false;
    };
    crate::ffx_api::set_enabled(folder);
    true
}

/// The FSR library row stands dimmed while FSR is not the effective upscaler
/// (`upscaler::effective`), as the DLSS model row does for DLSS, and always with
/// `NVF_FFX_API=0`. The choice is kept meanwhile.
fn fsr_library_dimmed(world: &World) -> Option<&'static str> {
    if !crate::ffx_api::allowed() {
        return Some("Built-in (NVF_FFX_API=0)");
    }
    crate::upscaler::effective(world).fsr().is_none().then_some("FSR only")
}

/// The INI's text: one of the five names, else Default (said once in the log), so a stored
/// value from elsewhere never leaves an unknown model.
fn load_model(world: &mut World, value: &str) -> bool {
    let model = DlssModel::named(value).unwrap_or_else(|| {
        let names: Vec<&str> = DlssModel::ALL.iter().map(|m| m.name()).collect();
        warn!("settings: {MODEL_KEY}={} is not one of {}: DLSS model {}", value.trim(), names.join(", "), DlssModel::Default.name());
        DlssModel::Default
    });
    world.resource_mut::<Graphics>().dlss_model = model;
    true
}

/// The DLSS model row stands dimmed while DLSS is not the effective upscaler
/// (`upscaler::effective`): FSR or off draws, DLSS waits for a restart or cannot run. The model
/// is kept meanwhile.
fn model_dimmed(world: &World) -> Option<&'static str> {
    crate::upscaler::effective(world).dlss().is_none().then_some("DLSS only")
}

/// The Upscaler row's choices by the names the INI, the log and the test aids use (the page
/// shows them by [`upscaler_label`]).
const UPSCALERS: [(&str, Upscaler); 3] = [("Off", Upscaler::Off), ("FSR 3.1", Upscaler::Fsr), ("DLSS", Upscaler::Dlss)];

/// The quality the Upscaling mode row holds while the upscaler is Off, when the request
/// (`Graphics::upscaler`) has none: an upscaler chosen again takes it up.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct KeptQuality(pub UpscaleQuality);

/// What the two rows hold: the request, and the quality kept for while it is Off.
type RowsState = (UpscalerChoice, UpscaleQuality);

fn rows_state(world: &World) -> RowsState {
    (world.resource::<Graphics>().upscaler, world.get_resource::<KeptQuality>().map_or_else(UpscaleQuality::default, |k| k.0))
}

fn set_rows_state(world: &mut World, (choice, kept): RowsState) {
    world.resource_mut::<Graphics>().upscaler = choice;
    world.insert_resource(KeptQuality(kept));
}

/// The quality the quality row shows: the request's, or the one kept while it is Off.
fn shown_quality((choice, kept): RowsState) -> UpscaleQuality {
    choice.quality().unwrap_or(kept)
}

/// `upscaler` chosen, at the quality the quality row shows.
fn with_upscaler(state: RowsState, upscaler: Upscaler) -> RowsState {
    let q = shown_quality(state);
    (UpscalerChoice::new(upscaler, q), q)
}

/// The quality `q` chosen: the request's when it has one, else kept for later.
fn with_quality((choice, _): RowsState, q: UpscaleQuality) -> RowsState {
    (UpscalerChoice::new(choice.upscaler(), q), q)
}

/// The whole of `choice` chosen (the older row's choices).
fn with_choice(state: RowsState, choice: UpscalerChoice) -> RowsState {
    (choice, choice.quality().unwrap_or(shown_quality(state)))
}

/// The Upscaler row's choice of this name (any case).
fn upscaler_named(name: &str) -> Option<Upscaler> {
    UPSCALERS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name.trim())).map(|(_, u)| *u)
}

/// The quality of this name (any case).
fn quality_named(name: &str) -> Option<UpscaleQuality> {
    UpscaleQuality::ALL.into_iter().find(|q| q.name().eq_ignore_ascii_case(name.trim()))
}

/// The older row's text read as it read it: one of its eleven names, or a number written by
/// the early local slider build of the FSR row (a part of its six FSR choices, which is all
/// that build had). `None`: not one of its states.
pub(crate) fn parse_legacy(value: &str) -> Option<UpscalerChoice> {
    if let Some((_, choice)) = graphics::FSR_UPSCALING_CHOICES.iter().find(|(n, _)| n.eq_ignore_ascii_case(value.trim())) {
        return Some(*choice);
    }
    let part = value.trim().parse::<f32>().ok().filter(|v| v.is_finite() && (0.0..=1.0).contains(v))?;
    Some(UpscalerChoice::from_fsr(graphics::FSR_CHOICES[choice_index(part, graphics::FSR_CHOICES.len())].1.quality()))
}

/// The older row's value (`fsr-upscaling`) as the two rows' own values, for
/// `nvf_play::settings::set_older_split`: the Upscaler row's name and, for FSR or DLSS, the
/// quality's. So in a test run whose command line keeps both rows, both keep the stored value
/// and a save writes it, not the command line's quality beside the stored upscaler (the review
/// of 2026-11-12). Off has no quality: the quality row keeps its own. Nothing for a value that
/// is not one of the older row's (the Upscaler row then reads it as it is, and skips it).
fn split_legacy(value: &str) -> Vec<(&'static str, String)> {
    let Some(choice) = parse_legacy(value) else { return Vec::new() };
    let Some((name, _)) = UPSCALERS.iter().find(|(_, u)| *u == choice.upscaler()) else { return Vec::new() };
    let mut rows = vec![(UPSCALER_KEY, (*name).to_string())];
    if let Some(q) = choice.quality() {
        rows.push((QUALITY_KEY, q.name().to_string()));
    }
    rows
}

/// The Upscaler row's INI text applied to `state`: one of its own names (`Off`, `FSR 3.1`,
/// `DLSS`) at the quality shown, or the older `fsr-upscaling` row's text, a whole choice (the
/// row reads that key while the INI lacks its own). The start reads the INI with it too
/// ([`upscaler_after_ini`]), so the two cannot disagree.
fn upscaler_text(state: RowsState, text: &str) -> Option<RowsState> {
    match upscaler_named(text) {
        Some(upscaler) => Some(with_upscaler(state, upscaler)),
        None => parse_legacy(text).map(|choice| with_choice(state, choice)),
    }
}

/// The quality row's INI text applied to `state`.
fn quality_text(state: RowsState, text: &str) -> Option<RowsState> {
    quality_named(text).map(|q| with_quality(state, q))
}

/// The requested upscaler's name in the Upscaler row.
/// The anti-aliasing `text` names (or an older slider's part stands for).
fn take_aa(w: &mut World, text: &str) -> bool {
    let names: Vec<String> = graphics::AA_CHOICES.iter().map(|c| c.0.to_string()).collect();
    match nvf_play::settings::choice_of(text, &names, |v| choice_index(v, graphics::AA_CHOICES.len())) {
        Some(i) => {
            graphics::set_aa_choice(w, i);
            true
        }
        None => false,
    }
}

/// The Full-detail scenery row's key (#2701).
pub(crate) const FULL_DETAIL_KEY: &str = "full-detail";

fn full_detail_names() -> Vec<String> {
    crate::full_detail::FullDetail::CHOICES.iter().map(|c| c.0.to_string()).collect()
}

fn full_detail_chosen(w: &World) -> String {
    w.resource::<Graphics>().full_detail.name().to_string()
}

/// The full-detail ring `text` names (or a slider's part stands for).
fn take_full_detail(w: &mut World, text: &str) -> bool {
    use crate::full_detail::FullDetail;
    match nvf_play::settings::choice_of(text, &full_detail_names(), |v| choice_index(v, FullDetail::CHOICES.len())) {
        Some(i) => {
            w.resource_mut::<Graphics>().full_detail = FullDetail::CHOICES[i].1;
            true
        }
        None => false,
    }
}

/// The Distant objects row's key (#3014).
pub(crate) const DISTANT_OBJECTS_KEY: &str = "distant-objects";

fn distant_objects_names() -> Vec<String> {
    crate::distant_objects::DistantObjects::CHOICES.iter().map(|c| c.0.to_string()).collect()
}

fn distant_objects_chosen(w: &World) -> String {
    w.resource::<Graphics>().distant_objects.name().to_string()
}

/// The distant objects' reach `text` names (or a slider's part stands for).
fn take_distant_objects(w: &mut World, text: &str) -> bool {
    use crate::distant_objects::DistantObjects;
    match nvf_play::settings::choice_of(text, &distant_objects_names(), |v| choice_index(v, DistantObjects::CHOICES.len())) {
        Some(i) => {
            w.resource_mut::<Graphics>().distant_objects = DistantObjects::CHOICES[i].1;
            true
        }
        None => false,
    }
}

fn shadow_filter_chosen(w: &World) -> String {
    let f = w.resource::<Graphics>().shadow_filter;
    graphics::SHADOW_FILTERS[graphics::SHADOW_FILTERS.iter().position(|&(_, m)| m == f).unwrap_or(1)].0.to_string()
}

/// The shadow filtering `text` names (or an older slider's part stands for).
fn take_shadow_filter(w: &mut World, text: &str) -> bool {
    let names: Vec<String> = graphics::SHADOW_FILTERS.iter().map(|f| f.0.to_string()).collect();
    match nvf_play::settings::choice_of(text, &names, |v| choice_index(v, graphics::SHADOW_FILTERS.len())) {
        Some(i) => {
            w.resource_mut::<Graphics>().shadow_filter = graphics::SHADOW_FILTERS[i].1;
            true
        }
        None => false,
    }
}

fn shadow_resolution_chosen(w: &World) -> String {
    graphics::SHADOW_RESOLUTIONS[graphics::shadow_resolution_choice(w)].0.to_string()
}

/// The shadow resolution `text` names (or an older slider's part stands for: 0 is 2048, 1
/// is 4096).
fn take_shadow_resolution(w: &mut World, text: &str) -> bool {
    let names: Vec<String> = graphics::SHADOW_RESOLUTIONS.iter().map(|r| r.0.to_string()).collect();
    match nvf_play::settings::choice_of(text, &names, |v| choice_index(v, graphics::SHADOW_RESOLUTIONS.len())) {
        Some(i) => {
            graphics::set_shadow_resolution_choice(w, i);
            true
        }
        None => false,
    }
}

/// The ALCS (experimental) row's note (light_shadows.rs; ALCS: All Lights Cast Shadows; the
/// row was ALC shadows until 2026-11-24 and Lamp shadows until 2026-11-19; the user-visible
/// name since 2026-11-19, once Lamp shadows; the INI key stays `light-shadows`).
const LIGHT_SHADOWS_NOTE: &str = "ALCS (All Lights Cast Shadows): lamps, fires and spot lights cast shadows in the shadow distance; Performance casts fewer. Costs frame rate. Not in the original.";

fn light_shadows_chosen(w: &World) -> String {
    let choice = w.get_resource::<crate::light_shadows::LightShadows>().map_or(0, |l| l.choice);
    crate::light_shadows::CHOICES[choice.min(crate::light_shadows::CHOICES.len() - 1)].to_string()
}

/// The ALC shadows choice `text` names.
fn take_light_shadows(w: &mut World, text: &str) -> bool {
    let names: Vec<String> = crate::light_shadows::CHOICES.iter().map(|c| c.to_string()).collect();
    match nvf_play::settings::choice_of(text, &names, |v| choice_index(v, crate::light_shadows::CHOICES.len())) {
        Some(i) => {
            w.insert_resource(crate::light_shadows::LightShadows { choice: i });
            true
        }
        None => false,
    }
}

/// The Shadow Distance row's choices: 0% (no shadow range) to 100% in steps of 20%
/// [design: owner 2026-11-15, the menu overhaul; a 10-100% slider before].
pub(crate) const SHADOW_DISTANCES: [f32; 6] = [0.0, 0.2, 0.4, 0.6, 0.8, 1.0];

fn shadow_distance_names() -> Vec<String> {
    SHADOW_DISTANCES.iter().map(|v| format!("{:.0}%", v * 100.0)).collect()
}

fn shadow_distance_chosen(w: &World) -> String {
    shadow_distance_names()[nvf_play::settings::nearest(&SHADOW_DISTANCES, w.resource::<DrawDistances>().shadows)].clone()
}

/// The shadow distance `text` names (or an older slider's part, the nearest).
fn take_shadow_distance(w: &mut World, text: &str) -> bool {
    match nvf_play::settings::choice_of(text, &shadow_distance_names(), |v| nvf_play::settings::nearest(&SHADOW_DISTANCES, v)) {
        Some(i) => {
            draw_distance::set_shadows(w, SHADOW_DISTANCES[i]);
            true
        }
        None => false,
    }
}

fn upscaler_chosen(world: &World) -> String {
    let upscaler = world.get_resource::<Graphics>().map_or(Upscaler::Off, |g| g.upscaler.upscaler());
    UPSCALERS.iter().find(|(_, u)| *u == upscaler).map_or("Off", |(name, _)| *name).to_string()
}

/// The Upscaler row's choices: Off, FSR 3.1, DLSS, and Download DLSS… while DLSS's files are
/// missing (`dlss_download`: picking it opens its dialog; #2250).
fn upscaler_names(world: &World) -> Vec<String> {
    let mut names: Vec<String> = UPSCALERS.iter().map(|(name, _)| (*name).to_string()).collect();
    if dlss_download_offered(world) {
        names.push(crate::dlss_download::CHOICE.to_string());
    }
    names
}

/// The pick: an upscaler, or Download DLSS…, which opens its dialog and leaves the request
/// (`Graphics::upscaler`) as it is.
fn pick_upscaler(world: &mut World, name: &str) -> bool {
    if crate::dlss_download::is_choice(name) {
        if !dlss_download_offered(world) {
            return false;
        }
        crate::dlss_download::ask(world);
        return true;
    }
    let Some(upscaler) = upscaler_named(name) else { return false };
    set_rows_state(world, with_upscaler(rows_state(world), upscaler));
    true
}

/// Loads the INI's text, also a DLSS choice DLSS cannot meet this run: the request is kept
/// and native draws with the reason (`upscaler::resolve`).
fn load_upscaler(world: &mut World, value: &str) -> bool {
    let Some(state) = upscaler_text(rows_state(world), value) else { return false };
    set_rows_state(world, state);
    true
}

fn quality_chosen(world: &World) -> String {
    shown_quality(rows_state(world)).name().to_string()
}

/// The quality row's pick and load (the page holds the row dimmed while the upscaler is Off;
/// the INI's text then sets the quality kept for later).
fn load_quality(world: &mut World, value: &str) -> bool {
    let Some(state) = quality_text(rows_state(world), value) else { return false };
    set_rows_state(world, state);
    true
}

/// The share of the screen's width and height (in percent) the world is drawn at in `mode` with
/// `upscaler`; `None` with the upscaler Off. FSR's from its own ratio (`fsr::FsrQuality::ratio`,
/// what `render_size` divides by); DLSS's from the render sizes DLSS 310.9.16 chose at
/// 1280x720: 1280x720, 853x480, 742x418, 640x360, 427x240 [observed, 2026-11-12:
/// the DLSS runs recorded with #2206], since DLSS's own sizes are known only once Streamline
/// answers for a view (`dlss::sizing`) and may differ with the output size and DLSS's version.
fn render_share(upscaler: Upscaler, mode: UpscaleQuality) -> Option<u32> {
    match upscaler {
        Upscaler::Off => None,
        Upscaler::Fsr => Some((100.0 / mode.ratio()).round() as u32),
        Upscaler::Dlss => Some(match mode {
            UpscaleQuality::NativeAa => 100,
            UpscaleQuality::Quality => 67,
            UpscaleQuality::Balanced => 58,
            UpscaleQuality::Performance => 50,
            UpscaleQuality::UltraPerformance => 33,
        }),
    }
}

/// The words the Upscaling mode row shows for a mode: its name in the language shown and, with
/// an upscaler asked for (`Graphics::upscaler`), the share it draws at, `Quality (67%)`
/// [design: the user's, 2026-11-12]; with the upscaler Off (the row dimmed) the name alone.
/// The INI, the log and the test aids keep the plain names.
fn quality_label(world: &World, name: &str) -> String {
    let shown = fnv_util::words::tr_str(name).to_string();
    let upscaler = world.get_resource::<Graphics>().map_or(Upscaler::Off, |g| g.upscaler.upscaler());
    match quality_named(name).and_then(|mode| render_share(upscaler, mode)) {
        Some(share) => format!("{shown} ({share}%)"),
        None => shown,
    }
}

/// The words the Upscaler row shows for a choice: `Off` in the language shown, `FSR 3.1`,
/// `DLSS`; the requested DLSS as `DLSS (restart)` while a restart is pending for it
/// (`upscaler::UpscalerStatus::restart_pending`; the row's note says why); Download DLSS…, or
/// how far its download is (`dlss_download::choice_words`).
fn upscaler_label(world: &World, name: &str) -> String {
    use fnv_util::words::tr_str;
    if crate::dlss_download::is_choice(name) {
        return crate::dlss_download::choice_words();
    }
    match upscaler_named(name) {
        Some(Upscaler::Off) => tr_str("Off").to_string(),
        Some(Upscaler::Dlss) => {
            let pending = world.get_resource::<crate::upscaler::UpscalerStatus>().is_some_and(|s| s.restart_pending());
            if pending && world.get_resource::<Graphics>().is_some_and(|g| g.upscaler.is_dlss()) {
                tr_str("DLSS (restart)").to_string()
            } else {
                "DLSS".to_string()
            }
        }
        Some(Upscaler::Fsr) | None => name.to_string(),
    }
}

/// Why DLSS cannot be taken this run (`upscaler::Unavailable::words`), by the status's rule.
/// Off and FSR are always in reach (FSR on a device without it draws natively, as before).
/// A stored DLSS choice still loads from the INI.
fn dlss_unavailable(world: &World) -> Option<String> {
    dlss_reason(world).map(|why| why.words().to_string())
}

/// Why DLSS cannot be taken this run, by the status's rule ([`dlss_unavailable`]'s reason).
fn dlss_reason(world: &World) -> Option<crate::upscaler::Unavailable> {
    match world.get_resource::<crate::upscaler::UpscalerStatus>() {
        Some(status) => status.dlss_unavailable(),
        None => match crate::upscaler::dlss_state_now() {
            crate::upscaler::DlssState::Unavailable(why) => Some(why),
            _ => None,
        },
    }
}

/// [design] Offer the download for missing upscaling or frame generation files, without
/// changing the upscaler's availability (`dlss::FILES` remains its four-file requirement).
pub(crate) fn dlss_download_offered(world: &World) -> bool {
    crate::dlss_download::offered(dlss_reason(world))
}

fn upscaler_unavailable(world: &World, name: &str) -> Option<String> {
    (upscaler_named(name)? == Upscaler::Dlss).then(|| dlss_unavailable(world)).flatten()
}

/// The older row's name for the request now (the test aid's `settings: fsr-upscaling <name>`).
fn legacy_chosen(world: &World) -> String {
    let requested = world.resource::<Graphics>().upscaler;
    graphics::FSR_UPSCALING_CHOICES.iter().find(|(_, choice)| *choice == requested).map_or("Off", |(name, _)| *name).to_string()
}

/// The test aid's `fsr-upscaling=<name>`: the older row's whole choice, set on both rows.
fn pick_legacy(world: &mut World, name: &str) -> bool {
    let Some((_, choice)) = graphics::FSR_UPSCALING_CHOICES.iter().find(|(n, _)| n.eq_ignore_ascii_case(name.trim())) else { return false };
    set_rows_state(world, with_choice(rows_state(world), *choice));
    true
}

fn legacy_unavailable(world: &World, name: &str) -> Option<String> {
    let (_, choice) = graphics::FSR_UPSCALING_CHOICES.iter().find(|(n, _)| n.eq_ignore_ascii_case(name.trim()))?;
    choice.is_dlss().then(|| dlss_unavailable(world)).flatten()
}

/// The INI's texts for the two rows, as `nvf_play::settings::apply_stored` finds them:
/// `upscaler` and `upscaler-quality`, the older `fsr-upscaling` split into them first where
/// the INI lacks `upscaler` ([`split_legacy`], `nvf_play::settings::with_older_split`), else
/// read as it is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct UpscalerIni {
    pub upscaler: Option<String>,
    pub quality: Option<String>,
}

impl UpscalerIni {
    /// From an INI's text.
    pub(crate) fn read(text: &str) -> Self {
        let text = nvf_play::settings::with_older_split(text, UPSCALER_KEY, LEGACY_KEY, split_legacy);
        let get = |key: &str| fnv_install::ini_get(&text, nvf_play::settings::SECTION, key);
        UpscalerIni { upscaler: get(UPSCALER_KEY).or_else(|| get(LEGACY_KEY)), quality: get(QUALITY_KEY) }
    }
}

/// The upscaler this run starts with, decided before the renderer exists (the DLSS loader runs
/// for a DLSS choice; `main`): the command line's choice, then the INI's two rows by
/// `nvf_play::settings::apply_stored`'s order (Upscaler, then quality) and precedence
/// (`after_ini`), each read with its row's own rule ([`upscaler_text`], [`quality_text`]).
/// `given`: the switches this run was given, `None` when it has no settings page (not
/// `--play`: the INI is not applied, the command line decides).
pub(crate) fn upscaler_after_ini(command_line: UpscalerChoice, ini: Option<&UpscalerIni>, given: Option<&nvf_play::settings::GivenSwitches>) -> UpscalerChoice {
    let Some(given) = given else { return command_line };
    // The quality kept while Off starts as the rows' (KeptQuality's default).
    let start = (command_line, UpscaleQuality::default());
    let stored = ini.filter(|ini| ini.upscaler.is_some() || ini.quality.is_some()).map(|ini| {
        let after_upscaler = ini.upscaler.as_deref().and_then(|t| upscaler_text(start, t.trim())).unwrap_or(start);
        ini.quality.as_deref().and_then(|t| quality_text(after_upscaler, t.trim())).unwrap_or(after_upscaler).0
    });
    nvf_play::settings::after_ini(command_line, stored, given, UPSCALER_SWITCHES)
}

/// The Frame generation row's key and note (#2317).
pub(crate) const FRAME_GENERATION_KEY: &str = "frame-generation";
pub(crate) const FRAME_GENERATION_NOTE: &str = "Frames made between the real ones: smoother motion, input a little later. FSR 3.1 needs FSR upscaling; DLSS an RTX 40 or newer. Not in the original.";

/// The Frame generation row's command-line switch: a test run that gives it keeps the row at
/// the command line's choice over the INI.
pub(crate) const FRAME_GENERATION_SWITCHES: &str = "--frame-generation";

/// The Generated frames row (#2317): DLSS Frame Generation's frames per real frame
/// (`frame_gen::DlssgFrames`), dimmed unless DLSS is the Frame generation choice; 3x and 4x only
/// where the GPU offers them (`dlss::frame_generation_max`, `DLSSGState::numFramesToGenerateMax`).
pub(crate) const FRAMES_KEY: &str = "frame-generation-frames";
pub(crate) const FRAMES_NOTE: &str = "How many frames DLSS frame generation makes per real frame: 2x one, 3x two, 4x three (3x and 4x on RTX 50 series). Not in the original.";
const FRAMES_CHOICES: [&str; 3] = ["2x", "3x", "4x"];

/// The frames generated per real frame of a Generated frames choice (`2x` is one).
fn frames_named(name: &str) -> Option<u32> {
    FRAMES_CHOICES.iter().position(|c| c.eq_ignore_ascii_case(name.trim())).map(|i| i as u32 + 1)
}

fn frames_chosen(world: &World) -> String {
    FRAMES_CHOICES[(world.resource::<crate::frame_gen::DlssgFrames>().0.clamp(1, 3) - 1) as usize].to_string()
}

fn pick_frames(world: &mut World, name: &str) -> bool {
    let Some(frames) = frames_named(name) else { return false };
    world.resource_mut::<crate::frame_gen::DlssgFrames>().0 = frames;
    true
}

/// Real frames a second under which the Generated frames row's note warns ([`FRAMES_FEW_NOTE`]).
const FEW_REAL_FRAMES: u32 = 60;

/// The Generated frames row's note while the frame rate limit divided by the frames shown per
/// real frame leaves fewer than [`FEW_REAL_FRAMES`] real frames a second (#2731): a player found
/// 4x at a limit of 144 (36 real frames) choppy and 2x (72) fine [observed: the report in
/// #2731]. Its `{choice}`, `{limit}` and `{real}` are filled in ([`frames_note_for`]).
pub(crate) const FRAMES_FEW_NOTE: &str = "{choice} at a limit of {limit}: {real} real frames a second, which can feel choppy. Fewer generated frames, or a limit up to the display's refresh rate, give more.";

/// [`FRAMES_FEW_NOTE`] for `frames` generated per real frame (1 for 2x) under a cap of `cap`
/// frames shown a second (0: none), when the real frames left are fewer than
/// [`FEW_REAL_FRAMES`]; `None` otherwise (the row's own note stands).
fn frames_note_for(frames: u32, cap: u32) -> Option<String> {
    let frames = frames.clamp(1, 3);
    let real = cap / (frames + 1);
    (cap > 0 && real < FEW_REAL_FRAMES).then(|| {
        fnv_util::words::trf(FRAMES_FEW_NOTE, &[("choice", &FRAMES_CHOICES[(frames - 1) as usize]), ("limit", &cap), ("real", &real)])
    })
}

/// The Generated frames row's note now ([`frames_note_for`]): with DLSS the Frame generation
/// choice, the frames it generates (the choice, at most what the GPU offers, as
/// `frame_gen::follow_switch` asks for) and the cap frame pacing hands Reflex while it runs
/// (the page's or variable refresh's, `frame_pace::effective_cap`).
fn frames_note_now(world: &World) -> Option<String> {
    if world.resource::<crate::frame_gen::FrameGeneration>().kind != crate::frame_gen::FrameGenKind::Dlss {
        return None;
    }
    let chosen = world.resource::<crate::frame_gen::DlssgFrames>().0.clamp(1, 3);
    let frames = match crate::dlss::frame_generation_max() {
        0 => chosen,
        max => chosen.min(max.max(1)),
    };
    let cap = frame_pace::effective_cap(world.get_resource::<FramePace>().map_or(0, |p| p.cap), world.get_resource::<frame_pace::Refresh>().map_or(0, |r| r.vrr_cap));
    frames_note_for(frames, cap)
}

/// A Generated frames choice the GPU does not offer (once its limit is known).
fn frames_unavailable(_world: &World, name: &str) -> Option<String> {
    let max = crate::dlss::frame_generation_max();
    frames_named(name).filter(|&n| max != 0 && n > max).map(|_| "Not on this GPU".to_string())
}

/// The Generated frames row stands dimmed unless DLSS is the Frame generation choice, or when
/// the GPU makes one frame at most.
fn frames_dimmed(world: &World) -> Option<&'static str> {
    if world.resource::<crate::frame_gen::FrameGeneration>().kind != crate::frame_gen::FrameGenKind::Dlss {
        Some("Needs DLSS frame generation")
    } else if crate::dlss::frame_generation_max() == 1 {
        Some("Not on this GPU")
    } else {
        None
    }
}

/// Off while the frame generation's state is not there (a test app): the page's hidden rows
/// ask every run (`settings_layout::SHOWN_WITH`).
fn frame_generation_chosen(world: &World) -> String {
    world.get_resource::<crate::frame_gen::FrameGeneration>().map_or(crate::frame_gen::FrameGenKind::Off, |f| f.kind).name().to_string()
}

fn frame_generation_saved(world: &World) -> String {
    world.resource::<crate::frame_gen::FrameGeneration>().kind.stored().to_string()
}

/// A Smooth terrain choice by its name or INI text (also the row's older on/off: #2149 kept
/// `1`/`0`), for the next start (nvf_play::smooth_terrain).
fn pick_smoothing(_: &mut World, name: &str) -> bool {
    let Some(s) = nvf_play::smooth_terrain::Smoothing::parse(name) else { return false };
    nvf_play::smooth_terrain::set_setting(s);
    true
}

/// A Frame generation choice by its name or INI text (also the row's old on/off).
fn pick_frame_generation(world: &mut World, name: &str) -> bool {
    let Some(kind) = crate::frame_gen::FrameGenKind::parse(name) else { return false };
    world.resource_mut::<crate::frame_gen::FrameGeneration>().kind = kind;
    true
}

/// Why a Frame generation choice cannot run now: FSR 3.1's reasons (`frame_gen::unavailable_reason`)
/// or DLSS's (`frame_gen::dlss_unavailable_reason`). Off is always in reach.
fn frame_generation_unavailable(world: &World, name: &str) -> Option<String> {
    match crate::frame_gen::FrameGenKind::parse(name)? {
        crate::frame_gen::FrameGenKind::Off => None,
        crate::frame_gen::FrameGenKind::Fsr => crate::frame_gen::unavailable_reason(world).map(str::to_string),
        crate::frame_gen::FrameGenKind::Dlss => crate::frame_gen::dlss_unavailable_reason(world).map(str::to_string),
    }
}

/// The Frame generation choice this run starts with, decided before the renderer exists (DLSS
/// Frame Generation stays loaded from the start only for DLSS; `main`): the command line's,
/// then the INI's row by `nvf_play::settings::after_ini`'s precedence. `given`: `None` without
/// a settings page (not `--play`: the command line decides).
pub(crate) fn frame_generation_after_ini(command_line: crate::frame_gen::FrameGenKind, ini: Option<&str>, given: Option<&nvf_play::settings::GivenSwitches>) -> crate::frame_gen::FrameGenKind {
    let Some(given) = given else { return command_line };
    let stored = ini.and_then(|text| fnv_install::ini_get(text, nvf_play::settings::SECTION, FRAME_GENERATION_KEY)).and_then(|v| crate::frame_gen::FrameGenKind::parse(&v));
    nvf_play::settings::after_ini(command_line, stored, given, FRAME_GENERATION_SWITCHES)
}

/// Distant LOD on or off now; indoors there is none either way, and the door out goes
/// by `Args::no_lod` (`world::travel`).
fn set_lod(world: &mut World, on: bool) {
    world.resource_mut::<Args>().no_lod = !on;
    let outdoors = world.resource::<Streamer>().interior.is_none();
    world.resource_scope(|world, mut lod: Mut<LodState>| {
        lod.set_enabled(&mut world.commands(), on && outdoors);
    });
    world.flush();
}

fn set_overlay(world: &mut World, on: bool) {
    world.resource_mut::<Args>().no_overlay = !on;
    let mut overlays = world.query_filtered::<&mut Visibility, With<Overlay>>();
    for mut v in overlays.iter_mut(world) {
        *v = if on { Visibility::Inherited } else { Visibility::Hidden };
    }
}

#[cfg(test)]
pub(crate) fn register_mobile_test_rows(app: &mut App) {
    register_graphics(app);
    register_frame_cap(app);
}

#[cfg(test)]
mod fsr_settings_tests {
    use super::*;

    /// The shared tables the page and the launcher hide and grey rows by
    /// (`nvf_play::settings_layout::SHOWN_WITH`, `NEEDS_CHOICE`) name choices these lists have.
    #[test]
    fn the_shared_rules_name_real_choices() {
        let upscalers: Vec<&str> = UPSCALERS.iter().map(|(n, _)| *n).collect();
        let frame_gen: Vec<&str> = crate::frame_gen::FrameGenKind::ALL.iter().map(|k| k.name()).collect();
        for (row, parent, choices) in nvf_play::settings_layout::SHOWN_WITH {
            let names = match *parent {
                UPSCALER_KEY => &upscalers,
                FRAME_GENERATION_KEY => &frame_gen,
                other => panic!("{row}: a rule on {other}; name its choices here"),
            };
            assert!(choices.iter().all(|c| names.contains(c)), "{row}: {choices:?} not all of {names:?}");
        }
        assert_eq!(nvf_play::settings_layout::SHOWN_WITH.iter().filter(|(_, p, c)| *p == UPSCALER_KEY && !c.contains(&"Off")).count(), 3, "upscaling mode, DLSS model, FSR library: only with an upscaler");
    }

    // [design] Menu tests install a scoped probe: choice validation and INI snapshots can
    // reach offered indirectly, and neither the real folder nor the global cache is a fixture.

    #[test]
    fn renderer_registration_covers_aa_ao_fsr_and_defaults_without_latching_no_ops() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        use nvf_play::{graphics_resume::GraphicsResume, pause::Paused, settings, PlayState};
        let mut app = App::new();
        app.insert_resource(Graphics::preset(graphics::Preset::Vanilla))
            .insert_resource(graphics::MsaaSetting(Msaa::Off)).init_resource::<GraphicsResume>()
            .insert_resource(Paused(true)).insert_resource(State::new(PlayState::Menu));
        register_graphics(&mut app);
        register_pipeline_waits(&mut app);
        let rows = app.world().resource::<settings::Settings>();
        let aa = rows.index("anti-aliasing").unwrap();
        let ao = rows.index("ambient-occlusion").unwrap();
        let fsr = rows.index(UPSCALER_KEY).unwrap();
        let world = app.world_mut();
        // Anti-Aliasing is a list since the menu overhaul.
        settings::pick(world, aa, "Off");
        settings::pick(world, ao, "Off");
        assert!(!settings::pick(world, fsr, "Off"));
        assert!(world.resource_mut::<GraphicsResume>().request(0.0));
        settings::pick(world, aa, "TAA");
        assert!(!world.resource_mut::<GraphicsResume>().request(0.0));
        world.resource_mut::<GraphicsResume>().reset();
        settings::restore(world, aa, 0.0);
        assert!(!world.resource_mut::<GraphicsResume>().request(0.0));
        world.resource_mut::<GraphicsResume>().reset();
        settings::pick(world, ao, "Performance");
        assert!(!world.resource_mut::<GraphicsResume>().request(0.0));
        world.resource_mut::<GraphicsResume>().reset();
        settings::pick(world, fsr, "FSR 3.1");
        assert!(!world.resource_mut::<GraphicsResume>().request(0.0));
        world.resource_mut::<GraphicsResume>().reset();
        settings::restore(world, fsr, 0.0);
        assert!(!world.resource_mut::<GraphicsResume>().request(0.0), "Defaults also restores named choices");
    }

    /// The Ambient occlusion row is a list of Off, Performance and Quality like Sun shadows:
    /// the choice sets the switch and the level, Off keeps the level for the next On, and the
    /// INI's older on/off (and a slider's 0 or 1) still loads as Quality or Off.
    #[test]
    fn ambient_occlusion_is_a_list_of_off_performance_and_quality() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        let mut app = App::new();
        app.insert_resource(Graphics::preset(graphics::Preset::Vanilla)).insert_resource(graphics::MsaaSetting(Msaa::Off));
        register_graphics(&mut app);
        let row = app.world().resource::<nvf_play::settings::Settings>().index(AO_KEY).unwrap();
        let chosen = |app: &App| (app.world().resource::<nvf_play::settings::Settings>().choices(row).unwrap().chosen)(app.world());
        let state = |app: &App| {
            let g = app.world().resource::<Graphics>();
            (g.ambient_occlusion, g.ambient_occlusion_level)
        };
        let names = (app.world().resource::<nvf_play::settings::Settings>().choices(row).unwrap().names)(app.world());
        assert_eq!(names, ["Off", "Performance", "Quality"]);
        assert_eq!(chosen(&app), "Off", "vanilla starts without it");
        assert!(nvf_play::settings::pick(app.world_mut(), row, "Performance"));
        assert_eq!(state(&app), (true, AoLevel::Performance));
        assert_eq!(chosen(&app), "Performance");
        assert!(nvf_play::settings::pick(app.world_mut(), row, "Off"));
        assert_eq!(state(&app), (false, AoLevel::Performance), "Off keeps the level");
        assert!(nvf_play::settings::pick(app.world_mut(), row, "Quality"));
        assert_eq!(state(&app), (true, AoLevel::Quality));
        // The INI: the names in any case, the switch's older values, a slider's numbers.
        for (text, want) in [
            ("performance", (true, AoLevel::Performance)),
            ("0", (false, AoLevel::Performance)),
            ("1", (true, AoLevel::Quality)),
            ("off", (false, AoLevel::Quality)),
            ("true", (true, AoLevel::Quality)),
            ("false", (false, AoLevel::Quality)),
            ("on", (true, AoLevel::Quality)),
            ("no", (false, AoLevel::Quality)),
            ("yes", (true, AoLevel::Quality)),
            ("0.5", (true, AoLevel::Performance)),
        ] {
            assert!(take_ao(app.world_mut(), text), "{text}");
            assert_eq!(state(&app), want, "{text}");
        }
        assert!(!take_ao(app.world_mut(), "ultra"));
        assert_eq!(state(&app), (true, AoLevel::Performance), "a value not taken changes nothing");
        assert!(AO_NOTE.contains("Off, Performance") && AO_NOTE.ends_with("Not in the original."));
    }

    #[test]
    fn native_aa_row_is_off_and_locked_for_every_supported_fsr_mode_then_restores_the_saved_choice() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        let mut app = App::new();
        app.insert_resource(Graphics::preset(graphics::Preset::Vanilla))
            .insert_resource(graphics::MsaaSetting(Msaa::Sample8))
            .insert_resource(crate::fsr::FsrSupport(true));
        register_graphics(&mut app);
        let row = app.world().resource::<nvf_play::settings::Settings>().index("anti-aliasing").unwrap();
        for (_, mode) in graphics::FSR_CHOICES.into_iter().filter(|(_, m)| m.quality().is_some()) {
            app.world_mut().resource_mut::<Graphics>().set_fsr(mode.quality());
            let settings = app.world().resource::<nvf_play::settings::Settings>();
            assert_eq!(settings.disabled_status(app.world(), row), Some("Off (upscaling active)"));
            // A list since the menu overhaul: its chosen index.
            assert_eq!((settings.choices(row).unwrap().chosen)(app.world()), "8x");
            assert!(!nvf_play::settings::pick(app.world_mut(), row, "Off"));
            assert_eq!(graphics::aa_choice(app.world()), 3, "disabled UI must preserve saved 8x");
            app.world_mut().resource_mut::<Graphics>().set_fsr(None);
            let settings = app.world().resource::<nvf_play::settings::Settings>();
            assert_eq!(settings.disabled_status(app.world(), row), None);
            assert_eq!((settings.choices(row).unwrap().chosen)(app.world()), "8x");
        }
        app.world_mut().resource_mut::<crate::fsr::FsrSupport>().0 = false;
        app.world_mut().resource_mut::<Graphics>().set_fsr(Some(crate::fsr::FsrQuality::NativeAa));
        assert_eq!(app.world().resource::<nvf_play::settings::Settings>().disabled_status(app.world(), row), None, "unsupported FSR leaves native AA editable");
        assert!(nvf_play::settings::pick(app.world_mut(), row, "Off"));
        assert_eq!(graphics::aa_choice(app.world()), 0);
    }

    #[test]
    fn the_fsr_library_row_switches_between_the_built_in_upscaler_and_the_folders() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        let mut app = App::new();
        app.insert_resource(Graphics::preset(graphics::Preset::Vanilla));
        register_graphics(&mut app);
        let list = {
            let settings = app.world().resource::<nvf_play::settings::Settings>();
            settings.choices(settings.index("fsr-library").unwrap()).expect("a dropdown")
        };
        // No library is beside the test's executable, and none was loaded.
        assert_eq!((list.names)(app.world()), ["Built-in", "Game folder (none found)"]);
        assert_eq!((list.chosen)(app.world()), "Game folder (none found)", "the folder's library is used by default");
        assert!((list.pick)(app.world_mut(), "Built-in"));
        assert!(!crate::ffx_api::enabled());
        assert_eq!(((list.chosen)(app.world()), (list.save)(app.world())), ("Built-in".to_string(), "Built-in".to_string()));
        // As the page gives it (with the library named) and as the INI keeps it.
        assert!((list.pick)(app.world_mut(), "Game folder (amd_fidelityfx_vk.dll)"));
        assert!(crate::ffx_api::enabled());
        assert_eq!((list.save)(app.world()), "Game folder");
        assert!((list.load)(app.world_mut(), "built-in") && !crate::ffx_api::enabled());
        assert!((list.load)(app.world_mut(), "Game folder") && crate::ffx_api::enabled());
        assert!(!(list.pick)(app.world_mut(), "Somewhere else"));
        assert!(crate::ffx_api::enabled(), "an unknown name changes nothing");
    }

    #[test]
    fn native_aa_row_is_locked_while_dlss_draws_and_follows_the_effective_upscaler() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        use crate::upscaler::{DlssState, Session, UpscaleQuality, UpscalerStatus};
        let mut app = App::new();
        app.insert_resource(Graphics::preset(graphics::Preset::Vanilla))
            .insert_resource(graphics::MsaaSetting(Msaa::Sample4))
            .insert_resource(crate::fsr::FsrSupport(true));
        register_graphics(&mut app);
        let row = app.world().resource::<nvf_play::settings::Settings>().index("anti-aliasing").unwrap();
        let ready = Session { streamline_loaded: true, dlss: DlssState::Ready, fsr: None };
        let mut status = UpscalerStatus::new(ready);
        status.update(UpscalerChoice::Dlss(UpscaleQuality::Quality));
        app.insert_resource(status);
        app.world_mut().resource_mut::<Graphics>().upscaler = UpscalerChoice::Dlss(UpscaleQuality::Quality);
        let locked = |app: &App| app.world().resource::<nvf_play::settings::Settings>().disabled_status(app.world(), row);
        assert_eq!(locked(&app), Some("Off (upscaling active)"), "DLSS draws");
        assert!(nvf_play::settings::set_slider(app.world_mut(), row, 0.0).is_none());
        // Off asked for: native at once (Streamline loaded), the row editable again.
        app.world_mut().resource_mut::<Graphics>().upscaler = UpscalerChoice::Off;
        app.world_mut().resource_mut::<UpscalerStatus>().update(UpscalerChoice::Off);
        assert_eq!(locked(&app), None);
        assert_eq!(graphics::aa_choice(app.world()), 2, "the saved 4x is back");
        // DLSS asked for in a session that started without Streamline (the files there now):
        // what drew, native, stays until the restart, so the row stays editable.
        let stock = Session { streamline_loaded: false, dlss: DlssState::NotLoaded, fsr: None };
        let mut status = UpscalerStatus::new(stock);
        status.update(UpscalerChoice::Off);
        status.update(UpscalerChoice::Dlss(UpscaleQuality::Quality));
        assert!(status.restart_pending());
        app.insert_resource(status);
        app.world_mut().resource_mut::<Graphics>().upscaler = UpscalerChoice::Dlss(UpscaleQuality::Quality);
        assert_eq!(locked(&app), None);
    }

    use crate::upscaler::{DlssState, Session, Unavailable, UpscalerStatus};
    use nvf_play::settings::{apply_stored, GivenSwitches, Settings, SettingsFile};
    use UpscaleQuality::*;

    /// The graphics rows registered, without an INI.
    fn rows_app() -> App {
        let mut app = App::new();
        app.insert_resource(Graphics::preset(graphics::Preset::Vanilla));
        register_graphics(&mut app);
        app
    }

    #[test]
    fn android_player_defaults_allow_the_settings_export_but_explicit_overrides_do_not() {
        use clap::Parser;
        use nvf_play::settings_menu::write_menu;
        use window_mode::WindowChoice::{Borderless, Windowed};

        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        let dir = std::env::temp_dir().join(format!("nvf-android-start-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = rows_app();
        app.insert_resource(SettingsFile(Some(dir.join("NVForever.ini"))));
        let export = dir.join("settings-menu.json");

        let line = crate::android_startup_line(Vec::new());
        let mut restart = crate::restart_args(&line[1..]);
        restart.extend(["--load", "quicksave"].map(String::from));
        for line in [line, crate::android_startup_line(restart)] {
            let args = Args::try_parse_from(&line).unwrap();
            assert!(args.play);
            assert_eq!(args.msaa, 1);
            assert!(args.window == Borderless);
            app.insert_resource(given_switches(&args, &line));
            write_menu(app.world());
            assert!(export.is_file(), "a normal Android start publishes the settings catalogue");
            std::fs::remove_file(&export).unwrap();
        }

        let explicit = crate::android_startup_line(["--play", "--msaa=1", "--window=borderless"].map(String::from).to_vec());
        let args = Args::try_parse_from(&explicit).unwrap();
        assert_eq!(args.msaa, 1);
        assert!(args.window == Borderless);
        let given = given_switches(&args, &explicit);
        assert!(given.given.iter().any(|flag| flag == "--window"));
        app.insert_resource(given);
        write_menu(app.world());
        assert!(!export.exists(), "an explicit switch setting the same value still blocks export");

        let custom = crate::android_startup_line(vec!["--play".into()]);
        let args = Args::try_parse_from(&custom).unwrap();
        assert_eq!(args.msaa, 4);
        assert!(args.window == Windowed);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn row(app: &App, key: &str) -> usize {
        app.world().resource::<Settings>().index(key).unwrap()
    }

    fn requested(app: &App) -> UpscalerChoice {
        app.world().resource::<Graphics>().upscaler
    }

    /// The two rows' chosen names, as the page and the INI have them.
    fn chosen(app: &App) -> (String, String) {
        (upscaler_chosen(app.world()), quality_chosen(app.world()))
    }

    const READY: Session = Session { streamline_loaded: true, dlss: DlssState::Ready, fsr: None };
    const STOCK: Session = Session { streamline_loaded: false, dlss: DlssState::NotLoaded, fsr: None };

    #[test]
    fn the_upscaler_and_its_quality_are_two_named_dropdowns_that_round_trip_every_choice() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        let mut app = rows_app();
        app.insert_resource(UpscalerStatus::new(READY));
        let (u, q) = (row(&app, UPSCALER_KEY), row(&app, QUALITY_KEY));
        let settings = app.world().resource::<Settings>();
        assert_eq!(q, u + 1, "the quality row right after the Upscaler row");
        assert_eq!((settings.list()[u].label, settings.list()[q].label), ("Upscaler", "Upscaling mode"));
        assert!(settings.index(LEGACY_KEY).is_none(), "the older row is no row of its own");
        let upscalers = settings.choices(u).expect("a native dropdown");
        let qualities = settings.choices(q).expect("a native dropdown");
        assert_eq!((upscalers.names)(app.world()), ["Off", "FSR 3.1", "DLSS"]);
        assert_eq!((qualities.names)(app.world()), ["Native AA", "Quality", "Balanced", "Performance", "Ultra Performance"]);
        assert_eq!(settings.older_key(u), Some(LEGACY_KEY));
        assert_eq!(chosen(&app), ("Off".to_string(), "Quality".to_string()), "off, the quality kept at Quality");
        for upscaler in [Upscaler::Fsr, Upscaler::Dlss] {
            let name = if upscaler == Upscaler::Fsr { "FSR 3.1" } else { "DLSS" };
            assert!(nvf_play::settings::pick(app.world_mut(), u, name), "{name}");
            for quality in UpscaleQuality::ALL {
                nvf_play::settings::pick(app.world_mut(), q, quality.name());
                let want = UpscalerChoice::new(upscaler, quality);
                assert_eq!(requested(&app), want);
                // What the INI keeps loads back into the same request, from any other.
                let saved = ((upscalers.save)(app.world()), (qualities.save)(app.world()));
                app.world_mut().resource_mut::<Graphics>().upscaler = UpscalerChoice::Off;
                assert!((upscalers.load)(app.world_mut(), &saved.0) && (qualities.load)(app.world_mut(), &saved.1));
                assert_eq!(requested(&app), want, "{saved:?}");
            }
        }
        assert!((upscalers.pick)(app.world_mut(), "fsr 3.1"), "any case");
        assert!((qualities.pick)(app.world_mut(), "ultra performance"));
        assert_eq!(requested(&app), UpscalerChoice::Fsr(UltraPerformance));
        assert!(!pick_upscaler(app.world_mut(), "XeSS") && !load_quality(app.world_mut(), "Extreme") && !load_upscaler(app.world_mut(), "NaN"));
        assert_eq!(requested(&app), UpscalerChoice::Fsr(UltraPerformance), "nothing taken");
    }

    /// The Upscaling mode row's words follow the upscaler asked for: FSR's shares from its
    /// ratios, DLSS's from the sizes it chose, none while Off; the names the INI and the test
    /// aids use stay the plain ones.
    #[test]
    fn the_mode_row_shows_the_share_each_upscaler_draws_at() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        let mut app = rows_app();
        app.insert_resource(UpscalerStatus::new(READY));
        let q = row(&app, QUALITY_KEY);
        let labels = |app: &App| -> Vec<String> {
            let settings = app.world().resource::<Settings>();
            UpscaleQuality::ALL.iter().map(|m| settings.choice_label(app.world(), q, m.name())).collect()
        };
        assert_eq!(labels(&app), ["Native AA", "Quality", "Balanced", "Performance", "Ultra Performance"], "Off: the plain names");
        app.world_mut().resource_mut::<Graphics>().upscaler = UpscalerChoice::Dlss(Quality);
        assert_eq!(labels(&app), ["Native AA (100%)", "Quality (67%)", "Balanced (58%)", "Performance (50%)", "Ultra Performance (33%)"]);
        app.world_mut().resource_mut::<Graphics>().upscaler = UpscalerChoice::Fsr(Balanced);
        assert_eq!(labels(&app), ["Native AA (100%)", "Quality (67%)", "Balanced (59%)", "Performance (50%)", "Ultra Performance (33%)"]);
        for mode in UpscaleQuality::ALL {
            assert_eq!(render_share(Upscaler::Fsr, mode), Some((100.0 / mode.ratio()).round() as u32), "{mode:?}: FSR's own ratio");
            // FSR's share is what render_size draws at, to the percent.
            let drawn = mode.render_size(UVec2::new(2000, 1000));
            assert_eq!(render_share(Upscaler::Fsr, mode), Some((drawn.x as f32 / 20.0).round() as u32), "{mode:?}");
            assert_eq!(render_share(Upscaler::Off, mode), None);
        }
        // DLSS's from the sizes it chose at 1280x720 (853x480 for Quality, 742x418 for Balanced …).
        for (mode, width) in [(NativeAa, 1280), (Quality, 853), (Balanced, 742), (Performance, 640), (UltraPerformance, 427)] {
            assert_eq!(render_share(Upscaler::Dlss, mode), Some((width as f32 * 100.0 / 1280.0).round() as u32), "{mode:?}");
        }
        // The names stay what the INI and the test aids use.
        assert!(nvf_play::settings::pick(app.world_mut(), q, "Performance"));
        assert_eq!(quality_chosen(app.world()), "Performance");
        assert!(!nvf_play::settings::pick(app.world_mut(), q, "Quality (67%)"), "the words are not a name");
    }

    /// The quality row is hidden while the upscaler is Off (the menu overhaul, the owner:
    /// the upscaler's own rows show only for it); the quality is kept meanwhile, so the next
    /// upscaler takes it up. The Upscaler row stands for it on the search.
    #[test]
    fn the_quality_row_is_hidden_while_the_upscaler_is_off_and_keeps_its_quality() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        let mut app = rows_app();
        app.insert_resource(UpscalerStatus::new(READY));
        let (u, q) = (row(&app, UPSCALER_KEY), row(&app, QUALITY_KEY));
        let hidden = |app: &App| app.world().resource::<Settings>().hidden(app.world()).contains(&(q, u));
        assert!(hidden(&app));
        assert!(nvf_play::settings::pick(app.world_mut(), u, "FSR 3.1"));
        assert!(!hidden(&app));
        assert!(nvf_play::settings::pick(app.world_mut(), q, "Balanced"));
        assert!(nvf_play::settings::pick(app.world_mut(), u, "Off"));
        assert_eq!((requested(&app), hidden(&app)), (UpscalerChoice::Off, true));
        assert_eq!(quality_chosen(app.world()), "Balanced", "kept while Off");
        assert!(nvf_play::settings::pick(app.world_mut(), u, "FSR 3.1"));
        assert_eq!(requested(&app), UpscalerChoice::Fsr(Balanced), "taken up again");
        // The INI's text loads into the kept quality while Off (loading is not the page).
        assert!(nvf_play::settings::pick(app.world_mut(), u, "Off"));
        assert!(load_quality(app.world_mut(), "Performance"));
        assert_eq!(requested(&app), UpscalerChoice::Off);
        assert!(nvf_play::settings::pick(app.world_mut(), u, "DLSS"));
        assert_eq!(requested(&app), UpscalerChoice::Dlss(Performance));
    }

    #[test]
    fn choices_are_shown_in_short_words_and_dlss_says_restart_only_while_it_waits() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        let mut app = rows_app();
        let (u, q) = (row(&app, UPSCALER_KEY), row(&app, QUALITY_KEY));
        let label = |app: &App, i: usize, name: &str| app.world().resource::<Settings>().choice_label(app.world(), i, name);
        assert_eq!([label(&app, u, "Off"), label(&app, u, "FSR 3.1"), label(&app, u, "DLSS")], ["Off", "FSR 3.1", "DLSS"]);
        for quality in UpscaleQuality::ALL {
            assert_eq!(label(&app, q, quality.name()), quality.name(), "Off: the plain names");
        }
        // DLSS asked for in a session that started without Streamline, the files there now.
        app.insert_resource(UpscalerStatus::new(STOCK));
        assert!(nvf_play::settings::pick(app.world_mut(), u, "DLSS"));
        let requested = requested(&app);
        app.world_mut().resource_mut::<UpscalerStatus>().update(requested);
        assert!(app.world().resource::<UpscalerStatus>().restart_pending());
        assert_eq!(label(&app, u, "DLSS"), "DLSS (restart)");
        assert_eq!(label(&app, u, "FSR 3.1"), "FSR 3.1", "only DLSS says so");
        assert!("DLSS (restart)".len() <= "Ultra Performance".len(), "short enough for the box");
        // With Streamline loaded nothing waits.
        app.insert_resource(UpscalerStatus::new(READY));
        app.world_mut().resource_mut::<UpscalerStatus>().update(requested);
        assert_eq!(label(&app, u, "DLSS"), "DLSS");
    }

    /// Download DLSS… (#2250) is a choice of the Upscaler row only while DLSS's files are what is
    /// missing (on Windows, where that reason arises): picking it asks for its dialog and leaves
    /// the request as it is.
    #[test]
    fn download_dlss_is_a_choice_only_while_the_files_are_missing() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(true));
        use crate::dlss_download::{Asked, CHOICE};
        let missing = Session { streamline_loaded: false, dlss: DlssState::Unavailable(Unavailable::FilesMissing), fsr: None };
        let mut app = rows_app();
        app.insert_resource(UpscalerStatus::new(missing));
        let u = row(&app, UPSCALER_KEY);
        let names = |app: &App| (app.world().resource::<Settings>().choices(u).unwrap().names)(app.world());
        if !cfg!(windows) {
            assert_eq!(names(&app), ["Off", "FSR 3.1", "DLSS"], "DLSS, and its download, are Windows only");
            assert!(!pick_upscaler(app.world_mut(), CHOICE));
            return;
        }
        assert_eq!(names(&app), ["Off", "FSR 3.1", "DLSS", CHOICE]);
        let settings = app.world().resource::<Settings>();
        assert_eq!(settings.choice_unavailable(app.world(), u, CHOICE), None, "in reach while DLSS is not");
        assert_eq!(settings.choice_unavailable(app.world(), u, "DLSS").as_deref(), Some(Unavailable::FilesMissing.words()));
        assert_eq!(settings.choice_label(app.world(), u, CHOICE), "Download DLSS…");
        assert!(nvf_play::settings::pick(app.world_mut(), u, CHOICE));
        assert!(app.world().contains_resource::<Asked>(), "the dialog asked for");
        assert_eq!(requested(&app), UpscalerChoice::Off, "the request unchanged");
        assert_eq!(upscaler_chosen(app.world()), "Off");
        // Other refusals: no download offered. Ready/restart can still lack frame generation.
        for session in [
            Session { streamline_loaded: false, dlss: DlssState::Unavailable(Unavailable::NotVulkan), fsr: None },
            Session { streamline_loaded: false, dlss: DlssState::Unavailable(Unavailable::UpdaterInFolder), fsr: None },
            Session { streamline_loaded: false, dlss: DlssState::Unavailable(Unavailable::GpuNotSupported), fsr: None },
        ] {
            let mut app = rows_app();
            app.insert_resource(UpscalerStatus::new(session));
            assert_eq!(names(&app), ["Off", "FSR 3.1", "DLSS"], "{session:?}");
            assert!(!pick_upscaler(app.world_mut(), CHOICE), "{session:?}");
            assert!(!app.world().contains_resource::<Asked>());
        }
    }

    #[test]
    fn missing_frame_generation_does_not_disable_upscaling() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(true));
        for session in [READY, STOCK] {
            let mut app = rows_app();
            app.insert_resource(UpscalerStatus::new(session));
            assert_eq!(dlss_unavailable(app.world()), None, "the four-file upscaler remains available");
            assert_eq!(dlss_download_offered(app.world()), cfg!(windows), "missing frame generation files can still be downloaded");
            let u = row(&app, UPSCALER_KEY);
            assert!(nvf_play::settings::pick(app.world_mut(), u, "DLSS"));
            let request = requested(&app);
            assert!(request.is_dlss());
            let mut status = app.world_mut().resource_mut::<UpscalerStatus>();
            status.update(request);
            if session.streamline_loaded {
                assert_eq!(status.effective(), request, "upscaling still draws with only its four files");
            } else {
                assert!(status.restart_pending());
            }
        }
    }

    #[test]
    fn download_choice_and_frame_generation_hint_follow_the_probe() {
        use crate::dlss_download::{Asked, CHOICE, TestOfferProbe};
        let missing = Session { streamline_loaded: false, dlss: DlssState::Unavailable(Unavailable::FilesMissing), fsr: None };
        for probe in [Some(true), Some(false), None] {
            let _offer_probe = TestOfferProbe::new(probe);
            for session in [READY, STOCK, missing, READY] {
                let mut app = rows_app();
                app.insert_resource(UpscalerStatus::new(session));
                let want = cfg!(windows) && probe.unwrap_or(session.dlss == missing.dlss);
                let u = row(&app, UPSCALER_KEY);
                let names = (app.world().resource::<Settings>().choices(u).unwrap().names)(app.world());
                assert_eq!(names.iter().any(|name| name == CHOICE), want, "{probe:?}, {session:?}");
                let hint = if !cfg!(windows) {
                    "Not on this GPU"
                } else if want {
                    "Files missing: Download DLSS… on the Upscaler row"
                } else {
                    "The DLSS Frame Generation files are missing"
                };
                assert_eq!(frame_generation_unavailable(app.world(), "DLSS").as_deref(), Some(hint), "{probe:?}, {session:?}");
                assert_eq!(nvf_play::settings::pick(app.world_mut(), u, CHOICE), want);
                assert_eq!(app.world().contains_resource::<Asked>(), want);
                assert_eq!(requested(&app), UpscalerChoice::Off);
            }
        }
    }

    #[test]
    fn dlss_is_out_of_reach_with_the_reason_but_a_stored_one_loads() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        for why in [Unavailable::FilesMissing, Unavailable::NotWindows, Unavailable::NotVulkan, Unavailable::GpuNotSupported, Unavailable::UpdaterInFolder, Unavailable::EvaluateFailed] {
            let mut app = rows_app();
            app.insert_resource(UpscalerStatus::new(Session { streamline_loaded: false, dlss: DlssState::Unavailable(why), fsr: None }));
            let (u, q) = (row(&app, UPSCALER_KEY), row(&app, QUALITY_KEY));
            let settings = app.world().resource::<Settings>();
            assert_eq!(settings.choice_unavailable(app.world(), u, "DLSS").as_deref(), Some(why.words()), "{why:?}");
            assert_eq!(settings.choice_unavailable(app.world(), u, "Off"), None);
            assert_eq!(settings.choice_unavailable(app.world(), u, "FSR 3.1"), None);
            for quality in UpscaleQuality::ALL {
                assert_eq!(settings.choice_unavailable(app.world(), q, quality.name()), None, "the qualities are FSR's too");
            }
            assert!(!nvf_play::settings::pick(app.world_mut(), u, "DLSS"), "{why:?}: not taken from the page or a test aid");
            assert!(!nvf_play::settings::pick_alias(app.world_mut(), LEGACY_KEY, "DLSS Quality"), "{why:?}: nor by the older key");
            assert_eq!(requested(&app), UpscalerChoice::Off);
            assert!(nvf_play::settings::pick(app.world_mut(), u, "FSR 3.1"));
            // The INI's text still loads: the request survives the run.
            assert!(load_upscaler(app.world_mut(), "DLSS"));
            assert_eq!(requested(&app), UpscalerChoice::Dlss(Quality));
            assert!(load_upscaler(app.world_mut(), "DLSS Performance"), "the older row's text too");
            assert_eq!(requested(&app), UpscalerChoice::Dlss(Performance));
        }
        // Ready, or not loaded with the files there (a restart brings it): in reach.
        for session in [READY, STOCK] {
            let mut app = rows_app();
            app.insert_resource(UpscalerStatus::new(session));
            let u = row(&app, UPSCALER_KEY);
            assert_eq!(app.world().resource::<Settings>().choice_unavailable(app.world(), u, "DLSS"), None, "{session:?}");
        }
    }

    /// DLSS given up on (`EvaluateFailed`): DLSS is out of reach with the reason, the stored
    /// DLSS request stays the request and the camera draws natively; FSR and off follow.
    #[test]
    fn dlss_that_stopped_working_says_why_and_fsr_still_follows() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        let mut app = rows_app();
        let u = row(&app, UPSCALER_KEY);
        let want = UpscalerChoice::Dlss(Quality);
        app.world_mut().resource_mut::<Graphics>().upscaler = want;
        let mut status = UpscalerStatus::new(READY);
        status.update(want);
        status.session.dlss = DlssState::Unavailable(Unavailable::EvaluateFailed);
        status.update(want);
        app.insert_resource(status);
        assert_eq!(app.world().resource::<Settings>().choice_unavailable(app.world(), u, "DLSS").as_deref(), Some(Unavailable::EvaluateFailed.words()));
        assert_eq!(chosen(&app), ("DLSS".to_string(), "Quality".to_string()), "the request is kept");
        assert_eq!(app.world().resource::<UpscalerStatus>().effective(), UpscalerChoice::Off, "native, not FSR");
        assert!(nvf_play::settings::pick(app.world_mut(), u, "FSR 3.1"));
        app.world_mut().resource_mut::<UpscalerStatus>().update(UpscalerChoice::Fsr(Quality));
        assert_eq!(app.world().resource::<UpscalerStatus>().effective(), UpscalerChoice::Fsr(Quality), "FSR at once");
    }

    #[test]
    fn with_streamline_every_switch_is_live_and_without_it_dlss_waits_for_a_restart() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        let mut app = rows_app();
        let (u, q) = (row(&app, UPSCALER_KEY), row(&app, QUALITY_KEY));
        // What follow_request does each frame, without the process-wide loader state.
        let pick = |app: &mut App, i: usize, name: &str| {
            assert!(nvf_play::settings::pick(app.world_mut(), i, name), "{name}");
            let requested = requested(app);
            app.world_mut().resource_mut::<UpscalerStatus>().update(requested);
            let status = *app.world().resource::<UpscalerStatus>();
            (status.effective(), status.restart_pending())
        };
        app.insert_resource(UpscalerStatus::new(READY));
        assert_eq!(pick(&mut app, u, "FSR 3.1"), (UpscalerChoice::Fsr(Quality), false));
        assert_eq!(pick(&mut app, u, "DLSS"), (UpscalerChoice::Dlss(Quality), false), "entering DLSS at once");
        assert_eq!(pick(&mut app, q, "Ultra Performance"), (UpscalerChoice::Dlss(UltraPerformance), false));
        assert_eq!(pick(&mut app, u, "FSR 3.1"), (UpscalerChoice::Fsr(UltraPerformance), false), "leaving DLSS at once");
        assert_eq!(pick(&mut app, u, "Off"), (UpscalerChoice::Off, false));
        assert_eq!(pick(&mut app, u, "DLSS"), (UpscalerChoice::Dlss(UltraPerformance), false));
        // A session that started without Streamline (the files put there since).
        app.insert_resource(UpscalerStatus::new(STOCK));
        assert_eq!(pick(&mut app, u, "FSR 3.1"), (UpscalerChoice::Fsr(UltraPerformance), false), "FSR at once");
        assert_eq!(pick(&mut app, u, "DLSS"), (UpscalerChoice::Fsr(UltraPerformance), true), "DLSS saved, FSR keeps drawing");
        assert_eq!(chosen(&app).0, "DLSS", "the request is what the INI keeps");
        assert_eq!(pick(&mut app, q, "Balanced"), (UpscalerChoice::Fsr(UltraPerformance), true), "DLSS's quality waits with it");
        assert_eq!(pick(&mut app, u, "Off"), (UpscalerChoice::Off, false), "off at once");
    }

    /// An INI written before the two rows has only `fsr-upscaling`: every one of its eleven
    /// names and the early slider build's numbers load into both rows. With the rows' own keys
    /// there, those win.
    #[test]
    fn an_older_ini_loads_into_both_rows() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        let dir = std::env::temp_dir().join(format!("nvf-upscaler-older-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("NVForever.ini");
        let mut cases: Vec<(String, UpscalerChoice)> = graphics::FSR_UPSCALING_CHOICES.iter().map(|(n, c)| (n.to_string(), *c)).collect();
        cases.extend([("0".to_string(), UpscalerChoice::Off), ("0.2".to_string(), UpscalerChoice::Fsr(NativeAa)), ("0.40".to_string(), UpscalerChoice::Fsr(Quality)), ("1".to_string(), UpscalerChoice::Fsr(UltraPerformance))]);
        cases.push(("dlss balanced".to_string(), UpscalerChoice::Dlss(Balanced)));
        for (text, want) in cases {
            let ini = format!("[Settings]\nfsr-upscaling={text}\n");
            let app = ini_app(&path, &ini, UpscalerChoice::Off, GivenSwitches::default());
            assert_eq!(requested(&app), want, "{text}");
            assert_eq!(upscaler_after_ini(UpscalerChoice::Off, Some(&UpscalerIni::read(&ini)), Some(&GivenSwitches::default())), want, "{text}: the start agrees");
            let (u, q) = chosen(&app);
            assert_eq!(u, UPSCALERS.iter().find(|(_, k)| *k == want.upscaler()).unwrap().0, "{text}");
            assert_eq!(q, want.quality().unwrap_or_default().name(), "{text}");
        }
        let both = "[Settings]\nfsr-upscaling=DLSS Balanced\nupscaler=FSR 3.1\nupscaler-quality=Performance\n";
        assert_eq!(requested(&ini_app(&path, both, UpscalerChoice::Off, GivenSwitches::default())), UpscalerChoice::Fsr(Performance), "the rows' own keys win");
        assert_eq!(UpscalerIni::read(both), UpscalerIni { upscaler: Some("FSR 3.1".into()), quality: Some("Performance".into()) });
        assert_eq!(UpscalerIni::read("[Settings]\nfsr-upscaling=Performance\n"), UpscalerIni { upscaler: Some("FSR 3.1".into()), quality: Some("Performance".into()) }, "split into the two rows' values");
        assert_eq!(UpscalerIni::read("[Settings]\nfsr-upscaling=Off\nupscaler-quality=Balanced\n"), UpscalerIni { upscaler: Some("Off".into()), quality: Some("Balanced".into()) }, "the quality row's own key kept");
        assert_eq!(UpscalerIni::read("[Settings]\nfsr-upscaling=nonsense\n"), UpscalerIni { upscaler: Some("nonsense".into()), quality: None }, "not one of its values: as it is");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The review's P2 (2026-11-12): an INI with only the older `fsr-upscaling=Performance`, a
    /// test run whose `--fsr quality` keeps both rows at the command line's FSR Quality, then a
    /// change of another row: the save writes the stored FSR Performance for both rows, not
    /// FSR with the command line's Quality, and the next start loads FSR Performance.
    #[test]
    fn a_test_run_keeps_the_older_inis_quality_through_a_save() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        let dir = std::env::temp_dir().join(format!("nvf-upscaler-kept-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("NVForever.ini");
        let test_run = GivenSwitches { given: vec!["--fsr".into()], win: true };
        let mut app = ini_app(&path, "[Settings]\nfsr-upscaling=Performance\n", UpscalerChoice::Fsr(Quality), test_run);
        assert_eq!(requested(&app), UpscalerChoice::Fsr(Quality), "the test run's command line");
        let bloom = row(&app, "bloom");
        nvf_play::settings::flip(app.world_mut(), bloom);
        let ini = std::fs::read_to_string(&path).unwrap();
        let get = |key: &str| fnv_install::ini_get(&ini, nvf_play::settings::SECTION, key);
        assert_eq!((get(UPSCALER_KEY), get(QUALITY_KEY)), (Some("FSR 3.1".to_string()), Some("Performance".to_string())), "both rows as the INI had them: {ini}");
        let reloaded = ini_app(&path, &ini, UpscalerChoice::Off, GivenSwitches::default());
        assert_eq!(requested(&reloaded), UpscalerChoice::Fsr(Performance), "the next normal start: FSR Performance");
        assert_eq!(upscaler_after_ini(UpscalerChoice::Off, Some(&UpscalerIni::read(&ini)), Some(&GivenSwitches::default())), UpscalerChoice::Fsr(Performance), "the start agrees");
        // DLSS through the older key alike.
        let test_run = GivenSwitches { given: vec!["--upscaler".into()], win: true };
        let mut app = ini_app(&path, "[Settings]\nfsr-upscaling=DLSS Ultra Performance\n", UpscalerChoice::Fsr(Quality), test_run);
        nvf_play::settings::flip(app.world_mut(), bloom);
        let ini = std::fs::read_to_string(&path).unwrap();
        assert_eq!(requested(&ini_app(&path, &ini, UpscalerChoice::Off, GivenSwitches::default())), UpscalerChoice::Dlss(UltraPerformance), "{ini}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The test aid's `fsr-upscaling=<name>` (the scenarios written for the one row) sets both
    /// rows, logged under the older key.
    #[test]
    fn the_older_key_is_still_taken_by_the_test_aid() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        let mut app = rows_app();
        app.insert_resource(UpscalerStatus::new(READY));
        let alias = app.world().resource::<Settings>().alias(LEGACY_KEY).map(|(_, a)| a.rows);
        assert_eq!(alias, Some(&[UPSCALER_KEY, QUALITY_KEY][..]), "shown on the Upscaler row");
        for (name, want) in graphics::FSR_UPSCALING_CHOICES {
            let taken = nvf_play::settings::pick_alias(app.world_mut(), LEGACY_KEY, name);
            assert_eq!(taken, requested(&app) == want && name != "Off", "{name}");
            assert_eq!(requested(&app), want, "{name}");
            assert_eq!(legacy_chosen(app.world()), name, "the log line's name");
        }
        assert!(nvf_play::settings::pick_alias(app.world_mut(), LEGACY_KEY, "quality"), "any case");
        assert_eq!(requested(&app), UpscalerChoice::Fsr(Quality));
        assert!(!nvf_play::settings::pick_alias(app.world_mut(), LEGACY_KEY, "0.40"), "names only");
    }

    /// The DLSS model row: right under the quality, NVIDIA's names, every choice round-trips the
    /// INI's text, an unknown stored value loads as Default, and it names its switch.
    #[test]
    fn the_dlss_model_row_round_trips_and_an_unknown_value_loads_as_default() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        let mut app = rows_app();
        app.insert_resource(UpscalerStatus::new(READY));
        app.world_mut().resource_mut::<Graphics>().upscaler = UpscalerChoice::Dlss(Quality);
        app.world_mut().resource_mut::<UpscalerStatus>().update(UpscalerChoice::Dlss(Quality));
        let (q, m) = (row(&app, QUALITY_KEY), row(&app, MODEL_KEY));
        let settings = app.world().resource::<Settings>();
        assert_eq!(m, q + 1, "right under Upscaling mode");
        assert_eq!(settings.list()[m].label, "DLSS model");
        let models = settings.choices(m).expect("a dropdown");
        assert_eq!((models.names)(app.world()), ["Default", "K", "L", "M", "J"]);
        assert_eq!(model_chosen(app.world()), "Default", "Default by default");
        for model in DlssModel::ALL {
            nvf_play::settings::pick(app.world_mut(), m, model.name());
            assert_eq!(app.world().resource::<Graphics>().dlss_model, model);
            let saved = (models.save)(app.world());
            assert_eq!(saved, model.name());
            app.world_mut().resource_mut::<Graphics>().dlss_model = DlssModel::Default;
            assert!((models.load)(app.world_mut(), &saved));
            assert_eq!(app.world().resource::<Graphics>().dlss_model, model, "{saved}");
        }
        assert!(pick_model(app.world_mut(), "k"), "any case");
        assert_eq!(app.world().resource::<Graphics>().dlss_model, DlssModel::K);
        assert!(!pick_model(app.world_mut(), "A"), "not one of the five: not taken");
        assert_eq!(app.world().resource::<Graphics>().dlss_model, DlssModel::K);
        // The INI's earlier spellings (`Preset K` …) load into the same models.
        for (older, model) in [("Preset J", DlssModel::J), ("preset k", DlssModel::K), ("Preset L", DlssModel::L), ("PRESET M", DlssModel::M)] {
            app.world_mut().resource_mut::<Graphics>().dlss_model = DlssModel::Default;
            assert!(load_model(app.world_mut(), older), "{older:?}");
            assert_eq!(app.world().resource::<Graphics>().dlss_model, model, "{older:?}");
            assert_eq!(model_chosen(app.world()), model.name(), "saved by its letter from then on");
        }
        for unknown in ["A", "Preset A", "N", "Preset N", "Preset Default", "", "13", "transformer"] {
            app.world_mut().resource_mut::<Graphics>().dlss_model = DlssModel::L;
            assert!(load_model(app.world_mut(), unknown), "{unknown:?}");
            assert_eq!(app.world().resource::<Graphics>().dlss_model, DlssModel::Default, "{unknown:?} loads as Default");
        }
        assert_eq!(nvf_play::settings::note_switches(app.world().resource::<Settings>().list()[m].switches), ["--dlss-model"], "a test run's --dlss-model keeps the row at the command line's model");
        let label = |name: &str| app.world().resource::<Settings>().choice_label(app.world(), m, name);
        assert_eq!(label("L"), "L", "NVIDIA's letters");
        assert_eq!(label("Default"), "Default", "in English here (the page translates it)");
    }

    /// The DLSS model row stands dimmed with a reason while DLSS is not the effective upscaler
    /// (FSR, off, DLSS waiting for a restart or given up on); the model is kept meanwhile and
    /// the INI still loads into it.
    #[test]
    fn the_dlss_model_row_is_dimmed_while_dlss_does_not_draw() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        let mut app = rows_app();
        let m = row(&app, MODEL_KEY);
        let dimmed = |app: &App| app.world().resource::<Settings>().disabled_status(app.world(), m);
        let ask = |app: &mut App, choice: UpscalerChoice| {
            app.world_mut().resource_mut::<Graphics>().upscaler = choice;
            app.world_mut().resource_mut::<UpscalerStatus>().update(choice);
        };
        app.insert_resource(UpscalerStatus::new(READY));
        ask(&mut app, UpscalerChoice::Off);
        assert_eq!(dimmed(&app), Some("DLSS only"));
        assert!(!nvf_play::settings::pick(app.world_mut(), m, "L"), "dimmed: not taken from the page or a test aid");
        ask(&mut app, UpscalerChoice::Fsr(Quality));
        assert_eq!(dimmed(&app), Some("DLSS only"), "FSR draws");
        ask(&mut app, UpscalerChoice::Dlss(Balanced));
        assert_eq!(dimmed(&app), None, "DLSS draws");
        assert!(nvf_play::settings::pick(app.world_mut(), m, "L"));
        ask(&mut app, UpscalerChoice::Fsr(Balanced));
        assert_eq!(dimmed(&app), Some("DLSS only"));
        assert_eq!(model_chosen(app.world()), "L", "kept while FSR draws");
        assert!(load_model(app.world_mut(), "M"), "the INI loads into it all the same");
        assert_eq!(app.world().resource::<Graphics>().dlss_model, DlssModel::M);
        // DLSS asked for but waiting for a restart (FSR drawing until then), or given up on.
        let mut waiting = UpscalerStatus::new(STOCK);
        waiting.update(UpscalerChoice::Fsr(Quality));
        app.insert_resource(waiting);
        ask(&mut app, UpscalerChoice::Dlss(Quality));
        assert!(app.world().resource::<UpscalerStatus>().restart_pending());
        assert_eq!(dimmed(&app), Some("DLSS only"), "DLSS asked for, not drawing yet");
        app.insert_resource(UpscalerStatus::new(Session { streamline_loaded: true, dlss: DlssState::Unavailable(Unavailable::EvaluateFailed), fsr: None }));
        ask(&mut app, UpscalerChoice::Dlss(Quality));
        assert_eq!(dimmed(&app), Some("DLSS only"), "DLSS given up on: native draws");
    }

    /// The INI's `dlss-model` line by the settings' precedence: applied in normal play; in a test
    /// run a given `--dlss-model` keeps the command line's model.
    #[test]
    fn the_dlss_model_follows_the_settings_precedence() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        let dir = std::env::temp_dir().join(format!("nvf-dlss-model-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("NVForever.ini");
        let model = |app: &App| app.world().resource::<Graphics>().dlss_model;
        let given = |switches: &[&str], win: bool| GivenSwitches { given: switches.iter().map(|s| s.to_string()).collect(), win };
        let app = ini_app(&path, "[Settings]
dlss-model=J
", UpscalerChoice::Off, GivenSwitches::default());
        assert_eq!(model(&app), DlssModel::J, "the INI's model");
        let app = ini_app(&path, "[Settings]
dlss-model=J
", UpscalerChoice::Off, given(&["--dlss-model"], false));
        assert_eq!(model(&app), DlssModel::J, "normal play: the INI wins over --dlss-model");
        let app = ini_app(&path, "[Settings]
dlss-model=J
", UpscalerChoice::Off, given(&["--dlss-model"], true));
        assert_eq!(model(&app), DlssModel::Default, "a test run: --dlss-model (the command line's Default here) is kept");
        let app = ini_app(&path, "[Settings]
dlss-model=J
", UpscalerChoice::Off, given(&["--upscaler"], true));
        assert_eq!(model(&app), DlssModel::J, "another switch given keeps nothing of this row");
        let app = ini_app(&path, "[Settings]
dlss-model=Preset Z
", UpscalerChoice::Off, GivenSwitches::default());
        assert_eq!(model(&app), DlssModel::Default, "an unknown stored value loads as Default");
        let app = ini_app(&path, "[Settings]
dlss-model=Preset K
", UpscalerChoice::Off, GivenSwitches::default());
        assert_eq!(model(&app), DlssModel::K, "the earlier spelling loads");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn both_upscaler_rows_name_the_same_switches() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        let app = rows_app();
        let settings = app.world().resource::<Settings>();
        let switches = |key| {
            let mut s = nvf_play::settings::note_switches(settings.list()[row(&app, key)].switches);
            s.sort();
            s
        };
        assert_eq!(switches(UPSCALER_KEY), ["--fsr", "--upscaler", "--upscaler-quality"]);
        assert_eq!(switches(QUALITY_KEY), switches(UPSCALER_KEY), "a switch given keeps both rows or neither");
    }

    /// #2731: the Generated frames row's note says when the frame rate limit divided by the
    /// frames shown per real frame leaves fewer than 60 real frames a second, within the budget.
    #[test]
    fn the_generated_frames_note_warns_under_sixty_real_frames() {
        let at = |frames, cap| frames_note_for(frames, cap);
        // A limit of 144: 2x leaves 72, fine; 3x 48 and 4x 36 are said.
        assert_eq!(at(1, 144), None);
        assert_eq!(
            at(2, 144).as_deref(),
            Some("3x at a limit of 144: 48 real frames a second, which can feel choppy. Fewer generated frames, or a limit up to the display's refresh rate, give more.")
        );
        assert_eq!(
            at(3, 144).as_deref(),
            Some("4x at a limit of 144: 36 real frames a second, which can feel choppy. Fewer generated frames, or a limit up to the display's refresh rate, give more.")
        );
        // A limit of 240 (variable refresh's on a fast screen): 120, 80 and 60, none under 60.
        assert_eq!((at(1, 240), at(2, 240), at(3, 240)), (None, None, None));
        // Lower limits; no limit says nothing.
        assert!(at(1, 90).is_some_and(|n| n.starts_with("2x at a limit of 90: 45 real frames")));
        assert!(at(3, 120).is_some_and(|n| n.contains(": 30 real frames")));
        assert_eq!((at(1, 0), at(3, 0)), (None, None));
        for frames in 1..=3 {
            for cap in frame_pace::CAPS.into_iter().chain([240, 225, 1000]) {
                if let Some(note) = at(frames, cap) {
                    assert_eq!(nvf_play::settings::note_over_budget(&note), None, "{note}");
                }
            }
        }
    }

    /// #2377: the renderer's rows that register without the game's files or the command line
    /// (the Graphics tab's and the frame rate counter's) keep their notes within the budget.
    /// (`register` as a whole reads the quality presets from the game folder and the command
    /// line, so it is left to the start's own check, `nvf_play::settings::check_notes`.)
    #[test]
    fn the_renderer_notes_are_within_the_budget() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        let mut app = rows_app();
        register_counter_rows(&mut app);
        let settings = app.world().resource::<Settings>();
        assert!(settings.list().len() > 25, "{}", settings.list().len());
        for s in settings.list() {
            assert_eq!(nvf_play::settings::note_over_budget(s.note), None, "{}: {}", s.key, s.note);
            // The menu overhaul: 7 or fewer values are a list, never a slider.
            assert!(!nvf_play::settings::few_step_slider(s), "{}: a slider of 7 or fewer values; make it a list", s.key);
        }
        for note in [FPS_COUNTER_NOTE, FPS_UPSCALING_NOTE, FPS_SPLIT_NOTE] {
            assert_eq!(nvf_play::settings::note_over_budget(note), None, "{note}");
        }
    }

    /// #2701: the Full-detail scenery row is off by default (the vanilla look) and takes each
    /// size by its name, in any case; another name is refused.
    #[test]
    fn the_full_detail_row_is_off_by_default_and_takes_each_size() {
        use crate::full_detail::FullDetail;
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        let mut app = rows_app();
        let _ = row(&app, FULL_DETAIL_KEY);
        assert_eq!(full_detail_chosen(app.world()), "Off");
        for (name, f) in FullDetail::CHOICES {
            assert!(take_full_detail(app.world_mut(), name), "{name}");
            assert_eq!(app.world().resource::<Graphics>().full_detail, f);
            assert_eq!(full_detail_chosen(app.world()), name);
        }
        assert!(take_full_detail(app.world_mut(), "7 X 7"));
        assert_eq!(app.world().resource::<Graphics>().full_detail, FullDetail::Seven);
        assert!(!take_full_detail(app.world_mut(), "11 x 11"));
        assert_eq!(app.world().resource::<Graphics>().full_detail, FullDetail::Seven, "unchanged");
    }

    /// #3014: the Distant objects row is off by default (both presets) and takes each reach
    /// by its name, in any case; another name is refused.
    #[test]
    fn the_distant_objects_row_is_off_by_default_and_takes_each_reach() {
        use crate::distant_objects::DistantObjects;
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        let mut app = rows_app();
        let _ = row(&app, DISTANT_OBJECTS_KEY);
        assert_eq!(distant_objects_chosen(app.world()), "Off");
        for (name, d) in DistantObjects::CHOICES {
            assert!(take_distant_objects(app.world_mut(), name), "{name}");
            assert_eq!(app.world().resource::<Graphics>().distant_objects, d);
            assert_eq!(distant_objects_chosen(app.world()), name);
        }
        assert!(take_distant_objects(app.world_mut(), "8 CELLS"));
        assert_eq!(app.world().resource::<Graphics>().distant_objects, DistantObjects::Eight);
        assert!(!take_distant_objects(app.world_mut(), "16 cells"));
        assert_eq!(app.world().resource::<Graphics>().distant_objects, DistantObjects::Eight, "unchanged");
    }

    /// The Volumetric clouds row is a list of Off and its levels: the choice sets the switch and
    /// the level, Off keeps the level, and the INI's older on/off loads as Normal and Off.
    #[test]
    fn volumetric_clouds_are_a_list_of_off_and_the_levels() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        let mut app = rows_app();
        let i = row(&app, CLOUDS_KEY);
        let s = app.world().resource::<Settings>().list()[i];
        assert_eq!((s.group, s.label, s.note, s.switches), (GROUP_GRAPHICS, "Volumetric clouds (experimental)", CLOUDS_NOTE, "--graphics-only"));
        assert!(CLOUDS_NOTE.chars().count() <= 200 && CLOUDS_NOTE.ends_with("Not in the original."));
        let list = app.world().resource::<Settings>().choices(i).expect("a list");
        assert_eq!((list.names)(app.world()), ["Off", "Performance", "Normal", "High"]);
        assert_eq!((list.chosen)(app.world()), "Off", "off at the start");
        let state = |app: &App| {
            let g = app.world().resource::<Graphics>();
            (g.volumetric_clouds, g.volumetric_clouds_level)
        };
        for (name, level) in [("Performance", CloudLevel::Performance), ("High", CloudLevel::High), ("Normal", CloudLevel::Normal)] {
            assert!(nvf_play::settings::pick(app.world_mut(), i, name));
            assert_eq!((state(&app), (list.save)(app.world()).as_str()), ((true, level), name));
        }
        // The INI: the names in any case, the switch's older values.
        for (text, want) in [
            ("high", (true, CloudLevel::High)),
            ("0", (false, CloudLevel::High)),
            ("1", (true, CloudLevel::Normal)),
            (" PERFORMANCE ", (true, CloudLevel::Performance)),
            ("off", (false, CloudLevel::Performance)),
            ("true", (true, CloudLevel::Normal)),
            ("false", (false, CloudLevel::Normal)),
            ("on", (true, CloudLevel::Normal)),
            ("no", (false, CloudLevel::Normal)),
            ("yes", (true, CloudLevel::Normal)),
        ] {
            assert!(take_clouds(app.world_mut(), text), "{text}");
            assert_eq!(state(&app), want, "{text}");
        }
        for text in ["ultra", "0.5", "2", ""] {
            assert!(!take_clouds(app.world_mut(), text), "{text}");
        }
        assert_eq!(state(&app), (true, CloudLevel::Normal), "a value not taken changes nothing");
    }

    /// #2377: the command-line switches moved out of the notes into `Setting::switches`; each
    /// row of the Graphics tab still names the switches its note named, in the note's order
    /// (`--shadow-distance` of Longer sun shadows too: it was not "a part of" there), and no
    /// note names one any more.
    #[test]
    fn the_graphics_rows_name_the_switches_their_notes_named() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        let app = rows_app();
        let settings = app.world().resource::<Settings>();
        let want: &[(&str, &[&str])] = &[
            (UPSCALER_KEY, &["--upscaler", "--upscaler-quality", "--fsr"]),
            (QUALITY_KEY, &["--upscaler", "--upscaler-quality", "--fsr"]),
            (MODEL_KEY, &["--dlss-model"]),
            ("fsr-library", &[]),
            ("anti-aliasing", &["--msaa", "--graphics-only"]),
            ("shadow-filter", &[]),
            ("shadow-resolution", &[]),
            ("light-shadows", &["--light-shadows"]),
            (FRAME_GENERATION_KEY, &["--frame-generation"]),
            (FRAMES_KEY, &[]),
            ("latency-reduction", &["--latency-reduction"]),
            ("ambient-occlusion", &["--graphics", "--ambient-occlusion"]),
            ("fine-shadows", &["--shadow-distance", "--graphics"]),
            ("cloud-shadows", &["--graphics", "--graphics-only"]),
            ("terrain-shadows", &["--graphics", "--graphics-only"]),
            ("bloom", &["--graphics"]),
            ("glow", &["--graphics-only"]),
            ("eye-adaptation", &["--graphics-only"]),
            ("neon-glow", &["--graphics-only"]),
            ("sharpen", &["--graphics"]),
            ("anisotropic", &["--graphics"]),
            (crate::graphics::AUTHORED_MATERIALS_KEY, &["--graphics-only"]),
            (crate::graphics::AUTHORED_REFLECTIONS_KEY, &["--graphics-only"]),
            ("terrain-height-blend", &["--graphics", "--graphics-only"]),
            ("terrain-anti-tiling", &["--graphics", "--graphics-only"]),
            ("terrain-parallax", &["--graphics-only"]),
            ("light-shafts", &["--graphics"]),
            ("heat-haze", &["--graphics-only"]),
            ("desert-haze", &["--graphics", "--graphics-only"]),
            ("volumetric-clouds", &["--graphics-only"]),
            ("aerial-perspective", &["--graphics", "--graphics-only"]),
            ("night-sky-detail", &["--graphics", "--graphics-only"]),
            ("physical-pbr", &["--graphics-only"]),
            ("distant-lights", &["--graphics", "--graphics-only"]),
            (VOLUMETRIC_LIGHT_KEY, &["--graphics-only"]),
            (VOLUMETRIC_LIGHT_STRENGTH_KEY, &[]),
            ("water-reflections", &["--graphics"]),
            ("water-effects", &["--graphics", "--graphics-only"]),
            ("water-mirror", &["--graphics-only"]),
            (MESH_BLEND_KEY, &["--graphics-only"]),
            (FULL_DETAIL_KEY, &["--graphics-only"]),
            (DISTANT_OBJECTS_KEY, &["--graphics-only"]),
        ];
        for (key, switches) in want {
            let s = settings.list()[row(&app, key)];
            assert_eq!(nvf_play::settings::note_switches(s.switches), *switches, "{key}");
            assert!(!s.note.contains("--"), "{key}: the note names no switch");
        }
        assert!(settings.list().iter().all(|s| !s.note.contains("--")), "no Graphics row's note names a switch");
        assert_eq!(nvf_play::settings::note_switches(FPS_COUNTER_SWITCHES), ["--fps"]);
        assert!(nvf_play::settings::note_switches(FPS_SPLIT_SWITCHES).is_empty());
    }

    /// #3032 stage 3: the Volumetric light switch is off in a vanilla start and switches
    /// `Graphics::volumetric_light`; its strength slider runs 0 to 200 % in steps of 10 from
    /// 100 % and is greyed while the switch is off (`settings_layout::NEEDS`); Sun shafts are
    /// greyed while the light is wanted at a strength above 0.
    #[test]
    fn the_volumetric_light_rows_switch_scale_and_grey() {
        use crate::medium_air_demand::AirStrengthSetting;
        let mut app = rows_app();
        let (on_row, strength_row, shafts) = (row(&app, VOLUMETRIC_LIGHT_KEY), row(&app, VOLUMETRIC_LIGHT_STRENGTH_KEY), row(&app, "light-shafts"));
        let s = app.world().resource::<Settings>().list()[on_row];
        assert_eq!((s.group, s.label, s.note, s.switches), (GROUP_GRAPHICS, "Volumetric light (experimental)", VOLUMETRIC_LIGHT_NOTE, "--graphics-only"));
        assert!(s.slider.is_none() && !(s.get)(app.world()), "a switch, off in a vanilla start");
        let k = app.world().resource::<Settings>().list()[strength_row];
        assert_eq!((k.group, k.label, k.note, k.switches), (GROUP_GRAPHICS, "Volumetric light strength", VOLUMETRIC_LIGHT_STRENGTH_NOTE, ""));
        let slider = k.slider.expect("a slider");
        // 0..=1 in twentieths: 21 values, 0 % to 200 % in steps of 10 %.
        assert_eq!(nvf_play::settings::slider_values(&slider), 21);
        assert_eq!((slider.text(0.0), slider.text(slider.step), slider.text(1.0)), ("0%".to_string(), "10%".to_string(), "200%".to_string()));
        assert_eq!((slider.get)(app.world()), 0.5, "100 % by default (no resource yet: the default)");
        assert_eq!(slider.text((slider.get)(app.world())), "100%");
        // The greyed states: the slider while the switch is off, Sun shafts while it is on.
        let status = |app: &App, i: usize| app.world().resource::<Settings>().disabled_status(app.world(), i);
        assert_eq!(status(&app, strength_row), Some("Needs Volumetric Light."));
        assert_eq!(status(&app, shafts), None);
        assert_eq!(nvf_play::settings::flip(app.world_mut(), on_row), Some(true));
        assert!(app.world().resource::<Graphics>().volumetric_light);
        assert_eq!(status(&app, strength_row), None);
        // The demand follows in the next frame (medium_air_demand.rs; not on a device that cannot).
        assert_eq!(status(&app, shafts), None, "not wanted yet");
        app.insert_resource(crate::medium_air_demand::AirWanted(true));
        assert_eq!(status(&app, shafts), Some("Replaced by Volumetric Light."));
        // At 0 % the shafts draw, and the row says nothing.
        assert_eq!(nvf_play::settings::set_slider(app.world_mut(), strength_row, 0.0), Some(0.0));
        assert_eq!(status(&app, shafts), None, "0 %");
        assert_eq!((shafts_status(true, true, 0.0), shafts_status(false, true, 1.0)), (Some("Replaced by Volumetric Light."), None), "the aid has a strength of its own");
        // The slider sets the strength: 150 % is the part 0.75, strength 1.5.
        assert_eq!(nvf_play::settings::set_slider(app.world_mut(), strength_row, 0.75), Some(0.75));
        assert_eq!(*app.world().resource::<AirStrengthSetting>(), AirStrengthSetting(1.5));
        assert_eq!(nvf_play::settings::flip(app.world_mut(), on_row), Some(false));
        app.insert_resource(crate::medium_air_demand::AirWanted(false));
        assert_eq!(status(&app, shafts), None);
        assert_eq!(nvf_play::settings::set_slider(app.world_mut(), strength_row, 0.25), None, "greyed: not moved");
    }

    /// #3032 stage 3: both rows are kept in the INI by their keys and read back at the start.
    #[test]
    fn the_volumetric_light_rows_are_kept_in_the_ini() {
        use crate::medium_air_demand::AirStrengthSetting;
        let dir = std::env::temp_dir().join(format!("nvf-volumetric-light-rows-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("NVForever.ini");
        let mut app = ini_app(&path, "[Settings]\nvolumetric-light=1\nvolumetric-light-strength=0.75\n", UpscalerChoice::Off, GivenSwitches::default());
        assert!(app.world().resource::<Graphics>().volumetric_light, "the stored switch turns it on at the start");
        assert_eq!(*app.world().resource::<AirStrengthSetting>(), AirStrengthSetting(1.5), "the stored 150 %");
        let i = row(&app, VOLUMETRIC_LIGHT_STRENGTH_KEY);
        nvf_play::settings::set_slider(app.world_mut(), i, 0.25);
        let ini = std::fs::read_to_string(&path).unwrap();
        assert_eq!(fnv_install::ini_get(&ini, nvf_play::settings::SECTION, VOLUMETRIC_LIGHT_STRENGTH_KEY), Some("0.25".to_string()), "{ini}");
        assert_eq!(fnv_install::ini_get(&ini, nvf_play::settings::SECTION, VOLUMETRIC_LIGHT_KEY), Some("1".to_string()), "{ini}");
        let again = ini_app(&path, &ini, UpscalerChoice::Off, GivenSwitches::default());
        assert_eq!(*again.world().resource::<AirStrengthSetting>(), AirStrengthSetting(0.5), "50 % at the next start");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #3322 review: in the game the Terrain parallax row follows the run's Authored
    /// materials state, not the row's saved value (the launcher's NEEDS rule, for the next
    /// start): free while the run has them, even with the row turned off; greyed with a
    /// restart note while the row is on but the run started without them.
    #[test]
    fn the_terrain_parallax_row_follows_this_runs_authored_materials() {
        assert_eq!(parallax_status(true, true), None);
        assert_eq!(parallax_status(true, false), None, "turned off for the next start: still drawn this run");
        assert_eq!(parallax_status(false, true), Some(PARALLAX_RESTART));
        assert_eq!(parallax_status(false, false), Some(PARALLAX_NEEDS));
        assert_eq!(nvf_settings_layout_why("terrain-parallax"), Some(PARALLAX_NEEDS), "the same words as the shared NEEDS rule");
        assert!(PARALLAX_RESTART.len() <= 40);
        let mut app = rows_app();
        let i = row(&app, "terrain-parallax");
        let status = |app: &App| app.world().resource::<nvf_play::settings::Settings>().disabled_status(app.world(), i);
        assert_eq!(status(&app), None, "Authored materials on, as this run's");
        app.world_mut().resource_mut::<Graphics>().authored_materials = false;
        assert_eq!(status(&app), None, "the saved row off does not grey it while this run has them");
        assert!(!app.world().resource::<nvf_play::settings::Settings>().list()[i].note.contains("original"), "the page note says what the row adds, not what the original lacks (observer check 268: it draws none)");

        fn nvf_settings_layout_why(key: &str) -> Option<&'static str> {
            nvf_play::settings_layout::needs(key).first().map(|r| r.why)
        }
    }

    /// #2661: the mesh terrain blend's row stands among the Graphics switches, off in a
    /// vanilla start, switches `Graphics::mesh_terrain_blend` on the page and is kept in
    /// the INI, read back at the next start.
    #[test]
    fn the_mesh_blend_row_switches_the_blend_and_is_kept_in_the_ini() {
        let mut app = rows_app();
        let i = row(&app, MESH_BLEND_KEY);
        let s = app.world().resource::<Settings>().list()[i];
        assert_eq!((s.group, s.label, s.note, s.switches), (GROUP_GRAPHICS, "Blend meshes into ground", MESH_BLEND_NOTE, "--graphics-only"));
        assert!(!(s.get)(app.world()), "off in a vanilla start");
        assert_eq!(nvf_play::settings::flip(app.world_mut(), i), Some(true));
        assert!(app.world().resource::<Graphics>().mesh_terrain_blend);
        assert_eq!(nvf_play::settings::flip(app.world_mut(), i), Some(false));
        assert!(!app.world().resource::<Graphics>().mesh_terrain_blend);

        let dir = std::env::temp_dir().join(format!("nvf-mesh-blend-row-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("NVForever.ini");
        let mut app = ini_app(&path, "[Settings]\nmesh-terrain-blend=1\n", UpscalerChoice::Off, GivenSwitches::default());
        assert!(app.world().resource::<Graphics>().mesh_terrain_blend, "the stored row turns it on at the start");
        let i = row(&app, MESH_BLEND_KEY);
        nvf_play::settings::flip(app.world_mut(), i);
        let ini = std::fs::read_to_string(&path).unwrap();
        assert_eq!(fnv_install::ini_get(&ini, nvf_play::settings::SECTION, MESH_BLEND_KEY), Some("0".to_string()), "{ini}");
        assert!(!ini_app(&path, &ini, UpscalerChoice::Off, GivenSwitches::default()).world().resource::<Graphics>().mesh_terrain_blend, "and off again at the next");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #2981 reflections: the Reflections on authored materials row follows Authored
    /// materials, on in a vanilla start, switches `Graphics::authored_reflections` and is
    /// start-only (no pipeline wait: the run's state is fixed in main.rs).
    #[test]
    fn the_reflections_row_follows_authored_materials_and_switches_the_next_start() {
        let mut app = rows_app();
        let i = row(&app, crate::graphics::AUTHORED_REFLECTIONS_KEY);
        assert_eq!(i, row(&app, crate::graphics::AUTHORED_MATERIALS_KEY) + 1, "right after Authored materials");
        let s = app.world().resource::<Settings>().list()[i];
        assert_eq!((s.group, s.label, s.note, s.switches), (GROUP_GRAPHICS, "Reflections on authored materials", AUTHORED_REFLECTIONS_NOTE, "--graphics-only"));
        assert_eq!(nvf_play::settings::note_over_budget(s.note), None);
        assert!((s.get)(app.world()), "on in a vanilla start");
        assert_eq!(nvf_play::settings::flip(app.world_mut(), i), Some(false));
        assert!(!app.world().resource::<Graphics>().authored_reflections);
        assert!(app.world().resource::<Graphics>().authored_materials, "the materials row is its own");
    }

    /// A settings page with the INI at `path` holding `text`, `apply_stored` run once.
    fn ini_app(path: &std::path::Path, text: &str, command_line: UpscalerChoice, given: GivenSwitches) -> App {
        ini_app_with_frame_generation(path, text, command_line, given, crate::frame_gen::FrameGenKind::Off)
    }

    fn ini_app_with_frame_generation(path: &std::path::Path, text: &str, command_line: UpscalerChoice, given: GivenSwitches, frame_generation: crate::frame_gen::FrameGenKind) -> App {
        std::fs::write(path, text).unwrap();
        let mut app = App::new();
        let mut graphics = Graphics::preset(graphics::Preset::Vanilla);
        graphics.upscaler = command_line;
        app.insert_resource(graphics)
            .insert_resource(graphics::MsaaSetting(Msaa::Off))
            .insert_resource(crate::frame_gen::FrameGeneration { kind: frame_generation })
            .init_resource::<crate::frame_gen::DlssgFrames>()
            .insert_resource(crate::anti_lag::LatencyReduction { on: false, by_frame_generation: false })
            .insert_resource(SettingsFile(Some(path.to_path_buf())))
            .insert_resource(given);
        register_graphics(&mut app);
        app.add_systems(PostStartup, apply_stored);
        app.update();
        app
    }

    #[test]
    fn saved_frame_generation_is_changed_and_defaults_restore_the_command_line() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        use crate::frame_gen::{FrameGenKind as K, FrameGeneration};
        use nvf_play::settings::{changed_rows, restore, StartChoices, StartStates};
        let dir = std::env::temp_dir().join(format!("nvf-frame-generation-defaults-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("NVForever.ini");
        for (saved, command_line, win, want) in [
            ("true", K::Off, false, K::Fsr),
            ("fsr", K::Off, false, K::Fsr),
            ("dlss", K::Off, false, K::Dlss),
            ("dlss", K::Fsr, false, K::Dlss),
            ("dlss", K::Fsr, true, K::Fsr),
            ("true", K::Off, true, K::Off),
        ] {
            let text = format!("[Settings]\nframe-generation={saved}\n");
            let given = GivenSwitches { given: if command_line != K::Off || win { ["--frame-generation".into()].into() } else { Default::default() }, win };
            // Only the loader gets the early INI-resolved choice (main.rs).
            assert_eq!(frame_generation_after_ini(command_line, Some(&text), Some(&given)), want);
            let mut app = ini_app_with_frame_generation(&path, &text, UpscalerChoice::Off, given, command_line);
            let world = app.world_mut();
            let row = world.resource::<Settings>().list().iter().position(|s| s.key == FRAME_GENERATION_KEY).unwrap();
            let baseline = world.resource::<StartStates>().0.clone();
            assert_eq!(world.resource::<FrameGeneration>().kind, want, "INI/command-line precedence");
            assert_eq!(world.resource::<StartChoices>().0.iter().find(|(i, _)| *i == row).unwrap().1, command_line.stored());
            assert_eq!(changed_rows(world, &baseline).contains(&row), want != command_line, "Changed compares with the pre-INI choice");
            restore(world, row, baseline[row]);
            assert_eq!(world.resource::<FrameGeneration>().kind, command_line, "Defaults restores the pre-INI choice");
            assert!(!changed_rows(world, &baseline).contains(&row));
        }
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }

    /// The Frame generation row's names, INI texts and the old switch's values; the Generated
    /// frames row's choices (#2317).
    #[test]
    fn frame_generation_choices_read_names_texts_and_the_old_switch() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        use crate::frame_gen::FrameGenKind as K;
        for (text, kind) in [("Off", K::Off), ("FSR 3.1", K::Fsr), ("DLSS", K::Dlss), ("dlss", K::Dlss), ("fsr", K::Fsr), ("true", K::Fsr), ("false", K::Off), ("on", K::Fsr), ("off", K::Off)] {
            assert_eq!(K::parse(text), Some(kind), "{text}");
        }
        assert_eq!(K::parse("maybe"), None);
        assert_eq!(K::ALL.map(K::stored), ["off", "fsr", "dlss"]);
        assert_eq!((frames_named("2x"), frames_named("4X"), frames_named("5x")), (Some(1), Some(3), None));
        // The GPU's limit is not known in the tests: no choice is held back.
        assert_eq!(frames_unavailable(&World::new(), "4x"), None);
        let ini = "[Settings]\nframe-generation=dlss\n";
        let given = nvf_play::settings::GivenSwitches { given: Default::default(), win: false };
        assert_eq!(frame_generation_after_ini(K::Off, Some(ini), Some(&given)), K::Dlss, "the INI wins without the switch");
        assert_eq!(frame_generation_after_ini(K::Fsr, Some(ini), None), K::Fsr, "without a settings page the command line decides");
    }

    /// The start's choice (before the renderer, `main`) and `apply_stored`'s at `PostStartup`
    /// agree for every precedence case, for the rows' keys and the older one.
    #[test]
    fn the_start_and_the_settings_read_the_upscaler_alike() {
        let _offer_probe = crate::dlss_download::TestOfferProbe::new(Some(false));
        let dir = std::env::temp_dir().join(format!("nvf-upscaler-start-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("NVForever.ini");
        let fsr_q = UpscalerChoice::Fsr(Quality);
        let dlss_b = UpscalerChoice::Dlss(Balanced);
        // The INI's lines, the command line, the switches given, whether they win, the result.
        type Case<'a> = (&'a str, UpscalerChoice, &'a [&'a str], bool, UpscalerChoice);
        let cases: [Case; 18] = [
            ("fsr-upscaling=DLSS Balanced", fsr_q, &["--fsr"], false, dlss_b),
            ("fsr-upscaling=DLSS Balanced", fsr_q, &["--fsr"], true, fsr_q),
            ("fsr-upscaling=DLSS Balanced", fsr_q, &["--upscaler"], true, fsr_q),
            ("fsr-upscaling=DLSS Balanced", fsr_q, &["--upscaler-quality"], true, fsr_q),
            ("fsr-upscaling=DLSS Balanced", UpscalerChoice::Off, &["--msaa"], true, dlss_b),
            ("fsr-upscaling=Quality", dlss_b, &[], false, fsr_q),
            ("fsr-upscaling=0.40", UpscalerChoice::Off, &[], false, fsr_q),
            ("fsr-upscaling=2", dlss_b, &[], false, dlss_b),
            ("fsr-upscaling=nonsense", fsr_q, &[], false, fsr_q),
            ("bloom=0", dlss_b, &[], false, dlss_b),
            ("upscaler=DLSS\nupscaler-quality=Balanced", fsr_q, &[], false, dlss_b),
            ("upscaler=DLSS\nupscaler-quality=Balanced", fsr_q, &["--upscaler"], true, fsr_q),
            ("upscaler=DLSS\nupscaler-quality=Balanced", fsr_q, &["--upscaler-quality"], true, fsr_q),
            ("upscaler=DLSS", UpscalerChoice::Fsr(Performance), &[], false, UpscalerChoice::Dlss(Performance)),
            ("upscaler-quality=Balanced", fsr_q, &[], false, UpscalerChoice::Fsr(Balanced)),
            ("upscaler-quality=Balanced", UpscalerChoice::Off, &[], false, UpscalerChoice::Off),
            ("upscaler=Off\nupscaler-quality=Balanced", dlss_b, &[], false, UpscalerChoice::Off),
            ("fsr-upscaling=Performance\nupscaler=DLSS\nupscaler-quality=Ultra Performance", fsr_q, &[], false, UpscalerChoice::Dlss(UltraPerformance)),
        ];
        for (lines, command_line, given, win, want) in cases {
            let text = format!("[Settings]\n{lines}\n");
            let given = GivenSwitches { given: given.iter().map(|g| g.to_string()).collect(), win };
            let early = upscaler_after_ini(command_line, Some(&UpscalerIni::read(&text)), Some(&given));
            let late = requested(&ini_app(&path, &text, command_line, given.clone()));
            assert_eq!(early, want, "{lines:?} {command_line:?} {given:?}");
            assert_eq!(late, early, "{lines:?} {command_line:?} {given:?}: the start and the settings disagree");
        }
        // No settings page (not --play): the INI is not applied, the command line decides.
        assert_eq!(upscaler_after_ini(fsr_q, Some(&UpscalerIni::read("[Settings]\nupscaler=DLSS\n")), None), fsr_q);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod shadow_resolution_tests {
    use super::*;

    /// The Shadow resolution list takes its choices by name (2048, 4096, as it saves them) and
    /// the older slider's part of the row before the menu overhaul (0 is 2048, 1 is 4096).
    #[test]
    fn the_row_takes_its_names_and_the_older_slider_part() {
        let mut w = World::new();
        assert_eq!(shadow_resolution_chosen(&w), "2048", "the default without the map resource");
        for (text, want) in [("4096", "4096"), ("0", "2048"), ("1", "4096"), (" 2048 ", "2048"), ("1.0", "4096")] {
            assert!(take_shadow_resolution(&mut w, text), "{text}");
            assert_eq!(shadow_resolution_chosen(&w), want, "{text}");
            assert_eq!(w.resource::<bevy::light::DirectionalLightShadowMap>().size.to_string(), want, "{text}");
        }
        assert!(!take_shadow_resolution(&mut w, "big"));
        assert_eq!(shadow_resolution_chosen(&w), "4096", "an unknown text leaves the choice");
    }
}

#[cfg(test)]
mod viewmodel_fov_tests {
    use super::*;

    #[test]
    fn the_viewmodel_slider_reaches_the_originals_55_and_its_ends() {
        let slider = Slider::choice(VIEWMODEL_FOV_CHOICES, |_| 0.0, |_, _| {}, |_| String::new());
        for deg in [VIEWMODEL_FOV_MIN, crate::player_body::FIRST_PERSON_FOV, VIEWMODEL_FOV_MAX] {
            let part = viewmodel_fov_part(deg);
            assert_eq!(slider.fit(part), part, "{deg} is a stop of the slider");
            assert_eq!(viewmodel_fov_degrees(part), deg);
        }
        // Each step is 5 degrees.
        assert_eq!(viewmodel_fov_degrees(slider.fit(viewmodel_fov_part(55.0) + slider.step)), 60.0);
        // Out of range stays at the ends.
        assert_eq!(viewmodel_fov_part(10.0), 0.0);
        assert_eq!(viewmodel_fov_part(120.0), 1.0);
    }
}

#[cfg(test)]
mod tree_distance_tests {
    use super::*;
    use clap::Parser;
    use nvf_play::settings::Settings;

    /// #3016: the Tree distance row is a slider of the distant LOD's far limit in steps of
    /// 0.02 (2,500 units at the defaults), starting at `--tree-distance`'s 40,000 on a step,
    /// shown in units; its note keeps to the budget.
    #[test]
    fn the_row_reaches_the_far_limit_and_starts_at_the_flag() {
        let args = Args::try_parse_from(crate::with_default_mode(vec!["nvforever".to_string()])).unwrap();
        let mut app = App::new();
        app.insert_resource(DrawDistances::new(&args));
        register_distances(&mut app);
        let settings = app.world().resource::<Settings>();
        let s = settings.list()[settings.index("tree-distance").unwrap()];
        let slider = s.slider.expect("a slider");
        assert_eq!(nvf_play::settings::note_over_budget(s.note), None);
        assert_eq!(s.switches, "--tree-distance");
        assert_eq!(nvf_play::settings::slider_values(&slider), 50);
        assert!(!nvf_play::settings::few_step_slider(&s));
        let start = (slider.get)(app.world());
        assert_eq!(start, 0.32);
        assert_eq!(slider.fit(start), start, "the default is on a step");
        // No test builds DrawDistancePlugin: shown against the default far limit, 125,000.
        assert_eq!(slider.text(start), "40,000 units");
        assert_eq!(slider.text(slider.fit(start + slider.step)), "42,500 units");
        assert_eq!(slider.text(slider.fit(start - slider.step)), "37,500 units");
        assert_eq!(slider.text(slider.fit(0.08)), "10,000 units");
        assert_eq!(slider.text(slider.fit(1.0)), "125,000 units");
        assert_eq!(slider.fit(0.0), 0.02, "the least is one step");
        assert_eq!(slider.text(slider.fit(0.0)), "2,500 units");
    }
}

#[cfg(test)]
mod terrain_wireframe_tests {
    use super::*;
    use crate::terrain_wireframe::{TerrainWireframe, KEY, NOTE};
    use nvf_play::settings::Settings;

    fn app(on: bool) -> App {
        let mut app = App::new();
        app.insert_resource(TerrainWireframe::new(on));
        register_terrain_wireframe(&mut app);
        app
    }

    /// The Debug row: registered with its own switch and a note within the page's budget;
    /// off by default, `--terrain-wireframe` on; a click switches the wireframe live and
    /// the page's Defaults puts it back off. No row without the wireframe.
    #[test]
    fn the_row_switches_the_wireframe_live() {
        let mut app = app(false);
        let i = row_of(&app, KEY);
        let s = app.world().resource::<Settings>().list()[i];
        assert_eq!((s.group, s.label, s.switches), (GROUP_DEVELOPER, "Terrain wireframe", "--terrain-wireframe"));
        assert_eq!(nvf_play::settings::note_over_budget(NOTE), None);
        assert!(NOTE.len() < 200);
        assert!(!(s.get)(app.world()));
        assert_eq!(nvf_play::settings::flip(app.world_mut(), i), Some(true));
        assert!(app.world().resource::<TerrainWireframe>().on);
        nvf_play::settings::restore(app.world_mut(), i, 0.0);
        assert!(!app.world().resource::<TerrainWireframe>().on);
        let on = app_on();
        assert!((on.world().resource::<Settings>().list()[row_of(&on, KEY)].get)(on.world()));
        let mut none = App::new();
        register_terrain_wireframe(&mut none);
        assert!(none.world().get_resource::<Settings>().is_none_or(|s| s.index(KEY).is_none()));
    }

    fn app_on() -> App {
        app(true)
    }

    fn row_of(app: &App, key: &str) -> usize {
        app.world().resource::<Settings>().index(key).unwrap()
    }
}

/// Register the Display group's window rows (`resolution`, `borderless`, `monitor`).
fn register_window_display(app: &mut App) {
    add_choice_list(
        app,
        Setting {
            key: "resolution",
            group: GROUP_PICTURE,
            label: "Resolution",
            note: "The window's size, or in full screen the monitor's (its own size: borderless). A new one goes back after 20 seconds unless you keep it.",
            switches: "--size",
            get: |_| false,
            set: |_, _| {},
            slider: None,
        },
        ChoiceList {
            names: window_mode::resolution_names,
            chosen: window_mode::resolution_chosen,
            pick: window_mode::resolution_pick,
            save: window_mode::resolution_save,
            load: window_mode::resolution_load,
            ask_keep: true,
        },
    );
    add_setting(
        app,
        Setting {
            key: "borderless",
            group: GROUP_PICTURE,
            label: "Full screen",
            note: "On: the game fills its monitor without a frame, at the full screen resolution above. Off: a window of the window resolution.",
            // [design, #3884] In a test run (`given_switches` win), an explicit `--size`
            // asks for a window of that size, so a saved `borderless=1` in `NVForever.ini`
            // does not override it into borderless full screen (which ignores `--size`).
            switches: "--window --size",
            get: |w| w.resource::<Args>().window == window_mode::WindowChoice::Borderless,
            set: window_mode::set_borderless,
            slider: None,
        },
    );
    add_choice_list(
        app,
        Setting {
            key: "monitor",
            group: GROUP_PICTURE,
            label: "Monitor",
            note: "The display the game opens on in full screen (the main display until you pick one).",
            switches: "--window",
            get: |_| false,
            set: |_, _| {},
            slider: None,
        },
        ChoiceList {
            names: window_mode::monitor_names,
            chosen: window_mode::monitor_chosen,
            pick: window_mode::monitor_pick,
            save: window_mode::monitor_save,
            load: window_mode::monitor_load,
            ask_keep: false,
        },
    );
}

#[cfg(test)]
mod display_precedence_tests {
    use super::*;
    use clap::Parser;
    use nvf_play::settings::{apply_stored, SettingsFile};
    use window_mode::WindowChoice;

    fn run_with_ini(cli: &[&str], ini_text: &str, tag: &str) -> Args {
        let dir = std::env::temp_dir().join(format!("nvf-display-prec-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("NVForever.ini");
        std::fs::write(&path, ini_text).unwrap();
        let line: Vec<String> = cli.iter().map(|s| (*s).to_string()).collect();
        let args = Args::try_parse_from(&line).unwrap();
        let given = given_switches(&args, &line);
        let mut app = App::new();
        app.insert_resource(args);
        app.init_resource::<window_mode::FullScreenSize>();
        app.init_resource::<window_mode::MonitorModes>();
        app.init_resource::<window_mode::MonitorChoice>();
        app.insert_resource(SettingsFile(Some(path)));
        app.insert_resource(given);
        register_window_display(&mut app);
        apply_stored(app.world_mut());
        let out = app.world().resource::<Args>().clone();
        let _ = std::fs::remove_dir_all(&dir);
        out
    }

    /// #3884: in a timed/test run (`--run-seconds`), an explicit `--size` keeps the run
    /// windowed at that size instead of letting a saved `borderless=1` in `NVForever.ini`
    /// switch the window to borderless full screen (which ignores `--size`). Interactive
    /// runs without `--run-seconds` still honor the INI, and an explicit `--window
    /// borderless` still enters borderless full screen.
    #[test]
    fn explicit_size_in_a_test_run_is_not_overridden_by_saved_borderless() {
        let ini = "[Settings]\nborderless=1\nresolution=1920x1080;desktop\n";
        let timed_sized = run_with_ini(&["nvforever", "--play", "--size", "1280x720", "--run-seconds", "16"], ini, "timed-sized");
        assert_eq!(timed_sized.window, WindowChoice::Windowed);
        assert_eq!(timed_sized.size, "1280x720");

        let timed_borderless = run_with_ini(
            &["nvforever", "--play", "--size", "1280x720", "--window", "borderless", "--run-seconds", "16"],
            "[Settings]\nborderless=0\nresolution=1920x1080;desktop\n",
            "timed-borderless",
        );
        assert_eq!(timed_borderless.window, WindowChoice::Borderless);
        assert_eq!(timed_borderless.size, "1280x720");

        let interactive = run_with_ini(&["nvforever", "--play"], ini, "interactive");
        assert_eq!(interactive.window, WindowChoice::Borderless);
        assert_eq!(interactive.size, "1920x1080");
    }
}
