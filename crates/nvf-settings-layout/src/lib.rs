//! The settings page's menu: its tabs, the categories (headings) in each and the rows in
//! each category, in the page's order [design: #1953, the owner's menu overhaul 2026-11-15;
//! docs/design/settings-page.md]. One table for both menus that show the settings: the
//! game's pause menu (`engine/nvf-play/src/settings_layout.rs` applies it to the rows a run
//! registers) and the launcher's Settings page (`crates/nvf-launcher/src/catalog.rs`), so the
//! launcher never shows a tab or category the game does not have [design: owner 2026-11-16].
//!
//! [`menu`] is the file the game writes beside `NVForever.ini` once every row is registered:
//! the page as that run lays it out, with each row's label, note and kind, which the
//! launcher shows in place of its built-in copy while the versions match.
//!
//! Presentation only: a row keeps its key, note, default and registry group in the game
//! (`crate::settings` there); this table only says where the page draws it.

pub mod menu;

/// Where a row is shown: its tab, the Tweaks tab's group, the category (heading) over it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Placement {
    pub tab: &'static str,
    /// The Tweaks tab's group (`None` on every other tab).
    pub subgroup: Option<&'static str>,
    pub heading: Option<&'static str>,
}

impl Placement {
    /// A tab of its own, with no heading.
    pub const fn tab(tab: &'static str) -> Placement {
        Placement { tab, subgroup: None, heading: None }
    }

    /// Its tab, group and heading in English joined by " > " (`Gameplay > Dialogue`), as
    /// the log names it.
    pub fn path(&self) -> String {
        [Some(self.tab), self.subgroup, self.heading].into_iter().flatten().collect::<Vec<_>>().join(" > ")
    }

    /// A category of the Tweaks tab (closed until the player opens it) [design: owner
    /// 2026-11-15].
    pub fn closed_by_default(&self) -> bool {
        self.tab == TWEAKS_TAB
    }
}

/// A run of rows under one heading: registered rows by their key, the Controls tab's own by
/// [`CONTROL_PREFIX`] and their name.
pub struct Section {
    pub placement: Placement,
    pub keys: &'static [&'static str],
}

/// The tabs, in the order the page lists them [design: #1953; the overhaul].
pub const TAB_ORDER: [&str; 8] = ["Gameplay", "Display", "Audio", "Controls", "Graphics", NVF_FEATURES_TAB, TWEAKS_TAB, "Debug"];
/// The tab whose own rows (the game's `crate::bindings`) the table names by [`CONTROL_PREFIX`].
pub const CONTROLS_TAB: &str = "Controls";
/// The tab #1892's tweaks register under.
pub const TWEAKS_TAB: &str = "Tweaks";
/// NVForever's own features the owner names [design: owner 2026-11-15].
pub const NVF_FEATURES_TAB: &str = "NVF Features";
/// A Controls row in the table: `controls:` and its name (`crate::settings_view::ControlRow`).
pub const CONTROL_PREFIX: &str = "controls:";

pub const fn section(tab: &'static str, heading: Option<&'static str>, keys: &'static [&'static str]) -> Section {
    Section { placement: Placement { tab, subgroup: None, heading }, keys }
}

/// A category of the Tweaks tab: under its group `subgroup`, headed `heading`.
pub const fn group_section(subgroup: &'static str, heading: &'static str, keys: &'static [&'static str]) -> Section {
    Section { placement: Placement { tab: TWEAKS_TAB, subgroup: Some(subgroup), heading: Some(heading) }, keys }
}

pub const MOBILE_TAB: &str = "Mobile";
pub const MOBILE_TAB_ORDER: [&str; 9] = ["Gameplay", "Display", "Audio", "Controls", MOBILE_TAB, "Graphics", NVF_FEATURES_TAB, TWEAKS_TAB, "Debug"];

/// Additional controller rows registered by the port.
pub const ANDROID_CONTROLLER_KEYS: [&str; 4] = [
    "controller-ads-v1", "controller-dead-zone-v1",
    "controller-toggle-aim-v1", "controller-hold-sneak-v1",
];

// Android adds only its own rows; the shared Main table stays authoritative.
// A matching category appends these keys after its shared rows.
pub const ANDROID_SECTIONS: &[Section] = &[
    section("Gameplay", Some("Camera"), &["touch-pipboy-size-v1", "touch-pipboy-tab-swipes"]),
    section("Gameplay", Some("Dialogue"), &["touch-dialogue", "dialogue-choice-text-size", "dialogue-subtitle-text-size", "dialogue-touch-choice", "dialogue-speaker-side", "dialogue-device-side-v1", "dialogue-theme-v1", "dialogue-background-opacity-v1", "dialogue-background-tint-v1", "dialogue-frame-opacity-v1", "dialogue-hints-v1"]),
    section("Display", Some("Interface"), &["hud-scale-v2", "hud-health-size-v1", "hud-action-points-size-v1", "hud-compass-size-v1", "settings-menu-scale-v1", "barter-menu-size-v1", "repair-menu-size-v1", "workbench-menu-size-v1"]),
    section("Gameplay", Some("Inventory menus"), &["touch-barter-v1", "touch-maintenance-v1", "touch-terminal-v1", "touch-companion-v1"]),
    section("Controls", Some("Touch"), &["touch-layout-editor-v1", "touch-practice-v1", "touch-hotbar", "touch-hotbar-slots", "touch-hotbar-size", "touch-hotbar-tray", "touch-tray-editor-v1", "touch-bottom-swipe-target-v1", "touch-pipboy-swipe-v1", "touch-pipboy-distance-v1", "touch-pipboy-dismiss-left", "touch-pipboy-dismiss-right", "touch-pipboy-dismiss-side-v2", "touch-pipboy-dismiss-travel-v1", "touch-pipboy-close-button", "touch-pipboy-button-graphics", "touch-pipboy-model-wheel", "touch-direct-pipboy-v1", "touch-world-activation-v1", "touch-icon-family", "adaptive-touch-actions", "lockpick-touch", "lockpick-touch-size", "lockpick-touch-hints", "touch-gyro-v1", "touch-gyro-sensitivity-v1", "touch-gyro-ads-v1", "touch-gyro-toggle-v1", "touch-gyro-ads-only-v1", "touch-gyro-recenter-v1", "touch-gyro-invert-x-v1", "touch-gyro-invert-y-v1", "touch-analog-movement-v1", "touch-fire-drag-look-v1", "touch-auto-move-lock-v1", "touch-vats-double-tap-v1", "touch-vats-targeting-v1"]),
    section(CONTROLS_TAB, Some("Controller"), &ANDROID_CONTROLLER_KEYS),
    section("Display", Some("Mobile performance"), &["mobile-performance", "mobile-fps", "mobile-resolution", "mobile-thermal-auto", "mobile-thermal-status"]),
];

