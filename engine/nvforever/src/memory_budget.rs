//! The machine's memory budgets and the asset caches' keep limits that follow them (#1170).
//!
//! At the start the game reads the machine's RAM (`GlobalMemoryStatusEx` on Windows,
//! `/proc/meminfo`'s `MemTotal` and `MemAvailable` elsewhere) and the GPU's memory budget, and
//! logs both (`memory budget: RAM … MB, GPU budget … MB (…)`, or that the GPU budget could not
//! be read). The GPU budget is `VK_EXT_memory_budget`'s, read through
//! wgpu's Vulkan device (`Device::as_hal`): wgpu-hal 30 enables that extension whenever the
//! adapter offers it (except on Mesa's RADV) [documented: wgpu-hal 30.0.1
//! `vulkan/adapter.rs`, "Optional `VK_EXT_memory_budget`"], so no wgpu patch is needed. Of
//! the device-local heaps ([`device_local`]) it sums `heapBudget`, "an estimate of how much
//! memory the process can allocate from that heap before allocations may fail or cause
//! performance degradation", and `heapUsage`, "the estimated heap usage of the process"
//! [documented: Vulkan `VkPhysicalDeviceMemoryBudgetPropertiesEXT`]. Budget and use are the
//! same memory, measured by the driver: what the process holds of it, blocks the allocator
//! keeps empty included (#898). An integrated GPU's memory is the machine's RAM: it has no
//! GPU budget here, and the RAM check covers it.
//!
//! Where the driver does not give the extension (RADV: wgpu leaves it off there), Linux takes
//! the card's VRAM from amdgpu's sysfs
//! (`/sys/bus/pci/devices/<pci>/drm/card*/device/mem_info_vram_{total,used}`, bytes)
//! [documented: kernel `Documentation/gpu/amdgpu/driver-misc.rst`], the card chosen by the
//! adapter's PCI address, and this process's share of it from amdgpu's per-client fdinfo
//! (`/proc/self/fdinfo`, `drm-memory-vram`, an amdgpu alias for `drm-resident-vram`, KiB)
//! [documented: kernel `Documentation/gpu/drm-usage-stats.rst`; observed on this machine's
//! kernel 7.0.0 and Mesa 25.2.8]. The DRM files are filtered to the card's render node. A
//! `dup()`ed file descriptor reports the same `drm-client-id`, so the files are grouped by it
//! before summing [documented: `drm-usage-stats.rst`, "Userspace should make sure to not
//! double account any usage statistics"]. Both feed the press check as the extension's
//! figures do. Where fdinfo has no such line, the card's whole use is every program's and is
//! only logged: the press check then stays on RAM. The log's second line says which of the
//! two it got.
//!
//! **Keep limits** ([`nvf_assets::KeepLimits`], read by the release sweep each 2 s) scale
//! down from five minutes and 2048 unused entries per cache on a machine with less than
//! [`FULL_RAM_MB`] of RAM or [`FULL_VRAM_MB`] of GPU budget, by the smaller share ([`limits_for`]).
//! **Pressed** ([`pressed`], checked every 2 s, the GPU budget read again each time so other
//! programs' use counts): when the process uses over 85 % of the GPU budget, or the machine
//! has under 10 % of its RAM free, the sweep keeps [`PRESSED`] (unused entries go after
//! 30 s, at most 128 per cache, the longest unused first) until the GPU use is under 75 % and
//! over 20 % of the RAM is free. Each change is logged. Each check's figures stay on
//! [`MemoryBudget`] (`vram_use_mb`, `ram_share`) for the performance overlay's VRAM and
//! RAM rows (`perf_overlay`, issue #1868), which do not read them again.
//!
//! **The GPU allocator's blocks** (#2754: a 2 GB GeForce GT 1030 ran out of GPU memory at
//! the default settings). wgpu takes GPU memory from the driver in blocks and hands parts
//! of them out; by default a block is 128 to 256 MB ("favor performance"), or 8 to 64 MB
//! when asked to favour memory [documented: wgpu-hal 30.0.1 `src/lib.rs`,
//! `AllocationSizes::from_memory_hints`; wgpu-types `MemoryHints`]. What the process holds
//! of the budget is the blocks, not what is in them: in Goodsprings at 1280x800 the default
//! blocks held 1,408 MB for 822 MB in use [observed: this machine, `stress sample`'s `gpu
//! allocated` and `gpu reserved`, 2026-11-16], and the GT 1030's log read 1,430 of its
//! 1,520 MB at ten minutes. So on a card with less than [`LARGE_BLOCKS_VRAM_MB`] of budget
//! the small blocks are asked for ([`hints_for`]). The hint is given when the device is
//! made, before any budget can be read, so the budget is the one **the run before** read,
//! kept in `<home>\cache\gpu-budget.txt` (a number of MB); a first run, which has none,
//! takes the small blocks [design: a card that turns out large gets the large blocks from
//! its second run]. `NVF_GPU_BLOCKS=large` or `small` says which instead, and
//! `NVF_PLAY_TEST_VRAM_MB` counts as the remembered budget.
//!
//! **Told once** [design, #2754]: the first time this process holds over
//! [`TELL_SHARE`] of the GPU budget the player gets a corner message that the graphics
//! memory is almost full, and the log a line.
//!
//! **Giving memory back** [design, #2754: an 8 GB RTX 4060 at 2560x1440 with 4096 shadow
//! maps held 7,037 MB of a 6,844 MB budget and stalled 1-2 s at a time; #3349: a GTX 1080
//! at 3840x2160 with 8x multisampling went past its budget and lost the device]. The caches
//! cannot give back what the allocator keeps (#898), but the largest single textures can:
//! the sun's shadow maps (a 4096² layer is 64 MiB against 16 MiB at 2048², five layers by
//! day in first person) and the multisampled view targets (each sample is a full-size
//! colour and depth target). Textures larger than the allocator's blocks get allocations
//! of their own from the driver, so dropping them returns their memory [observed:
//! `gpu-sheds-over-budget`, 2026-11-25: 4096 to 2048 maps took the use from 1,444 to
//! 1,204 MB]; smaller ones go back into blocks the allocator keeps (#898) and return
//! nothing. At 1920x1080 the steps gave back 240 MB (maps), 128 MB (8x to 4x) and 0 MB
//! (4x to 2x); at 1280x720 the multisampling steps gave back 0-1 MB. The last step is kept
//! for larger windows (a 4x target at 2560x1440 is past the largest block) [hypothesis]. When this process holds over
//! [`SHED_SHARE`] of the GPU budget at [`SHED_AFTER_CHECKS`] checks in a row, one step of
//! [`next_shed`] is taken (shadow maps 4096 to 2048, then multisampling 8x to 4x, then 4x
//! to 2x), at most one every [`SHED_WAIT_SECS`]; each is logged with the use before, the
//! next check logs the use after, and the player is told in a corner message what was
//! lowered. The change is made as the settings page's own rows make it, so the page shows
//! the lower value; it is kept in `NVForever.ini` only if the player saves the settings
//! page afterwards [design: on that card the lower value is the one that fits]. Nothing
//! is raised again in the run.
//!
//! Test aids, since there is no small machine to test on: `NVF_PLAY_TEST_VRAM_MB=<MB>` and
//! `NVF_PLAY_TEST_RAM_MB=<MB>` take the GPU budget and the RAM as at most that (the RAM's
//! free part shrinks by what was taken off).

