# Spec note: extender event handlers

Covers registering a function script for a game event, removing it, dispatching events
of one's own, and the handler priority queries.

Filled for task G4 of the extender plan ([extender-core-tasks.md](../handoff/extender-core-tasks.md))
from the GECK wiki export, as input to E1–E3. Everything the pages state is tagged
`[documented: <page>]`; what they leave out is **open (to observe)**. Not in the
export: none now. (User-Defined Event, Event Handler Priority System and New xNVSE Event
Filtering System arrived on 2026-10-25 in `NVSE.xml`; GetCurrentEventName and
DispatchEventAlt were pasted by the owner from geckwiki.com, 2026-10-25.)

Behaviour is our own wording of the GECK wiki export (`C:\fnv-private\geckwiki\`,
private); no extender source, no opcode numbers or argument encodings (those come only
from a cited document or from compiling our own test scripts with xNVSE,
[extender-compat.md](../design/extender-compat.md) §4.1).

## 1. The model

An event handler is a user-defined function (or a lambda) registered for an event by
name; when the event happens, every matching handler is called with the event's
arguments, without any script being attached to the objects involved (NVSE 4.6, ported
from OBSE) [documented: NVSE Event Handling; SetEventHandler]. The handler's parameter
list must match the event's arguments in order; the names are free [documented:
SetEventHandler]. After a handler runs, its variables are cleared as for any
user-defined function [documented: SetEventHandler].

Handlers managed by the extender are the ones set with SetEventHandler and
SetEventHandlerAlt; plugin-specific ones (JIP's "LN" events such as OnCellEnter, and
the older `SetOn…EventHandler` functions) are separate and not covered here
[documented: GetEventHandlers; SetEventHandlerAlt].

## 2. The built-in events

[documented: NVSE Event Handling]. "First" and "second" are the arguments passed, in
order (and the targets of the `"first"`/`"second"` filters, §3).

### 2.1 Game events

| Event | First | Second | Notes |
|---|---|---|---|
| OnActivate | the activated reference | the activating reference | **Does not block activation**, unlike an OnActivate block; runs **before** the object's OnActivate block, and even when activation will be refused (a locked or inaccessible door); runs with `GetSelfAlt == 0` and after `ShowOff:OnPreScriptedActivate` on scripted `Activate` (suppressed when pre-scripted vetoes) [observed: 2026-11-24, Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, ShowOffNVSE 184, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `tools/capture/nvse/catalogs/wave6d1_pre_scripted_activate_order.json` probes `w6d1p2_11_on_activate_and_pre_scripted_order` (`ord=S:PRE,\|caps=0->0`) and `w6d1p3_11_on_activate_noveto` (`ord0=S:PRE,ACT,\|ord1=S:PRE,ACT,\|act(s0=1,sR=0,a1=1,a2=1)`); no DLL, PDB, or third-party source consulted] |
| OnActorEquip (alias OnEquip) | the actor | the item (base form) | just after whatever held the same slots is unequipped [documented: NVSE Event Handling]; shares one canonical handler list (`onactorequip`) with `OnEquip`, runs with `GetSelfAlt == 0` and `GetEquipped == 1` (`in=1`, contra wiki's "before the item counts as equipped") [observed: 2026-11-24, Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, JIP LN 57.30, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `tools/capture/nvse/catalogs/wave6d1_on_actor_equip_unequip_alias.json` probe `w6d1_08_on_actor_equip_unequip_aliases`: `alias(g=1/1,rem=1/1)\|eq(n=1,in=1,same=0,tick=1,s0=1,sP=0,nm=onactorequip)`; no DLL, PDB, or third-party source consulted] |
| OnActorUnequip (alias OnUnequip) | the actor | the item (base form) | shares one canonical handler list (`onactorunequip`) with `OnUnequip`; fires PRE-unequip (`GetEquipped == 1` inside callback) with `GetSelfAlt == 0` [observed: `w6d1_08_on_actor_equip_unequip_aliases`: `alias(g=1/1,rem=1/1)` and `un(n=1,in=1,s0=1,sP=0,nm=onactorunequip)`]; not when equipped ammo is dropped |
| OnAdd | the item (world reference) | the container | only when picking up from the world (silent on scripted `AddItem`) [observed: 2026-11-24, Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, ShowOffNVSE 184, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `tools/capture/nvse/catalogs/wave6d1_showoff_xnvse_events_observed.json` probe `w6d1_09_on_drop_and_xnvse_on_add`: `ai(add=0,so=1,soS0=1)\|pick(add=1,a1W=1,a2P=1,so=2,soSW=1,soS0=0)`; no DLL, PDB, or third-party source consulted] |
| OnClose, OnOpen | the reference | the same reference | |
| OnCombatEnd | target (actor) | ender (actor) | |
| OnDeath | killed (actor) | killer (actor) | |
| OnDrop | dropper | the dropped world reference | fires with `GetSelfAlt == 0`, `(dropper, worldRef)` [observed: 2026-11-24, Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, ShowOffNVSE 184, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `tools/capture/nvse/catalogs/wave6d1_showoff_xnvse_events_observed.json` probe `w6d1_09_on_drop_and_xnvse_on_add`: `drp(n=1,a1P=1,a2Ref=1,sP=0,s0=1)`; no DLL, PDB, or third-party source consulted]; not when moving items between containers in the inventory menu, unlike the OnDrop block |
| OnHit | target (any reference) | attacker: an actor, or the projectile when the target is not an actor and the weapon is not melee/unarmed | runs **before damage**, unlike the OnHit block |
| OnHitWith | target | weapon (base form) | runs before damage |
| OnLoad | the reference | the same | |
| OnMagicEffectHit | target | base effect | |
| OnMurder | murdered (actor) | murderer (actor) | |
| OnPackageStart, OnPackageChange, OnPackageDone | actor | package | |
| OnReset | the reset reference | the same | |
| OnSell | the sold item (base form) | seller (player or vendor) | never fired before xNVSE 6.2.8 (a bug) |
| OnStartCombat | target | starter | not fired again when a save is loaded mid-combat |
| OnTrigger, OnTriggerEnter, OnTriggerLeave | trigger | action reference | |
| SayToDone | speaker | info | |
| OnApplyIMOD, OnRemoveIMOD | — | the imagespace modifier | |
| OnLockBroken, OnLockpickSuccess, OnLockpickBroken | — | the object | |
| OnTerminalHacked, OnTerminalHackFailed | — | the terminal | |
| OnRepair | — | the repaired inventory item; then the repairer and the repair kind (0 normal, 1 vendor) | four arguments |
| OnDisable, OnEnable | the reference | `0` | both fire PRE-update (`GetDisabled == 0` inside `OnDisable`, `GetDisabled == 1` inside `OnEnable`) with `(ref, 0)`; `OnEnable` runs with `GetSelfAlt == 0` (`s0=1,sR=0`) and `OnDisable` runs with `GetSelfAlt != ref` (`sOk=0`) [observed: 2026-11-24, Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, JIP LN 57.30, ShowOffNVSE 184, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `tools/capture/nvse/catalogs/wave6d1_on_enable_disable_pre_update.json` probes `w6d1_07_on_enable_on_disable_args` (`dis(n1=1,n2=1,sOk=0,a1Ok=1,a2z=1,inDis=0)`) and `w6d1p2_07_on_enable_tick` (`en(n1=1,n2=1,s0=1,sR=0,a1R=1,a2z=1,inDis=1)`); no DLL, PDB, or third-party source consulted]; `OnDisable` `GetSelfAlt == 0` [hypothesis: by parity with `OnEnable`] |

"—" means the table gives no first argument; how many arguments those handlers take:
open.

### 2.2 Session events (the plugin API's messages)

| Event | Arguments | Notes |
|---|---|---|
| LoadGame | the save file name | not for entering the game from the main menu with CenterOnCell; **runs an extra time** for the missing-plugins warning; runs once the save is mostly loaded (the player's cell is already the new one) |
| PostLoadGame | success flag | not for a new game, not for CenterOnCell from the main menu |
| SaveGame | the save file name | |
| NewGame | — | not for CenterOnCell from the main menu |
| ExitGame, ExitToMainMenu, QQQ | — | |
| RuntimeScriptError | the error message, an int | |
| DeleteGame, RenameGame, RenameNewGame | full file name, its length | |
| DeleteGameName, RenameGameName, RenameNewGameName | relative file name, its length | |

The SetEventHandler page's LoadGame example takes the name and its length as two
parameters [documented: SetEventHandler], while the table lists only the name; the
argument count: open.

### 2.3 User-defined events

Any name may be dispatched with DispatchEvent and listened for with SetEventHandler
(§5) [documented: DispatchEvent; SetEventHandlerAlt]. Names should carry the mod's name
to avoid clashes. Their arguments have no declared types, so SetEventHandlerAlt cannot
type-check filters on them [documented: User-Defined Event].

## 3. SetEventHandler

- **Syntax**: `SetEventHandler "Event" handler ["first"::x] ["second"::y]
  ["priority"::n]`; the pairs in any order [documented: SetEventHandler].
- **Result**: bool, success.
- **Filters**: `"first"::value` filters the first argument (usually what the event
  happens to), `"second"::value` the second (usually its cause). A form list as the value
  matches any of its forms, sublists included; the extender does not guard against two
  lists containing each other. **An array as a filter does not work: the filter is
  ignored.** The player is given as `playerref`. Whether an argument is a reference or a
  base form matters (the filter must be the same kind) [documented: SetEventHandler].
  Filtering in the registration is cheaper than inside the handler.
- **Priority** (xNVSE 6.2.9): default 1; higher runs first; equal priorities run in
  registration order; any number but **0**, which is reserved as "no priority" and
  refused here; negative numbers allowed, from **-9999 (min) to 9999 (max)**; not
  available for plugin (LN) events or the older `SetOn…EventHandler` functions
  [documented: SetEventHandler; Event Handler Priority System].
- **Lambdas** work as handlers, block or one-line [documented: SetEventHandler; Lambda].
- **Session only**: handlers are not saved; they last for the session, so scripts
  register them again when a session starts (GetGameRestarted) [documented:
  SetEventHandler].
- Registering the same handler twice with identical event, filters, and priority is
  deduplicated and returns `0` on the duplicate call for both `SetEventHandler` and
  `SetEventHandlerAlt`, firing once per dispatch [observed: 2026-11-24, Fallout: New
  Vegas 1.4.0.525, xNVSE 6.4.9, JIP LN 57.30, `GSDocMitchellHouse` (`0x00103DF9`),
  10 official base masters,
  `tools/capture/nvse/catalogs/wave6d1_set_event_handler_dedup.json` probe
  `w6d1_03_set_event_handler_dups_and_bounds`:
  `PROBE|w6d1_03_set_event_handler_dups_and_bounds|std=1,0,n=1|alt=1,0,n=1|prio=0,1,1,0,0|arr=1,n=1`;
  no binary, DLL, PDB, or third-party source consulted].

## 4. SetEventHandlerAlt (xNVSE 6.2.8)

- **Syntax**: `SetEventHandlerAlt "Event" handler [i::filter …] ["priority"::n]`
  [documented: SetEventHandlerAlt].
- **Filters**: up to 16, each `index::value`: index 0 filters the calling reference
  (what GetSelf returns), 1–15 the event's arguments in order. A value may be a number,
  form, string or array; an array means "any of these" (`1::(Ar_List Player, SunnyREF)`).
- **Cleared on every load**, unlike SetEventHandler's session lifetime: "all" handlers
  set with this function are flushed when a game is loaded [documented:
  SetEventHandlerAlt].
- **Not for plugin (LN) events.** Duplicate registrations with identical event, handler,
  filters, and priority return `0` and fire once (contra the wiki note) [observed:
  `w6d1_03_set_event_handler_dups_and_bounds`, `alt=1,0,n=1`]. Priority as §3.
- **How filters match** (xNVSE 6.2.8) [documented: New xNVSE Event Filtering System]:
  - a number must equal the dispatched number; for a user-defined event a numeric filter
    is a float; a plugin's event may declare an int filter, which floors the value;
  - a form matches the same form (two null forms match); a dispatched reference also
    matches a filter that is its base form; a form list matches any of its forms; a base
    form filter refuses a reference with an error;
  - a string matches without regard to case;
  - an array matches an array of the same kind and size with equal elements (nested
    arrays not supported);
  - an **array of filters** (list, map or string map; keys ignored) matches when any
    element matches. It holds a reference to the array, so changing the array later
    changes the filter.
- **Why flushed on load**: array filters point at savebaked arrays, which a load could
  replace [documented: New xNVSE Event Filtering System].

## 5. RemoveEventHandler and DispatchEvent

**RemoveEventHandler** `RemoveEventHandler "Event" handler [filter1] [filter2]
["priority"::n]`: removes matching handlers; returns 1 when at least one was removed.
With no filters, every handler of that function for that event goes; with one, those
matching it. The default priority is 0, meaning any priority [documented:
RemoveEventHandler]; a non-zero `"priority"::n` removes only registrations at that
priority while leaving other priorities intact, and `"priority"::0` removes all
priorities [observed: 2026-11-24, Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, JIP LN
57.30, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters,
`tools/capture/nvse/catalogs/wave6d1_remove_event_handler_priority.json` probe
`w6d1_02_remove_event_handler_priority`:
`PROBE|w6d1_02_remove_event_handler_priority|r5=1,0|keys=1,0,1|n=2|r0=1,sz0=0|rf=1,n100=0,n200=1|end=1,0`;
no binary, DLL, PDB, or third-party source consulted]. It also takes
SetEventHandlerAlt's numeric indexes [documented: SetEventHandlerAlt].

**DispatchEvent** `DispatchEvent "Name" [infoMap] [senderName]`: sends a user-defined
event to every handler registered for that name. A handler receives one string map: by
default `"eventName"` (the name) and `"eventSender"` (the dispatching mod's file name);
`infoMap` replaces the whole map, and `senderName` replaces the sender string
[documented: DispatchEvent]. Returns `1`, runs handlers synchronously inside the
dispatching line before the dispatching script continues (so in-place mutations to
`infoMap` are visible immediately after the call), and unconditionally adds/overwrites
`"eventName"` and `"eventSender"` alongside custom keys even when the caller-supplied
`infoMap` already defines those keys [observed: Fallout: New Vegas 1.4.0.525 + xNVSE
6.4.9 + JIP LN 57.30, 2026-11-23/24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official
base masters, read-only `PrintToConsole` probes `core_09_dispatch_event_custom_map_sync`
(`tools/capture/nvse/catalogs/wave4a_extenders.json`, #3679, dispatching `"W4CustomEvent"`
with custom stringmap `{"k": 42}` and sender `"W4Sender"` and reading the handler's side
effects immediately afterward: `PROBE|core_09_dispatch_event_custom_map_sync|1,1,1,42`)
and `w6d1_04_dispatch_event_map_and_sync`
(`tools/capture/nvse/catalogs/wave6d1_dispatch_event_return_overwrite.json`:
`PROBE|w6d1_04_dispatch_event_map_and_sync|def(r=1,n=1,en=W6D1_DEv,es=)|c1(r=1,mut=1/1,en=W6D1_DEv)|c2(r=1,en=W6D1_DEv,es=NewSnd)`);
no binary, DLL, PDB, or third-party source consulted].

**DispatchEventAlt** `[ref.]DispatchEventAlt "Name" [arg1 … arg15]` (xNVSE 6.2.8):
calls every handler registered for that user-defined event whose filters match the
arguments, like Call for many functions at once. Up to 15 arguments of any type (number,
form, string, array), and an optional calling reference the handlers run on. Returns a
success flag. A handler may take **fewer** parameters than the event sends, so an event
can add arguments later without breaking older handlers. SetEventHandlerAlt suits it
best, since it can filter each argument by index [documented: DispatchEventAlt, pasted by the owner from geckwiki.com, 2026-10-25].
Returns `1`, runs handlers declaring fewer or more parameters than sent, matches `0::baseForm` against a placed calling reference, matches string filters case-insensitively, and reflects live `ar_Append` mutations to an array filter [observed: 2026-11-24, Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, JIP LN 57.30, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `tools/capture/nvse/catalogs/wave6d1_showoff_xnvse_events_observed.json` probe `w6d1_05_dispatch_event_alt_arity_and_filters`: `PROBE|w6d1_05_dispatch_event_alt_arity_and_filters|ret=1/1|arity(0=1,3=1,selfDoc=1)|flt=1,1,2`; no binary, DLL, PDB, or third-party source consulted]; extra declared parameters default to 0 [hypothesis].

**GetCurrentEventName** (returns a string): inside a SetEventHandler-style handler, the
name of the event being handled, so one function can serve several events
[documented: GetCurrentEventName, pasted by the owner from geckwiki.com, 2026-10-25].
Returns `""` outside any handler and inside JIP `SetOn*EventHandler` callbacks, and the
registered event name inside `SetEventHandler`, `SetEventHandlerAlt`, and ShowOff
handlers [observed: 2026-11-24, Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, JIP LN
57.30, ShowOffNVSE 184, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters,
`tools/capture/nvse/catalogs/wave6d1_showoff_xnvse_events_observed.json` probe
`w6d1_06_get_current_event_name_contexts`:
`PROBE|w6d1_06_get_current_event_name_contexts|out=[]|std=[w6d1_evstd]|alt=[w6d1_evalt]|add=[ShowOff:OnAdd]|jip=[]`;
no binary, DLL, PDB, or third-party source consulted].

## 6. Inspecting handlers (xNVSE 6.2.9)

- **GetEventHandlers** `[eventName, handler, priority]`: with no event name, a string map
  from event names to handler lists; with one, the list for that event. A list sorted by
  priority is a map from priority to handler arrays; with a priority filter, that level
  is skipped. Each handler entry is an array: [0] the script (or, for a plugin handler,
  a string map with "Plugin" and "Handler", possibly empty), [1] a string map of its
  filters [documented: GetEventHandlers]. With no handlers registered, `"ShowOff:OnAdd"`
  returns an empty Map (`ar_Size == 0`), whereas `"OnCrosshairOn"` returns `ar_Null`
  (`ar_Size == -1`); filter StringMap keys are `"first"` / `"second"` for
  `SetEventHandler` and `"0"` / `"1"` (stringified argument indices) for
  `SetEventHandlerAlt` [observed: 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`),
  Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE 184 + JIP LN 57.30 with the
  10 official base masters, isolated clean-room staging copy (read-only `PrintToConsole`
  probe; zero writes to game install or Saves), committed probe
  `w6t2_16_xnvse_get_event_handlers_filter_keys`
  (`tools/capture/nvse/catalogs/wave6_t2_get_event_handlers.json`):
  `PROBE|w6t2_16_xnvse_get_event_handlers_filter_keys|empty(cross=-1,soAdd=0)|std(first,second)|alt(0,1)`;
  zero DLL, PDB, or third-party source consulted; hypothesis: the other 10 implemented
  `ShowOff:*` events in `nvse_handler_lists::KNOWN` behave like `ShowOff:OnAdd`].
