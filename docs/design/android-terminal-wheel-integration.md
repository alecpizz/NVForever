# Terminal and companion wheel input integration

[design] Terminal and companion-wheel touch adapters share the engine's actual puzzle and command rules. Their modal input capture also covers the generic touch overlay and terminal monitor hit testing.

[design] A modal capture bridge suppresses/releases ordinary touch contacts, aim latches, held-sneak and drawer recognition while either adapter captures. Physical keys/mouse retain their independent ownership. Capture is based on actual screen resources before delayed PlayState transitions and drains the closing batch. TouchDeviceSwitch consumption gates both dedicated adapters so a controller-return touch only restores the input device. Keyboard/controller routes and existing user bindings remain authoritative.

[design] Accessible flat terminal capture clears stale3D pointer state and suppresses monitor/power hit processing. Focus/suspend/missing-window/session/viewport changes discard old contacts without choosing an action. Tests exercise actual message routes and final puzzle/command outcomes.

[design] Column changes and completed or canceled pans discard the rest of their raw touch batch, so later contacts cannot use geometry from the previous viewport or replay on the next frame. Closing options blocks both queued and physical confirms for that frame. Default companion-wheel taps and the Mode button reject out-and-back drags; drag-release commands remain opt-in. Test fixtures disable real settings writes; persistence controls use explicit temporary files.
