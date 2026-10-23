# SPEC-011: Items, Drugs, Barter, and Character Progression

## Clean-Room Rules for Research Agents
- Document ONLY binary file layouts (byte offsets, endianness, data types), mathematical formulas, state transitions, and observable behavior.
- DO NOT copy C/C++ code, struct definitions, function names, internal variable names, or implementation-source paths from the inspected implementation.
- Provide small synthetic hex/text test vectors at the bottom of the specification.

## 1. Overview & Scope

This specification describes reference revision R1, targeting the 32-bit Windows engine. Evidence is static inspection of the drug, withdrawal, inventory, equipment, ammunition, barter, experience, perk, and character-editor advancement paths, plus the call sites that consume perk ranks. A structural survey of a locally owned unpacked corpus (section 4.12) checked the prototype invariants these rules depend on. No original item statistics, drug amounts, prices, or message texts are reproduced. All fixtures use synthetic prototypes and explicit random draws.

This document covers:

- drug consumption: immediate stat changes, the two timed stages, the random-range form, death by overdose, addiction rolls, withdrawal onset and duration, re-dosing, and map-exit handling;
- the drug-use contract that SPEC-009 §3.7 (NPC healing) depends on (section 3.7);
- equipment: armor AC/DR/DT application, armor perks, hand slots and two-handed weapons, strength requirement, and opening the inventory in combat;
- inventory: stacking, ammunition stacks, weight, carry and container limits, item value, reload and unload;
- barter: the merchant price formula, reaction and script modifiers, Master Trader, and transaction acceptance;
- the experience table, level-up HP, skill points, perk timing, the complete perk eligibility table, and perk effects.

Relationship to other specifications: SPEC-002 defines `.MSG` parsing. SPEC-003 defines the prototype layouts that this document restates by meaning. SPEC-005 defines opcode stack contracts. SPEC-006 defines effective stats, trait adjustments, skill percentages, the random generator, reaction levels, and the timed queue. SPEC-007 defines AP costs, attack preflight, hit chance, and damage application. SPEC-008 defines save components and queue payload bytes. SPEC-009 defines NPC healing and reloading in combat. This document references those rules and does not restate them.

**Compatibility tags.** Every documented quirk carries one tag:

- `[Must-match]`: gameplay-visible behavior, data grammar, formula, ordering, or random-draw sequence. A clean-room engine must reproduce it exactly.
- `[May-improve]`: reference memory-safety defects, undefined behavior, stale state, presentation-only transients, or a lookup that reads the wrong table in an obviously unintended way. A clean-room engine may apply the stated safe rule. It should keep every other observable behavior. An engine that wants bit-exact parity may offer a compatibility switch that keeps the reference behavior.

Terminology used in this document (descriptive, not reference identifiers):

| Term | Meaning |
| --- | --- |
| Player | The player-controlled critter |
| Bonus array | The per-prototype additive stat words 0-34 (SPEC-006). Changing a SPECIAL bonus recomputes derived stats |
| R(X), E(X) | Raw-plus-trait and effective primary attribute (SPEC-006 §3.1) |
| `roll(a,b)` | SPEC-006 inclusive random integer. Every call consumes one draw, including equal bounds |
| Tick | Game-time unit: 600 per game minute, 864000 per day (SPEC-006 §3.7) |
| Stage | One of a drug's three effect groups: immediate, delayed 1, delayed 2 |
| Addiction table | The fixed seven-entry PID → campaign-variable table in section 2.3 |
| Hand slot | Left or right hand. An item in a hand carries the matching in-hand flag |
| Merchant | The dialogue target in barter |

Integer division truncates toward zero throughout, as in SPEC-006. `[Must-match]`

## 2. Binary Layout / Data Format (Byte Offset Tables)

### 2.1 Numeric Conventions

Every multi-byte integer in prototypes, saves, and queue payloads is a **big-endian signed 32-bit** word unless stated otherwise (SPEC-003, SPEC-008). Message files are byte-oriented text with no binary integers, so endianness does not apply to them. The tables below restate fields by meaning. Offsets are relative to the item subtype payload at file byte `0x39` of an item prototype.

Stat indices used by drugs, perks, and armor (SPEC-003 order):

| Index | Stat | Index | Stat |
| ---: | --- | ---: | --- |
| 0-6 | Strength, Perception, Endurance, Charisma, Intelligence, Agility, Luck | 15 | Critical chance |
| 7 | Maximum HP | 16 | Better-critical modifier |
| 8 | Maximum AP | 17-23 | Damage threshold, by damage type 0-6 |
| 9 | Armor class | 24-30 | Damage resistance, by damage type 0-6 |
| 10 | Unarmed damage | 31 | Radiation resistance |
| 11 | Melee damage | 32 | Poison resistance |
| 12 | Carry weight | 33, 34 | Age, gender |
| 13 | Sequence | 35 | Current HP (instance state; not in the bonus array) |
| 14 | Healing rate | 36, 37 | Current poison level, current radiation level (instance state) |

Damage types 0-6 are normal, laser, fire, plasma, electrical, EMP, explosion. `[Must-match]`

### 2.2 Drug Prototype Payload (68 bytes)

| Payload offset | Bytes | Endianness | Meaning |
| --- | ---: | --- | --- |
| `0x00` | 12 | Big-endian, 3 × signed 32 | Stat selectors S0, S1, S2. Each is -1 (unused slot) or a stat index 0-37. S0 alone may be -2 (random-range form, section 3.2) |
| `0x0C` | 12 | Big-endian, 3 × signed 32 | Immediate amounts I0, I1, I2 |
| `0x18` | 4 | Big-endian signed 32 | Delayed-stage-1 delay D1, in game minutes from consumption |
| `0x1C` | 12 | Big-endian, 3 × signed 32 | Delayed-stage-1 amounts |
| `0x28` | 4 | Big-endian signed 32 | Delayed-stage-2 delay D2, in game minutes from consumption (not from stage 1) |
| `0x2C` | 12 | Big-endian, 3 × signed 32 | Delayed-stage-2 amounts |
| `0x38` | 4 | Big-endian signed 32 | Addiction chance, percent |
| `0x3C` | 4 | Big-endian signed 32 | Withdrawal perk index (section 3.12), applied as a perk effect |
| `0x40` | 4 | Big-endian signed 32 | Withdrawal onset, in game minutes |

All three stages share the same selectors S0-S2. `[Must-match]`

### 2.3 Addiction Table

| Drug PID | Campaign variable written on addiction (player only) | Withdrawal note |
| ---: | ---: | --- |
| 106 (`0x6A`) | 189 | — |
| 87 (`0x57`) | 190 | — |
| 53 (`0x35`) | 191 | — |
| 110 (`0x6E`) | 192 | — |
| 48 (`0x30`) | 193 | — |
| 124 (`0x7C`) | 194 | Shares variable 194 with PID 125 |
| 125 (`0x7D`) | 194 | Shares variable 194 with PID 124 |

A PID not in the table maps to "no variable" (-1). `[Must-match]` The table is fixed engine data, not read from a file. `[Must-match]`

### 2.4 Other Prototype Fields Consumed Here

