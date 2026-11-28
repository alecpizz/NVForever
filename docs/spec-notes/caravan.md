# Caravan (the card game)

The original's Caravan card game: a dialogue line runs `ShowCaravanMenu` and the player
bets caps against the speaker, builds a deck of at least 30 cards from the cards they own
and plays three caravans against the speaker's three. Rules and the opponent:
`fnv_game::caravan`; the menu: nvf-play `caravan.rs` (drawn flat in `caravan/screen.rs`
from the game's textures).

Sources: FalloutNV.esm (the CCRD and CDCK records, the dialogue result scripts, the card
and deck items' scripts, the four `HelpCaravan*` messages), the menu files
`menus\caravan_*.xml` and their prefabs (`Fallout - Misc.bsa`), the textures and models
under `terminals\nv_caravan\` and `terminals\nv_playingcards\`, the sounds under
`sound\fx\fx\games\caravan\`, the GECK wiki (owner's export: ShowCaravanMenu,
AddCardToPlayer, GetMenuTargetRef), the Fallout fandom wiki's Caravan page (pasted by the
owner, 2026-10-19), 43 screenshots of a game against Ringo taken by the owner
(2026-10-19, kept privately in `C:\fnv-private\caravan`) and the owner's rulings of
2026-10-19. Nothing here comes from the executable.

## 1. Opening it

- `ShowCaravanMenu deck difficulty maxBetPercent` (0x125A) starts a game against the
  calling reference with the deck form (CDCK) as the opponent's cards, a difficulty from
  0 to 3 and the share of the opponent's currency the opponent bets at most [documented:
  GECK wiki ShowCaravanMenu]. Every shipped call is in a dialogue line's end result,
  bare (on the speaker), with difficulty 0 or 1 and shares from 0.25 to 1 [observed: 29
  calls in the decompiled INFO results, e.g. Ringo `CaravanDeckRingo 0 0.25`, Johnson Nash
  `CaravanDeckNash 1 0.25`].
- The difficulty is not used. How hard an opponent plays follows the share of face cards
  (Jack, Queen, King, Joker) in its deck (§6) [owner ruling 2026-10-19: "more face cards
  in a deck determines how difficult the opponent is for now"].
- The menu's code to scripts is 1083 [documented: GECK wiki GetMenuTargetRef, "1083 The
  opponent actor in a Caravan game"]. It is a menu: the world and the game clock stand
  still (`PlayState::Menu`).
- The game limits some players set (five games) are kept by their dialogue scripts, not by
  the menu [observed: `set 0x00157F76.inashcaravangames to … + 1` beside the call].
- Refused, with a corner message, when the player owns fewer than 30 cards [hypothesis:
  the words and the refusal are NVForever's].

## 2. Cards and decks

- A caravan card is a CCRD record: name, model, `TX00` face and `TX01` back textures
  (`textures\terminals\NV_PlayingCards\Deck<x>\card<x>_<code>.dds`), then two INTV, suit
  then value (suit 1 Hearts, 2 Spades, 3 Diamonds, 4 Clubs, 5 Joker; value 1 Ace, 2–10, 12
  Jack, 13 Queen, 14 King, 15 Joker) [documented: xEdit, simulation.md §1; observed:
  `CardClubs10b` INTV 4 then 10]. FalloutNV.esm holds 270 cards in several sets (the
  Tops, Gomorrah, the Ultra-Luxe, the Lucky 38, decks B, G, L, T, U) [observed].
- A deck is a CDCK record: `EDID`, `FULL` and one `CARD` subrecord per card, a card's
  FormID (u32) [observed: `CaravanDeckNash` "Mojave Express Spectacular", every CARD 4
  bytes naming a CCRD]. FalloutNV.esm holds 13 [observed].
- The player's cards are not kept as items. Every card's script, when the card is added
  to the player's inventory or the player activates it in the world, runs
  `AddCardToPlayer` and takes the card away (`RemoveMe`, or `Disable` + `MarkForDelete`)
  [observed: SCPT 0x0015D7B0]. `AddCardToPlayer` "adds a playing card to the player's
  Caravan deck" [documented: GECK wiki]. NVForever keeps the player's cards as a list of
  card forms (`PlayerState::caravan.cards`), saved with the game.
- Ringo's Caravan Deck item (MISC `CaravanDeck`) moves every item of container 0x0015D8B1
  to the player the first time it is added [observed: `CaravanDeckScript`], so each of
  those cards goes through the card script above. RemoveAllItems therefore runs OnAdd
  on what it moves into a container (once per stack, as AddItem once per call)
  [hypothesis: the GECK wiki's OnAdd runs when the object is added to a container; until
  2026-10-23 NVForever ran none and the player owned 0 cards after Ringo's gift, owner's
  playtest list 7 item 8].

### 2.1 The card's shape (2026-11-12)

- The card pictures are opaque, corners included. The faces and backs are DXT1, 128 by
  256, with no transparent texel; their corners are pale paper [observed: cardb_s09.dds,
  cardg_back.dds, cardt_c01.dds, the first level's corner blocks].
- The original's card models round the corners with their shape. The face of
  `meshes\terminals\nv_caravan\player_1_01.nif` is a 12-point outline, 2.5 across and 3.5
  long. Each corner is cut by three points, in the top right quarter (1.107, 1.75),
  (1.205, 1.705) and (1.25, 1.597), mirrored into the others [observed: its 12-vertex
  NiTriStripsData; the 24-vertex shapes are the shadow round it].
- Its picture coordinates lay the whole picture (0 to 1 both ways, the 128 across the
  card's width) on the full 2.5 by 3.5 rectangle, so each outline point takes the
  picture at its own place: 0.0572 and 0.0437 at the corner's ends, 0.018 and 0.0129 at
  its middle point, and one minus those [observed: the same block's UVs]. No part of the
  picture is inset, and nothing lies past 0 to 1. The picture's corners outside the
  outline are near white (an `fffaff` texel at 0,0 outside the drawn grey arc) and are
  never drawn [observed: cardb_s09.dds].
- NVForever draws a Caravan or poker card as that outline fitted to the card's size, the
  picture laid on it as on the whole rectangle (`card_shape.rs`) [design]. Before
  2026-11-12 the card was a full rectangle and its square corners showed past the rounded
  ones [design]. Blackjack draws the game's own card
  models, so it already has the outline.

## 3. The bet (first screen)

From the screenshots [observed] and the in-game help `HelpCaravanBetting` [documented]:

- One bet per game. The opponent bids first: the line under its name shows the bid. The
  player's line "Current Ante" starts at 0, "Total Funds" is the player's currency.
- Buttons: "Auto Match W)", "Raise A)", "Accept F)", "Exit R)" [observed]. Accept is
  dimmed at an ante of 0; after Auto Match with fewer caps than the bid the ante is the
  player's funds and Raise is dimmed [observed: Ringo bid 54, the player held 18; Auto
  Match gave 18, and the pot on the table was 36].
- The opponent's bid is its currency times `maxBetPercent`, rounded down [hypothesis:
  fits Ringo, 54 of about 216 caps at 0.25; the fandom wiki's "one quarter of their
  currency" for merchants]. Its currency is the caps it carries and those in its merchant
  container (XMRC) [hypothesis, as barter, menu/model.rs `Trade::stock`].
- Auto Match sets the ante to the bid (no more than the player's funds or the opponent's).
  Raise adds a tenth of the bid (at least 1) up to one and a half times the bid [design:
  the fandom wiki says opponents raise to about half again their bid; the step is
  NVForever's]. An opponent with nothing to bid plays for nothing (Accept at 0) [design].
- Caps, NCR dollars and Legion coins are bet (§3.1).
- The pot is twice the ante. The player's ante is taken at Accept; the winner gets the
  pot, so the opponent's ante leaves its caps (merchant container first) when the player
  wins, and the player's stays with the opponent when the player loses [owner ruling
  2026-10-19: "you get caps paid out"; the order is a design choice].

### 3.1 Other currencies (2026-10-19)

- [documented: `HelpCaravanBetting` (MESG 0x00000186): players "are allowed to use any
  common currency available", "bottle caps are common, players may also offer NCR dollars
  or Legion coins"] Each side's funds ("Total Funds" for the player, the bid's base for the
  opponent) are its caps plus its NCR dollars and Legion coins at their records' value: the
  FalloutNV.esm MISC records NCRMoney5 ($5 NCR), NCRMoney20, NCRMoney100, LegionMoney1
  (Legion Denarius) and LegionMoney25 (Legion Aureus) [documented: the records; the
  mangled denarius is not money]. The opponent's count its merchant container's too.
- [design] An ante is paid in caps first, then in notes and coins from the least valued
  up, as many as the rest needs; the last one's overpay comes back as change in caps. The
  pot is paid out in caps. Log: `caravan: the player's ante <n> paid with <c> caps, <k> x
  $20 NCR (change <m> caps)`, and the same for the opponent's ante when the player wins.

## 4. Building the deck (second screen)

- At least 30 cards [documented: `HelpCaravanDeckBuilding`; owner ruling 2026-10-19].
- Two rows of cards: the top row is the deck being built ("Caravan Deck" and its count),
  the bottom row every card owned ("Total Cards:", "All Cards"); "Add W)" moves the
  bottom row's middle card up, "Remove A)" takes the top row's middle card out,
  "Randomize Caravan Deck S)" makes a new random deck, "Play Caravan F)" starts the game
  [observed: screenshots; the rows scroll left and right under the arrows]. A card owned
  twice (the same form) is offered once [documented: fandom wiki].
- The last deck built is remembered for the next game [documented: the help].
- Randomize picks between 30 cards and every card owned, at random [design: the
  screenshot's random deck had 51 of 54].

## 5. The game (third screen)

From the help messages `HelpCaravanStartingCaravans` and `HelpCaravanContractWar`
[documented], the fandom wiki [documented], the screenshots [observed] and the owner's
rulings:

- Each side draws 8 cards from its shuffled deck. The three tracks are named on the
  table: Boneyard, Redding, Shady Sands for the opponent, Dayglow, New Reno, The Hub for
  the player [observed: carved into the table, a value beside each name]. The opponent's
  caravans lie at the top, the player's below; hands and decks at the right with the
  number of cards left on each deck [observed].
- **Opening.** In turn, each side lays one numbered card or ace on one of its own empty
  caravans until both have three. Laying a card draws none. A side may instead discard a
  card and draw one (Discard Card was lit with a face card chosen in the opening; the
  help allows it; the fandom wiki says no discarding — the game's own help wins). Discard
  Track is dimmed in the opening [observed]. The opponent goes first [observed: its 9♣
  was down before the player's first card].
- **Contract war.** A turn is one of: play a card from the hand and draw one; discard a
  card from the hand and draw one; discard one of one's own caravans (all its cards).
  Buttons: "Select Card W)", "Discard Card Q)", "Discard Track E)", "Cancel R)" (Cancel
  forfeits the game) [observed].
- **Numbered cards and aces** go on the player's own caravans only. A caravan's suit is
  its last card's, its direction (up or down) is set by its last two cards. A card must
  carry on the direction or match the suit, and never repeat the last card's value
  [documented: the help].
- **Face cards** go on a numbered card or ace in any caravan:
  - Jack: that card and every face card on it leave the table.
  - Queen: reverses the caravan's direction and makes its suit the queen's. Only on a
    caravan's last card [hypothesis: the help speaks of "the hand", i.e. the caravan's
    current direction and suit, which only the last card sets].
  - King: doubles the card it is on, and each further King doubles it again (10 → 20 →
    40) [owner ruling 2026-10-19].
  - Joker on an ace: every other numbered card or ace of the ace's suit leaves the table
    (with their face cards); on 2–10: every other card of that value leaves. Several
    Jokers may go on one card [documented].
- **Value and sale.** A caravan's value is the sum of its numbered cards (aces 1, Kings
  doubling). A caravan is sold at 21 to 26 when it is higher than the opponent's on the
  same track, or the opponent's is not in range; tied values sell neither [documented].
  A value over 26 is shown in red [observed: "36"]; a value in the sold range, 21 to 26,
  in white [observed: buckallobill, in game, FNV 1.4.0.525]. The menu file sets no colour for the values
  (`caravan_game_menu.xml`, `CGM_*TrackNValue`) [observed]: the game's code does.
- **The end.** The game ends when all three tracks are sold; the side with two or three
  wins the pot [documented]. A side that runs out of cards (no card in the hand and none
  in the deck) loses [owner ruling 2026-10-19]. Cancel forfeits (a loss).
- Top right: "Ante Pot:", "Net Total:" (the player's net against this opponent) and
  "Win/Loss:" (games won and lost against it) [observed].

## 6. The opponent

Not the original's (unknown: it is in the executable) [design]:

- Skill from 0 to 1 from the share of face cards in its deck: 10 % or fewer → 0, 35 % or
  more → 1, in between in proportion [design, owner ruling 2026-10-19; a full 54-card
  set holds 14 face cards, 26 %, skill 0.64].
- Each turn it scores every legal move by the position after it (its caravans sold and
  winning, close to 21–26, none over 26, the player's sold caravans broken) and takes the
  best; the less skill, the more random noise on the scores and the less it attacks the
  player's caravans with face cards [design].
- It takes a moment over each move (0.8 to 1.4 s) [design].
- What that gives the shipped decks [observed: `crates/fnv-game/tests/caravan_data.rs`
  on the ten masters]: Ringo's (0x0015D7A2, 30 cards, 5 face cards) 0.27, Private Erwin's
  and Quartermaster Mayes' 0.30, Lacey's 0.33, Isaac's 0.56, Dale Barton's and Ambassador
  Crocker's 0.60, Jules' 0.64, Little Buster's 0.88, Johnson Nash's "Mojave Express
  Spectacular" (52 cards, 28 face cards) and No-bark Noonan's (0x0017B41B, also named
  "Ringo's Caravan Deck") 1.0. The fandom wiki's note that Johnson Nash plays many face
  cards fits.

## 7. Results (last screen)

"Press Any Button to Continue", "Winnings to Date:", "Losses to Date:", "Win/Loss
Totals:", "Biggest Ante Pot Won:" over the betting table [observed]. NVForever keeps these
per opponent (`PlayerState::caravan.records`, saved) [hypothesis: the screenshot after the
first game against Ringo reads 18, 0, 1/0, 18].

Misc statistics: Caravan Games Won and Caravan Games Lost are counted (misc-stats.md).

## 8. Not built

- The original's own 3D table layout. Its models are in `meshes\terminals\nv_caravan\`:
  the table, one model per caravan place of each side (`player_<track>_<n>`,
  `opponent_<track>_<n>`, seven places a track) holding four cards each (the card and up to
  three face cards over it, each a quarter card to the right), the decks and the hands; they
  have no animation sequences, and where the original puts them at run time is not known
  (the player's places lie off the table model) [observed: the files' nodes]. NVForever's
  game screen is drawn in 3D its own way instead (nvforever `caravan_view.rs`) [design]:
  the planks (`nv_caravan-table01.dds` and its normal map) and the cards' own textures as
  lit, shadowed cards, seen straight down where the flat screen puts them, so the screen's
  names, values, outlines and buttons lie over them; a warm lamp above the table's left.
  Moves slide: a card played goes from the hand to its place (the opponent's turns face up
  on the way), a card drawn comes from the deck, a card thrown away leaves off the left.
  The bet, deck building and results screens stay flat.
- The Caravan challenges (NVChallengeCaravanWin, …Lost, …Master).
- The help messages are shown once each, over the screen they explain (betting, deck
  building, the opening, the contract war) [hypothesis: the original's tutorial timing
  is unknown].
- On the betting screen the help box (the tutorial box, 0.8 of the view) is made smaller
  about its middle so its lines end 40 units above the bet box and left of the buttons'
  labels, in the table frame's units, scaled into the view (`help_clear`,
  `lockpick::help::box_rect`); at full size it ran through both (issue #1663) [design: the
  original's placement over this menu is not captured].

## 9. Better Caravan INI compatibility

Clean-room support for `BetterCaravan.ini` (`crates/fnv-game/src/caravan.rs`,
`engine/nvf-play/src/caravan.rs`, `engine/nvf-play/src/caravan/screen.rs`).

**Sources and provenance.** All 16 key names, `[Better Caravan]` section name, install
path (`Data\NVSE\Plugins\BetterCaravan.ini`), and key meanings come strictly from
**Better Caravan v1.9** (by lStewieAl), Nexus Mods
`https://www.nexusmods.com/newvegas/mods/81002`, specifically the public Nexus mod page
and the shipped `Data\NVSE\Plugins\BetterCaravan.ini` file with its inline `;` comments
[documented: Better Caravan v1.9, `https://www.nexusmods.com/newvegas/mods/81002`,
shipped `Data\NVSE\Plugins\BetterCaravan.ini`]. The third-party plugin's DLL/binary and
source code were never consulted.

- **Lookup and vanilla-unchanged default:** when opening the Caravan menu,
  `BetterCaravanConfig::load_from_game` reads `Data\NVSE\Plugins\BetterCaravan.ini`
  first [documented: Better Caravan v1.9 archive install path
  `Data\NVSE\Plugins\BetterCaravan.ini`] and falls back to
  `Data\Config\BetterCaravan.ini` [design: NVForever fallback lookup path, not stated in
  Better Caravan v1.9's shipped package or Nexus page] through `ModFiles` and the Data
  VFS. When neither file exists, every option defaults to off (`false` / `0` / `None`),
  leaving unmodded vanilla Caravan rules, visuals, key bindings, RNG draws, and AI
  bit-for-bit unchanged [design].
- **INI syntax** [design]: UTF-8 BOM safe (both UTF-8 `\u{FEFF}` and Latin-1-decoded
  `0xEF 0xBB 0xBF` bytes from `ModFiles`), ASCII case-insensitive section (`[Better Caravan]`)
  and key matching, `;` and `#` comment lines skipped, inline comments stripped on boolean,
  integer, and key-name values, and full trimmed text preserved (including `=` characters)
  for `sDescription*` strings.
- **AI options** (`bAiFixes`, `bImprovedAI`) [documented: key names and comments in
  Better Caravan v1.9 shipped `Data\NVSE\Plugins\BetterCaravan.ini` ("; fix various
  oversights in the AI", "; make various changes to improve NPC win detection");
  **design: `bAiFixes` and `bImprovedAI` are NVForever's own scoring heuristics on top of
  §6's `evaluate`/`choose`, NOT Better Caravan's AI implementation**]:
  - When both are off, `choose_with_config` delegates directly to vanilla `choose`.
  - `bAiFixes` [design: NVForever heuristic]: deterministically prefers immediate wins
    and avoids immediate losses without noise/attack penalties overriding them; rewards
    busting an opponent caravan over 26 and penalizes sabotaging one's own winning
    caravan, discarding a non-busted track, or playing a face card that helps/un-busts
    an opponent's caravan.
  - `bImprovedAI` [design: NVForever heuristic]: adds table-state win-threat detection
    (heavily rewards breaking an opponent's 2-track lead, penalizes leaving the opponent
    at 2 winning tracks, rewards securing a 2nd winning track and stronger 21–26 totals,
    and lowers low-skill noise and attack hesitation).
- **Move preview and winning highlight** (`bPreviewMoveResult`,
  `bHighlightWinningCaravans`) [documented: Better Caravan v1.9 shipped
  `Data\NVSE\Plugins\BetterCaravan.ini` comments ("; show the resulting caravan values
  when hovering over a move", "; highlight winning caravans in white"); design: NVForever
  display formatting]:
  - `bHighlightWinningCaravans`: renders any caravan currently winning its track (21–26
    and ahead of the opponent) in white (`Color::WHITE`) instead of amber.
  - `bPreviewMoveResult`: previews the resulting values and track winners of all six
    caravans for the highlighted placement or track discard (`Game::preview_move`) as
    `old->new` ASCII text (red if busting over 26, white if winning with
    `bHighlightWinningCaravans`, warm highlight otherwise), updating both on keyboard
    navigation and on pointer hover over a placement mark.
- **Deck modification at match start** (`iModCardMode`, `iModCardCount`) [documented:
  Better Caravan v1.9 shipped `Data\NVSE\Plugins\BetterCaravan.ini` comments ("; randomly
  modifies your deck when starting the minigame", `0 = add`, `1 = remove`, `2 = replace`,
  "; number of cards to randomly modidy"); design: match-only deck mutation and
  `MIN_DECK` floor]:
  - Active only when `iModCardCount > 0`; leaves the saved deck untouched and modifies
    only the dealt match deck (`apply_deck_mod`). `Add` adds up to `count` unused owned
    cards, `Remove` removes up to `count` cards while keeping at least `MIN_DECK` (30)
    cards when the deck starts at or above `MIN_DECK`, and `Replace` swaps up to `count`
    deck cards with unused owned cards.
- **Opponent cards face up** (`bViewNpcCards`) [documented: Better Caravan v1.9 shipped
  `Data\NVSE\Plugins\BetterCaravan.ini` comment ("; deal opponent cards face up (cheat)")]:
  draws cards in the opponent's hand face up when non-zero; face down otherwise.
- **Face-card reminders** (`sDescriptionJack`, `sDescriptionQueen`, `sDescriptionKing`,
  `sDescriptionJoker`) [documented: Better Caravan v1.9 shipped
  `Data\NVSE\Plugins\BetterCaravan.ini` comments ("; reminder shown when viewing a
  Jack/Queen/King/Joker"); design: banner placement]: displays the configured reminder
  text while viewing a matching face card in the Deck builder or Play hand
  (`active_face_description`).
- **Additional directional keys** (`sKeyUp`, `sKeyDown`, `sKeyLeft`, `sKeyRight`)
  [documented: Better Caravan v1.9 shipped `Data\NVSE\Plugins\BetterCaravan.ini` comments
  ("; additional key for the up/down/left/right input"); design: key mapping and vertical
  track stepping]: parsed case-insensitively (`parse_better_caravan_key`) and
  deduplicated against all vanilla keys bound in that stage (so overlapping configs such
  as WASD never fire conflicting actions). In `Deck`, Left/Right scroll cards and Up/Down
  map to Add/Remove. In `Play`, Left/Right step horizontally across hand cards or
  destinations, Up lifts a hand card onto the table or moves upward along a track (player
  cards to opponent cards), and Down moves downward along a track or returns to the hand
  from the bottom (`step_vertical`).
- **Debug logging** (`bDebug`) [documented: Better Caravan v1.9 shipped
  `Data\NVSE\Plugins\BetterCaravan.ini` comment ("; print various debug statements to
  help test the minigame"); design: log lines]: emits `caravan [BetterCaravan debug]: …`
  `info!` lines for loaded config path/flags, deck modification, and AI move choices;
  silent when off.
