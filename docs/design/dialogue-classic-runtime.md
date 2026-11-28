# Classic dialogue casing

Classic reads an optional local override at
`Textures/Interface/NVForever/Dialogue/classic.png` through `GameData`. When no
override is present, it uses the same bundled PNG on desktop and Android. The
desktop executable embeds it; Android reads `assets/dialogue/classic.png` from
the APK via the native asset manager. No file is written into the player's data.

Original AI-generated artwork created by the PR author using ChatGPT image generation
for NVForever's Classic dialogue frame; not extracted or derived from *Fallout: New Vegas*,
Bethesda assets, or any third-party mod. Contributed by the author under the repository's
own licence (`GPL-3.0-or-later`; carries no upstream third-party copyright/licence
restrictions).

The bundled atlas is a 1774×887 (2:1) PNG. Local 8-bit RGB/RGBA overrides can
use that layout or the previous 1447×1087 layout, with openings and controls
in the corresponding positions.

The real supplied casing, lamps, speaker and controls remain opaque and retain their
original colours. A bounded connected-black mask clears the four actual rounded
apertures in memory: the left scene window, upper-right NPC text screen, lower-right
response screen and lower-left caps readout. The original PNG is never edited and no
transformed image is saved. This is a blank-template route, not a general screenshot
content remover; different dimensions or absent apertures are refused.

The casing is cut into four bands (header, world window, controls, dialogue screens)
that meet in plain dark gaps. One uniform scale fits the whole picture into the safe
canvas, so the tubes, grille, cables, speaker, lights, every rounded corner, the
caps bezel corners and Trade hardware keep their native aspect at any admitted window shape.
Spare width or height goes only to flexible segments: the two plain dark gaps
beside the header tubes and the straight bars and interiors of the openings. On a
wide phone the dialogue screens take the extra width; in portrait the world window
and screens take the extra height; the speech screen always keeps one 48-unit row.
Under the safe-area insets, each band's outer four source pixels are repeated to
the window edge, so no border of the world or clear colour shows; text, the caps
readout and the Trade target remain inside the safe canvas.

The caps glyphs are sized from the counter opening's height (its interior less a
small bezel margin), not from a fixed type size. The opening's plain interior widens
only when the value shown needs it, including both softened-font overhangs. If
the wide template's counter would otherwise drop below 12 logical units, its
straight center takes just enough height from the world opening; the caps bezel
corners and Trade plate are not stretched. Extreme signed values fit unshortened.
Unknown cash remains blank (the layout is sized for 0). A layout that cannot fit
the readable counter, a 48-unit speech row and 48-unit targets is refused.

The left aperture is the real world camera's viewport. The dialogue lease admits only
a live perspective player camera on the current primary window and only an unowned
full-window viewport, or the exact viewport that this lease previously wrote. Closing,
pausing, focus loss, disabled touch dialogue, camera replacement and unavailable data
retire the lease. Restoration changes only the viewport still owned by this lease;
a foreign viewport, explicit reset, transform or default-UI marker is preserved. A
foreign takeover is not immediately reacquired while the same Classic view stays open.
The stored speaker side places Field Notes and the device models (Retro Console,
Wrist Terminal); it does not move Classic or Normal.

The upper-right screen consumes the authoritative last-spoken-line state, retaining
that actual text while responses are offered. Its independent scroll owner is not a
Skip action while choosing; during speech it uses the existing guarded Skip action.
The lower-right screen uses the existing offered choices, requirements, selection,
release guards, optional confirmation and scroll/reveal consumers. All interactive
rows and the Trade hit target retain 48-logical-unit minima. Wrapped text can
make a row taller; the template does not promise that all choices fit without scrolling.

The bundled plate's TRADE label and red button form one target. It is published
only for a current merchant/list target and opens the existing barter menu
through the guarded release and action consumer. Otherwise the red button is
dimmed and has no action. The earlier override's Review label remains decorative;
the retained spoken line and scroll behavior are unchanged. The installed native
Font 5 and existing dark-screen undercoat provide live content; no default-font
stand-in is reported as a ready Classic view.

The old custom backing, tint, accent and frame-opacity choices remain stored for the
configurable Field Notes treatment. They do not fade or recolour real hardware. Text
sizes and the saved confirmation mode still apply to Classic.

`Cache::prepare` retries while `GameData` is absent, then reads and decodes the
override or bundled default once. An invalid override is refused rather than
silently replaced. A presentation/data reload can explicitly reset the caches.
Ordinary frames do not read the file again. `Surface` describes actual prepared art/font/geometry; it alone is not a rendered-frame
claim. Resolve keeps its camera proposal private. The ordered Admit phase validates
current ownership and writes the actual viewport before either UI painter selects a
view; only then does `Frame.classic` publish that applied identity. Before Scripts,
validation also withdraws last frame's identity if a foreign camera change made it
stale, so a terminal raw touch cannot use it. `ready_for` requires the current matching
committed lease. CameraUpdate remains in its ordinary PostUpdate schedule. A later
foreign takeover is respected and retired at the next admission, rather than revoking
the selected UI after it has already been painted.

The normal presenter can temporarily display genuine Normal dialogue if Classic is
unavailable, preserving the stored Classic request; a capture must refuse that fallback
as Classic success. All touch styles and recovery share one explicitly targeted
full-window UI camera, whose lifetime follows available dialogue/menu content. Its
existence does not assert side framing or a Classic world lease, and it requires
neither ScriptState nor a player camera merely to display ordinary dialogue. Foreign
default-UI markers remain untouched.

Source controls cover the four apertures in the bundled bytes, runtime VFS
override, native-scale hardware and inset coverage, Taffy-painted text at 20:9,
19.5:9, 16:9, 4:3 and portrait, caps fit, and raw Trade release into the real
barter menu. Camera lease cleanup/foreign ownership and raw-contact retirement
are covered separately.
An ordered handoff control changes the actual Camera target, viewport or activity
between Resolve and Admit and checks the real Classic/Normal painter selection. The
CPU scene deliberately publishes authored `Camera.computed.target_info` for scheduling
and Taffy layout; it does not run or prove the actual CameraUpdate/window/GPU provider.
A second control changes the actual camera before Scripts and verifies that the old
contact cannot answer through last frame's publication. These CPU controls do
not establish real GPU rendering, original-game parity or physical handset acceptance.
