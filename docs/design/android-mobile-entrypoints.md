# Mobile control entry points

NVForever design for Android issue #2436. The persistent gameplay settings cog
and question-mark shortcut are removed, including their reserved hit regions.
Pause > Settings > Mobile > Touch layout and guide contains **Customize layout**
and **Controls help / gesture practice**. Desktop keeps these same registered
actions in Controls > Touch.

Customize layout opens the existing layout editor over Settings, without
resuming gameplay or changing bindings. Save commits the normal layout file;
Cancel or Escape restores the original draft. Both return to the same Settings
page. Opening, closing, focus loss, resize and context loss cancel touch roles;
the opening batch and closing drain cannot activate Settings or the world.
The settings form remains owned until that drain completes. Practice retains
its existing isolated teaching input and return path.

This is independent of the Pip-Boy graphics preference: removing settings/help
shortcuts removes their actions, whereas hiding Pip-Boy graphics must preserve
the exact input regions and dispatch.

Validation must exercise registered Settings actions, actual editor touch input,
Save/Cancel/Escape and same-batch tails, plus absence of utility entities and
actions across startup and device changes. Source-only work is not runtime or
phone acceptance.

The user's permanent removal request changes the former cog/help policy. Tests
that required these shortcuts to open the editor now require their absence;
geometry packers no longer reserve their rectangles. Existing gameplay,
controller, gyro, drag, profile rollback and editor assertions remain, with real
Settings entry/return tests replacing the former shortcut entry checks.