/// Android claims only its named presentation overrides. Main's original controller
/// rows keep their Input/Aim Assist categories, while all hardware routes stay Controls.
pub const MOBILE_SECTIONS: &[Section] = &[
    section(MOBILE_TAB, Some("Touch layout and guide"), &[
        "touch-layout-editor-v1", "touch-practice-v1", "touch-icon-family", "adaptive-touch-actions",
    ]),
    section(MOBILE_TAB, Some("Movement and gestures"), &[
        "touch-analog-movement-v1", "touch-fire-drag-look-v1",
        "touch-auto-move-lock-v1", "touch-vats-double-tap-v1", "touch-vats-targeting-v1", "touch-world-activation-v1",
    ]),
    section(MOBILE_TAB, Some("Gyroscope"), &[
        "touch-gyro-v1", "touch-gyro-recenter-v1", "touch-gyro-toggle-v1",
        "touch-gyro-ads-only-v1", "touch-gyro-invert-x-v1", "touch-gyro-invert-y-v1",
        "touch-gyro-sensitivity-v1", "touch-gyro-ads-v1",
    ]),
    section(MOBILE_TAB, Some("Inventory and quick access"), &[
        "touch-hotbar", "touch-hotbar-slots", "touch-hotbar-tray", "touch-tray-editor-v1", "touch-hotbar-size",
        "touch-pipboy-swipe-v1", "touch-bottom-swipe-target-v1", "touch-pipboy-distance-v1", "touch-pipboy-size-v1", "touch-pipboy-tab-swipes", "touch-direct-pipboy-v1",
        "touch-pipboy-dismiss-left", "touch-pipboy-dismiss-right", "touch-pipboy-dismiss-side-v2", "touch-pipboy-dismiss-travel-v1", "touch-pipboy-close-button",
        "touch-pipboy-button-graphics", "touch-pipboy-model-wheel",
        "touch-barter-v1", "touch-maintenance-v1", "touch-terminal-v1", "touch-companion-v1",
    ]),
    section(MOBILE_TAB, Some("Dialogue"), &[
        "touch-dialogue", "dialogue-theme-v1", "dialogue-touch-choice", "dialogue-speaker-side",
        "dialogue-device-side-v1", "dialogue-background-tint-v1", "dialogue-hints-v1", "dialogue-choice-text-size",
        "dialogue-subtitle-text-size", "dialogue-background-opacity-v1", "dialogue-frame-opacity-v1",
    ]),
    section(MOBILE_TAB, Some("Lockpicking"), &["lockpick-touch", "lockpick-touch-hints", "lockpick-touch-size"]),
    section(MOBILE_TAB, Some("Interface size"), &[
        "menu-scale", "settings-menu-scale-v1", "ui-scale", "hud-scale-v2", "hud-health-size-v1", "hud-action-points-size-v1", "hud-compass-size-v1",
        "barter-menu-size-v1", "repair-menu-size-v1", "workbench-menu-size-v1",
    ]),
    section(MOBILE_TAB, Some("Mobile performance"), &[
        "mobile-performance", "mobile-fps", "mobile-resolution", "mobile-thermal-auto", "mobile-thermal-status",
    ]),
];

/// The rows the NVF Features tab may hold: only what the owner named there [design: owner
/// 2026-11-15]. Adding a key needs the owner's word; a bigger gameplay change is offered to
/// the owner for this tab, never moved here unasked (docs/design/settings-page.md).
pub const NVF_FEATURES_KEYS: [&str; 16] = [
    "advanced-movement",
    "movement-weight",
    "movement-inertia",
    "weapon-handling",
    "advanced-recoil",
    "sprint",
    "weapon-inertia",
    "animation-smoothing",
    "camera-motion",
    "grass-fire",
    // The Quick Loot menu and its columns [design, #2933].
    "quick-loot",
    "quick-loot-scale",
    "quick-loot-background",
    "quick-loot-weight",
    "quick-loot-value",
    "quick-loot-value-per-weight",
];