use bevy::prelude::*;
use bevy::render::renderer::{RenderAdapterInfo, RenderDevice};
use nvf_assets::KeepLimits;

/// RAM at which the caches keep their full limits [design: the stress tour's process
/// stays under about 5 GB (owner 2026-10-22: about 4.7 GB used); 16 GB leaves the rest of
/// the machine room].
pub const FULL_RAM_MB: u64 = 16 * 1024;
/// GPU budget at which the caches keep their full limits [design: an 8 GB card; the tour's
/// GPU allocations peak around 3-4 GB].
pub const FULL_VRAM_MB: u64 = 8 * 1024;
/// A share of the GPU budget used: over `.0` presses the caches, under `.1` lets go [design].
pub const GPU_PRESSURE: (f64, f64) = (0.85, 0.75);
/// A share of the RAM in use by the whole machine: over `.0` (under 10 % free) presses the
/// caches, under `.1` lets go [design: what the machine has free is what paging follows;
/// the game's own commit (about 5 GB in a soak) on an 8 GB machine leaves about a quarter
/// free, which is not pressed].
pub const RAM_PRESSURE: (f64, f64) = (0.90, 0.80);
/// The keep limits while pressed: the least the budgets scale to [design: 10 s and 32
/// entries were tried first; on a tour slice capped at a 2 GB card they pressed at 77 s
/// and never let go (the allocator keeps emptied blocks, #898, so dropping assets does not
/// lower the process's use), and re-preparing what was dropped took the 1 % low from 35 to
/// 95 ms; streaming-hitches.md §5].
pub const PRESSED: KeepLimits = MIN_LIMITS;
/// The least the scaled limits go to [design].
const MIN_LIMITS: KeepLimits = KeepLimits { secs: 30.0, max: 128 };

/// A GPU budget under this (or not known yet) takes the allocator's small blocks [design:
/// the 4 GB card in #2754 reached 0.95 of its budget; a 6 GB card reads a budget under
/// 6 GB, so this takes it too].
pub const LARGE_BLOCKS_VRAM_MB: u64 = 6 * 1024;
/// The share of the GPU budget held at which the player is told, once a run [design:
/// past the 0.85 that presses the caches, before the 1.0 where allocations may fail].
pub const TELL_SHARE: f64 = 0.95;
/// What the player is told (a corner message).
pub const TELL_TEXT: &str = "Graphics memory is almost full. If the game stutters or stops, lower the graphics settings.";
/// The share of the GPU budget held over which a step of [`next_shed`] is taken [design:
/// the same share at which the player is told].
pub const SHED_SHARE: f64 = TELL_SHARE;
/// Checks in a row (2 s apart) over [`SHED_SHARE`], with no loading screen up, before a
/// step is taken [design: a moment's peak, such as a load, does not lower anything].
pub const SHED_AFTER_CHECKS: u32 = 2;
/// The least time between two steps, seconds [design: the memory of the step before is
/// back by then; checked in `gpu-sheds-over-budget`].
pub const SHED_WAIT_SECS: f64 = 10.0;

/// A step that gives GPU memory back (module notes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shed {
    /// The sun's shadow maps (and the other maps the Shadow resolution row sizes) from
    /// 4096² to 2048².
    Shadows,
    /// Multisampling from 8x to 4x.
    Msaa8,
    /// Multisampling from 4x to 2x.
    Msaa4,
}

impl Shed {
    /// What the log and the player are told was lowered.
    pub fn what(self) -> &'static str {
        match self {
            Shed::Shadows => "shadow resolution lowered to 2048",
            Shed::Msaa8 => "anti-aliasing lowered to 4x",
            Shed::Msaa4 => "anti-aliasing lowered to 2x",
        }
    }
}

/// The next step that gives memory back: the shadow maps' side now, and the multisampling
/// drawn (`None` while the anti-aliasing is done after the picture or by an upscaler,
/// where the samples are not drawn and lowering them gives nothing back).
pub fn next_shed(shadow_side: usize, msaa_drawn: Option<bevy::render::view::Msaa>) -> Option<Shed> {
    use bevy::render::view::Msaa;
    if shadow_side > 2048 {
        return Some(Shed::Shadows);
    }
    match msaa_drawn {
        Some(Msaa::Sample8) => Some(Shed::Msaa8),
        Some(Msaa::Sample4) => Some(Shed::Msaa4),
        _ => None,
    }
}