| Subtype | Payload offset | Bytes | Meaning used by this spec |
| --- | --- | ---: | --- |
| Common item | file `0x28` | 4 | Size (container capacity, steal penalty) |
| Common item | file `0x2C` | 4 | Weight |
| Common item | file `0x30` | 4 | Base cost |
| Common item | file `0x18` | 4 | Extended flags; `0x00000200` = two-handed weapon |
| Armor | `0x00` | 4 | Armor class |
| Armor | `0x04` | 28 | DR for damage types 0-6 |
| Armor | `0x20` | 28 | DT for damage types 0-6 |
| Armor | `0x3C` | 4 | Armor perk index, or -1 |
| Container | `0x00` | 4 | Maximum contained size |
| Weapon | `0x1C` | 4 | Minimum strength |
| Weapon | `0x34` | 4 | Caliber |
| Weapon | `0x3C` | 4 | Magazine capacity |
| Ammunition | `0x00` | 4 | Caliber |
| Ammunition | `0x04` | 4 | Rounds per ammunition item (the "pack size") |

Instance words (SPEC-003 §3): a weapon instance stores its loaded-round count and current ammunition PID. An ammunition instance stores the rounds remaining in its representative pack, at the same instance position. `[Must-match]`

### 2.5 Persistent Progression and Queue State

| Location | Bytes | Endianness | Meaning |
| --- | ---: | --- | --- |
| `SAVE.DAT` component 10 | 256 | Big-endian, 64 × signed 32 | Perk rank per perk index 0-63 |
| `SAVE.DAT` component 13 | 20 | Big-endian, 5 × signed 32 | Unspent skill points, level, experience, reputation, karma |
| `SAVE.DAT` component 19 | 5 | Big-endian signed 32, then 1 byte | Last reconciled level; pending-perk byte |
| Queue event type 0 | 24 | Big-endian, 6 × signed 32 | Drug stage: three selectors, then three amounts |
| Queue event type 2 | 12 | Big-endian, 3 × signed 32 | Withdrawal: kind (1 = onset, 0 = end), drug PID, perk index |

The drug-stage payload does **not** store the drug PID. Nothing reads that PID after scheduling, so this has no visible effect. `[Must-match]` Withdrawal perk effects are never written as perk ranks: component 10 holds only selected ranks. `[Must-match]` The progression bounds are: unspent points ≥ 0, level ≥ 1, experience ≥ 0, reputation -20..20, karma ≥ 0. `[Must-match]`

## 3. Behavioral Rules & Edge Cases

### 3.1 Drug Consumption Sequence

Consuming drug item G by critter C runs these steps in order:

1. If C is dead, or C has body type 2 (robotic), the result is **-1**. Nothing else happens: no effect, no draw, and no queue change. `[Must-match]`
2. Record C, the addiction-table variable of G's PID, and G's withdrawal onset for the re-dose rule (section 3.5). Then run the re-dose rule. `[Must-match]`
3. Apply the immediate stage (section 3.2) with amounts I0-I2, marked "immediate". `[Must-match]`
4. Schedule delayed stage 1, then delayed stage 2 (section 3.3). `[Must-match]`
5. Run the addiction check (section 3.4). `[Must-match]`
6. The result is **1**. `[Must-match]`

The result is 1 even when the immediate stage killed C. `[Must-match]` The caller decides whether to destroy the item (section 3.6).

### 3.2 Applying a Stage

A stage has the three selectors and three amounts A0-A2. Process the slots in order:

1. If S0 = -2, skip slot 0 and start at slot 1. The **first processed slot** (slot 1) uses the amount `roll(A0, A1)`. Slot 2 uses A2 directly. Otherwise slots 0, 1, 2 use A0, A1, A2 directly. `[Must-match]` A reversed range follows the SPEC-006 reversed-range rule. `[Must-match]`
2. A slot whose selector is -1 is skipped. A skipped slot does not consume the random amount. With S0 = -2 and S1 = -1, the range draw is moved to slot 2, which then uses `roll(A0, A1)` instead of A2. `[Must-match]`
3. For selector 35 (current HP), first clear the critter's fleeing intent bit `0x04` (SPEC-009 §2.1). `[Must-match]`
4. For a stat in the bonus array (0-34), set the bonus to `old bonus + amount`. A SPECIAL change (0-6) recomputes derived stats (SPEC-006 §3.1). `[Must-match]` Bonus arrays live on the critter **prototype**, so a drug taken by one NPC changes every critter that shares its prototype. `[Must-match]`
5. For selector 35, add the amount to current HP, capped at effective maximum HP. If the result is ≤ 0 and the critter is not already dead, the critter is killed. `[Must-match]`
6. For selectors 36 and 37, apply the SPEC-005 poison/radiation adjustment. It acts **only on the player**: a positive amount is first reduced by `amount × resistance / 100`. It is a silent no-op for NPCs. `[Must-match]`
7. For the player only, compare the effective value before and after each slot. If they differ, print `item.msg` 1 ("gained") or 2 ("lost") with the absolute difference and the stat name. `[Must-match]`

After all slots: if C's current HP is > 0, and C is the player, and no slot printed a change, and the stage is immediate, print `item.msg` 10 ("nothing happens"). `[Must-match]` If C's HP is ≤ 0, the reference composes an overdose text (`item.msg` 4 for the player, 600 for an NPC) but **never displays it**. `[Must-match]` A clean engine shows no overdose text, and death presentation comes from the kill path.

### 3.3 Delayed Stages

For each delayed stage in order (stage 1, then stage 2):

1. If all three of its amounts are zero, it is not scheduled. With S0 = -2, amount A0 is the range's low bound and still counts as nonzero. `[Must-match]`
2. The delay is `600 × D` ticks. For the **player** with the Chem Resistant trait, halve it (integer division). `[Must-match]`
3. Queue a type-0 event owned by C, holding the selectors and that stage's amounts. `[Must-match]`

When the event fires, apply the stage (section 3.2) as non-immediate. A random range is drawn at **firing time**, independently of the immediate draw. `[Must-match]` The event handler stops the current queue pass (SPEC-006 §3.7) only when its owner is the player and the player is dead afterwards. `[Must-match]`

Delayed amounts are deltas to the bonus array, not absolute values. A drug "wears off" only because its author supplied opposite-signed delayed amounts. `[Must-match]`

### 3.4 Addiction Check

1. The reference's "already addicted" test looks up G's PID in the addiction table. It then reads the campaign variable **whose index equals the PID** (48, 53, 87, 106, 110, 124 or 125), not the mapped variable 189-194. PIDs outside the table read as "not addicted". `[May-improve]` Safe rule: read the mapped addiction variable. Either way, the test is global and is applied to NPC consumers too. `[Must-match]`
2. If the test says "addicted", skip the rest of this section. No draw is made. `[Must-match]`
3. Otherwise let P = addiction chance. For the **player** only, apply in this order: Chem Reliant doubles P; then Chem Resistant halves P; then Flower Child halves P (integer division each time). `[Must-match]`
4. Draw `roll(1, 100)`. The draw happens **even when P ≤ 0**. `[Must-match]`
5. If the draw ≤ P: queue a withdrawal **onset** event (kind 1) owned by C, delayed `600 × onset` ticks, holding G's PID and withdrawal perk. No trait or perk scales the onset delay. `[Must-match]` If C is the player, set the mapped campaign variable to 1 (if any) and set the player's "addicted" flag `0x00000010` (SPEC-006 §2.2). `[Must-match]`

Because the step-1 test normally reads an unrelated campaign variable, a player who is already addicted keeps rolling on every dose. Each success queues another onset event, so withdrawal effects can stack. `[May-improve]` The safe rule in step 1 removes this.

### 3.5 Re-Dose Rule (Withdrawal Reset)

