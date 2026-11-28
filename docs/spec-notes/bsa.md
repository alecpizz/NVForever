# Spec note: BSA archives (version 104, Fallout 3 / New Vegas)

Clean-room description in plain language. Implementation (`crates/fnv-bsa`) is written
from this note. Confidence tags: **[documented]** cites a public source, **[observed]**
was checked against the user's own install (FNV 1.4.0.525, Steam), **[hypothesis]**
needs verification.

Sources: UESP "Oblivion Mod:BSA File Format" and "Skyrim Mod:Archive File Format"
(content2.uesp.net mirror). Observations: all 15 archives under 400 MB in the user's
`Data` folder (15,072 files).

## Layout

All integers little-endian.

1. **Header, 36 bytes** [documented][observed]
   - magic `BSA\0`
   - version u32 = 104 in every FNV archive [observed]
   - offset u32 = 36 (start of folder records) [observed]
   - archive flags u32 (see below)
   - folder count u32, file count u32
   - total folder name length u32: sum over folders of (name length + 1) — the +1 is the
     terminating NUL; the length *byte* of each folder name is not counted [observed]
   - total file name length u32: size of the file-name block [observed]
   - file flags u16 (content type bits) + 2 bytes that are **not always zero**
     (`DeadMoney - Sounds.bsa` has 0x0378, `LonesomeRoad - Sounds.bsa` 0x02F9) —
     preserve them [observed]
2. **Folder records**, 16 bytes each, sorted ascending by hash [observed]:
   hash u64, file count u32, offset u32. The offset equals the absolute position of this
   folder's file-record block **plus the total file name length** [observed: true for
   every folder in every shipped archive].
3. **File-record blocks**, one per folder, in folder order. Each starts (if archive flag
   0x1) with the folder path as a length-prefixed string: one length byte that *includes*
   the trailing NUL, then the bytes, then NUL [observed]. Then `count` file records of
   16 bytes, sorted ascending by hash: hash u64, size u32, offset u32 (absolute) [observed].
   - size bit 30 (0x40000000) inverts the archive's default compression for that file
     [documented]; bits 0–29 are the stored size. Bit 31 never set in the corpus [observed].
4. **File-name block** (if archive flag 0x2): every file name (no directory), each
   NUL-terminated, in the same order as the file records [observed].
5. **File data**, immediately after the name block, in record order with no gaps
   [observed: true for 18 of the 21 archives in the owner's install; the exceptions are
   listed under Anomalies]. The writer packs blocks in record order; `rewrite` keeps an
   archive's own block order and name-block tail so the exceptions still rebuild.

## Archive flags

| Bit | Meaning | Status |
|---|---|---|
| 0x1 | folder names present | [documented][observed] |
| 0x2 | file names present | [documented][observed] |
| 0x4 | files compressed by default | [documented][observed: Misc archive] |
| 0x8, 0x10, 0x20 | "retain names/offsets" hints | [documented], no layout effect seen |
| 0x40 | Xbox 360 (big-endian) | [documented]; not supported |
| 0x80 | set on all mesh/texture-type archives (`0x83`) | meaning unknown; **not** "embedded names": data in those archives starts directly with file contents [observed] |
| 0x100 | file data prefixed by the file's full path as a u8-length string (no NUL), before any compression header | [documented]; set (flags 0x107) on `Fallout - Textures.bsa` and `Fallout - Textures2.bsa`, where all 22,054 files carry their own path [observed 2026-10-07 on the full install; an earlier note said "never set", written before those two archives were available] |

## Compressed file data

A u32 uncompressed size, then a zlib stream [observed]. Two tolerated irregularities:
- `Fallout - Misc.bsa`, `menus\s.txt`: the zlib stream has **no Adler-32 trailer**
  (stored size 12 = 4 + 8). The deflate data is complete and yields the declared 4 bytes.
  The original game must therefore not require the trailer [observed; game behaviour
  hypothesis].
- `menus\do_not_delete.txt`: compressed, declared size 0, no zlib data at all (block is
  just the 4-byte size) [observed].