/// Take the next step of [`next_shed`] in `world`, if there is one, as the settings
/// page's rows would.
fn shed_one(world: &mut World) -> Option<Shed> {
    use crate::graphics::{self, ScreenAa};
    use bevy::render::view::Msaa;
    let side = crate::sky::map_side(world.get_resource::<bevy::light::DirectionalLightShadowMap>());
    let samples_drawn = world.get_resource::<graphics::Graphics>().is_some_and(|g| g.screen_aa == ScreenAa::Off) && !crate::upscaler::temporal_active(world);
    let drawn = world.get_resource::<graphics::MsaaSetting>().map(|m| m.0).filter(|_| samples_drawn);
    let step = next_shed(side, drawn)?;
    match step {
        Shed::Shadows => graphics::set_shadow_resolution_choice(world, 0),
        Shed::Msaa8 | Shed::Msaa4 => {
            let to = if step == Shed::Msaa8 { Msaa::Sample4 } else { Msaa::Sample2 };
            let i = graphics::AA_CHOICES.iter().position(|&(_, a, m)| a == ScreenAa::Off && m == to)?;
            graphics::set_aa_choice(world, i);
        }
    }
    Some(step)
}

/// Says which blocks to ask for (`large` or `small`), whatever the budget.
pub const BLOCKS_ENV: &str = "NVF_GPU_BLOCKS";

/// Which blocks the GPU allocator is asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blocks {
    /// wgpu's default, 128 to 256 MB a block.
    Large,
    /// 8 to 64 MB a block.
    Small,
}

/// The blocks for a GPU budget of `vram_mb` (the run before's; `None` on a first run) and
/// what [`BLOCKS_ENV`] says.
pub fn hints_for(vram_mb: Option<u64>, said: Option<&str>) -> Blocks {
    match said.map(str::trim) {
        Some("large") => Blocks::Large,
        Some("small") => Blocks::Small,
        _ if vram_mb.is_some_and(|mb| mb >= LARGE_BLOCKS_VRAM_MB) => Blocks::Large,
        _ => Blocks::Small,
    }
}

/// Where the GPU budget is kept from run to run, in NVForever's home.
fn remembered_file() -> Option<std::path::PathBuf> {
    let root = fnv_install::EngineHome::default_root()?;
    Some(fnv_install::EngineHome::new(root).cache().join("gpu-budget.txt"))
}

/// A remembered budget's text: a number of MB over 0.
fn parse_remembered(text: &str) -> Option<u64> {
    text.trim().parse::<u64>().ok().filter(|mb| *mb > 0)
}

/// What the start decided, said in the log once it is up.
static BLOCKS_AT_START: std::sync::OnceLock<(Blocks, String)> = std::sync::OnceLock::new();

/// The memory hint for the device about to be made (the module notes' "blocks"). Called
/// once, before the renderer starts.
pub fn hints_at_start() -> bevy::render::settings::MemoryHints {
    let said = std::env::var(BLOCKS_ENV).ok();
    let aid = std::env::var("NVF_PLAY_TEST_VRAM_MB").ok().and_then(|v| parse_remembered(&v));
    let kept = remembered_file().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| parse_remembered(&t));
    let blocks = hints_for(aid.or(kept), said.as_deref());
    let why = match (said.as_deref().map(str::trim), aid, kept) {
        (Some(s @ ("large" | "small")), _, _) => format!("{BLOCKS_ENV}={s}"),
        (_, Some(mb), _) => format!("the test aid's GPU budget is {mb} MB"),
        (_, None, Some(mb)) => format!("the run before read a GPU budget of {mb} MB"),
        (_, None, None) => "no GPU budget is known from a run before".to_string(),
    };
    let _ = BLOCKS_AT_START.set((blocks, why));
    match blocks {
        Blocks::Large => bevy::render::settings::MemoryHints::Performance,
        Blocks::Small => bevy::render::settings::MemoryHints::MemoryUsage,
    }
}

/// Keep the budget the device gave for the next run's start.
fn remember(budget_mb: u64) {
    let Some(file) = remembered_file() else { return };
    if fnv_install::guard_write(&file, None).is_err() {
        return;
    }
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(&file, format!("{budget_mb}\n")) {
        warn!("memory budget: the GPU budget could not be kept for the next run ({e})");
    }
}

/// What was read at the start (MB, test aids applied), and whether the caches are pressed.
#[derive(Resource, Clone, Debug, Default)]
pub struct MemoryBudget {
    /// The GPU budget, `None` when not read (not Vulkan, no extension, an integrated GPU).
    pub vram_mb: Option<u64>,
    /// The last check's figures, kept for the performance overlay (`perf_overlay`), which
    /// reads nothing a second time: this process's use of the GPU budget and the budget,
    /// MB (the test aid's cap applied), and the machine's RAM in use, a share (the test
    /// aid's RAM taken as RAM the machine does not have, as the press check takes it).
    /// `None` where not read, and where the driver gives only the card's whole use (amdgpu
    /// sysfs), which is not this process's to compare with the budget.
    pub vram_use_mb: Option<(u64, u64)>,
    pub ram_share: Option<f64>,
    /// The test aids' caps (MB), applied again at each check.
    aid_vram: Option<u64>,
    aid_ram: Option<u64>,
    /// The limits the budgets give, when not pressed.
    pub base: KeepLimits,
    pub pressed: bool,
    /// The player was told the GPU memory is almost full (once a run).
    told: bool,
    /// The most of the GPU budget this process held at a check, MB.
    vram_peak_mb: u64,
    /// Checks in a row over [`SHED_SHARE`].
    over_checks: u32,
    /// When the last step that gave memory back was taken (real seconds).
    shed_at: Option<f64>,
    /// The steps taken, in order.
    pub shed: Vec<Shed>,
    /// The step just taken and the use before it (MB), said with the next check's use.
    shed_before: Option<(Shed, u64)>,
}

pub struct MemoryBudgetPlugin;

impl Plugin for MemoryBudgetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MemoryBudget>().add_systems(Startup, read_budgets).add_systems(Update, watch_use).add_systems(Last, say_peak);
    }
}

