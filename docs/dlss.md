# NVIDIA DLSS in NVForever

DLSS Super Resolution is an optional upscaler beside FSR 3.1: the world is drawn at a lower
resolution and DLSS rebuilds the picture at the window's size, so the game runs faster for a
picture close to native. It is off by default, and NVForever plays the same without it.

DLSS needs:

- Windows;
- an NVIDIA RTX graphics card with a current driver;
- the Vulkan renderer, which is NVForever's default;
- four of NVIDIA's files, which the game can download from NVIDIA's own release once you
  accept NVIDIA's licence, or which you copy yourself (below).

NVForever ships no NVIDIA file. NVIDIA's files come under NVIDIA's own licence terms.

## Getting the files from the launcher

[design: #2765, `crates/nvf-launcher/src/dlss.rs`] The launcher's **Settings > Graphics** starts
with an NVIDIA DLSS card: what DLSS needs and what this PC has (Windows, the graphics card
Windows names, how many of the four upscaling and four frame generation files the `dlss`
folder holds), **Download DLSS…**, **Open dlss folder**, NVIDIA's release page and **Check
again**. Download DLSS… shows NVIDIA's licence first and runs the very download the game's
dialog runs (below; the code is shared, `crates/nvf-dlss-files`): nothing goes online before
**I accept** is clicked with the mouse, the same eight files, pins and signature checks, and
the same `dlss` folder beside `nvforever.exe`. The card shows its progress (Cancel download
stops it with nothing written); the game uses DLSS from its next start.

## Getting the files from the settings: Download DLSS…

[design: `settings.rs`, `nvf-dlss-files` `download/files.rs`] While any of the eight upscaling and frame
generation files is missing, the Upscaler row (pause menu, **Settings > Display**, Upscaling
and Anti-Aliasing) offers
**Download DLSS…** beside Off, FSR 3.1 and DLSS (Windows only, with the Vulkan renderer).
Choosing it changes nothing in the row; it opens a dialog:

1. **NVIDIA's licence.** The dialog says what is fetched and shows NVIDIA's licence for
   `nvngx_dlss.dll` and `nvngx_dlssg.dll`: the RTX SDKs licence in the archive's
   `bin/x64/nvngx_dlss.license.txt` [documented: that licence, DLSS SDK supplement]. It scrolls
   with PgUp, PgDn, the arrow keys, Home, End or the mouse wheel (on a controller: LB, RB and
   the D-pad); **Open on GitHub** opens the release's page in your browser. **Nothing is
   downloaded and the game does not go online before you press I accept.** I accept takes a
   mouse click only, so that no key or button pressed by habit accepts a licence: a player
   using a controller alone needs a mouse for that one click, or gets the files by hand (below).
   Cancel (or Esc, or B and Start on a controller) closes the dialog with nothing done. The
   six `sl.*` files are under Streamline's MIT licence [documented: Streamline 2.14.16,
   `LICENSE.txt`].
2. **The download.** The game fetches only the eight files from the official archive of
   NVIDIA's Streamline release 2.14.16 on GitHub
   (`https://github.com/NVIDIA-RTX/Streamline/releases/download/v2.14.16/streamline-sdk-v2.14.16.zip`):
   it reads the archive's list of files from its end and asks for just those eight entries,
   including the four frame generation files listed below, or only the ones your `dlss` folder
   does not hold yet [design: `nvf-dlss-files` `download/install.rs`, `still_missing`]. The four frame
   generation files are about 4.6 MB of the download [observed: the official zip, 2026-11-13]. The dialog shows the step,
   the file and the megabytes; **Hide** (Esc) closes it and the download goes on while you play (the Upscaler
   row then reads "Downloading DLSS… n%", and choosing it shows the dialog again);
   **Cancel download** stops it. Loading a save, going to the main menu or quitting the game
   stops a download that is still running (each of them starts the game anew), with nothing
   written; choose Download DLSS… again afterwards.
3. **The checks.** Each file must be exactly the file of that release (its size and SHA-256,
   pinned in NVForever), its ZIP CRC-32 must match, and `sl.interposer.dll`, `nvngx_dlss.dll`
   and `nvngx_dlssg.dll` must pass the signature checks described below. The files are unpacked
   into a temporary folder beside the `dlss`
   folder and moved in only when all the fetched files passed; on any failure or cancel nothing is written
   in the `dlss` folder and the temporary folder is removed. `nvngx_update.exe` is never
   written, and a file of yours already in the `dlss` folder is never overwritten.
