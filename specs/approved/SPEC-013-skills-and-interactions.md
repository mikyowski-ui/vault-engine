# SPEC-013: Active Skills, Loot and Barter Windows, Poison and Radiation Ticks, and Elevation Transitions

## Clean-Room Rules for Research Agents
- Document ONLY binary file layouts (byte offsets, endianness, data types), mathematical formulas, state transitions, and observable behavior.
- DO NOT copy C/C++ code, struct definitions, function names, internal variable names, or implementation-source paths from the inspected implementation.
- Provide small synthetic hex/text test vectors at the bottom of the specification.

## 1. Overview & Scope

This specification describes reference revision R1, targeting the 32-bit Windows engine. Evidence is static inspection of the skill module, the action scheduler, the object-use and lock paths, the inventory/loot/barter windows, the timed queue, the critter state module, the elevator module and the Skilldex window. Every message index was checked against the English `.msg` files of the adjacent local corpus, and every art size against the corpus FRM headers. No original executable was run.

This document covers:

- active skill execution: entry points, gating, the script dispatch point, and the default behavior of Sneak, Lockpick, Steal, Traps, First Aid, Doctor, Science and Repair, including the kit route, the daily usage timers, experience and random-draw order;
- explosive arming, timers and detonation, which are the engine's only built-in Traps mechanics, and the explosion art and sounds of every explosion;
- skill books;
- the Skilldex and Set Timer windows;
- the container/corpse loot window and the barter window as interaction state machines;
- the timed poison and radiation events, including their behavior for non-player critters;
- elevator, ladder and stairs transitions between maps and elevations.

Relationship to other specifications: SPEC-005 owns opcode stack contracts (`roll_vs_skill`, `obj_lock`, `poison`, `radiation_inc`, `metarule`). SPEC-006 owns skill percentages (§3.3), the random generator and check results (§3.4), the timed queue clock (§3.7) and the generic object-action dispatch and script procedures (§3.8). SPEC-007 owns combat, hit modes and attack damage. SPEC-008 owns the save-file payloads, including the queue payload grammar (§2.6) and the usage-slot block. SPEC-011 owns item stacking, weight, value, the barter price formula and acceptance (§3.13-§3.19) and perk definitions (§3.22-§3.23). This document adds the behavior those specifications reference but do not define.

**Compatibility tags.** Every documented quirk carries one tag:

- `[Must-match]`: gameplay-visible behavior, data grammar, formula, ordering, message selection, or random-draw sequence. A clean-room engine must reproduce it exactly.
- `[May-improve]`: reference memory-safety defects, undefined behavior, uninitialized reads, stale state, presentation-only transients, a lookup that reads the wrong table or object in an obviously unintended way, or another clear reference bug. A clean-room engine may apply the stated safe rule and should keep every other observable behavior. An engine that wants bit-exact parity may offer a compatibility switch that keeps the reference behavior.

**Notation.** These terms are descriptive, not reference identifiers.

| Term | Meaning |
| --- | --- |
| D(a, b) | One draw from the shared generator over the inclusive range a through b (SPEC-006 §3.4) |
| Percentage check(C, K) | Draw D(1, 100); margin = C minus the draw; the result is failure for a negative margin, otherwise success. When the game clock is at least 864000 ticks (one day), a second D(1, 100) can upgrade it as in SPEC-006 §3.4: failure becomes critical failure when the draw is at most `trunc(-margin / 10)`; success becomes critical success when it is at most `trunc(margin / 10) + K` |
| Skill check(A, s, m) | Percentage check(effective skill s of A plus m, effective Critical Chance of A) |
| Result codes | 0 critical failure, 1 failure, 2 success, 3 critical success |
| Player | The player character object |
| Tick | One tenth of a game second; one game hour is 36000 ticks |
| Override | A script that sets its override flag during the procedure, suppressing the default (SPEC-006 §3.8) |

Skill indices are those of SPEC-006 §3.3: 6 First Aid, 7 Doctor, 8 Sneak, 9 Lockpick, 10 Steal, 11 Traps, 12 Science, 13 Repair.

Evidence areas in reference revision R1 (source file paths and line ranges are withheld under the clean-room quarantine policy):

| Subject | Reference area inspected |
| --- | --- |
| Skill gating, approach, animation | Skill-on action request and proximity callback |
| Default skill routine, experience, usage slots, steal check | Skill module |
| Sneak flag, recheck event, perception hook | Critter state module, AI perception test, animation movement requests |
| Jam check, locks, explosives, kits, skill books | Object-use module |
| Explosion art and sounds | Action scheduler: explosion and ranged-attack sequences; animation sequencer |
| Timed events, map-exit cleanup | Timed queue module |
| Skilldex window | Skilldex module and interface-bar button |
| Loot, steal, barter and timer windows | Inventory module and dialogue barter module |
| Poison and radiation | Critter state module and timed queue |
| Elevators, ladders, stairs | Elevator module, action scheduler and map-transition request |

### Revision history

- **R1-rev2 (2026-10-19).** Section 3.10 was rewritten. Explosion damage is center-only, matching SPEC-007 §3.7. Scenery and other blockers are affected only through radius-3 `damage_p_proc` notification, replacing the earlier "center-hex scenery" wording. The section now gives the detonation order, the retry rule for timed explosives, and the servicing and no-retry rule for scripted explosions.
- **R1-rev3 (2026-10-19).** Builder placeholders filled. Section 3.10 gains the explosion art and sound table for timed, scripted, grenade and rocket explosions. New section 3.25 Skill Books gives the PIDs, the increase formula, time cost, fade and messages, with Fixture G. `proto.msg` 800-806 were added to section 2.4.
- **R1-rev4 (2026-10-19).**
  - Section 3.21 now states that its table is the complete elevator set: there is no Vault 15 elevator and no other elevator art.
  - Section 3.22 now covers leaving a map, which saves it and then reloads the destination's working copy (SPEC-014 §3.35), and the town-map and world-map destinations (maps -1 and -2), including the save-failure dialog.
- **R1-rev5 (2026-10-20).**
  - Section 3.16 step 3 now states the same container reach timing as SPEC-007 §3.11.3. The previous text put the missing-art fault on the wrong branch and gave a different improvement rule. The current rule opens the container on the reach's action frame when the art loads.
  - Sections 3.2 and 3.23 now point to the shared adjacency check and approach-goal rule in SPEC-007.
- **R1-rev6 (2026-10-21).** Section 3.16: the row-formula offset affects rows 1-5 only; SPEC-014 §3.34 now uses the same `[May-improve]` tag.
- **R1-rev7 (2026-10-22).** Section 3.22: exit grids at the entrance hex are checked as usual, and a request made while a map loads replaces the pending one (new `[May-improve]` for transition processing). A town-map hotspot in another town opens the world map travelling there; arrival does not enter the town (SPEC-014 §3.39).
- **R1-rev8 (2026-10-23).** Section 3.10: a not-yet-loaded script gets a startup request (number 1), which is its first request under SPEC-006 §3.5; no `damage_p_proc` body runs at that point.

## 2. Binary Layout / Data Format (Byte Offset Tables)

### 2.1 Numeric Conventions

All quantities in this document are signed 32-bit integers unless stated otherwise. Divisions truncate toward zero. Where a value is persisted, SPEC-008 states its byte layout; save-file integers are **big-endian 32-bit words**. No file format is introduced by this document except the data tables below, which are built-in constants rather than on-disk records. `[Must-match]`

### 2.2 Per-Skill Constants

This table is exact built-in data. `[Must-match]`

| Index | Skill | Base experience per success | Difficulty-bonus flag | Default target rule (section 3.2) |
| ---: | --- | ---: | :---: | --- |
| 6 | First Aid | 25 | no | Critter prototype only |
| 7 | Doctor | 50 | no | Critter prototype only |
| 8 | Sneak | 0 | no | No target; toggles immediately |
| 9 | Lockpick | 25 | yes | Item or scenery prototype |
| 10 | Steal | 25 | yes | Item or critter prototype, or scenery PID `0x02000384`; never the actor |
| 11 | Traps | 25 | yes | Any prototype except critter |
| 12 | Science | 0 | no | Any non-critter; critter only with kill type 10 (robot) or 5 (brahmin) |
| 13 | Repair | 0 | no | Any non-critter; critter only with kill type 10 (robot) |
| 17 | Outdoorsman | 100 | no | Not a targeted skill |

All other skills have base experience 0 and no difficulty-bonus flag.

### 2.3 Persistent State Used Here

| State | Representation | Persistence |
| --- | --- | --- |
| Sneak flag | Bit 0 (value `0x00000001`) of the player's critter prototype flag word | Saved with the player prototype data (SPEC-006/SPEC-008) |
| Sneak working | Boolean stored as one 32-bit word | SPEC-008 player critter component |
| Usage slots | 18 skills by 3 slots of 32-bit game-tick timestamps, skill-major order, 0 meaning empty | SPEC-008, 54 big-endian words |
| Lock state | Instance flag word bit `0x02000000` locked, `0x04000000` jammed; the container item flag word or the door scenery state word | Map/save object data (SPEC-003) |
| Explosive timer | Timed-queue event type 8 or 11 owned by the explosive object, no payload | SPEC-008 §2.6 |
| Sneak recheck | Timed-queue event type 10 owned by the player, no payload | SPEC-008 §2.6 |
| Radiation-exposed flag | Bit 1 (value `0x00000002`) of the player's critter prototype flag word; the same bit means "barters" on a merchant's prototype | Saved with the player prototype data |
| Poison tick | Timed-queue event type 5 owned by the player, no payload | SPEC-008 §2.6 |
| Radiation stage | Timed-queue event type 6 owned by the player; payload level (1-6) and healing indicator (0 onset, 1 recovery) | SPEC-008 §2.6 |

The table is one layout rule. `[Must-match]`

### 2.4 Message Resources

All files are under `text/<language>/game/`. Numbers are decimal message indices; texts are the English corpus strings. Two spaces after a period are preserved where the corpus has them. The tables are exact data. `[Must-match]`

`skill.msg`:

| Index | Text | Used by |
| ---: | --- | --- |
| 500 | You heal %d hit points. | First Aid, Doctor |
| 501 | You look healthy already. | First Aid, Doctor, target is the player |
| 502 | %s looks healthy already. | First Aid, Doctor, other target |
| 503 | You fail to do any healing. | First Aid, Doctor |
| 505 | You earn %d XP for honing your skills. | Experience routine |
| 510 | (debug text) invalid skill used | Diagnostic only, never displayed |
| 512-514 | You can't heal the dead. / Let the dead rest in peace. / It's dead, get over it. | Dead target |
| 520 / 521 | You heal your %s. / You heal the %s. | Doctor limb success |
| 525 / 526 | You fail to heal your %s. / You fail to heal the %s. | Doctor limb failure |
| 530-534 | damaged eye / crippled left arm / crippled right arm / crippled right leg / crippled left leg | Doctor limb names |
| 550 | You fail to pick the lock. | Never displayed by the engine |
| 551 | You fail to find any traps. | Traps default |
| 552 | You fail to learn anything. | Science default |
| 553 | You cannot repair that. | Repair default |
| 570 / 571 | You're caught stealing the %s. / You steal the %s. | Steal, taking |
| 572 / 573 | You're caught planting the %s. / You plant the %s. | Steal, planting |
| 590-592 | You've taxed your ability with that skill.  Wait a while. / You're too tired. / The strain might kill you. | Usage limit reached |

`skilldex.msg`: 100 `SKILLDEX`, 101 `CANCEL`, 102-109 `SNEAK`, `LOCKPICK`, `STEAL`, `TRAPS`, `FIRST AID`, `DOCTOR`, `SCIENCE`, `REPAIR`.

`proto.msg`: 487 "It is locked.", 582 "That does nothing.", 589 "You set the timer.", 800 "You read the book.", 801 "You learn nothing new.", 802 "You learn new science information.", 803 "You learn a lot about repairing broken electronics.", 804 "You learn new ways to heal injury.", 805 "You learn how to handle your guns better.", 806 "You learn a lot about wilderness survival.", 590 "The timer is already ticking!", 900 "The supplies in the Doctor's Bag run out.", 901 "The supplies in the First Aid Kit run out.", 902 "You cannot do that in combat."