- **DumpEventHandlers** (same filters): prints them by event and priority, plugin
  handlers included [documented: DumpEventHandlers].
- **GetHigherPriorityEventHandlers / GetLowerPriorityEventHandlers** `event, handler,
  startPriority[, scriptsToIgnore, pluginsToIgnore, pluginHandlersToIgnore]`: a map of
  the handlers at higher-or-equal (lower-or-equal) priority than that handler; Ar_Null
  when the handler is not at that priority [documented: each page].
- **IsEventHandlerFirst / IsEventHandlerLast** (same arguments): 1 or 0; 0 when the
  handler is not at that priority. Higher priority runs first; among priorities `10, 5a,
  5b, 1` and absent `99`, `10` returns `1/0`, middle tie `5a` returns `0/0`, `1` returns
  `0/1`, absent `99` returns `0/0`, and `GetHigherPriorityEventHandlers` /
  `GetLowerPriorityEventHandlers` at `5a` both include the equal-priority key `5`
  (`k5=hi(1,1),lo(1,1)`) [observed: 2026-11-24, Fallout: New Vegas 1.4.0.525, xNVSE
  6.4.9, JIP LN 57.30, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters,
  `tools/capture/nvse/catalogs/wave6d1_event_handler_first_last.json` probe
  `w6d1p2_01_priority_first_last_higher_lower`:
  `PROBE|w6d1p2_01_priority_first_last_higher_lower|ord=S:10,5a,5b,1,|fl10=1/0|fl5a=0/0|fl1=0/1|fl99=0/0|hi=0,2,2,-1|lo=2,2,0|k5=hi(1,1),lo(1,1)`;
  no binary, DLL, PDB, or third-party source consulted]. Returning `0` when handlers are
  tied at the top (`IsEventHandlerFirst`) or bottom (`IsEventHandlerLast`) — i.e., no
  other handler at higher-or-equal / lower-or-equal priority — follows the inclusive
  equal-priority maps [hypothesis].