/// Every named row's tab and category, in the order the page shows them [design: the
/// owner's menu overhaul, 2026-11-15, tab by tab; docs/design/settings-page.md §2]. A key
/// not registered in a run is left out (`resolve_layout`), so rows of features still to
/// come (the movement rows, #1892's tweaks) can have their place already. The Tweaks tab's
/// groups come in the plugin's order: Gameplay, Combat, Quality of Life, HUD, Menus,
/// Hotkeys, Cheats, Misc.
pub const SECTIONS: &[Section] = &[
    // #2688's sound cover with the game's own rules (a bigger gameplay change: NVF Features is the
    // owner's call, recommended to them, not moved there).
    section("Gameplay", Some("Game"), &["difficulty", "difficulty-player-damage", "difficulty-npc-damage", "difficulty-kill-xp", "hardcore", "reload-slows", "UsePipBoy", "sound-cover", "npc-weapon-mods", "sneak-bounding-box"]),
    // "V.A.T.S." as the data spells it (MESG `HelpVATS`).
    section("Gameplay", Some("V.A.T.S."), &["vats-style", "vats-slowdown", "kill-camera"]),
    section("Gameplay", Some("Camera"), &["detached-camera", "true-iron-sights", "first-person-body", "weapon-lower", "gore-shake", "field-of-view", "viewmodel-field-of-view"]),
    section("Gameplay", Some("Dialogue"), &["dialogue-zoom", "dialogue-look", "dialogue-recentre", "menu-pause", "dialogue-goodbye-hint", "lip-sync"]),
    section("Gameplay", Some("Saving"), &["autosave-travel", "autosave-fast-travel", "autosave-wait", "autosave-sleep", "autosave-scripts"]),
    section("Gameplay", Some("Combat"), &["localized-dt-dr"]),
    section("Gameplay", Some("Menus"), &["quantity-select-short-keys"]),
    section("Gameplay", Some("Bug Reports"), &["send-reports", "reports-delete"]),
    // Co-op, and its networking (docs/design/coop-networking.md §10).
    section("Gameplay", Some("Co-op"), &["coop", "coop-connection", "coop-router-port", "coop-relay", "coop-ask"]),
    section("Display", Some("Screen"), &["monitor", "resolution", "hdr-output", "borderless"]),
    section("Display", Some("Frame Rate"), &["display-sync", "frame-cap", "physics-rate"]),
    section("Display", Some("Upscaling and Anti-Aliasing"), &["upscaler", "upscaler-quality", "dlss-model", "fsr-library", "anti-aliasing", "sharpen"]),
    section("Display", Some("Frame Generation and Latency"), &["frame-generation", "frame-generation-frames", "latency-reduction"]),
    section("Display", Some("Interface"), &["language", "menu-font", "sneak-gauges", "caravan-tips", "dynamic-crosshair", "crosshair-style", "location-load-screens", "ui-scale", "menu-scale"]),
    section("Display", Some("Colours"), &["hud-colour", "hud-red", "hud-green", "hud-blue", "pipboy-colour", "pipboy-red", "pipboy-green", "pipboy-blue"]),
    section(
        "Display",
        Some("Performance Overlay"),
        &[
            "fps-counter",
            "fps-counter-fps",
            "fps-counter-graph",
            "fps-counter-split",
            "fps-counter-cpu",
            "fps-counter-gpu",
            "fps-counter-vram",
            "fps-counter-ram",
            "fps-counter-loaded",
            "fps-counter-upscaling",
            "fps-counter-low",
        ],
    ),
    section("Audio", Some("Volume"), &["volume-master", "volume-music", "volume-footstep", "volume-voice", "volume-effects", "volume-radio"]),
    section("Audio", Some("Sound"), &["music", "voice", "reverb", "voice-reverb"]),
    section("Controls", Some("Input"), &["controls:device", "button-style", "controls:automatic-input", "controls:rumble"]),
    section("Controls", Some("Look"), &["controls:invert", "controls:sensitivity", "controls:vertical", "controls:aim-sensitivity"]),
    section("Controls", Some("Aim Assist"), &["aim-assist", "aim-slowdown", "aim-lock"]),
    section(
        "Controls",
        Some("Movement"),
        &[
            "controls:bind:Forward",
            "controls:bind:Back",
            "controls:bind:Slide Left",
            "controls:bind:Slide Right",
            "controls:bind:Jump",
            "controls:bind:Crouch/Sneak",
            "controls:bind:Run",
            "controls:bind:Always Run",
            "controls:bind:Auto Move",
            // NVForever's own, greyed while its NVF Features switch is off (#3000).
            "controls:bind:Sprint",
        ],
    ),
    section("Controls", Some("Combat"), &["controls:bind:Use", "controls:bind:Block", "controls:bind:Ready Item", "controls:bind:Vats", "controls:bind:Ammo Swap", "controls:bind:Lower Weapon"]),
    section(
        "Controls",
        Some("Actions"),
        &["controls:bind:Activate", "controls:bind:Grab", "controls:bind:Menu Mode", "controls:bind:Rest", "controls:bind:Toggle POV", "controls:bind:QuickSave", "controls:bind:QuickLoad", "controls:bind:Photo Mode"],
    ),
    section(
        "Controls",
        Some("Hotkeys"),
        &["controls:bind:Hotkey1", "controls:bind:Hotkey3", "controls:bind:Hotkey4", "controls:bind:Hotkey5", "controls:bind:Hotkey6", "controls:bind:Hotkey7", "controls:bind:Hotkey8"],
    ),
    // The tweaks' own keys (#1892, `crate::bindings::TWEAK_CONTROLS` on), none bound until the
    // player binds one, and the console's and the screenshot's (bound): their own category
    // after the game's [design: the menu overhaul, merged with #1892; owner 2026-11-16:
    // "Additional Features", the console and screenshot keys moved here as bindings].
    section(
        "Controls",
        Some("Additional Features"),
        &[
            "controls:bind:Holster Weapon",
            "controls:bind:Toggle Light",
            "controls:bind:Toggle Crosshair",
            "controls:bind:Toggle Sneak Label",
            "controls:bind:Toggle Menus",
            "controls:bind:Exit to Main Menu",
            "controls:bind:Exit Game",
            "controls:bind:No Collision",
            "controls:bind:Pip-Boy Stats",
            "controls:bind:Pip-Boy Items",
            "controls:bind:Pip-Boy Data",
            "controls:bind:Local Map",
            "controls:bind:World Map",
            "controls:bind:Quests",
            "controls:bind:Radio",
            "controls:bind:Toggle Iron Sights",
            "controls:bind:Drop Weapon",
            "controls:bind:Radio Volume Up",
            "controls:bind:Radio Volume Down",
            "controls:bind:Last Weapon",
            "controls:bind:Smooth Camera",
            "controls:bind:Repeat Activate",
            "controls:bind:Toggle Sighting Node",
            "controls:bind:Place Marker Here",
            "controls:bind:Hide HUD",
            "controls:bind:Skip Radio Song",
            "controls:bind:Console",
            "controls:bind:Screenshot",
        ],
    ),
    section("Graphics", Some("Quality"), &["quality"]),
    // Shadow resolution sizes the flashlight's maps too, so it needs no Sun shadows (no NEEDS
    // line) (#2843).
    section("Graphics", Some("Shadows"), &["sun-shadows", "shadow-filter", "shadow-resolution", "shadow-distance", "fine-shadows", "cloud-shadows", "terrain-shadows", "light-shadows"]),
    section("Graphics", Some("Lighting and Effects"), &["ambient-occlusion", "volumetric-clouds", "light-shafts", "heat-haze", "desert-haze", "volumetric-light", "volumetric-light-strength", "aerial-perspective", "night-sky-detail", "physical-pbr", "tonemap-preset", "bloom", "grade", "glow", "eye-adaptation", "neon-glow"]),
    section("Graphics", Some("Water"), &["water-reflections", "water-mirror", "water-effects"]),
    section("Graphics", Some("Detail and Distance"), &["distant-lod", "full-detail", "distant-objects", "grass", "anisotropic", "authored-materials", "authored-reflections", "terrain-height-blend", "terrain-anti-tiling", "terrain-parallax", "mesh-terrain-blend", "occlusion-culling", "smooth-terrain", "object-distance", "actor-distance", "item-distance", "grass-distance", "tree-distance", "distant-lights"]),
    section("Graphics", Some("World and Animation"), &["head-tracking", "foot-ik", "sky-clock"]),
    // The owner's own layout of the movement work (tech/movement-inertia, 2026-11-15).
    section(NVF_FEATURES_TAB, Some("Advanced Movement"), &["advanced-movement", "movement-weight"]),
    // Sprint after Advanced Recoil (#3000).
    section(NVF_FEATURES_TAB, Some("Gameplay"), &["movement-inertia", "weapon-handling", "advanced-recoil", "sprint"]),
    section(NVF_FEATURES_TAB, Some("Visual Only"), &["weapon-inertia", "animation-smoothing", "camera-motion"]),
    section(NVF_FEATURES_TAB, Some("World Interaction"), &["grass-fire"]),
    // The Quick Loot menu (docs/design/quick-loot.md) [design, #2933].
    section(NVF_FEATURES_TAB, Some("Quick Loot"), &["quick-loot", "quick-loot-scale", "quick-loot-background", "quick-loot-weight", "quick-loot-value", "quick-loot-value-per-weight"]),
    // The Tweaks tab (#1892), placed ahead of it: Stewie's Tweaks' groups, each in its
    // categories (docs/design/settings-tweaks-layout.md) [design: owner 2026-11-15].
    // Tweaks > Gameplay.
    group_section(
        "Gameplay",
        "Character and Experience",
        &[
            "tweak-perks-per-level",
            "tweak-free-wild-wasteland",
            "tweak-frenzy-kill-xp",
            "tweak-no-skill-tags",
            "tweak-custom-skill-points",
            "tweak-skill-point-base",
            "tweak-skill-points-per-int",
            "tweak-cap-skills-by-special",
            "tweak-cap-skills-spare-tagged",
            "tweak-skill-cap-base",
            "tweak-skill-cap-most",
            "tweak-skill-cap-per-special",
            "tweak-skill-cap-per-luck",
            "tweak-xp-multiplier",
            "tweak-key-xp-share",
            "tweak-password-xp-share",
            "tweak-kill-xp-per-level",
            "tweak-max-level",
            "tweak-special-points",
        ],
    ),
    group_section(
        "Gameplay",
        "Movement and Jumping",
        &[
            "tweak-armor-sinks",
            "tweak-no-aiming-slowdown",
            "tweak-jump-swims-up",
            "tweak-speed-scales-jump",
            "tweak-jump-ap-cost",
            "tweak-jump-ap-danger-only",
            "tweak-jump-without-enough-ap",
            "tweak-run-ap-cost",
            "tweak-sneak-run-ap-cost",
            "tweak-sneak-run-ap-endurance",
            "tweak-agility-speed",
            "tweak-agility-jump",
            "tweak-back-pace",
            "tweak-crippled-leg-jump",
            "tweak-strafe-pace",
            "tweak-breath-regain",
            "tweak-walk-angle",
            "tweak-wading-pace",
        ],
    ),
    group_section(
        "Gameplay",
        "Weapons and Reloading",
        &[
            "tweak-aiming-draws-weapon",
            "tweak-full-reload-swaps",
            "tweak-hold-breath",
            "tweak-breath-activate",
            "tweak-breath-ap",
            "tweak-breath-sway",
            "tweak-jam-reload-pace",
            "tweak-wheel-cycles-hotkeys",
            "tweak-no-hotkeys-reloading",
            "tweak-god-mode-no-jams",
            "tweak-npcs-earn-casings",
            "tweak-per-bullet-reload",
            "tweak-reload-press-queue",
            "tweak-reload-rounds-per-press",
            "tweak-power-attack-if-blocking",
            "tweak-ready-does-not-draw",
            "tweak-reload-when-empty",
            "tweak-reload-while-firing",
            "tweak-weapons-unhurt-by-hits",
            "tweak-no-wear-on-dead",
            "tweak-scope-wheel-zoom",
            "tweak-melee-reach",
            "tweak-npc-weapon-volume",
            "tweak-scope-zoom-time",
            "tweak-semi-auto-queue",
            "tweak-player-weapon-volume",
        ],
    ),
    group_section(
        "Gameplay",
        "V.A.T.S.",
        &[
            "tweak-vats-target-projectiles",
            "tweak-vats-target-mines",
            "tweak-vats-ignore-non-explosive",
            "tweak-vats-target-own-projectiles",
            "tweak-vats-drops-disarmed-shots",
            "tweak-vats-hip-fire",
            "tweak-vats-throw-visibility",
        ],
    ),
    group_section(
        "Gameplay",
        "Explosives",
        &[
            "tweak-contact-mines-npcs",
            "tweak-contact-mines-player",
            "tweak-disarm-needs-skill",
            "tweak-disarm-mult",
            "tweak-disarm-explode-mult",
            "tweak-explosion-radius-buff",
            "tweak-blasts-spare-items",
            "tweak-mine-blasts-heard",
            "tweak-npcs-drop-live-grenades",
            "tweak-planting-needs-skill",
            "tweak-knockdown-by-strength",
        ],
    ),
    group_section(
        "Gameplay",
        "Damage and Health",
        &[
            "tweak-aid-at-full-health",
            "tweak-crit-ignores-fire-rate",
            "tweak-character-radiation",
            "tweak-character-radiation-stages",
            "tweak-crippling-never-disarms",
            "tweak-fall-deaths-dismember",
            "tweak-drowning-drains-ap",
            "tweak-hardcore-tweaks",
            "tweak-hardcore-ammo-weight",
            "tweak-hardcore-essential-companions",
            "tweak-hardcore-sleep-heals",
            "tweak-keep-fall-height-on-load",
            "tweak-no-npc-addiction",
            "tweak-no-player-stagger",
            "tweak-power-armor-radiation",
            "tweak-power-armor-no-training",
            "tweak-standing-sneak-attacks",
            "tweak-armor-condition-penalty",
            "tweak-crippled-fall-pain",
            "tweak-fall-height-endurance",
            "tweak-power-armor-fall",
            "tweak-power-armor-limbs",
            "tweak-sleep-heal-min-hours",
            "tweak-sleep-misc-stat-requires-duration",
        ],
    ),
    group_section(
        "Gameplay",
        "Weight",
        &[
            "tweak-grabbed-weighs",
            "tweak-run-overencumbered",
            "tweak-over-jump-ap",
            "tweak-over-run-ap",
            "tweak-weightless-worn-armor",
            "tweak-weightless-worn-power-armor",
            "tweak-encumbered-ap-regen",
        ],
    ),
    group_section(
        "Gameplay",
        "Locks, Hacking and Stealing",
        &[
            "tweak-pickpocket-caught-detects",
            "tweak-force-spares-lock",
            "tweak-grabbing-is-theft",
            "tweak-grabbing-is-theft-min-value",
            "tweak-hack-dud-when-full",
            "tweak-hack-spare-guessed",
            "tweak-hack-no-repeat-words",
            "tweak-hack-no-single-chars",
            "tweak-hack-mark-guesses-as-duds",
            "tweak-hack-keep-matching-chars",
            "tweak-hack-compact-guesses",
            "tweak-hack-replenish-bonus",
            "tweak-key-name",
            "tweak-no-grab-owned",
            "tweak-no-terminal-lockout",
            "tweak-pick-without-skill",
            "tweak-pickpocket-overhaul",
            "tweak-pp-reward-xp",
            "tweak-pp-free-items",
            "tweak-pp-agility",
            "tweak-pp-base",
            "tweak-pp-detection",
            "tweak-pp-value",
            "tweak-pp-weight",
            "tweak-pp-luck",
            "tweak-pp-sneak",
            "tweak-pp-perception",
            "tweak-pp-worn",
            "tweak-pickpocket-fleeing",
            "tweak-stealing-no-alarm",
            "tweak-remember-pin-health",
            "tweak-loot-unconscious",
            "tweak-auto-unlock-terminals",
            "tweak-auto-unlock-margin",
            "tweak-skip-easy-locks",
            "tweak-skip-lock-margin",
            "tweak-lockpick-force-near",
        ],
    ),
    group_section(
        "Gameplay",
        "Trading and Repair",
        &[
            "tweak-barter-by-kind",
            "tweak-barter-water-unscaled",
            "tweak-barter-buy-aid",
            "tweak-barter-buy-ammo",
            "tweak-barter-buy-armor",
            "tweak-barter-buy-misc",
            "tweak-barter-buy-weapon",
            "tweak-barter-buy-weapon-mod",
            "tweak-barter-sell-aid",
            "tweak-barter-sell-ammo",
            "tweak-barter-sell-armor",
            "tweak-barter-sell-misc",
            "tweak-barter-sell-weapon",
            "tweak-barter-sell-weapon-mod",
            "tweak-repair-scales-crafting",
            "tweak-vendor-lists",
            "tweak-repair-caps-container",
            "tweak-repair-kits-in-list",
            "tweak-repair-at-workbench",
            "tweak-modding-needs-skill",
            "tweak-barter-repair-discount",
            "tweak-crafted-item-health-pct",
            "tweak-repair-xp",
            "tweak-free-item-price",
        ],
    ),
    group_section(
        "Gameplay",
        "Companions",
        &[
            "tweak-hostiles-prefer-player",
            "tweak-prefer-player-start",
            "tweak-prefer-player-mid",
            "tweak-prefer-player-detection",
            "tweak-light-step-companions",
            "tweak-no-companion-item-damage",
            "tweak-revive-companions",
            "tweak-revive-down-seconds",
            "tweak-revive-on-cell-change",
            "tweak-map-summon-companions",
            "tweak-summon-unconscious",
        ],
    ),
    group_section(
        "Gameplay",
        "Travel and World",
        &[
            "tweak-travel-costs-bottle",
            "tweak-fast-travel-encumbered",
            "tweak-hold-activate",
            "tweak-hold-containers",
            "tweak-hold-furniture",
            "tweak-hold-water",
            "tweak-no-lock-encounter-zone-levels",
            "tweak-disable-fast-travel",
            "tweak-no-travel-crippled-leg",
            "tweak-quick-use",
            "tweak-sleep-on-chairs",
            "tweak-wait-only-seated",
            "tweak-clear-marker-within",
            "tweak-mark-nearby-places",
        ],
    ),
    // Tweaks > Combat.
    group_section("Combat", "In Combat", &["tweak-no-combat-music", "tweak-hide-enemy-markers", "tweak-no-healing-in-combat", "tweak-no-pipboy-in-combat", "tweak-kill-reward-ap"]),
    group_section("Combat", "V.A.T.S.", &["tweak-first-person-vats", "tweak-vats-burst-stops-on-kill", "tweak-vats-ignores-companions", "tweak-vats-auto-head", "tweak-vats-entry-ap"]),
    group_section(
        "Combat",
        "Criticals and Sneak Attacks",
        &["tweak-fatal-crits-gib", "tweak-no-sneak-criticals", "tweak-npcs-sneak-crit-player", "tweak-sneak-criticals-head", "tweak-sneak-criticals-melee", "tweak-crit-damage-scale"],
    ),
    group_section(
        "Combat",
        "Weapons and Armour",
        &[
            "tweak-npcs-disarm-player",
            "tweak-npc-melee-part-damage",
            "tweak-hold-release-throws",
            "tweak-keep-chosen-ammo",
            "tweak-no-condition-damage-buffer",
            "tweak-no-condition-damage-penalty",
            "tweak-no-spin-up",
            "tweak-npc-dr-ignores-condition",
            "tweak-npc-dt-ignores-condition",
            "tweak-npc-weapons-whole",
            "tweak-weapon-requirements-matter",
            "tweak-power-attack-ap",
        ],
    ),
    group_section(
        "Combat",
        "Companions and Blasts",
        &[
            "tweak-no-disarm-companions",
            "tweak-teammates-meltdown",
            "tweak-no-self-meltdown",
            "tweak-no-companion-kill-xp",
            "tweak-no-self-explosion-damage",
            "tweak-no-self-explosion-non-hostiles",
        ],
    ),
    // Tweaks > Quality of Life.
    group_section(
        "Quality of Life",
        "Controls and Movement",
        &[
            "tweak-seat-keeps-on-activate",
            "tweak-console-skips-player",
            "tweak-repeat-jumping",
            "tweak-hold-to-sneak",
            "tweak-hold-to-wait",
            "tweak-move-during-pipboy-raise",
            "tweak-no-knockdown-god",
            "tweak-no-land-clip",
            "tweak-toggle-device",
            "tweak-seated-wide",
        ],
    ),
    group_section(
        "Quality of Life",
        "Weapons and Aiming",
        &[
            "tweak-dpad-cycles-hotkeys",
            "tweak-double-tap-ammo",
            "tweak-hi-res-npc-weapons",
            "tweak-holster-real-time",
            "tweak-hotkey-toggles-in-hand",
            "tweak-night-vision-toggle",
            "tweak-night-vision-by-day",
            "tweak-night-vision-off-first",
            "tweak-no-sight-zoom",
            "tweak-no-weapon-move-penalty",
            "tweak-npcs-drop-holstered",
            "tweak-empty-reload-swaps-ammo",
            "tweak-vats-shift-ignores-friends",
            "tweak-swap-ammo-holstered",
            "tweak-deselect-hotkeys",
            "tweak-vats-keeps-holstered",
            "tweak-aim-turn-x",
            "tweak-aim-turn-y",
            "tweak-scope-delay",
        ],
    ),
    group_section(
        "Quality of Life",
        "Items and Repair",
        &[
            "tweak-equip-broken-items",
            "tweak-hide-misc-quest",
            "tweak-keep-broken-equipped",
            "tweak-no-poison-confirm",
            "tweak-repair-needs-weapon-skill",
            "tweak-unequip-broken-armor",
            "tweak-stack-figures",
        ],
    ),
    group_section("Quality of Life", "Companions", &["tweak-quiet-followers", "tweak-robots-heal-with-scrap", "tweak-robot-heal-base", "tweak-robot-heal-per-repair", "tweak-robot-heal-expert"]),
    group_section(
        "Quality of Life",
        "Minigames and Machines",
        &["tweak-faster-hacking", "tweak-no-tutorials", "tweak-pick-locks-with-key", "tweak-vigor-review-first", "tweak-show-current-skill", "tweak-slots-hold-to-spin"],
    ),
    group_section("Quality of Life", "Travel and Loading", &["tweak-unsafe-fast-travel", "tweak-midair-fast-travel", "tweak-faster-sleep-wait", "tweak-pause-on-load", "tweak-plain-going-out"]),
    group_section(
        "Quality of Life",
        "Messages and Sounds",
        &[
            "tweak-mark-unspoken",
            "tweak-no-quest-set-on-complete",
            "tweak-no-quest-set-on-objective",
            "tweak-no-empty-book-message",
            "tweak-god-mode-quiet-landing",
            "tweak-no-reputation-messages",
            "tweak-xp-sound-at-cap",
            "tweak-player-pain-gap",
        ],
    ),
    group_section("Quality of Life", "Conversations and Radio", &["tweak-no-auto-continue-dialogue", "tweak-select-unavailable-radios", "tweak-dialogue-music", "tweak-dialogue-radio"]),
    group_section(
        "Quality of Life",
        "Screen and Camera",
        &[
            "tweak-faster-pad-orbit",
            "tweak-move-during-vats-camera",
            "tweak-no-chem-wore-off",
            "tweak-no-explosion-blur",
            "tweak-no-interior-fog",
            "tweak-scale-explosion-shake",
            "tweak-explosion-shake",
            "tweak-shift-screenshot-hides",
            "tweak-skip-death-cam",
            "tweak-skip-death-cam-attack",
            "tweak-kill-cam-exit-key",
            "tweak-min-splatter",
        ],
    ),
    // Tweaks > HUD.
    group_section(
        "HUD",
        "Messages",
        &[
            "tweak-crit-message-limb",
            "tweak-discovered-in-corner",
            "tweak-descriptive-marker-added",
            "tweak-no-caps-added",
            "tweak-no-dehydration-messages",
            "tweak-no-location-popup",
            "tweak-no-objective-popup",
            "tweak-no-quest-failed",
            "tweak-no-sleep-messages",
            "tweak-no-unconscious-message",
            "tweak-no-xp-meter-combat",
            "tweak-objectives-on-arrival",
            "tweak-objectives-gap",
            "tweak-no-xp-messages",
            "tweak-no-karma-messages",
            "tweak-item-added-seconds",
        ],
    ),
    group_section(
        "HUD",
        "Compass and Place Names",
        &[
            "tweak-companion-pip",
            "tweak-compass-fade-left",
            "tweak-compass-pips-danger",
            "tweak-hide-unmapped-places",
            "tweak-compass-doors",
            "tweak-compass-doors-fade",
            "tweak-compass-doors-interior",
            "tweak-compass-doors-exterior",
            "tweak-compass-pips-fade",
            "tweak-compass-places-fade",
            "tweak-neutral-foes-compass",
            "tweak-nearest-unfound",
            "tweak-hud-marker-name",
            "tweak-marker-name-units",
            "tweak-marker-name-hide-unfound",
            "tweak-marker-name-angle",
            "tweak-compass-quest-height",
            "tweak-hud-region-names",
            "tweak-compass-marker-height",
            "tweak-quest-marker-reach",
            "tweak-recent-dead-secs",
        ],
    ),
    group_section(
        "HUD",
        "Crosshair",
        &["tweak-crosshair-when-aiming", "tweak-no-red-crosshair-cloaked", "tweak-hide-crosshair-first", "tweak-hide-crosshair-third", "tweak-hide-crosshair-reloading", "tweak-no-red-crosshair"],
    ),
    group_section(
        "HUD",
        "Bars and Indicators",
        &[
            "tweak-ammo-label-under",
            "tweak-armor-condition",
            "tweak-armor-in-weapon-place",
            "tweak-armor-flash-below",
            "tweak-armor-red-below",
            "tweak-armor-visible-below",
            "tweak-hud-fatigue",
            "tweak-buffed-health-bars",
            "tweak-no-grenade-indicator",
            "tweak-grenade-indicator-destructibles",
            "tweak-precise-rad-meter",
            "tweak-rad-decimals",
            "tweak-hide-ammo-label",
            "tweak-no-limb-indicator",
            "tweak-grenade-indicator-harmless",
            "tweak-weapon-cnd-flash-below",
            "tweak-red-ap-below",
            "tweak-red-hp-below",
            "tweak-weapon-cnd-red-below",
        ],
    ),
    group_section("HUD", "Sneaking", &["tweak-detected-by-whom", "tweak-detected-names", "tweak-no-sneak-label", "tweak-shift-talks-sneaking", "tweak-sneak-label-standing"]),
    group_section(
        "HUD",
        "Prompts and Subtitles",
        &["tweak-hide-unknown-names", "tweak-mark-unvisited-doors", "tweak-terminal-password-prompt", "tweak-subtitle-names", "tweak-water-shows-h2o", "tweak-subtitle-distance"],
    ),
    group_section("HUD", "Other", &["tweak-date-style", "tweak-female-facial-hair", "tweak-no-hotkey-wheel"]),
    // Tweaks > Menus.
    group_section(
        "Menus",
        "Pip-Boy Items",
        &[
            "tweak-tab-click-sorts",
            "tweak-compare-armor",
            "tweak-compare-weapons",
            "tweak-mark-other-ammo",
            "tweak-hide-quest-item-figures",
            "tweak-no-right-click-drop",
            "tweak-single-item-figures",
            "tweak-drop-key",
            "tweak-remember-item-places",
            "tweak-weapon-ammo-use",
            "tweak-show-poison-effects",
            "tweak-book-effects",
            "tweak-usable-ammo-first",
        ],
    ),
    group_section(
        "Menus",
        "Pip-Boy Stats and Data",
        &[
            "tweak-pipboy-12-hour",
            "tweak-detailed-stats",
            "tweak-effect-time-left",
            "tweak-stats-xp-percent",
            "tweak-hide-empty-reputation",
            "tweak-hide-completed-quests",
            "tweak-hide-read-notes",
            "tweak-mark-notes-unread",
            "tweak-mark-recurring-challenges",
            "tweak-no-show-quest-location",
            "tweak-notes-of-all-quests",
            "tweak-quest-text-in-menus",
            "tweak-show-fame-infamy",
            "tweak-sort-misc-stats",
            "tweak-embolden-tags",
        ],
    ),
    group_section(
        "Menus",
        "Maps",
        &[
            "tweak-companions-location-font",
            "tweak-no-min-companion-distance",
            "tweak-map-door-kinds",
            "tweak-map-unconscious-companions",
            "tweak-local-map-reset-cells",
            "tweak-map-marker-distance",
            "tweak-map-recentre-key",
            "tweak-map-remembers",
            "tweak-map-zoom-at-pointer",
            "tweak-recent-markers-red",
            "tweak-no-marker-popup",
            "tweak-marker-removed-on-it",
            "tweak-marker-shift-removes",
            "tweak-hide-marker-reputation",
            "tweak-quiet-undiscovered",
        ],
    ),
    group_section(
        "Menus",
        "Containers and Barter",
        &[
            "tweak-barter-caps-change",
            "tweak-full-barter-total",
            "tweak-containers-show-ammo",
            "tweak-hide-hotkeyed-barter",
            "tweak-hide-hotkeyed-containers",
            "tweak-hide-equipped-barter",
            "tweak-hide-equipped-containers",
            "tweak-sync-categories",
            "tweak-sync-categories-keep-empty",
            "tweak-quantity-shows-price",
            "tweak-quantity-shows-weight",
            "tweak-quantity-companion-cap",
            "tweak-title-case-categories",
            "tweak-invert-title-wheel",
            "tweak-right-click-previous-category",
            "tweak-store-all",
            "tweak-take-all-confirm",
            "tweak-take-all-confirm-min",
            "tweak-take-all-stays",
            "tweak-warn-encumbrance",
        ],
    ),
    group_section(
        "Menus",
        "Repair and Crafting",
        &[
            "tweak-hide-hotkeyed-repair",
            "tweak-recipe-caps",
            "tweak-repair-all-confirm",
            "tweak-repair-all-threshold",
            "tweak-repair-preview",
            "tweak-repair-low-first",
            "tweak-repair-same-first",
            "tweak-repair-any-armor",
        ],
    ),
    group_section(
        "Menus",
        "Level-Up and Perks",
        &["tweak-levelup-wheel", "tweak-living-anatomy-dr", "tweak-hide-trait-ranks", "tweak-perk-plugin-names", "tweak-skip-skill-page", "tweak-preview-perks"],
    ),
    group_section(
        "Menus",
        "Terminals and Minigames",
        &[
            "tweak-blackjack-total",
            "tweak-grey-read-notes",
            "tweak-minigames-timescale",
            "tweak-lockpick-move-keys",
            "tweak-lockpick-mouse",
            "tweak-no-exit-hacking",
            "tweak-terminal-number-keys",
            "tweak-tab-closes-terminals",
        ],
    ),
    group_section("Menus", "Messages and Conversations", &["tweak-need-icons", "tweak-keep-acting-notes", "tweak-no-pointer-dialogue", "tweak-no-pointer-message-box"]),
    group_section("Menus", "Main and Pause Menus", &["tweak-continue-shows-save", "tweak-no-loading-tips", "tweak-quick-load-in-menus", "tweak-tab-back-in-pause"]),
    group_section(
        "Menus",
        "Keys and Mouse",
        &["tweak-key-repeat-accel", "tweak-key-repeat-delay", "tweak-key-repeat-mult", "tweak-key-repeat-delay-alt", "tweak-key-repeat-mult-alt", "tweak-wasd-as-arrow-keys", "tweak-horizontal-look"],
    ),
    // Tweaks > Hotkeys.
    group_section("Hotkeys", "Hide HUD Key", &["tweak-hide-hud-ammo", "tweak-hide-hud-needs", "tweak-hide-hud-compass", "tweak-hide-hud-on-load", "tweak-hide-hud-sneak"]),
    group_section("Hotkeys", "Other Keys", &["tweak-place-marker-here-says", "tweak-repeat-activate-takes-only"]),
    // Tweaks > Cheats.
    group_section("Cheats", "Weight", &["tweak-weightless-aid", "tweak-weightless-ammo", "tweak-weightless-armor", "tweak-weightless-misc", "tweak-weightless-weapons"]),
    group_section(
        "Cheats",
        "Hacking, Stealing and Casinos",
        &["tweak-hack-without-skill", "tweak-no-casino-bans", "tweak-no-max-casino-bet", "tweak-pickpocket-after-caught", "tweak-pickpocket-worn"],
    ),
    group_section(
        "Cheats",
        "Travel, Rest and Skills",
        &["tweak-console-homes", "tweak-fast-travel-enemies", "tweak-god-mode-free-crafting", "tweak-reallocate-skill-points", "tweak-sleep-in-owned-beds", "tweak-wait-anywhere"],
    ),
    // Tweaks > Misc.
    group_section(
        "Misc",
        "Sound and Radio",
        &[
            "tweak-level-sound-a",
            "tweak-level-sound-every-a",
            "tweak-level-sound-b",
            "tweak-level-sound-every-b",
            "tweak-discovered-sound",
            "tweak-armor-sounds-3d",
            "tweak-heartbeat-fade",
            "tweak-heartbeat-fade-seconds",
            "tweak-no-voice-filter",
            "tweak-quiet-hotkeys",
            "tweak-quiet-teammate-steps",
            "tweak-radiation-radio-static",
            "tweak-radio-music",
            "tweak-radio-talk",
        ],
    ),
    group_section(
        "Misc",
        "Camera",
        &[
            "tweak-kill-cam-hit-location",
            "tweak-kill-cam-outside-vats",
            "tweak-vats-stop-following-dead",
            "tweak-vanity-returns-first",
            "tweak-pov-follows-weapon",
            "tweak-camera-wheel-pov",
            "tweak-kill-cam-chance",
        ],
    ),
    group_section(
        "Misc",
        "Effects and Animation",
        &[
            "tweak-armor-blocks-blood",
            "tweak-scale-ash-piles",
            "tweak-detached-beams",
            "tweak-power-armor-no-screen-blood",
            "tweak-no-stealth-shimmer-pipboy",
            "tweak-no-turn-clip",
            "tweak-splash-torso-gib",
            "tweak-weapon-impacts",
        ],
    ),
    group_section(
        "Misc",
        "Controller and Keys",
        &[
            "tweak-custom-dead-zones",
            "tweak-left-dead-zone",
            "tweak-right-dead-zone",
            "tweak-invert-x",
            "tweak-keys-with-pad",
            "tweak-responsive-stick",
            "tweak-no-weapon-hotkeys",
            "tweak-pipboy-buzz",
            "tweak-duplicate-binds",
            "tweak-custom-trigger-dead-zones",
            "tweak-lt-dead-zone",
            "tweak-rt-dead-zone",
        ],
    ),
    group_section(
        "Misc",
        "Console",
        &[
            "tweak-better-flycam",
            "tweak-help-is-console",
            "tweak-print-plugin-names",
            "tweak-console-output-file",
            "tweak-console-model-details",
            "tweak-console-text-shadow",
            "tweak-console-timestamps",
            "tweak-console-compress-repeats",
            "tweak-remember-console-history",
        ],
    ),
    group_section("Misc", "Menus and Conversations", &["tweak-no-skip-dialogue", "tweak-pipboy-opens-items", "tweak-terminal-fade", "tweak-terminal-fade-seconds"]),
    group_section("Misc", "Starting and Loading", &["tweak-auto-continue", "tweak-god-mode-off-on-load", "tweak-no-respec", "tweak-no-name-limit", "tweak-click-to-exit-loading"]),
    group_section(
        "Misc",
        "World and Items",
        &[
            "tweak-no-travel-time",
            "tweak-improved-weather",
            "tweak-weather-trip-distance",
            "tweak-lethal-knockdowns",
            "tweak-no-selling",
            "tweak-no-skill-books-at-max",
            "tweak-no-ammo-casings",
            "tweak-pick-up-disarmed",
            "tweak-no-container-after-pick",
            "tweak-no-door-after-pick",
            "tweak-region-borders-message-only",
        ],
    ),
    section("Debug", None, &["overlay", "hit-markers", "god-mode", "navmesh", "terrain-wireframe", "scope-3d"]),
];