`misc.msg`: 2000 "You cannot get there.", 2001 "The lock is jammed.", 4000 "Due to your inept handling, the explosive detonates prematurely."

`inventry.msg`: 21 "MOVE ITEMS", 22 "ALL", 23 "SET TIMER", 25 "You cannot pick that up. You are at your maximum weight capacity.", 26 "There is no space left for that item.", 27 and 28 barter results (SPEC-011 §3.19), 29 "You gain %d experience points for successfully using your Steal skill." The corpus file holds indices 0 through 29 only.

`proto.msg` (interaction entries): 480 "You see: %s.", 485 "You search the %s.", 486 "You close the %s.", 700 "You don't have enough action points.", 903 "This person will not barter with you."

`misc.msg` (timed events): 1000-1005 radiation stage texts (section 2.6), 1006 "You have died from radiation sickness.", 1007 "You have received a large dose of radiation.", 1008 "The geiger counter is clicking.", 1009 "The geiger counter is clicking wildly.", 3000 "You have been poisoned!", 3001 "You take damage from poison."

### 2.5 Timed-Event Types Used Here

| Type | Meaning | Owner | Map-exit handling |
| ---: | --- | --- | --- |
| 4 | Midnight | None | Removed without a handler |
| 5 | Poison tick | Player | Kept |
| 6 | Radiation stage onset or recovery | Player | Kept |
| 8 | Explosion | Explosive item | Detonate immediately, unanimated, then remove |
| 10 | Sneak recheck | Player | Turn sneak off, then remove |
| 11 | Premature explosion | Explosive item | Detonate immediately, unanimated, then remove |

Events with equal due times run in insertion order. A handler that returns "interrupt" ends the current queue pass; later due events wait for the next pass. An interrupt also stops resting and world-map travel (SPEC-008). The table is exact behavior. `[Must-match]`

### 2.6 Radiation Tables

Base level from total accumulated radiation, and the Endurance check modifier for each base level. The table is exact data. `[Must-match]`

| Total radiation | Base level | Endurance modifier |
| --- | ---: | ---: |
| 0-99 | 0 | +2 |
| 100-199 | 1 | 0 |
| 200-399 | 2 | -2 |
| 400-599 | 3 | -4 |
| 600-999 | 4 | -6 |
| 1000 or more | 5 | -8 |

Per-level penalties, applied in this column order. Message is the `misc.msg` stage text. The table is exact data. `[Must-match]`

| Level | Message | ST | PE | EN | CH | IN | AG | Current HP | Healing rate |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 1000 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 2 | 1001 | -1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 3 | 1002 | -1 | 0 | 0 | 0 | 0 | -1 | 0 | -3 |
| 4 | 1003 | -2 | 0 | -1 | 0 | 0 | -2 | -5 | -5 |
| 5 | 1004 | -4 | -3 | -3 | -3 | -1 | -5 | -15 | -10 |
| 6 | 1005 | -6 | -5 | -5 | -5 | -3 | -6 | -20 | -10 |

### 2.7 Elevator Table

Map numbers are positions in `text/<language>/game/map.msg` from 0: 6 Vault 13, 9 Necropolis vault, 13-15 Brotherhood levels, 31-32 military base, 33-34 the Master's lair, 42-43 the Glow. Each destination is map / elevation / hex. Backgrounds and panels are under `art/intrface/`. The table is exact data. `[Must-match]`

| Type | Background | Panel | Buttons | Key characters | Destinations, button order |
| ---: | --- | --- | ---: | --- | --- |
| 0 | `el_bos.frm` | none | 4 | `1 2 3 4` | 14/0/18940, 14/1/18936, 15/0/21340, 15/1/21340 |
| 1 | `el_bos.frm` | `el_bos2.frm` | 2 | `G 1` | 13/0/20502, 14/0/14912 |
| 2 | `el_mast1.frm` | none | 3 | `1 2 3` | 33/0/12498, 33/1/20094, 34/0/17312 |
| 3 | `el_mast1.frm` | `el_mast2.frm` | 2 | `3 4` | 34/0/16140, 34/1/16140 |
| 4 | `el_mil1.frm` | none | 3 | `1 2 3` | 31/0/14920, 31/1/14920, 32/0/12944 |
| 5 | `el_mil1.frm` | `el_mil2.frm` | 2 | `3 4` | 32/0/24520, 32/1/24520 |
| 6 | `el_mil1.frm` | none | 3 | `1 2 3` | 42/0/22526, 42/1/22526, 42/2/22526 |
| 7 | `el_mil1.frm` | `el_mil3.frm` | 3 | `3 4 6` | 42/2/14086, 43/0/14086, 43/2/14086 |
| 8 | `el_vault.frm` | none | 3 | `1 2 3` | 6/0/14104, 6/1/22504, 6/2/17312 |
| 9 | `el_mil1.frm` | none | 3 | `1 2 3` | 9/0/13704, 9/1/23302, 9/2/17308 |
| 10 | `el_mil1.frm` | none | 3 | `1 2 3` | Same as type 9 |
| 11 | `el_mil1.frm` | `el_mil4.frm` | 3 | `4 5 6` | 43/0/14130, 43/1/14130, 43/2/14130 |

Background sizes are 230 by 284, except `el_mil1.frm` at 231 by 285. Panels are 107 by 248, except `el_mast2.frm` at 107 by 247. Buttons use `ebut_out.frm` (up) and `ebut_in.frm` (down), 48 by 50. The level gauge `gaj000.frm` is 92 by 715: 13 stacked cells of 92 by 55.


## 3. Behavioral Rules & Edge Cases

### 3.1 Entry Points and Targeting Modes

A targeted skill reaches the skill-on request (section 3.2) from four places. `[Must-match]`

1. **Skilldex** (section 3.15). Sneak toggles immediately. Any other row puts the mouse into the matching skill targeting mode with the use-crosshair cursor.
2. **Hotkeys**, only while the interface is enabled. Each first plays `ib1p1xx1`. `1` toggles Sneak; `2` through `8` select Lockpick, Steal, Traps, First Aid, Doctor, Science and Repair targeting, respectively. The shifted characters `!`, `@`, `#`, `$`, `%`, `^`, `&`, `*` are equivalent.
3. **Targeting click.** With no object under the cursor, the mode returns to movement. Otherwise the player's skill-on request is made on that object. An accepted request returns the mode to movement. A rejected request leaves the targeting mode and cursor active, with no message other than the combat refusal, so the player can click again.
4. **Action menu "skill" entry** (SPEC-014), offered for container items, critters including the player, and scenery, never for other items or walls. It opens the Skilldex; the chosen skill is requested on the clicked object, and Sneak toggles without regard to that object.

Hotkey `1` also switches the cursor art to the use crosshair although no targeting mode is entered. `[May-improve]`: leave the cursor unchanged for the Sneak hotkey.

The Skilldex and hotkey Sneak paths first clear the player's current animation, then toggle. The skill-on request path for Sneak toggles without clearing. `[Must-match]`

### 3.2 Skill-On Request: Gating, Approach and Animation

Gating is evaluated first, in this order per skill. A rejection returns "rejected" and schedules nothing. The combat refusal displays `proto.msg` 902 only when the actor is the player. `[Must-match]`

| Skill | Rejected when |
| --- | --- |
| First Aid, Doctor | In combat (902); target prototype is not a critter |
| Sneak | Never: toggle sneak (section 3.7) and return accepted, with no movement, animation or script |
| Lockpick | In combat (902); target prototype is neither item nor scenery |
| Steal | In combat (902); target prototype is neither item nor critter and the target is not scenery PID `0x02000384`; target is the actor |
| Traps | In combat (902); target prototype is a critter |
| Science | In combat (902); target is a critter whose kill type is neither 10 nor 5 |
| Repair | In combat (902); target is a critter whose kill type is not 10 |

Accepted requests are scheduled as one animation sequence: `[Must-match]`

1. If the actor is the player and is walking or running, its current animation is cleared.
2. Out of combat, if the target is not the player, the actor runs to the target when object distance is at least 5, otherwise walks. The player as target causes no movement. The approach uses the ordinary move-to-object rules of SPEC-007 §3.1.
3. A proximity step requires object distance at most 1. When it fails, the sequence aborts and `misc.msg` 2000 is displayed only when the **target** is the player. `[May-improve]`: display it when the actor is the player. This is the shared adjacency check of SPEC-007 §3.11.3. The approach goal rule, under which the target's own hex is never tested for a blocker, is in SPEC-007 §3.1.
4. The actor plays the manipulation animation: the ground variant when the target's art is a critter and the target is prone (SPEC-007 §3.3), otherwise the middle variant.
5. After that animation completes, the skill-on stage of section 3.3 runs. The animation's action frame is looked up but not used; the stage is a sequential step after the full animation.

The request itself charges no action points. No skill other than Sneak passes gating in combat. `[Must-match]`

### 3.3 Skill-On Stage: Jam Check and Script Dispatch

In order: `[Must-match]`

1. **Jam check.** If the target is lockable (an item container or door scenery) and carries the jammed bit, display `misc.msg` 2001 when the actor is the player and stop. No script runs. This blocks every skill on a jammed container or door, not only Lockpick.
2. If the target's prototype cannot be resolved, stop.
3. If the target has a script, request `use_skill_on_p_proc` with source = actor, target = the target object and action detail = the zero-based skill index (SPEC-006 §3.8).
4. Unless that procedure overrode, run the default routine of section 3.4 for (actor, target, skill) with kit bonus 0. An override suppresses all of the default: no time cost, no experience, no message and no deferred steal.

### 3.4 Default Skill Routine

| Skill | Default behavior |
| --- | --- |
| First Aid | Section 3.11 |
| Doctor | Section 3.12 |
| Sneak, Lockpick | Nothing: no message, roll, time or experience; lock and jam bits unchanged |
| Steal | Post a deferred steal request (actor, target), processed by the next out-of-combat deferred-request pass (section 3.9) |
| Traps | Display `skill.msg` 551 and return failure |
| Science | Display `skill.msg` 552 and return failure |
| Repair | Display `skill.msg` 553 and return failure |
| Any other index | Diagnostic 510 only, return failure |

The table is exact behavior. `[Must-match]` Message 550 is never displayed by the engine. There is no lock-difficulty field, no engine lockpick roll and no engine trap detection in R1. Locks, traps, computers and repairs work only through object scripts that use `roll_vs_skill`, `obj_is_locked`, `obj_lock`, `obj_unlock` and related opcodes. `[Must-match]`

### 3.5 Skill-Use Experience

The experience routine receives (actor, skill, count, kit bonus). `[Must-match]`

1. Do nothing unless the actor is the player and count is greater than 0.
2. Let B be the skill's base experience (section 2.2). Do nothing if B is 0.
3. If the skill's difficulty-bonus flag is set and the kit bonus argument is negative, add its absolute value to B.
4. Add `count * B` through the ordinary experience addition, which applies Swift Learner and the level cap (SPEC-011 §3.20).
5. If the addition succeeds, display `skill.msg` 505 with the **actual** change in accumulated experience, after Swift Learner and any cap. At the cap the message can show 0.

In R1 only First Aid and Doctor reach this routine. Lockpick, Steal, Traps and Outdoorsman never do, so their base values and the difficulty-bonus flag are unreachable data. Steal experience comes from the loot window instead (section 3.9.5). There is no first-use bonus. `[Must-match]`

### 3.6 Daily Usage Limit (First Aid and Doctor)

Each of the two skills has three timestamp slots (section 2.3). The slots are global: they ignore which critter is the actor and which is the target, so a non-player actor using a kit through a script consumes the same slots. `[Must-match]`

**Availability** is computed at the start of each use: `[Must-match]`

1. If any slot is 0, the skill is available, and the first empty slot is the recording slot.
2. Otherwise let `H = trunc((now - slot 0) / 36000)`, slot 0 being the oldest. If H is at most 24 the skill is unavailable. Otherwise it is available and slot 2 is the recording slot.

