# CPU skinning compatibility controls

Run `cargo test --manifest-path tools/android/skinning-check/Cargo.toml --locked`.
No game files, Bevy application, GPU device or window are created.

The tests compose the **actual vendored production skinning WESL**, validate Naga IR,
emit SPIR-V, and check current/previous matrix binding offsets, strides and minimum
sizes for storage and uniform variants. Negative controls deliberately corrupt layout
or an active member access. Separate authored two-joint, five-instance, two-pose and
morph math uses handwritten expected results and noncontiguous packed matrix offsets.
This is a focused shader-module test with representative mesh imports, not validation
of every complete PBR pipeline or proof of any phone's rendered output.

`fixtures/skinning_before.wesl` is the pre-wrapper Bevy 0.20.1 shader, retained only as
a compatibility baseline. It carries the same MIT OR Apache-2.0 terms and attribution
as `engine/vendor/bevy_pbr`; the complete licences remain there. The small synthetic
probe data and test harness are original GPL-3.0-or-later work. No game mesh, animation,
texture, screenshot, decompiled material or external GLB is included.

The production change wraps each runtime joint-matrix storage array in a single-field
structure. Offset zero and matrix stride 64 remain unchanged, as do both binding slots,
CPU allocation, instance offsets, weights, uniform selection and morph allocation.
Uniform SPIR-V remains byte-identical to the baseline; the storage access chain differs.
The motivation is the primary Android skinning report
https://github.com/bevyengine/bevy/issues/24926. That report and these CPU checks do not
establish the cause of the R5 screenshots or device loss. Affected-device comparison
and full rendering acceptance remain necessary.