Before the immediate stage, scan the queue in order for withdrawal events. Find the first one whose owner is C and whose drug's mapped variable equals G's mapped variable. Two PIDs that are both outside the table compare equal (both -1). `[Must-match]` For that **first** match only:

1. If it is an end event (kind 0), remove its perk effect now and, for the player, print `item.msg` 3. Do not clear the addiction variable. `[Must-match]`
2. Remove it. Queue a new **onset** event for C with the matched event's PID and perk, delayed by **G's** onset, in game minutes × 600 ticks. `[Must-match]`

Later matching events are left untouched. `[Must-match]` The newly queued event is never matched again in the same scan. `[Must-match]` Re-dosing therefore postpones withdrawal or ends an active withdrawal and schedules a fresh one. It does not cure an addiction.

### 3.6 Who Destroys the Item

| Path | Destroys the drug item when | Messages |
| --- | --- | --- |
| Inventory "use" on self | Result is nonzero, which **includes -1** | None beyond section 3.2 |
| Use item on a critter (world action or `use_obj_on_obj`) | Result is exactly 1 | Player using on another critter: `proto.msg` 581 with item and target names |
| NPC healing (SPEC-009 §3.7) | SPEC-009 removes one unit first and forces it back on -1 | `misc.msg` 5000 on success |

The table rows are `[Must-match]` except where a bullet below says otherwise.

- Inventory "use": the -1 case needs a dead or robotic consumer, which the player inventory cannot normally reach. `[May-improve]` Safe rule: destroy only on 1.
- Use item on a critter: the target's script procedure for being used-on runs first and can override the default (SPEC-006 §3.8). `[Must-match]`
- Use item on a non-critter target: the player gets `proto.msg` 582 ("does nothing"), the result is -1, and the item is kept. `[Must-match]`
- Use item on a dead critter: one `roll(0, 3)` selects `proto.msg` 583-586, the result is -1, and the item is kept. `[Must-match]`
- The reference's "you use the X" message 580 for the self case is unreachable on the use-on path. `[Must-match]`

### 3.7 Drug-Use Contract for SPEC-009 NPC Healing

SPEC-009 §3.7 refers to "the normal drug-consumption rules". That contract is:

1. **Input:** a living critter C and a drug item G that has been removed from C's inventory. `[Must-match]`
2. **Failure:** result -1 only when C is dead or robotic (section 3.1 step 1). On failure nothing changes and no draw is consumed. SPEC-009 then puts the unit back and still charges 2 AP. `[Must-match]`
3. **Success:** result 1. In order, this means: the re-dose scan; the immediate stage (one range draw if S0 = -2); delayed scheduling (no draws); and the addiction check (one `roll(1, 100)` unless the step-1 test reads "addicted"). `[Must-match]`
4. **Healing amount:** current HP rises by the stage's HP amount, capped at maximum HP. The fleeing intent bit is cleared. The SPEC-009 loop then re-tests `HP < floor(max HP / 2)`. `[Must-match]`
5. **NPC-specific differences:** no trait or perk scaling (Chem Reliant, Chem Resistant, Flower Child apply only to the player); poison/radiation slots do nothing; no "gained/lost" or "nothing happens" text; and addiction variables are not written. A successful addiction roll still queues withdrawal for the NPC, and its perk effect changes the NPC's prototype bonus array. `[Must-match]`
6. **Draw order inside one SPEC-009 healing iteration:** range draw (if any) → addiction draw (if any). SPEC-009 itself adds no draws. `[Must-match]`

### 3.8 Withdrawal Onset, Duration, and End

When an onset event (kind 1) fires for owner C:

1. Apply perk effect W (section 3.12) to C. `[Must-match]`
2. For the player, print W's `perk.msg` description (201 + W). `[Must-match]`
3. Queue an end event (kind 0) with the same PID and perk, delayed `600 × 10080` ticks (7 days). For the player only, halve the minutes for Chem Reliant, then halve them again for Flower Child. `[Must-match]`

When an end event fires: remove perk effect W from C. For the player, print `item.msg` 3, set the mapped addiction variable to 0 (if any), and clear the "addicted" flag if no addiction-table PID still reads as addicted. `[Must-match]` That final "any addiction" test reads the **PID-indexed** campaign variables, as in section 3.4 step 1. `[May-improve]` Safe rule: test variables 189-194.

Every withdrawal event owned by the player stops the current queue pass when handled. During world-map travel or Pip-Boy waiting, that interrupts the travel or wait (SPEC-008). `[Must-match]` NPC withdrawal events do not stop the pass. `[Must-match]`

### 3.9 Leaving a Map

On map exit, pending type-0 and type-2 events are filtered (SPEC-006 queue clearing). Owners that are party members are kept; the player is always a party member. `[Must-match]` For every other owner:

- A drug stage is **applied immediately** (non-immediate presentation), then discarded. `[Must-match]`
- A withdrawal end event removes its perk effect, then is discarded. An onset event is discarded without effect. `[Must-match]`

### 3.10 Armor Application

Armor contributes by editing the wearer's bonus array. Changing from old armor O to new armor N (either may be absent, which counts as 0 everywhere) does the following, **only when the wearer is the player**:

1. AC bonus += `AC(N) - AC(O)`. `[Must-match]`
2. For each damage type t = 0..6: DR[t] bonus += `DR(N,t) - DR(O,t)`; DT[t] bonus += `DT(N,t) - DT(O,t)`. `[Must-match]`
3. If O exists, remove O's armor-perk effect. Then, if N exists, add N's armor-perk effect. A perk index of -1 does nothing. `[Must-match]`

Raw armor values are added unclamped. Only the effective stat query clamps: DR 0-90 except EMP DR 0-100, DT 0-100, AC ≥ 0 (SPEC-006 §3.1). The corpus stores EMP DR above 100 on every armor (section 4.12), so worn armor gives the full clamp value. `[Must-match]`

Paths that change worn armor:

| Path | Player | NPC |
| --- | --- | --- |
| Inventory screen: drag to or from the armor slot, swap, drop, or drop into a container | Applies section 3.10 | n/a |
| `wield_obj_critter` with armor | Changes art; the opcode then applies section 3.10 | Sets the worn flag only. **No AC/DR/DT/perk change** |
| AI weapon switching (SPEC-009) | n/a | Weapons only |
| Dropping all items on death (worn armor dropped) | Applies removal | No change |

- NPC protection comes only from the prototype base stats and bonus array, never from worn armor. `[Must-match]`
- Taking armor off restores the player's default jumpsuit art (SPEC-005 §3.8). `[Must-match]`
- Armor art: female wearers use the female art field, others the male one. A -1 field falls back to the jumpsuit in the inventory-window path, but to art index 1 in the `wield_obj_critter` path. SPEC-005 §3.8 has the full rules, the corpus values and the tags for each path. `[Must-match]`

### 3.11 Hands, Two-Handed Weapons, and Strength

- There is **no equip-time restriction** for two-handed weapons. A two-handed weapon may sit in one hand while the other hand holds any item. `[Must-match]`
- Two-handedness matters only in these places:
  1. Attack preflight (SPEC-007): both arms crippled refuses any weapon attack. One crippled arm refuses a two-handed weapon. `[Must-match]`
  2. The interface greys out the weapon under the same two arm conditions, for the player. `[Must-match]`
  3. The One Hander trait, player only: -40 hit chance with two-handed weapons, +20 otherwise (SPEC-007). `[Must-match]`
  4. SPEC-009 weapon evaluation rejects weapons under the same arm conditions. `[Must-match]`