/// Rows that cannot apply while another row is off: (the row, the row it needs, why), the
/// row greyed with its value kept and the reason in its note while that row is off (a
/// switch off, a list on its first choice) [design: the menu overhaul, owner 2026-11-15:
/// "rows that can't apply: grey them out"]. `crate::settings::Settings::disabled_status`
/// reads it; a row of a missing module is passed over.
pub const NEEDS: &[(&str, &str, &str)] = &[
    ("dialogue-recentre", "dialogue-look", "Needs Turn to Each Other."),
    ("volume-music", "music", "Needs Play Music."),
    ("volume-voice", "voice", "Needs Dialogue Voice."),
    ("crosshair-style", "dynamic-crosshair", "Needs Dynamic Crosshair."),
    ("quick-loot-scale", "quick-loot", "Needs Quick Loot."),
    ("quick-loot-background", "quick-loot", "Needs Quick Loot."),
    ("quick-loot-weight", "quick-loot", "Needs Quick Loot."),
    ("quick-loot-value", "quick-loot", "Needs Quick Loot."),
    ("quick-loot-value-per-weight", "quick-loot", "Needs Quick Loot."),
    ("aim-slowdown", "aim-assist", "Needs Aim Assist."),
    ("aim-lock", "aim-assist", "Needs Aim Assist."),
    ("shadow-filter", "sun-shadows", "Needs Sun Shadows."),
    ("shadow-distance", "sun-shadows", "Needs Sun Shadows."),
    ("fine-shadows", "sun-shadows", "Needs Sun Shadows."),
    ("eye-adaptation", "glow", "Needs Light Glow (HDR)."),
    ("neon-glow", "glow", "Needs Light Glow (HDR)."),
    ("volumetric-light-strength", "volumetric-light", "Needs Volumetric Light."),
    ("grass-distance", "grass", "Needs Grass."),
    // #3322: the terrain parallax draws the authored height maps.
    ("terrain-parallax", "authored-materials", "Needs Authored Materials."),
    // #3014: the distant objects stand where the distant LOD's blocks leave gaps.
    ("distant-objects", "distant-lod", "Needs Distant terrain and objects."),
    ("tree-distance", "distant-lod", "Needs Distant terrain and objects."),
    // #2981: the reflections act only on authored materials, so the row greys while they are off.
    ("authored-reflections", "authored-materials", "Needs Authored materials."),
    ("fps-counter-fps", "fps-counter", "Needs the Frame Rate Counter."),
    ("fps-counter-graph", "fps-counter", "Needs the Frame Rate Counter."),
    ("fps-counter-split", "fps-counter", "Needs the Frame Rate Counter."),
    ("fps-counter-cpu", "fps-counter", "Needs the Frame Rate Counter."),
    ("fps-counter-gpu", "fps-counter", "Needs the Frame Rate Counter."),
    ("fps-counter-vram", "fps-counter", "Needs the Frame Rate Counter."),
    ("fps-counter-ram", "fps-counter", "Needs the Frame Rate Counter."),
    ("fps-counter-loaded", "fps-counter", "Needs the Frame Rate Counter."),
    ("fps-counter-upscaling", "fps-counter", "Needs the Frame Rate Counter."),
    ("fps-counter-low", "fps-counter", "Needs the Frame Rate Counter."),
    ("fps-counter-low", "fps-counter-fps", "Needs Counter: Frame Rate."),
    ("tweak-hack-keep-matching-chars", "tweak-hack-mark-guesses-as-duds", "Needs Hacking: Mark Guesses as Duds."),
    ("tweak-breath-activate", "tweak-hold-breath", "Needs Hold Breath Through a Scope."),
    ("tweak-sync-categories-keep-empty", "tweak-sync-categories", "Needs Matching Categories."),
    ("tweak-revive-on-cell-change", "tweak-revive-companions", "Needs Revive Unconscious Companions."),
    ("tweak-no-self-explosion-non-hostiles", "tweak-no-self-explosion-damage", "Needs Own Blasts Spare the Player."),
    ("tweak-grabbing-is-theft-min-value", "tweak-grabbing-is-theft", "Needs Grabbing Is Stealing."),
    ("tweak-marker-name-units", "tweak-hud-marker-name", "Needs Place Name on the HUD."),
    ("dialogue-choice-text-size", "touch-dialogue", "Needs Touch dialogue."),
    ("dialogue-subtitle-text-size", "touch-dialogue", "Needs Touch dialogue."),
    ("dialogue-touch-choice", "touch-dialogue", "Needs Touch dialogue."),
    ("dialogue-theme-v1", "touch-dialogue", "Needs Touch dialogue."),
    ("dialogue-speaker-side", "touch-dialogue", "Needs Touch dialogue."),
    ("dialogue-device-side-v1", "touch-dialogue", "Needs Touch dialogue."),
    ("dialogue-background-opacity-v1", "touch-dialogue", "Needs Touch dialogue."),
    ("dialogue-background-tint-v1", "touch-dialogue", "Needs Touch dialogue."),
    ("dialogue-frame-opacity-v1", "touch-dialogue", "Needs Touch dialogue."),
    ("dialogue-hints-v1", "touch-dialogue", "Needs Touch dialogue."),
    ("touch-gyro-toggle-v1", "touch-gyro-v1", "Needs Gyroscope."),
    ("touch-gyro-ads-only-v1", "touch-gyro-v1", "Needs Gyroscope."),
    ("touch-gyro-recenter-v1", "touch-gyro-v1", "Needs Gyroscope."),
    ("touch-gyro-invert-x-v1", "touch-gyro-v1", "Needs Gyroscope."),
    ("touch-gyro-invert-y-v1", "touch-gyro-v1", "Needs Gyroscope."),
    ("touch-gyro-sensitivity-v1", "touch-gyro-v1", "Needs Gyroscope."),
    ("touch-gyro-ads-v1", "touch-gyro-v1", "Needs Gyroscope."),
    ("touch-hotbar-slots", "touch-hotbar", "Needs Touch hotbar."),
    ("touch-hotbar-size", "touch-hotbar", "Needs Touch hotbar."),
    ("touch-hotbar-tray", "touch-hotbar", "Needs Touch hotbar."),
    ("touch-pipboy-distance-v1", "touch-pipboy-swipe-v1", "Needs Bottom swipe handle."),
    ("touch-direct-pipboy-v1", "touch-pipboy-swipe-v1", "Needs Bottom swipe handle."),
    ("touch-pipboy-dismiss-travel-v1", "touch-pipboy-dismiss-side-v2", "Needs Pip-Boy downward dismissal."),
    ("lockpick-touch-hints", "lockpick-touch", "Needs Touch lockpicking."),
    ("lockpick-touch-size", "lockpick-touch", "Needs Touch lockpicking."),
];

