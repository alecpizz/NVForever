# Independent HUD sizing

[design] Follow-up to #2436, first functional increment: separate health, action-point
panel and compass size sliders, from 75% to 200% in 5% steps. Existing UI scale remains
the global base. Every new value defaults to 100%, preserving the original layout.
The action-point panel includes ammo and weapon condition. Health size leaves the
compass independent, including its markers. The shared settings page and INI writer
persist version-one keys; missing old settings use the unchanged defaults.

[design] Both hand-built and tile-drawn HUDs mark real drawing subtrees. Whole-view
wrappers scale their descendants around the corresponding bottom corner, keeping
the same transform in both drawing paths. A compass is classified before its health
ancestor. Unrelated mod HUD roots and menu content keep their existing transforms.
Scaling applies before Bevy UI layout/transform propagation; no world-camera FOV or
gameplay values change. Returning a slider to 100% restores that part.

[design] This isolated source increment is not a complete accessibility editor. Live
phone rendering, arbitrary anchor placement, overlap/safe-area preview, a dedicated
save/cancel/reset panel and independent prompt/subtitle/hotbar/minimap sizing remain
next increments. Dialogue typography belongs to the separate dialogue adapter.
The current settings page saves through its normal existing route. No new full
profile document or unknown-version rewriting is introduced.

[design] Four authored tests cover semantic classification, bounded independent
values, the live ECS transform route, and actual settings-file write/reload. The
existing settings-layout oracle explicitly adds the three new rows (109 to 112).
Parser and whitespace checks pass; Rust compilation/execution and device rendering
remain unrun until resource admission. No game files or images are fixtures.