- **Bytes after the Adler-32**: none in any shipped archive. `fnvtool bsa verify` over the
  22 archives of the base game, the add-ons and the packs (about 181,000 files) reported
  no `zlib_bytes_after_trailer` [observed 2026-10-31, #998]. The reader accepts such
  bytes (`fnv_inflate::Trailer::Extra`) and the verify command reports them.

## Name hash

64-bit value from a lower-cased, backslash-normalised name [documented: UESP BSA page
describes the algorithm; verified on every file and folder name in the corpus, observed]:

- For files, split off the extension at the last `.`; folders have no extension.
- Low 32 bits: last char of the stem | (second-to-last char << 8, only if stem length > 2)
  | (stem length << 16) | (first char << 24). Then OR a marker for four extensions:
  `.kf` 0x80, `.nif` 0x8000, `.dds` 0x8080, `.wav` 0x80000000.
- High 32 bits: rolling value `h = h * 0x1003F + c` (mod 2^32) over the stem's characters
  from index 1 up to (length − 2) exclusive, plus the same rolling value computed over the
  extension (including the dot), summed mod 2^32.

## Version 103 (Oblivion's archives)

The original game also loads version 103 archives, which some mods ship [observed by the
owner, #2092]. NVForever reads them by these rules (`fnv_bsa::VERSION_103`); no v103
archive was at hand to check them against (none of the 63 archives on the maintainer's
machine is v103, 2026-11-12), so each rests on the public pages:

- **Same layout as 104.** The UESP Oblivion page gives the same 36-byte header (magic,
  version 103, offset 36, archive flags, folder and file counts, the two name lengths,
  content-type flags), the same 16-byte folder records (hash, file count, an offset that
  includes the total file name length), the same file-record blocks (a folder name with a
  length byte when flag 0x1, then 16-byte file records: hash, size with bit 30 inverting
  the default compression, absolute offset), the same file-name block and the same sort
  by hash. The Skyrim page calls the two formats "nearly identical" and names 104 for
  Fallout 3 and New Vegas, 103 for Oblivion [documented: UESP "Oblivion Mod:BSA File
  Format", "Skyrim Mod:Archive File Format", read 2026-11-12].
- **Compressed blocks:** a u32 uncompressed size, then zlib data, as in 104 [documented:
  the Oblivion page].
- **Flags 0x100, 0x200, 0x400:** the Oblivion page's flag table marks all three "set in
  all official BSA files", 0x100 and 0x200 with "a different meaning for BSA v104", and
  the footnotes say such bits have no effect on the file structure for 0x8/0x10/0x20/0x80
  only [documented]. The Skyrim page gives 0x100 as "Embed File Names" (each block starts
  with its path) for 104 [documented]. NVForever therefore never looks for a path before
  a v103 block, and knows 0x200 and 0x400 in a v103 archive (no warning, kept on rewrite)
  [hypothesis: the Oblivion page's file-block table still says the path is "only present
  if bit 9 of the archive flags is set", which contradicts its own flag table; with a
  path stripped, every official v103 archive, all of which set 0x100, would read wrong,
  so the flag table is taken. Confirm on a real v103 archive: `fnvtool bsa verify` and the
  `bsa-version-103` smoke scenario with `NVF_BSA_103` set].
- **Flag 0x40** means big-endian (Xbox 360) in 103 as in 104 [documented]; not supported.
- **Name hash:** the same algorithm [hypothesis: the Oblivion page links the same Hash
  Calculation page; a mismatch would only warn, files are found by name].

A v103 archive is rewritten as 103 (`rewrite`, `fnvtool bsa verify`); the writer's own
archives are 104. Version 105 (Skyrim SE, wider folder records and LZ4) and any other
version are refused with `unsupported BSA version N (expected 104, or Oblivion's 103)`.

## Anomalies

- `Fallout - Invalidation.bsa` (83 bytes): its folder name has length byte 0 (no NUL,
  while the header's total folder name length is 1), its only file's data offset points 3
  bytes past the end of the name block, and 1 byte follows the (zero-length) data
  [observed]. It is probably generated by a mod manager for archive invalidation rather
  than shipped by Bethesda [hypothesis: its modification time differs from the shipped
  archives']. It parses, the reader reports `noncanonical_folder_name`,
  `data_not_contiguous` and `trailing_bytes`, and it cannot be rebuilt byte-identically.

- `Fallout - Textures.bsa` (shipped, 1,196,767,196 bytes, 11,470 files): the data blocks
  are contiguous and fill the file exactly, but are **not stored in record order** (the
  first record's block is at 324,286,810) [observed: `fnvtool bsa verify` on the owner's
  install, 2026-10-06]. The order actually used is unknown [hypothesis: the packing
  tool's input order]. The reader reports `data_not_in_record_order`.
- `Fallout - Voices1.bsa` (shipped, 1,602,331,889 bytes, 105,517 files): the file-name
  block is **105 bytes longer** than the names it holds; `total_file_name_length` and
  every folder offset count those bytes, and the data starts after them [observed: same
  run; the rebuilt header differed at offset 28 and the rebuild was 105 bytes short].
  Their content is not yet known (the reader's `name_block_slack` warning says whether
  they are all NUL).

## Unknowns (need the user's help)

- Archives over 400 MB (`Fallout - Meshes/Textures/Textures2/Sound/Voices1`,
  `LonesomeRoad - Main`, `OldWorldBlues - Main`) could not be transferred. Their listings
  were validated on the owner's machine on 2026-10-06 (all match `oracle_bsa.py`); see
  Anomalies for the two that did not rebuild with the canonical rule.
- Loose-file precedence (see Precedence below) and the order of `Update.bsa` against the
  DLC archives.
- Meaning of archive flag 0x80 and of file-flag padding bytes.

## Precedence (the same path in several archives)

Settings [observed]: `Fallout_default.ini`, the owner's `Fallout.ini` and
`FalloutPrefs.ini` all have `SArchiveList=Fallout - Textures.bsa, Fallout - Textures2.bsa,
Fallout - Meshes.bsa, Fallout - Voices1.bsa, Fallout - Sound.bsa, Fallout - Misc.bsa`,
`bInvalidateOlderFiles=1`, `SInvalidationFile=ArchiveInvalidation.txt`, plus
`bUseArchives=1`, `bCheckRuntimeCollisions=0` and three `iRetain…Table=1` lines. `Update.bsa` is
not in the list, and no plugin is named `Update`; the game uses it all the same (below).

Survey [observed 2026-10-09, `fnvtool bsa list` of all 22 archives with content hashes]:
420 paths are present in more than one archive, 300 of them with different contents.
`Update.bsa` replaces 35 files of `Fallout - Meshes.bsa` and 2 of `Fallout - Textures2.bsa`;
the DLC `<Plugin> - Main.bsa` archives replace base meshes too (e.g. `OldWorldBlues -
Main.bsa` has its own `architecture\nellishangar\nvnellishangar.nif`, using
`nellis afb\nellishanger_d.dds` instead of the base copy's Citadel wall textures and
window band). Many differences are collision-only (Havok blocks) and invisible.

Test [observed 2026-10-09, plugin v7 screenshots from save `nvftest`, 1280 × 720, noon,
NVWastelandClear; `nvforever` rendered each recorded camera twice with the archive
order replaced through `NVF_ARCHIVE_ORDER`; compared over the pixels where the two
renders differ (mean absolute sRGB difference to the game, lower is closer; SSIM of that
region, higher is closer)]:

| Camera (object) | Hypothesis A (game order below) | Hypothesis B | A | B |
|---|---|---|---|---|
| Nellis hangar, `nvnellishangar.nif` | `OldWorldBlues - Main` over `Fallout - Meshes` | base over DLC | 15.7 / 0.716 | 24.4 / 0.680 |
| Camp Forlorn Hope tower, `ncr\ncr_guardtower01.nif` | `Update` over `Fallout - Meshes` | base over `Update` | 29.7 / 0.577 | 60.6 / 0.561 |
| Westside, `freeside\atomicwranglerext.nif` | `Update` over `Fallout - Meshes` | base over `Update` | 6.8 / 0.777 | 21.1 / 0.741 |

All three favour A. Rule used by `crates/fnv-vfs` (lowest precedence first):
1. the `SArchiveList` archives in list order [the order among them never matters for
   the shipped data: the six share no path, observed]. The game's own INI is never
   read (install-layout.md): the list is the shipped one (`fnv_vfs::DEFAULT_ARCHIVE_LIST`)
   unless NVForever's own `NVForever.ini` sets `[Archive] SArchiveList` (the original's
   section and key), which replaces it for the run, logged as `vfs: SArchiveList from
   <file>` (#1142; a name not in Data is left out with a warning, and
   `NVF_ARCHIVE_ORDER` still overrides the whole order). `fnvtool vfs order
   --archive-list` takes the same list (smoke `vfs-order-archive-list` checks the
   order); reading the key from the INI is checked by the unit test
   `the_inis_archive_list_replaces_the_default_only_when_set` [design: for mod setups
   that add base archives];
2. `Update.bsa` — **overrides the base archives** [observed, two objects above];
3. each active plugin's own archives (`<plugin stem> - *.bsa`, `<plugin stem>.bsa`) in
   load order — a **DLC archive overrides the base archives** [observed, Nellis hangar].
   An archive that two loaded plugins' stems both match (`Foo - Bar.bsa` for `Foo.esp`
   and `Foo - Bar.esp`) goes with the longer stem, so it takes `Foo - Bar.esp`'s place
   in the load order (#996) [hypothesis: no source says how the original picks; the
   shipped data has no such pair (every archive matches one plugin), so the rule changes
   nothing for an unmodded install, `vfs-order-shipped`; chosen because the longer name
   is the more specific claim].
   A plugin also owns the numbered pieces `<plugin stem>N.bsa` and `<plugin stem>N - *.bsa`
   (N decimal digits), after its unnumbered archives, in number order (#2521). [observed]
   the Begin Again mod list ships `Begin Again Worldspace Merge.bsa` with
   `…Merge1.bsa` to `…Merge6.bsa` beside its one plugin `Begin Again Worldspace Merge.esp`,
   and the textures its rock meshes use (`textures\landscape\nv_rocks\nv_rockformations_04_d.dds`
   and others) are only in `…Merge3.bsa` and `…Merge4.bsa`; without the pieces those rocks
   drew untextured. [hypothesis] that the original mounts them: the list is built for the
   original game; no source states its rule. A name that only begins with the stem
   (`MergeExtra.bsa`) is not claimed, and the longest-stem rule above still applies
   (`Merge3.bsa` is `Merge3.esp`'s when that plugin is loaded);
   A plugin also owns space-suffixed archives `<plugin stem> *.bsa` and `<plugin stem>N *.bsa`
   (of which `<plugin stem> - *.bsa` is a special case), in case-insensitive name order
   (#3756). [observed: player-supplied report on #3756, chapter 7] the Nuclear Sunset modlist
   ships space-suffixed archives such as `NS A Content T 00.bsa` beside plugin `NS.esp`, and
   mounting them restored missing meshes and textures. [hypothesis] that the original game or
   MO2 profile mounts these via space-delimited prefix ownership (which original-game profile
   or load-order file supplies their owner and exact order remains open on #3756); files
   sharing the short prefix without a space boundary (`NSExtra.bsa`, `NS_Other.bsa`,
   `NS-Other.bsa`) stay unclaimed, the longest-stem rule still assigns `NS A Content T 00.bsa`
   to `NS A Content.esp` when both are loaded, and an archive already listed explicitly in
   `SArchiveList` or MO2's `archives.txt` keeps its explicit position without being mounted a
   second time in either flat or layered discovery;
4. loose files [hypothesis: no loose mesh or texture exists in the owner's `Data`
   (only music, radio songs, shader packages and the intro video), and the project never
   writes into the game folder, so this stays untested; `bInvalidateOlderFiles=1` is
   commonly said to make loose files win [hypothesis]].

File timestamps do not decide it: `Update.bsa` and the DLC archives are dated July 2025,
older than the base archives' modification times (6 October 2026) [observed],
and still override them. Still [hypothesis]: `Update.bsa` before the DLC archives (they
share only `characters\_1stperson\1hpaim.kf` with different contents, not testable by a
screenshot) and DLC against DLC in load order (only DLC-worldspace files differ).
`Fallout - Invalidation.bsa` is in no list and named after no plugin; the VFS ignores it
[hypothesis that the game does too].


## NVForever home loose files

Implementation context at pinned source `6692d2da4612fa4aae9f0a9eea177452ba511c4e`:
`EngineHome::mods` in `crates/fnv-install/src/lib.rs`, `mod_file_roots` in
`engine/nvf-play/src/script_input.rs`, and `docs/design/install-layout.md` describe
NVForever's own `mods` folder
is a read-only input layer above the game's Data. The script file reader already
searches those loose files before Data. `Vfs::open_for_plugins_in` receives this
folder, but previously used it only to discover plugin archives; the asset reader
therefore could not read a loose asset that the script file reader could find.

[hypothesis: NVForever layout policy, not native multi-directory parity] Index
Data loose files, then the explicitly supplied own-mods loose root. Keep the
existing archive selection and precedence. `data_dir()` still names game Data;
`source_of` reports `loose` for either layer, and `loose_path` names the actual
winning file with its filesystem spelling. `read_archived` keeps the highest
archive copy underneath both loose layers, rather than treating a hidden Data
loose file as an archive. With no mods root, single-root behavior is retained.
Missing optional roots add no layer; other traversal failures use the existing
skipped-folder diagnostics. An indexed winner that later cannot be read returns
its read error, without silently falling back to hidden bytes.

[hypothesis: retained snapshot policy] Opening indexes the available paths once.
Installing a new file requires a new VFS; this change adds no filesystem polling,
cache invalidation, extra archive discovery, or writes to either input root.
Top-level files remain outside the asset namespace.

Authored portable controls cover home-only mixed-case paths, precedence across
both loose layers and archives, archived fallback, absent/removed layers,
single-root controls, top-level exclusions, read errors, and reopening after a
new file appears. Index lifetime does not freeze an indexed file's bytes: a
later read still opens its winning path. The same-root control preserves the
existing archive rule that an explicitly supplied mods copy has a full-path
archive name, even when both roots name Data.

These controls validate VFS path selection and error behavior. They do not by
themselves establish asset decoding, a live animation consumer or rendering.
Exact-head workspace checks and those consumer checks are tracked separately;
no complete original-mod or native opcode compatibility follows from this leaf.