## 7. Order and persistence, collected

- **Before the block**: OnActivate, OnHit and OnHitWith handlers run before the matching
  Begin block or before damage [documented: NVSE Event Handling]. For the other events,
  handler-vs-block order: open.
- **Activation is not blocked** by a handler [documented: NVSE Event Handling].
- **Saves**: SetEventHandler registrations last for the session and are never saved;
  SetEventHandlerAlt ones are dropped on every load [documented: SetEventHandler;
  SetEventHandlerAlt]. Scripts re-register in a GetGameRestarted or GetGameLoaded test
  [documented: SetEventHandler; DispatchEvent examples]. NVForever must copy both
  lifetimes (ext-save.md).
- **Too early**: when an event fires just before the change it reports can be seen, the
  page suggests waiting a few frames with a main-loop callback [documented: NVSE Event
  Handling].

## 8. Open (to observe), collected

Argument counts for the events with no first argument and for LoadGame; handler-vs-block
order for most events; duplicate SetEventHandler registrations; filter matching under
SetEventHandlerAlt; DispatchEvent's map when replaced, and whether handlers run inline;
IsEventHandlerFirst/Last's real meaning.

## Implementation tables (E1–E3)

One row per function E1–E3 cover, then one row per built-in event, from §2–§7 and their pages ([documented] unless marked open). **Filters**: SetEventHandler's `"first"` and `"second"` filter the First and Second columns; SetEventHandlerAlt's `1::`, `2::` … filter the arguments in the same order, and `0::` the calling reference (§3–§4). An event whose First is "—" passes no first argument the table names; its handler's parameter count is open.

