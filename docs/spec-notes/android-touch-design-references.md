# Android touch design references

Clean-room scope: this note describes NVForever's own Android port design, not the original game. Here `[hypothesis]` marks a port design proposal or pending device acceptance, and `[documented]` cites repository source, an owner request, a pinned dependency or public platform documentation; neither is a claim about Fallout: New Vegas (see the [classification](../design/android.md#clean-room-classification-of-the-ports-constants-and-behaviour)).

- [documented] Activision's 2019 [Call of Duty: Mobile controls guide](https://blog.activision.com/call-of-duty/2019-10/Getting-a-Grip-on-the-Call-of-Duty-Mobile-Controls)
  describes a movement joystick, a mock-HUD editor with drag placement, size and
  opacity adjustment, and fixed versus floating joystick choices. It also documents
  separate firing schemes. This is a dated design reference, not a claim about the
  current game's complete settings.
- [documented] Microsoft's [touch controls designer guide](https://learn.microsoft.com/en-us/gaming/gdk/docs/features/common/game-streaming/building-touch-layouts/game-streaming-tak-designers-guide)
  recommends removing unnecessary buttons, reserving large targets for frequent
  actions, using context-specific controls, and supporting direct inventory/menu
  touch. It calls for input-specific prompts and correct state restoration after
  leaving and returning to a session. Its streaming APIs do not apply to this
  native Android implementation.
- [documented] Epic's 2020 [Fortnite mobile guide](https://www.fortnite.com/news/getting-started---fortnite-for-mobile?lang=en-US)
  describes movable, resizable HUD buttons and alternate presets. Its
  [mobile design guidance](https://dev.epicgames.com/documentation/fortnite/designing-for-mobile-in-fortnite)
  emphasizes a clear hierarchy and controls suited to the activity. NVForever
  applies that rationale through its existing profile editor and optional views.
- [documented] Mojang's [Minecraft Bedrock 1.19.50 notes](https://www.minecraft.net/de-de/article/1-19-50-update-available-bedrock)
  document distinct joystick/tap, joystick/crosshair and directional-pad schemes,
  settings onboarding and independent joystick preferences. This supports teaching
  the selected scheme and preserving explicit alternatives, rather than changing
  interaction policy invisibly. It is a historical release reference.
- [hypothesis] For NVForever, movement should remain visibly explained in the
  controls practice screen: drag the left stick to move, and drag free right-hand
  space to look. Icons need action names in help and the editor so unfamiliar
  symbols remain discoverable. These are product design choices to validate on
  devices, not observations about the original game.
- [hypothesis] Keep frequent movement/combat actions reachable and expose secondary
  utilities through the tray. Direct dialogue choices, inventory rows and nearby
  world targets should use the game's existing semantic actions. A direct touch
  must retain distance, visibility, lock, script and crime rules.
- [hypothesis] A profile editor should preview the same fitted geometry that receives
  input, use explicit Save/Cancel, and prevent editing contacts from becoming game
  actions. Rotation, safe-inset changes, controller switching and suspension must
  retire old contacts before fresh input is accepted.
- [documented] Current implementation and unrun coverage are tracked in Draft
  [PR2478](https://github.com/LootJunkieAzza/NVForever/pull/2478). These references
  establish design rationale only; they do not establish NVForever device comfort,
  frame presentation, resume recovery or crash resolution. No external artwork,
  game screenshots or implementation code is copied.