4. **The result.** "DLSS is ready: restart NVForever to use it." The restart is needed once,
   because NVIDIA's Streamline must be loaded before the game's renderer starts; after it,
   every switch is live (below). If the download failed, the dialog says why (no answer from
   GitHub, an error from GitHub, a file that did not match, a folder that holds a file of its
   own) and offers the manual way: **Open release page**, **Open dlss folder** (made if it is
   not there) and **Try again**.

[design: `settings.rs`, `nvf-dlss-files` `download/install.rs`] If you already have the four upscaling
files, DLSS upscaling keeps working and **Download DLSS…** stays offered for the missing frame
generation files. Accepting again fetches and adds only the files the folder lacks: a file
already there is kept as it is, so your own `nvngx_dlss.dll` of another DLSS version stays.
Restart after adding the files to make frame generation available.

[design: `dlss::frame_generation_unavailable`, `settings::dlss_download_offered`,
`dlss_download::offered`] Beside the dimmed DLSS Frame Generation
choice, missing files read "Files missing: Download DLSS… on the Upscaler row" while that
download is offered. After a successful download in this session they read "Restart to use
DLSS Frame Generation". Otherwise they read "The DLSS Frame Generation files are missing". The download
choice and hints share a one-second cache of the all-eight-file probe, refreshing when the
download phase changes. Other loader refusals and a successful download in this session
suppress the offer before probing. If the folder cannot be resolved, the download stays
offered only when the loader reports missing files, so its dialog can explain the folder error.

The files go into the `dlss` folder beside `nvforever.exe` (or the folder `NVF_DLSS_DIR`
names), never into the game folder. The game downloads only when you choose Download DLSS…
and accept: there are no automatic updates and no checks in the background.