### Functions

| Function | Aliases | Arguments (? = optional) | Returns | Behaviour | Edge cases |
|---|---|---|---|---|---|
| SetEventHandler | — | event name string; handler udf/lambda; `"first"::x`?; `"second"::y`?; `"priority"::n`? (pairs in any order) | bool | registers the handler for the event | filters: forms or form lists (any member); an array filter is ignored; priority default 1, range -9999..9999, 0 refused, higher runs first, ties by registration order (6.2.9); duplicate registrations rejected (return 0) [observed: `w6d1_03_set_event_handler_dups_and_bounds`]; lasts for the session, never saved |
| SetEventHandlerAlt | — | event name; handler; up to 16 `index::filter`?; `"priority"::n`? | bool | registers with indexed filters (6.2.8): 0 the calling reference, 1-15 the arguments | filter matching per §4; array value = any of; duplicate registrations rejected (return 0) [observed: `w6d1_03_set_event_handler_dups_and_bounds`]; flushed on every load; not for plugin (LN) events |
| RemoveEventHandler | — | event name; handler; filter1?; filter2?; `"priority"::n`? (default 0 = any) | bool | removes matching handlers; 1 if any was removed | no filters: all of that handler for that event; takes Alt's numeric indexes |
| DispatchEvent | — | event name; infoMap stringmap?; senderName string? | bool (1) | calls handlers of a user-defined event synchronously with one string map | default map: eventName, eventSender (the mod file); infoMap replaces the map, and eventName/eventSender are overwritten even if already present [observed: `w6d1_04_dispatch_event_map_and_sync`] |
| DispatchEventAlt | — | [ref.] event name; up to 15 values any | bool | calls handlers whose filters match the arguments (6.2.8) | optional calling reference passed on; handlers may take fewer parameters than sent |
| GetCurrentEventName | — | — | string | the event being handled | inside a SetEventHandler-style handler; outside: open |
| IsEventHandlerFirst | — | event name; handler; startPriority int; scriptsToIgnore?; pluginsToIgnore array?; pluginHandlersToIgnore array? | bool | 1 for unique highest priority, 0 for middle/lowest/absent [observed: `w6d1p2_01_priority_first_last_higher_lower`: `10->1/0`, `5a->0/0`, `1->0/1`, `99->0/0`, `k5=hi(1,1),lo(1,1)`] | 0 when not at startPriority [observed]; 0 when tied at the top [hypothesis] (6.2.9) |
| IsEventHandlerLast | — | as IsEventHandlerFirst | bool | 1 for unique lowest priority, 0 for highest/middle/absent [observed: `w6d1p2_01_priority_first_last_higher_lower`: `10->1/0`, `5a->0/0`, `1->0/1`, `99->0/0`, `k5=hi(1,1),lo(1,1)`] | 0 when not at startPriority [observed]; 0 when tied at the bottom [hypothesis] (6.2.9) |
| GetEventHandlers | — | eventName string?; handler?; priority int? | array | handlers matching the filters: a stringmap by event, then a map by priority, then handler arrays [script or plugin map, filters map] | only extender-managed handlers (6.2.9) |
| Get(Higher\\|Lower)PriorityEventHandlers | — | as IsEventHandlerFirst | array (map by priority) | handlers at higher-or-equal (lower-or-equal) priority | Ar_Null when the handler is not at startPriority (6.2.9) |
| DumpEventHandlers | — | as GetEventHandlers | none | prints them by event and priority, plugin handlers included | (6.2.9) |