/// The keep limits for `ram_mb` of RAM and `vram_mb` of GPU budget: full at
/// [`FULL_RAM_MB`] / [`FULL_VRAM_MB`] and over, else by the smaller share, no lower than
/// 30 s and 128 entries. An unknown budget does not count.
pub fn limits_for(ram_mb: Option<u64>, vram_mb: Option<u64>) -> KeepLimits {
    let share = |have: Option<u64>, full: u64| have.map_or(1.0, |h| (h as f64 / full as f64).min(1.0));
    let s = share(ram_mb, FULL_RAM_MB).min(share(vram_mb, FULL_VRAM_MB));
    let full = KeepLimits::default();
    KeepLimits { secs: (full.secs * s).round().max(MIN_LIMITS.secs), max: ((full.max as f64 * s).round() as usize).max(MIN_LIMITS.max) }
}

/// Whether the caches are pressed now, from each budget's share used with its
/// (press, let go) thresholds: over a press threshold presses them, under every let-go
/// threshold lets go, between keeps `was`.
pub fn pressed(was: bool, shares: &[(f64, (f64, f64))]) -> bool {
    if shares.iter().any(|&(s, (on, _))| s > on) {
        true
    } else if shares.iter().all(|&(s, (_, off))| s < off) {
        false
    } else {
        was
    }
}

/// One memory heap from `VK_EXT_memory_budget`: device-local or not, its budget and this
/// process's use, bytes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Heap {
    pub device_local: bool,
    pub budget: u64,
    pub usage: u64,
}

/// The GPU's own memory, (budget, this process's use) in bytes: the device-local heaps'
/// sums. `None` for an integrated GPU (its "device-local" heap is a carve-out of the RAM,
/// and what spills goes to the RAM, which [`RAM_PRESSURE`] watches) and without a
/// device-local budget.
pub fn device_local(heaps: &[Heap], integrated: bool) -> Option<(u64, u64)> {
    if integrated {
        return None;
    }
    let local = heaps.iter().filter(|h| h.device_local);
    let (budget, usage) = local.fold((0, 0), |(b, u), h| (b + h.budget, u + h.usage));
    (budget > 0).then_some((budget, usage))
}

/// A test aid's MB (`NVF_PLAY_TEST_VRAM_MB` / `_RAM_MB`), when set and a number.
fn aid_mb(name: &str) -> Option<u64> {
    let v = std::env::var(name).ok()?;
    match v.trim().parse::<u64>() {
        Ok(mb) if mb > 0 => {
            info!("test aid {name}: {mb} MB");
            Some(mb)
        }
        _ => {
            warn!("{name}={v}: expected megabytes, a whole number over 0");
            None
        }
    }
}

/// The lower of what was read and what a test aid allows.
fn capped(read: Option<u64>, aid: Option<u64>) -> Option<u64> {
    match (read, aid) {
        (Some(r), Some(a)) => Some(r.min(a)),
        (r, a) => r.or(a),
    }
}

const MB: u64 = 1024 * 1024;

/// The machine's RAM in MB: (total, free now).
#[cfg(windows)]
fn read_ram() -> Option<(u64, u64)> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..unsafe { std::mem::zeroed() } };
    // SAFETY: `m` is a MEMORYSTATUSEX with its length set, as the call asks.
    (unsafe { GlobalMemoryStatusEx(&mut m) } != 0).then_some((m.ullTotalPhys / MB, m.ullAvailPhys / MB))
}

