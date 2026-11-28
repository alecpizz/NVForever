# Optional Android gyro aiming

[design] Gyro is off by default. Controls > Touch owns its enable switch, bounded
sensitivity and ADS scale, hold/toggle activation, optional ADS-only gate, and recenter
action. A local `Gyro` target is appended after the existing 18 touch actions; its
stable layout key supports the existing move/resize/hide editor. It never creates a
game action, key, mouse event, or physical-device activity.

[documented] Android's game rotation vector provides relative rotation without the
magnetic-field reference; its yaw can drift. Sensor axes retain the device's natural
orientation when the display rotates. We use the actual display rotation and relative
quaternions, rather than assuming a particular handset or portrait-native display.
Sources: https://developer.android.com/develop/sensors-and-location/sensors/sensors_position
and https://developer.android.com/develop/sensors-and-location/sensors/sensors_overview .

[design] The Java listener exists only while requested, resumed and focused. It uses
TYPE_GAME_ROTATION_VECTOR when available, otherwise reports unavailable. Relative
samples have monotonic timestamps, a 50 ms gap bound, a 12 radians/second angular-rate
bound and a 100 ms drain freshness bound. A 0.001 radians/second stationary-noise
floor suppresses slow fusion drift; deliberate motion below that floor is also ignored.
This is an input design bound, not a claim of device-specific bias calibration. Invalid samples, display changes, lifecycle
changes, recenter, and activation changes discard the reference and pending motion.
No absolute heading or magnetic-north correction is applied. Recenter discards motion;
it never changes the player's current view.

[design] Rust admits only a bounded finite per-frame radians contribution. Actual
AimZoom.aimed controls ADS scaling and ADS-only activation. Playing state, focus,
touch-device consumption, controller selection, the platform input barrier, layout
editor, practice and dedicated modal capture all cancel activation and drain motion.
The player's existing look path applies script locks and camera ownership. Sensor
motion cannot turn a menu pointer, free camera, dialogue camera or a loading screen.

[design] Java lifecycle hooks are supplied as a separate integration patch because the
loading/handoff lane owns GameActivity. Tests use synthetic rotations and actual ECS
input routes; no game data, gameplay saves, hardware permission state or device IDs
are used. Hardware orientation/sign feel still requires device verification.
