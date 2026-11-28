# Spec notes: display sync and variable refresh (G-Sync, FreeSync) on Windows

Not a behaviour of the original game: how NVForever's frames meet the screen's refresh
(#1082). Tags: **[documented]** cites a public vendor, Microsoft, Khronos or crate source,
**[observed]** was measured here, **[hypothesis]** needs checking, **[design]** is our
choice. Code: `engine/nvforever/src/frame_pace.rs`, the settings row in
`engine/nvforever/src/settings.rs`; design: docs/design/graphics-enhanced.md §3.10.

## 1. What NVForever does

- **Display sync** (`display-sync` in `[Settings]` of NVForever.ini: `off`, `vsync`,
  `variable`) replaces the old Vertical sync switch. An INI with only the old `vsync=1`/`0`
  still sets Vertical sync / Off at the start (`read_old_vsync`) [design].
- **Vertical sync** asks Bevy for `PresentMode::AutoVsync`; Bevy presents FIFO relaxed where
  the surface offers it, else FIFO [documented: bevy_render 0.20 `view/window/mod.rs`
  `present_mode`; wgpu-types 30 `PresentMode::AutoVsync`]. Off is `AutoNoVsync` (Immediate,
  else Mailbox, else FIFO) [documented: same].
- **Variable refresh** keeps vsync on and caps the frame rate below the refresh rate:
  refresh − refresh²/3600, at least 3 below, never under 30 (60 Hz → 57, 144 Hz → 138,
  240 Hz → 224) [design]. The r − r²/3600 rule is one in wide use for VRR caps (it gives
  138 at 144 Hz); no vendor document found here states a figure [hypothesis: the margin
  is enough for our sleep-then-spin limiter, whose error is under 1 ms]. A page frame-rate
  limit below it wins.
- **Variable refresh with DLSS Frame Generation**: DLSS-G needs vsync off on Vulkan
  [documented: "VSync with Frame Generation is only supported on D3D12",
  `ProgrammingGuideDLSS_G.md` §22.5], so while it is asked for the window presents with
  vsync off (`AutoNoVsync`) and the same cap, on frames shown, goes whole to Reflex's frame
  limit (`frame_pace::wait_for_cap`) [design], which holds the frames shown, generated ones
  included: 152 at 2x is 152 shown and 76 real [observed: 13158 us held 76 shown and 38 real,
  a run on an RTX 5090 at 160 Hz, 2026-11-23].
  Frames then reach the screen under its refresh rate, where a VRR screen refreshes on each
  one; vsync is only what would hold a frame that came in faster than the screen's top rate.
  The cap's margin is not enough: a frame now and then comes in under the refresh interval
  and the screen tears near the bottom [observed 2026-11-23, 160 Hz G-SYNC, 2x, 152 shown /
  76 real]. The game cannot hold it back: Streamline presents DLSS-G's frames through a
  swapchain of its own with sync interval 0 whatever present mode the game asks for (Mailbox
  was tried and made no difference) [observed: its log, "interpolation state changed from
  disabled to enabled (... SyncInterval=0)", "platform: Vulkan (VSync with FG: not
  supported)"]. The driver can: NVIDIA Control Panel's Vertical sync On for the game took
  the tearing away with DLSS-G still generating [observed 2026-11-23, a run on an RTX 5090 at 160 Hz: state
  eOk, 2 frames presented a frame]; NVIDIA's guide says the Control Panel's setting takes
  precedence over the application's [documented: `ProgrammingGuideDLSS_G.md` §22.3]. That is
  the setup the player pages give for a G-SYNC screen.
  Vertical sync still dims DLSS-G ("Needs vsync off").
- The refresh rate is the window's monitor's, from winit, which documents it as "the
  monitor refresh rate used by the system" and says to use the video mode's rate in
  exclusive full screen [documented: winit 0.30.13 `MonitorHandle::refresh_rate_millihertz`];
  in exclusive full screen NVForever takes the video mode it chose.
- At the start the log names the present modes the window's surface supports and the one
  vsync presents as (`frame pace: the surface (Vulkan) supports present modes [...]`).
- Mailbox was measured as the default and dropped: far worse frame times than vsync on the
  owner's RTX 4070 (fps 18.2/12.4 against 40.3/28.4) [observed: #1082, 2026-10-30].

Why it helps: with vsync and no VRR, a frame that misses a 60 Hz refresh waits for the
next one and shows for 33 ms [observed: #1082, Primm 18–19 ms frames came out at 41–44
fps]. A VRR screen refreshes when the frame is ready, as long as the rate stays inside its
range; capping under the top of the range keeps vsync from ever being the thing that
waits [hypothesis, the usual reasoning for a cap; to be felt by the owner].

## 2. Variable refresh, Vulkan and Windows: what is documented

**Microsoft (DXGI, the path a windowed game's frames reach the screen through)**

- "Variable refresh rate displays require tearing to be enabled, this is also known as
  'vsync-off' support"; the flag is to be passed "when the app is in a windowed mode -
  including border-less fullscreen mode" [documented: Microsoft Learn, *Variable refresh
  rate displays*, learn.microsoft.com/windows/win32/direct3ddxgi/variable-refresh-rate-displays].
  That page is about D3D11/12 flip-model swap chains, not Vulkan.

**NVIDIA**

- Release 440.97 (2019-10-22) "adds support for windowed G-SYNC for OpenGL and Vulkan-based
  applications" [documented: Khronos news, *NVIDIA GeForce 440.97 WHQL Driver released with
  Vulkan improvements*, khronos.org/news/permalink/nvidia-geforce-440.97-whql-driver-released-with-vulkan-improvements].
  So on NVIDIA a Vulkan game in a window, borderless full screen included, can run G-SYNC
  when the control panel's G-SYNC setting is "windowed and full screen mode".
- With "full screen mode only" chosen, G-SYNC may still be active in a window: from Windows
  10 20H1, Volta and later GPUs and Release 460 drivers support multiplane overlays, and
  "Windows may grant a windowed application to its own plane, in which case the plane
  behaves as if it were operating in full-screen mode" [documented: NVIDIA Release 595
  driver notes, 596.49, §5.2 "G-SYNC May be Enabled in Windowed Mode Even if Only
  Full-screen Mode is Specified", us.download.nvidia.com/Windows/596.49/596.49-win11-win10-release-notes.pdf].
  So a borderless window on a 4070 with Windows 10 20H1 or later can get G-SYNC even with
  "full screen only" chosen. Whether it does is up to Windows.
- Windowed G-SYNC is turned off while hardware-accelerated video plays on another display
  (it comes back when the video pauses or closes) [documented: same notes, §5.4].
- The control panel's "Vulkan/OpenGL present method" ("Prefer layered on DXGI swapchain")
  is often said to matter for windowed Vulkan G-SYNC. No NVIDIA release note found here
  documents it (searched 522.25, 526.47, 526.86, 527.37, 531.18–531.61, 596.49)
  [hypothesis: worth the owner trying if G-SYNC does not engage in borderless].
- Exclusive full screen: G-SYNC's "full screen mode" setting covers it [documented: §5.2
  above names that setting]. NVForever's exclusive full screen is winit changing the
  monitor's video mode with an ordinary window swapchain; wgpu does not use
  `VK_EXT_full_screen_exclusive` [hypothesis: whether the NVIDIA driver treats it as
  full screen or as a window covering the screen is not documented].

