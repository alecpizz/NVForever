# Controller aiming choices

[design] The existing Stick Sensitivity remains the controller's normal look gain.
Additional Controls rows own an ADS multiplier (0.1 to 2), a stick dead zone (0 to
0.4), toggle aim and hold sneak. Defaults preserve the existing 1x ADS gain, 0.12
dead zone, held aim and toggled sneak. The actual AimZoom.aimed state chooses ADS
gain; holding an aim binding while no weapon can sight does not apply ADS scaling.

[design] The configured dead zone replaces the existing radial threshold in the
filtered ActionPads stick route. Hardware/Bevy axis filtering still applies. Its
default retains the existing stick magnitudes and normalization. Raw hardware
history, controller detection and saved remappings are not rewritten.

[design] A separate controller gesture resource tracks toggle aim and momentary sneak.
It reads actual remapped ControlInput after device selection. Hold sneak contributes
through a separate controller source in HeldSneakInput, keeping the keyboard/script
toggle base and touch source independent. Device invalidation, focus/lifecycle,
menus, text capture, loading, rebinding and option changes cancel latches and require
the relevant physical control to reach neutral before rearming. Consumed switching
presses and a second pad pressing an already-held action cannot create a toggle edge.

[design] These choices neither change script permissions nor inject keys, mouse events,
camera poses or hardware button state. The existing player look and gameplay control
paths retain ownership of the resulting requests.
