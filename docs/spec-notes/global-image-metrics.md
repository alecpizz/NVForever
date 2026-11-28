# Loaded global-image metrics

[documented: GECK Wiki, Tutorial: Understanding XML, sections 3.2(b) and
3.2(d), https://geckwiki.com/index.php?title=Tutorial%3A_Understanding_XML,
read 2026-11-14] An image filename is relative to Data/Textures. Numeric zoom
is a percentage; zero hides the image and -1 stretches it to its element.
The engine supplies read-only filewidth/fileheight after zoom, with zero for
stretch zoom. The visibility property controls display and input. This source
does not establish when hidden global-image metadata is evaluated or readable.

Consumer context from private installed XML metadata inspection (no mod text
transcribed):
A loaded globals subtree contains a hidden own-image calibration. A global
expression divides a fixed logical metric by its fileheight, and menu image
zooms depend on that result. This is a consumer data dependency, not a native
rendering measurement or proof that a full Pip-Boy figure is fixed.

[documented: NVForever public source at
576d39630287845c598914d17ddffd71eebbef28,
engine/nvf-play/src/tiles/mod.rs and its page-specific image-metric writers]
The existing writers visit their open menus rather than the loaded globals
image. The UI expression evaluator retains the numerator on division by zero
(crates/fnv-ui/src/tree.rs::apply, OpKind::Div). In that source path the missing metric can therefore
produce a much larger dependent zoom. The hand-built CND density consumer in
PR #2475 is separate.

[observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24,
`GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated staging copy,
read-only, committed probe `w4g_09_global_image_metrics_missing` in
`tools/capture/nvse/catalogs/wave4g_f2_extenders.json` (companion catalog PR #3803);
no DLL or source consulted] On injection, a visible `<image>` tile populated
`filewidth`/`fileheight` immediately (`64.0x64.0`), while a hidden
`<visible>0</visible>` `<image>` tile started at `-999.0x-999.0` and settled to
`filewidth = 64.0` on a subsequent UI tick (`M054`). Initializing a tile with or
setting a tile's `filename` to a missing DDS (`interface\__w4g_missing__.dds`) resolved
`filewidth` to `256.0` on BOTH visible and hidden `<image>` tiles (neither `0.0` nor
the stale `64.0`). Initializing with `<filename></filename>` or setting `filename` to
empty `""` (`SetUIStringEx "%z" ""`) substituted `Interface\Shared\empty.dds` with
`filewidth = 1024.0` (not `0.0`). Verbatim printed probe output (4 lines):
- `inj(okV=1,vis=64.0x64.0|okH=1,hid=-999.0x-999.0)`
- `settled(vis=64.0,hid=64.0,initMiss=256.0,initEmp=1024.0)`
- `missNext(vis=256.0,hid=256.0)`
- `trueEmpNext(fn=[Interface\Shared\empty.dds],vis=1024.0)`

[hypothesis] Because `w4g_09_global_image_metrics_missing` in the committed catalog
prints `filewidth` only for the settled, missing-DDS, and empty-filename steps and does
not include a post-missing restore step, NVForever assumes square fallback height
(`fileheight == filewidth`: `256.0` for missing DDS and `1024.0` for empty `""` /
`Interface\Shared\empty.dds`) and that restoring a valid DDS after a missing path
updates both `filewidth` and `fileheight` back to the valid texture's real dimensions.

[design] Before the existing post-script expression update and late page settle, write
own-image metrics throughout the loaded globals subtree, including hidden images. Reuse
the actual FontKit/Pictures/VFS own-image decoder without guessing a mod's desired zoom
or replacing its authored expressions. Match the observed original-game fallback rules:
an empty `filename` substitutes `Interface\Shared\empty.dds` and resolves via VFS (with
`1024.0x1024.0` fallback when absent from synthetic VFS fixtures), and a missing DDS
resolves to `256.0x256.0` (scaled by positive finite zoom) instead of `0.0` or stale
dimensions. Nonempty-atlas-tagged and non-global images are unchanged. Empty/whitespace
atlas values select an own file, matching the public painter. Unchanged numeric metrics
make no write. This does not impose a new general atlas-density rule.

[hypothesis: engineering safety and cache policy] Bound the new work to 4096
visited global nodes and 64 image children. Admit the complete traversal before
image reads or metadata writes. Log coarse resolution/unavailable/limit changes.
Cache only currently referenced filenames within that bounded tree; a rebuilt
globals TileId clears cached dimensions. Same-name external asset replacement
without UI reload and native rendering parity are outside this cache contract.
Symbolic/nonpositive/nonfinite zoom produces zero dimensions; finite products
are required. These bounds are engineering choices, not measured native limits.

[hypothesis: resource admission policy] Without GameData there is no metric read,
write or negative cache entry. Later insertion is admitted normally; a changed
GameData resource clears cached dimensions before resolution. The scheduled
fixture first has UI without data, then inserts actual authored VFS data under
the same UI identity. This differs from a decoded missing image.

Implementation coverage is exercised by authored Rust fixtures for global
expressions and actual child `Piece`/placement geometry; filename changes;
zoom and invalid products; UI lifetime/no-op writes; full image/node admission;
atlas/ordinary neighbors; and the real scheduled FontKit/VFS read before
expression settle. These are repository implementation controls, not an
observation of original-game or native timing. Private process history belongs
in audit artifacts and the PR validation record.

[observed: superseded zero-metrics tradeoff, `w4g_09_global_image_metrics_missing`,
2026-11-24] Earlier drafts published `0.0` for missing or empty filenames; the
original-game probe above supersedes that assumption (`filewidth = 256.0` for missing
DDS and `Interface\Shared\empty.dds` with `filewidth = 1024.0` for empty `""`; square
`fileheight == filewidth` is `[hypothesis]`). Invalid/nonpositive/nonfinite zoom (`0`,
`-1`, `NaN`, `Inf`, symbolic) and nonfinite scaled products still publish `0.0`
[documented: GECK Wiki 3.2(d), "zero for stretch zoom"; design: finite guard]. This
leaf does not clamp consuming expressions, force a verifier, change `Div` semantics or
claim that a complete popup/Pip-Boy is repaired.

[hypothesis: writer ownership remains unchanged in this follow-up] The producer
continues to compare the current numeric trait and restore decoded metadata
when it differs. `Ui::write_changed` has a distinct existing `code_written`
policy: an unchanged producer value does not overwrite a later script write.
Switching between those policies needs an explicit sequential-owner control;
it is not treated as a mechanical no-op here.

The source-only follow-up adds an exact 4096-node positive case and a 4097-node
negative case that requires no decode or trait write. Its seven selected
controls, new-head strict checks, canonical CI and opt-in smoke have not been
run by this preparation. Same-name external file replacement without a changed
GameData/UI lifetime, full-menu/native/GPU acceptance, and complete mod support
remain separate.