#[cfg(not(windows))]
fn read_ram() -> Option<(u64, u64)> {
    meminfo_ram(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

/// `MemTotal` and `MemAvailable` from `/proc/meminfo`, kB, as MB: (total, free now).
#[cfg(not(windows))]
fn meminfo_ram(text: &str) -> Option<(u64, u64)> {
    fn kb(text: &str, key: &str) -> Option<u64> {
        let line = text.lines().find(|l| l.starts_with(key))?;
        Some(line.split_whitespace().nth(1)?.parse::<u64>().ok()? / 1024)
    }
    Some((kb(text, "MemTotal:")?, kb(text, "MemAvailable:")?))
}

/// The RAM's share in use, a test aid's cap taken as RAM the machine does not have.
fn ram_share(ram: (u64, u64), aid: Option<u64>) -> f64 {
    let (total, free) = ram;
    let cap = aid.map_or(total, |a| a.min(total));
    let free = free.saturating_sub(total - cap);
    1.0 - free as f64 / cap.max(1) as f64
}

/// The GPU's heaps from `VK_EXT_memory_budget` and whether it is integrated, or why not.
fn read_heaps(device: &RenderDevice) -> Result<Vec<Heap>, String> {
    use ash::vk;
    let wgpu_device = device.wgpu_device();
    // SAFETY: only reads through the hal device, which stays alive in the guard.
    let Some(hal) = (unsafe { wgpu_device.as_hal::<wgpu::hal::api::Vulkan>() }) else {
        return Err("not the Vulkan backend".into());
    };
    if !hal.enabled_device_extensions().contains(&ash::ext::memory_budget::NAME) {
        return Err("VK_EXT_memory_budget is not enabled on this device (wgpu leaves it off on Mesa's RADV)".into());
    }
    let instance = hal.shared_instance().raw_instance();
    let mut budget = vk::PhysicalDeviceMemoryBudgetPropertiesEXT::default();
    let mut props = vk::PhysicalDeviceMemoryProperties2::default().push_next(&mut budget);
    // SAFETY: the physical device belongs to this instance; the budget struct is chained
    // as the extension (enabled, checked above) asks. Vulkan 1.1 core, which wgpu requires.
    unsafe { instance.get_physical_device_memory_properties2(hal.raw_physical_device(), &mut props) };
    let n = props.memory_properties.memory_heap_count as usize;
    let heaps = props.memory_properties.memory_heaps[..n].iter().enumerate();
    Ok(heaps.map(|(i, h)| Heap { device_local: h.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL), budget: budget.heap_budget[i], usage: budget.heap_usage[i] }).collect())
}

/// The GPU budget, and this process's use of it when the driver gives that, MB.
struct Vram {
    budget_mb: u64,
    /// `None` where only the card's whole use is readable: that is every program's, so it
    /// neither feeds the press check nor is labelled this process's.
    used_mb: Option<u64>,
    /// Where the budget came from, short enough for the start-up line.
    source: String,
    /// The start-up line's second sentence, for a source that needs explaining.
    detail: Option<String>,
}

/// The GPU budget and this process's use of it, MB, or why not.
fn read_vram(device: Option<&RenderDevice>, integrated: bool, pci_bus_id: Option<&str>) -> Result<Vram, String> {
    if integrated {
        return Err("an integrated GPU: its memory is the RAM".into());
    }
    let from_extension = read_heaps(device.ok_or("no render device")?)
        .and_then(|heaps| device_local(&heaps, false).ok_or_else(|| "no device-local heap budget".to_string()));
    match from_extension {
        Ok((b, u)) => Ok(Vram {
            budget_mb: b / MB,
            used_mb: Some(u / MB),
            source: "VK_EXT_memory_budget, device-local heaps".into(),
            detail: None,
        }),
        Err(why) => vram_beside_the_extension(pci_bus_id, why),
    }
}

/// The budget read where the driver does not give the extension's figures: amdgpu's sysfs
/// (Linux) has the card's VRAM, and its per-client fdinfo has this process's share of it.
#[cfg(not(windows))]
fn vram_beside_the_extension(pci_bus_id: Option<&str>, why: String) -> Result<Vram, String> {
    let id = pci_bus_id.ok_or_else(|| format!("{why}; the adapter has no PCI address"))?;
    let (b, card_used) = sysfs_vram(id).map_err(|e| format!("{why}; {e}"))?;
    let ours = process_vram_mb(id);
    Ok(Vram {
        budget_mb: b,
        used_mb: ours,
        source: "amdgpu sysfs, the card's VRAM".into(),
        detail: Some(match ours {
            Some(_) => format!(
                "{why}; this process's use is /proc/self/fdinfo's drm-memory-vram, summed over the card's \
                 DRM files, and the press check compares it with the budget; the card's {card_used} MB is every program's"
            ),
            None => format!(
                "{why}; the card reports {card_used} MB in use by every program and fdinfo has no \
                 drm-memory-vram for this process, so the GPU press check is off and only RAM pressure applies"
            ),
        }),
    })
}

/// This process's own use of the card's VRAM, MB, from amdgpu's per-client fdinfo
/// (`drm-memory-vram`, KiB [documented: `Documentation/gpu/drm-usage-stats.rst`]). A DRM file
/// belongs to the card it was opened on, so only the files whose render node sits at the
/// adapter's PCI address count. A `dup()`ed fd shares one `drm-client-id`, so those count once.
#[cfg(not(windows))]
fn process_vram_mb(pci_bus_id: &str) -> Option<u64> {
    let mut texts = Vec::new();
    for entry in std::fs::read_dir("/proc/self/fdinfo").ok()?.flatten() {
        let fd = entry.file_name();
        let Ok(node) = std::fs::read_link(format!("/proc/self/fd/{}", fd.to_string_lossy())) else {
            continue;
        };
        if !node.starts_with("/dev/dri") || drm_node_card(&node).as_deref() != Some(pci_bus_id) {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(entry.path()) {
            texts.push(text);
        }
    }
    vram_by_client(texts.iter().map(String::as_str))
}

/// The VRAM these fdinfo texts report for their clients, MB: one figure per `drm-client-id`,
/// so a `dup()`ed descriptor is not counted twice [documented: `drm-usage-stats.rst`]. A text
/// with no client id counts on its own.
#[cfg(not(windows))]
fn vram_by_client<'a>(texts: impl IntoIterator<Item = &'a str>) -> Option<u64> {
    use std::collections::HashMap;
    let mut per_client: HashMap<String, u64> = HashMap::new();
    for (i, text) in texts.into_iter().enumerate() {
        let Some(mb) = fdinfo_vram_mb(text) else { continue };
        let key = fdinfo_client_id(text).map_or_else(|| format!("#{i}"), str::to_string);
        per_client.entry(key).or_insert(mb);
    }
    let sum = per_client.values().sum();
    (!per_client.is_empty()).then_some(sum)
}

/// `drm-client-id: 2771` as the client's id, which every fd of one open file shares.
#[cfg(not(windows))]
fn fdinfo_client_id(text: &str) -> Option<&str> {
    text.lines().find(|l| l.starts_with("drm-client-id:"))?.split_whitespace().nth(1)
}

/// The card a `/dev/dri/renderD*` node belongs to, as its PCI address.
#[cfg(not(windows))]
fn drm_node_card(node: &std::path::Path) -> Option<String> {
    let drm = std::path::Path::new("/sys/class/drm").join(node.file_name()?);
    let device = std::fs::canonicalize(drm.join("device")).ok()?;
    Some(device.file_name()?.to_string_lossy().to_string())
}

/// `drm-memory-vram:   607296 KiB` as MB. amdgpu prints this key in KiB [documented:
/// `Documentation/gpu/drm-usage-stats.rst` allows bytes, `KiB` or `MiB`; observed `KiB` on
/// this machine's kernel 7.0.0].
#[cfg(not(windows))]
fn fdinfo_vram_mb(text: &str) -> Option<u64> {
    let line = text.lines().find(|l| l.starts_with("drm-memory-vram:"))?;
    Some(line.split_whitespace().nth(1)?.parse::<u64>().ok()? / 1024)
}

/// Off Linux there is no sysfs to fall back on.
#[cfg(windows)]
fn vram_beside_the_extension(_pci_bus_id: Option<&str>, why: String) -> Result<Vram, String> {
    Err(why)
}

/// The card's VRAM from amdgpu's sysfs, MB: (total, used). The adapter's PCI address names
/// the card, since a machine with two GPUs has a `card*` for each. Both files are in bytes
/// [documented: `Documentation/gpu/amdgpu/driver-misc.rst`].
#[cfg(not(windows))]
fn sysfs_vram(id: &str) -> Result<(u64, u64), String> {
    let drm = format!("/sys/bus/pci/devices/{id}/drm");
    let entries = std::fs::read_dir(&drm).map_err(|e| format!("no VRAM figures: {drm}: {e}"))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        // `card3` is the card; `card3-DP-2`, `renderD131` and `controlD67` are not.
        if !name.starts_with("card") || !name[4..].chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let device = entry.path().join("device");
        let total = read_sysfs_mb(&device.join("mem_info_vram_total"));
        let used = read_sysfs_mb(&device.join("mem_info_vram_used"));
        if let (Some(total), Some(used)) = (total, used) {
            return Ok((total, used));
        }
    }
    Err(format!("no mem_info_vram_* under {drm} (not an amdgpu card?)"))
}

