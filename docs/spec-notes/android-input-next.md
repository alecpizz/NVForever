# Android input additions

Clean-room scope: this note describes NVForever's own Android port design, not the original game. Here `[hypothesis]` marks a port design proposal or pending device acceptance, and `[documented]` cites repository source, an owner request, a pinned dependency or public platform documentation; neither is a claim about Fallout: New Vegas (see the [classification](../design/android.md#clean-room-classification-of-the-ports-constants-and-behaviour)).

- [documented] The owner's `gyro-inversion` request asks for independent persisted X/Y
  signs, both off by default, without changing touch/controller inversion or recenter.
- [documented] `engine/nvforever/src/touch/gyro.rs` admits finite sensor radians after
  lifecycle, device, modal and ADS gates. The new signs belong at this one gyro-only
  output boundary, after gain and before the existing clamp.
- [documented] `engine/nvf-play/src/player.rs::mouse_look` combines device counts and
  gyro radians. Device vertical inversion must be applied before that combination;
  otherwise its final inversion also flips gyro, violating independent settings.
- [hypothesis] Physical comfort and orientation preference still require device
  acceptance. CPU route and INI tests establish sign/persistence behavior only.
