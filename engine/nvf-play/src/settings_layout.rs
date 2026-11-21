//! Where the settings page shows each row: its tab, and the category (a heading) it stands
//! under in that tab (#1953, the menu overhaul; docs/design/settings-page.md). Presentation
//! only: a row keeps the key, the note, the default and the group it registered with
//! (`crate::settings`), and the registry keeps its order, which the values, the INI and the
//! test aids go by. This table only says where the page draws it [design].
//!
//! **Eight tabs, the original's four categories first** [design: #1953; the original's
//! Settings opens Gameplay, Display, Audio and Controls, observed in the original's pause
//! menu, 2026-11-09; the owner's menu overhaul, 2026-11-15]: Gameplay, Display, Audio,
//! Controls, then Graphics, NVF Features, Tweaks and Debug ([`TAB_ORDER`]). Each tab lists
//! its [`SECTIONS`] in order, each a **category**: a heading the player can open and close,
//! an empty line after each open one ([`ResolvedLayout::tab_entries`]).
//!
//! **The Controls tab's own rows** (`crate::settings_view::ControlRow`: the input device,
//! each control's key or button, Invert Y, the sensitivities, Rumble) are named here too, by
//! [`CONTROL_PREFIX`] and their name (`controls:device`, `controls:bind:Jump`), beside the
//! registered rows of the same category (Button Style under Input) [design: the overhaul].
//!
//! **NVF Features** holds only what the owner names (`NVF_FEATURES_KEYS`, a test keeps
//! it so) [design: owner 2026-11-15: "do not move any feature there without my say so"].
//!
//! **A row the table does not name** stays under the group it registered with, used as its
//! tab with no heading, after the tab's named rows, in registry order; a group that is
//! none of the tabs becomes a tab of its own after Debug ([`resolve_layout`]). The engine's
//! own rows are all named (a test keeps it so); the fallback is for what a later change
//! adds before it places its row.
//!
//! [`Placement::subgroup`] is the Tweaks tab's group (Gameplay, Combat, … as Stewie's
//! Tweaks sorts them): the tab shows one group at a time, chosen by its Category row
//! (`crate::settings_view`), its categories closed until opened [design: owner 2026-11-15].
//! Every row #1892 registers (`tweak-<key>`) has its group and category here already
//! ([`group_section`]; the list for the owner's review: docs/design/settings-tweaks-layout.md):
//! the group the plugin's catalogue gives its INI key, and in a category the lists first,
//! then the switches, then the sliders, each by label, a row that only matters with
//! another right after that one [design: owner 2026-11-15].
//!
//! What a row's visibility is (the colour sliders only for Custom, the controller rows only
//! with a controller, a row its module hides) is not here: the page decides it each frame
//! (`crate::settings_view`), so a hidden row keeps its tab and category.

// The table itself (the tabs, categories, row order, NEEDS) is in crates/nvf-settings-layout,
// shared with the launcher so its Settings page has the game's menus [design: owner
// 2026-11-16]; this module applies it to the rows a run registers.
pub use nvf_settings_layout::*;

use crate::settings::Setting;


/// One line of a section: a registered row (its index in the registry) or a Controls row
/// (its name after [`CONTROL_PREFIX`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutItem {
    Setting(usize),
    Control(&'static str),
}

/// A section of the rows: its place and its lines in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedSection {
    pub placement: Placement,
    /// Its registered rows (indices in the registry), in order.
    pub rows: Vec<usize>,
    /// Its lines in order: the registered rows and the Controls rows among them.
    pub items: Vec<LayoutItem>,
}

/// A tab and its sections.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedTab {
    pub name: &'static str,
    pub sections: Vec<ResolvedSection>,
}

/// The Tweaks tab's group for rows the table does not place in one yet (a tweak added after
/// the table was written): listed last, so such a row is never out of sight [design].
pub const UNGROUPED: &str = "Other";

impl ResolvedTab {
    /// The Tweaks tab's groups, in the table's order, and [`UNGROUPED`] last while a row of
    /// the tab has none (no groups on another tab).
    pub fn subgroups(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for g in self.sections.iter().filter_map(|s| s.placement.subgroup) {
            if !out.contains(&g) {
                out.push(g);
            }
        }
        if !out.is_empty() && self.sections.iter().any(|s| s.placement.subgroup.is_none()) {
            out.push(UNGROUPED);
        }
        out
    }
}

/// One line of a tab as the table gives it: a category's heading, a registered row, a
/// Controls row, or the empty line after an open category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutEntry {
    Heading(Placement),
    Setting(usize),
    Control(&'static str),
    Gap,
}

impl From<LayoutItem> for LayoutEntry {
    fn from(i: LayoutItem) -> LayoutEntry {
        match i {
            LayoutItem::Setting(i) => LayoutEntry::Setting(i),
            LayoutItem::Control(c) => LayoutEntry::Control(c),
        }
    }
}

/// The table applied to the registered rows ([`resolve_layout`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolvedLayout {
    pub tabs: Vec<ResolvedTab>,
    /// Each registered row's place, by its index.
    placements: Vec<Placement>,
    /// The rows the table does not name, the Tweaks tab's left out
    /// ([`ResolvedLayout::unnamed`]).
    unnamed: usize,
}