[design] Before creating anything, the download checks its destination and temporary folder
with the install guard, including a destination set through `NVF_DLSS_DIR`. The guard refuses
the game install and the original game's user folders. **Open dlss folder** uses the same
guard; a refusal creates nothing and the log says why
([install layout](design/install-layout.md#rules-enforced-in-code-cratesfnv-install)).

[design] The download's test aids, `NVF_PLAY_TEST_DLSS_DOWNLOAD` (including automatic
acceptance, cancellation and clearing smoke files) and `NVF_DLSS_DOWNLOAD_URL` (a loopback
server), work only in developer runs with `--data` or `NVFOREVER_DATA`. A normal player run
ignores them with one warning in the log.

## Getting the files by hand

1. From NVIDIA's Streamline repository on GitHub
   ([NVIDIA-RTX/Streamline, releases](https://github.com/NVIDIA-RTX/Streamline/releases)),
   download the release **2.14.16**, the archive `streamline-sdk-v2.14.16.zip`. Other
   versions are not tested with NVForever.
2. Open the archive's `bin\x64` folder and take these eight files
   [design: `nvf-dlss-files` `download/files.rs`]:
   - `sl.interposer.dll`
   - `sl.common.dll`
   - `sl.dlss.dll`
   - `nvngx_dlss.dll` (DLSS itself; the version tested is 310.9.16)
   - `sl.dlss_g.dll`
   - `nvngx_dlssg.dll`
   - `sl.reflex.dll`
   - `sl.pcl.dll`
3. Make a folder named `dlss` next to `nvforever.exe` (in the folder NVForever was unpacked
   to, not the game folder) and copy the eight files into it:

   ```
   NVForever\
     nvforever.exe
     dlss\
       sl.interposer.dll
       sl.common.dll
       sl.dlss.dll
       nvngx_dlss.dll
       sl.dlss_g.dll
       nvngx_dlssg.dll
       sl.reflex.dll
       sl.pcl.dll
   ```

[design: `dlss::FILES`, `dlss::FG_FILES`] The first four enable DLSS Super Resolution;
the last four add DLSS Frame Generation. Super Resolution works without those last four.
`NvLowLatencyVk.dll` is not downloaded: Reflex uses the driver's `VK_NV_low_latency2` and
loads that file only as a fallback [documented: #2317; Streamline 2.14.16 Reflex's
`reflexEntry.cpp` and `vknvll2.cpp`; see
[Streamline notes](spec-notes/streamline.md#dlss-frame-generation)].

Copy only these files. **Do not copy `nvngx_update.exe`**: NVForever refuses to use a `dlss`
folder that holds a file of that name, because NVIDIA's code would start it without checking
it.

### What is checked, and by whom

- **NVForever** checks `sl.interposer.dll` before it loads it: it must carry NVIDIA's
  signature, and it must be exactly the file of Streamline 2.14.16 (its SHA-256 is compared).
  Anything it imports is looked up only in the `dlss` folder and in Windows' System32 folder.
- **NVForever** also checks every other DLL in the `dlss` folder before Streamline starts,
  because Streamline loads every `sl.*.dll` it finds there: each `sl.*.dll` (and
  `NvLowLatencyVk.dll`) must be exactly the file of Streamline 2.14.16's `bin\x64`, compared by
  SHA-256, and the only other DLLs allowed are NVIDIA's `nvngx_*.dll`, each of which must be
  signed by NVIDIA Corporation (Windows checks the signature), so a newer DLSS from NVIDIA works.
  So copying the whole `bin\x64` folder works too, while any other DLL in the folder keeps DLSS
  off.
- **Kept as checked.** From its check until the game quits, NVForever keeps every checked file
  in the `dlss` folder open so that no other program can change, rename or delete it: the files'
  contents cannot be swapped while the game runs. The `dlss` folder must be a real folder and
  each DLL in it a real file, not a link (a symbolic link or junction), or DLSS stays off. The
  folder is your own, beside `nvforever.exe` and outside the game folder; the folders above it
  are not checked.
- **NVIDIA's Streamline** then checks the plugins it loads (`sl.common.dll`, `sl.dlss.dll`) for
  NVIDIA's signature and refuses unsigned ones. The game's log shows it, in lines such as
  "File 'sl.common.dll' is signed by NVIDIA and the signature was verified".
- **NVIDIA's NGX**, part of the driver, loads `nvngx_dlss.dll`. NVForever does not compare its
  hash, so a newer DLSS can be used, but checks that it is signed by NVIDIA Corporation (above).
  Whether NGX checks that file itself is not shown in the log, so this page makes no claim about
  it.

### Other tools that change the upscaler

Tools that replace or hook the game's upscaler themselves (OptiScaler, for example) should not
be used together with DLSS through Streamline: both reach into the same place, and that is
untested. With the frame generation files present, NVIDIA's latency reduction goes through
Streamline's Reflex (`sl.reflex.dll`) rather than its own calls; without them it is unchanged.

## Turning it on

In the pause menu, **Settings > Display** (Upscaling and Anti-Aliasing), set **Upscaler** to
**DLSS**, and
**Upscaling mode** to Native AA (DLSS at full resolution, also called DLAA), Quality,
Balanced, Performance or Ultra Performance. Each mode shows the share of the screen's width and
height the world is drawn at, for example Quality (67%): Native AA for full resolution with
anti-aliasing only, Quality for the sharpest upscaled look (1440p and 4K), Balanced for more
speed with little visible loss, Performance for the highest frame rate (4K or heavy scenes),
Ultra Performance for very high resolutions only. The mode row serves FSR 3.1 the same way
(its shares differ a little: Balanced is 59% with FSR, 58% with DLSS); it is dimmed while
Upscaler is Off. (It is not the Quality preset row, Low to Very High: it only
says how much smaller the world is drawn before the upscaler rebuilds it.)

With the four files in the `dlss` folder when the game starts, every change takes effect at
once: Off, FSR 3.1 and DLSS, and every mode. Files put into the folder while the game runs
are found at the next start: until then a DLSS choice reads **DLSS (restart)** and the picture
stays as it was. That holds too when the game started with DLSS chosen and the files missing:
once all four are in the folder, DLSS is no longer greyed out and reads DLSS (restart). Loading
a save starts the game again too.

On the command line, `--upscaler dlss --upscaler-quality balanced` (or `native-aa`,
`quality`, `performance`, `ultra-performance`) chooses DLSS; as with every setting, a choice
saved in the settings wins in normal play.

To see what draws, turn on the Frame rate counter and its **Counter: upscaling** row
(**Settings > Display > Performance**): it names the upscaler that actually draws, its mode,
the size the world is drawn at and the screen's size, and the DLSS model chosen, for example
`DLSS Quality  853x480 -> 1280x720  model Default`; a DLSS choice that cannot run reads `Off`
and the window's size. When DLSS stops working during play, the game draws on without it for
about 60 frames before giving DLSS up for the session; the row reads `Off` from then on.

## The DLSS model

Under Upscaling mode, **DLSS model** chooses which of NVIDIA's DLSS models rebuilds the
picture. NVIDIA calls these models presets and names them by letters. The row can be set while
DLSS draws (it is dimmed with "DLSS only" otherwise) and takes effect at once.

| Choice | What it is |
|---|---|
| Default (recommended) | NVIDIA picks for each mode. With NVIDIA's DLSS 310.9.16 that was K for Native AA, Quality and Balanced, M for Performance and L for Ultra Performance; a newer DLSS may pick others. |
| K | NVIDIA's transformer model with the most detail, at a higher cost in frame rate. |
| L | Sharper and steadier than J and K, with less ghosting; the dearest of the four. |
| M | Close to L in looks, closer to J and K in speed. |
| J | Like K, with a little less ghosting and a little more flicker. |

The letters are the ones NVIDIA gives these presets. NVIDIA's other presets are left out: some
are withdrawn and the rest fall back to the default. On the command line: `--dlss-model default`
(or `k`, `l`, `m`, `j`). It is saved as `dlss-model=` in `NVForever.ini` (`dlss-model=K`; the
earlier spelling `dlss-model=Preset K` still loads); a value that is not one of the five loads
as Default.

While DLSS is on:

- the anti-aliasing row reads "Off (upscaling active)": DLSS does the anti-aliasing, and the
  saved anti-aliasing comes back when Upscaler is Off;
- the Sharpening switch drives NVForever's ordinary sharpening pass;
- FSR 3.1's frame generation is not available (its choice reads "Not with DLSS"); DLSS Frame
  Generation is (below).

Whenever the files are in the `dlss` folder, NVIDIA's driver may update DLSS in the background,
as it does for other games that use DLSS, whichever upscaler is chosen: NVForever loads NVIDIA's
Streamline at every start the files are there, so that DLSS can be switched to without a
restart. NVForever cannot switch the driver's updates off. With the files there, FSR 3.1 runs
through Streamline too; remove the folder to keep NVIDIA's code out altogether.

## DLSS Frame Generation

With the four frame generation files in the `dlss` folder as well (above), **Settings >
Graphics > Frame generation** offers **DLSS**: NVIDIA's DLSS Frame Generation makes frames
between the real ones, and **Generated frames** below it chooses how many: 2x (one per real
frame), 3x or 4x (two or three, where the graphics card offers them: the RTX 50 series). On the
command line: `--frame-generation dlss`. Switching it on or off takes effect at once (the screen
may blink once as the game remakes its swapchain).

It needs:

- an NVIDIA RTX 40 series or newer graphics card with a current driver;
- Windows' **hardware-accelerated GPU scheduling** turned on (Windows Settings > System >
  Display > Graphics > Change default graphics settings), then a restart of Windows;
- an upscaler: FSR 3.1 or DLSS, any mode (frame generation reads the upscaler's depth and
  motion);
- **Display sync: Off or Variable refresh**: on Vulkan, NVIDIA's frame generation does not run
  with vertical sync. With Variable refresh the game turns vsync off while it runs and keeps the
  frames a few under the screen's refresh rate (through NVIDIA's Reflex), so a G-Sync screen
  shows each frame as it comes; that is the choice for a G-Sync screen. A frame now and then
  still comes in a little early and tears near the bottom of the screen; to stop that, set
  **Vertical sync: On** for the game in the NVIDIA Control Panel (Manage 3D settings >
  Program settings > `nvforever.exe`). The driver's vsync works with frame generation where
  the game's own cannot;
- the game's window in front: while another window is in front, no frames are generated.

When it cannot run, the DLSS choice is greyed out with the reason: "The DLSS Frame Generation
files are missing", "Needs an NVIDIA RTX 40 series or newer", "Needs hardware-accelerated GPU
scheduling", "Needs a newer NVIDIA driver", "Not on this GPU", "Needs an upscaler" or "Needs
vsync off". Latency reduction turns on with it, through NVIDIA's Reflex. The HUD and menus are
not generated; menus and loading screens show real frames only. The performance overlay's frame
rate shows the frames shown and, in brackets, the real ones. The log's `DLSS-G:` lines say what
happened.

The **Frame rate limit** counts the frames shown: with frame generation on, the whole limit goes
to NVIDIA Reflex, which holds the frames shown, generated ones included, so real frames come at
the limit divided by the frames shown for each (144 at 2x: 72 real frames a second). In menus,
and whenever no frames are being generated (another window in front), real frames get the whole
limit. A limit divided that far that fewer than 60 real frames a second are left (144 at 4x: 36)
can feel choppy; the Generated frames row then says so. Fewer generated frames, or a limit nearer
the screen's refresh rate, give more real frames.

## When DLSS is greyed out

When DLSS cannot run, the Upscaler row's DLSS entry is greyed out with the reason. If DLSS is
the upscaler you chose, the game then draws without upscaling; an FSR 3.1 choice keeps working
as before, and FSR 3.1 or Off can still be chosen, taking effect at once. A DLSS choice already
saved stays saved, so it is tried again at every start: fix what the reason says and start the
game again. The log
(`Documents\My Games\NVForever\logs`) has the details in its lines starting `DLSS:`.

| Reason shown | What it means |
|---|---|
| DLSS needs Windows | DLSS runs on Windows only. Elsewhere, FSR 3.1 is the upscaler. |
| DLSS needs the Vulkan renderer | The game was started with another renderer (the `WGPU_BACKEND` variable, for example `dx12`). Start it with Vulkan, the default. |
| DLSS files missing: choose Download DLSS… in this list, or put them in the dlss folder beside nvforever.exe, then restart | The `dlss` folder is not next to `nvforever.exe`, or one of the four files is not in it. The log names the missing files. Download DLSS… in the same list gets them (above), or copy them by hand. Once all four are there, DLSS reads DLSS (restart). |
| The dlss folder holds NVIDIA's updater (nvngx_update.exe): remove it, then restart | Delete `nvngx_update.exe` from the `dlss` folder. This is also shown when the folder could not be read. |
| A file in the dlss folder is not from the tested NVIDIA release (Streamline 2.14.16) | A DLL in the `dlss` folder did not pass NVForever's checks. Every `sl.*.dll` (and `NvLowLatencyVk.dll`) must be exactly the file of Streamline 2.14.16's `bin\x64`, and `sl.interposer.dll` must also carry NVIDIA's signature; besides those, only NVIDIA's `nvngx_*.dll` files may be there, signed by NVIDIA Corporation. Another version, a changed file, an `nvngx_*.dll` without NVIDIA's signature, a DLL that is a link, or any other DLL is refused. The log names the file, and its SHA-256 or signer. Download release 2.14.16 again from NVIDIA's page, copy its files unchanged, and remove any other DLL from the folder. |
| DLSS did not start (the log says why) | NVIDIA's Streamline would not load or start. The log's `DLSS:` lines say where it stopped. This is also shown when the `dlss` folder itself is a link (a symbolic link or junction) rather than a real folder, or a DLL in it could not be read. |
| A graphics card the game may use cannot run DLSS | A graphics card Vulkan can see lacks what DLSS needs. This is typical on laptops whose built-in graphics also show up in Vulkan beside the NVIDIA card: the game checks every card Vulkan shows, since it could end up starting on any of them. DLSS stays off rather than risk a crash at start. The log names the card. |
| Not on this GPU or driver (DLSS needs an NVIDIA RTX card) | DLSS does not run on this graphics card or driver. It needs an NVIDIA RTX card; updating the driver may help. |
| DLSS stopped working this session; it is tried again at the next start | DLSS gave no picture for a while in this session. For the rest of the session a DLSS choice draws without upscaling; FSR 3.1 and Off can still be chosen and take effect at once. DLSS is tried again at the next start. |

## NVIDIA's attribution

While the four files are in the `dlss` folder (downloaded or copied by hand), the end credits
finish with a short plain-text line naming NVIDIA DLSS: upscaling by NVIDIA Corporation, through
NVIDIA Streamline. NVIDIA's licence asks for attribution in the credits (its supplement,
section 7.1(b)). The licence also says that NVIDIA's marks it names (NVIDIA RTX, GeForce RTX and
Quadro RTX) and NVIDIA's logos need NVIDIA's prior written approval (section 7.2(c)); NVForever
has no such approval and uses none of them, in the game or in its pages beyond naming them here.

## Removing it

Set Upscaler to Off or to FSR 3.1, quit the game, then delete the `dlss` folder. If the folder
is deleted while DLSS is still chosen, the next start draws without upscaling and DLSS reads
"DLSS files missing".

## Settings saved by an earlier build

Earlier builds kept the upscaler in one row, `fsr-upscaling` in `NVForever.ini` (for example
`fsr-upscaling=DLSS Balanced` or `fsr-upscaling=Quality`). Such a line still loads into the two
rows; once a row is changed, the two rows are saved as `upscaler=` and `upscaler-quality=`, and
those win over the older line.

How DLSS is built into NVForever: [design/dlss-upscaling.md](design/dlss-upscaling.md).
