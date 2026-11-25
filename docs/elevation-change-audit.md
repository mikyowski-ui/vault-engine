# Owner brief: carried objects keep their elevation through an elevation change

Basis: `vault-engine` at revision `4d5183a` (the tree the acceptance binary
`vault-platform` was built from). Modules: `vault_core::world`
(`WorldState::move_object`, `WorldState::set_current_elevation`, in its
spatial module), `vault_core::host_skills`
(`process_transition`, `transition_to`, `place_player`), `vault_core::host_save`
(the slot load, which seats the player at the saved elevation) and
`vault_platform::autoplay_runner` (`Auditor::audit`, `two_loads`,
`diff_ledgers`: the live world against load A, load A against load B).
Specifications: SPEC-008 §3.22 (transitions by map number, step 3) and the
exit-grid record of the map format (map, hex, elevation, orientation).

Tags: `[Must-match]` must be exactly as stated, and leaves every existing
result unchanged; `[May-improve]` an implementation freedom that changes no
stated result; `[Safe rule]` what the engine does with a value or state it
cannot honour.

## 1. Purpose

Maps of several elevations join their elevations with exit grids whose
destination is the map's own number and another elevation (a staircase, a
ramp, a ladder well). Such a grid saves the map and loads it again, then
places the player on the destination hex and elevation. After that, the
autoplay audit run at the transition reports a mismatch between the live
world and the world loaded from its own save: every object the player
carries has elevation 0 in the live world and the player's new elevation
after the load. Nothing in the map data decides this; the two paths
disagree about the elevation of carried objects.

## 2. What happens today

1. `transition_to` loads the map and calls `place_player(hex, elevation,
   facing)`, which calls `WorldState::move_object(player, hex, elevation,
   false)`.
2. `move_object` writes `raw.hex` and `raw.elevation` of the moved object
   only. The objects it holds (`holder == player`, `raw.hex == None`) keep
   the elevation they had when they were picked up.
3. The audit saves the live world and loads it twice. Load A seats every
   carried object on its holder's elevation. `diff_ledgers(live, A)` then
   finds, for each carried object, `elevation: 0` became `elevation: 1`,
   and the run fails with `AuditMismatch` (`report.txt`):

   ```
   failure: AuditMismatch at tick 51 (Checks)
     object 1335: ObjectRow { pid: 619, hex: None, elevation: 0, ... holder: Some(1), contents: {} }
       became ObjectRow { pid: 619, hex: None, elevation: 1, ... holder: Some(1), contents: {} }
   audited at map transition: 2
   ```

## 3. Reproduction (synthetic, generic Fallout 2 data)

A mod layer with one map `twofloor.map`, registered in `data/maps.txt`:

- elevation 0: floor tiles over a square of 20 by 20 hexes around hex
  20100; one ground item (a misc item prototype) on hex 20102; an exit grid
  on hex 20110 with destination map = the map's own number, hex 20300,
  elevation 1, orientation 2;
- elevation 1: floor tiles over the same square; an exit grid on hex 20310
  back to the map's own number, hex 20108, elevation 0;
- entrance hex 20100 on elevation 0.

Steps (a replay or the autoplay bot): pick up the item on hex 20102, walk
onto hex 20110. The engine saves and reloads the map and places the player
on hex 20300, elevation 1. Run `--autoplay-headless` on the map: the audit
at that transition reports the carried item's elevation 0 against 1
(observed on a three-elevation town map: the bot picked up two items
and failed at the first elevation change, tick 51 of a plain run and tick
18 of a `--chaos-doors-c4` run, with the same two objects).

## 4. Requested behaviour

- `[Must-match]` After any move of an object to another elevation
  (`move_object`, and so `place_player`, `move_to`, transitions), every
  object it holds, recursively, has the holder's elevation; the live world
  then equals what a save and load of it produce. No other field changes.
- `[May-improve]` Doing the same at pick-up time (a picked-up object takes
  the picker's elevation), or treating a carried object's elevation as
  undefined and excluding it from the state hash and the audit ledger, is
  equally acceptable, provided the live world and load A agree.
- `[Safe rule]` A holder chain that loops is left as it is and reported
  once, not followed.

## 5. Synthetic test vectors

1. The map of §3; pick up the item, cross the grid: the item's elevation is
   1 in the live world, and `Auditor::audit` returns `Clean`.
2. The same, carrying a container that holds an item: both have elevation
   1.
3. Cross back: both have elevation 0; the audit is clean.
4. Nothing carried: the state hash after crossing is unchanged by the fix.

## 6. Out of scope

Exit grids to other maps (a different map number), whose load seats the
player and the inventory together already; elevators and scripted
`move_to` between elevations behave the same way and are covered by §4.

## 7. As built (2026-11-24)

- The fix follows §4's first rule and the pick-up option together: a move
  seats everything the moved object holds on its elevation, and an object
  entering an inventory takes the elevation of its outermost holder. The
  map-entrance placement seats the player's inventory too.
- SPEC-003 §2.10 already states the rule for a load ("child elevations are
  set to the enclosing elevation"), so no specification changes: the live
  world now agrees with it.
- Corrections to the text above: the audit type is `ParityAuditor`; the
  transition rule is SPEC-013 §3.22, not SPEC-008; the load seats held
  objects in the map and save readers of `vault-formats`, on the elevation
  of the root of the tree; and §6 is wrong that other-map exit grids were
  unaffected, since a map change keeps the player's inventory. They are
  covered by the same fix.
- The audit's `held object's elevation replaced by a load` allowance, which
  hid this mismatch, is removed.