/// The table applied to `list`, the registry in its order (module notes): the tabs (an
/// empty one too), each named section with the rows registered and its Controls rows (a
/// missing optional row left out, a section left with none dropped), then each tab's
/// unnamed rows, then the unnamed groups' tabs. Every registered row is in it once.
pub fn resolve_layout(list: &[Setting]) -> ResolvedLayout {
    let index: std::collections::HashMap<&str, usize> = list.iter().enumerate().map(|(i, s)| (s.key, i)).collect();
    let mut placements: Vec<Option<Placement>> = vec![None; list.len()];
    let mut tabs: Vec<ResolvedTab> = TAB_ORDER.iter().map(|&name| ResolvedTab { name, sections: Vec::new() }).collect();
    for s in SECTIONS {
        let mut items = Vec::new();
        for k in s.keys {
            if let Some(c) = k.strip_prefix(CONTROL_PREFIX) {
                items.push(LayoutItem::Control(c));
            } else if let Some(&i) = index.get(k).filter(|&&i| placements[i].is_none()) {
                placements[i] = Some(s.placement);
                items.push(LayoutItem::Setting(i));
            }
        }
        if items.is_empty() {
            continue;
        }
        let rows = items.iter().filter_map(|i| if let LayoutItem::Setting(i) = i { Some(*i) } else { None }).collect();
        if let Some(t) = tabs.iter_mut().find(|t| t.name == s.placement.tab) {
            t.sections.push(ResolvedSection { placement: s.placement, rows, items });
        }
    }
    // The unnamed rows, by tab, in registry order: one section after each tab's named ones.
    let mut unnamed: Vec<(usize, Vec<usize>)> = Vec::new();
    for (i, s) in list.iter().enumerate() {
        if placements[i].is_some() {
            continue;
        }
        placements[i] = Some(Placement::tab(s.group));
        let t = match tabs.iter().position(|t| t.name == s.group) {
            Some(t) => t,
            None => {
                tabs.push(ResolvedTab { name: s.group, sections: Vec::new() });
                tabs.len() - 1
            }
        };
        match unnamed.iter_mut().find(|(at, _)| *at == t) {
            Some((_, rows)) => rows.push(i),
            None => unnamed.push((t, vec![i])),
        }
    }
    // The Tweaks tab's rows are not counted: #1892 registers them under Tweaks without
    // naming them here, by design (module notes), so the count stays the rows another
    // tab's table left out [design: #1953, #1892].
    let count = unnamed.iter().filter(|(t, _)| tabs[*t].name != TWEAKS_TAB).map(|(_, r)| r.len()).sum();
    for (t, rows) in unnamed {
        let placement = Placement::tab(tabs[t].name);
        let items = rows.iter().map(|&i| LayoutItem::Setting(i)).collect();
        tabs[t].sections.push(ResolvedSection { placement, rows, items });
    }
    ResolvedLayout { tabs, placements: placements.into_iter().map(|p| p.unwrap_or(Placement::tab(""))).collect(), unnamed: count }
}