**AMD**

- AMD's driver pages for older Radeon cards are reported to list, for FreeSync, support
  for "Borderless Fullscreen Mode for applications and gaming with AMD FreeSync technology
  displays" (Radeon Software Crimson ReLive, around 16.12.2, early 2017), and the 17.2.1 to
  17.7.1 release notes to list as a known issue that a few applications may have trouble
  with borderless full screen and FreeSync while other applications or launchers run on
  the primary screen [hypothesis: seen only in search results on amd.com and
  community.amd.com on 2026-10-31; the pages timed out or redirected, so not read here].
- No AMD document found here speaks of Vulkan and FreeSync in a window separately from
  D3D [hypothesis: borderless support applies to Vulkan too, since Windows composes a
  Vulkan window the same way; unchecked, no AMD card here].

**Summary for players** [hypothesis, from the above]: on NVIDIA, VRR works in borderless
full screen with "windowed and full screen mode" chosen in the control panel (and often
with "full screen only" too); on AMD, FreeSync is reported to support borderless full screen since
2017 drivers. Exclusive full screen is the surer path when borderless does not engage.

## 3. What is left to check

- The owner's screen (RTX 4070): whether G-SYNC engages in borderless full screen and in
  exclusive full screen with Variable refresh chosen (the monitor's OSD refresh counter, or
  the NVIDIA control panel's "G-SYNC Indicator"), and whether it feels smoother than
  Vertical sync in Primm. docs/owner-review.md.
- The present modes this machine's surface offers, from the start-up log line (§1).
