# Android continuous handoff (#2436)

[design] Continue/Load retain fresh isolated :game processes and unchanged LoadRequest arguments. An original fullscreen landscape loading window must submit its own Java UI frame before the old owned game is stopped. The successor keeps a matching above-surface overlay until an epoch/request-scoped native primary-surface content submission witness. Activity creation, focus, timers and surface creation cannot complete the handoff.

[design] Coordinator admission is idempotent, foreground-aware, and bounded by preparation/termination/content timeouts. Back/error offers explicit return to launcher and stops only owned game processes. Restore after coordinator process loss fails visibly rather than duplicating a launch. Main-process cover remains until native acknowledgement; its callbacks never bring an activity forward merely to report ready.

[design] A central optional platform input gate filters action views without modifying hardware, script permissions or user bindings. It preserves physical ownership, suppresses releases, and quarantines keys/mouse/pad/analog until neutral. Generic touch contacts, aim latch, held-sneak and drawer recognition cancel while covered. Desktop behavior remains unchanged when the resource is absent.

[documented: main_menu.rs/intro.rs] New Game is an in-process PlayIntro route. An optional Android NewGameCover keeps the actual menu/root until a fresh Java cover epoch commits; only then does the existing menu route emit PlayIntro once. A playing intro blocks reveal until a correctly sized decoded RGBA frame has been applied to its live image, followed by the same matching primary-surface submission witness. With no/unreadable/unsupported intro the existing begin-game route qualifies the actual loading screen or settled world. Desktop lacks this optional resource and remains immediate. No new process is created for New Game.

[design] Reuse existing primary-window presentation and loading/main-menu/world phase evidence; actual presentation submission is stronger than RenderCleanup frame counts but is not physical scanout proof. Reset witnesses on surface/background epochs. Preserve inset observation/system gestures, process isolation, saves and diagnostics privacy. All sources/test fixtures are original or existing project source.

[qualification] Implemented source and authored host/App tests require separate Java/ARM execution and private device continuity qualification. No claims of seamless physical rendering before that validation.

[design] `RestartActivity` is explicitly launched into the main task (NEW_TASK); the fresh game clears only its separate game task. An ordinary caller-task restart activity would be destroyed by that clear. Loading refusal is surfaced through a private bounded dialog, preserving saves and allowing the actual main menu to present before reveal.

[qualification] CPU tests inject native submission evidence explicitly; they verify admission/epochs but do not execute GPU work. New Game film-end-to-world still uses the existing engine loading transition; device continuity and film texture upload timing remain private runtime qualification, not inferred from host tests.

[design] The successor's completion callback also requires a successful logical
hide of its current cover lease. The shared `LoadingReveal` protocol checks the
state/ticket/cover generation again after hide callbacks. A refused hide retains
the loading gate and existing failure recovery; an obsolete callback cannot
acknowledge or fail a newer episode. See the owning
[loading-cover note](android-loading-cover.md) for the actual paired Java
regression results and the separate physical-window/input qualification limits.
