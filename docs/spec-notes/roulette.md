# Roulette (the casino tables)

The original's roulette minigame: a roulette table of the Strip's casinos opens a menu
where the player bets the casino's chips on a double-zero wheel. Rules:
`fnv_game::roulette`; what the casino games share (the casino record, earnings, levels,
the ban): `fnv_game::casino`; the menu: nvf-play `roulette.rs`; the table, the wheel and
the chips: nvforever `roulette_view.rs`.

Sources: FalloutNV.esm (the CSNO records, the tables' scripts, the SOUN and MESG records),
`menus\roulette_menu.xml`, the models in `meshes\terminals\nv_roulette\` and their
textures, the GECK wiki (owner's export) and the owner's two screenshots of a table at The
Tops (2026-10-16, a console copy of the game: betting, and the wheel turning; kept
privately). Nothing here comes from the executable. Blackjack: blackjack.md.

## 1. Opening it

- Each casino's roulette table is an activator whose script runs, on activation by the
  player, `ShowRouletteMenuParams <casino> 1 200 0`, or a message instead while the
  casino's winnings quest has `bFloorManagerIncoming` (the Atomic Wrangler: `banned`)
  set [observed: decompiled scripts `RouletteTableTheTopsScript` 0x0014C137,
  `GomorrahRouletteTableSCRIPT` 0x00161E42, `RouletteTableUltraLuxeScript` 0x001614D7,
  `RouletteTableAtomicWranglerScript` 0x0016150E, `VikkiAndVanceRouletteTableSCRIPT`
  0x0016206E]. Activated by anyone else the table plays its `Forward` group.
- `ShowRouletteMenuParams casino [minBet] [maxBet] [maxWinnings]` (srmp, 0x1256) opens the
  roulette menu for a casino form [documented: GECK wiki ShowRouletteMenuParams; opcode:
  script-functions-catalogue.md]. Every table passes min 1, max 200, max winnings 0
  [observed: the five scripts]; 0 is the casino's own max winnings [hypothesis].
- The menu opens only when the player carries the casino's chips and is not banned there
  (`fnv_game::casino::opening`) [documented: GECK wiki ShowBlackJackMenuParams, the same
  requirement]. Otherwise a corner message says why [hypothesis: the words are
  NVForever's].
- It is a menu: the world and the game clock stand still (`PlayState::Menu`), menu code
  1082 to scripts (`RouletteMenu` in `fnv_ui::value::MENU_CLASSES`) [documented: GECK wiki
  MenuMode].
- Placed tables: The Tops (0x0011ED93 in `TOPSCasino`), Gomorrah (3), the Ultra-Luxe (4),
  the Atomic Wrangler and Vikki & Vance [observed: `fnvtool world refs --interiors`].

## 2. The casino record (CSNO)

As blackjack.md §2, and [observed: all five casinos]:

| Field | Meaning |
|---|---|
| DATA 40, u32 | max winnings (The Tops 10000, Gomorrah 9000, Ultra-Luxe 15000, Atomic Wrangler 5000, Vikki & Vance 2500) |
| DATA 44, FormID | the casino's chip (CHIP), staked and paid at the table |
| MOD4 | the roulette table's model: `terminals\NV_Roulette\NV_Roulette-Table<x>.NIF` (`x`: none for The Tops, `G`, `U`, `A`, `V`) |
| MODL (7th) | `terminals\NV_Roulette\NV_Roulette-Chip.NIF` |

## 3. The wheel and the bets

- **The wheel**: 38 pockets, 1 to 36, 0 and 00 [documented: the help message
  `HelpRoulette` 0x00172095]. Their order round the wheel: the number strip
  `textures\terminals\nv_roulette\nv_roulette03.dds` (2048 × 128, 38 equal cells) holds,
  from its left edge, 35 23 4 16 33 21 6 18 31 19 8 12 29 25 10 27 00 1 13 36 24 3 15 34
  22 5 17 32 20 7 11 30 26 9 28 0 2 14 [observed: the picture]: the American wheel's
  order. Red: 1 3 5 7 9 12 14 16 18 19 21 23 25 27 30 32 34 36 [observed: the strip and
  the felt].
- **Each pocket as likely** [hypothesis].
- **The bet points**: `nv_roulette-points.nif` has 144 nodes `A01`–`F24` in a grid
  (columns 2 apart along X from −24 to 22, rows 2.67 apart along Y from −2.67 to 10.67)
  and 15 nodes `S01`–`S15` around it [observed: the file's nodes]. Read against the
  felt [hypothesis: each place's bet, from where it lies]:
  - even columns (`02` … `24`, X −22 … 22) are the twelve columns of numbers, odd ones the
    lines between them, column `01` the line along the zeros;
  - rows `B`, `D`, `F` the three rows of numbers (1 4 7 … nearest the player, 3 6 9 …
    farthest [observed: the felt]), `C` and `E` the lines between them, `A` the near edge;
  - on a number: straight-up; on a line between two numbers: split; where four meet:
    corner; on the near edge a street (on a column) or a six line (on a line);
  - along the zeros: splits 0-1 (`B01`) and 00-3 (`F01`), baskets 0-1-2 (`C01`), 0-00-2
    (`D01`) and 00-2-3 (`E01`), the top line (`A01`);
  - `S01` 00, `S02` the 0-00 split, `S03` 0 (beyond the zeros); `S04`, `S07`, `S10` the
    dozens; `S05` 1-18, `S06` even, `S08` red, `S09` black, `S11` odd, `S12` 19-36 (in the
    felt's order "1 to 18, EVEN, ◆, ◆, ODD, 19 to 36" [observed: the screenshot]);
    `S13`–`S15` the columns' "2 to 1" at the far end.
  The help message lists exactly these kinds: straight-up, split, street, six line,
  corner, basket (the three), top line; 1-18/19-36, red/black, even/odd, dozens, columns
  [documented].
- **Payouts**: a winning bet gets its stake back and 36 / (numbers covered) − 1 times it:
  35, 17, 11, 8, 5, 2 and 1 to 1; the top line 6 to 1 [hypothesis: the original's are not
  in the plugins; these are a double-zero table's].

## 4. A round

- **Betting**: the current bet starts at 5 (within the limits) and moves 1 under 10, 5
  under 50, 25 from there, between the table's min and max [design, as blackjack's].
  Place Bet adds it to the point under the marker; a spot holds at most the table's max,
  and the bets on the table no more than the chips carried [hypothesis]. Remove Bet takes
  the bet at the marker back.
- **Finish Bet** spins: the chips on the table are taken from the inventory, the wheel
  turns for 9.83 s (§6), then the bets covering the pocket are paid and the table clears.
  The "Chips" row shows the chips carried less those on the table [hypothesis].
- **Earnings and bans**: the net of each spin goes to the player's earnings at the casino
  and the winnings level (blackjack.md §4, `fnv_game::casino`); at the max winnings the
  player is banned and the menu closes after the result [hypothesis].
- **Leaving** (Exit) takes the bets on the table back; it is not offered while the wheel
  turns [observed: Exit dimmed on the screenshot of the turning wheel].

## 5. The menu (`menus\roulette_menu.xml`)

- The information box: 400 × 150, 10 in from the main rectangle's left and bottom, the
  bracket of lines, four rows in font 7 35 apart: the bet, the bets on the table, the
  chips and the casino's winnings [documented: `ROM_InfoRect`, its four displays].
  NVForever's titles are its own words, "Your bet:", "On the table:", "Your chips:" and
  "Won at <casino>:" [design, #4495: in place of the titles read from the original's
  screen].
- Six buttons right-aligned 50 in from the right: Place Bet (W), Remove Bet (F), Finish
  Bet (S), Decrease Bet (Q), Increase Bet (E), Exit (R) [documented: the order and
  `_PCButton_*`; observed: the words]. The file puts them 144 above the box; the
  screenshots show the first about 606 down the screen, 52 apart [observed]. NVForever
  follows the screenshots.
- Dimmed: Remove Bet and Finish Bet with nothing on the table; all six while the wheel
  turns [observed: the screenshots]. Place Bet where the bet cannot go, Decrease and
  Increase at the limits [design].
- The status text (`ROM_StatusText`, font 7, at the top left): "Bet: …" and "Payout: …"
  [observed: on the betting screenshot, with nothing after them]. NVForever fills them
  for the point under the marker ("Bet: Split 17-20", "Payout: 17 to 1"), and puts a
  refusal or the spin's result there [hypothesis].
- The help message `HelpRoulette` (0x00172095, Message Box, Auto Display) comes up the
  first time the menu opens, as lockpicking's [observed: the flags; hypothesis: once per
  game].
- Keys besides the file's: the arrow keys move the marker to the next bet point that way,
  the mouse moves it to the point under the pointer, a left click places a bet there and
  a right click removes it, Esc and Tab leave [design].
- Sounds [observed: SOUN records named for them; design: which event each is for]:
  `GAMERouletteIncreaseBet` / `…DecreaseBet` as the bet changes (Decrease also when a bet
  is taken back), `…ValidBet` as a bet goes down, `…InvalidBet` when refused, `…Spin` as
  the wheel turns, `…Win` when a spin pays.

## 6. The table on the screen

- The table: the casino's MOD4 model: felt, wheel (`Roulette_Wheel`, in `Wheel_Housing`),
  ball (`Ball`, under `Point01`), three point lights (`Omni01`–`03`, under the camera) and
  a camera `Camera01` aimed at `Camera01.Target` [observed: the file's nodes]. The felt's
  picture differs by casino (`nv_roulette02<x>.dds`).
- `Spin_1`, `Spin_2`, `Spin_3` (9.83 s each) move `Point01`, `Ball`, `Roulette_Wheel`,
  `Wheel_Housing`, `Camera01.Target` and `Camera01`: the wheel turns, the ball runs round
  and drops into a pocket, the camera flies from over the felt to the wheel (the second
  screenshot) [observed: the sequences]. The camera's path is an `NiLookAtInterpolator`
  (a position path, aimed at the target node), read for its position (`fnv_nif::anim`).
  NVForever plays one of the three at random [hypothesis].
- In a given sequence the ball comes to rest at one place on the wheel, so the wheel is
  turned about its axis by the angle that brings the spun pocket under the ball
  [hypothesis]. The pocket angles: the strip's U runs once round the wheel's shape,
  angle = −15.33° + 360° × U about the shape's Z [observed: `fnvtool nif decode`, vertex
  angle against U].
- The camera's view is 34.5° high (its NiCamera frustum, ±0.311 at a near plane of 1)
  [observed]; betting, the camera stands where the file puts it, looking down the felt
  from the player's side.
- The bet points are placed with the table's origin [hypothesis: the points model has no
  place of its own in the CSNO record]; the marker is `nv_roulette-bet.nif`
  (`Bet_Selector`, a ring; `Bet_Invalid` where the bet cannot go [hypothesis: by the
  names]); the bets are stacks of `nv_roulette-chip.nif` [design: the height by the chips
  the amount takes in 100s, 25s, 10s, 5s and 1s].
- Not built: the dealer at the table, the chips' colours by amount, the dealer's chip
  rake (`meshes\animobjects\nv_roulette_dealerchips.nif`), the "Roulette Games Played"
  statistic, the challenge `ChallengeRouletteSCRIPT` (its trigger is not read).

## 7. Tests

- `fnv_game::roulette` and `fnv_game::casino` unit tests: the strip, every bet point, the
  payouts, placing, limits and paying.
- Smoke `roulette` (tools/smoke/scenarios.toml): The Tops' table activated with 500
  chips, the help the first time, 25 on 17 and 10 on red, the ball made to stop in 17;
  screenshots of the help, the bets, the wheel turning and the result.
- Test aid `NVF_PLAY_TEST_ROULETTE` (nvf-play `roulette.rs`).

## 8. Open questions for the owner

- The words of the status line and the refusals (screenshots of a win, a loss and a
  refused bet would settle them).
- The payouts, the per-spot or per-table limit, and whether the chips are taken as bets go
  down or as the wheel turns.
- Where the winnings levels 1–3 lie (shared with blackjack).
