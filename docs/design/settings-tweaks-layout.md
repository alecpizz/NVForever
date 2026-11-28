# The Tweaks tab: groups and categories (draft for the owner's review)

The Tweaks tab shows one of Stewie's Tweaks' groups at a time, picked by its first row,
Category; each group's rows stand in categories the player opens and closes, closed by
default [design: owner 2026-11-15]. This note lists where every Tweaks row goes, as
`crates/nvf-settings-layout/src/lib.rs` `SECTIONS` places it (`group_section`), ahead of
#1892, so its rows fall into place when it lands (`resolve_layout` skips a key that is not
registered).

**Source**: the 560 rows #1892 registers (`tweak-<key>`) at its head 25bc53a28
[documented: `engine/nvf-play/src/tweaks.rs`, `TweaksPlugin::build`, its `switch!`,
`weightless!`, `part!`, `amount!`, `sound_list!` and plain `Setting` rows]; the labels are
the PR's. The branch's later head a6c8c7ec2 only merges main: `tweaks.rs` and
`docs/design/tweaks.md` are the same.

**The rule** [design: owner 2026-11-15]:

- **Group**: the catalogue group of the INI key the row's note names
  [documented: #1892 `docs/design/tweaks.md` "Catalogue"; `### Fixes` has no rows], in
  this order: Gameplay, Combat, Quality of Life (`Quality-of-life`), HUD, Menus, Hotkeys,
  Cheats (`Cheat`), Misc.
- **Categories**: about 4 to 25 rows each, named in Title Case (a, an, the, and, or, of,
  at, in, on, to in lower case). Gameplay's are the owner's approved draft; the other seven
  groups' are a draft to review.
- **Order in a category**: the lists first, then the switches, then the sliders, each by
  label. A row that only matters with another (its note says "With X", its label starts
  "X:", a sub-option of X) comes right after that row, marked ↳ below. Choices with 7 or
  fewer values (Perks per Level, Date Format, Companion Compass Colour, Place Name:
  Distance) are dropdown lists alongside the three sound lists (`sound_list!`); choices
  with more than 7 values (S.P.E.C.I.A.L. Points, Skill Points: Base, Steepest Walkable
  Slope: `Slider::choice`) count as sliders: the page draws them with a meter [documented:
  `pause.rs`, the value cell].

**Rows whose INI key is not in the catalogue** (a sub-option's own key, or none), placed
with the row they belong to:

- Gameplay › Character and Experience: Skill Points: Base (`iSkillPointBase`), Skill
  Points: per Intelligence (`fPointsPerInt`), under Own Skill Points Rule
  (`bModifySkillPointsEarned`).
- Gameplay › Locks, Hacking and Stealing: the ten Pickpocket Overhaul options with only
  their option's key (`fPlayerAgilityMult` … `bRewardXP`), under Pickpocket Overhaul.
- Gameplay › Travel and World: Hold to Activate: Containers and Bodies, Furniture, Water
  (`bContainers`, `bFurniture`, `bWater`), under Hold to Activate.
- Quality of Life › Weapons and Aiming: Night Vision Toggle: Off Until Switched On, By Day
  Too (`bDisableVisionByDefault`, `bAllowNightVisionDuringDay`), under Night Vision Toggle.
- Menus › Maps: No Question: Shift Removes, Remove When On It (`bPlaceMarkerShiftToReset`,
  `bResetIfHovered`), under No Place Marker Question.
- Misc › Controller and Keys: Left/Right Stick Dead Zone (`iControllerDeadzone`,
  `iControllerDeadzoneRS`), Left/Right Trigger Press Point (`iDeadzoneLT`, `iDeadzoneRT`),
  under Controller Dead Zones and Trigger Dead Zones.
- Misc › Camera: Kill Camera Outside V.A.T.S. (NVForever's own row; its note names
  `bUseHitLocationInCinematicKillcams`, a Misc key).

**Open points for the owner**:

- Gameplay › Locks, Hacking and Stealing has 37 rows (12 of them Pickpocket Overhaul and
  its options); split into Locks and Hacking (16) and Stealing (16) if it is too long.
- HUD › Other has 3 rows the catalogue files under HUD but no HUD category fits: Facial
  Hair for Women, No Hotkey Wheel, Date Format.

## The list

Group (rows) › category (rows): row labels in the page's order.

### Gameplay (198)

- **Character and Experience** (19): Perks per Level; Free Wild Wasteland; Frenzied Kills Earn Experience; No Skill Tags; Own Skill Points Rule; ↳ Skill Points: Base; ↳ Skill Points: per Intelligence; Skills Capped by S.P.E.C.I.A.L.; ↳ Skill Cap Spares Tagged Skills; ↳ Skill Cap Base; ↳ Skill Cap Most; ↳ Skill Cap per Attribute Point; ↳ Skill Cap per Luck Point; Experience Earned; Experience for Keys; Experience for Passwords; Kill XP by Level; Level Cap; S.P.E.C.I.A.L. Points
- **Movement and Jumping** (18): Armour Causes Sinking; Full Pace While Aiming; Jump Swims Upwards; Speed Scales the Jump; Action Points per Jump; ↳ Action Points per Jump: Only in Danger; ↳ Action Points per Jump: Allow Low AP; Action Points per Second Running; Action Points per Second Sneak-Running; ↳ Sneak-Running AP: Endurance Factor; Agility Moves Faster; Agility Scales Jump Height; Backward Pace; Jumping on a Crippled Leg; Sideways Pace; Slow Breath Regen; Steepest Walkable Slope; Wading Pace
- **Weapons and Reloading** (26): Aiming Draws the Weapon; Full Reload Swaps Ammo; Hold Breath Through a Scope; ↳ Held Breath: Activate Key; ↳ Held Breath AP; ↳ Held Breath Sway; Jams at Reload Pace; Mouse Wheel Changes Weapon; No Hotkeys While Reloading; No Jams in God Mode; NPCs Keep Spent Cases; Per-Bullet Looping Reloads; ↳ Per-Bullet Reloads: Most Queued; ↳ Per-Bullet Reloads: Rounds a Press; Power Attack From a Block; Ready Does Not Draw; Reload Only When Empty; Reload While Firing; Shots Spare Weapons; Spare the Blade on the Dead; Wheel Zooms Scopes; Melee Reach; Others' Weapon Volume; Scope Zoom Time; Semi-Auto Shot Queue; Your Weapon Volume
- **V.A.T.S.** (7): Target Projectiles in V.A.T.S.; ↳ Target Projectiles: Mines Too; ↳ V.A.T.S. Ignores Non-Explosive Projectiles; ↳ V.A.T.S. Targets Your Own Projectiles; V.A.T.S. Drops Shots at a Lost Weapon; V.A.T.S. Hip Fire; V.A.T.S. Throwables Use Visibility
- **Explosives** (11): Contact Mines (Others); Contact Mines (Player); Disarming Needs Skill; ↳ Disarm Chance Multiplier; ↳ Set-Off Chance Multiplier; Explosion Radius Buff; Explosions Leave Items Be; Mine Blasts Are Heard; NPCs Drop Live Grenades; Planting Grenades Needs the Skill; Strength Resists Blasts
- **Damage and Health** (24): Aid at Full Health; Automatic Criticals Not Cut by Fire Rate; Character Radiation; ↳ Character Radiation: Sickness Too; Crippling Never Disarms; Deadly Falls Take the Legs; Drowning Takes AP First; Hardcore Rules One by One; ↳ Hardcore Rules: Ammo Has Weight; ↳ Hardcore Rules: Companions Cannot Die; ↳ Hardcore Rules: Sleep Heals; Keep Fall Height on Load; No NPC Addiction; No Player Stagger; Power Armor Blocks Radiation; Power Armor Without Training; Sneak Attacks Standing; Armour Condition Penalty; Crippled Legs: Pain When Falling; Endurance Softens Falls; Falls in Power Armor; Limb Damage in Power Armor; Sleep Healing Min Duration; ↳ Sleep Healing: Stat Needs Full Duration
- **Weight** (7): Held Items Weigh; Run While Overencumbered; ↳ Overencumbered Jump AP; ↳ Overencumbered Running AP; Worn Armor Weighs Nothing; Worn Power Armor Weighs Nothing; Action Points Back When Over-Encumbered
- **Locks, Hacking and Stealing** (37): Caught Pickpocketing Means Detected; Forcing Breaks the Pin, Not the Lock; Grabbing Is Stealing; ↳ Grabbing Is Stealing: Min Value; Hacking: Dud When Attempts Full; Hacking: Duds Spare Tried Words; Hacking: No Repeat Guesses; Hacking: Symbols Do Nothing; Hacking: Mark Guesses as Duds; ↳ Hacking: Keep Matching Characters; Hacking: Compact Guess Log; Hacking: Replenish Bonus Attempts; Name the Key; No Grabbing Owned Items; No Terminal Lockout; Pick Any Lock; Pickpocket Overhaul; ↳ Pickpocket Overhaul: Experience; ↳ Pickpocket Overhaul: Free Items Always; ↳ Pickpocket Overhaul: Agility; ↳ Pickpocket Overhaul: Base Chance; ↳ Pickpocket Overhaul: Being Seen; ↳ Pickpocket Overhaul: Item Value; ↳ Pickpocket Overhaul: Item Weight; ↳ Pickpocket Overhaul: Luck; ↳ Pickpocket Overhaul: Sneak; ↳ Pickpocket Overhaul: Target Perception; ↳ Pickpocket Overhaul: Worn Items; Pickpocket the Fleeing; Quiet Theft; Remember Bobby Pin Wear; Search the Unconscious; Skip Easy Hacks; ↳ Skip Easy Hacks: Margin; Skip Easy Locks; ↳ Skip Easy Locks: Margin; Force Lock Near the Sweet Spot
- **Trading and Repair** (24): Barter Prices by Kind; ↳ Barter by Kind: Purified Water Unscaled; ↳ Barter by Kind: Buying Aid; ↳ Barter by Kind: Buying Ammo; ↳ Barter by Kind: Buying Armor; ↳ Barter by Kind: Buying Misc; ↳ Barter by Kind: Buying Weapon; ↳ Barter by Kind: Buying Weapon Mod; ↳ Barter by Kind: Selling Aid; ↳ Barter by Kind: Selling Ammo; ↳ Barter by Kind: Selling Armor; ↳ Barter by Kind: Selling Misc; ↳ Barter by Kind: Selling Weapon; ↳ Barter by Kind: Selling Weapon Mod; Crafting Uses Repair; Merchants Keep to Their Lists; Repair Caps Go to the Till; Repair Kits in the Repair List; Repair Only at a Workbench; Weapon Mods Need the Skill; Barter Lowers Repair Costs; Crafted Item Condition; Experience per Repair; Price for Worthless Things
- **Companions** (11): Hostiles Prioritize Player; ↳ Hostiles Prioritize Player: At Start; ↳ Hostiles Prioritize Player: Mid-Fight; ↳ Hostiles Prioritize Player: Detection; Light Step for Companions; No Companion Item Damage; Revive Unconscious Companions; ↳ Revive: Companions Stay Down; ↳ Revive: On Changing Cells; Summon Companions from the Map; ↳ Summon Companions: Knocked-Out Too
- **Travel and World** (14): Fast Travel Costs a Nuka-Cola; Fast Travel Encumbered; Hold to Activate; ↳ Hold to Activate: Containers and Bodies; ↳ Hold to Activate: Furniture; ↳ Hold to Activate: Water; No Encounter Zone Level Lock; No Fast Travel; No Travel on a Crippled Leg; Quick Use; Sleep on Chairs; Wait Only When Seated; Clear Marker When Near; Mark Nearby Places

### Combat (34)

- **In Combat** (5): No Battle Music; No Enemies on the Compass; No Healing In Combat; No Pip-Boy In Combat; Action Points per Kill
- **V.A.T.S.** (5): First Person V.A.T.S.; V.A.T.S. Burst Stops on a Kill; V.A.T.S. Ignores Companions; V.A.T.S. Starts on the Head; Action Points to Enter V.A.T.S.
- **Criticals and Sneak Attacks** (6): Fatal Criticals Always Gib; No Sneak Criticals; NPCs Sneak Attack the Player; Sneak Criticals Head Only; Sneak Criticals Melee Only; Critical Damage
- **Weapons and Armour** (12): Enemies Can Disarm You; Enemy Blows Hit Where They Land; Hold and Release Throws; Keep Chosen Ammo; No Condition Damage Buffer; No Condition Damage Penalty; No Spin-Up; NPC Armour DR Ignores Condition; NPC Armour DT Ignores Condition; NPC Weapons Hit Whole; Weapon Requirements Matter; Action Points per Power Attack
- **Companions and Blasts** (6): Companions Keep Their Weapons; Meltdown for Companions; No Self Meltdown Damage; No XP for Companion Kills; Own Blasts Spare the Player; ↳ Own Blasts Spare Non-Hostiles

### Quality of Life (76)

- **Controls and Movement** (10): Activate Doesn't Stand Up; Console Clicks Skip the Player; Hold to Keep Jumping; Hold to Sneak; Hold to Wait; Move While the Pip-Boy Comes Up; No Knockdowns in God Mode; No Landing Animation; Switch Device by Pressing; Turn Further While Seated
- **Weapons and Aiming** (19): D-Pad Changes Weapon; Double-Tap Reload Changes Ammo; Fine Weapon Models for Everyone; Holster in Real Time; Hotkey Draws and Holsters; Night Vision Toggle; ↳ Night Vision Toggle: By Day Too; ↳ Night Vision Toggle: Off Until Switched On; No Iron Sight Zoom; No Weapon Movement Penalty; NPCs Drop Holstered Weapons; Reload Swaps Spent Ammo; Shift Ignores Friends in V.A.T.S.; Swap Ammo While Holstered; Unassign Hotkeys; V.A.T.S. Puts the Weapon Back; Aiming Turn Speed: Across; Aiming Turn Speed: Up and Down; Scope Delay (ms)
- **Items and Repair** (7): Equip Broken Items; Hide Quest Items in Misc; Keep Broken Items Equipped; No Poison Confirmation; Repair Needs the Weapon's Skill; Unequip Broken Armour; Whole-Stack Pickup Figures
- **Companions** (5): Quiet Companions; Robot Companions Heal With Scrap Metal; ↳ Scrap Metal: Health Each; ↳ Scrap Metal: Health per 10 Repair; ↳ Scrap Metal: Robotics Expert Bonus
- **Minigames and Machines** (6): Faster Hacking Exit; No Minigame Help; Pick Locks Even With the Key; S.P.E.C.I.A.L. Machine: Start on Summary; Say My Skill; Slots: Hold to Spin
- **Travel and Loading** (5): Fast Travel From Anywhere; Fast Travel in the Air; Faster Sleep and Wait; Pause On Load; Plain Screen Going Outside
- **Messages and Sounds** (8): Mark Strangers; No Active Quest After Completing One; No Active Quest From New Objectives; No Message for Spent Books; Quiet Landings in God Mode; Quiet Reputation; XP Sound at the Level Cap; Less Frequent Pain Sounds
- **Conversations and Radio** (4): Click to Continue Dialogue; Keep Stations Out of Range; Music While Talking; Radio While Talking
- **Screen and Camera** (12): Faster Controller Camera Circling; Move During V.A.T.S. Camera; No Chem Wore-Off Effect; No Explosion Blur; No Interior Fog; Scale Explosion Shake; ↳ Scale Explosion Shake: Camera; Shift Screenshot Hides the HUD; Skip Death Camera; ↳ Skip Death Camera: By Attacking After; Skip Kill Camera; Blood Only on Big Hits

### HUD (76)

- **Messages** (16): Critical Hit Messages Name the Limb; Discoveries in the Corner; Map Marker Added Names the Place; No Caps Added Line; No Dehydration Messages; No Location Banner; No Objective List; No Quest Failed Banner; No Sleep Deprivation Messages; No Unconscious Message; No XP Meter in Combat; Objectives on Arrival; ↳ Objectives on Arrival Gap (s); Quiet Experience; Quiet Karma; Item Added Line Time
- **Compass and Place Names** (21): Companion Compass Colour; Compass Fades on the Left; Compass Pips Only in Danger; Compass: Only Mapped Places; Doors on the Compass; ↳ Doors on the Compass: Fade by Distance; ↳ Doors on the Compass: Reach Inside; ↳ Doors on the Compass: Reach Outside; Fade Pips by Distance; Fade Places by Distance; Foes in the Usual Colour; Nearest Unfound Place; Place Name on the HUD; ↳ Place Name: Distance; ↳ Place Name: Only Places Found; ↳ Place Name: Widest Angle; Quest Marker Height; Region Name on the HUD; Your Marker's Height; Quest Markers Within; Recently Dead on the Compass (s)
- **Crosshair** (6): Crosshair Down the Sights; Invisible Foes Stay Hidden; No Crosshair in First Person; No Crosshair in Third Person; No Crosshair While Reloading; No Red Crosshair
- **Bars and Indicators** (19): Ammo Label Under the Count; Armor Condition on the HUD; ↳ Armor Condition: In the Weapon's Place; ↳ Armor Condition: Flashing Below; ↳ Armor Condition: Red Below; ↳ Armor Condition: Shown Below; Fatigue Indicator; Health Bars Count Buffs; Hide Grenade Indicator; Mark Objects About to Explode; More Precise RAD Meter; ↳ More Precise RAD Meter: Decimal Places; No Ammo Counter; No Crippled Limb Figure; No Indicator for Harmless Blasts; Flashing Weapon Condition Below; Red Action Point Bar Below; Red Health Bar Below; Red Weapon Condition Below
- **Sneaking** (5): Detected by Whom; ↳ Detected by Whom: Names Shown; No Sneak Label; Shift to Talk While Sneaking; Sneak Label While Standing
- **Prompts and Subtitles** (6): Hide Unknown Names; Mark Unvisited Doors; Show Use Password; Subtitles Name the Speaker; Water Sources Show H2O; Subtitle Distance
- **Other** (3): Date Format; Facial Hair for Women; No Hotkey Wheel

### Menus (100)

- **Pip-Boy Items** (13): Clicking the Tab Sorts; Compare Armor; Compare Weapons; Mark Other Ammo; No Figures for Quest Items; No Right-Click Drop; One Item's Figures; Q Drops Items; Remember Item List Places; Rounds per Shot; Show Weapon Poison Effects; Skill Book Effects; Usable Ammo First
- **Pip-Boy Stats and Data** (15): 12-Hour Pip-Boy Clock; Detailed Stats; Effect Time Left; Experience as a Percentage; Hide an Empty Reputation List; Hide Finished Quests; Hide Read Notes; Mark Notes Unread; Mark Repeating Challenges; No Show Location; Notes of Every Quest; Quest Text Over the Pip-Boy; Show Fame and Infamy; Sorted Statistics; Tag Skills in Bold
- **Maps** (15): Companions in the Locations' Font; Companions Near You on the Map; Exterior and Interior Door Icons; Knocked-Out Companions on the Map; Local Map Shows Reset Areas; Map Marker Distance; Map Recentre Key; Map Remembers Its Place; Map Zooms at the Pointer; New Places Red on the Map; No Place Marker Question; ↳ No Question: Remove When On It; ↳ No Question: Shift Removes; No Reputation on Map Markers; Quiet Undiscovered Markers
- **Containers and Barter** (20): Barter Shows Caps After; Barter Total Past the Merchant's Caps; Containers Show Equipped Ammo; Hide Hotkeyed Items in Barter; Hide Hotkeyed Items in Containers; Hide Worn Items in Barter; Hide Worn Items in Containers; Matching Categories; ↳ Matching Categories: Keep Empty; Quantity Box Shows the Price; Quantity Box Shows the Weight; Quantity Menu: Companion Carry Cap; Quiet Category Titles; Reverse Category Wheel; Right Click for the Previous Category; Store All; Take All Confirmation; ↳ Take All Confirmation: From This Many; Take All Stays Open; Weight Warning
- **Repair and Crafting** (8): Hide Hotkeyed Items in Repair; Recipe Categories in Capitals; Repair All Confirmation; ↳ Repair All Confirmation: From This Price; Repair Items Preview; Repair List: Low Condition First; Repair List: Same Item First; Repairers Take Any Armor
- **Level-Up and Perks** (6): Level Up: Wheel Sets Skills; Living Anatomy Shows DR; No Ranks for Traits; Perk Plugins; Perks First When No Points; Preview Perks on Level Up
- **Terminals and Minigames** (8): Blackjack Hand Total; Dim Read Terminal Entries; Lockpicking Follows the Time Speed; Lockpicking With the Movement Keys; Mouse Turns the Lock; No Leaving a Hack; Number Keys on Terminals; Tab Closes Terminals
- **Messages and Conversations** (4): Custom Popup Icons; Keep Acting Notes; No Pointer in Conversations; No Pointer Over Message Boxes
- **Main and Pause Menus** (4): Continue Shows Its Save; No Loading Tips; Quick Load From the Menus; Tab Goes Back in the Pause Menu
- **Keys and Mouse** (7): Key Repeat Acceleration; ↳ Key Repeat Acceleration: First Delay; ↳ Key Repeat Acceleration: First Speed; ↳ Key Repeat Acceleration: Second Delay; ↳ Key Repeat Acceleration: Second Speed; WASD as Arrow Keys in Menus; Horizontal Look Speed

### Hotkeys (9)

- **Hide HUD Key** (5): Hide HUD Key: Action Points and Ammo; Hide HUD Key: Hardcore Needs; Hide HUD Key: Health and Compass; Hide HUD Key: Hidden After Loading; Hide HUD Key: Sneak
- **Other Keys** (4): Place Marker Here Says So; Repeat Activate Takes Only; Console Key; Screenshot Key

### Cheats (16)

- **Weight** (5): Weightless Aid; Weightless Ammo; Weightless Armor; Weightless Misc Items; Weightless Weapons
- **Hacking, Stealing and Casinos** (5): Hack Any Terminal; No Casino Bans; No Casino Bet Limit; Pickpocket Again; Pickpocket Worn Items
- **Travel, Rest and Skills** (6): Console Homes; Fast Travel Near Enemies; Free Crafting in God Mode; Reallocate Skill Points on Level-Up; Sleep in Owned Beds; Wait Anywhere

### Misc (70)

- **Sound and Radio** (14): Level-Up Sound A; ↳ Level-Up Sound A: Every N Levels; Level-Up Sound B; ↳ Level-Up Sound B: Every N Levels; Location Discovered Sound; Armour Rustle From the Body; Heartbeat Fades; ↳ Heartbeat Fades: Seconds; No Helmet Voice Filter; No Hotkey Sounds; Quiet Companion Footsteps; Radiation Brings Radio Static; Radio Songs Volume; Radio Talk Volume
- **Camera** (7): Kill Camera Focuses on the Hit Location; Kill Camera Outside V.A.T.S.; V.A.T.S. Stops Following the Dead; Vanity Camera Returns to First Person; View Follows the Weapon; Wheel Zoom Needs Toggle View; Kill Camera Chance
- **Effects and Animation** (8): Armor Prevents Blood Decals; Ash Piles Fit the Body; Detached Beams; No Screen Blood in Power Armor; No Stealth Shimmer in the Pip-Boy; No Turning Animation; Splash Kills Can Gib the Torso; Weapon Impact Effects
- **Controller and Keys** (12): Controller Dead Zones; ↳ Left Stick Dead Zone; ↳ Right Stick Dead Zone; Invert Look Left/Right; Keyboard and Mouse With a Controller; More Responsive Controller Aiming; No Weapon Hotkeys; Pip-Boy Buzz; Shared Keys; Trigger Dead Zones; ↳ Left Trigger Press Point; ↳ Right Trigger Press Point
- **Console** (9): Better Free Camera; Console in the Pause Menu; Console Lines Name Their Plugin; Console Output File; Console Shows Models; Console Text Shadow; Console Times; Count Repeated Console Lines; Remember Console History
- **Menus and Conversations** (4): No Skipping Lines; Pip-Boy Opens on Items; Terminal Fade Time; ↳ Terminal Fade Time: Seconds
- **Starting and Loading** (5): Continue at Start; God Mode Off on Load; No Character Remake Offer; No Name Length Limit; Press to Leave Loading Screens
- **World and Items** (11): Fast Travel Takes No Time; Improved Weather; ↳ Improved Weather: Short Trip; Killing Knockdowns Count; No Selling; No Skill Books at 100; No Spent Cases; Pick Up Disarmed Mines; Picked Containers Stay Shut; Picked Doors Stay Shut; World Borders: Warning Only