/// A sysfs counter in bytes as MB [documented: `Documentation/gpu/amdgpu/driver-misc.rst`].
#[cfg(not(windows))]
fn read_sysfs_mb(path: &std::path::Path) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.trim().parse::<u64>().ok().map(|b| b / MB)
}

fn integrated(adapter: Option<&RenderAdapterInfo>) -> bool {
    adapter.is_some_and(|a| a.device_type == wgpu::DeviceType::IntegratedGpu)
}

/// Read the budgets, log them and set the keep limits.
fn read_budgets(mut commands: Commands, device: Option<Res<RenderDevice>>, adapter: Option<Res<RenderAdapterInfo>>, mut budget: ResMut<MemoryBudget>) {
    let ram = read_ram();
    let aid_ram = aid_mb("NVF_PLAY_TEST_RAM_MB");
    let ram_mb = capped(ram.map(|r| r.0), aid_ram);
    let vram = read_vram(device.as_deref(), integrated(adapter.as_deref()), adapter.as_deref().map(|a| a.device_pci_bus_id.as_str()));
    let aid_vram = aid_mb("NVF_PLAY_TEST_VRAM_MB");
    let vram_mb = match &vram {
        Ok(v) => capped(Some(v.budget_mb), aid_vram),
        Err(_) => aid_vram,
    };
    let source = match &vram {
        Ok(v) => match v.used_mb {
            Some(used) => format!("{}; this process uses {used} MB", v.source),
            None => format!("{}; this process's use not read", v.source),
        },
        Err(e) => format!("not read: {e}"),
    };
    let base = limits_for(ram_mb, vram_mb);
    let ram_text = match (ram, ram_mb) {
        (Some((t, free)), Some(mb)) if mb < t => format!("RAM {mb} MB (test aid; the machine has {t} MB, {free} free)"),
        (Some((t, free)), _) => format!("RAM {t} MB ({free} free)"),
        (None, Some(mb)) => format!("RAM {mb} MB (test aid; the machine's not read)"),
        (None, None) => "RAM not read".to_string(),
    };
    let vram_text = match (&vram, vram_mb) {
        (Ok(v), Some(mb)) if mb < v.budget_mb => format!("GPU budget {mb} MB (test aid; the device's {} MB, {source})", v.budget_mb),
        (_, Some(mb)) => format!("GPU budget {mb} MB ({source})"),
        (_, None) => format!("GPU budget {source}"),
    };
    info!("memory budget: {ram_text}, {vram_text}; caches keep unused models and textures {:.0} s, at most {} per cache", base.secs, base.max);
    if let Ok(v) = &vram {
        if let Some(detail) = &v.detail {
            info!("memory budget: {detail}");
        }
        // The device's own budget, not the test aid's: the next run's blocks follow it.
        remember(v.budget_mb);
    }
    if let Some((blocks, why)) = BLOCKS_AT_START.get() {
        let size = match blocks {
            Blocks::Large => "large (128 to 256 MB)",
            Blocks::Small => "small (8 to 64 MB)",
        };
        info!("memory budget: GPU memory is taken in {size} blocks: {why}");
    }
    let vram_use_mb = vram.as_ref().ok().and_then(|v| v.used_mb.map(|used| (used, vram_mb.unwrap_or(v.budget_mb))));
    let ram_in_use = ram.map(|r| ram_share(r, aid_ram));
    *budget = MemoryBudget { vram_mb, vram_use_mb, ram_share: ram_in_use, aid_vram, aid_ram, base, pressed: false, told: false, vram_peak_mb: vram_use_mb.map_or(0, |u| u.0), ..default() };
    commands.insert_resource(base);
}