impl ResolvedLayout {
    /// The tabs' names, in order.
    pub fn tab_names(&self) -> Vec<&'static str> {
        self.tabs.iter().map(|t| t.name).collect()
    }

    pub fn tab(&self, name: &str) -> Option<&ResolvedTab> {
        self.tabs.iter().find(|t| t.name == name)
    }

    /// Registered row `i`'s place.
    pub fn placement_of(&self, i: usize) -> Option<Placement> {
        self.placements.get(i).copied()
    }

    /// The place of the Controls row named `name` (after [`CONTROL_PREFIX`]).
    pub fn control_placement(&self, name: &str) -> Option<Placement> {
        self.tabs.iter().flat_map(|t| &t.sections).find(|s| s.items.iter().any(|i| matches!(i, LayoutItem::Control(c) if *c == name))).map(|s| s.placement)
    }

    /// How many registered rows the table does not name, leaving out the Tweaks tab's
    /// ([`TWEAKS_TAB`]), which #1892 lists in registry order without naming them here
    /// [design: #1953, #1892].
    pub fn unnamed(&self) -> usize {
        self.unnamed
    }

    /// Every registered row in the page's order: tab by tab, section by section (what
    /// search and the Changed list go by).
    pub fn ordered_settings(&self) -> impl Iterator<Item = usize> + '_ {
        self.tabs.iter().flat_map(|t| t.sections.iter().flat_map(|s| s.rows.iter().copied()))
    }

    /// The section at `p`.
    pub fn section(&self, p: &Placement) -> Option<&ResolvedSection> {
        self.tab(p.tab)?.sections.iter().find(|s| s.placement == *p)
    }

    /// Tab `name`'s lines (of its group `subgroup` on the Tweaks tab; `None`: every
    /// section): each category with a line `visible` says is shown, its heading (when it has
    /// one) and, while `open` says it is open, those lines; after each open category with
    /// another after it, an empty line [design: the overhaul]. A category whose lines are all
    /// hidden is left out, heading too; a hidden line keeps its place for when it shows.
    pub fn tab_entries(&self, name: &str, subgroup: Option<&str>, visible: impl Fn(LayoutItem) -> bool, open: impl Fn(&Placement) -> bool) -> Vec<LayoutEntry> {
        let mut out = Vec::new();
        let Some(t) = self.tab(name) else { return out };
        let mut after_open = false;
        let of_group = |s: &&ResolvedSection| match subgroup {
            None => true,
            Some(UNGROUPED) => s.placement.subgroup.is_none(),
            g => s.placement.subgroup == g,
        };
        for s in t.sections.iter().filter(of_group) {
            let items: Vec<LayoutItem> = s.items.iter().copied().filter(|&i| visible(i)).collect();
            if items.is_empty() {
                continue;
            }
            if after_open {
                out.push(LayoutEntry::Gap);
            }
            let headed = s.placement.heading.is_some();
            let shows = !headed || open(&s.placement);
            if headed {
                out.push(LayoutEntry::Heading(s.placement));
            }
            if shows {
                out.extend(items.into_iter().map(LayoutEntry::from));
            }
            after_open = shows;
        }
        out
    }

    /// How many of the category at `p`'s lines `visible` says are shown (what its heading
    /// counts while chosen).
    pub fn shown_count(&self, p: &Placement, visible: impl Fn(LayoutItem) -> bool) -> usize {
        self.section(p).map_or(0, |s| s.items.iter().filter(|&&i| visible(i)).count())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    fn row(key: &'static str, group: &'static str) -> Setting {
        Setting { key, group, label: key, note: "", switches: "", get: |_| false, set: |_, _| {}, slider: None }
    }

    /// The registry as `Settings::add` orders it.
    fn registry(rows: &[(&'static str, &'static str)]) -> Vec<Setting> {
        let mut s = Settings::default();
        for (k, g) in rows {
            s.add(row(k, g));
        }
        s.list().to_vec()
    }

    /// A tab's lines, every category open, every line shown: `[Heading]`, keys, `@control`,
    /// and "" for an empty line.
    fn keys(list: &[Setting], l: &ResolvedLayout, tab: &str) -> Vec<String> {
        lines(list, l, tab, |_| true)
    }

    fn lines(list: &[Setting], l: &ResolvedLayout, tab: &str, open: impl Fn(&Placement) -> bool) -> Vec<String> {
        l.tab_entries(tab, None, |_| true, open)
            .into_iter()
            .map(|e| match e {
                LayoutEntry::Heading(p) => format!("[{}]", p.heading.unwrap_or("")),
                LayoutEntry::Setting(i) => list[i].key.to_string(),
                LayoutEntry::Control(c) => format!("@{c}"),
                LayoutEntry::Gap => String::new(),
            })
            .collect()
    }

    /// Every row the registry can hold today (upstream main be6db0948: #1966's quality row
    /// (its enhanced row removed by the menu overhaul), #2014's counter rows, the VATS rows,
    /// #2198's upscaler rows, #2169's FSR library row, #2242's upscaling counter row, #2317's
    /// Generated frames row, the field of view rows, the menu scale and Physics rate rows,
    /// #2662's grass fires, #2550's terrain height blending, #2551's terrain anti-tiling,
    /// #2688's sound cover, #2670's cloud shadows, #2671's terrain shadows, #2149's smooth terrain and its Debug row terrain wireframe, the
    /// overhaul's menu font, the Advanced Movement rows of tech/movement-inertia, Weapon
    /// Inertia and Camera Motion in place of weapon sway and head bob, #2843's Shadow
    /// resolution row, Gore Shake and #2670's Cloud shadows; #2661's mesh terrain blend row, the
    /// Quick Loot rows; #2965's heat haze row; #2675's desert haze row; Sprint; #2671's terrain
    /// shadows row), by the group it registers with.
    const KNOWN: [(&str, &str); 152] = [
        ("modern-feel", "Gameplay"),
        ("feel-hit-reactions", "Gameplay"),
        ("feel-camera-kick", "Gameplay"),
        ("feel-damage-direction", "Gameplay"),
        ("feel-gun-audio", "Gameplay"),
        ("feel-sound-occlusion", "Gameplay"),
        ("feel-shell-casings", "Gameplay"),
        ("feel-impact-effects", "Gameplay"),
        ("feel-grounded-footsteps", "Gameplay"),
        ("feel-npc-aim", "Gameplay"),
        ("feel-volumetric-fog", "Gameplay"),
        ("feel-motion-blur", "Gameplay"),
        ("detached-camera", "Gameplay"),
        ("gore-shake", "Gameplay"),
        ("advanced-movement", "Gameplay"),
        ("movement-weight", "Gameplay"),
        ("movement-inertia", "Gameplay"),
        ("weapon-handling", "Gameplay"),
        ("advanced-recoil", "Gameplay"),
        ("sprint", "Gameplay"),
        ("animation-smoothing", "Gameplay"),
        ("sound-cover", "Gameplay"),
        ("menu-pause", "Gameplay"),
        ("dialogue-goodbye-hint", "Gameplay"),
        ("reload-slows", "Gameplay"),
        ("grass-fire", "Gameplay"),
        ("quick-loot", "Gameplay"),
        ("quick-loot-scale", "Gameplay"),
        ("quick-loot-background", "Gameplay"),
        ("quick-loot-weight", "Gameplay"),
        ("quick-loot-value", "Gameplay"),
        ("quick-loot-value-per-weight", "Gameplay"),
        ("music", "Gameplay"),
        ("voice", "Gameplay"),
        ("dialogue-look", "Gameplay"),
        ("dialogue-recentre", "Gameplay"),
        ("dialogue-zoom", "Gameplay"),
        ("dynamic-crosshair", "Gameplay"),
        ("crosshair-style", "Gameplay"),
        ("sneak-gauges", "Gameplay"),
        ("UsePipBoy", "Gameplay"),
        ("kill-camera", "Gameplay"),
        ("vats-style", "Gameplay"),
        ("vats-slowdown", "Gameplay"),
        ("difficulty", "Gameplay"),
        ("hardcore", "Gameplay"),
        ("lip-sync", "Gameplay"),
        ("true-iron-sights", "Gameplay"),
        ("scope-3d", "Gameplay"),
        ("button-style", "Controller"),
        ("aim-assist", "Controller"),
        ("aim-slowdown", "Controller"),
        ("aim-lock", "Controller"),
        ("autosave-travel", "Saving"),
        ("autosave-fast-travel", "Saving"),
        ("autosave-wait", "Saving"),
        ("autosave-sleep", "Saving"),
        ("autosave-scripts", "Saving"),
        ("hud-colour", "Interface"),
        ("hud-red", "Interface"),
        ("hud-green", "Interface"),
        ("hud-blue", "Interface"),
        ("pipboy-colour", "Interface"),
        ("pipboy-red", "Interface"),
        ("pipboy-green", "Interface"),
        ("pipboy-blue", "Interface"),
        ("language", "Interface"),
        ("menu-font", "Interface"),
        ("head-tracking", "World"),
        ("foot-ik", "World"),
        ("sky-clock", "World"),
        ("resolution", "Display"),
        ("borderless", "Display"),
        ("monitor", "Display"),
        ("first-person-body", "Display"),
        ("weapon-inertia", "Display"),
        ("camera-motion", "Display"),
        ("weapon-lower", "Display"),
        ("sun-shadows", "Display"),
        ("grass", "Display"),
        ("distant-lod", "Display"),
        ("grade", "Display"),
        ("field-of-view", "Display"),
        ("viewmodel-field-of-view", "Display"),
        ("ui-scale", "Display"),
        ("menu-scale", "Display"),
        ("fps-counter", "Display"),
        ("fps-counter-fps", "Display"),
        ("fps-counter-low", "Display"),
        ("fps-counter-graph", "Display"),
        ("fps-counter-split", "Display"),
        ("fps-counter-cpu", "Display"),
        ("fps-counter-gpu", "Display"),
        ("fps-counter-vram", "Display"),
        ("fps-counter-ram", "Display"),
        ("fps-counter-loaded", "Display"),
        ("fps-counter-upscaling", "Display"),
        ("display-sync", "Display"),
        ("frame-cap", "Display"),
        ("physics-rate", "Display"),
        ("object-distance", "Display"),
        ("actor-distance", "Display"),
        ("item-distance", "Display"),
        ("shadow-distance", "Display"),
        ("grass-distance", "Display"),
        ("tree-distance", "Display"),
        ("upscaler", "Graphics"),
        ("upscaler-quality", "Graphics"),
        ("dlss-model", "Graphics"),
        ("fsr-library", "Graphics"),
        ("frame-generation", "Graphics"),
        ("frame-generation-frames", "Graphics"),
        ("anti-aliasing", "Graphics"),
        ("latency-reduction", "Graphics"),
        ("shadow-filter", "Graphics"),
        ("shadow-resolution", "Graphics"),
        ("ambient-occlusion", "Graphics"),
        ("fine-shadows", "Graphics"),
        ("cloud-shadows", "Graphics"),
        ("terrain-shadows", "Graphics"),
        ("bloom", "Graphics"),
        ("glow", "Graphics"),
        ("eye-adaptation", "Graphics"),
        ("neon-glow", "Graphics"),
        ("sharpen", "Graphics"),
        ("anisotropic", "Graphics"),
        ("terrain-height-blend", "Graphics"),
        ("terrain-anti-tiling", "Graphics"),
        ("mesh-terrain-blend", "Graphics"),
        ("smooth-terrain", "Graphics"),
        ("light-shafts", "Graphics"),
        ("heat-haze", "Graphics"),
        ("desert-haze", "Graphics"),
        ("water-reflections", "Graphics"),
        ("water-effects", "Graphics"),
        ("water-mirror", "Graphics"),
        ("quality", "Graphics"),
        ("volume-master", "Sound"),
        ("volume-music", "Sound"),
        ("volume-footstep", "Sound"),
        ("volume-voice", "Sound"),
        ("volume-effects", "Sound"),
        ("volume-radio", "Sound"),
        ("reverb", "Sound"),
        ("voice-reverb", "Sound"),
        ("overlay", "Debug"),
        ("hit-markers", "Debug"),
        ("god-mode", "Debug"),
        ("navmesh", "Debug"),
        ("terrain-wireframe", "Debug"),
        ("send-reports", "Reports"),
        ("reports-delete", "Reports"),
    ];

    /// What every tweak's key starts with (#1892's `tweaks.rs`: `concat!("tweak-", $key)`).
    const TWEAK_PREFIX: &str = "tweak-";

    /// Every row #1892 registers under Tweaks (PR head 25bc53a28, `engine/nvf-play/src/tweaks.rs`),
    /// in its order, without [`TWEAK_PREFIX`]: what the table places ahead of it.
    const TWEAK_ROWS: &str = "\
        discovered-sound level-sound-every-a level-sound-a level-sound-every-b level-sound-b xp-multiplier \
        no-xp-messages no-karma-messages no-quest-failed no-location-popup no-objective-popup disable-fast-travel \
        midair-fast-travel fast-travel-enemies fast-travel-encumbered no-red-crosshair hide-enemy-markers \
        no-sneak-label no-reputation-messages no-combat-music no-loading-tips no-explosion-blur no-chem-wore-off \
        kill-cam-exit-key skip-skill-page no-empty-book-message recipe-caps weapon-ammo-use book-effects \
        deselect-hotkeys map-remembers terminal-number-keys faster-hacking dialogue-music dialogue-radio \
        console-skips-player holster-real-time power-armor-no-screen-blood no-red-crosshair-cloaked \
        god-mode-quiet-landing console-compress-repeats console-timestamps living-anatomy-dr sync-categories \
        perks-per-level special-points pick-without-skill hack-without-skill no-terminal-lockout no-casino-bans \
        loot-unconscious revive-companions revive-down-seconds pickpocket-worn no-sneak-criticals reload-when-empty \
        no-travel-time power-armor-fall weightless-armor weightless-aid weightless-ammo weightless-weapons \
        weightless-misc weightless-worn-armor show-poison-effects no-poison-confirm map-summon-companions \
        summon-unconscious no-min-companion-distance map-unconscious-companions companions-location-font \
        map-door-kinds hostiles-prefer-player prefer-player-start prefer-player-mid prefer-player-detection \
        equip-broken-items keep-broken-equipped hardcore-tweaks hardcore-ammo-weight hardcore-essential-companions \
        hardcore-sleep-heals weightless-worn-power-armor no-travel-crippled-leg wait-only-seated kill-reward-ap \
        jump-ap-cost run-ap-cost no-selling custom-skill-points skill-point-base skill-points-per-int \
        no-unconscious-message no-voice-filter no-skip-dialogue lockpick-mouse no-max-casino-bet no-aiming-slowdown \
        repeat-jumping hold-to-sneak vats-entry-ap power-attack-ap quest-marker-reach scope-zoom-time drop-key \
        no-container-after-pick no-door-after-pick mark-unvisited-doors no-exit-hacking no-grab-owned agility-speed \
        auto-unlock-terminals auto-unlock-margin weapons-unhurt-by-hits god-mode-no-jams knockdown-by-strength \
        drowning-drains-ap aiming-draws-weapon walk-angle crippled-leg-jump aid-at-full-health grabbing-is-theft \
        vendor-lists key-name no-skill-tags no-npc-addiction pickpocket-fleeing full-reload-swaps melee-reach \
        repair-xp repair-caps-container no-wear-on-dead encumbered-ap-regen light-step-companions no-player-stagger \
        ready-does-not-draw jam-reload-pace no-hotkeys-reloading wading-pace sleep-on-chairs stealing-no-alarm \
        no-companion-kill-xp no-disarm-companions no-self-explosion-damage sneak-criticals-melee teammates-meltdown \
        speed-scales-jump no-tutorials no-knockdown-god show-current-skill min-splatter xp-sound-at-cap hold-to-wait \
        no-sight-zoom hide-misc-quest tab-click-sorts hotkey-toggles-in-hand no-interior-fog mark-unspoken \
        no-caps-added subtitle-names compass-pips-danger neutral-foes-compass sneak-label-standing no-xp-meter-combat \
        no-limb-indicator skip-death-cam skip-death-cam-attack help-is-console heartbeat-fade heartbeat-fade-seconds \
        terminal-fade terminal-fade-seconds containers-show-ammo descriptive-marker-added no-grenade-indicator \
        grenade-indicator-harmless grenade-indicator-destructibles keep-acting-notes hide-completed-quests \
        hide-marker-reputation no-show-quest-location hide-trait-ranks hide-quest-item-figures single-item-figures \
        mark-other-ammo show-fame-infamy take-all-confirm take-all-confirm-min repair-all-confirm repair-all-threshold \
        take-all-stays hide-equipped-barter hide-equipped-containers title-case-categories no-right-click-drop \
        quiet-undiscovered repair-any-armor sort-misc-stats effect-time-left perk-plugin-names item-added-seconds \
        pipboy-12-hour notes-of-all-quests aim-turn-x aim-turn-y pickpocket-after-caught wait-anywhere radio-music \
        radio-talk camera-wheel-pov quiet-hotkeys detached-beams god-mode-off-on-load no-stealth-shimmer-pipboy \
        quiet-teammate-steps no-weapon-hotkeys pipboy-opens-items hide-crosshair-reloading kill-xp-per-level \
        key-xp-share password-xp-share invert-x nearest-unfound discovered-in-corner power-armor-radiation \
        power-armor-limbs fall-height-endurance invert-title-wheel armor-condition armor-visible-below armor-red-below \
        armor-flash-below armor-in-weapon-place scale-explosion-shake explosion-shake toggle-device hud-fatigue \
        key-repeat-accel key-repeat-delay key-repeat-mult key-repeat-delay-alt key-repeat-mult-alt precise-rad-meter \
        rad-decimals compass-doors compass-doors-exterior compass-doors-interior compass-doors-fade detected-by-whom \
        detected-names quest-text-in-menus continue-shows-save auto-continue hide-ammo-label barter-by-kind \
        barter-water-unscaled barter-sell-armor barter-sell-weapon barter-sell-aid barter-sell-ammo barter-sell-misc \
        barter-sell-weapon-mod barter-buy-armor barter-buy-weapon barter-buy-aid barter-buy-ammo barter-buy-misc \
        barter-buy-weapon-mod free-item-price skip-easy-locks skip-lock-margin pick-up-disarmed npc-weapons-whole \
        lethal-knockdowns duplicate-binds crippling-never-disarms no-spin-up god-mode-free-crafting pov-follows-weapon \
        kill-cam-chance subtitle-distance sneak-criticals-head compare-weapons warn-encumbrance mark-nearby-places \
        scale-ash-piles pipboy-buzz per-bullet-reload reload-rounds-per-press reload-press-queue reload-while-firing \
        semi-auto-queue repair-scales-crafting no-healing-in-combat power-attack-if-blocking breath-regain \
        vats-hip-fire pickpocket-caught-detects agility-jump lockpick-force-near contact-mines-player \
        contact-mines-npcs sleep-heal-min-hours barter-repair-discount armor-condition-penalty vats-ignores-companions \
        no-pipboy-in-combat frenzy-kill-xp free-wild-wasteland stats-xp-percent repair-same-first repair-low-first \
        repair-kits-in-list repair-preview crippled-fall-pain quantity-companion-cap armor-blocks-blood embolden-tags \
        pickpocket-overhaul pp-base pp-agility pp-luck pp-sneak pp-perception pp-detection pp-value pp-weight pp-worn \
        pp-free-items pp-reward-xp hold-activate hold-containers hold-furniture hold-water quick-use \
        vats-throw-visibility quiet-followers crit-message-limb no-dehydration-messages no-sleep-messages need-icons \
        repair-at-workbench character-radiation character-radiation-stages reallocate-skill-points \
        local-map-reset-cells hold-release-throws npcs-disarm-player npcs-drop-live-grenades water-shows-h2o no-respec \
        weapon-impacts robots-heal-with-scrap robot-heal-base robot-heal-per-repair robot-heal-expert \
        no-weapon-move-penalty unequip-broken-armor hide-crosshair-first hide-crosshair-third no-skill-books-at-max \
        red-hp-below red-ap-below weapon-cnd-red-below weapon-cnd-flash-below blackjack-total player-pain-gap \
        pause-on-load swap-ammo-holstered slots-hold-to-spin no-auto-continue-dialogue shift-talks-sneaking \
        no-quest-set-on-objective no-quest-set-on-complete hide-read-notes tab-closes-terminals \
        tab-back-in-pause unsafe-fast-travel vats-auto-head vats-drops-disarmed-shots vats-target-projectiles \
        vats-target-mines vats-target-own-projectiles vats-ignore-non-explosive vats-burst-stops-on-kill \
        repair-needs-weapon-skill date-style hide-unknown-names vats-keeps-holstered no-hotkey-wheel ammo-label-under \
        companion-pip female-facial-hair terminal-password-prompt lockpick-move-keys store-all map-recentre-key \
        map-marker-distance quantity-shows-price quantity-shows-weight hide-empty-reputation quick-load-in-menus \
        preview-perks pick-locks-with-key right-click-previous-category mark-notes-unread full-barter-total \
        compass-fade-left custom-dead-zones left-dead-zone right-dead-zone custom-trigger-dead-zones lt-dead-zone \
        rt-dead-zone responsive-stick faster-pad-orbit vanity-returns-first horizontal-look vats-shift-ignores-friends \
        remember-console-history console-output-file console-text-shadow click-to-exit-loading \
        select-unavailable-radios radiation-radio-static barter-caps-change no-self-meltdown max-level \
        standing-sneak-attacks no-companion-item-damage modding-needs-skill planting-needs-skill travel-costs-bottle \
        jump-swims-up armor-sinks remember-pin-health force-spares-lock no-condition-damage-penalty \
        no-condition-damage-buffer crit-ignores-fire-rate crit-damage-scale npc-dr-ignores-condition \
        npc-dt-ignores-condition explosion-radius-buff npcs-sneak-crit-player fatal-crits-gib fall-deaths-dismember \
        splash-torso-gib npcs-drop-holstered npcs-earn-casings first-person-vats no-ammo-casings crosshair-when-aiming \
        move-during-vats-camera stack-figures seated-wide no-turn-clip no-land-clip hide-unmapped-places \
        vigor-review-first scope-delay no-pointer-message-box no-pointer-dialogue levelup-wheel compare-armor \
        detailed-stats minigames-timescale no-name-limit seat-keeps-on-activate plain-going-out \
        mark-recurring-challenges hide-hotkeyed-containers hide-hotkeyed-barter hide-hotkeyed-repair \
        map-zoom-at-pointer console-homes shift-screenshot-hides print-plugin-names \
        weapon-requirements-matter console-model-details back-pace strafe-pace hack-no-single-chars \
        hack-no-repeat-words hack-dud-when-full hack-spare-guessed run-overencumbered over-run-ap over-jump-ap \
        npc-melee-part-damage disarm-needs-skill disarm-mult disarm-explode-mult cap-skills-by-special skill-cap-base \
        skill-cap-per-special skill-cap-per-luck skill-cap-most cap-skills-spare-tagged hold-breath breath-sway \
        breath-ap scope-wheel-zoom wheel-cycles-hotkeys dpad-cycles-hotkeys keys-with-pad double-tap-ammo \
        improved-weather vats-stop-following-dead night-vision-toggle night-vision-off-first night-vision-by-day \
        kill-cam-outside-vats kill-cam-hit-location weather-trip-distance compass-pips-fade compass-places-fade \
        buffed-health-bars recent-dead-secs armor-sounds-3d mine-blasts-heard blasts-spare-items grabbed-weighs \
        usable-ammo-first better-flycam recent-markers-red compass-quest-height compass-marker-height hud-marker-name \
        hud-region-names move-during-pipboy-raise region-borders-message-only marker-name-angle \
        marker-name-hide-unfound marker-name-units hide-hud-compass hide-hud-ammo hide-hud-needs hide-hud-sneak \
        hide-hud-on-load grey-read-notes remember-item-places player-weapon-volume npc-weapon-volume \
        keep-fall-height-on-load no-marker-popup marker-shift-removes marker-removed-on-it clear-marker-within \
        place-marker-here-says repeat-activate-takes-only hi-res-npc-weapons empty-reload-swaps-ammo \
        objectives-on-arrival objectives-gap sleep-in-owned-beds power-armor-no-training";

    /// [`TWEAK_ROWS`] as keys, under the Tweaks tab's group.
    fn tweak_rows() -> Vec<(&'static str, &'static str)> {
        TWEAK_ROWS.split_whitespace().map(|k| (&*Box::leak(format!("{TWEAK_PREFIX}{k}").into_boxed_str()), TWEAKS_TAB)).collect()
    }

    /// Whether `key` is one of [`TWEAK_ROWS`].
    fn is_tweak(key: &str) -> bool {
        key.strip_prefix(TWEAK_PREFIX).is_some_and(|k| TWEAK_ROWS.split_whitespace().any(|r| r == k))
    }

    /// Stewie's Tweaks' groups as the Tweaks tab lists them [design: owner 2026-11-15].
    const TWEAK_GROUPS: [&str; 8] = ["Gameplay", "Combat", "Quality of Life", "HUD", "Menus", "Hotkeys", "Cheats", "Misc"];

    #[test]
    fn the_table_names_each_key_once_in_one_of_the_tabs() {
        assert_eq!(check(SECTIONS), Ok(()));
        let twice = [section("Gameplay", Some("A"), &["a"]), section("Debug", None, &["a"])];
        assert!(check(&twice).unwrap_err().contains("placed twice"));
        let odd = [section("Sound", None, &["a"])];
        assert!(check(&odd).is_err());
        let unasked = [section(NVF_FEATURES_TAB, Some("New"), &["a-big-change"])];
        assert!(check(&unasked).unwrap_err().contains("owner named"), "NVF Features only by the owner's word");
        let control = [section("Gameplay", Some("A"), &["controls:device"])];
        assert!(check(&control).is_err());
        let headless = [section("Gameplay", Some("A"), &["a"]), section("Gameplay", None, &["b"])];
        assert!(check(&headless).is_err());
        // Every row there is has its place in the table; the table names no other but the
        // Controls tab's own rows and #1892's tweaks, each of those on the Tweaks tab.
        let named: Vec<&str> = SECTIONS.iter().flat_map(|s| s.keys.iter().copied()).filter(|k| !k.starts_with(CONTROL_PREFIX)).collect();
        for (k, _) in KNOWN {
            assert!(named.contains(&k), "{k} has no place");
        }
        for k in &named {
            assert!(KNOWN.iter().any(|(n, _)| n == k) || is_tweak(k), "{k} is placed but no row has it");
        }
        for s in SECTIONS {
            for k in s.keys.iter().filter(|k| k.starts_with(TWEAK_PREFIX)) {
                assert_eq!(s.placement.tab, TWEAKS_TAB, "{k} is a tweak");
            }
        }
    }

    /// docs/settings-reference.md is the launcher's copy of the rows' labels and notes until
    /// the game has written its menu (`crate::settings_menu`): it lists every row the game has
    /// (but the Tweaks tab's and the Controls tab's own), each under the tab and category the
    /// table places it in, and no row the game does not have [design: owner 2026-11-16].
    #[test]
    fn the_reference_has_every_row_where_the_page_shows_it() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/settings-reference.md")).unwrap();
        let mut listed: Vec<(String, String, Option<String>)> = Vec::new();
        let (mut tab, mut heading) = (String::new(), None);
        for line in text.lines() {
            if let Some(h) = line.strip_prefix("## ") {
                if h.contains("the tab's own rows") {
                    break;
                }
                let mut parts = h.split('›').map(str::trim);
                tab = parts.next().unwrap_or_default().to_string();
                heading = parts.next().map(str::to_string);
            } else if let Some((key, _)) = line.strip_prefix("### `").and_then(|h| h.split_once('`')) {
                listed.push((key.to_string(), tab.clone(), heading.clone()));
            }
        }
        let list = registry(&KNOWN);
        let l = resolve_layout(&list);
        for (key, tab, heading) in &listed {
            let i = list.iter().position(|s| s.key == key).unwrap_or_else(|| panic!("{key}: in the reference, but the game has no such row"));
            let p = l.placement_of(i).unwrap();
            assert_eq!((p.tab, p.heading), (tab.as_str(), heading.as_deref()), "{key}: the reference places it under {tab} › {heading:?}, the page under {}", p.path());
        }
        for (k, _) in KNOWN {
            let p = l.placement_of(list.iter().position(|s| s.key == k).unwrap()).unwrap();
            assert!(p.tab == TWEAKS_TAB || listed.iter().any(|(n, _, _)| n == k), "{k} ({}) is not in docs/settings-reference.md", p.path());
        }
    }

    #[test]
    fn the_tabs_come_first_with_every_row_once_in_the_tables_order() {
        let list = registry(&KNOWN);
        let l = resolve_layout(&list);
        assert_eq!(l.tab_names(), TAB_ORDER, "no group is a tab of its own: every row is named");
        assert_eq!(l.unnamed(), 0);
        let mut all: Vec<usize> = l.ordered_settings().collect();
        assert_eq!(all.len(), list.len());
        all.sort_unstable();
        assert_eq!(all, (0..list.len()).collect::<Vec<_>>(), "each registered row once");
        assert_eq!(keys(&list, &l, "Gameplay")[..11], ["[Game]", "difficulty", "hardcore", "reload-slows", "UsePipBoy", "sound-cover", "", "[V.A.T.S.]", "vats-style", "vats-slowdown", "kill-camera"]);
        assert_eq!(keys(&list, &l, "Audio"), ["[Volume]", "volume-master", "volume-music", "volume-footstep", "volume-voice", "volume-effects", "volume-radio", "", "[Sound]", "music", "voice", "reverb", "voice-reverb"]);
        assert_eq!(keys(&list, &l, "Controls")[..6], ["[Input]", "@device", "button-style", "@automatic-input", "@rumble", ""]);
        assert_eq!(
            keys(&list, &l, NVF_FEATURES_TAB),
            ["[Advanced Movement]", "advanced-movement", "movement-weight", "", "[Gameplay]", "movement-inertia", "weapon-handling", "advanced-recoil", "sprint", "", "[Visual Only]", "weapon-inertia", "animation-smoothing", "camera-motion", "", "[World Interaction]", "grass-fire", "", "[Modern Feel]", "modern-feel", "", "[Combat Feel]", "feel-hit-reactions", "feel-camera-kick", "feel-damage-direction", "feel-shell-casings", "feel-impact-effects", "", "[Sound Feel]", "feel-gun-audio", "feel-sound-occlusion", "", "[Animation and Graphics]", "feel-grounded-footsteps", "feel-npc-aim", "feel-volumetric-fog", "feel-motion-blur", "", "[Quick Loot]", "quick-loot", "quick-loot-scale", "quick-loot-background", "quick-loot-weight", "quick-loot-value", "quick-loot-value-per-weight"]
        );
        assert_eq!(keys(&list, &l, "Debug"), ["overlay", "hit-markers", "god-mode", "navmesh", "terrain-wireframe", "scope-3d"]);
        assert!(keys(&list, &l, "Tweaks").is_empty(), "an empty tab is still a tab");
        let at = |k: &str| l.placement_of(list.iter().position(|s| s.key == k).unwrap()).unwrap().path();
        assert_eq!(at("menu-pause"), "Gameplay > Dialogue");
        assert_eq!(at("hud-red"), "Display > Colours");
        assert_eq!(at("upscaler"), "Display > Upscaling and Anti-Aliasing");
        assert_eq!(at("sky-clock"), "Graphics > World and Animation");
        assert_eq!(at("grass-fire"), "NVF Features > World Interaction");
        assert_eq!(at("quick-loot"), "NVF Features > Quick Loot");
        assert_eq!(at("button-style"), "Controls > Input");
        assert_eq!(at("terrain-anti-tiling"), "Graphics > Detail and Distance", "beside terrain height blending (#2551)");
        assert_eq!(at("mesh-terrain-blend"), "Graphics > Detail and Distance", "beside the terrain rows (#2661)");
        assert_eq!(at("cloud-shadows"), "Graphics > Shadows", "after Longer sun shadows (#2670)");
        assert_eq!(at("terrain-shadows"), "Graphics > Shadows", "after Cloud shadows (#2671)");
        assert_eq!(at("sound-cover"), "Gameplay > Game", "#2688");
        assert_eq!(l.control_placement("bind:Jump").unwrap().path(), "Controls > Movement");
        assert_eq!(l.control_placement("bind:Sprint").unwrap().path(), "Controls > Movement", "beside the movement controls (#3000)");
        // Sprint is not greyed under the Global Toggle, as its Advanced Movement neighbours
        // (#3000); only its binding is, while it is off.
        assert!(needs("sprint").is_empty());
        // The registry itself is left as it was: its order and each row's group.
        let again = registry(&KNOWN);
        assert!(list.iter().zip(&again).all(|(a, b)| a.key == b.key && a.group == b.group && a.label == b.label && a.note == b.note));
    }

    #[test]
    fn a_closed_category_keeps_its_heading_and_an_empty_line_follows_only_an_open_one() {
        let list = registry(&KNOWN);
        let l = resolve_layout(&list);
        let closed = |p: &Placement| p.heading != Some("V.A.T.S.") && p.heading != Some("Camera");
        let g = lines(&list, &l, "Gameplay", closed);
        assert_eq!(g[..10], ["[Game]", "difficulty", "hardcore", "reload-slows", "UsePipBoy", "sound-cover", "", "[V.A.T.S.]", "[Camera]", "[Dialogue]"]);
        let p = l.placement_of(list.iter().position(|s| s.key == "vats-style").unwrap()).unwrap();
        assert_eq!(l.shown_count(&p, |_| true), 3);
        assert_eq!(l.shown_count(&p, |i| i != LayoutItem::Setting(list.iter().position(|s| s.key == "vats-slowdown").unwrap())), 2);
    }

    #[test]
    fn a_missing_row_and_an_empty_category_are_left_out() {
        // No lip sync, no player body; no head tracking, foot IK or sky clock at all.
        let list = registry(&[("difficulty", "Gameplay"), ("menu-pause", "Gameplay"), ("field-of-view", "Display"), ("god-mode", "Debug")]);
        let l = resolve_layout(&list);
        assert_eq!(keys(&list, &l, "Gameplay"), ["[Game]", "difficulty", "", "[Camera]", "field-of-view", "", "[Dialogue]", "menu-pause"]);
        assert!(keys(&list, &l, "Graphics").is_empty());
        // A hidden row hides its category when it is the only one, and keeps its place.
        let hidden = list.iter().position(|s| s.key == "menu-pause").unwrap();
        let shown = l.tab_entries("Gameplay", None, |i| i != LayoutItem::Setting(hidden), |_| true);
        assert!(!shown.iter().any(|e| matches!(e, LayoutEntry::Heading(p) if p.heading == Some("Dialogue"))));
        assert_ne!(shown.last(), Some(&LayoutEntry::Gap), "no empty line at the end");
        assert_eq!(l.placement_of(hidden).unwrap().path(), "Gameplay > Dialogue");
    }

    #[test]
    fn an_unnamed_row_stays_under_its_own_group() {
        let list = registry(&[("difficulty", "Gameplay"), ("new-switch", "Gameplay"), ("odd-sound", "Sound"), ("x1", "Mods"), ("volume-master", "Sound"), ("x2", "Extras"), ("x3", "Mods")]);
        let l = resolve_layout(&list);
        // After Debug, in the order the groups first appear in the registry; Sound is not Audio.
        assert_eq!(l.tab_names()[8..], ["Sound", "Mods", "Extras"]);
        assert_eq!(keys(&list, &l, "Gameplay"), ["[Game]", "difficulty", "", "new-switch"]);
        assert_eq!(keys(&list, &l, "Sound"), ["odd-sound"]);
        assert_eq!(keys(&list, &l, "Audio"), ["[Volume]", "volume-master"]);
        assert_eq!(keys(&list, &l, "Mods"), ["x1", "x3"]);
        let new = list.iter().position(|s| s.key == "new-switch").unwrap();
        assert_eq!(l.placement_of(new), Some(Placement::tab("Gameplay")));
        assert_eq!(l.unnamed(), 5);
    }

    #[test]
    fn hundreds_of_unnamed_tweaks_keep_the_registrys_order() {
        // A tweak the table does not name yet: under Tweaks, in the order registered.
        let names: Vec<&'static str> = (0..650)
            .map(|i| {
                let n: &'static str = Box::leak(format!("tweak-x{i:03}").into_boxed_str());
                n
            })
            .collect();
        let mut rows: Vec<(&'static str, &'static str)> = vec![("difficulty", "Gameplay")];
        rows.extend(names.iter().map(|n| (*n, "Tweaks")));
        rows.push(("god-mode", "Debug"));
        let list = registry(&rows);
        let l = resolve_layout(&list);
        assert_eq!(l.tab_names(), TAB_ORDER);
        let shown = keys(&list, &l, "Tweaks");
        assert_eq!(shown.len(), 650, "no heading: all in one run");
        assert!(shown.iter().zip(&names).all(|(a, b)| a == b), "in the order registered");
        assert_eq!(l.ordered_settings().count(), list.len());
        assert_eq!(l.unnamed(), 0, "the Tweaks tab's rows are listed by design, not counted as left out of the table");
    }

    #[test]
    fn every_tweak_of_1892_has_its_group_and_category() {
        let tweaks = tweak_rows();
        // 560 less the console and screenshot keys, bindings since 2026-11-16 (Controls).
        assert_eq!(tweaks.len(), 558);
        let mut rows: Vec<(&'static str, &'static str)> = KNOWN.to_vec();
        rows.extend(&tweaks);
        let list = registry(&rows);
        let l = resolve_layout(&list);
        assert_eq!(l.unnamed(), 0, "every tweak named");
        assert_eq!(l.tab_names(), TAB_ORDER);
        let t = l.tab(TWEAKS_TAB).unwrap();
        for s in &t.sections {
            assert!(s.placement.subgroup.is_some() && s.placement.heading.is_some(), "{}: a group and a heading", s.placement.path());
            assert!(s.placement.closed_by_default());
        }
        assert_eq!(t.sections.iter().map(|s| s.rows.len()).sum::<usize>(), tweaks.len(), "the tweaks, and only they, on the tab");
        let at = |k: &str| l.placement_of(list.iter().position(|s| s.key == k).unwrap()).unwrap().path();
        assert_eq!(at("tweak-no-xp-messages"), "Tweaks > HUD > Messages");
        assert_eq!(at("tweak-weightless-ammo"), "Tweaks > Cheats > Weight");
        assert_eq!(at("tweak-weightless-worn-armor"), "Tweaks > Gameplay > Weight");
        // A row that only matters with another comes right after it.
        let next = |k: &str| list[l.ordered_settings().skip_while(|&i| list[i].key != k).nth(1).unwrap()].key;
        assert_eq!(next("tweak-take-all-confirm"), "tweak-take-all-confirm-min");
        assert_eq!(next("tweak-revive-companions"), "tweak-revive-down-seconds");
        // The engine's own tabs are as they were.
        assert_eq!(keys(&list, &l, "Audio"), keys(&registry(&KNOWN), &resolve_layout(&registry(&KNOWN)), "Audio"));
    }

    #[test]
    fn the_tweaks_tab_lists_stewies_eight_groups_in_order_one_at_a_time() {
        let list = registry(&tweak_rows());
        let l = resolve_layout(&list);
        let t = l.tab(TWEAKS_TAB).unwrap();
        assert_eq!(t.subgroups(), TWEAK_GROUPS);
        // Each group's categories together, in the table's order.
        let mut runs: Vec<&str> = t.sections.iter().filter_map(|s| s.placement.subgroup).collect();
        runs.dedup();
        assert_eq!(runs, TWEAK_GROUPS, "no group split by another");
        // One group at a time, its categories closed: only their headings.
        let hotkeys = l.tab_entries(TWEAKS_TAB, Some("Hotkeys"), |_| true, |_| false);
        let headings: Vec<&str> = hotkeys.iter().filter_map(|e| if let LayoutEntry::Heading(p) = e { p.heading } else { None }).collect();
        assert_eq!(headings, ["Hide HUD Key", "Other Keys"]);
        assert_eq!(hotkeys.len(), 2, "closed: no rows, no empty line");
        // No other tab has a group.
        assert!(l.tabs.iter().filter(|t| t.name != TWEAKS_TAB).all(|t| t.subgroups().is_empty()));
        // A tweak the table does not place yet: under the last group, Other, never out of sight.
        let mut rows = tweak_rows();
        rows.push(("tweak-new-one", "Tweaks"));
        let list = registry(&rows);
        let l = resolve_layout(&list);
        let t = l.tab(TWEAKS_TAB).unwrap();
        assert_eq!(t.subgroups().last(), Some(&UNGROUPED));
        let other = l.tab_entries(TWEAKS_TAB, Some(UNGROUPED), |_| true, |_| true);
        assert_eq!(other, [LayoutEntry::Setting(list.iter().position(|s| s.key == "tweak-new-one").unwrap())]);
        assert!(l.tab_entries(TWEAKS_TAB, Some("Hotkeys"), |_| true, |_| true).iter().all(|e| *e != other[0]));
    }
}