/// The Controls tab's rows that are shown only while the input device is a controller
/// (`crate::bindings::InputDevice`) [design: #1953].
pub const CONTROLLER_KEYS: [&str; 8] = ["button-style", "aim-assist", "aim-slowdown", "aim-lock",
    ANDROID_CONTROLLER_KEYS[0], ANDROID_CONTROLLER_KEYS[1], ANDROID_CONTROLLER_KEYS[2], ANDROID_CONTROLLER_KEYS[3]];

/// Whether row `key` is shown only with a controller ([`CONTROLLER_KEYS`]).
pub fn controller_only(key: &str) -> bool {
    CONTROLLER_KEYS.contains(&key)
}

/// The Controls tab's Input Device row, as a rule names it ([`Rule::parent`]): kept as
/// `[Controls] Input Device`, not in `[Settings]`.
pub const DEVICE_ROW: &str = "controls:device";

/// The input device [`CONTROLLER_KEYS`] need, by the name `[Controls] Input Device` keeps.
pub const CONTROLLER_DEVICE: &str = "Controller";

/// Rows shown only while a list row has one of the named choices: (the row, the list row,
/// the choices by name) [design: the menu overhaul, owner 2026-11-15: the upscaler's own rows
/// and Generated Frames are hidden, not greyed, while they cannot apply]. While hidden, the
/// list row stands for the row (the search, the Changed list). The game's page
/// (`nvf_play::settings::Settings::hidden`) and the launcher both go by this table: a row
/// hidden by another row's choice is written here, not in code, so the two stay one.
pub const SHOWN_WITH: &[(&str, &str, &[&str])] = &[
    ("upscaler-quality", "upscaler", &["FSR 3.1", "DLSS"]),
    ("dlss-model", "upscaler", &["DLSS"]),
    ("fsr-library", "upscaler", &["FSR 3.1"]),
    ("anti-aliasing", "upscaler", &["Off"]),
    ("frame-generation-frames", "frame-generation", &["DLSS"]),
];