- Minimum strength is never an equip restriction. Each point of `min ST - effective ST` above 0 costs -20 hit chance. When it is positive for the player's attack, `combat.msg` 107 is printed. `[Must-match]`
- Dropping ammunition onto a weapon held in a hand reloads that weapon (section 3.15). It does not swap the items. `[Must-match]`
- Opening the inventory during combat (player only) costs `4 - Quick Pockets rank` AP. If that cost is > 0 and exceeds current AP, print `inventry.msg` 19 and refuse to open. Otherwise deduct the cost. `[Must-match]`

### 3.12 Perk Effects (Mechanism)

A perk effect, whether gained as a rank, from armor, or from withdrawal, applies two kinds of change:

1. If the perk has an effect stat, its bonus += the perk's modifier. `[Must-match]`
2. If the perk is **not selectable** (indices 53-63), each SPECIAL bonus additionally += the perk's per-attribute value from the table's requirement columns (section 3.21). `[Must-match]`

Removal subtracts exactly the same amounts. `[Must-match]` A perk index outside 0..63 does nothing. `[Must-match]` Effects stack: applying the same withdrawal perk twice doubles its penalties. `[Must-match]`

### 3.13 Item Stacking

Two items stack (share one inventory entry) only if all of these hold:

- same PID and same script reference;
- neither is equipped (worn or in a hand) or flagged as used/armed;
- the item is not a container;
- neither has inventory contents;
- their 32 bytes of instance data are identical. For ammunition and money, the round/amount word is excluded from the comparison. `[Must-match]`

Weapons with different loaded rounds or ammunition PIDs therefore never stack. `[Must-match]`

An inventory entry holds a quantity Q and one representative object. **Ammunition entries:** the representative's round count R is the front pack; the other Q-1 packs are full (pack size K). Adding an ammunition object with front rounds R' and quantity Q' to an existing entry works like this:

- If `R + R' > K`: the new front is `R + R' - K`, and Q becomes `Q + Q'`, which accounts for one extra full pack. `[Must-match]`
- Otherwise: the new front is `R + R'`, and Q becomes `Q + Q' - 1`. `[Must-match]`
- The added object becomes the representative, and the old representative is destroyed. Total rounds are conserved. `[Must-match]`

For non-ammunition items, `Q += Q'`, and the added object likewise replaces the representative. `[Must-match]` Adding the exact object that is already the representative is a no-op that succeeds. `[Must-match]`

Removing q units:

- Fewer than q units present: fail. `[Must-match]`
- Exactly q: the entry is deleted and later entries shift down. `[Must-match]`
- Otherwise: a copy takes over as the entry's representative, and the removed object is detached. For ammunition, the remaining representative's rounds are set to K. The removed pack keeps the old front count, so a partial pack always leaves first. `[Must-match]`
- Removal searches the top level first. It recurses into a container only while scanning past that container. `[Must-match]`
- Removing an item always clears its equipped flags. `[Must-match]`

### 3.14 Weight, Carry Limit, and Container Capacity

Item weight:

| Item | Weight |
| --- | --- |
| Container | Own weight + total weight of its contents |
| Weapon with L > 0 loaded rounds and a valid ammunition PID | Own weight + `ammo weight × ((L - 1) / K + 1)`, that is, whole packs rounded up |
| Anything else, including an ammunition object | Own weight |

The weight table is `[Must-match]`.

Entry weight = item weight × Q. Ammunition entries therefore count every pack at full weight, even a partial front pack. `[Must-match]` A critter's total weight is the sum of its entries. Equipped items are already entries. While the inventory screen holds an equipped item outside the list, that item is added once. `[Must-match]`

Adding Q items to an owner:

| Owner | Refused when | Result code meaning |
| --- | --- | --- |
| Critter, body type biped | `total + weight × Q > effective carry weight` | Too heavy |
| Critter, any other body type | Always | Wrong body type |
| Container | `current size + size × Q ≥ maximum size`. Also refused if the container's outermost owner is a critter and that critter's carry check fails | Too big / too heavy |
| Misc item | Unless the added PID equals its power-cell PID | Refused |
| Any other item | Always | Refused |
| Map or other owner | Never | — |

- Q < 1 is always refused. `[Must-match]`
- The container test uses **≥** (strict capacity), but the critter test uses **>**. `[Must-match]`
- Container size is the sum of `size × Q` over direct contents only. `[Must-match]`
- Forced transfers skip these checks and can exceed the carry limit. Examples are barter settlement, table returns, and failed-move rollback. `[Must-match]`

### 3.15 Ammunition, Reload, and Unload

- **Compatibility** of weapon W with ammunition A: W is a weapon; A is ammunition; W's prototype caliber equals A's caliber; and, if W has L > 0 loaded rounds, W's current ammunition PID equals A's PID. An empty weapon accepts any matching caliber. `[Must-match]`
- **Reload** W from A: as SPEC-009 §3.13. Transfer `min(capacity - L, R_A)`, set W's ammunition PID to A's PID, and return the rounds left in A. A full weapon returns R_A unchanged. Incompatible pairs return -1. `[Must-match]`
- **Search reload** (combat/interface path): take the first compatible ammunition entry in inventory order. Reload once. If 0 rounds remain, destroy one unit (section 3.13). A full weapon is not reloaded. `[Must-match]` AP costs are SPEC-007 (2 AP).
- **Inventory drag reload:** dropping an A entry of Q > 1 onto W asks for a count n (quantity prompt, 1..Q). Then:
  1. W is lifted out of inventory first. `[Must-match]`
  2. Repeat up to n times: reload; on 0 remaining, destroy that unit and continue with the same entry; stop as soon as a pack is left non-empty. `[Must-match]`
  3. W is re-added, so it may re-stack. If anything loaded, the weapon's ready sound plays. `[Must-match]`
- **Unload:** requires capacity > 0, L > 0, and a valid ammunition PID. Create one new ammunition object of W's ammunition PID holding `min(L, K)` rounds. W keeps `L - min(L, K)`. `[Must-match]`

### 3.16 Item Value

| Item | Value |
| --- | --- |
| Container | Base cost + total value of its contents |
| Weapon with L > 0 and valid ammunition PID | Base cost + `L × ammo cost / K` (one truncating division of the product) |
| Ammunition object | `base cost × R / K` |
| Anything else | Base cost |

The value table is `[Must-match]`.

- Entry value for non-ammunition: value × Q. `[Must-match]`
- Ammunition entry value: `base cost × (Q - 1) + base cost × R / K`. `[Must-match]`
- An owner's total value is the sum of entry values, plus any equipped item the inventory screen holds outside the list. `[Must-match]`
- Money (PID 41) has base cost 1, so caps count at face value. `[Must-match]`
- Total caps of an owner = the quantities of money entries, plus money inside containers, recursively. `[Must-match]`

### 3.17 Barter Session Setup

Barter starts from dialogue, by player choice, `gdialog_barter(modifier)`, or a deferred barter request (SPEC-006 §3.9).

1. The **script barter modifier** M_s is reset to 0 at the start of every dialogue. `gdialog_set_barter_mod(m)` and `gdialog_barter(m)` replace it. `[Must-match]`
2. On entry, the merchant's worn armor is removed from its inventory. Then the merchant's right-hand item is removed; if it has none, its **first weapon** in inventory order is removed. These items are hidden during barter, cannot be bought, and are restored at exit (armor worn again, the right-hand item back in the right hand, the fallback weapon unequipped). `[Must-match]`
3. The reaction modifier M_r comes from the merchant's current reaction value (SPEC-006 §3.10): above 75 gives **-15**; above 25 gives **0**; otherwise **+25**. `[Must-match]`
4. The effective modifier is `M = M_s + M_r`. It is assigned at the top of every input iteration. The very first price shown, before any input, uses M_s alone. `[May-improve]` Safe rule: use `M_s + M_r` from the first frame.
5. The reference's "reaction modifier ≤ -30 ends barter" test can never fire, since M_r ≥ -15. `[Must-match]`