/// Every 2 s: this process's use of the GPU budget (both read again: other programs change
/// the budget) and the machine's free RAM; press the caches near either.
fn watch_use(
    real: Res<Time<Real>>,
    device: Option<Res<RenderDevice>>,
    adapter: Option<Res<RenderAdapterInfo>>,
    mut budget: ResMut<MemoryBudget>,
    mut limits: ResMut<KeepLimits>,
    (mut corner, mut commands, loading): (Option<ResMut<nvf_play::corner_messages::CornerMessages>>, Commands, Option<Res<nvf_play::loading::Loading>>),
    mut last: Local<f64>,
) {
    let now = real.elapsed_secs_f64();
    if now - *last < f64::from(nvf_assets::release::SWEEP_SECS) {
        return;
    }
    *last = now;
    let gpu = budget.vram_mb.and_then(|_| {
        let pci = adapter.as_deref().map(|a| a.device_pci_bus_id.as_str());
        read_vram(device.as_deref(), integrated(adapter.as_deref()), pci).ok()
    }).and_then(|v| v.used_mb.map(|used| (capped(Some(v.budget_mb), budget.aid_vram).unwrap_or(v.budget_mb), used)));
    let ram = read_ram();
    budget.vram_use_mb = gpu.map(|(b, used)| (used, b));
    budget.ram_share = ram.map(|r| ram_share(r, budget.aid_ram));
    let mut shares = Vec::new();
    if let Some((b, used)) = gpu {
        let share = used as f64 / b.max(1) as f64;
        shares.push((share, GPU_PRESSURE));
        budget.vram_peak_mb = budget.vram_peak_mb.max(used);
        if share > TELL_SHARE && !budget.told {
            budget.told = true;
            warn!("memory budget: the GPU memory is almost full (this process holds {used} of {b} MB); the player is told once");
            if let Some(c) = corner.as_mut() {
                c.push(TELL_TEXT.to_string().into());
            }
        }
        if let Some((step, before)) = budget.shed_before.take() {
            info!("memory budget: after {} this process holds {used} of {b} MB (it held {before} MB)", step.what());
        }
        // A loading screen's peak (the cells and pipelines coming in) does not count.
        let loading_now = loading.as_deref().is_some_and(nvf_play::loading::Loading::active);
        budget.over_checks = if share > SHED_SHARE && !loading_now { budget.over_checks + 1 } else { 0 };
        if budget.over_checks >= SHED_AFTER_CHECKS && budget.shed_at.is_none_or(|t| now - t >= SHED_WAIT_SECS) {
            budget.shed_at = Some(now);
            commands.queue(move |world: &mut World| {
                let Some(step) = shed_one(world) else { return };
                warn!("memory budget: over {:.0} % of the GPU budget ({used} of {b} MB) at {SHED_AFTER_CHECKS} checks in a row: {} to give memory back", SHED_SHARE * 100.0, step.what());
                let mut m = world.resource_mut::<MemoryBudget>();
                m.shed.push(step);
                m.shed_before = Some((step, used));
                if let Some(mut c) = world.get_resource_mut::<nvf_play::corner_messages::CornerMessages>() {
                    c.push(format!("Graphics memory is full: {}.", step.what()).into());
                }
            });
        }
    }
    if let Some(r) = ram {
        shares.push((ram_share(r, budget.aid_ram), RAM_PRESSURE));
    }
    let now_pressed = pressed(budget.pressed, &shares);
    if now_pressed == budget.pressed {
        return;
    }
    budget.pressed = now_pressed;
    *limits = if now_pressed { PRESSED } else { budget.base };
    let gpu_text = gpu.map_or("?".to_string(), |(b, used)| format!("{used} of {b} MB"));
    let ram_text = ram.map_or("?".to_string(), |r| format!("{:.0} % in use", ram_share(r, budget.aid_ram) * 100.0));
    info!(
        "memory budget: {} (GPU: this process uses {gpu_text}; RAM: {ram_text}); caches keep unused {:.0} s, at most {} per cache",
        if now_pressed { "near a budget, caches pressed" } else { "back under the budgets" },
        limits.secs,
        limits.max
    );
}

/// At the exit: the most of the GPU budget the run held.
fn say_peak(mut exit: MessageReader<AppExit>, budget: Res<MemoryBudget>) {
    if exit.read().next().is_none() {
        return;
    }
    if let Some((_, b)) = budget.vram_use_mb {
        info!("memory budget: this process held at most {} of the {b} MB GPU budget in this run", budget.vram_peak_mb);
    }
}

#[cfg(test)]
mod tests {
    /// #2754: small blocks under 6 GB of budget and on a first run; the variable wins.
    #[test]
    fn the_allocators_blocks_follow_the_remembered_budget() {
        use super::{hints_for, parse_remembered, Blocks};
        assert_eq!(hints_for(Some(1819), None), Blocks::Small);
        assert_eq!(hints_for(Some(3600), None), Blocks::Small);
        assert_eq!(hints_for(Some(7300), None), Blocks::Large);
        assert_eq!(hints_for(None, None), Blocks::Small, "a first run");
        assert_eq!(hints_for(Some(1819), Some("large")), Blocks::Large);
        assert_eq!(hints_for(Some(7300), Some(" small ")), Blocks::Small);
        assert_eq!(hints_for(Some(7300), Some("other")), Blocks::Large);
        assert_eq!(parse_remembered("1819\n"), Some(1819));
        assert_eq!(parse_remembered("0"), None);
        assert_eq!(parse_remembered("lots"), None);
    }

    use super::*;

    #[test]
    fn keep_limits_scale_with_the_smaller_budget_share() {
        let full = KeepLimits::default();
        assert_eq!(limits_for(Some(48 * 1024), Some(12 * 1024)), full, "the owner's machine");
        assert_eq!(limits_for(None, None), full, "nothing read: as before");
        assert_eq!(limits_for(Some(8 * 1024), None), KeepLimits { secs: 150.0, max: 1024 }, "8 GB of RAM: half");
        assert_eq!(limits_for(Some(32 * 1024), Some(4 * 1024)), KeepLimits { secs: 150.0, max: 1024 }, "a 4 GB card: half");
        assert_eq!(limits_for(Some(8 * 1024), Some(2 * 1024)), KeepLimits { secs: 75.0, max: 512 }, "the smaller share");
        assert_eq!(limits_for(Some(1024), Some(256)), MIN_LIMITS, "never under the least");
    }

    /// The Linux RAM read: `/proc/meminfo`'s two lines, kB as MB; another key that starts the
    /// same way, or a missing line, does not fool it.
    #[cfg(not(windows))]
    #[test]
    fn meminfo_gives_the_ram_and_what_of_it_is_available() {
        let sample = "MemTotal:       32143116 kB\nMemFree:         1234567 kB\n\
                      MemAvailable:   20000000 kB\nCached:           800000 kB\n";
        assert_eq!(meminfo_ram(sample), Some((32143116 / 1024, 20000000 / 1024)));
        assert_eq!(meminfo_ram("MemTotal:       32143116 kB\n"), None, "no MemAvailable");
        assert_eq!(meminfo_ram(""), None);
        // MemFree is not MemAvailable: "Mem" alone must not match.
        assert_eq!(meminfo_ram("MemFree: 100 kB\nMemTotal: 200 kB\n"), None);
    }