A fourth use within the window therefore needs `now - slot 0` of at least 900000 ticks, which is 25 game hours, not 24.

**Unavailable.** Draw D(0, 2) and display `skill.msg` 590 plus the draw, then return failure with no time cost and no experience. The message is displayed whoever the actor is. `[Must-match]`

**Recording** happens only on a successful heal, as sections 3.11 and 3.12 state. Recompute availability. If unavailable, skip recording silently; the heal still applies. If the recording slot was empty, store the current tick there. If it is slot 2 with all slots full, shift slot 1 into slot 0 and slot 2 into slot 1, then store the current tick in slot 2. `[Must-match]`

### 3.7 Sneak

**Toggle on.** Set the sneak flag. Immediately run the recheck below, which also queues the next recheck event. Refresh the indicator boxes so that `SNEAK` shows (SPEC-014). `[Must-match]`

**Recheck event.** Event type 10, owned by the player, due 600 ticks (60 game seconds) after scheduling. It recomputes "sneak working" and schedules the next type-10 event 600 ticks later; the chain continues until sneak is turned off. `[Must-match]`

In R1 the recheck sets "sneak working" to true exactly when the player's effective Sneak percentage is at least 2. It compares the percentage with the success result code rather than performing a check, so it makes **no random draw** and sneaking effectively always works. `[May-improve]`: at each recheck perform Skill check(player, Sneak, 0) and set working exactly when the result is 2 or 3. This adds one or two draws per recheck, so an engine that keeps RNG parity must keep the reference rule.

**Toggle off.** Clear the flag, remove every type-10 event owned by the player, and refresh the indicator boxes. `[Must-match]`

**Automatic off.** `[Must-match]`

- A run order for the player turns sneak off unless the player has Silent Running.
- Leaving a map runs the sneak event's map-exit handler, which turns sneak off. Sneak therefore ends on every map change.

Removing all type-10 events while the map-exit pass is iterating the queue is unsafe if more than one such event exists. `[May-improve]`: keep at most one sneak event, or make the queue walk tolerate removal of later events.

**Effects.** `[Must-match]`

- AI perception (SPEC-007 §3.2, SPEC-009 §3.3): when the observed critter is the player and sneak is on **and** working, the maximum detection distance is divided by 4, in both the forward-sector and the rear-sector branch.
- Movement: while the flag is set and the player lacks Silent Running, the player's move orders use walking instead of running.
- Silent Death (SPEC-007 §3.6) and the `using_skill` opcode for skill 8 test the flag, not "working".
- Ghost's +20 Sneak bonus (SPEC-011 §3.23) changes the percentage, which under the reference rule cannot change "working" once the percentage is already at least 2.

Sneak displays no message.

### 3.8 Lockpick and Lock State

Lockpick gating and dispatch follow sections 3.2 through 3.4; its default does nothing. Lock Picks (PID 84) and Electronic Lock Pick (PID 77) carry no engine effect. Using them on an object goes through the generic use-on path, whose default without a script override is `proto.msg` 582. `[Must-match]`

Lock bits change only as follows. `[Must-match]`

- `obj_lock` and `obj_unlock` set and clear the locked bit; `jam_lock` sets the jammed bit.
- The jammed bit is cleared on every object of the map at each midnight event, on loading a saved map whose last visit was at least 864000 ticks earlier, and whenever the object is opened or closed through the scripted open/close path.
- Using a locked container shows `proto.msg` 487 with the locked sound; a locked door plays its locked sound.

### 3.9 Steal

#### 3.9.1 Starting

After gating and dispatch, the default posts a deferred steal request. When processed: `[Must-match]`

1. If actor equals target, nothing happens.
2. **Steal mode** applies when the actor is a critter and the target is an active critter: a critter prototype whose combat-result flags contain none of knocked out `0x01`, dead `0x80` and lose turn `0x8000`.
3. Otherwise (an item, scenery `0x02000384`, a dead or unconscious critter, or a critter that has lost its turn) the request opens an ordinary loot window (section 3.16) with no steal checks and no steal experience.
4. The attempt counter N is reset to 0.

In steal mode the loot window differs as follows. `[Must-match]`

- No `pickup_p_proc` is requested on entry.
- The victim's left-hand item, right-hand item and worn armor are removed from the victim's displayed inventory for the session, so they can be neither seen nor stolen. When the window closes they are added back with their equipped state restored.
- Take All is disabled, and stacked-corpse cycling is not offered.

#### 3.9.2 Per-Item Attempts

1. A left press on an item slot of either pane in the hand cursor increments N **first**. The increment stands even when the drag ends in the same pane, outside both panes, or with the quantity dialog cancelled. `[Must-match]`
2. Dropping into the opposite pane is an attempt: victim pane to player pane is taking; player pane to victim pane is planting. `[Must-match]`
3. For a stack of more than one, the quantity dialog (section 3.16) runs before any check. Cancel makes no check and moves nothing. `[Must-match]`
4. Run the steal check (section 3.9.3) once for the moved item. Caught ends the attempt with no move. Otherwise move the chosen quantity; a failed move displays `inventry.msg` 25 when taking or 26 when planting and counts no success. `[Must-match]`
5. Each successful move, taking or planting, adds to pending steal experience (section 3.9.5). Being caught ends the window at once. `[Must-match]`

#### 3.9.3 Steal Check

Let T be the thief, V the object that currently owns the victim pane, and I the item. `[Must-match]`