### Events

| Event | Fires when | First (filter `"first"`, `1::`) | Second (filter `"second"`, `2::`) |
|---|---|---|---|
| OnActivate | a reference is activated; **before** its OnActivate block, even when activation is refused; does not block it | activated ref | activating ref |
| OnActorEquip (OnEquip) | an actor equips an item, just after whatever held the same slots is unequipped [documented: NVSE Event Handling]; shares canonical list with `OnEquip`, runs with `GetSelfAlt == 0` and `GetEquipped == 1` (`in=1`) inside callback [observed: `w6d1_08_on_actor_equip_unequip_aliases`] | actor | item (base form) |
| OnActorUnequip (OnUnequip) | an actor unequips an item; shares canonical list with `OnUnequip`, fires PRE-unequip (`GetEquipped == 1` inside callback) with `GetSelfAlt == 0` [observed: `w6d1_08_on_actor_equip_unequip_aliases`]; not for dropped equipped ammo | actor | item (base form) |
| OnAdd | an item is picked up from the world | item (world ref) | container |
| OnClose | a reference closes | ref | same ref |
| OnCombatEnd | an actor leaves combat | target (actor) | ender (actor) |
| OnDeath | an actor dies | killed | killer |
| OnDrop | a reference drops an item into the world; not container-to-container moves | dropper | dropped world ref |
| OnHit | a reference is hit, **before damage** | target | attacker, or the projectile when the target is not an actor and the weapon is not melee/unarmed |
| OnHitWith | a reference is hit, before damage | target | weapon (base form) |
| OnLoad | a reference's 3D loads | ref | same ref |
| OnMagicEffectHit | a magic effect hits | target | base effect |
| OnMurder | an actor is murdered | murdered | murderer |
| OnOpen | a reference opens | ref | same ref |
| OnPackageStart / OnPackageChange / OnPackageDone | an AI package starts / changes / ends | actor | package |
| OnReset | a reference is reset | ref | same ref |
| OnSell | an item is sold (never fired before xNVSE 6.2.8) | item (base form) | seller (player or vendor) |
| OnStartCombat | an actor enters combat; not again on loading a save mid-combat | target | starter |
| OnTrigger / OnTriggerEnter / OnTriggerLeave | something is in / enters / leaves a trigger | trigger | action ref |
| SayToDone | a SayTo line ends | speaker | info |
| OnApplyIMOD / OnRemoveIMOD | an imagespace modifier is applied / removed | — | imagespace modifier |
| OnLockBroken / OnLockpickSuccess / OnLockpickBroken | a lock is broken / picked / a pick breaks | — | object ref |
| OnTerminalHacked / OnTerminalHackFailed | a terminal hack succeeds / fails | — | terminal ref |
| OnRepair | an item is repaired | — | repaired inventory ref; 3rd the repairer, 4th 0 normal / 1 vendor |
| OnDisable / OnEnable | a reference is disabled / enabled | — | ref |
| LoadGame | a save is loaded (mostly loaded already; again for the missing-plugins warning; not CenterOnCell from the main menu) | save file name | — (its example also takes the name length) |
| PostLoadGame | after a load (not a new game, not CenterOnCell from the main menu) | success flag | — |
| SaveGame | the game is saved | save file name | — |
| NewGame | a new game starts (not CenterOnCell from the main menu) | — | — |
| ExitGame / ExitToMainMenu / QQQ | the game quits / returns to the main menu / is quit by console | — | — |
| RuntimeScriptError | a script raises an extender error | error message | an int |
| DeleteGame / RenameGame / RenameNewGame | a save is deleted / renamed (old full name / new full name) | file name | its length |
| DeleteGameName / RenameGameName / RenameNewGameName | the same, relative names | relative file name | its length |