### 3.18 Merchant Price Formula

Let T be the merchant's offer table, V_T its total value (section 3.16, caps included), and C_T its total caps. Let B_p be the effective Barter skill of the player-side inventory owner, and B_m the merchant's effective Barter skill (SPEC-006 §3.3; player-only skill adjustments apply only to the player). Let MT be 25 if the player-side owner is the player and has Master Trader, otherwise 0. Evaluate in IEEE-754 binary64, in this order:

1. `g = V_T - C_T` (integer value of the non-money goods). `[Must-match]`
2. `m = (M + 100 - MT) × 0.01`. If `m < 0`, set `m = 0.0099999998`. `[Must-match]`
3. `r = (160 + B_m) / (160 + B_p) × (2 × g)`. `[Must-match]`
4. **Price** = `trunc(m × r + C_T)`. `[Must-match]`

- Caps on the merchant table are neither marked up nor discounted. Goods cost twice their value at equal skills and modifier 0. `[Must-match]`
- There is no party-wide "best barterer" selection, no separate Charisma roll, and no random draw. Charisma enters only through the Barter skill base `20 + 2 × Charisma`. Speaker (+20 Barter) and Good Natured (+15 Barter) enter through the skill. `[Must-match]`
- The player's own offer is valued at plain V_P, with no markdown and no skill factor. `[Must-match]`

### 3.19 Transaction Acceptance

Pressing the offer key while either table is non-empty attempts the trade:

1. Refuse if the player's offer table P is **empty**. `[Must-match]`
2. Refuse if any item in P, recursively including container contents, is flagged as used/armed (for example a primed explosive). `[Must-match]`
3. Refuse if `Price(T) > V_P`. `[Must-match]`
4. Otherwise move all of T to the player-side owner and all of P to the merchant. Print `inventry.msg` 27. A refusal prints 28. `[Must-match]`

- An empty merchant table has price 0, so any non-empty, unarmed player offer is accepted as a gift. `[Must-match]`
- Transfers here bypass carry limits. `[Must-match]`
- Leaving barter returns T to the merchant and P to the player. `[Must-match]`

### 3.20 Experience and Level-Up

Total experience required to leave level L (1 ≤ L ≤ 20) is `1000 × L × (L + 1) / 2`:

| Level reached | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Total XP | 1000 | 3000 | 6000 | 10000 | 15000 | 21000 | 28000 | 36000 | 45000 | 55000 | 66000 |

| Level reached | 13 | 14 | 15 | 16 | 17 | 18 | 19 | 20 | 21 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Total XP | 78000 | 91000 | 105000 | 120000 | 136000 | 153000 | 171000 | 190000 | 210000 |

The maximum level is 21. At level 21 the threshold query returns -1, and the loop below stops on the level test first. `[Must-match]`

Adding experience x, from `give_exp_points`, combat kill awards (SPEC-007), or skill/quest awards:

1. `x' = x + trunc(Swift Learner rank × 5 × x / 100)`. The product is formed first, so negative x truncates toward zero. `[Must-match]`
2. New XP = clamp(old XP + x', 0, 2147483647). `[Must-match]`
3. While level < 21 and XP ≥ threshold(level): increase the level by 1 and, for each step:
   - print `stat.msg` 600;
   - set the pending-level-up flag `0x00000008`;
   - play sound `levelup`;
   - compute `h = trunc(R(Endurance) / 2) + 2 + 4 × Lifegiver rank`;
   - heal h HP, capped at the **old** maximum;
   - add h to the maximum-HP bonus. `[Must-match]`
4. Levels never decrease, even when XP falls below the current threshold. `[Must-match]`

Setting XP directly through the player-stat setter re-runs step 3 with x = 0. `[Must-match]` The reference's player-stat setter has an always-false index guard, so an out-of-range index is not rejected. `[May-improve]` Safe rule: reject indices outside 0-4.

### 3.21 Skill Points and Perk Timing (Character Screen)

Skill points and perk picks are **not** granted at the moment of level-up. They are reconciled when the player next opens the character screen outside character creation. `[Must-match]` Let `last` be the stored last-reconciled level (component 19; it starts at 1). If level ≠ last and level ≤ 21, then for each `n = last+1 .. level`:

1. `sp = unspent + 5 + 2 × R(Intelligence) + 2 × Educated rank`. If Gifted, subtract 5 and floor at 0. Then cap at **99**; points above 99 are lost. `[Must-match]` Skilled adds no points. `[Must-match]`
2. If fewer than 7 distinct perks have rank > 0, and `n mod I = 0` (I = 3, or 4 with Skilled), set the pending-perk byte. `[Must-match]` The byte is a flag, not a counter: crossing two perk levels in one reconciliation grants **one** pick. `[Must-match]`

Then, if the pending-perk byte is set, the perk chooser opens. It lists every perk that passes eligibility (below), in display-name order. Choosing one adds a rank and clears the byte. Closing the chooser without a choice keeps the byte for the next visit. `[Must-match]` Finally `last = level`. Cancelling the whole character screen restores the saved progression, `last`, and the pending byte, so the reconciliation repeats on the next visit. `[Must-match]`

Immediate extras when a perk rank is gained in the chooser:

| Perk | Immediate extra |
| --- | --- |
| Tag! | Opens the fourth-tag selection (SPEC-014 §3.42). If cancelled, the rank is removed again, and the pending pick is still consumed |
| Mutate! | Opens trait replacement (SPEC-014 §3.42). If cancelled, the rank is removed again, and the pending pick is still consumed |
| Lifegiver | Maximum-HP bonus +4 and heal 4 |
| Educated | Unspent skill points +2, without the 99 cap of step 1 but within the stat bounds |

Only the first matching row applies. `[Must-match]`

**Eligibility** of perk p for selection:

- p is in 0..52, i.e. selectable;
- rank(p) < max rank;
- player level ≥ minimum level;
- if p has a skill requirement, the effective player skill ≥ that percentage;
- each effective SPECIAL (SPEC-006 E(X), including drug and armor bonuses) ≥ the listed minimum. `[Must-match]`

Temporary bonuses, such as a drug's Intelligence boost, can therefore make a perk eligible. `[Must-match]` Adding a rank by script (`critter_add_trait` kind 0, SPEC-005) uses the same eligibility test and fails silently when it does not pass. `[Must-match]`

### 3.22 Perk Table

Requirement columns list effective minimums. "-" means none. Ranks are maximum ranks. The stat effect is applied per rank through section 3.12.

| # | Perk | Max rank | Min level | Requirements | Stat effect per rank |
| ---: | --- | ---: | ---: | --- | --- |
| 0 | Awareness | 1 | 3 | PE 5 | - |
| 1 | Bonus HtH Attacks | 1 | 6 | AG 6 | - |
| 2 | Bonus HtH Damage | 3 | 3 | ST 6, AG 6 | +2 melee damage |
| 3 | Bonus Move | 3 | 6 | AG 5 | - |
| 4 | Bonus Ranged Damage | 2 | 6 | AG 6, LK 6 | - |
| 5 | Bonus Rate of Fire | 1 | 9 | PE 6, IN 6, AG 7 | - |
| 6 | Earlier Sequence | 3 | 3 | PE 6 | +2 sequence |
| 7 | Faster Healing | 3 | 3 | EN 6 | +1 healing rate |
| 8 | More Criticals | 3 | 6 | LK 6 | +5 critical chance |
| 9 | Night Vision | 3 | 3 | PE 6 | - |
| 10 | Presence | 3 | 3 | CH 6 | - |
| 11 | Rad Resistance | 3 | 6 | EN 6, IN 4 | +15 radiation resistance |
| 12 | Toughness | 3 | 3 | EN 6, LK 6 | +10 normal DR |
| 13 | Strong Back | 3 | 3 | ST 6, EN 6 | +50 carry weight |
| 14 | Sharpshooter | 2 | 6 | PE 7, IN 6 | - |
| 15 | Silent Running | 1 | 6 | AG 6, Sneak 50% | - |
| 16 | Survivalist | 3 | 3 | EN 6, IN 6, Outdoorsman 40% | - |
| 17 | Master Trader | 1 | 9 | CH 7, Barter 60% | - |
| 18 | Educated | 3 | 6 | IN 6 | - |
| 19 | Healer | 3 | 3 | PE 7, IN 5, AG 6, First Aid 40% | - |
| 20 | Fortune Finder | 1 | 6 | LK 8 | - |
| 21 | Better Criticals | 1 | 9 | PE 6, AG 4, LK 6 | +20 better-critical modifier |
| 22 | Empathy | 1 | 6 | PE 7, IN 5 | - |
| 23 | Slayer | 1 | 18 | ST 8, AG 8, Unarmed 80% | - |
| 24 | Sniper | 1 | 18 | PE 8, AG 8, Small Guns 80% | - |
| 25 | Silent Death | 1 | 18 | AG 10, Sneak 80% | - |
| 26 | Action Boy | 3 | 12 | AG 5 | +1 maximum AP |
| 27 | Mental Block | 1 | 15 | - | - |
| 28 | Lifegiver | 3 | 12 | EN 4 | - |
| 29 | Dodger | 2 | 9 | AG 4 | +5 AC |
| 30 | Snakeater | 1 | 6 | EN 3 | +25 poison resistance |
| 31 | Mr. Fixit | 1 | 12 | - | - |
| 32 | Medic | 1 | 12 | - | - |
| 33 | Master Thief | 1 | 12 | - | - |
| 34 | Speaker | 1 | 12 | - | - |
| 35 | Heave Ho! | 3 | 6 | - | - |
| 36 | Friendly Foe | 1 | 6 | PE 4 | - |
| 37 | Pickpocket | 1 | 9 | AG 8, Sneak 80% | - |
| 38 | Ghost | 1 | 6 | Sneak 60% | - |
| 39 | Cult of Personality | 1 | 12 | CH 10 | - |
| 40 | Scrounger | 1 | 9 | LK 8 | - |
| 41 | Explorer | 1 | 9 | - | - |
| 42 | Flower Child | 1 | 9 | EN 5 | - |
| 43 | Pathfinder | 2 | 6 | EN 6, Outdoorsman 40% | - |
| 44 | Animal Friend | 1 | 9 | IN 5, Outdoorsman 25% | - |
| 45 | Scout | 1 | 3 | PE 8 | - |
| 46 | Mysterious Stranger | 1 | 6 | LK 7 | - |
| 47 | Ranger | 3 | 6 | PE 6 | - |
| 48 | Quick Pockets | 3 | 3 | AG 5 | - |
| 49 | Smooth Talker | 3 | 3 | IN 4 | - |
| 50 | Swift Learner | 3 | 3 | IN 4 | - |
| 51 | Tag! | 1 | 12 | - | - |
| 52 | Mutate! | 1 | 9 | - | - |

The table is `[Must-match]`, including these data quirks:

- Pickpocket requires **Sneak**, not Steal. `[Must-match]`
- Several level-12 perks have no attribute requirement. `[Must-match]`
- The table is engine data; `perk.msg` supplies only names (101 + index) and descriptions (201 + index). `[Must-match]`

Not-selectable effect perks (always ineligible; applied by drugs, armor, or weapons):

| # | Perk | Effect when applied |
| ---: | --- | --- |
| 53 | Nuka-Cola Craving | None (description only) |
| 54 | Buffout Craving | ST -2, EN -2, AG -3 |
| 55 | Mentats Craving | IN -3, AG -2 |
| 56 | Psycho Craving | IN -2 |
| 57 | Radaway Craving | Radiation resistance -20 |
| 58-61 | Weapon Long Range, Weapon Accurate, Weapon Penetrate, Weapon Knockback | None through this mechanism. Weapon perk selectors are read by SPEC-007 attack rules |
| 62 | Powered Armor | Radiation resistance +30, ST +3 |
| 63 | Combat Armor | Radiation resistance +20 |

These effects are `[Must-match]`.

### 3.23 Behavioral Perk Effects

Effects that are not stat bonuses apply **only to the player** unless stated otherwise, and test the rank as below. Detailed formulas belong to the cited specifications.

| Perk | Engine effect |
| --- | --- |
| Awareness | Examining a living non-player critter shows its current/maximum HP (`proto.msg` 535/536 by gender for bipeds, 537 otherwise) and its wielded weapon and ammunition |
| Bonus HtH Attacks | Melee/unarmed attack AP -1, before aiming, floor 1 (SPEC-007) |
| Bonus Rate of Fire | Ranged attack AP -1, before aiming, floor 1 |
| Bonus Move | `2 × rank` free movement points at the start of each player turn |
| Bonus Ranged Damage | `+2 × rank` damage term for ranged attacks (SPEC-007) |
| Sharpshooter | Effective range distance reduced by `2 × rank` hexes in the hit-chance formula |
| Heave Ho! | Thrown-weapon range cap `3 × (ST + 2 × rank)` instead of `3 × ST` |
| Slayer | A melee/unarmed **success** becomes a critical success |
| Sniper | A ranged success becomes critical if `roll(1, 10) ≤ effective Luck` (one draw, only on success) |
| Silent Death | Melee/unarmed success while sneaking, not hit from the front, and not the target's last attacker: damage multiplier 4 instead of 2 |
| Silent Running | Sneaking does not force walking |
| Night Vision | Ambient light + `6553 × rank` before the light clamp |
| Presence | NPC starting reaction `+10 × rank` |
| Cult of Personality | NPC starting reaction + \|reputation\| |
| Empathy | Dialogue options are colored by their good/neutral/bad reaction tag (SPEC-006) |
| Friendly Foe | Combat outlines of critters on the player's team use the friendly outline type instead of hostile |
| Healer | First Aid heals `roll(1 + 2×rank, 5 + 5×rank)`; Doctor uses `roll(4 + 2×rank, 10 + 5×rank)` |
| Medic / Mr. Fixit / Master Thief / Speaker / Ghost | Skill bonuses per SPEC-006 §3.3 (presence test, not rank-scaled) |
| Pickpocket | Steal ignores the item-size penalty and the facing penalty |
| Quick Pockets | Inventory-open AP `4 - rank` (section 3.11) |
| Master Trader | Barter modifier -25 (section 3.18) |
| Educated / Lifegiver | Sections 3.20-3.21 |
| Swift Learner | XP +5% per rank (section 3.20) |
| Smooth Talker | +rank to the dialogue IQ test, unclamped (SPEC-006 §3.10) |
| Flower Child | Halves the addiction chance and the withdrawal duration |
| Explorer | Special-encounter test `+2 × rank` (SPEC-008) |
| Scout | Larger world-map reveal pattern (SPEC-008) |
| Pathfinder | World-map travel time × `(1 - 0.25 × rank)` (SPEC-008) |
| Tag! / Mutate! | Section 3.21 |

These effects are `[Must-match]`. Survivalist, Fortune Finder, Mental Block, Scrounger, Mysterious Stranger, Ranger, and Animal Friend have **no engine-side effect** in this revision. Scripts can only query them with `has_trait`. `[Must-match]`

## 4. Synthetic Test Fixtures (Hex / Text)

All fixtures use synthetic data and explicit draws. "Tick T" is the consumption time. All hex is big-endian.

### 4.1 Fixture A: Drug Payload and Timeline

Synthetic drug, PID `0x00000400`, payload (68 bytes, starting at file `0x39`):

```text
00000004 00000005 FFFFFFFF   selectors: IN, AG, unused
00000002 00000001 00000000   immediate: +2, +1, 0
0000003C                     D1 = 60 minutes
FFFFFFFF FFFFFFFF 00000000   stage 1: -1, -1, 0
00000078                     D2 = 120 minutes
FFFFFFFF 00000000 00000000   stage 2: -1, 0, 0
00000014 00000037 000005A0   addiction 20%, withdrawal perk 55, onset 1440 minutes
```

Player, no traits, IN/AG bonuses 0, campaign variable 0x400 = 0. Draws given: addiction draw 57.

| Tick | Event | IN bonus | AG bonus | Messages |
| --- | --- | ---: | ---: | --- |
| T | Immediate stage | +2 | +1 | `item.msg` 1 for IN and for AG |
| T | Addiction: draw 57 > 20 | +2 | +1 | — |
| T + 36000 | Stage 1 | +1 | 0 | `item.msg` 2 for IN and for AG |
| T + 72000 | Stage 2 | 0 | 0 | `item.msg` 2 for IN |

Variant A2, Chem Resistant player: stage ticks become T+18000 and T+36000, and P = 10. Variant A3, Chem Reliant plus Chem Resistant: P = 20 × 2 = 40, then 40 / 2 = 20; delays are unchanged. Variant A4, Flower Child plus Chem Resistant: P = 20 / 2 = 10, then 10 / 2 = 5, and delays are halved.

### 4.2 Fixture B: Random-Range Healing Drug

Selectors `FFFFFFFE 00000023 FFFFFFFF` (S0 = -2, slot 1 = current HP). Immediate amounts `0000000A 00000014 00000000`. Both delayed stages are all-zero (not scheduled). Addiction 0%.

NPC with HP 15 / 41 and the fleeing bit set. Draws: range 14, addiction 88.

1. The fleeing bit is cleared. HP 15 + `roll(10, 20)` = 15 + 14 = **29**. One draw.
2. The synthetic PID is outside the addiction table, so the test reads "not addicted". `roll(1, 100)` = 88 > 0, so there is no withdrawal. The second draw is consumed even at 0%.
3. Result 1. Total draws: 2.

Variant B2: HP 39 / 41 with range draw 14 gives HP **41** (capped). Variant B3: the same payload with selectors `FFFFFFFE FFFFFFFF 00000023` (S1 unused). The range draw moves to slot 2, so HP rises by `roll(10, 20)`, not by A2 = 0.

### 4.3 Fixture C: Addiction and Withdrawal Timeline

Fixture A drug, player, no traits, addiction draw 7 (≤ 20).

| Tick | Event |
| --- | --- |
| T | Onset event queued for T + 864000. Mapped variable: PID `0x400` is not in the table, so none is set. Addicted flag `0x10` set |
| T + 864000 | Onset: perk 55 effect (IN -3, AG -2); `perk.msg` 256 printed; end event queued for T + 864000 + 6048000; queue pass stops |
| T + 6912000 | End: effect removed (IN +3, AG +2); `item.msg` 3; queue pass stops; flag cleared if no other addiction reads as set |

Variant C2, Chem Reliant plus Flower Child: withdrawal lasts `10080 / 2 / 2 = 2520` minutes = 1512000 ticks.

### 4.4 Fixture D: Re-Dose During Withdrawal

The Fixture C state at T + 1000000, with the end event pending. The player takes another dose of the same drug, which has onset 1440:

1. Re-dose scan: the first matching withdrawal event is an end event. Remove perk 55's effect now (IN +3, AG +2) and print `item.msg` 3. Remove the event and queue an onset event at T + 1000000 + 864000.
2. Immediate stage +2 / +1, then the delayed stages are scheduled.
3. PID `0x400` is outside the addiction table, so the test reads "not addicted" and the addiction draw is consumed. A draw of 15 ≤ 20 queues a **second** onset event, also at T + 1864000.
4. At T + 1864000 both onsets fire: perk 55 is applied twice (IN -6, AG -4 in total), and each schedules its own end event.

For a table PID such as 87 after a player addiction, the reference reads campaign variable 87, which is normally 0. It keeps rolling and can stack onsets. The `[May-improve]` safe rule reads variable 190 = 1 and skips the roll.

### 4.5 Fixture E: Weight, Value, Stacking

Synthetic ammunition X: weight 1, cost 50, pack size K = 20, caliber 7. Synthetic weapon W: weight 5, cost 300, capacity 12, caliber 7, loaded 7 rounds of X.

| Quantity | Value |
| --- | ---: |
| Weight(W) | `5 + 1 × ((7 - 1) / 20 + 1)` = **6** |
| Value(W) | `300 + 7 × 50 / 20` = 300 + 17 = **317** |
| Entry X: Q = 3, front R = 5, weight | `1 × 3` = **3** |
| Entry X: value | `50 × 2 + 50 × 5 / 20` = 100 + 12 = **112** |

Merge: add an X object with R' = 18, Q' = 1. Since 5 + 18 = 23 > 20, the new front is 3 and Q = 3 + 1 = 4. Rounds before: 5 + 2×20 + 18 = 63. Rounds after: 3 + 3×20 = 63.

Search reload of W (7/12) from that entry: transfer min(5, 3) = 3, so W = 10/12. The front pack is empty, so one unit is destroyed, leaving Q = 3 with front reset to 20.

Carry: biped with effective carry weight 150 and total 148. Adding an item of weight 3 is refused (151 > 150). Weight 2 is accepted (150 is not > 150).

Container: maximum size 10, contents size 7. Adding size 3 is refused (10 ≥ 10). Size 2 is accepted.

Unload W at 10/12: new X object with min(10, 20) = 10 rounds, and W = 0.

### 4.6 Fixture F: Barter Prices

Merchant table: goods value 200 plus 30 caps (V_T = 230, C_T = 30, g = 200). B_p = 50, B_m = 40.

| Case | M_s | Reaction | MT | M | m | Price |
| --- | ---: | --- | ---: | ---: | --- | ---: |
| F1 | 0 | 50 (neutral, 0) | 0 | 0 | 1.00 | `trunc(1.00 × 380.952… + 30)` = **410** |
| F2 | 10 | 80 (good, -15) | 25 | -5 | 0.70 | `trunc(266.666… + 30)` = **296** |
| F2, first frame | 10 | (not yet applied) | 25 | 10 | 0.85 | **353** |
| F3 | 0 | 10 (bad, +25) | 0 | 25 | 1.25 | **506** |
| F4 | -200 | neutral | 0 | -200 | clamped 0.0099999998 | **33** |
| F5 | any | any | any | — | — | Empty merchant table, so **0** |

Acceptance for F1: a player table worth 410 is accepted, and one worth 409 is refused (`inventry.msg` 28). A player table holding a primed explosive is refused regardless of value. An empty player table is refused.

Equal-skill check: B_p = B_m = 100, g = 50, M = 0, no caps: price = **100**, twice the value.

### 4.7 Fixture G: Experience and Level-Up

Player at level 1, XP 0, Swift Learner rank 1, R(EN) = 6, R(IN) = 7, Lifegiver 0, Educated 0, no Gifted or Skilled. Unspent points 0, last = 1. Max HP 40 (bonus 0), current HP 40.

1. `give_exp_points(2900)`: x' = 2900 + trunc(1×5×2900/100) = 2900 + 145 = **3045**.
2. 3045 ≥ 1000, so level 2: heal 5 (capped at 40) and max-HP bonus +5 (max 45). 3045 ≥ 3000, so level 3: heal 5 (HP 40 → 45, capped at the old max 45) and bonus +5 (max 50). 3045 < 6000, so stop. Final HP 45 / 50, level 3, flag `0x08` set, `stat.msg` 600 printed twice.
3. Character screen opens. For n = 2 and n = 3, sp += 5 + 14 = 19, so 38 in total. Since 3 mod 3 = 0, the pending-perk byte is set and the chooser opens. last = 3.

Negative award: `give_exp_points(-510)` from XP 3045: x' = -510 + trunc(-2550/100) = -510 - 25 = -535. XP becomes 2510 and the level stays 3.

Cap: unspent 90 + 19 = 109 is capped to **99**.

Single-pick quirk: last = 2, level jumps to 7, no Skilled. Levels 3 and 6 both qualify, but the byte grants **one** pick. With Skilled, the qualifying levels would be 4 only (n = 3..7).

### 4.8 Fixture H: Perk Eligibility

Player at level 6. Effective SPECIAL ST 5, PE 7, EN 5, CH 5, IN 6, AG 6, LK 5. Sneak 55%, Outdoorsman 45%, Barter 58%. No ranks held.

| Perk | Eligible? | Reason |
| --- | --- | --- |
| Awareness | Yes | Level 6 ≥ 3, PE 7 ≥ 5 |
| Silent Running | Yes | AG 6, Sneak 55 ≥ 50 |
| Sharpshooter | Yes | PE 7, IN 6 |
| Survivalist | No | EN 5 < 6 |
| Ghost | No | Sneak 55 < 60 |
| Master Trader | No | Level 6 < 9 |
| Pickpocket | No | Level 6 < 9 (and Sneak 55 < 80) |
| Educated | Yes | IN 6 |
| Nuka-Cola Craving | No | Not selectable |

Variant: with effective IN 5, Sharpshooter and Educated both fail. While a drug's IN +1 bonus is active, both become eligible (section 3.21).

### 4.9 Fixture I: Armor Swap (Player)

Bonus array starts at zero. Armor A: AC 20, DR normal 30, DR EMP 500, DT normal 4, perk -1. Armor B: AC 25, DR normal 40, DR EMP 500, DT normal 12, perk 62.

| Step | AC bonus | DR normal bonus | DR EMP bonus | DT normal bonus | ST bonus | Rad res bonus |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Wear A | 20 | 30 | 500 | 4 | 0 | 0 |
| Swap to B | 25 | 40 | 500 | 12 | 3 | 30 |
| Remove B | 0 | 0 | 0 | 0 | 0 | 0 |

With B worn, the effective EMP DR query returns **100** (clamped) and normal DR returns 40 + base. The ST +3 bonus recomputes derived stats.

NPC variant: `wield_obj_critter(B, npc)` sets B worn and changes nothing in the NPC's bonus array.

### 4.10 Fixture J: SPEC-009 Healing Hook Trace

This extends SPEC-009 Fixture I with the Fixture B drug standing in for the stimpak. The NPC biped has max HP 41 (B = 20), HP 15, and AP 5. Inventory: [Fixture B drug ×2]. Draws: 6 (range), 90 (addiction).

1. 15 < 20 and AP ≥ 2. Remove one unit and consume it: HP 15 + roll(10,20) = 15 + 6 = 21, then the addiction draw 90. Result 1. Hands animation, `misc.msg` 5000, unit destroyed, AP 5 → 3.
2. 21 ≥ 20, so stop. Draws consumed: 2.

Failure variant: the NPC is robotic. Consumption returns -1 with no draws; SPEC-009 already excludes non-bipeds before this point. Dead-critter variant: the same -1, but unreachable because the AI never runs for dead critters.

### 4.11 Fixture K: Map Exit

A non-party NPC has a pending stage-1 event (AG -1) and a pending withdrawal end event (perk 56). On map exit the AG bonus becomes -1 immediately, the perk 56 effect is removed (IN +2), and both events are gone. The same events owned by the player or a party member remain queued.

### 4.12 Corpus Checks

Run `python3 -I tools/spec011/survey.py <unpacked corpus>` (a research-side tool that does not cross the wall). On the locally owned corpus it reported **ALL PASS**:

- 242 item prototypes by subtype: 9 armor, 77 container, 14 drug, 43 weapon, 16 ammunition, 77 misc, 6 key. Every subtype is 0-6, and every file's length equals the SPEC-003 reader size.
- All 14 drugs use selectors -1, 0-37, or -2 in slot 0 only. Two drugs use the -2 form, both with current HP in slot 1. 18 delayed stages carry nonzero amounts, and none has a delay with all-zero amounts.
- Addiction chances are within 0-100. Every addictive drug names withdrawal perk 53-57. All seven addiction-table PIDs are drugs. The two alcohol PIDs (124, 125) have 0% addiction, so variable 194 is never set from prototypes.
- PIDs 40 and 144 are drugs with current HP among their selectors. Money PID 41 has cost 1.
- Armor perk selectors are -1, 62, or 63. Non-EMP DR is within 0-90, EMP DR is above 100 on every armor, and DT is within 0-100.
- 16 of 43 weapons carry the two-handed flag. All 26 weapons with capacity > 0 have a caliber matched by some ammunition prototype. Every ammunition pack size is > 0, so the value and weight divisions are safe. Weapon minimum strength is within 0-10.
- `perk.msg` defines names 101-164 and descriptions 201-264.

## 5. Clean-Room Verification Checklist
- [x] Zero C/C++ code snippets or struct definitions included
- [x] All multi-byte integer endianness explicitly stated
- [x] Synthetic test vectors included
- [x] Every documented quirk tagged `[Must-match]` or `[May-improve]`
- [x] No internal reference symbol names; only public SSL opcode names and file-format key literals
- [x] Spec linter run against `fallout1-re` symbols with no findings

Verification notes: `tools/spec_lint.py` gives 0 errors. Its 43 plain-word warnings were reviewed and are benign. They are this spec's own formula notation: "roll" (the SPEC-006 random-integer notation), "trunc", "rank", "level", "bonus", "amount", "modifier", "weight", "size", "current", "maximum" and "capacity". The only fenced block is a big-endian hex dump of a synthetic payload. The corpus survey (section 4.12) passes every check. Tag count: 159 `[Must-match]`, 8 `[May-improve]`.

Update notes (2026-10-22): section 3.21 now says that cancelling Tag! or Mutate! still consumes the pending perk pick (SPEC-014 §3.42). Tag counts are unchanged.
