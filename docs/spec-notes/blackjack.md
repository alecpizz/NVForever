# Blackjack (the casino tables)

The original's blackjack minigame: a table of the Strip's casinos opens a menu where the
player bets the casino's chips against a dealer. Rules: `fnv_game::blackjack`; the menu:
nvf-play `blackjack.rs`; the table, the cards and the chips: nvforever `blackjack_view.rs`.

Sources: FalloutNV.esm (the CSNO records, the tables' scripts, SOUN and MESG records),
`menus\black_jack_menu.xml`, the models in `meshes\terminals\nv_blackjack\`, the GECK wiki
(owner's export) and the owner's screenshot of a Gomorrah table (2026-10-16, kept
privately). Nothing here comes from the executable.

## 1. Opening it

- Each casino's blackjack table is an activator whose script runs, on activation by the
  player, `ShowBlackJackMenuParams <casino> 1 200 0`, or a message instead while the
  casino's winnings quest has `bFloorManagerIncoming` set [observed: decompiled scripts
  `BlackjackTableTheTopsScript` 0x00145C85, `vGomorrahBlackjackSCRIPT` 0x00161E41, and
  0x001614D1 (Ultra-Luxe), 0x0016150F (Atomic Wrangler), 0x0016206D (Vikki & Vance)].
- `ShowBlackJackMenuParams casino [minBet] [maxBet] [maxWinnings]` (sbjmp, 0x1254) opens
  the blackjack menu for a casino form [documented: GECK wiki ShowBlackJackMenuParams;
  opcode: script-functions-catalogue.md]. Every table in the game passes min 1, max 200
  and max winnings 0 [observed: the five scripts].
- "Player must meet Casino requirements for the menu to display, such as having Casino
  Chips in the inventory and not being banned from the casino" [documented: same page].
  Otherwise a line says why [hypothesis: the words are NVForever's; the original's are
  not in the plugins].
- A `maxWinnings` of 0 means the casino's own (§2) [hypothesis: every script passes 0,
  and the casinos' records hold the caps the bans are known for].
- It is a menu: the world and the game clock stand still (`PlayState::Menu`), menu code
  1081 to scripts (`BlackJackMenu` in `fnv_ui::value::MENU_CLASSES`) [documented: GECK wiki
  MenuMode].

## 2. The casino record (CSNO)

Laid out in `schemas/fnv/objects.json` [documented: xEdit wbDefinitionsFNV.pas]:

| Field | Meaning | Gomorrah / The Tops / Ultra-Luxe [observed] |
|---|---|---|
| FULL | the casino's name | "Gommorah" (sic) / "The Tops" / "Ultra-Luxe" |
| DATA 0, f32 | the share of the shoe dealt before it is shuffled | 0.20 / 0.35 / 0.40 |
| DATA 4, f32 | blackjack's payout ratio | 1.5 / 1.5 / 1.5 |
| DATA 8, 7 × u32 | slot machine reel stops (not used here) | |
| DATA 36, u32 | decks in the shoe | 3 / 4 / 3 |
| DATA 40, u32 | max winnings | 9000 / 10000 / 15000 |
| DATA 44, FormID | the casino's chip (CHIP) | GomorrahChip / TheTopsChip / UltraLuxeChip |
| DATA 48, FormID | the casino's winnings quest | |
| DATA 52, u32 flags | bit 0: the dealer stays on soft 17 | unset / set / set |
| MOD3 | the blackjack table's model | `NV_Blackjack-TableG.NIF` / `…-Table.NIF` / `…-TableU.NIF` |
| ICO2 | the deck's back | `terminals\NV_PlayingCards\DeckG\cardG_Back.dds` (Gomorrah) |

- The share is "the percentage of the decks to exhaust before shuffling", 1.0 being all
  [documented: GECK wiki GetCasinoShufflePercent].
- The felt of the Gomorrah table reads "Dealer must hit soft 17" [observed: the owner's
  screenshot], the flag being unset there: the flag set means the dealer stands on a soft
  17, unset that the dealer hits it [observed].
- The faces are the back's folder with `card<deck>_<suit><rank>.dds`, suits `c d h s`,
  ranks `01` (ace) to `13` [observed: `fnvtool bsa list` of `Fallout - Textures.bsa`].

## 3. The rules

- **Cards**: 2 to 10 count their number, jacks, queens and kings 10, an ace 1 or 11,
  whichever is better; over 21 is a bust and loses at once [documented: the help message
  `HelpBlackjack` 0x00172094].
- **The shoe**: the casino's number of decks, shuffled; before a deal, when the dealt
  share has reached the casino's share, it is shuffled again [documented: §2; hypothesis:
  checked between rounds only].
- **Betting**: the bet starts at the table's minimum and moves by the steps of §5 between
  the minimum and the smaller of the maximum and the chips carried. The bet is taken from
  the chips as the cards are dealt [hypothesis].
- **The deal**: one card to the player, one face up to the dealer, a second to the player,
  the dealer's second face down [hypothesis: the order of the models' sequences
  `Hand1_01`, `Dealer_01`, `Hand1_02`, `Dealer_02`]. In the dealer model it is the first
  card that lies face down: `Dealer_01` lays its card face down and `Dealer_02` lays its
  card face up [observed: buckallobill, in game, FNV 1.4.0.525: frames of a round in NVForever
  playing the model's sequences]. NVForever keeps the rules' order and draws the hole card's face
  on the model's first card, which `Dealer_Reveal` turns over (nvforever
  `blackjack_view.rs`, `dealer_face_slot`).
- **A natural**: an ace and a ten-card in the first two. With the player's, the hole card
  is turned at once: the dealer's natural too is a push, else the player is paid the bet
  times the payout ratio (1.5: three to two, as the felt says [observed: "BLACKJACK PAYS 3
  TO 2" on the screenshot]), rounded down [hypothesis]. The dealer does not look at the
  hole card first: a dealer's natural shows when it is turned over, after the player has
  played [hypothesis: the menu has no insurance; `Dealer_Reveal` comes after the player's
  sequences].
- **The player's choices** [documented: the menu's six buttons, `BJM_Hit` …
  `BJM_Stay_Exit`, and their comments "P:" (play) / "D:" (deal)]:
  - Hit: one more card. 21 stands by itself [hypothesis].
  - Stay: the hand stands.
  - Double Down: on a hand's first two cards [hypothesis], the bet doubled (the chips
    taken), one card, then the hand stands.
  - Split: two cards of the same rank [hypothesis: ten-cards of different ranks do not
    split] become two hands, each with the bet and a second card; once only (the models
    have two hands, `NV_Blackjack-Hand1` and `-Hand2` [observed]). A natural after a split
    counts as 21, not blackjack [hypothesis].
  - Switch Hands: after a split, play the other hand [hypothesis: by the name; NVForever
    lets it move to the other hand while that one has not stood].
  - Surrender: on the first two cards of an unsplit hand, half the bet back (rounded up for
    the player [hypothesis]) and the hand is over.
- **The dealer** turns the hole card, then hits to 17; a soft 17 hits unless the casino's
  flag says to stay (§2). Not when every hand of the player has bust or surrendered
  [hypothesis].
- **Paying**: a hand over the dealer's (or with the dealer bust) wins its bet; equal is a
  push; lower loses.

## 4. Winnings and bans

[documented] GetCasinoWinningsLevel reports the player's casino level to scripts,
and GetCasinoWinningStage supplies the same value to dialogue conditions
([GECK](https://geckwiki.com/index.php/GetCasinoWinningsLevel)). The public
[Stewie changelog](https://www.nexusmods.com/newvegas/mods/66347?tab=logs) documents
a 9.45 correction so No Casino Bans does not report the banned stage through
GetCasinoWinningStage. That behavior is distinct from a provider version number.

[observed: original FNV 1.4.0.525 + Stewie's Tweaks v1005 + JIP LN 57.3, two-pass
capture toggling `bNoCasinoBans` under `[Tweaks]` between `0` and `1` (2026-11-22)]
With `bNoCasinoBans = 0`, `GetCasinoWinningsLevel` (`0x125C`) returned `0 -> 0`,
`1250 -> 2`, `2500 -> 4`, `3500 -> 4` at Vikki & Vance (`max_winnings = 2500`),
`0 -> 0`, `5000 -> 4` at Atomic Wrangler (`max_winnings = 5000`), and `10000 -> 4`
at The Tops (`max_winnings = 10000`). With `bNoCasinoBans = 1`, it returned
`0 -> 0`, `625 -> 1`, `1250 -> 2`, `1875 -> 3`, `2499 -> 3`, `2500 -> 3`,
`3500 -> 3` at Vikki & Vance, `5000 -> 3`, `6000 -> 3` at Atomic Wrangler, and
`10000 -> 3` at The Tops — directly observing both the `4 -> 3` replacement and the
quarter boundaries (`1/4`, `2/4`, `3/4`, `4/4` of `max_winnings`).
`GetCasinoWinningStage` (`0x1262`) is a condition-only command in the original game
(direct script calls crash `FalloutNV.exe`), so dialogue condition evaluation shares
the same `4 -> 3` clamp (`[observed]` on `GetCasinoWinningsLevel` + `[documented]`
Stewie's Tweaks 9.45 changelog for `GetCasinoWinningStage`).

[design] Engine design choice: the replacement is a read view. Stored levels,
earnings, earned chips and rewards remain unchanged. Turning the option off exposes
the saved stage again, and the existing table opening check continues to enforce its
earnings and saved-level ban. Stages 0..3 retain their existing results. The option
is applied by the existing settings consumer after restore; this change does not add
preference persistence.

[documented] `SetCasinoWinningsLevel casino level` sets a casino's winnings
level, with an integer parameter in 0..4. Source:
[GECK](https://geckwiki.com/index.php?title=SetCasinoWinningsLevel).

[observed: original FNV 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.3 + Stewie's Tweaks v1005,
two-pass `bNoCasinoBans` 0 vs 1 live capture (2026-11-22, #3474)] On an untouched casino
(Atomic Wrangler at `0` winnings, never entered in the player's casino list),
`SetCasinoWinningsLevel` 1..4 is a no-op (both `GetCasinoWinnings` and
`GetCasinoWinningsLevel` remain `0`). On an already-tracked casino (The Tops,
`max_winnings = 10000`, after `SetCasinoWinnings 10000`), `SetCasinoWinningsLevel 2`
sets net winnings to `5000` (`level * max_winnings / 4`) and level to `2`, and
`SetCasinoWinningsLevel 4` sets net winnings to `10000` and level to `4` (read as `3`
under `bNoCasinoBans = 1`).

[design] Engine design choice: source-name calls return `0`, treat an untouched casino
as a no-op, and on a tracked casino update both saved standing and net earnings to
`level * max_winnings / 4` (using the effective session max winnings). Subsequent rounds
retain the existing monotonic advancement rule. Invalid casino forms, missing arguments,
nonfinite numbers and nonintegral/out-of-range levels fail with an engine error without
changing casino state. Native invalid-input behavior and interaction with the original
game's quest scripts remain unobserved. Source-name admission does not admit native
numeric encoding.

- The player's earnings at each casino are kept with the game: the net chips won there
  over every round [documented: GECK wiki SetCasinoBan, GetCasinoWinnings].
- A casino bans the player when the earnings reach its max winnings: the winnings level is
  then 4, and the menu does not open [documented: GECK wiki GetCasinoBan, SetCasinoBan,
  GetCasinoWinningsLevel (0–4)].
- Levels 1 to 3 come at a quarter, a half and three quarters of the max winnings, and
  level 4 at max winnings [observed: 2026-11-22 live capture across Vikki & Vance
  (625/1250/1875/2500), Atomic Wrangler (5000), and The Tops (5000/10000)].
- A round that takes the earnings to the max ends the game: the menu closes after the round
  [hypothesis].

## 5. The menu (`menus\black_jack_menu.xml`)

- The information box: as the lockpicking menu's (400 × 175, the bracket of lines, three
  rows in font 7 at 35, 80, 125) [documented]: the bet, the chips and the casino's
  winnings, with the casino's FULL name; the titles are set in code. NVForever's titles
  are its own words, "Your bet:", "Your chips:" and "Won at <casino>:" [design, #4495: in
  place of the titles read from the original's screen].
- Six buttons, right-aligned 50 in from the right, under each other [documented], each its
  label and its key [observed: "Hit F)", "Double Down W)", "Split E)", "Switch Hands Q)",
  "Surrender S)", "Stay R)"; keys documented: `_PCButton_F/W/E/Q/S/R`]. While betting the
  same six buttons are [documented: the comments]: (none, hidden), Deal, Increase Bet,
  Decrease Bet, Max Bet, Exit Game [the words: hypothesis from the comments].
- Where the buttons stand: the file puts them 144 above the box's top; on the screenshot
  the first is about 536 interface units down the screen and the six are about 51 apart
  [observed]. NVForever follows the screenshot.
- The status line: font 7, right-aligned with the buttons, 10 down the main rectangle
  [documented], for the result of a hand [documented: "Display Message for win, lose,
  ect.."; the words are NVForever's (hypothesis)].
- A choice that does not apply is not drawn [hypothesis: on the screenshot all six are
  drawn at a fresh hand of 2 and 4 — a split pair — so dimmed ones cannot be ruled out].
- Bet steps: 1 under 10, 5 under 50, 25 above [design].
- The first time the menu opens the help message `HelpBlackjack` (0x00172094, "Message
  Box, Auto Display") comes up over it, as lockpicking's [observed: the flags; hypothesis:
  shown once per game].
- Esc leaves while betting [design]; in a round it does nothing.

## 6. The table on the screen

- The table: the casino's MOD3 model (`NV_Blackjack-Table<x>.NIF`): the felt with the
  casino's name and rules, the rail, the chip tray, the shoe (`Deck`) at (6.08, −13.61) and
  three point lights (`Omni01`–`03`) [observed: the file's nodes].
- The cards: `NV_Blackjack-Dealer.NIF` (`Dealer_01`–`07`), `-Hand1.NIF` (`Hand1_01`–`07`)
  and `-Hand2.NIF` (`Hand2_01`–`07`), every card at the shoe until its sequence moves it:
  `Hand1_0n` (0.63 s) deals the player's n-th card, `Dealer_0n` the dealer's,
  `Dealer_Reveal` (0.63 s) turns the hole card (NVForever plays it in 0.35 s, then waits
  0.65 s before the dealer draws or the result shows [design]), `<x>_Discard` (1.3 s) sweeps a hand away;
  the table's `Players_Discard` and `Dealers_Discard` move the shoe [observed: the files'
  sequences and text keys]. A card's face is the shape `<name>:n`, its back the shape under
  `<name>_Back`, its shadow `<name>_Shadow` [observed: the names]; NVForever puts the card's
  face picture on the face shape [hypothesis: the face shapes are textured with a stand-in].
- Each sequence keeps every other card where the earlier ones left them [hypothesis: each
  `_0n` sequence holds all seven cards; NVForever poses the cards dealt so far at the end
  of their own sequence].
- The chips: `NV_Blackjack-Chip_001/005/010/025/100.NIF` [observed: the CSNO model list].
  Each is a column about 1.7 across and 2 high whose top lies just under the felt, each
  value at its own place on the table (`Chips_1` at (−9.93, 0.25, −2.14), `Chips_25` at
  (−14.1, −3.81, −2.14)) [observed: the files' nodes and bounds]: the original places them
  itself. NVForever draws each chip of the bet as its value's model squeezed to 0.3 high,
  stacked by value on the spot `Chips_1` marks, beside the player's cards where the
  screenshot has the bet [hypothesis].
- The table's two green arrows (`Hand1_Arrow:0`, `Hand1_Arrow:0@#2`) show only after a
  split, at the hand being played [hypothesis: the screenshot of an unsplit hand shows
  none].
- The camera: from the player's side, looking down at the felt so that the dealer's cards
  are at the top and the player's at the bottom, as on the screenshot [design: fitted].
- Sounds [observed: SOUN records named for it; design: which event each is for]:
  `GAMEBlackJackCardDeal`, `…CardFlip`, `…CardShuffle`, `…CardCollect`,
  `…CardCollectDealer`, `…IncreaseBet`, `…DecreaseBet`, `…Win`, `…Lose`.

## 7. Open questions for the owner

- The deal-phase button words, the status lines and the refusals (screenshots of a fresh
  table, a win, a loss, a push and a bust would settle them).
- Whether the dealer peeks for a natural, whether split aces get one card, which choices are
  hidden or dimmed.
- What happens at the table the moment the max winnings threshold is reached (levels 1–3
  at `1/4`, `2/4`, `3/4` of `max_winnings` and `4` at `max_winnings` are now observed in §4).
- The bet steps of Increase/Decrease Bet.