## Activation-range crosshair acquisition

[documented] [OnCrosshairOn](https://geckwiki.com/index.php/OnCrosshairOn)
notifies a registered UDF when a new reference is under the crosshair within
activation range. Its argument is that reference. The event supports a form or
form-list filter and a colon-qualified form-type event name. This is separate
from the JohnnyGuitar-specific crosshair registration API.

[observed: original installed mod data] ySI Contextual Prompts registers a
CompileScript callback for this event. Its callback chooses an image from the
current reference and updates HUD root traits that an injected image reads.
The original restart guard requires yUI 140 as one dependency. That requirement
alone does not establish that every yUI feature is implemented or executed.

[design] The existing activation ray and its activation distance remain the
source of GetCrosshairRef, and the event is delivered through the existing
registration/filter machinery.

[hypothesis] A new nonempty reference produces one plain event and its
corresponding type-qualified event. Repeating the same reference does not produce
another acquisition; leaving to no target updates the getter without an On event;
acquiring a reference after that gap produces a new event. The callback sees the
new reference through GetCrosshairRef. None of this per-transition behaviour has
been observed in the original game; the page only says a new reference raises it.

[design] The existing JohnnyGuitar callback remains before getter publication. The
newly added event runs before the normal Scripts set. Plain then type-qualified
delivery is a mapped engine ordering; native relative ordering and timing are not
measured [hypothesis].

[design] No extra ray, per-frame target cache, replacement HUD, provider version
change or original mod/configuration edit is introduced. With no relevant
handlers, the new source-event path performs no type lookup or argument-vector
allocation. Session registration, removal and load behavior remain those of the
existing SetEventHandler implementation. Full original initialization, provider
coverage, selected XML/layout and native timing remain separate acceptance work.
GetCrosshairWater's existing unsupported result is not repaired by this event.

[validation plan] One standalone core test owns the process-wide CompileScript
file reader. Independently authored source and XML exercise real compilation,
registration, injection, current-reference readback, resolved image filename and
alpha, one acquisition per transition, form/list/type filters, existing Johnny
order and removal. One engine library control uses the existing production
activation ray against real collision, the registered source callback, injected
image and actual draw pieces; it retains a nonempty title, repeated target,
parent hiding, collision eviction and subsequent acquisition. The engine test's
callback is an in-memory compiled lambda; the file-backed core path covers
CompileScript without racing the engine suite's global file readers. All cases
were unexecuted when first authored. Hosted `35677cf5` and `22b8138c`
engine controls each passed once; their file-backed core controls failed at
separate earlier boundaries. In a later qualified runtime-only check of
`d27e4e0b`, file compilation, original callable admission, plain-event callback,
current getter, Johnny ordering and exact decimal source-value readback passed.
The first failure was the CONT type-qualified counter, not the plain callback.
The associated compile controller remained unqualified after descendant cleanup;
this does not invalidate or promote its separately admitted runtime result.

[design] Type-qualified delivery uses the existing canonical source-VM form-type
mapping, rather than the inventory-item subset. The source regression queries
GetType for its actual CONT and DOOR targets, then checks distinct type listeners
and a nonmatching armor listener. Existing base-reference lookup and callback
ordering are retained; cloned or dynamically substituted base-type parity is not
newly established. The new canonical-mapping successor and its two identical-
fixture baselines require their own compile, strict-lint and execution results.
The original absent-event baseline is distinct from the d27 typed-delivery
baseline. No original-mod, native, GPU or popup-resolution acceptance is claimed.

[design] Event filters retain their value kinds: numeric literals are numbers, not
forms with the same integer value. The crosshair fixture supplies actual reference
and form-list values, then reads the original registered filters through live
GetEventHandlers arrays to verify both kind and exact FormID. Raw numeric literals
remain nonmatching controls; no general filter coercion is added.

[validation] Hosted 6a04897b passed file compilation, original callable, plain
callback, getter/order/literal and typed CONT controls, then failed the form-filter
counter. Its filter source supplied numeric literals. Later list/image/lifecycle
controls were unreached. The engine control passed once (full engine 3422 passed,
zero failed, three ignored; strict engine Clippy passed). The typed-filter fixture
correction and its identical-fixture baseline are unexecuted at authoring. This
changes fixture inputs and diagnostics only, preserving production policy.

### Crosshair target loss (`OnCrosshairOff`)

[documented: GECK wiki, [OnCrosshairOff](https://geckwiki.com/index.php/OnCrosshairOff)]
`OnCrosshairOff` is the counterpart to `OnCrosshairOn`: it notifies a registered UDF
when a reference within activation range leaves the player's crosshair, passing that
reference as its single argument. Like `OnCrosshairOn`, it supports form/form-list
filters and colon-qualified form-type event names (`OnCrosshairOff:<formType>`).

[hypothesis] In `Game::set_crosshair_ref`, transitioning from `Some(prev)` to `None`
dispatches plain `OnCrosshairOff` then `OnCrosshairOff:<formType>` for `prev` (with
`GetCrosshairRef` already updated to `0`). Transitioning from `Some(prev)` to
`Some(next)` with `prev != next` dispatches `OnCrosshairOff` (and its typed variant)
for `prev` before `OnCrosshairOn` (and its typed variant) for `next`. Repeating the
same target or staying on `None` emits nothing. Plain-before-typed ordering and
`GetCrosshairRef` visibility inside `OnCrosshairOff` have not been measured in the
original game.

[design] When no `OnCrosshairOff` or `OnCrosshairOff:*` handler is registered,
`Game::set_crosshair_ref` performs no type lookup or argument allocation, leaving
unmodded gameplay unchanged. No third-party mod DLL or extender source code was
consulted.

## Named key and gamepad button events (`OnKeyDown`, `OnKeyUp`, `OnButtonDown`, `OnButtonUp`)

[documented: GECK wiki, [OnKeyDown](https://geckwiki.com/index.php?title=OnKeyDown),
[OnKeyUp](https://geckwiki.com/index.php?title=OnKeyUp)] `OnKeyDown` and `OnKeyUp`
notify a `SetEventHandler`-registered UDF when a keyboard key or mouse button is
pressed or released, passing the DirectX scancode (256 + n for mouse buttons) as one
numeric argument. Registration accepts either the plain event name or a colon-qualified
scancode (`"OnKeyDown:38"`, `"OnKeyUp:203"`).

[documented: GECK wiki, [OnButtonDown](https://geckwiki.com/index.php?title=OnButtonDown),
[OnButtonUp](https://geckwiki.com/index.php?title=OnButtonUp)] `OnButtonDown` and
`OnButtonUp` notify a `SetEventHandler`-registered UDF when a gamepad button is pressed
or released, passing the pressed or released XInput single-button code (`1` D-pad up,
`2` D-pad down, `4` D-pad left, `8` D-pad right, `16` Start, `32` Back, `64`/`128`
thumbsticks, `256`/`512` shoulders, `1024` guide, `4096` A, `8192` B, `16384` X,
`32768` Y) as one numeric argument. A colon-qualified suffix (`"OnButtonDown:4"`,
`"OnButtonDown:12288"`) accepts either a single button code or a combined decimal
bitmask so one registration listens for any button bit in that mask.

[design] `Game::jip_keys` (invoked from both `Game::tick` and `Game::page_tick`)
advances held-key and held-button snapshots once per frame even without subscribers,
matching `control_edges` ([control-input-events.md](control-input-events.md)). Key
edges read `ScriptInput::held` and dispatch JIP's `keydown`/`keyup` alongside plain
`"OnKeyDown"`/`"OnKeyUp"` and colon-qualified `"OnKeyDown:<code>"`/`"OnKeyUp:<code>"`.
Button edges read `Pad::buttons | Pad::held_buttons` (matching `GetPressedButtons` /
`IsButtonPressed`) and dispatch plain `"OnButtonDown"`/`"OnButtonUp"` followed by all
colon-qualified `"OnButtonDown:<mask>"`/`"OnButtonUp:<mask>"` handlers whose decimal
bitmask overlaps the changed single-button bit, ordered by priority descending and
registration sequence ascending, with `GetCurrentEventName` returning each handler's
registered event string.

[hypothesis] Relative ordering between JIP `SetOnKeyDownEventHandler` /
`SetOnKeyUpEventHandler`, plain `OnKeyDown`/`OnKeyUp`, colon-qualified
`OnKeyDown:<code>`/`OnKeyUp:<code>`, control events, and `OnButtonDown`/`OnButtonUp`
within the same frame is a mapped NVForever ordering not yet measured in the original
game. Whether `DisableKey` suppresses named `OnKeyDown`/`OnKeyUp` or `DisableButton`
suppresses `OnButtonDown`/`OnButtonUp`, whether `HoldButton` triggers `OnButtonDown`,
and how native handles multi-button bitmask priority ties or plain `"OnButtonDown"`
without a colon suffix have not been observed in original-game captures; NVForever
delivers disabled-key and disabled-button transitions consistently with
`SetOnKeyDownEventHandler` and `OnControlDown`.

## Player grab events (`OnPlayerGrab`, `OnPlayerRelease`)

[documented: GECK wiki, OnPlayerGrab, OnPlayerRelease] `OnPlayerGrab` and
`OnPlayerRelease` notify a `SetEventHandler`-registered UDF when the player picks up
(Control Code 27 / hold Activate) or releases (drops/throws) a grabbed world reference,
passing the grabbed or released reference as one reference argument. Registration accepts
the plain event name, `"first"::` form/base/form-list filters, `SetEventHandlerAlt`
indexed filters (`0::`/`1::`), and colon-qualified form-type names
(`"OnPlayerGrab:<type>"`, `"OnPlayerRelease:<type>"`).

[design] `Game::set_player_grab` (`crates/fnv-game/src/functions/senses.rs`, called by
`engine/nvf-play/src/grab.rs` on every grab start and release path: manual release, throw,
caught/out-of-range drop, menu/VATS/furniture/state exit, simulation absence or body
removal, and physics teardown) short-circuits on unchanged state so grab/release events
are symmetric with zero double-fire. With no matching `OnPlayerGrab` or `OnPlayerRelease`
handlers registered, the dispatch helper performs no form-type lookup or allocation and
leaves unmodded vanilla behavior unchanged.

[hypothesis] `Game::set_player_grab` updates `senses.grabbed` before dispatching (so
`IsPlayerGrabbedRef` and `GetPlayerGrabbedRef` report the new grab state inside callbacks),
runs the reference's `OnRelease` / `OnGrab` blocks before the named extender handlers,
sets `this = grabbedRef` alongside `arg0 = grabbedRef`, and delivers plain then
colon-qualified `:formType` events using the canonical `form_type_code` mapping. Relative
block-vs-handler ordering, exact calling reference, and whether `GetPlayerGrabbedRef`
still reports the released ref inside `OnPlayerRelease` have not been measured in
original-game captures.

## Image space modifier events (`OnApplyIMOD`, `OnRemoveIMOD`)

[documented: GECK wiki, [NVSE Event Handling](https://geckwiki.com/index.php?title=NVSE_Event_Handling)]
`OnApplyIMOD` and `OnRemoveIMOD` are built-in xNVSE events (already listed in
`nvse_handler_lists::KNOWN`) triggered when an image space modifier (`IMAD`) is applied
to or removed from the screen.

[observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9, 2026-11-23, GSDocMitchellHouse
(`0x00103DF9`), 10 official base masters, clean-room script probes
`w1_3656_objectives_unholster_imod` (`SetEventHandler`/`SetEventHandlerAlt` for
`OnApplyIMOD` and `OnRemoveIMOD`; `imod GetHit` `0x00000162` 0.4 s + ~1.5 s wait; `imod`
twice + `rimod` twice on `WhiteNonAnimated` `0x000C9AFA`) and
`b3_core_ex_06_on_apply_imod_event` (`ApplyImageSpaceModifier`/`RemoveImageSpaceModifier`
on `0x00144C41`) in #3679; no DLL or source consulted] Both `OnApplyIMOD` and
`OnRemoveIMOD` work via `SetEventHandler` and `SetEventHandlerAlt` with `on = None`
(`00000000`) and a single `IMAD` form argument (`vec![XVal::Ref(modifier.0)]`, matched by
`"first"::` / `1::`), and both dispatch **PRE-update**:
- `Game::apply_image_modifier` dispatches `OnApplyIMOD` before updating
  `Game::image_modifiers`, so `IsImageSpaceActive` reads `0` on the first apply and `1`
  when re-applying an already-active `IMAD`. Re-applying an active `IMAD` stacks another
  instance for event counting rather than discarding the previous instance.
- `Game::remove_image_modifier` removes all active instances of the `IMAD`, dispatching
  `OnRemoveIMOD` PRE-removal once per removed stacked instance (so `IsImageSpaceActive`
  reads `1` inside each callback, and removing two stacked instances fires `OnRemoveIMOD`
  twice before leaving `IsImageSpaceActive == 0`). Calling `RemoveImageSpaceModifier` on
  an inactive `IMAD` does not fire.
- `Game::update_image_modifiers` dispatches `OnRemoveIMOD` PRE-removal when an animatable
  `IMAD`'s duration expires (confirmed on `GetHit` 0.4 s with `IsImageSpaceActive == 1`
  inside the callback).
- Applying an invalid non-`IMAD` form fails without firing `OnApplyIMOD`; restoring a
  snapshot assigns `Game::image_modifiers` directly without firing `OnApplyIMOD` [design].
- [design] To keep unmodded visuals unchanged (e.g. `VCG00` re-applying
  `Addiction01ISFX` at stages 27 and 30) and prevent unbounded list growth from repeated
  re-applies, `nvforever::imagespace::screen_effect` draws at most one active instance per
  `IMAD` form (the most recently applied entry), while `Game::image_modifiers` keeps each
  stacked instance (animatable and non-animatable) in application order capped at
  `Game::MAX_IMOD_STACK = 64` per form (evicting the oldest matching entry at capacity).

[design] Both dispatch sites check `Game::handled("onapplyimod")` /
`Game::handled("onremoveimod")` before allocating an argument vector, leaving unmodded
vanilla gameplay unchanged. No extender DLL, PDB, or C++ source code was consulted.