1. M = 1 - N. N already includes the current attempt, so the first attempt has M = 0.
2. Unless T is the player and has Pickpocket: subtract `4 * size of I` (one unit's prototype size, independent of quantity). If V's art category is critter and T is "in front of" V, subtract 25. "In front of" holds when the absolute difference of the two stored facing directions (0 through 5) is 2, 3 or 4; positions are ignored.
3. If V's combat-result flags contain knocked out `0x01` or knocked down `0x02`, add 20. Pickpocket does not affect this.
4. Steal chance S = M + T's effective Steal, capped at 95. There is no lower cap.
5. Steal roll: when T is the player and V is a party member, the result is critical success with no draw. Otherwise Percentage check(S, T's effective Critical Chance).
6. Catch roll: after a critical-success steal roll the result is "not caught" with no draw; after a critical failure it is "caught" with no draw. Otherwise the catch chance is `V's effective Steal - M` when V's prototype is a critter, else `30 - M`, uncapped. Perform Percentage check(catch chance, 0).
7. The thief is caught exactly when the catch result is 2 or 3. An ordinary failure of the steal roll does not by itself cause a catch.
8. Display `skill.msg` 571 (taking) or 573 (planting) when not caught, 570 or 572 when caught, with the item's name.

Opening a container held in the victim pane makes that container V. Later checks then skip the facing penalty, use catch chance `30 - M`, read the prone bits from item data and fail party-member detection. `[May-improve]`: use the root victim critter as V for every check of the session.

#### 3.9.4 Being Caught

The window closes and the victim's removed equipment is restored. No steal experience is awarded, including for items moved earlier in the session. After closing, if N is positive and the victim has a script, request `pickup_p_proc` on the victim with source = player and no explicit target. The engine adds no combat, dialogue or message of its own; every consequence is scripted. `[Must-match]`

#### 3.9.5 Steal Experience

Pending experience P starts at 0 and increment B at 10. Each successful move adds B to P, then adds 10 to B, so n successes give `P = 5 * n * (n + 1)`. When the window closes in steal mode, not caught, with P positive and the victim not a party member, award `X = min(300 - T's effective Steal, P)` through the ordinary experience addition (Swift Learner applies) and display `inventry.msg` 29 with X **before** Swift Learner. A negative X is passed unchanged. `[Must-match]` `[May-improve]`: clamp X at 0 and skip the message when X is not positive.

Moves to and from a party member always succeed with no draws and never award experience. `[Must-match]`

### 3.10 Explosives and Traps

**Traps on an object.** After gating and dispatch, the default displays `skill.msg` 551; no roll, time or experience. `[Must-match]`

**Arming.** Direct use of PIDs 51 (Dynamite) or 85 (Plastic Explosives), or their armed variants 206 and 209, from the inventory "use" action or the item slot. Arming has no combat block; the item slot's ordinary use cost applies in combat. `[Must-match]`

1. If the item already carries the in-use object flag, which every queued event sets on its owner, display `proto.msg` 590 and stop.
2. Open the Set Timer dialog (section 3.15). Cancel does nothing. OK yields S seconds, a multiple of 10 from 10 through 180.
3. Display `proto.msg` 589. Change PID 51 to 206 and 85 to 209 permanently; there is no engine disarm.
4. Perform Skill check(**player**, Traps, 0) whoever the user is.
5. Schedule, owned by the explosive: result 0, event type 11 after 0 ticks; result 1, type 11 after `trunc(10 * S / 2)` ticks; result 2 or 3, type 8 after `10 * S` ticks. No experience is awarded.

**Detonation.** `[Must-match]`

- Type 11 first displays `misc.msg` 4000, then detonates with animation. Type 8 detonates with animation and no message.
- The center hex is the hex and elevation of the explosive's outermost holder, following nested containers, or the explosive's own hex when nobody holds it.
- Damage range is 30-50 for PIDs 51 and 206, 40-80 otherwise. For the critter blocking the center hex, if it is a living critter: damage `v = D(min, max) - explosion DT`; if v is positive subtract `trunc(v * explosion DR / 100)`; clamp at 0. Knockback is `trunc(v / 10)` for a non-multihex victim.
- **Damage covers the center target only.** The splash-victim enumeration is shared with SPEC-007 §3.7 and has its radius setup disabled in R1, so it returns no victims and makes no draws. A timed or scripted explosion therefore damages only the critter blocking the center hex. A critter on any other hex, including the six adjacent ones, takes nothing. The six adjacent hexes receive explosion art only. This is the same rule as SPEC-007 §3.7, which states it for every explosion. `[Must-match]`
- **Scenery and other blockers.** The engine itself never damages, destroys, opens or moves scenery, walls, doors, rocks or items in an explosion. Instead it notifies object scripts within a radius, after the center explosion art has finished playing, or immediately for an unanimated detonation. `[Must-match]`
  1. Collect, in script-list order, every item or scenery object script that has a `damage_p_proc` (procedure index 14) and whose owner is on the explosion's elevation within hex distance 3 of the center hex. Then collect every spatial script that has `damage_p_proc` and whose own hex is on that elevation within distance 3. Before a script is tested, the host checks whether it has a cached `damage_p_proc` index. If it has none and its program is not loaded yet, the host sends it a startup request, request number 1. That is the script's first request (SPEC-006 §3.5). It loads the program, caches the procedure indices, and runs the initial stream, which runs the `start` body with `script_action` returning 1. Nothing else is called unless `start` calls it, and no `damage_p_proc` body runs at this point. The script is then tested with its newly cached indices. Scripts present when the map loaded were already loaded by the step 8 startup requests of SPEC-006 §3.5, so only instances created later and not requested since are loaded here.
  2. Spatial-trigger checks are disabled while collecting. Then each collected script's `damage_p_proc` runs once, in collection order.
  3. Any visible effect, such as a door blown open or a rock removed, is whatever that procedure does. An object without such a script is unaffected. A critter's own script is not notified by this step.
- **Order.** Damage values are computed when the explosion starts: one D(min, max) draw for the center critter, if there is one. For an animated detonation:
  1. Play `whn1xxx1` and the center art, together with any victim reaction.
  2. Play the six adjacent arts.
  3. Notify the scenery scripts.
  4. Remove the art objects.
  5. Apply the damage and the attribution described below.

  An unanimated detonation kills any critter whose computed damage is lethal at once, then applies damage, then notifies the scenery scripts. `[Must-match]`
- The player is the credited attacker. A critter killed by the blast awards its kill experience to the player. A surviving victim records the player as its last attacker, and the first such victim requests combat against the player if combat is not active.
- **Retry.** Only one animated explosion can be in progress. The in-progress state starts when an animated explosion registers its animation and ends after its damage step. A timed explosive (type 8 or 11) that would detonate during another animated explosion is re-queued as type 8 after 50 ticks (5 game seconds) and not destroyed. Because type 8 is re-queued, message 4000 is not repeated. There is no retry limit: each retry is checked again in the same way. Otherwise the explosive is destroyed, even when the explosion could not be set up for lack of memory or animation slots; in that case nothing explodes. `[Must-match]`
- **Scripted explosions.** The `explosion` opcode (SPEC-005) records one pending explosion request: hex, elevation, minimum and maximum damage. A second request before the next request pass overwrites the first. Out of combat, the request pass detonates it with animation and **no credited source**: no kill experience, no last-attacker change and no combat request. In combat the request is discarded by the combat request pass (section 3.16, step 5). A scripted request that finds another animated explosion in progress is dropped, with no retry. `[Must-match]` `[May-improve]`: retry a refused scripted explosion on the next request pass instead of dropping it.

**Explosion art and sounds.** Every explosion shows one art object on the center hex and one on each of the six adjacent hexes. Each plays its art once at the art's own rate (a stored rate of 0 means 10 frames per second, SPEC-001) and is then removed. Only the center object plays a sound. Explosion art is miscellaneous art (`art/misc/`, identifier `0x05000000` plus the `misc.lst` index). The table is exact. `[Must-match]`

| Source | Art (`misc.lst` index, identifier) | Frames | Sound |
| --- | --- | ---: | --- |
| Timed explosive: Dynamite (PIDs 51, 206), Plastic Explosives (85, 209); scripted `explosion` | `roktxpd.frm` (10, `0x0500000A`) | 6 | `whn1xxx1` |
| Rocket Launcher (PID 13), and any other weapon with explosion damage (type 6) that is not thrown | `roktxpd.frm` (10, `0x0500000A`) | 6 | Weapon hit sound: `whn1xxx1` for the Rocket Launcher |
| Frag Grenade (25), Molotov Cocktail (159): thrown, explosion damage | `expa.frm` (29, `0x0500001D`) | 7 | `who1xxx1` |
| Plasma Grenade (26): thrown, plasma damage (3) | `expp.frm` (31, `0x0500001F`) | 9 | `whp1xxx1` |
| Pulse Grenade (27): thrown, EMP damage (5) | `empxpld.frm` (2, `0x05000002`) | 5 | `whq1xxx1` |

Frame counts are per direction, from the corpus FRM headers. The weapon sounds are the SPEC-010 weapon hit names. They are built from the corpus weapon sound codes `N`, `O`, `P` and `Q`, with variant 1 for the primary hit modes these weapons use, and material letter `X`, because the material lookup is skipped for plasma, EMP and explosion damage. All five sound files are in the corpus. Weapon explosions follow these rules. `[Must-match]`

- **Classification.** A weapon explodes when it is thrown with explosion, plasma or EMP damage, or used without throwing with explosion damage. The weapon prototype's damage type decides this, not the ammunition. Plasma and EMP weapons that are not thrown, such as the Plasma Rifle and Plasma Pistol, never explode.
- **Center.** The center hex is where the projectile ends: the defender's hex on a hit, or the miss impact hex (SPEC-007 §3.7.1) on a miss. A critical failure that keeps its critical bit launches no projectile, so it shows no explosion (SPEC-007 §3.9).
- **Order.**
  1. The projectile flies with the weapon's flight sound (`wfo1xxx1`, `wfp1xxx1`, `wfq1xxx1` or `wfn1xxx1`).
  2. On arrival a grenade first shows its own item art, then the projectile object turns into the center explosion art and plays the hit sound. The thrown grenade is erased afterwards and does not stay on the ground.
  3. The six adjacent arts are queued after the center art, in direction order 0-5. Direction 0's art carries a scheduler start delay of 2 (4 for the Plasma Grenade); the other five carry none. Sequence entries start in order, so none of the six starts before direction 0's art. The delay counts down only while an earlier entry of the same animation sequence is still running, and it is ignored once none is.
- **Timed and scripted explosions.** These use `roktxpd.frm` for all seven objects, with the `whn1xxx1` sound on the center object, in the detonation order given earlier in this section.

**Map exit.** The map-exit handlers for types 8 and 11 detonate immediately without animation and without message 4000, on the map being left. An armed explosive carried by the player therefore detonates at the player's hex as the player leaves. `[Must-match]`

**Dropping an armed explosive** from the inventory sets a flag. On closing the inventory, the engine enumerates explosion victims around the player and lets each enumerated non-player, non-team critter make a Perception stat check D(1, 10) to notice it. Because the enumeration returns no victims in R1, this makes no draws and has no effect. `[Must-match]`

### 3.11 First Aid (Default Routine)

Inputs: actor A, target T, kit bonus K (0 from the skill path, 20 from the kit route). H is the actor's Healer rank when A is the player, otherwise 0. CC is A's effective Critical Chance. T's current and maximum HP are sampled once at the start. `[Must-match]`

1. **Limit.** If unavailable (section 3.6), draw D(0, 2), display 590 plus the draw and return failure.
2. **Dead target** (SPEC-007 §3.3 dead query): draw D(0, 2) and write message 512 plus the draw to the diagnostic log only, not to the message display. No time is spent and no experience is awarded; return completion. `[May-improve]`: keep the draw and display the message as Doctor does.
3. **Injured** (current HP below maximum; crippled limbs are ignored): fade the palette to black. A robotic body type (2) is a forced failure with no draw. Otherwise perform Skill check(A, First Aid, CC + K): the chance is First Aid + CC + K and the critical modifier is CC.
   - **Success (2 or 3).** Draw `D(1 + 2H, 5 + 5H)` and add it to T's HP, capped at the maximum. If A is the player, display 500 with `min(heal, maximum - current)`. Record a usage slot. Refresh the HP counter if T is the player. Request a map-update script pass. Fade the palette back. The success count is 1.
   - **Failure (0 or 1).** Display 503, whoever the actor is. The palette is not faded back on this branch in R1, leaving the screen black until a later palette change. `[May-improve]`: fade back on every exit after the fade to black.
4. **Not injured.** If A is the player, display 501 (T is the player) or 502 with T's name, and award no experience.
5. **Time.** For a living target, whether injured or not, add 1800 game seconds (18000 ticks) when A is the player. The clock increment does not process due events at that moment (SPEC-006 §3.7).
6. **End.** Unless suppressed, run the experience routine with the success count, giving 25 on a successful heal. Then request a map-update script pass. A successful First Aid therefore requests two passes; the kit route adds a third.

### 3.12 Doctor (Default Routine)

Inputs as for First Aid. `[Must-match]`

1. **Limit.** As First Aid.
2. **Dead target.** Draw D(0, 2) and **display** 512 plus the draw. No time, no experience; return completion.
3. **Injured or crippled.** The condition is current HP below maximum, or any crippled bit `0x7C` set (left leg `0x04`, right leg `0x08`, left arm `0x10`, right arm `0x20`, blind `0x40`). Fade the palette to black. Let the attempt counter be 1.
4. **Limb loop**, only for a non-robotic, crippled target. Examine bits in this fixed order: blind `0x40`, left arm `0x10`, right arm `0x20`, right leg `0x08`, left leg `0x04`. For each bit present:
   1. Add 1 to the attempt counter.
   2. Perform Skill check(A, Doctor, CC + K).
   3. On success clear the bit, record a usage slot and display 520 (T is the player) or 521; on failure display 525 or 526. The part name is message 530 plus the position in this order. These messages are displayed whoever the actor is.
   4. Call the experience routine with a count of 1 if **any** limb has succeeded so far in this use, else 0. Once one limb has succeeded, every later limb attempt awards 50 experience, even a failed one. `[May-improve]`: pass 1 only when this attempt succeeded.
   5. Suppress the end-of-routine experience.
5. **HP roll**, always performed in this branch, even at full HP when only crippled. A robotic target is a forced failure with no draw. Otherwise perform Percentage check(Doctor, CC + K): the chance is the bare Doctor skill and the critical modifier is CC + K. This differs from First Aid and the limb rolls.
   - **Success.** Draw `D(4 + 2H, 10 + 5H)` and add it to HP, capped. If A is the player, display 500 with `min(heal, maximum - current)`, which shows 0 at full HP. Record a usage slot only if no limb succeeded in this use. Refresh the HP counter if T is the player. Run the experience routine with count 1 (50 experience). Request a map-update pass, fade the palette back, and suppress the end-of-routine experience.
   - **Failure.** Display 503, whoever the actor is. Request a map-update pass and fade the palette back.
6. **Neither injured nor crippled.** If A is the player, display 501 or 502 and award no experience.
7. **Time.** For a living target, add `3600 * attempt counter` game seconds when A is the player: one hour plus one hour per crippled bit present on a non-robot, even when the target was healthy.
8. **End.** Run the experience routine unless suppressed, then request a map-update pass.

Doctor repairs blindness and the four limbs only. Knockdown, knockout and lose-turn flags are untouched. `[Must-match]`

Messages 503, 512-514, 520-534 and 590-592 are displayed even when an NPC is the actor, which only scripted kit use reaches. `[May-improve]`: display them only when the actor is the player, keeping every draw.

### 3.13 Kit Route

Using a First Aid Kit (PID 47) or a Doctor's Bag (PID 91) on a target takes this route instead of the generic use-on path. `[Must-match]`

1. Neither the kit's nor the target's `use_obj_on_p_proc` is requested.
2. In combat, display `proto.msg` 902 when the actor is the player and reject.
3. Run the default First Aid routine for a First Aid Kit, or Doctor for a Doctor's Bag, with K = 20.
4. If the routine returned failure (the usage limit), stop with no further draw.
5. Otherwise draw D(1, 10). On 1, display `proto.msg` 900 (bag) or 901 (kit) when the actor is the player and destroy the kit. This draw follows every completed use, including a dead, healthy or robotic target and a failed roll.
6. Request one more map-update pass.

The inventory "use" action on a kit applies it to the user; using it from the item slot enters use-crosshair targeting. `[Must-match]`

The kit route does not check that the target is a critter, and reads critter data from a non-critter. `[May-improve]`: for a non-critter target display `proto.msg` 582, spending no time, making no draw and consuming nothing.

### 3.14 Science and Repair

After gating (section 3.2) and dispatch, with no override, Science displays `skill.msg` 552 and Repair 553. No roll, time or experience. Every functional use is scripted. `[Must-match]`

### 3.15 Skilldex and Set Timer Windows

**Skilldex window.** All art is under `art/intrface/`. The window and its layout are exact. `[Must-match]`

| Element | Value |
| --- | --- |
| Background | `skldxbox.frm`, 185 by 368 |
| Window position | Screen (451, 5): `640 - 185 - 4` and `480 - 100 - 1 - 368 - 6` |
| Skill button art | Up `skldxoff.frm`, down `skldxon.frm`, 86 by 33 |
| Cancel button art | Up `lilredup.frm`, down `lilreddn.frm`, 15 by 16 |
| Counter digits | `bignum.frm`, 336 by 24; digit d is the 14-by-24 cell at x = 14d |
| Font | 103 |
| Title | `skilldex.msg` 100 at (55, 14), color RGB555 (18, 17, 3) |
| Rows i = 0 through 7 | Sneak, Lockpick, Steal, Traps, First Aid, Doctor, Science, Repair |
| Row button | At (15, 45 + 36i), label `skilldex.msg` 102 + i |
| Row label placement | x = `max(0, trunc((86 - text width) / 2) + 1)`, y = `trunc((33 - font height) / 2) + 1`, inside the button image |
| Row label color | RGB555 (18, 17, 3) on the up image, (14, 12, 3) on the down image |
| Row counter | Hundreds, tens and ones digits at x = 110, 124, 138, y = 48 + 36i; value is the player's effective skill |
| Cancel | Label `skilldex.msg` 101 at (72, 337); button at (48, 338) |
| Sounds | Row buttons `ib3p1xx1` press, `ib3lu1x1` release; Cancel `ib1p1xx1`, `ib1lu1x1` |

While the window is open, map background processing and palette cycling are suspended and the arrow cursor is shown. Esc, Cancel and a quit request return "cancel". **Enter also cancels**, after playing `ib1p1xx1`. A row button returns its skill; the window then waits `trunc(1000 / 9)` = 111 milliseconds before closing. `[Must-match]`

A counter value of -1 is drawn as 0. Other negative values index outside the digit strip. `[May-improve]`: clamp the displayed value to 0 through 999.

The interface-bar Skilldex button is defined in SPEC-014; it sends the `s` key.

**Set Timer dialog.** `[Must-match]`

| Element | Value |
| --- | --- |
| Window | Screen (140, 80), 259 by 162, background `art/intrface/movemult.frm`, font 103 |
| Title | `inventry.msg` 23, centered, y = 9, color key 21091 |
| Timer overlay | `timer.frm`, 105 by 81, at (113, 34) |
| Item picture | Inventory art scaled into 90 by 61 at (16, 46) |
| Plus / minus | `splsoff.frm` / `splson.frm` at (194, 64), 16 by 12; `snegoff.frm` / `snegon.frm` at (194, 76), 17 by 12 |
| Done / Cancel | Little red buttons at (98, 128) and (148, 128); they act as Enter and Esc |
| Sounds | Every button, plus and minus included: `ib1p1xx1` on press, `ib1lu1x1` on release |
| Button timing | Plus and minus act on the mouse press, once per press; holding a button does not repeat. Done and Cancel act on release. All four are drawn transparently |
| Value | Starts at 60; plus adds 10 below 180, minus subtracts 10 above 10 |
| Display | `bignum.frm` digits: minutes `trunc(S / 60)` at x = 133, tens of seconds `trunc((S mod 60) / 10)` at x = 161, ones `S mod 10` at x = 175, y = 64; the cell at x = 147 is left to the background |

Digit keys and Backspace are ignored. Enter accepts a value from 10 through 180 that is a multiple of 10 and plays `ib1p1xx1`; any other value would play `iisxxxx1`, which steps of 10 cannot reach. Esc cancels. `[Must-match]`

### 3.16 Container and Corpse Loot Window

**Reaching the window.** `[Must-match]` unless tagged.

1. **Container item on the map.** A left click or the action-menu Use runs the "get object" action. The player walks to the item, or runs when out of combat at object distance 5 or more, and must end adjacent. In combat it costs 3 AP, and without them `proto.msg` 700 is shown and the sequence stops. A container whose prototype is not flagged as pick-up-able then:
   1. puts away a held weapon (animation 39, with sound);
   2. plays the reach animation: ground (animation 10) when bit `0x01` of the container prototype's open-flags word (payload word `0x04`, SPEC-003) is set, else middle (animation 11);
   3. runs the use-container step below, except item PID 213, which skips it;
   4. takes the weapon out again;
   5. posts a loot request only when the container's frame was 0 (closed) at the time of the click, or the PID is 213.

   Clicking an open container therefore only closes it and does not loot. A pick-up-able container, such as a bag on the ground, is picked up instead.
2. **Use-container step.**
   - When the locked bit is set: play the object's locked sound (the `sfx_build_open_name` naming rule, SPEC-010), display `proto.msg` 487 for the player and stop.
   - Otherwise request the item's `use_p_proc`; an override stops here.
   - Otherwise animate: from frame 0, play the open sound and the forward animation; from any other frame, play the close sound and the reverse animation.
   - Display 485 when the container was closed before the step, otherwise 486, with the container's name.
   - The jammed bit is not tested on this path. A locked container still posts its loot request, but the window refuses the closed container.
3. **Reach timing.** SPEC-007 §3.11.3 ("Container reach timing") is authoritative, and this step states the same rule.
   - The delay of the use-container step is meant to come from the reach art's action frame, looked up with weapon code 0. The art test is inverted, so when the art loads, the delay is -1.
   - With corpus art, the use-container step therefore always starts after the reach animation has finished. `[Must-match]`
   - `[May-improve]`: when the reach art loads, start the use-container step on its action frame, as the pickup does. When it does not load, start it after the reach has finished.
   - The weapon take-out and the loot request keep delay -1 under either rule.
4. **Critter targets.** A click on a critter that cannot be talked to loots it. "Can talk" requires the prototype's talk flag and an active critter (not dead, knocked out or lost-turn). The action-menu Use is offered exactly for such critters and does the same: approach, 3 AP in combat, loot request. A living critter whose prototype lacks the talk flag therefore opens an ordinary loot window, not steal mode.
5. **In combat**, loot and elevator requests are serviced by the per-input combat request pass. That pass then discards every other pending deferred request, including dialogue, explosion, endgame and steal requests.

**Opening**, for a non-steal session unless stated. `[Must-match]` unless tagged.

1. Refuse when the looter is not the player.
2. Refuse silently a container item at frame 0 whose art has more than one frame.
3. Non-steal only: request the target's `pickup_p_proc` with source = player and no target; an override opens no window.
4. Load the inventory cursors `hand.frm`, `actarrow.frm`, `actpick.frm`, `actmenu.frm` and `blank.frm`. A disabled interface is enabled for the session.
5. The reference creates an inert temporary holder object (item PID 467, which has no prototype in the corpus). At close it moves the holder's always-empty contents to the target and destroys it, consuming one object identifier per session. `[May-improve]`: omit it.
6. Steal mode hides the victim's equipment (section 3.9.1).
7. The player's equipped left-hand, right-hand (one object if two-handed) and armor items are removed from the player's list for the session and restored with their equipped state at close. The player's equipped items therefore never appear in the left pane.
8. In a non-steal session, the target's equipped items stay in its list as ordinary entries with no marker. This covers a corpse's armor and weapons.
9. **Stacked bodies**, non-steal sessions with a critter target only:
   - Collect the critters on the target's hex and elevation, keeping only dead (`0x80`) or knocked-out (`0x01`) ones. Each discarded entry is replaced by the current last kept entry.
   - With more than one body, add the cycling buttons and start at the target's position in the list. If the target is not in the list, start at the list length, which wraps on the first press.
10. Draw both panes and portraits and set the hand cursor.

**Geometry.** Coordinates are relative to the window. `[Must-match]` unless tagged.

| Element | Value |
| --- | --- |
| Window | `art/intrface/loot.frm`, 537 by 376, at screen (80, 0) |
| Player pane | 6 rows; buttons (44, 35 + 48i) 64 by 48; icons scaled into 56 by 40 at (48, 39 + 48i) |
| Target pane | 6 rows; buttons (422, 35 + 48i); icons at (426, 41 + 48i), 2 pixels lower than the player pane |
| Quantity label | Font 101, white (color key 32767), at the icon's top-left, clip width 80 |
| Player portrait | 60 by 100 box at (176, 37): the player's current figure with the held weapon's animation code, facing southwest (3), frame 0 |
| Target portrait | 60 by 100 box at (297, 37): the target's current art at its stored facing, **last frame** |
| Portraits inside a container | That container's world art |
| Player scroll | Up (128, 39), down (128, 62), 22 by 23; `invupout.frm` / `invupin.frm`, `invdnout.frm` / `invdnin.frm`; keys Up and Down |
| Target scroll | Up (379, 39), down (379, 62), same art; keys Ctrl+Up and Ctrl+Down |
| Body cycling | (436, 162) `slu.frm` / `sld.frm` sends PgUp; (456, 162) `sru.frm` / `srd.frm` sends PgDn; 20 by 18 |
| Done | Red button at (288, 328); sends Esc |

- Quantity labels show `x` and the quantity when it exceeds 1, capped at 99999. Ammunition shows `x` and its round count: pack size × (quantity - 1) plus the rounds in the front pack. While an entry is dragged it is redrawn reduced by one unit, and ammunition omits the front pack.
- Scroll buttons use the red-button sounds and move one row; up needs a positive offset, down needs offset + 6 below the entry count.
- Each portrait is centered in its box, clipped to 60 by 100, and drawn over the background.
- Portrait redraws are skipped within `trunc(1000 / 6)` = 166 milliseconds of the previous one, so fast body cycling can leave a stale portrait. `[May-improve]`: always redraw on a target change.
- **Portrait hot zones.** Invisible portrait buttons go up one container level. The right one is at (297, 37) 60 by 100. The left one is placed at (44, 35) 60 by 100 rather than over the drawn portrait, and because later buttons are tested first it shadows the player's first two item rows except their rightmost 4 pixels. `[May-improve]`: put the left zone at (176, 37) and let item slots win.
- There is no Take All button.

**Input.** `[Must-match]` unless tagged.

- **Take All** is Shift+A (code `A`) only, in non-steal sessions.
  - It succeeds when the root target's total weight, counting all entries, equipped items and nested contents, is at most the player's carry limit minus the player's current total weight (SPEC-011 §3.14).
  - Everything then moves to the root player with forced moves, and both panes redraw.
  - If the right pane was showing a sub-container, it keeps showing it after the move. `[May-improve]`: reset the target view to the root.
  - The refusal looks up `inventry.msg` 31, which is absent, so a refusal is silent.
- The right mouse button toggles the hand and arrow cursors.
- **Arrow cursor.** Hovering a filled slot shows the pick cursor and prints the item's short look line when the hovered item changes. A quick click prints the short look text; holding opens the action menu below.
- **Hand cursor.** A left press on a filled slot starts a drag.
- Esc, Done or a quit request closes the window. Ctrl+Q, Ctrl+X and F10 open the quit confirmation (SPEC-014).

**Drag.** `[Must-match]` unless tagged.

1. Lift the entry and set the item's inventory art as the cursor, with the hotspot at its center. Play `ipickup1`.
2. On release play `iputdown`. The drop zones are screen x 502-566 by y 35-323 for the target pane and screen x 124-188 by y 35-323 for the player pane. A release elsewhere does nothing. Items go into whichever container is open in the destination pane, never onto a particular bag icon.
3. For a quantity above 1, run the move-items counter (section 3.17); Cancel aborts.
4. In steal mode, run the steal check (section 3.9).
5. Move with the normal capacity rules (SPEC-011 §3.14). A failure into the target shows `inventry.msg` 26; a failure into the player shows 25.
6. Moving an item out of a critter clears its equip flags, but the reference's right-hand art reset tests the flags after they were cleared, so the source critter's art never changes. No armor-stat reversal happens either.
7. The reference also clears the three equip bits (`0x01000000`, `0x02000000`, `0x04000000`) on the open source container object itself. `[May-improve]`: leave the container's own flags unchanged.
8. No `drop_p_proc` or `pickup_p_proc` runs for transfers inside the window.

**Arrow-cursor action menu** in the loot and barter windows. Moving the mouse more than 10 pixels vertically moves the highlight one entry, and release selects the highlighted entry. `[Must-match]`

| Item | Weapon with removable ammunition | Container | Other |
| --- | --- | --- | --- |
| Not owned by the player at root | Look, Unload, Cancel | Look, Use, Cancel | Look, Cancel |
| Owned by the player | Look, Unload, Drop, Cancel | Look, Use, Drop, Cancel | Look, Drop, Cancel |

- Look prints the full examine text.
- Use opens the container in that pane, up to nine levels deep.
- Unload adds the ammunition, forced, to the item's current owner, so unloading a corpse's gun leaves the ammunition in the corpse.
- Drop drops the item at the player's position, using the counter for stacks.
- Dropping an armed explosive sets the dropped-explosive flag of section 3.10. Planting one by dragging does not.

**Nested containers.** `[Must-match]` unless tagged.

- An opened container becomes that pane's list, scroll and portrait. Transfers into the player side go into the bag open there, with the container size and owner weight checks.
- If the player opens one of the player's own bags during steal mode, that bag becomes the thief for later steal checks, read through critter data. `[May-improve]`: the thief is always the player.
- In the ordinary inventory window, the list row under the cursor is computed as `trunc((screen y - 39) / 48)` although rows start at 35, so the top 4 pixels of rows 1-5 map to the row above (row 0's top pixels truncate toward zero and stay in row 0). SPEC-014 §3.34 uses the same formula and tag for drops. `[May-improve]`: use `trunc((screen y - 35) / 48)`.

**Closing**, in this order: `[Must-match]`

1. Release the cycling art.
2. In steal mode, restore the victim's equipment.
3. Empty and destroy the temporary holder.
4. In steal mode when not caught, award steal experience (section 3.9.5).
5. Restore the player's equipment, close the window and run the dropped-explosive check.
6. Release cursors and messages.
7. In steal mode when caught with N positive, request the victim's `pickup_p_proc`.

No other script runs at close.

### 3.17 Move-Items Counter

`[Must-match]`

| Element | Value |
| --- | --- |
| Window | Screen (140, 80), 259 by 162, `art/intrface/movemult.frm`, font 103 |
| Title | `inventry.msg` 21, centered at y = 9, color key 21091 |
| Item picture | Inventory art scaled into 90 by 61 at (16, 46) |
| Value | Three `bignum.frm` digits with leading zeros at (153, 45), (167, 45), (181, 45) |
| Plus / minus | (200, 46) 16 by 12 `splsoff.frm` / `splson.frm`; (200, 58) 17 by 12 `snegoff.frm` / `snegon.frm` |
| ALL | (120, 80) 94 by 33; up image `allbon.frm`, down image `allboff.frm`; caption `inventry.msg` 22 centered in 94 pixels at row 4, color key 21091 up and 18977 down |
| Done / Cancel | Red buttons at (98, 128) and (148, 128), acting as Enter and Esc |
| Sounds | `ib1p1xx1` press, `ib1lu1x1` release on every button |

- **Range.** The value starts at 1. The minimum is 1 and the maximum is the entry quantity, capped at 999.
- **Buttons.** Plus adds 1 below the maximum. Minus subtracts 1 above the minimum, even when a typed value exceeds the maximum. ALL sets the maximum.
- **Typing.** The first digit key after any other action replaces the value. Later digits make it `(10 × value) mod 1000 + digit`. Backspace also starts fresh after another action, then makes it `trunc(value / 10)`. Typed values may be 0 or above the maximum.
- **Enter** accepts a value from the minimum through the maximum with `ib1p1xx1`; any other value plays `iisxxxx1` and the counter stays open.
- **Esc** cancels and moves nothing. The arrow cursor is shown while the counter is open.

### 3.18 Barter Window

This section adds the interaction rules that SPEC-011 §3.17-§3.19 (prices, acceptance) leave out. `[Must-match]` unless tagged.

**Entry.** The barter request in dialogue (the `b` key, the dialogue control, or `gdialog_barter`) tests bit `0x02` of the merchant's critter prototype flag word. Clear: show `proto.msg` 903 in the reply area. Set: switch to barter mode.

**Window.**

- `art/intrface/barter.frm` (640 by 191) replaces the dialogue subwindow at screen (0, 289), using the dialogue slide animation.
- Offer button (41, 163) and Talk button (584, 162), 14 by 14, up image `di_rdbt2.frm` and down image `di_rdbt1.frm`, sounds `ib2p1xx1` / `ib2lu1x1`, keys `m` and `t`.
- The item subwindow is at screen (80, 290), 480 by 180, with 3 visible rows per column. Its background is copied from barter-window x = 80, row 0, so it sits 1 pixel below the window art. `[May-improve]`: copy from row 1.
- Portraits use 60-by-100 boxes on the barter window, the player at (15, 25) and the merchant at (560, 25), drawn as in the loot window. Invisible buttons over them go up one container level.

| Column | Buttons, subwindow coordinates | Icons | Total |
| --- | --- | --- | --- |
| Player inventory | (29, 35 + 48i) 64 by 48 | (26, 39 + 48i), 59 by 40 | |
| Player offer table | (165, 20 + 48i) | (169, 24 + 48i), 56 by 40 | `$` and the plain value at (169, 168), font 101, white |
| Merchant offer table | (250, 20 + 48i) | (254, 24 + 48i) | `$` and the merchant price at (254, 168), always priced with the player as barterer |
| Merchant inventory | (395, 35 + 48i) | (397, 39 + 48i) | |

| Scroll button | Position | Size | Art | Key |
| --- | --- | --- | --- | --- |
| Player inventory up / down | (109, 56) / (109, 82) | 23 by 24 / 24 by 25 | `di_up1.frm` / `di_up2.frm`, `di_down1.frm` / `di_down2.frm` | Up / Down |
| Merchant inventory up / down | (342, 56) / (342, 82) | Same | Same | Ctrl+Up / Ctrl+Down |
| Player table up / down | (128, 113) / (128, 136) | 22 by 23 | `invupout.frm` / `invupin.frm`, `invdnout.frm` / `invdnin.frm` | PgUp / PgDn |
| Merchant table up / down | (333, 113) / (333, 136) | 22 by 23 | Same | Ctrl+PgUp / Ctrl+PgDn |

**Moves.** Drags follow the loot-window lift, cursor and sound rules, and every route is forced: no weight or size checks. Each route has one drop rectangle in screen x, all spanning screen y 310-454, and quantities above 1 use the counter.

| Route | Drop rectangle (screen x) | Failure message |
| --- | --- | --- |
| Player inventory to player table | 245-309 | 26 |
| Merchant inventory to merchant table | 330-394 | 25 |
| Player table to player | 80-144 | 26 |
| Merchant table to merchant | 475-539 | 25 |

- The "player table to player" zone covers only part of the visible player column (screen x 109-172), so drops on its right 28 pixels fail silently. `[May-improve]`: also accept the visible column.
- For table slots the reference compares the row with the table length before adding the scroll offset, then reads row plus offset, which can read past the end once scrolled. `[May-improve]`: require row + offset below the table length.

**Keys.**

- `m` attempts the trade (SPEC-011 §3.19) when either table is nonempty, then shows `inventry.msg` 27 or 28 in the reply area.
- `t` returns the merchant table to the merchant and the player table to the player's root inventory, both forced, then returns to talk mode.
- Esc leaves barter. The dialogue barter cleanup then returns, forced, the player table to the player, the merchant table to the merchant and the temporary holder to the merchant.

Either way no item remains on a table, and dialogue resumes. The merchant's hidden armor and weapon are restored afterward (SPEC-011 §3.17). In the action menu, table entries count as not owned by the player. Drop on a player-owned item drops it on the ground even during barter.

### 3.19 Poison

Poison applies to the player only. `[Must-match]` unless tagged.

**Adjustment** (the `poison` opcode, stat setting and drug effects):

1. If the target is not the player, fail: the `poison` opcode emits its diagnostic, other paths do nothing, and the stored value is unchanged.
2. A positive amount is reduced by `trunc(amount * Poison Resistance / 100)`; negative amounts are not reduced.
3. Add the amount to the poison level.
4. If the level is positive: remove all type-5 events, queue one at `now + 10 * (505 - 5 * level)` ticks, and display `misc.msg` 3000.
5. Otherwise set the level to 0. Pending poison events are **not** removed.

**Tick** (event type 5):

1. If the owner is not the player, do nothing and return no interrupt.
2. Adjust poison by -2 through the rule above. While the level stays positive this queues the next tick from the new level and displays 3000 again.
3. Subtract 1 HP. At 0 HP or below the player dies by the normal death path.
4. Refresh the HP counter and display 3001.
5. Return interrupt when HP is at most 5, otherwise no interrupt.

No random draws are made.

**Consequences.**

- A level P lasts `ceil(P / 2)` ticks at 1 HP each. The interval uses the level remaining after each decrement: level 1 waits 5000 ticks, level 100 waits 50.
- A level above 101 yields a zero or negative delay, so the event is due at once and runs again in the same pass. It drains 2 per tick and 1 HP per tick within one frame until the level is at most 100, or the interrupt at HP 5 or below ends the pass.
- Every dose or partial cure restarts the timer from the current time.
- No ticks occur during combat, because game time does not advance there.

**Display glitch.** Every tick but the last, and every partial cure, displays "You have been poisoned!" before "You take damage from poison." `[May-improve]`: display 3000 only when a positive amount was applied.

**Stale event.** A cure to exactly 0 leaves the pending event, which fires once more, costs 1 HP and displays 3001. `[May-improve]`: remove pending poison events when an adjustment brings the level to 0.

### 3.20 Radiation and Non-Player Critters

**Exposure** (`radiation_inc`, `radiation_dec`, stat setting, drug effects), player only. `[Must-match]`

1. A target other than the player is a silent no-op.
2. A positive amount is reduced by `trunc(Radiation Resistance * amount / 100)`.
3. If the result is still positive, set the radiation-exposed flag (section 2.3).
4. If the amount is positive, look for an active Geiger counter (PID 52 or 207, with charges and an active drain event) in the left hand, then the right hand; the right hand wins. If one is found, display `misc.msg` 1009 when the amount exceeds 5, else 1008.
5. If the amount is at least 10, display 1007, after any Geiger message.
6. Add the amount to the radiation total, clamping at 0 from below. There is no upper clamp.

**Midnight check.** It runs only in the midnight event (type 4), after that event's other work and before the next midnight is queued. When a movie is already playing at midnight, the whole midnight handler returns early: there is no check and no re-queue. The check applies only to the player and only when the exposed flag is set. `[Must-match]`

1. Scan the queued radiation events in due order without removing them. The last one's level (onset or recovery) is the last scheduled level, or 0 if there are none.
2. Take the base level from the total radiation (section 2.6).
3. Draw D(1, 10). If the draw exceeds effective Endurance plus the level's modifier, add 1 to the level, which can therefore reach 6.
4. If the level exceeds the last scheduled level, draw D(4, 18) and queue an onset event {level, 0} at `now + 36000 * draw` ticks.
5. Clear the exposed flag, even when nothing was queued.

Lowering radiation never sets the flag, never triggers a check and never cancels queued sickness. `[Must-match]`

**Onset event.** `[Must-match]`

1. Remove every other radiation event. Each removed recovery event is applied first, reversing its level and displaying its stage message. Removed onset events vanish.
2. Queue a recovery event {same level, 1} at `now + 6048000` ticks (7 days).
3. Apply the level with sign +1.

**Recovery event.** Apply the same level with sign -1 and nothing more. Total radiation is not re-examined, so a player who still holds 500 radiation recovers after 7 days and is not sickened again until the next exposure. `[Must-match]`

**Applying level L with sign s.** `[Must-match]`

1. For the player, display `misc.msg` 1000 + L - 1, on onset and on recovery.
2. For each column of section 2.6 in order, add s times the penalty. SPECIAL and healing rate change through the prototype bonus values, so derived statistics are recomputed. Current HP has no bonus value, so it changes HP directly: a loss on onset can kill, and a gain on recovery is clamped to maximum HP.
3. If the critter is not dead and any SPECIAL's base plus trait adjustment plus bonus is below 1, it dies.
4. If the player is dead afterward, display 1006.

Recovery repeats the sickness stage text. `[May-improve]`: an engine may suppress the stage message on recovery but must still apply the reversal.

The radiation event handler does not test that its owner is the player. A save record owned by another critter would alter that critter's prototype bonuses, affecting every instance of the PID, and a record with no resolvable owner is read through a missing object. `[May-improve]`: ignore radiation events whose owner is not the player.

**Non-player critters.** Poison and radiation events are only ever created for the player. `[Must-match]`

- On another critter, `poison` changes nothing and emits its diagnostic.
- `radiation_inc` and `radiation_dec` do nothing.
- Setting stats 36 and 37 fails, and drug poison and radiation effects do nothing (SPEC-011).
- A non-player critter's stored poison or radiation, from map or save data, can be read by `get_poison` and stat queries. It never ticks, never damages, never triggers a check, and is not used by AI or combat.
- The character screen lights `editor.msg` 312 or 313 when the player's poison or radiation is nonzero.

### 3.21 Elevators

**Request.** `metarule` rule 15 stores its parameter as the elevator type and posts a deferred request; this is the only request path. Scenery elevator-type and level fields (SPEC-003) are loaded and saved but never read by the engine. Types outside 0 through 11 fail and nothing happens. In the normal loop the request is serviced by the deferred-request pass; in combat by the combat request pass (section 3.16). `[Must-match]`

**Window.** `[Must-match]` unless tagged.

- The window is placed at screen (`trunc((640 - w) / 2)`, `trunc((480 - 100 - 1 - h) / 2)`): (205, 47) for 230-by-284 backgrounds and (204, 47) for `el_mil1.frm`.
- A panel, when present, is drawn at (0, h - panel height).
- Button i is at (13, 40 + 60i), 48 by 50, drawn transparently. It reports on release and plays `ib1p1xx1` on press, with no release sound.
- A key whose character equals a table key character selects that button. Matching is case-sensitive, so only uppercase `G` works for type 1. `[May-improve]`: also accept lowercase `g`.
- Esc cancels.
- While the window is open, map background processing and palette cycling are suspended, the in-world cursor is off and the arrow cursor is shown.
- The gauge cell `trunc(level * 12 / (n - 1))` for n buttons is drawn at (121, 41).

**Travel animation**, when the chosen level differs from the start level. `[Must-match]` unless tagged.

1. Play the travel sound for (buttons, levels travelled):

   | Buttons | 1 level | 2 levels | 3 levels |
   | ---: | --- | --- | --- |
   | 2 | `elv1_1` | | |
   | 3 | `elv1_2` | `elv1_1` | |
   | 4 | `elv1_3` | `elv2_3` | `elv1_1` |

2. Repeatedly move the gauge position by `(n - 1) / 12` cell units toward the target (negative going down), draw cell `trunc(position)`, and busy-wait `trunc((n - 1) / 12 * 276.92307)` milliseconds: 23, 46 or 69 for 2, 3 or 4 buttons.
3. Stop when the position reaches or passes the target, then pause 200 milliseconds.

With single-precision accumulation the final cell can be one off; for 3 buttons from level 2 to level 1 it ends on cell 5 instead of 6. `[May-improve]`: draw the exact target cell at the end.

**Start level.** `[May-improve]`: start at the entry that matches both the current map and the player's elevation, else 0, never negative.

- The start level is the index of the first table entry whose map equals the current map, validated against an entry selected by an always-zero requested level. It is 0 when the map is absent from the table.
- Type 7 on map 42 subtracts 2 more, giving -2, which draws the gauge from outside its image and reads past the sound table.
- The indicator therefore depends only on the map, never on the actual elevation. Choosing the "start" button still moves the player there, with no animation or sound; on Vault 13 elevation 2, pressing `1` silently drops the player to elevation 0.

**Performing the result.** The selection returns success even for Esc, with destination hex -1. The caller then always: `[Must-match]` unless tagged.

1. Save the automap state.
2. **Same map, same elevation.** Cancel the player's animations, face southeast (2), and place the player at the destination hex.
3. **Same map, other elevation.** Before moving, find the first scenery door with PID `0x02000099`, `0x020001A5` or `0x020001D6` within 4 hexes of the player on the current elevation, in object order. Cancel animations, face southeast, and place the player. Then close that door instantly: frame 0; clear movement-nonblocking `0x10`, light-through `0x20000000` and projectile-through `0x80000000`; clear its open bit `0x01`; recompute lighting. There is no sound or animation. A missing door only produces a diagnostic.
4. **Other map**, in the normal pass only. Close the door as above, then request a map transition {map, elevation, hex, facing southeast}. The combat pass does not close the door on a map change.
5. **Placement.** Hex -1 does nothing. An unblocked destination is used. A blocked one uses the first unblocked neighbor in direction order NE, E, SE, SW, W, NW, else a candidate at distance 6.
6. **Elevation change.** Moving the player to another elevation switches the displayed elevation, recenters the view, ends combat if it is active and re-syncs the party (section 3.22). The exit-grid test runs on the new hex.

On Esc the player is still turned southeast and loses pending animations. A nearby elevator door is closed when the computed level's elevation differs from the actual one, although nobody moved. `[May-improve]`: Cancel changes nothing.

**Coverage.** `[Must-match]` The section 2.7 table is the complete elevator set in R1:

- the Brotherhood, types 0-1;
- the Master's lair under the Cathedral, types 2-3;
- the Military Base, types 4-5;
- the Glow, types 6, 7 and 11;
- Vault 13, type 8;
- the Necropolis vault, types 9 and 10.

No type names a Vault 15 map (`map.msg` 7 or 8), so any level change there is scripted. There is no other elevator art: the backgrounds, panels, buttons and gauge in section 2.7 are all of it. A destination hex is reached by the placement rule above, and on another map through the transition of section 3.22, which saves the outgoing map (SPEC-014 §3.35).

### 3.22 Exit Grids, Map Transitions and Party Following

**Exit grids.** Exit grids are miscellaneous PIDs `0x05000010` through `0x05000017`; their instance data holds map, hex, elevation and facing (SPEC-003). Whenever the player is moved onto a hex, by walking, placement or script, every exit grid on that hex and elevation requests a transition, and the last one wins. `[Must-match]`

**Transition request.** `[Must-match]`

- Destination map 0 means "world map", so map 0 can never be targeted directly. Map -1 means the town map, then the world map.
- A request made in combat ends combat; the transition happens once out of combat.

**Transition processing**, in the main loop out of combat. `[Must-match]`

1. Load the map by number; the same number still saves and reloads.
2. If the destination hex is neither -1 nor 0 and the elevation is 0 through 2, move the player there, set the elevation (which re-syncs the party) and set the facing. Otherwise keep the map's default entrance.
3. Recenter, and move the world-map position to the map's town.
4. At the end of the load, apply the facing again when the destination map is positive and the facing is not negative.

There is no palette fade. Loading shows the busy cursor and plays background sound `wind2` until the city music starts.

**Exit grids at the entrance.** The entrance placement of a map load (SPEC-006 §3.5 step 5) and the step-2 placement are ordinary player placements. Exit grids on that hex and elevation request a transition as usual: R1 neither skips the check nor moves the player off the grid. `[Must-match]` There is only one pending request. Any request made while a map loads replaces it: such an exit grid, `load_map` from the outgoing map's `map_exit_p_proc` or from a `map_enter_p_proc`, or any other request. What follows depends on what started the load. `[Must-match]` unless tagged.

- **Transition processing (steps 1-4).** Steps 2 and 4 read the replaced request, and the request is then cleared. The replacing request's map never loads. Instead, its destination hex, elevation and facing are applied on the map just loaded, when they pass the step-2 test. `[May-improve]`: save the hex, elevation and facing when processing starts and use them in steps 2 and 4; still clear the requests made during the load.
- **A town-map or world-map load started by map -1 or -2.** The request is cleared when the world map closes, so it is discarded.
- **A game load, the new game's first map, or a world map opened by a script.** The request stays pending, and the next main-loop pass processes it, so the player leaves the new map at once.

Example: an exit grid on map A leads to hex 12345, elevation 0 of map B. Map B's default entrance, hex 20100 on elevation 0, holds an exit grid to map C, hex 5000, elevation 1, facing 2. Map B loads, and at the end of the load the player turns to facing 2. The player is then moved to hex 5000 on elevation 1 **of map B**, and the request is cleared, so map C never loads. Under the safe rule the player stands on hex 12345, elevation 0 of map B, with the facing of map A's grid. No corpus map has an exit grid on its default entrance hex and elevation, or on the hex and elevation an exit grid leads to. The 72 corpus maps hold 13055 exit grids, 6409 of them with a positive destination map. On shipped data these cases can therefore arise only through scripts.

**Leaving a map.** `[Must-match]` Before the destination loads, the outgoing map is saved into the working map set and then cleared. The destination is then loaded from its working `.SAV` when one exists, otherwise from its original `.MAP`. SPEC-014 §3.35 gives both steps, the revisit aging and the maps that are never saved.

**World map and town map destinations.** `[Must-match]` These run in the main loop, out of combat, after all animations are stopped.

- **Map -2** (an exit grid holding map 0): open the world map at the current town.
- **Map -1:** open the town map of the current town first. Its result decides what follows:
  - **A hotspot was chosen.** If the party is already in that town on the world map, load the hotspot's map directly. Otherwise the world map opens with the party already travelling to that town. On arrival the party stops at rest on the world map: it is not entered automatically, and the chosen hotspot is not used (SPEC-014 §3.39). SPEC-008 §3.7 gives the hotspots, their maps and their entry selectors.
  - **The World Map button:** open the world map normally.
  - **Failure** (an out-of-range town or a missing town picture): the world map is entered with a failure state and returns at once. The outgoing map has already been saved and unloaded, so no map is loaded and play is left on an empty map. `[May-improve]`: open the world map at the current town.
  - **A quit request:** nothing more.
- **Saving first.** Both the town map and the world map save the outgoing map first (SPEC-014 §3.35). If that save fails:
  - show `map.msg` 1000 ("Error: Not enough disk space to save map!") in a large dialog at (169, 116), with the arrow cursor and sound `iisxxxx1`;
  - end the game session with a return to the main menu.
- The transition request is cleared after the world map closes.

**Party following.** On an elevation change, each visible party member other than the player is placed around the player's new hex by the placement rule with starting radius 2: in practice successive members fill successive free neighbors in NE through NW order. On a map change, members travel with the map save and load and are placed the same way. The elevator code itself moves only the player. `[Must-match]`

### 3.23 Ladders and Stairs

Ladder and stairs instances store packed destination words (SPEC-003), but no engine code reads them. Every actual move or map change is performed by the object's `use_p_proc` script, so the script decides between "same map" and "other map". `[Must-match]`

**Ladder up**, plain use without an item. `[Must-match]` unless tagged.

1. Stop the player's walk or run.
2. Move to the hex one step southeast of the ladder, on the ladder's elevation: walking in combat or at distance below 5, otherwise running, and AP-limited in combat.
3. Require adjacency. The "cannot get there" test checks the wrong object, so `misc.msg` 2000 never appears for the player. `[May-improve]`: display it when the actor is the player. This is the adjacency check every approaching interaction shares, not a ladder-only fault (SPEC-007 §3.11.3).
4. Turn to face the ladder.
5. In combat, require 3 AP, or show `proto.msg` 700.
6. Put away a held weapon (animation 39).
7. Play the climb sound (the `sfx_build_char_name` rule for animation 4) and the climb animation 4.
8. Request `use_p_proc`. Without an override, display `proto.msg` 480 with the object's name.
9. Take the weapon out again.

**Ladder down and stairs** use the generic scenery use. Approach and adjacency as above, the 3-AP combat check, put away the weapon, then the reach animation: ground (10) when bit `0x01` of the scenery prototype's extended flags is set, else middle (11). Then `use_p_proc` or message 480, and take the weapon out again. `[Must-match]`

### 3.24 Random-Draw Order Summary

| Operation | Draws, in order |
| --- | --- |
| Skilldex, hotkeys, approach, jam check, script dispatch | None |
| Sneak toggle or recheck | None (reference); Skill check under the section 3.7 safe rule |
| Lockpick, Traps, Science, Repair defaults | None |
| First Aid | Limit or dead: D(0, 2). Injured non-robot: D100, critical-upgrade D100 when the day gate allows, heal draw on success |
| Doctor | Limit or dead: D(0, 2). Otherwise per present crippled bit in order blind, left arm, right arm, right leg, left leg: D100 and upgrade; then the HP roll D100, upgrade and heal draw on success. Robots draw nothing |
| Kit route | The routine's draws, then D(1, 10) unless the routine failed |
| Steal attempt | Steal D100 and upgrade (none for a party member); then, when the steal result is not critical, catch D100 and upgrade |
| Arming an explosive | After OK: Traps D100 and upgrade |
| Explosion | One damage draw for the center-hex critter, if any |
| Dropped armed explosive | None in R1 (empty enumeration) |
| Poison tick | None |
| Radiation midnight check | D(1, 10); then D(4, 18) only when a new onset is queued |
| Elevators, exit grids, ladders, loot, counter, barter moves | None |
| Reading a skill book (section 3.25) | None |

"Upgrade" means the critical-upgrade D100, drawn only when the clock is at least 864000 ticks. The table is exact. `[Must-match]`

### 3.25 Skill Books

`[Must-match]` unless tagged.

| PID | Book (corpus name) | Skill | Message |
| ---: | --- | --- | ---: |
| 73 | Big Book of Science | 12 Science | 802 |
| 76 | Deans Electronics | 13 Repair | 803 |
| 80 | First Aid Book | 6 First Aid | 804 |
| 86 | Scout Handbook | 17 Outdoorsman | 806 |
| 102 | Guns and Bullets | 0 Small Guns | 805 |

No other PID is a skill book. Messages are `proto.msg` (section 2.4).

**Entry.** A book is read through the item-use route. That route tests the book PIDs first, before flares, radios and explosives.

- **Interface-bar item slot.** Use is offered when the prototype is usable. In combat it needs and spends the slot's ordinary use cost.
- **Inventory action menu.** Use (SPEC-014 §3.34).

There is no other route. The book's own script gets no `use_p_proc` call on this route.

**Procedure.**

1. **In combat**, show message 902 and stop. The book is kept. Through the item slot the AP cost is still spent. `[May-improve]`: refuse before spending AP.
2. Let S be the player's effective skill percentage (SPEC-006 §3.3, with traits, perks, difficulty and the 200 cap). Compute `n = trunc((100 - S) / 10)`, truncating toward zero.
3. If n ≤ 0, that is S ≥ 91, the result message is 801 and the skill is unchanged. Otherwise the result message is the book's message. Then repeat n times: add 1 to the unspent skill points, then spend one point on the skill (SPEC-006 §3.3). Each point adds +1 percentage point, or +2 for a tagged skill. Unspent points therefore end where they started. n is computed once, before any increase.
4. Fade to black (SPEC-014 §3.2).
5. Advance the game clock by `36000 × (11 - IN)` ticks, where IN is the player's effective Intelligence, so between 1 and 10 game hours. The clock is advanced directly. Timed events that fall due are not run during the read; they run in order on the next queue pass (SPEC-006 §3.7).
6. Run the map-update scripts.
7. Fade back to the game palette.
8. Show message 800, then the result message.
9. The book is destroyed, **including** when the result is 801.
   - From the inventory, it is first placed on the player's hex and then destroyed.
   - The item-slot route runs the map-update scripts once more after the use.

A book draws no random numbers and grants no experience.

## 4. Synthetic Test Fixtures (Hex / Text)

These vectors are invented. Random outcomes are explicit inputs listed in draw order. Unless stated, the clock is at least 864000 ticks, so critical-upgrade draws are made.

### 4.1 Fixture A: Steal Checks

Thief: the player, effective Steal 60, Critical Chance 5, no Pickpocket. Victim: an active non-party critter with effective Steal 40. Item size 2. First attempt of the session, so N = 1.

| Case | Facings (thief, victim) | Outcomes | Computation | Result |
| --- | --- | --- | --- | --- |
| A1 | 1, 4 | `20, 50, 70, 90` | Difference 3, in front: M = 0 - 8 - 25 = -33; S = 27; margin 7, success; threshold `0 + 5`, 50 does not upgrade. Catch chance `40 + 33 = 73`; margin 3, success; upgrade threshold 0 | Caught: `skill.msg` 570; window closes |
| A2 | 4, 4 | `20, 50, 70, 30` | Difference 0, behind: M = -8; S = 52; success, no upgrade (threshold 8). Catch chance 48; margin -22, failure; 30 exceeds 2 | Not caught: 571 |
| A3 | 4, 4 | `2, 5` | S = 52; margin 50; threshold `5 + 5 = 10`; 5 upgrades to critical success | Not caught, no catch draws |
| A4 | Any | none | Victim is a party member | Critical success, no draws, not caught, no experience |

Four successful moves in one uncaught session give pending experience `10 + 20 + 30 + 40 = 100`; with Steal 60 the cap is 240, so 100 is awarded and message 29 shows 100.

### 4.2 Fixture B: First Aid, Doctor and Usage Slots

First Aid through a First Aid Kit: player First Aid 50, Critical Chance 5, K = 20, no Healer, target HP 10 of 30. Outcomes `40, 90, 4, 7`: chance 75, margin 35, success; threshold `3 + 5 = 8` does not upgrade; heal 4 gives HP 14 and message 500 shows 4. A slot is recorded, 18000 ticks pass and experience is 25. The kit draw 7 keeps the kit. Three map-update passes are requested.

Doctor through a Doctor's Bag: player Doctor 40, Critical Chance 5, K = 20, target at full HP 30, blind and with a crippled left leg. The attempt counter runs 1, 2, 3.

| Step | Outcomes | Result |
| --- | --- | --- |
| Blind | `30, 80` | Chance 65; success; clear `0x40`; 521 with "damaged eye"; slot recorded; experience 50 |
| Left leg | `90, 50` | Margin -25; failure; 526 with "crippled left leg"; experience 50 more in the reference (a limb already succeeded), 0 under the safe rule |
| HP roll | `10, 99, 6` | Chance 40, critical modifier 25; margin 30; threshold `3 + 25 = 28`, 99 does not upgrade; heal 6 leaves HP 30; 500 shows 0; no slot recorded; experience 50 |
| Kit | `3` | Kit kept |

Time spent: `3 * 3600` seconds = 108000 ticks. Total experience is 150 in the reference and 100 under the safe rule.

Usage slots holding `100000, 200000, 300000`: at now = 999999, H = 24, so the skill is unavailable. At now = 1000000, H = 25, so it is available, and a successful heal leaves the slots `200000, 300000, 1000000`.

### 4.3 Fixture C: Explosives

Player Traps 50, Critical Chance 5, timer 60 seconds (delay 600 ticks).

| Outcomes | Result |
| --- | --- |
| `60, 95` | Margin -10, failure; upgrade threshold 1 not met; type 11 queued after 300 ticks |
| `30, 50` | Success; type 8 queued after 600 ticks |
| `99, 3` | Margin -49; 3 is at most 4, critical failure; type 11 after 0 ticks |

Detonating Dynamite with damage draw 40 on a center-hex critter with explosion DT 4 and DR 20 gives `36 - trunc(36 * 20 / 100) = 29` damage and knockback 2. A critter on an adjacent hex takes nothing in R1.

### 4.4 Fixture D: Poison and Radiation Timelines

Poison: the player with Poison Resistance 20 receives `poison` 10. The amount becomes `10 - 2 = 8`; the level is 8; the next tick is due in `10 * (505 - 40) = 4650` ticks, and 3000 is displayed.

| Tick | Level after | Next delay | Messages | HP change |
| ---: | ---: | ---: | --- | ---: |
| 1 | 6 | 4750 | 3000, 3001 | -1 |
| 2 | 4 | 4850 | 3000, 3001 | -1 |
| 3 | 2 | 4950 | 3000, 3001 | -1 |
| 4 | 0 | none | 3001 | -1 |

Level 150: the first tick leaves 148 with delay `10 * (505 - 740)`, which is negative, so ticks repeat in the same pass until the level is 100 (25 ticks, 25 HP), unless HP reaches 5 first.

Radiation: total 450, the exposed flag set, effective Endurance 6, nothing queued. Base level 3, modifier -4. Outcomes `3, 10`: the Endurance draw 3 exceeds `6 - 4 = 2`, so the level becomes 4; 4 exceeds the last scheduled level 0, so an onset {4, 0} is queued at `now + 360000` ticks. At onset: display 1003, ST -2, EN -1, AG -2, HP -5, healing rate -5, and queue recovery {4, 1} for 7 days later. Recovery reverses those changes and displays 1003 again.

### 4.5 Fixture E: Elevators

| Case | Input | Expected |
| --- | --- | --- |
| E1 | Type 8 on map 6, player on elevation 2, press `1` | Reference start level 0, so no animation or sound; player placed at 14104 on elevation 0 facing southeast; nearby elevator door closed |
| E2 | Same, under the safe rule | Start level 2; sound `elv1_1` (3 buttons, 2 levels); gauge from cell 12 down to cell 0 |
| E3 | Type 0 on map 14, press `3` | Destination 15/0/21340: map transition request after closing the nearby door |
| E4 | Type 1, press lowercase `g` | Ignored in the reference; selects button 0 under the safe rule |
| E5 | Type 12 | Request fails, nothing happens |

Gauge cells for 4 buttons: levels 0-3 map to cells 0, 4, 8, 12. The step delay for 4 buttons is 69 milliseconds.

### 4.6 Fixture F: Move-Items Counter

Stack of 25. Start value 1. Keys `3`, `7` give 37; Enter plays `iisxxxx1` because 37 exceeds 25. Minus gives 36, still too high. ALL gives 25; Enter accepts. In a fresh counter, `1`, `2`, `3`, `4` give 1, 12, 123, then `(1230 mod 1000) + 4` = 234.

### 4.7 Fixture G: Skill Books

All cases are out of combat with effective Intelligence 6, unless stated.

| Case | Book and starting skill | n | Result |
| --- | --- | ---: | --- |
| G1 | Big Book of Science, Science 45, untagged | 5 | Science 50; unspent points unchanged; clock +180000 ticks (5 hours); messages 800, 802; book destroyed |
| G2 | Guns and Bullets, Small Guns 45, tagged | 5 | Small Guns 55 |
| G3 | Scout Handbook, Outdoorsman 90 | 1 | Outdoorsman 91; messages 800, 806 |
| G4 | Scout Handbook, Outdoorsman 91 | 0 | Unchanged; messages 800, 801; clock +5 hours; book destroyed |
| G5 | Big Book of Science, Science 120 after perks and difficulty | -2 | Unchanged; messages 800, 801 |
| G6 | First Aid Book, Intelligence 10 / Intelligence 1 | - | Clock +36000 / +360000 ticks |
| G7 | Any book from the item slot in combat | - | Message 902; book kept; the slot's use AP spent |

## 5. Clean-Room Verification Checklist
- [x] Zero C/C++ code snippets or struct definitions included
- [x] All multi-byte integer endianness explicitly stated
- [x] Synthetic test vectors included
- [x] Every documented quirk tagged `[Must-match]` or `[May-improve]`
- [x] No internal reference symbol names; only public SSL opcode names and file-format key literals
- [x] Spec linter run against `fallout1-re` symbols with no findings

Verification notes: `tools/spec_lint.py` gives 0 errors, and no source path or raw revision hash appears. Its 23 plain-word warnings were reviewed and are benign: this spec's own formula and UI words ("value", "level", "draw", "maximum", "position", "size", "current", "trunc", "slot", "count", "counter", "attempt"). The public opcode names `sfx_build_open_name` and `sfx_build_char_name` appear only as naming-rule references. The fenced blocks hold no code; fixtures are tables. Cross-spec note (resolved): SPEC-007 §3.7 now states the same rule as section 3.10. Every explosion, whether weapon, grenade, timed or scripted, uses the one disabled victim enumeration, so in R1 only the center target takes damage. Retaliation and onlooker wording now matches the single rule in SPEC-007 §3.6. The added warning "start" is the public SSL procedure name start, used in section 3.10. Explosion art, sound and skill-book names in sections 3.10 and 3.25 are corpus file and prototype names, checked against `art/misc/misc.lst`, the corpus FRM headers, `sound/sfx` and `pro_item.msg`. The section 3.21 coverage note was checked against `art/intrface/intrface.lst` and `map.msg`. Section 3.16 step 3 and the pointers in sections 3.2 and 3.23 were checked against SPEC-007 §3.11.3 (R1-rev7) so that both specs state one container reach timing rule and one adjacency check. Tag count, excluding the two tag definitions: 98 `[Must-match]`, 36 `[May-improve]`.

Update notes (2026-10-22, R1-rev7): section 3.22's exit-grid and hotspot-travel rules; the corpus scan found no exit grid on any default entrance hex or exit-grid destination among 13055 grids. Tag count is now 100 `[Must-match]`, 37 `[May-improve]`. `tools/spec_lint.py` gives 0 errors.

Update notes (2026-10-23): the section 3.15 Set Timer table gains the title color and the button sounds and timing. All four buttons use `ib1p1xx1` / `ib1lu1x1`; plus and minus act once per press with no repeat. No tag change. `tools/spec_lint.py` gives 0 errors.

Update notes (2026-10-23, R1-rev8): the section 3.10 first-load wording now matches SPEC-006 §3.5. No tag change. `tools/spec_lint.py` gives 0 errors.