/// Rows greyed unless a list row has one of the named choices: (the row, the list row, the
/// choices by name, why) [design: the menu overhaul], as [`NEEDS`] for a row that needs a
/// choice rather than a row switched on. The game's page
/// (`nvf_play::settings::Settings::disabled_status`) and the launcher both go by it.
pub const NEEDS_CHOICE: &[(&str, &str, &[&str], &str)] = &[
    ("vats-slowdown", "vats-style", &["Classic"], "Classic V.A.T.S. only."),
    // The Custom difficulty's own three, greyed under the five levels [design: #3570
    // review, maintainer decision 2026-11-24: greyed, not hidden].
    ("difficulty-player-damage", "difficulty", &["Custom"], "Custom difficulty only."),
    ("difficulty-npc-damage", "difficulty", &["Custom"], "Custom difficulty only."),
    ("difficulty-kill-xp", "difficulty", &["Custom"], "Custom difficulty only."),
    ("dialogue-speaker-side", "dialogue-theme-v1", &["Field Notes", "Retro Console", "Wrist Terminal"], "Available with Field Notes, Retro Console and Wrist Terminal."),
    ("dialogue-device-side-v1", "dialogue-theme-v1", &["Retro Console", "Wrist Terminal"], "Available with Retro Console and Wrist Terminal."),
    ("dialogue-background-opacity-v1", "dialogue-theme-v1", &["Field Notes"], "Available with the Field Notes theme."),
    ("dialogue-background-tint-v1", "dialogue-theme-v1", &["Field Notes"], "Available with the Field Notes theme."),
    ("dialogue-frame-opacity-v1", "dialogue-theme-v1", &["Field Notes"], "Available with the Field Notes theme."),
    ("dialogue-hints-v1", "dialogue-theme-v1", &["Field Notes"], "Available with the Field Notes theme."),
    ("coop-router-port", "coop-connection", &["Automatic", "Direct only"], "Not with LAN only."),
    ("coop-relay", "coop-connection", &["Automatic", "Direct only"], "Not with LAN only."),
];

