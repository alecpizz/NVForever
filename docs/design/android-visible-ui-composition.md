# Android visible UI composition

Compose reviewed source packets on public eaf20265 without changing R7 physics. Include controller/aim/drawer r2, four original icon families and visibility-aware cache, adaptive Aim/Block/Scope presentation, named Touch settings rows, and independent HUD size controls. Keep controller first-touch consumption, held-sneak ownership and drawer commit batch barrier intact.

Resolve settings-layout union explicitly, including the reviewed drawer placement correction. Adaptive fixtures must declare inherited visibility so tests exercise the actual icon rendering admission. Add a real App composition regression for adaptive presentation and controller/drawer cancellation.

Source-only: no Cargo regeneration, builds, device activity or publication. Preserve each immutable input and all vendor archive bytes. Parsing is not compilation; all new Rust tests remain unrun until admitted.

Composition includes the exact reviewed drawer placement correction 317c3b8d and the 3-file lint-only cleanup a1578c8a. No subsequent DDS codec work is included. The firstbundle engine lock is preserved byte-for-byte; normal lock regeneration must record tiny-skia as a direct nvforever dependency before locked validation. The dependency was already present transitively; no resolver outcome is assumed.
