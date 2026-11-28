# MCM integer RGB consumers

## Documented operators and original demand

[documented: GECK wiki, [UIO Extra XML Operators](https://geckwiki.com/index.php?title=UIO_Extra_XML_Operators)]
UIO adds accumulator bitwise AND (`land`), left shift (`shl`) and arithmetic right
shift (`shr`). These are distinct from logical `and`. The documented arithmetic
right shift is not an instruction to choose a particular integer width.

[observed: original MCM Extender 1.6.3 OptionRect and HSVColorPicker XML]
The option preview extracts red/green/blue from a packed colour using shifts
16/8/0 and mask 255. The picker rebuilds a packed colour from channel values
shifted by 16/8/0, adding their disjoint contributions. This is a consumer-demand
observation from original data, not an original-executable numeric-parity capture.
No original XML is included here.

## Bounded acceptance contract

[design] The first supported consumer domain is finite integer channels 0 through
255, packed integers 0 through 16777215, mask 255 and shift counts 0, 8 and 16.
These values remain exactly representable even in a binary32 trait. Within this
domain the required result is independent of choosing 32-bit versus wider signed
integer arithmetic: unpacked channels equal their independent input values, and
packing equals `red * 65536 + green * 256 + blue`. No implementation, clamp,
conversion, validation or rejection policy is selected by these fixtures.

[observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + UI Organizer 230, 2026-11-24,
`GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated clean-room staging
copy (synthetic prefab XML staged strictly inside the isolated copy and removed after exit;
read-only `PrintToConsole` probe; zero writes to game install or Saves),
`w4e_10_mcm_rgb_negative_frac_shift` (`tools/capture/nvse/catalogs/wave4e_mods.json`,
PR #3803 / #3802):
`PROBE|w4e_10_mcm_rgb_negative_frac_shift|rgb=255,128,64|land(neg=255,frac=4)|shl(neg=-20,frac=8,s32=0)|shr(neg=-4)`;
zero DLL, PDB, or third-party source consulted]
In original Fallout: New Vegas + UIO 230, `<land>`, `<shl>`, and `<shr>` do not
restrict inputs to nonnegative whole numbers:
- Negative operands use signed 32-bit two's-complement arithmetic (`-1 land 255 = 255`,
  `-5 shl 2 = -20`, `-8 shr 1 = -4` arithmetic right shift).
- Fractional operands are rounded to the nearest integer rather than truncated or
  rejected (`3.7 land 255 = 4`, `3.7 shl 1 = 8`).
- Left shift counts `>= 32` return `0` (`1 shl 32 = 0`).

[design] `crates/fnv-ui/src/tree.rs::integer_extra` rounds finite `Value::Num` operands
to nearest `i32` (`f64::round`, half away from zero) and evaluates signed 32-bit `x & y`,
`x.wrapping_shl(y as u32)` (`0` when `y >= 32`), and arithmetic `x >> (y as u32)` (`0`
when `y >= 32`). Non-numeric strings, nonfinite floats, operands whose rounded value
falls outside `i32::MIN..=i32::MAX`, and negative shift counts leave the accumulator
unchanged.

[hypothesis] Uncaptured native edge cases — halfway `.5` tie-breaking direction,
negative shift counts, `shr` with shift count `>= 32` on negative accumulators (sign-fill
`-1` vs `0`), out-of-`i32`-range float conversion, nonfinite values, and negative packed
colour sentinels — remain unobserved. The existing MCM system-colour path and its
negative/default sentinels retain their existing treatment.

## Fixtures and actual boundary

Three authored `fnv-ui` libtests load independent menus through the ordinary XML
reader, expression tree, trait setters and update scheduler. They use an option
preview's actual image red/green/blue traits and a picker output consumed by that
preview. Distinct channel values, all-zero/all-255 endpoints, live channel edits,
stable updates, a fresh tree receiving the selected value and an ordinary
arithmetic neighbor provide controls. Expected packed values use integer
place-value arithmetic rather than the implementation under test.

The first intended old-consumer failure is a wrong swatch channel or packed
output, before a later parse-warning assertion. Ignored-operator warnings alone
do not prove pixel output, physical UI interaction or full mod acceptance.
The three consumer cases and a mapped-domain/retained-operator control were
authored for the shared parser/evaluator's three operations under the
explicit bounded mapping above, preserving accepted `bt` and ordinary arithmetic
and logical operators. No native renderer, installed MCM options,
original HSV rounding, input/persistence or UIO-wide compatibility is certified.

[documented: CPU process evidence] At `c264d430`, normal release Cargo compiled the
candidate in its own target and ran all 120 fnv-ui libtests: four new controls and
116 retained controls passed once, zero ignored/filtered. The identical-fixture
baseline restored exact accepted `d2b45842` parser/evaluator files and passed 116,
failing the intended four numeric assertions. Actual source paths, executable
hashes, unchanged before/after source inputs and finite 2-GiB job/8-GiB global
reserve were retained privately. This comparison remains scoped to c264d430;
documentation and integration descendants have separate source identities.

[documented: process limits] The overall runner flag remains false: one unknown
owned descendant stayed after successful candidate Cargo/test exit and a bounded
shutdown grace, then was terminated with the job; owned active count reached zero.
This is separate from the genuine test results, not overall run qualification.
Earlier runner attempts remain preserved: the first stopped after baseline tests
at an immediate descendant check; the next candidate invocation did not establish
source binding and is excluded. No source, assertion, cap or false result was
changed to obtain a green flag. Engine callback/draw-piece, original-mod/rendered
acceptance and native numeric parity remain unqualified. Independent raw/source review
confirmed the bounded case results while retaining the false overall flag.

[documented: source integration] Accepted Main `f5cb523f` contains the
fnv-ui RGB consumers and fixtures alongside the named-source lookup index.
The earlier `060d69dd` and `a0ebe1c5` checkpoints do not contain this RGB work. Validation of an integrated graph is bound to
its exact executed source; matching Rust/fixture blobs alone does not transfer
whole-graph qualification from the c264d430 comparison.