/// What a row needs of another row to apply: `parent` on (a switch on, a list past its first
/// choice) when `choices` is empty, else `parent` on one of `choices` (by name; a switch's
/// choices are "On" and "Off"). `why` is
/// what the page says while it does not hold (English; the game shows it translated).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rule {
    pub parent: &'static str,
    pub choices: &'static [&'static str],
    pub why: &'static str,
}

/// The rule that hides row `key` while it does not hold: [`SHOWN_WITH`], or a controller
/// row's need of the controller ([`CONTROLLER_KEYS`], parent [`DEVICE_ROW`]).
pub fn shown_with(key: &str) -> Option<Rule> {
    if controller_only(key) {
        return Some(Rule { parent: DEVICE_ROW, choices: &[CONTROLLER_DEVICE], why: "" });
    }
    SHOWN_WITH.iter().find(|(k, _, _)| *k == key).map(|(_, parent, choices)| Rule { parent, choices, why: "" })
}

/// The rules that grey row `key` while one does not hold, in the page's order ([`NEEDS`],
/// then [`NEEDS_CHOICE`]); the page names the first that does not.
pub fn needs(key: &str) -> Vec<Rule> {
    let off = NEEDS.iter().filter(|(k, _, _)| *k == key).map(|(_, parent, why)| Rule { parent, choices: &[], why });
    let choice = NEEDS_CHOICE.iter().filter(|(k, _, _, _)| *k == key).map(|(_, parent, choices, why)| Rule { parent, choices, why });
    off.chain(choice).collect()
}