    /// The per-process VRAM read: amdgpu's fdinfo line in KiB as MB, and nothing else.
    #[cfg(not(windows))]
    #[test]
    fn fdinfo_vram_is_read_in_mb() {
        let sample = "pos:\t0\nflags:\t02100002\ndrm-client-id:\t2771\ndrm-memory-vram: \t607296 KiB\n\
                      drm-memory-gtt: \t1340812 KiB\n";
        assert_eq!(fdinfo_vram_mb(sample), Some(593));
        assert_eq!(fdinfo_vram_mb("drm-memory-gtt: 1 KiB\n"), None, "GTT is not VRAM");
        assert_eq!(fdinfo_vram_mb("drm-memory-vram: \n"), None);
    }

    /// A `dup()`ed descriptor and its original are one client and count once; two clients add
    /// up; a text without a client id still counts.
    #[cfg(not(windows))]
    #[test]
    fn fdinfo_vram_counts_a_shared_client_once() {
        let one = "drm-client-id:\t2771\ndrm-memory-vram: \t607296 KiB\n";
        let same = "drm-client-id:\t2771\ndrm-memory-vram: \t607296 KiB\n";
        let other = "drm-client-id:\t2772\ndrm-memory-vram: \t1024 KiB\n";
        assert_eq!(vram_by_client([one, same]), Some(593), "one client, counted once");
        assert_eq!(vram_by_client([one, other]), Some(594), "two clients add up");
        assert_eq!(vram_by_client(["drm-memory-vram: \t2048 KiB\n"]), Some(2), "no client id: counts alone");
        assert_eq!(vram_by_client(["drm-memory-gtt: 1 KiB\n"]), None, "no VRAM line: none");
    }

    #[test]
    fn memory_is_given_back_shadows_first_then_samples_down_to_2x() {
        use bevy::render::view::Msaa;
        assert_eq!(next_shed(4096, Some(Msaa::Sample8)), Some(Shed::Shadows));
        assert_eq!(next_shed(2048, Some(Msaa::Sample8)), Some(Shed::Msaa8));
        assert_eq!(next_shed(2048, Some(Msaa::Sample4)), Some(Shed::Msaa4));
        assert_eq!(next_shed(2048, Some(Msaa::Sample2)), None);
        assert_eq!(next_shed(2048, None), None, "samples not drawn: nothing to give back");
        assert_eq!(next_shed(4096, None), Some(Shed::Shadows));
    }

    #[test]
    fn caches_are_pressed_near_a_budget_and_let_go_well_under() {
        assert!(!pressed(false, &[]));
        assert!(pressed(false, &[(0.5, GPU_PRESSURE), (0.9, GPU_PRESSURE)]), "one budget over 85 %");
        assert!(pressed(true, &[(0.8, GPU_PRESSURE), (0.2, RAM_PRESSURE)]), "between: stays pressed");
        assert!(!pressed(false, &[(0.8, GPU_PRESSURE)]), "between: stays let go");
        assert!(!pressed(true, &[(0.7, GPU_PRESSURE), (0.74, RAM_PRESSURE)]), "every budget under its let-go share");
        assert!(!pressed(false, &[(0.88, RAM_PRESSURE)]), "RAM 88 % in use: not yet");
        assert!(pressed(false, &[(0.92, RAM_PRESSURE)]), "under 10 % of the RAM free");
    }

    /// The GPU budget is the device-local heaps' only, and the use compared with it is
    /// those heaps' too; an integrated GPU, or none device-local, has none.
    #[test]
    fn the_gpu_budget_is_the_device_local_heaps() {
        const G: u64 = 1 << 30;
        // A discrete card: 12 GB of its own, 32 GB of the machine's seen through it.
        let discrete = [Heap { device_local: true, budget: 11 * G, usage: 3 * G }, Heap { device_local: false, budget: 30 * G, usage: 5 * G }];
        assert_eq!(device_local(&discrete, false), Some((11 * G, 3 * G)), "the host heap's use does not count against the card");
        // A card with a second, small device-local heap (the host-visible BAR window).
        let bar = [Heap { device_local: true, budget: 7 * G, usage: G }, Heap { device_local: true, budget: G / 4, usage: G / 8 }, Heap { device_local: false, budget: 16 * G, usage: G }];
        assert_eq!(device_local(&bar, false), Some((7 * G + G / 4, G + G / 8)));
        // An APU: a 512 MB carve-out marked device-local, the rest in the RAM's heap.
        let apu = [Heap { device_local: true, budget: G / 2, usage: G / 2 }, Heap { device_local: false, budget: 14 * G, usage: 2 * G }];
        assert_eq!(device_local(&apu, true), None, "an integrated GPU: the RAM check covers it");
        // No device-local budget reported.
        assert_eq!(device_local(&[Heap { device_local: false, budget: 8 * G, usage: G }], false), None);
        assert_eq!(device_local(&[], false), None, "no extension: no heaps");
    }

    #[test]
    fn the_ram_aid_takes_ram_away_from_whats_free() {
        // 48 GB, 32 GB free: 33 % in use.
        assert!((ram_share((48_000, 32_000), None) - 1.0 / 3.0).abs() < 1e-9);
        // Capped at 6 GB: the 16 GB in use leave nothing free.
        assert_eq!(ram_share((48_000, 32_000), Some(6_000)), 1.0);
        // Capped at 24 GB: 8 of them free.
        assert!((ram_share((48_000, 32_000), Some(24_000)) - (1.0 - 8.0 / 24.0)).abs() < 1e-9);
    }

    #[test]
    fn a_test_aid_only_lowers_what_was_read() {
        assert_eq!(capped(Some(12000), Some(2048)), Some(2048));
        assert_eq!(capped(Some(1000), Some(2048)), Some(1000));
        assert_eq!(capped(None, Some(2048)), Some(2048));
        assert_eq!(capped(Some(12000), None), Some(12000));
    }
}