/// A stamp of this table (its tabs, every category and row in order, the rules): 16 hex
/// digits, the same in every build of the same table and different once a row moves, is
/// renamed, added or dropped or a rule changes. The game writes it in its menu file
/// ([`menu::Menu::layout`]) and the launcher takes the file only while it is its own, as dev
/// builds between releases share a version [design: #2830 review: the version alone let a
/// launcher show an older build's rows].
pub fn layout_stamp() -> String {
    stamp_of(SECTIONS)
}

/// The Android layout has its own identity, including its Mobile placements.
pub fn mobile_layout_stamp() -> String {
    stamp(&MOBILE_TAB_ORDER, MOBILE_SECTIONS.iter().chain(SECTIONS).chain(ANDROID_SECTIONS))
}

/// [`layout_stamp`] with `sections` for [`SECTIONS`].
pub(crate) fn stamp_of(sections: &[Section]) -> String {
    stamp(&TAB_ORDER, sections.iter())
}

fn stamp<'a>(tabs: &[&str], sections: impl Iterator<Item = &'a Section>) -> String {
    // FNV-1a, 64 bits: fixed by its definition, unlike std's hasher, so every build and
    // platform agrees; the separators keep `["ab"]` and `["a", "b"]` apart.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut put = |s: &str, end: u8| {
        for b in s.bytes().chain([end]) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for t in tabs {
        put(t, 0x1f);
    }
    for s in sections {
        let p = s.placement;
        put(p.tab, 0x1f);
        put(p.subgroup.unwrap_or(""), 0x1f);
        put(p.heading.unwrap_or(""), 0x1f);
        for k in s.keys {
            put(k, 0x1f);
        }
        put("", 0x1e);
    }
    for (row, parent, why) in NEEDS {
        put(&format!("{row}\u{1f}{parent}\u{1f}{why}"), 0x1e);
    }
    for (row, parent, choices) in SHOWN_WITH {
        put(&format!("{row}\u{1f}{parent}\u{1f}{}", choices.join("\u{1f}")), 0x1e);
    }
    for (row, parent, choices, why) in NEEDS_CHOICE {
        put(&format!("{row}\u{1f}{parent}\u{1f}{}\u{1f}{why}", choices.join("\u{1f}")), 0x1e);
    }
    for k in CONTROLLER_KEYS {
        put(k, 0x1f);
    }
    format!("{h:016x}")
}

/// The table's faults: a key named twice, a tab not in [`TAB_ORDER`], a group outside the
/// Tweaks tab, a category without a heading on a tab with categories, a key in NVF Features
/// the owner did not name.
pub fn check(sections: &[Section]) -> Result<(), String> {
    let mut seen: Vec<&str> = Vec::new();
    for s in sections {
        if !TAB_ORDER.contains(&s.placement.tab) {
            return Err(format!("{}: not one of the tabs", s.placement.tab));
        }
        if s.placement.subgroup.is_some() && s.placement.tab != TWEAKS_TAB {
            return Err(format!("{}: a group is for the Tweaks tab only", s.placement.path()));
        }
        if s.keys.is_empty() {
            return Err(format!("{}: an empty category", s.placement.path()));
        }
        for k in s.keys {
            if seen.contains(k) {
                return Err(format!("{k}: placed twice"));
            }
            if s.placement.tab == NVF_FEATURES_TAB && !NVF_FEATURES_KEYS.contains(k) {
                return Err(format!("{k}: NVF Features holds only what the owner named there"));
            }
            if k.starts_with(CONTROL_PREFIX) && s.placement.tab != CONTROLS_TAB {
                return Err(format!("{k}: a Controls row outside the Controls tab"));
            }
            seen.push(k);
        }
    }
    for tab in TAB_ORDER {
        let of_tab: Vec<&Section> = sections.iter().filter(|s| s.placement.tab == tab).collect();
        if of_tab.len() > 1 && of_tab.iter().any(|s| s.placement.heading.is_none()) {
            return Err(format!("{tab}: every category of a tab with several needs a heading"));
        }
    }
    Ok(())
}
