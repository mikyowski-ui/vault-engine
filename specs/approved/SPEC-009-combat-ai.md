# SPEC-009: Combat AI Packets, Threat Selection, and NPC Turn Behavior

## Clean-Room Rules for Research Agents
- Document ONLY binary file layouts (byte offsets, endianness, data types), mathematical formulas, state transitions, and observable behavior.
- DO NOT copy C/C++ code, struct definitions, function names, internal variable names, or implementation-source paths from the inspected implementation.
- Provide small synthetic hex/text test vectors at the bottom of the specification.

## 1. Overview & Scope

This specification describes revision `ac9784b0735468d57ddb2025b9ca76a43a404f4c`, targeting the 32-bit Windows engine. Evidence is static inspection of the AI-packet loader, the generic INI-style configuration reader it uses, NPC combat-turn decision logic, taunt presentation, and the combat/damage/script paths that read or write AI-relevant state. A structural (non-content) audit of a locally owned `data/ai.txt` confirmed the grammar and ordering assumptions below. No original packet values or taunt texts are reproduced. Synthetic checks use independent state models, not a live game session.

This document covers:

- `data/ai.txt` text grammar, all 25 packet keys, and packet resolution;
- the combat-wide critter snapshot the AI queries;
- perception-gated threat ("danger source") selection and the last-attacker reference;
- the complete NPC turn decision tree: fleeing, healing, weapon evaluation/switching/pickup, hit-mode and called-shot choice, the bounded attack loop, reloading, and party-member following;
- join/stop predicates, team switching, critical-failure random retargeting, and onlooker alerting;
- combat taunts (`combatai.msg`) and their floating-text presentation;
- the public script opcodes that read or write AI state.

Relationship to other specifications: SPEC-002 defines `.MSG` parsing, fonts, and the text-color word. SPEC-003 defines critter prototype/map fields that store the AI packet number and team, hex geometry, the multi-step directional operation, and hex direction. SPEC-005 defines opcode stack contracts. SPEC-006 defines effective stats/skills, the random generator, stat checks, and NPC reaction. SPEC-007 defines the combat round/turn loop, AP costs, attack preflight, hit chance, perception sectors, route search, and damage application. SPEC-008 records that the combat-AI save component has no bytes. This document does not restate those rules; it references them and adds AI-specific behavior.

**Compatibility tags.** Every documented quirk carries one tag:

- `[Must-match]`: gameplay-visible behavior, data grammar, formula, ordering, or random-draw sequence. A clean-room engine must reproduce it exactly.
- `[May-improve]`: reference memory-safety defects, undefined behavior, uninitialized reads, stale references, or unspecified library tie-breaking. A clean-room engine must not replicate the undefined behavior. It must return an explicit error or apply the stated safe rule, and it should keep the observable defined behavior.

Terminology used in this document (descriptive, not reference identifiers):

| Term | Meaning |
| --- | --- |
| Packet | One `ai.txt` section; a numbered set of AI tuning values |
| Packet table | All loaded packets, in loader order (section 3.1) |
| Snapshot | The combat-wide list of critters captured at combat start (section 3.2) |
| Last attacker | A critter's single stored reference to the critter it currently considers its opponent; SPEC-007 calls it "last opponent" |
| Intent bits | A critter's combat bit field: `0x01` alert, `0x02` stop attacking, `0x04` fleeing (same bits as SPEC-005 `critter_is_fleeing` / `critter_stop_attacking`) |
| Damage flags | A critter's persistent damage-result bit field (SPEC-007) |
| Effective X | Stat/skill value after all SPEC-006 adjustments |
| `dist(a,b)` | SPEC-007 object distance: hex distance minus one per multihex participant, clamped at 0 |
| Player | The player-controlled critter |

## 2. Binary Layout / Data Format (Byte Offset Tables)

### 2.1 Numeric Conventions and Stored AI Fields

`ai.txt` and `combatai.msg` are byte-oriented text. They contain no multi-byte binary integers, so endianness does not apply to them. All numeric values parsed from them become signed 32-bit integers.

AI state stored in binary resources uses the SPEC-003 big-endian 32-bit encoding:

| Resource | Offset | Size | Endianness | Meaning |
| --- | --- | --- | --- | --- |
| Critter prototype | `0x24` | 4 | Big-endian signed 32-bit | Default AI packet number |
| Map/save critter object | `0x58` | 4 | Big-endian signed 32-bit | Damage received last turn |
| Map/save critter object | `0x5C` | 4 | Big-endian 32-bit | Intent bits (below) |
| Map/save critter object | `0x60` | 4 | Big-endian signed 32-bit | Current AP |
| Map/save critter object | `0x64` | 4 | Big-endian 32-bit | Damage flags (below) |
| Map/save critter object | `0x68` | 4 | Big-endian signed 32-bit | Instance AI packet number (the value the AI uses) |
| Map/save critter object | `0x6C` | 4 | Big-endian signed 32-bit | Team number |
| Map/save critter object | `0x70` | 4 | Big-endian signed 32-bit | Last-attacker combat identifier, or -1 for none |
| `SAVE.DAT` component 12 | n/a | 0 | n/a | Combat-AI component; writes and reads **no bytes** (SPEC-008) `[Must-match]` |

The packet table, snapshot, and taunt buffers are runtime-only. None of them is serialized. `[Must-match]`

Intent bits (32-bit word, bit 0 least significant):

| Mask | Meaning | Set by | Cleared by |
| --- | --- | --- | --- |
| `0x01` | Alert / engaged | Onlooker notification, `attack` opcode in combat, combat start (retained) | Combat removal to waiting partition (SPEC-007), combat cleanup |
| `0x02` | Stop attacking | Flee refusal, max-distance rule, `critter_stop_attacking` | Combat cleanup |
| `0x04` | Fleeing | Flee start, `critter_set_flee_state(1)` | `critter_set_flee_state(0)`, combat cleanup |

Damage-flag masks consulted by the AI:

| Mask | Meaning | Mask | Meaning |
| --- | --- | --- | --- |
| `0x00000001` | Knocked out | `0x00000040` | Blinded |
| `0x00000002` | Knocked down | `0x00000080` | Dead |
| `0x00000004` | Crippled left leg | `0x00000100` | Hit (attack result) |
| `0x00000008` | Crippled right leg | `0x00000200` | Critical (attack result) |
| `0x00000010` | Crippled left arm | `0x00008000` | Lose next turn |
| `0x00000020` | Crippled right arm | `0x00100000` | Random-hit critical failure |

Enumerations used below:

| Domain | Values |
| --- | --- |
| Hit location | 0 head, 1 left arm, 2 right arm, 3 torso, 4 right leg, 5 left leg, 6 eyes, 7 groin, 8 uncalled |
| Hit mode | 0 left primary, 1 left secondary, 2 right primary, 3 right secondary, 4 punch, 5 kick, 6 left reload, 7 right reload |
| Attack subtype | 0 none, 1 unarmed, 2 melee, 3 throw, 4 ranged |
| Attack-mode nibble to subtype | 0 none; 1, 2 unarmed; 3, 4 melee; 5 throw; 6, 7, 8 ranged |
| Attack preflight result | 0 OK, 1 no ammo, 2 out of range, 3 not enough AP, 4 target already dead, 5 aim blocked, 6 one arm crippled with two-handed weapon, 7 both arms crippled |
| Body type | 0 biped, 1 quadruped, 2 robotic |
| Item type | 0 armor, 1 container, 2 drug, 3 weapon, 4 ammo, 5 misc, 6 key |
| Combat difficulty (preference) | 0 easy, 1 normal, 2 hard |

Attack subtype for a weapon and hit mode comes from the low nibble of the item's extended-flags word for primary modes (0, 2), or the next nibble for secondary modes (1, 3). A null weapon has subtype 1 (unarmed). `[Must-match]` Nibble values 9-15 index past the reference's 9-entry table. `[May-improve]`: treat 9-15 as subtype 0 (none) and emit a diagnostic.

Prototype IDs with AI meaning: Stimpak `0x00000028` (40), Super Stimpak `0x00000090` (144). `[Must-match]`

### 2.2 `data/ai.txt`: Line and Section Grammar

The logical path is `data\ai.txt`, resolved through the virtual filesystem (SPEC-008 precedence). The file is read in binary mode, so CR bytes are retained until whitespace trimming. `[Must-match]`

Line reading:

1. Read lines into a 256-byte buffer: at most 255 data bytes per read, stopping after an LF. A physical line longer than 255 bytes is processed as several consecutive "lines", with the split at byte 255. `[Must-match]`
2. Truncate the line at its first `;`, wherever it appears, including inside a value. There is no quoting or escaping, and `#` is not a comment marker. `[Must-match]`
3. **Section test:** if the truncated line contains `[`, and a `]` appears after that `[`, the bytes strictly between them become the current section name after trimming. Bytes before `[` and after `]` are ignored. The line is then finished, even if it also contains `=`. `[Must-match]` Example: `color=[x]` selects section `x` and assigns nothing.
4. **Key/value test:** otherwise, if the line contains `=`, the key is the bytes before the first `=` and the value is everything after it, both trimmed. A `[` without a later `]` stays part of the key or value text. Lines without `=` are ignored. `[Must-match]`
5. Trimming removes leading and trailing ASCII whitespace bytes: `0x20` and `0x09`-`0x0D`. Interior spaces are kept, so section names such as `Gun Thug` are valid. `[Must-match]`
6. An empty key (`=5`) is a valid key named by the empty string. `[Must-match]`

Section state and ordering:

- Keys before the first section header attach to the most recently selected section name of **any** configuration file previously parsed in the process. That name is initially `unknown`. `[May-improve]`: reset the current section to `unknown` at the start of each file. Keys before any header in `ai.txt` then form a section named `unknown`, which (lacking all required keys) makes initialization fail (section 3.1).
- A section exists only once it holds at least one key. A header followed by no key/value lines creates **no** packet. `[Must-match]`
- Section names and keys are compared case-insensitively by folding ASCII `A`-`Z` to lowercase, then comparing unsigned bytes. `[Must-match]`
- A repeated section header resumes the existing section; a repeated key replaces the earlier value (last occurrence wins). `[Must-match]`
- Sections are stored **sorted by case-folded name**, using the comparison above, not in file order. Ties cannot occur because duplicates merge. `[Must-match]` For example, `_` (`0x5F`) sorts before `a` (`0x61`), and `Z` folds to `z` and sorts after `a`.
- The stored section name keeps the case of its first occurrence. `[Must-match]`

Embedded NUL bytes end the line for every later parsing step. `[May-improve]`: reject a file containing NUL with a diagnostic.

### 2.3 Packet Keys

Every packet section must contain all 25 keys below; their order within the section is irrelevant. These are file-format key literals, which the parser must match. Integer keys use decimal-prefix semantics: skip leading whitespace, accept an optional sign, read decimal digits, and stop at the first non-digit. No digits gives 0. Trailing text is ignored, so `8 hexes` yields 8. `[Must-match]` Values outside the signed 32-bit range are undefined in the reference. `[May-improve]`: saturate to the 32-bit range and emit a diagnostic.

| Key | Type | Meaning / consumer |
| --- | --- | --- |
| `packet_num` | int | Packet number matched against the critter's instance AI packet (section 3.1) |
| `max_dist` | int | Flee/stop distance limit, in hexes (sections 3.6, 3.8) |
| `min_to_hit` | int | Minimum acceptable hit chance; also the minimum weapon skill (sections 3.9-3.12) |
| `min_hp` | int | Flee when current HP is strictly below this value (section 3.6) |
| `aggression` | int | Parsed and required, but **never consulted** by any behavior in this revision `[Must-match]` |
| `hurt_too_much` | string | Comma list converted to a damage-flag mask (section 2.4) |
| `secondary_freq` | int | Secondary hit mode chance denominator (section 3.10) |
| `called_freq` | int | Called-shot chance denominator (section 3.11) |
| `font` | int | Taunt font number (SPEC-002 font selection) |
| `color` | int | Taunt text-color word (SPEC-002: low 8 bits palette index, higher bits drawing flags) |
| `outline_color` | int | Taunt outline palette index, or -1 for no outline |
| `chance` | int | Percent chance (1-100 draw) that a taunt is attempted |
| `run_start` | int | First message ID of the flee taunt range |
| `move_start` | int | First ID of the approach range; inclusive end of the flee range |
| `attack_start` | int | First ID of the attack range; inclusive end of the approach range |
| `miss_start` | int | First ID of the miss range; inclusive end of the attack range |
| `hit_head_start` | int | Hit-location 0 range start; inclusive end of the miss range |
| `hit_left_arm_start` | int | Location 1 start; inclusive end of location 0 |
| `hit_right_arm_start` | int | Location 2 start; inclusive end of location 1 |
| `hit_torso_start` | int | Location 3 start; inclusive end of location 2 |
| `hit_right_leg_start` | int | Location 4 start; inclusive end of location 3 |
| `hit_left_leg_start` | int | Location 5 start; inclusive end of location 4 |
| `hit_eyes_start` | int | Location 6 start; inclusive end of location 5 |
| `hit_groin_start` | int | Location 7 start; inclusive end of location 6 |
| `last_msg` | int | Inclusive end of the location 7 (groin) range |

Note that the hit-location key order (head, left arm, right arm, torso, right leg, left leg, eyes, groin) follows hit-location numbering 0-7. It is not the alphabetical or file order. `[Must-match]`

Each critter also needs its per-instance packet number (section 2.1). The packet's section name serves only as a diagnostic/editor label and for the name query (section 3.1). `[Must-match]`

### 2.4 `hurt_too_much` Token Grammar

The (already trimmed) value is first lowercased in place. Then, starting at the first byte:

1. Skip leading `0x20` spaces only. Tabs are not skipped here. `[Must-match]`
2. The token runs up to the next `,` or the end of the value. **Trailing spaces in a token are not removed**, so `blind ,crippled` makes `blind ` unrecognized. `[Must-match]`
3. Compare the token exactly against the table below, then OR in the mask. Unrecognized tokens, including empty tokens from `,,` or a trailing `, `, contribute nothing and emit a diagnostic. `[Must-match]`
4. If a `,` ended the token, continue after it; otherwise stop. `[Must-match]`

An empty value yields mask 0, with no diagnostic. `[Must-match]`

| Token | Mask | Meaning |
| --- | --- | --- |
| `blind` | `0x40` | Blinded |
| `crippled` | `0x3C` | Any crippled limb (both legs and both arms) |
| `crippled_legs` | `0x0C` | Either leg crippled |
| `crippled_arms` | `0x30` | Either arm crippled |

The flee test (section 3.6) is `(damage flags AND mask) != 0`, so for example `crippled_legs` triggers on **either** leg. `[Must-match]`

### 2.5 Message Resources

`text\<language>\game\combatai.msg` is a SPEC-002 message catalog loaded at AI initialization. Taunt IDs come from packet ranges (section 3.17). When the `language_filter` preference in the game configuration's `preferences` section is nonzero at load, the SPEC-002 language filter is applied. `[Must-match]`

A preference change re-evaluates the filter. The remembered previous state starts at -1, so the first evaluation always acts. If the new value differs from the remembered one: value exactly 1 applies the filter to the loaded catalog; any other value (0, or nonzero values other than 1) unloads and reloads the catalog unfiltered. `[Must-match]` Consequently, preference value 2 loads filtered at initialization but becomes unfiltered on the first preference re-evaluation. `[Must-match]`

The AI also prints fixed lines from the general `misc.msg` catalog into the message monitor:

| ID | Use | Printed form |
| ---: | --- | --- |
| 5000 | NPC used a drug (healing) | `<critter name> <text> <item name>.` |
| 5001 | NPC is out of ammunition | `<critter name> <text> <weapon name>.` |
| 5002 | NPC reloaded | `<critter name> <text> <weapon name>.` |

The separators are single spaces and the form ends with `.`. If the catalog lacks the ID, nothing is printed, but the accompanying animation still occurs. `[Must-match]` The reference formats into a 200-byte buffer without bounds. `[May-improve]`: truncate the formatted line to 199 bytes.

## 3. Behavioral Rules & Edge Cases

### 3.1 Initialization, Packet Resolution, and Name Query

Initialization order: load `combatai.msg` (failure aborts with error), then parse `ai.txt` (section 2.2), then build the packet table in **sorted section order**. For each section in that order, read all 25 keys. If any section lacks any key, the whole AI initialization fails: no partial table is kept and game initialization treats it as an error. `[Must-match]`

If `ai.txt` is absent or unreadable, the reference parser still reports success, leaving zero sections and a zero-length packet table, and AI initialization reports success. Any later packet resolution then reads entry 0 of an empty table. `[May-improve]`: treat a missing file or a zero-packet table as an initialization error.

**Packet resolution for a critter** uses its instance packet number P: scan the packet table in sorted order and return the **first** entry whose `packet_num` equals P. If none matches, emit a diagnostic and return table entry 0. Entry 0 is the alphabetically first section name, not the packet numbered 0. `[Must-match]` Duplicate packet numbers are legal; the alphabetically earlier section wins. `[Must-match]`

Resolution happens at every query and is never cached on the critter. Script changes to the packet number (section 3.18) therefore take effect at the next query. `[Must-match]`

**Packet count query** returns the table length. `[Must-match]`

**Name query** for packet number N returns nothing if `N < 0` or `N >= table length`. Otherwise it returns the section name of the first sorted entry whose `packet_num` equals N, or nothing. The range guard conflates packet numbers with the table count: a valid packet numbered at or above the count cannot be named. `[Must-match]` This query is used only by tooling and debug displays, not by NPC behavior.

### 3.2 Combat Snapshot

At combat start (SPEC-007 §3.3), after the combat list is collected (non-hidden critters on the combat elevation, in ascending hex order 0-39999 and per-hex object order), the AI copies that list into the snapshot. If copying fails, the snapshot is empty. `[Must-match]` At combat end the snapshot is discarded and its count becomes 0. `[Must-match]`

The snapshot includes the player, party members, every team, and critters that later die. Dead critters remain until removed. `[Must-match]` When a critter is removed from combat (for example destroyed), it is swap-removed: the count is decremented and the removed entry trades places with the last live entry, so order changes. `[Must-match]`

**Distance sorting mutates the snapshot in place.** Danger-source, nearest-team, and attacker searches (sections 3.4, 3.5) re-sort the snapshot by ascending `dist` from the querying critter, and that order persists for later consumers, including random retargeting (section 3.16). `[Must-match]` Null entries sort after all non-null entries. The reference uses the host C library's unstable sort, so ties have no defined order. `[May-improve]`: use a **stable** ascending sort, keeping the existing snapshot order among equal distances.

**Restoring a save made during combat does not rebuild the snapshot** from the restored combat list. Depending on prior process state, it is either empty or refers to critters from before the load. `[May-improve]`: when restoring an in-combat save, rebuild the snapshot from the restored combat list in its restored order. An implementation that wants reference-faithful post-load behavior may instead use an **empty** snapshot, but it must never retain references to unloaded objects.

### 3.3 Perception Predicate and Onlooker Alerting

`perceives(observer, target)` is the SPEC-007 §3.2 perception predicate. Let d = `dist(target, observer)` and P = observer's effective Perception:

- If the target is inside the observer's forward sector (facing and both adjacent orientations), limit = `5P`. Halve it (truncating) when the target has object flag `0x00020000` (glass translucency). Then quarter it (truncating) when the target is the player and the player's sneak is currently working. `[Must-match]`
- Otherwise, limit = `2P` during combat or `P` outside combat. Quarter it (truncating) for a sneaking player target as above. There is **no** glass halving in this branch. `[Must-match]`
- Return `d <= limit`. No line-of-fire or obstacle ray is part of this predicate. `[Must-match]`

**Onlooker alerting.** Whenever damage is applied to a critter (the defender, each splash/burst extra, and an unintended "oops" victim distinct from the defender), every snapshot entry that perceives the damaged critter gets intent bit `0x01` set. This includes the damaged critter itself, dead entries, and entries of any team. `[Must-match]`

### 3.4 Last-Attacker Reference and Retaliation Rating

The last-attacker reference holds either nothing or a critter. Assigning it through the **standard setter**:

1. Refuses (no change) a non-null value whose art category is not critter. `[Must-match]`
2. Stores the value. `[Must-match]`
3. If the value is the player, writes numeric reaction 1 into the critter's script-local variable 0, which SPEC-006 classifies as a bad reaction. Without a script, nothing is written. `[Must-match]`

Direct writes that bypass steps 1 and 3 (no reaction change) are also observable: the in-combat `attack` opcode branch, `critter_stop_attacking`, combat removal (clears), and restoration cleanup. `[Must-match]`

Writers of the reference:

| Event | Effect | Path |
| --- | --- | --- |
| Combat start with an initiating pair (SPEC-007) | attacker and defender each reference the other | setter |
| Player attack debit (SPEC-007 §3.4) | player references the defender | setter |
| NPC attack loop entry (section 3.12) | attacker references its target | setter |
| Damage applied by a critter attacker, victim dead or knocked out afterwards | victim references the attacking critter | setter |
| Damage applied by a critter attacker to a still-active defender | retaliation check, only if the defender is the unintended victim or on a different team from the attacker | rating |
| Damage applied to a still-active extra | retaliation check, only if on a different team (no unintended-victim exception) | rating |
| Explosion damage attribution with a known source | each surviving defender/extra other than the source references the source | setter |
| Player drops a primed explosive while closing inventory | each non-player critter not on the player's team, in the explosion-extras set centered on the player's hex, that passes a Perception stat check (SPEC-006 D10) references the player | setter |
| Team switch (section 3.15) | cleared if it now references a teammate | setter |
| `attack` opcode during combat, intent bit `0x01` clear | set `0x01`, reference the target | direct |
| `critter_stop_attacking` | set `0x02`, clear the reference | direct |
| Restoration, or `critter_damage` with the -1 sentinel present | sentinel replaced with nothing | direct |

All of these are `[Must-match]`. A same-team attacker who is neither dead-making nor the unintended victim therefore does **not** change the defender's reference. `[Must-match]` For the explosive-drop row, the Perception check consumes one D10 draw per eligible critter in set order, and the first critter that references the player starts a combat request against the player when not already in combat. The geometry of the explosion-extras enumeration belongs to the explosion rules (SPEC-007 §3.7) and is not restated here.

**Retaliation rating** of a critter X:

- 0 if X is null, X's art category is not critter, or X is dead or knocked out. `[Must-match]`
- Otherwise start with R = effective Melee Damage. For the right-hand item, then the left-hand item: if it is a weapon whose damage range is available and its maximum damage is **greater than** R, set R to that maximum. `[Must-match]`
- Return `R + effective Armor Class`. `[Must-match]`

Note that this effective AC includes the unspent-AP AC bonus (SPEC-007 §3.4), so a critter that has not acted rates higher. `[Must-match]`

**Retaliation check** for victim V and attacker A: if `rating(A) > rating(V's current reference)` (strictly), assign A through the setter. Otherwise keep the current reference. `[Must-match]`

### 3.5 Danger-Source Selection

`danger(C)` for critter C returns a critter or nothing:

1. Let H be C's last attacker. If H is nothing or C itself, t0 = nothing. If H is **not dead**, return H immediately, without any perception, elevation, visibility, or team check. `[Must-match]` If H is dead and on a different team from C, t0 = `nearest_team_member(C, H)`; otherwise t0 = nothing.
2. Compute t1-t3 by the attacker search below. `[Must-match]`
3. Sort the four-entry list `[t0, t1, t2, t3]` by ascending `dist` from C, nulls last (stable per section 3.2's improvement rule). Return the first non-null entry that C perceives (section 3.3); otherwise nothing. `[Must-match]`

`nearest_team_member(C, H)`: if the snapshot is empty, nothing. Otherwise sort the snapshot from C and return the first entry that is not C, not dead, and on H's team. This can be the player, a hidden critter, or a critter on another elevation still in the snapshot. `[Must-match]`

**Attacker search** from C sorts the snapshot from C, then scans entries other than C in order. Each slot is filled at most once, by the first qualifying entry. A single entry may fill several slots. The scan stops once three slot fills have occurred or the list ends. `[Must-match]`

| Slot | Qualifying entry E | Slot value |
| --- | --- | --- |
| t1 "my victim" | E not dead and E's last attacker is C | E |
| t2 "teammate's attacker" | E on C's team; E's last attacker K is not nothing, not C, not on C's team, and not dead (E itself may be dead) | K |
| t3 "team's victim" | E not on C's team, not dead, and E's last attacker is on C's team | E |

### 3.6 NPC Turn Entry Decision Tree

When a non-player critter's turn reaches AI processing (SPEC-007 §3.3: not overridden by script, after automatic stand-up), the AI receives an optional **requested target** T. T is the scripted combat request's defender, but only during the very first turn after a scripted request starts combat; it is nothing otherwise. `[Must-match]` During that first turn, the request's accuracy bonus also adds to every hit-chance evaluation the AI makes (SPEC-007), including its threshold comparisons. `[Must-match]`

Let K be the resolved packet. Then:

1. **Forced flee.** If intent bit `0x04` is set, or `(damage flags AND K.hurt_too_much) != 0`, or effective current HP `< K.min_hp`: perform flee (section 3.8) and end AI processing. Return T unchanged; no healing or attack. `[Must-match]`
2. **No requested target:** perform healing (section 3.7). The reference's healing step always reports "no need to flee", so the flee branch tied to it is unreachable. `[Must-match]` Then T = `danger(self)`. If T exists, run the attack loop (section 3.12) against it.
3. **Requested target present:** run the attack loop against T, with no healing step. `[Must-match]`
4. **Max-distance stop.** If T exists, T is not dead, own AP is nonzero, and `dist(self, T) > K.max_dist`: set intent bit `0x02`. `[Must-match]`
5. **Hurt and unopposed (non-party).** If T is nothing and self is **not** a party member: if the last attacker exists, is not dead, and damage taken last turn is positive, perform flee. `[Must-match]`
6. **Follow (party).** If T is nothing and self **is** a party member: if `dist(self, player) > 5`, approach the player (section 3.12 approach rule, without the taunt). `[Must-match]`

Party membership: objects with object ID below 18000 are never party members. Otherwise membership is a lookup in the current party list, which includes the player. `[Must-match]`

### 3.7 Healing

Healing applies only to body type 0 (biped). Let B = `floor(effective max HP / 2)`. Loop:

1. Stop unless effective current HP `< B`. `[Must-match]`
2. Stop if AP `< 2`. `[Must-match]`
3. Find the next drug (item type 2) among the critter's **top-level** inventory entries after the last examined position. Containers are not searched, and equipped entries are included. Stop if none remains. `[Must-match]`
4. If its PID is neither 40 nor 144, examine the next drug without resetting the position (step 1 is re-evaluated first). `[Must-match]`
5. Otherwise remove one unit from that stack. If removal fails, continue from the next position without charging AP. `[Must-match]` On success, apply the drug through the normal drug-consumption rules:
   - If consumption reports failure, force the unit back into inventory. `[Must-match]`
   - Otherwise play the hands animation (critter animation code 11), print `misc.msg` 5000 with the drug name, and destroy the unit. `[Must-match]`
6. In **both** consumption outcomes, deduct 2 AP, saturating at 0, and restart the search from the first inventory position. `[Must-match]` A failing consumption therefore spends AP without healing and can repeat until AP is exhausted.

No packet value, chemical-use preference, or random draw participates, apart from the drug's own effect. `[Must-match]`

### 3.8 Flee Movement

Flee is computed **relative to the player**, not to the threat, for every critter, including party members. `[Must-match]`

1. Let K be the packet. If `dist(self, player) >= K.max_dist`: set intent bit `0x02` and stop, with no movement and no taunt. `[Must-match]`
2. Otherwise set intent bit `0x04`. Let D = the hex direction from the player's hex to self's hex (SPEC-003 §3.5 steps 1-3, applied once). `[Must-match]`
3. For N from current AP down to 1, try the candidates D, `(D+1) mod 6`, then `(D+5) mod 6`, in that order. Each candidate destination is the SPEC-003 §3.4 multi-step directional operation from self's hex, N steps (which stops at outer-border hexes). Accept the first candidate for which a SPEC-007 route search from self's hex with **strict goal** has a nonzero step count. `[Must-match]`
4. If a candidate was accepted: in one animation sequence, register a flee taunt (section 3.17, type run, delay 0) and a run-to-hex movement limited to the current AP. Run the sequence. If no candidate was accepted (including AP 0), there is no movement and no taunt, but the fleeing bit stays set. `[Must-match]`

Because of the strict-goal rule, an occupied or blocked candidate is rejected, and a candidate equal to the current hex (border stop) has route length 0 and is rejected. `[Must-match]`

### 3.9 Weapon Evaluation

**Usability** of weapon W in hit mode M for critter C:

- False if both arms are crippled (`0x10` and `0x20`). `[Must-match]`
- False if any arm is crippled and W is two-handed. `[Must-match]`
- False unless C has critter art for: C's base art index, W's animation for M, W's weapon-animation code, and directional selector `orientation + 1` (SPEC-007 §2.1-2.3 existence test). `[Must-match]`
- False if C's effective skill for W's skill in mode M is **below** `K.min_to_hit`. This compares a skill percentage, not a hit chance. `[Must-match]`
- Otherwise true. `[Must-match]`

**Preference between two weapons** W1 (incumbent) and W2 (challenger):

1. If either is nothing, return the other. `[Must-match]`
2. Compare their **primary-mode** subtypes S1, S2 (low nibble). If they differ: ranged (4) wins; otherwise throw (3) wins. `[Must-match]`
3. Otherwise, or if neither rule decided (for example unarmed vs. melee), return W2 only if `cost(W2) > cost(W1)` strictly; ties keep the incumbent. `[Must-match]`

Item cost is the prototype base cost. A weapon adds `floor(loaded rounds x ammo-prototype cost / ammo-prototype pack quantity)` for its loaded ammunition PID when loaded rounds > 0, using truncating signed 32-bit integer arithmetic. `[Must-match]` A zero pack quantity divides by zero in the reference. `[May-improve]`: treat the ammunition contribution as 0 and emit a diagnostic.

**Inventory search** for critter C, with an optional AP check:

- Returns nothing unless C's body type is 0 or 2 (biped or robotic). Quadrupeds never switch weapons. `[Must-match]`
- Iterate top-level inventory entries of item type 3 in order. Skip the entry currently in the right hand (identity comparison); the left-hand entry is eligible. `[Must-match]`
- With the AP check, skip W if the **raw prototype primary AP cost** exceeds current AP. This raw cost has no perk or aiming adjustments. `[Must-match]`
- Skip W unless it is usable in hit mode 2 (right primary). `[Must-match]`
- If W's primary subtype is ranged and its loaded rounds are 0, skip W unless a compatible ammunition entry exists in inventory (see below). Throw and melee weapons have no ammunition test. `[Must-match]`
- Fold the preference rule over the survivors in inventory order. The result is the inventory best weapon. `[Must-match]`

**Compatible ammunition in inventory** for weapon W: the first top-level inventory entry of item type 4 for which reload is permitted. Reload is permitted when the item is a weapon, the ammunition is ammo, their calibers are equal, and, if the weapon is loaded, the ammunition PID equals the loaded PID (SPEC-007 §3.4). `[Must-match]`

**Environment search** for item type T (weapon or ammo):

- Biped only. `[Must-match]`
- Collect map items: non-hidden objects whose **art category** is item, on the map's current elevation, in ascending hex order then per-hex object order. Sort by ascending `dist` from the critter (stable improvement as in section 3.2). `[Must-match]`
- Let limit = effective Perception + 5. Scan in order; stop at the first item with distance greater than the limit. Return the first item of type T that is usable in mode 2 (weapon), or for which reload of the critter's **current right-hand item** is permitted (ammo). `[Must-match]`
- No line-of-sight, reachability, AP, or ammunition check is applied to a found weapon. `[Must-match]` For ammo, the reference reload test dereferences a null right-hand item. In the reference this path is reached only with a non-null weapon. `[May-improve]`: treat a null weapon as "not permitted".

**Retrieval** of a map item: perform the standard pick-up action. In combat this is: walk toward the item with the current-AP movement budget, require adjacency (`dist <= 1`) or abort, charge 3 AP or abort when fewer than 3 remain, then play the pick-up animation and move the item into inventory. Run the sequence, then look the item up in the critter's inventory by object ID. The result is that item, or nothing if any step failed. `[Must-match]`

**Weapon switching** (outputs a weapon and a hit mode):

1. Initialize the outputs to nothing and hit mode 4 (punch). The caller's values are overwritten even when switching fails. `[Must-match]`
2. Run the inventory search with the AP check. If it finds W, set the weapon to W and the mode to the hit-mode selection for W (section 3.10). `[Must-match]`
3. Otherwise run the environment search for weapons. If it finds an item, retrieve it; if retrieved, set the weapon and its selected mode. `[Must-match]`
4. If a weapon was set: wield it in the right hand (no AP charge for NPCs) and run the animation. Succeed if the SPEC-007 attack AP cost of the chosen mode (non-aimed) is at most the current AP. Otherwise fail, with the weapon remaining wielded and output. `[Must-match]`
5. If no weapon was set, fail. `[Must-match]`

### 3.10 Hit-Mode Selection

For critter C and weapon W:

1. If W is nothing or not item type 3, select mode 4 (punch). `[Must-match]`
2. If W's secondary subtype is 0 (none), select 2. `[Must-match]`
3. If W is not usable in mode 3, select 2. `[Must-match]`
4. Draw `R = random(1, K.secondary_freq)` (SPEC-006 range draw, one generator draw). If R != 1, select 2. For `secondary_freq <= 1` the result is always 1: equal bounds still consume a draw, and the reversed-range wrapper returns its first bound. `[Must-match]`
5. If the secondary subtype is throw (3), run an inventory search **without** the AP check (which excludes the right-hand item). If it finds nothing, make an Intelligence stat check (SPEC-006: one D10 draw against effective Intelligence + 0). If that check succeeds (result code 2 or 3), select 2, keeping the only weapon instead of throwing it. `[Must-match]`
6. Otherwise select 3. `[Must-match]`

### 3.11 Called-Shot Location

For critter C, target X, and mode M, the default is location 3 (torso). Steps:

1. If the SPEC-007 aimed AP cost for M exceeds current AP, return torso, with no draws. `[Must-match]`
2. If aimed attacks are unavailable for M (SPEC-007 §3.4 availability), return torso. `[Must-match]`
3. Draw `random(1, K.called_freq)`. If the result != 1, return torso (same reversed/equal-range rule as section 3.10). `[Must-match]`
4. Read the `combat_difficulty` preference (default 1). Minimum Intelligence is 7 for 0 (easy), 5 for 1 (normal), and 3 for 2 (hard). Other values leave the threshold uninitialized in the reference. `[May-improve]`: clamp the preference into 0-2, and use 5 for values outside that range.
5. If effective Intelligence is below the minimum, return torso. `[Must-match]`
6. Draw `L = random(0, 8)`. Location 8 (uncalled) is a possible result. `[Must-match]`
7. If the SPEC-007 range-aware hit chance for (C, X, L, M) is below `K.min_to_hit`, return torso; otherwise return L. `[Must-match]`

A returned location other than 3 or 8 makes the attack aimed, adding SPEC-007's aiming AP cost at debit. Returning 8 makes it an unaimed attack that SPEC-007 later resolves to torso. `[Must-match]`

### 3.12 Bounded Attack Loop

`attack_loop(C, X)`:

1. Assign C's last attacker := X through the setter (section 3.4). This also applies the reaction side effect when X is the player. `[Must-match]`
2. Let W = C's right-hand item if it is item type 3, otherwise nothing. Let M = the hit-mode selection for W (section 3.10, possibly drawing). `[Must-match]`
3. **Unarmed upgrade.** If W is nothing and any of the following holds, run weapon switching (section 3.9) and adopt its outputs regardless of success: X's body type is not biped; X's art identifier has a nonzero weapon-animation code (bits 12-15, meaning X visibly holds a weapon); or C lacks critter art for animation 16 (punch) with weapon code 0 at selector `orientation + 1`. `[Must-match]`
4. Repeat for at most **10 attempts**. Each attempt first stops (returning failure) if C has damage flags `0x01`, `0x80`, or `0x8000`. Then classify with the SPEC-007 attack preflight for (C, X, M, non-aimed) and act: `[Must-match]`

| Preflight | Action |
| --- | --- |
| 0 OK | Let H = range-aware hit chance (SPEC-007) for (C, X, uncalled, M). If `H >= K.min_to_hit`: **attack** (below); if that fails, return failure. Then, if the non-aimed AP cost of M exceeds current AP, return failure; otherwise continue to the next attempt (attack again). If `H < K.min_to_hit`: compute the **no-range** hit chance H'. If `H' < K.min_to_hit`, flee and return success. Otherwise **approach** (taunt allowed if the taunt flag is set); if the approach fails, flee and return success. Clear the taunt flag. |
| 1 No ammo | Reload (section 3.13). |
| 2 Out of range | Compute H' (no-range). If `H' < K.min_to_hit`, flee and return success. If W exists, approach; failure returns failure; clear the taunt flag. If W is nothing, run weapon switching: when switching **fails** or yields no weapon, approach as above; when switching succeeds, do nothing more this attempt. |
| 3, 6, 7 Not enough AP / arm crippled | Run weapon switching; if it fails, return failure. |
| 5 Aim blocked | Approach; failure returns failure; clear the taunt flag. |
| 4 Target dead | No action; the attempt is consumed. |

5. After 10 attempts, return failure. `[Must-match]`

All rows are `[Must-match]`, including the asymmetric flee-vs-fail outcomes and the no-op dead-target attempts. The loop's return value is not consulted by the turn logic (section 3.6). `[Must-match]`

The **taunt flag** starts set. Only the first successful approach in one loop may register an approach taunt. `[Must-match]`

**Attack** (C, X, M):

1. If intent bit `0x04` is set, fail. `[Must-match]`
2. Register and run a turn-toward-X's-hex animation. `[Must-match]`
3. Choose the location by section 3.11. `[Must-match]`
4. Request the SPEC-007 combat attack. A nonzero (failure) result fails. `[Must-match]`
5. Run the queued animations and succeed. `[Must-match]`

**Approach** (C, X, taunt):

1. If `dist(C, X) <= 1`, fail. `[Must-match]`
2. Register an optional approach taunt (type move, delay 0), then a walk-to-object movement limited to current AP. A zero AP budget makes registration fail. `[Must-match]`
3. If the sequence cannot be completed, fail. Otherwise run it and succeed, even if the route reached no closer hex. `[Must-match]` Movement is a walk (SPEC-007 move-to-object, stopping short of the target), with AP charged per committed hex.

The hit mode M is not re-selected between consecutive attacks within one loop, except via weapon switching. `[Must-match]`

### 3.13 Reloading

On preflight result 1, with W the current weapon:

1. Find compatible inventory ammunition A (section 3.9). If found, **reload**. `[Must-match]`
2. Otherwise run the environment search for ammo. If found, retrieve it; if retrieved, reload with it. If retrieval fails, do nothing this attempt. `[Must-match]`
3. If neither found anything: request the weapon's out-of-ammo sound at the critter's positional volume, play the hands animation, print `misc.msg` 5001, unwield the right hand (running the animation if unwield succeeded), then run weapon switching, ignoring its result. `[Must-match]`

**Reload** of W with ammo object A: compute the result with the standard reload rule. If the magazine is below capacity, transfer `min(capacity - loaded, rounds in A)`, set W's ammunition PID to A's PID, and return the rounds left in A. A full magazine transfers nothing and returns A's rounds unchanged. An impermissible pair returns -1. `[Must-match]`

- If the result is 0, destroy A. For a multi-unit stack this removes one unit; the remaining units keep a full per-unit round count (inventory stack rules). `[Must-match]`
- If the result is not -1: request the weapon's ready sound, play the hands animation, print `misc.msg` 5002, and deduct **2 AP, saturating at 0, with no prior AP check**. `[Must-match]` An NPC can therefore reload at 0 AP, as SPEC-007 §3.4 notes.
- If the result is -1, nothing happens and the attempt is consumed. `[Must-match]`

Sound requests are host presentation requests. Their filename codec is outside this specification. The sound request precedes the hands animation and message. `[Must-match]`

### 3.14 Join and Stop Predicates

Both predicates first service pending background work (audio/input processing); this has no AI-state effect. `[Must-match]`

**Wants to join** (for a waiting critter W, SPEC-007 §3.3):

1. False if W is hidden (`0x01`), W's elevation differs from the player's, or W is dead or knocked out. `[Must-match]`
2. True if W's damage taken last turn > 0. `[Must-match]`
3. If W has a script, clear its source/target context, set its fixed parameter to 5, and run its combat procedure. `[Must-match]`
4. True if intent bit `0x01` is set; false if `0x02` is set; false if `0x04` is set. `[Must-match]`
5. True if and only if `danger(W)` exists. `[Must-match]`

**Wants to stop** (for critter S):

1. True if intent bit `0x02` is set, or S is knocked out or dead, or intent bit `0x04` is set. `[Must-match]`
2. Let G = `danger(S)`. True if G is nothing, or S does not perceive G. `[Must-match]`
3. True if S's current AP equals its effective maximum AP. `[Must-match]`
4. Otherwise false. `[Must-match]`

Step 3 makes any critter that has not spent AP this round willing to stop. `[Must-match]` Because `danger` returns a living last attacker without a perception test, the separate perception test in step 2 is what lets such a critter stop once it loses track of that attacker. `[Must-match]`

### 3.15 Team Switching

Setting critter C's team to N:

1. Store N. `[Must-match]`
2. If C's last-attacker slot holds the -1 restoration sentinel, clear it through the setter, emit a diagnostic, and fail. The team change has already happened. `[Must-match]`
3. If the last attacker exists and is on team N, clear it through the setter. `[Must-match]`
4. If combat is active, rebuild C's outline: hostile, or friendly when the player has the Friendly Foe perk and C's team equals the player's team. If C's outline was previously enabled and not disabled, re-enable and redraw it. `[Must-match]`

Callers: `critter_add_trait` kind 1, selector 6 (skipped when the team is unchanged or during save restoration), and `party_add`, which switches the new member to team 0. `[Must-match]`

### 3.16 Random Retargeting on Critical Failure

When an attack result carries damage flag `0x00100000` (SPEC-007 critical-failure random hit), choose a replacement defender:

1. If the snapshot is empty, return nothing, with no draw. `[Must-match]`
2. Draw `S = random(0, n - 1)` for snapshot count n. `[Must-match]`
3. Starting at snapshot index S and wrapping, examine each entry once. Return the first entry E that is neither the attacker nor the original defender, lies in the attacker's forward perception sector (SPEC-007 §3.2 sector test only, no distance limit), and has preflight result 0 for (attacker, E, the attack's hit mode, non-aimed). `[Must-match]`
4. If none qualifies, return nothing. `[Must-match]`

The scan order is the snapshot's **current** order, which earlier distance sorts may have changed (section 3.2). `[Must-match]` Teammates and the player are eligible. A range query in the reference has no effect.

### 3.17 Combat Taunts

A taunt request (critter C, attack context, type, delay) proceeds as follows:

1. Fails unless C's PID category is critter. `[Must-match]`
2. Read the `combat_taunts` preference (default 1). If it is 0, fail with no draw. `[Must-match]`
3. Fail if C is the player, or C is dead or knocked out (no draw). `[Must-match]`
4. Resolve packet K. Draw `random(1, 100)`. If the result is greater than `K.chance`, fail. `[Must-match]`
5. Choose the inclusive range [start, end] by type: `[Must-match]`

| Type | Start | End |
| --- | --- | --- |
| Run (flee) | `run_start` | `move_start` |
| Move (approach) | `move_start` | `attack_start` |
| Attack | `attack_start` | `miss_start` |
| Miss | `miss_start` | `hit_head_start` |
| Hit at location L (0-7) | start key for L | start key for L+1, or `last_msg` for L = 7 |

6. If end < start, fail with no further draw. `[Must-match]`
7. Draw `ID = random(start, end)`. If ID is absent from `combatai.msg`, fail. `[Must-match]`
8. Copy the text into the type's shared buffer: one buffer for run/move/attack and another for miss/hit. Then register an animation-sequence callback with the given delay that later displays the buffer. `[Must-match]`

Both endpoints are inclusive, so a boundary ID such as `move_start` is eligible for both adjacent types. `[Must-match]` The text displayed is the buffer content **at callback time**, so a later request of the same buffer class registered before display replaces the earlier text. `[Must-match]` The buffers hold 80 bytes and the reference copies without bounds. `[May-improve]`: truncate the copied text to 79 bytes.

Hit taunts are requested after the hit location is final. An uncalled request has already become torso (3), so L is always in 0-7 in the inspected paths. `[Must-match]` A hit-type request with L = 8 would read past the range keys in the reference. `[May-improve]`: treat L = 8 as 3.

Request sites:

| Attack form | Attacker | Defender |
| --- | --- | --- |
| Melee (animation code below 45 and not 18) | Attack (delay 0) at sequence start; then Hit (delay -1) if the attack hit and the defender did not die, or Miss (delay -1) if it missed | n/a |
| Ranged/thrown (animation code 45 or above, or 18) | Attack (delay 0) at sequence start | Miss (delay -1) if the attack missed; Hit (delay -1) if it hit and the defender did not die |
| Flee (section 3.8) | Run (delay 0) | n/a |
| Approach (section 3.12) | Move (delay 0) | n/a |

In ranged combat it is the **defender** who speaks the hit/miss taunt, using the defender's own packet and the location it was hit. In melee it is the attacker. `[Must-match]`

**Display callback:** if any floating text object currently exists anywhere, display nothing. `[Must-match]` Otherwise create a floating text object over C, with font `K.font`, color `K.color`, and outline `K.outline_color` (-1 means none). Lines are word-wrapped at 200 pixels and centered. The outline adds a 1-pixel border on each side and is drawn with the outline palette index. An empty taunt text, or a full floating-text table, creates nothing. Display lifetime belongs to the floating-text subsystem, whose default delays are 3500 ms base and 1399 ms per line. `[Must-match]`

### 3.18 Script Opcode Surface

Only public SSL opcode names appear below. Stack contracts are those of SPEC-005; this table adds the AI-visible effects.

| Opcode | Name | AI-visible effect |
| --- | --- | --- |
| `0x80D0`, `0x80DD` | `attack` | Out of combat: create a combat request with self as attacker, the target as defender, the given accuracy bonus, damage bounds, and optional flag pair. In combat: if self's intent bit `0x01` is clear, set it and directly assign the last attacker := target (no reaction change). Rejected when either party is inactive or the target has `0x04` set. `[Must-match]` |
| `0x8143` | `attack_setup` | Same as `attack` with accuracy 0 and damage range 0-2147483647. `[Must-match]` |
| `0x80F3` | `has_trait` | Kind 1, selector 5 returns the instance AI packet number; selector 6 returns the team. `[Must-match]` |
| `0x8102` | `critter_add_trait` | Kind 1, selector 5 stores the packet number verbatim, unvalidated; unknown numbers fall back per section 3.1. Selector 6 performs team switching (section 3.15). `[Must-match]` |
| `0x8151` | `critter_is_fleeing` | Returns intent bit `0x04`. |
| `0x8152` | `critter_set_flee_state` | Sets or clears intent bit `0x04` only. Bit `0x02` and the last attacker are untouched. `[Must-match]` |
| `0x8155` | `critter_stop_attacking` | Sets intent bit `0x02` and directly clears the last attacker. `[Must-match]` |
| `0x8153` | `terminate_combat` | Deferred quit-combat request; no direct AI-state change. |
| `0x80DC` | `obj_can_see_obj` | Uses the section 3.3 perception predicate plus a stride-16 blocker ray (SPEC-005/007). |
| `0x80F5` | `obj_can_hear_obj` | Recovered inverted null test (SPEC-005). Valid operands never reach the perception predicate. `[Must-match]` A null operand dereferences null in the reference. `[May-improve]`: return 0 and diagnose. |
| `0x8124` | `party_add` | Switches the member to team 0 (section 3.15). |

Combat-procedure fixed-parameter values seen by AI-relevant script calls: **4** at turn start (SPEC-007), **5** for the join query (section 3.14), **2** to the attacker after it hits with nonnegative damage. `[Must-match]`

### 3.19 Random-Draw Order Summary

All draws use the single SPEC-006 generator. A clean-room engine must consume draws in exactly this order and only under these conditions. `[Must-match]`

| Decision | Draw | Condition for drawing |
| --- | --- | --- |
| Hit-mode selection | `random(1, secondary_freq)` | Weapon has a secondary subtype and is usable in mode 3 |
| Throw retention | D10 Intelligence check | Secondary draw gave 1, secondary is throw, and no other inventory weapon |
| Called-shot gate | `random(1, called_freq)` | Aimed AP affordable and aiming available |
| Called-shot location | `random(0, 8)` | Gate gave 1 and the Intelligence threshold was met |
| Taunt chance | `random(1, 100)` | Taunts enabled; speaker is a living, conscious non-player critter |
| Taunt line | `random(start, end)` | Chance passed and end >= start |
| Random retarget | `random(0, n - 1)` | Snapshot nonempty |
| Explosive-drop noticing | D10 Perception check per eligible critter | Section 3.4 |

Within one attempt that attacks, draws interleave as: called-shot draws → SPEC-007 attack resolution draws → attack-taunt draws (registered while the attack animation is constructed) → hit/miss taunt draws. An approach attempt draws only its approach taunt. A secondary-mode draw happens at loop entry and again inside each weapon switch that selects a weapon. `[Must-match]`

## 4. Synthetic Test Fixtures (Hex / Text)

All fixtures are synthetic. Names, numbers, and texts are invented and are not taken from game data.

### 4.1 Fixture A: `ai.txt` Grammar, Sorting, and Packet Resolution

File bytes use CRLF line endings, shown here as text lines:

```text
; synthetic packets
[Zeta Guard]
packet_num=0
max_dist=8
min_to_hit=40
min_hp=12 ; low-hp flee
aggression=50
hurt_too_much=Blind, crippled_legs
secondary_freq=4
called_freq=3
font=101
color=145
outline_color=-1
chance=10
chance=35
run_start=1000
move_start=1010
attack_start=1020
miss_start=1030
hit_head_start=1040
hit_left_arm_start=1050
hit_right_arm_start=1060
hit_torso_start=1070
hit_right_leg_start=1080
hit_left_leg_start=1090
hit_eyes_start=1100
hit_groin_start=1110
last_msg=1119
[Empty Header]
[alpha brute]
packet_num=7
max_dist=6 hexes
min_to_hit=25
min_hp=0
aggression=0
hurt_too_much=blind ,crippled
secondary_freq=
called_freq=0
font=101
color=0x50
outline_color=0
chance=100
run_start=2000
move_start=2001
attack_start=2002
miss_start=2003
hit_head_start=2004
hit_left_arm_start=2004
hit_right_arm_start=2004
hit_torso_start=2004
hit_right_leg_start=2004
hit_left_leg_start=2004
hit_eyes_start=2004
hit_groin_start=2004
last_msg=2003
```

Expected:

| Check | Result |
| --- | --- |
| Packet table length | 2 (`Empty Header` has no keys and creates no packet) |
| Table order | index 0 `alpha brute` (7), index 1 `Zeta Guard` (0): case-folded `a` < `z` |
| `Zeta Guard.chance` | 35 (last duplicate wins) |
| `Zeta Guard.min_hp` | 12 (comment stripped, value trimmed) |
| `Zeta Guard.hurt_too_much` | `0x4C` (`0x40` OR `0x0C`; the leading space after the comma is skipped) |
| `alpha brute.max_dist` | 6 (decimal prefix stops at the space) |
| `alpha brute.hurt_too_much` | `0x3C`; the token `blind ` is unrecognized (diagnostic), `crippled` = `0x3C` |
| `alpha brute.secondary_freq` / `called_freq` | 0 / 0, so both gates always draw result 1 (one draw each) |
| `alpha brute.color` | 0 (decimal prefix of `0x50` is `0`; parsing stops at `x`) |
| Resolve packet 0 | `Zeta Guard` |
| Resolve packet 3 (absent) | index 0 = `alpha brute` (packet 7), **not** packet 0 |
| Name query 0 | `Zeta Guard` |
| Name query 7 | nothing: `7 >= table length 2` |
| `alpha brute` hit taunt at groin | range 2004..2003, end < start, so no line (chance draw still consumed) |
| `alpha brute` hit taunt at head | range 2004..2004, ID 2004 |

Exact bytes for one line, showing CR retention before trimming:

```text
63 68 61 6E 63 65 3D 33 35 0D 0A      "chance=35" CR LF -> key "chance", value "35"
5B 45 6D 70 74 79 20 48 65 61 64 65 72 5D 0D 0A   "[Empty Header]" CR LF -> section name "Empty Header"
```

Grammar mutations, each applied alone to Fixture A:

| Mutation | Expected |
| --- | --- |
| Delete `last_msg=1119` | Entire AI initialization fails; no packets |
| Insert `color=[x]` inside `Zeta Guard` | Current section becomes `x`; later keys populate `x`, which lacks keys, so initialization fails |
| Replace header `[alpha brute]` with `[ALPHA BRUTE]` placed after `Zeta Guard` | Same table order as before (case-folded sorting) |
| Insert a second `[zeta guard]` with `chance=1` | Merges into `Zeta Guard`; `chance` = 1; name keeps `Zeta Guard` |
| Keys placed before the first header | `[May-improve]` engine: section `unknown`, which lacks keys, so initialization fails |

### 4.2 Fixture B: Perception Limits

Observer P = 6; `d` as listed:

| Sector | Combat | Target | Limit | `d=3` | `d=7` | `d=15` |
| --- | --- | --- | ---: | --- | --- | --- |
| Forward | any | ordinary | 30 | yes | yes | yes |
| Forward | any | glass (`0x20000`) | 15 | yes | yes | yes |
| Forward | any | sneaking player (working) | 7 | yes | yes | no |
| Forward | any | glass + sneaking player | `floor(15/4)` = 3 | yes | no | no |
| Rear | no | ordinary | 6 | yes | no | no |
| Rear | yes | ordinary | 12 | yes | yes | no |
| Rear | yes | glass (no halving) | 12 | yes | yes | no |
| Rear | yes | sneaking player | 3 | yes | no | no |

### 4.3 Fixture C: Danger-Source Selection

The actor A is on team 1 with Perception 5, in combat, and faces away from everyone (rear-sector limit 10). Snapshot entries, with `dist` from A:

| Entry | Team | Dead | Last attacker | `dist(A,·)` |
| --- | ---: | --- | --- | ---: |
| A | 1 | no | X | 0 |
| B | 2 | no | A | 5 |
| C | 1 | no | D | 2 |
| D | 2 | no | none | 12 |
| E | 2 | no | F | 3 |
| F | 1 | no | none | 9 |
| X | 2 | **yes** | none | 4 |

Trace:

1. H = X is dead and on another team, so t0 = nearest living team-2 entry ≠ A: E (3).
2. Sorted scan from A: C (2) fills t2 = D, since C is a teammate whose attacker D is team 2 and alive. E (3) fills t3 = E, since E's attacker F is team 1. X (4) qualifies for nothing. B (5) fills t1 = B. That is three fills, so the scan stops.
3. Candidates sorted by distance: E (3), E (3), B (5), D (12). The first perceived is E (3 ≤ 10). **Result: E.**

Variants:

| Change | Result |
| --- | --- |
| X alive | X immediately (no perception test, even at distance 4 and even if X were on team 1) |
| A's Perception 1 (limit 2) | Candidates E 3, B 5, D 12 all exceed 2, so nothing |
| Remove E | t0 = B (5, nearest living team 2); t2 = D (via C); t1 = B; t3 = B (B's attacker A is team 1); list B, B, B, D, so B |

### 4.4 Fixture D: Weapon Preference and Inventory Search

Critter: biped, current AP 5, `min_to_hit` 40, every listed weapon has art, and all relevant skills are at least 40. Right hand: nothing. Inventory order:

| Pos | Item | Primary subtype | Raw primary AP | Loaded | Ammo in inventory | Cost |
| ---: | --- | --- | ---: | ---: | --- | ---: |
| 0 | Knife | melee | 3 | n/a | n/a | 40 |
| 1 | Pistol | ranged | 4 | 0 | none compatible | 250 |
| 2 | Rock | throw | 3 | n/a | n/a | 0 |
| 3 | Rifle | ranged | 7 | 5 | n/a | 1000 |
| 4 | SMG-A | ranged | 5 | 10 | n/a | 1000 |
| 5 | SMG-B | ranged | 5 | 10 | n/a | 1000 |

With the AP check: the pistol is excluded (empty, no ammo) and the rifle is excluded (7 > 5). The fold runs: none→Knife; Knife vs Rock gives Rock (throw beats melee); Rock vs SMG-A gives SMG-A (ranged); SMG-A vs SMG-B is a cost tie, so SMG-A is kept. **Result: SMG-A.** Without the AP check the rifle (position 3) is eligible, so the fold becomes Knife → Rock → Rifle (ranged), then Rifle vs SMG-A and Rifle vs SMG-B are cost ties that keep the incumbent. **Result: Rifle.** The pistol stays excluded in both cases.

Weapon cost with loaded ammunition: base 200, 10 rounds loaded of an ammunition prototype costing 50 per pack of 20 gives `200 + floor(10 x 50 / 20) = 225`.

Usability: with left arm crippled (`0x10`), a two-handed rifle is unusable and a one-handed pistol is usable. With effective Small Guns 39 and `min_to_hit` 40, every small-gun weapon is unusable regardless of hit chance.

### 4.5 Fixture E: Hit Mode, Called Shot, and Draw Accounting

Packet: `secondary_freq` 4, `called_freq` 3, `min_to_hit` 40. Difficulty 1 (normal, so minimum Intelligence 5). The critter has Intelligence 6. Weapon: spear, primary thrust (melee), secondary throw; usable in both modes; no other inventory weapon. AP is sufficient for aimed attacks, and aiming is available.

| Step | Draw | Value | Outcome |
| --- | --- | ---: | --- |
| Secondary gate | `random(1,4)` | 1 | Candidate secondary |
| Throw retention | D10 vs Int 6 | 4 | Success, so primary (mode 2) kept |
| Called gate | `random(1,3)` | 1 | Proceed |
| Location | `random(0,8)` | 0 (head) | Head hit chance 32 < 40, so torso, unaimed |

Same draws, but the location draw gives 8: the result is uncalled (8), an unaimed attack resolved to torso later. With location 1 (left arm) and hit chance 41, the result is location 1, aimed (+1 AP at debit).

With the D10 result 7 (> Int 6, failure), mode 3 (throw) is selected. With a second weapon in inventory, the D10 draw is **not** made and mode 3 is selected.

### 4.6 Fixture F: Flee Decisions

Packet `max_dist` 8. Critter AP 4.

| `dist(self, player)` | Expected |
| ---: | --- |
| 7 | Set `0x04`; try N = 4 (D, D+1, D+5), then N = 3, ...; run to the first strict-goal reachable candidate |
| 8 | Set `0x02` only; no movement, no taunt |
| 7, AP 0 | Set `0x04`; no candidates, so no movement and no taunt |

For the candidate order with D = 1 (east): (4, dir 1), (4, dir 2), (4, dir 0), (3, dir 1), (3, dir 2), (3, dir 0), and so on down to N = 1.

### 4.7 Fixture G: Taunt Ranges (Fixture A `Zeta Guard`, chance 35)

| Type / location | Range (inclusive) | Note |
| --- | --- | --- |
| Run | 1000-1010 | 1010 is shared with Move |
| Move | 1010-1020 | |
| Attack | 1020-1030 | |
| Miss | 1030-1040 | 1040 is shared with head |
| Hit head (0) | 1040-1050 | |
| Hit torso (3) | 1070-1080 | |
| Hit groin (7) | 1110-1119 | Ends at `last_msg` |

The chance draw must be ≤ 35 to continue. A ranged attack that hits location 4 (right leg) on a defender using this packet makes the **defender** draw within 1080-1090.

### 4.8 Fixture H: Retaliation Rating

| Critter | Melee Damage | Right/left weapon max damage | Effective AC | Rating |
| --- | ---: | --- | ---: | ---: |
| Current reference Q | 3 | right 12 | 10 | 22 |
| Candidate R1 | 5 | none | 20 | 25, so switch to R1 |
| Candidate R2 | 2 | left 20 | 0 | 22, a tie, so keep Q |
| Candidate R3 (knocked out) | 9 | right 30 | 40 | 0, so keep Q |

### 4.9 Fixture I: Healing Loop

Biped, effective max HP 41, so B = 20. Current HP 15, AP 5. Inventory: [pos 0 drug PID `0x00000035`, pos 1 Stimpak ×2].

1. HP 15 < 20 and AP 5 ≥ 2. Find a drug: position 0 is not a stimpak, so continue.
2. HP 15 < 20. Find the next drug: position 1 is a stimpak. Remove one, consume it (assume +8 HP, giving 23), animate, print 5000, destroy the unit, AP 5 → 3, and reset the position.
3. HP 23 ≥ 20, so stop. Final AP 3, one stimpak left.

Variant: if consumption fails each time, AP goes 5 → 3 → 1, then stops on AP < 2. HP stays 15 and both stimpaks remain (each forced back).

### 4.10 Fixture J: Attack Loop Trace

Pistol in the right hand (primary cost 4, loaded), AP 8, preflight 0, range-aware hit chance 55 ≥ 40. There is no requested target, and the danger source is the target.

| Attempt | Preflight | Action | AP after |
| ---: | --- | --- | ---: |
| 1 | 0 | Attack | 4 |
| 1 (post) | n/a | Cost 4 ≤ 4, so continue | 4 |
| 2 | 0 | Attack | 0 |
| 2 (post) | n/a | Cost 4 > 0, so return failure | 0 |

After the loop, AP is 0, so the max-distance rule (section 3.6 step 4) is skipped. With range-aware chance 30 and no-range chance 45 at `dist` 6, the critter approaches (taunt allowed once). With no-range chance 35, it flees immediately and the loop ends.

## 5. Clean-Room Verification Checklist
- [x] Zero C/C++ code snippets or struct definitions included
- [x] All multi-byte integer endianness explicitly stated
- [x] Synthetic test vectors included
- [x] Every documented quirk tagged `[Must-match]` or `[May-improve]`
- [x] No internal reference symbol names; only public SSL opcode names and file-format key literals
- [x] Spec linter run against `fallout1-re` symbols with no findings

Verification notes: `tools/spec_lint.py` gives 0 errors. The 25 `ai.txt` packet keys and the `fallout.cfg` preference keys are public file-format literals on the linter allowlist. Its 38 plain-word warnings were reviewed and are benign:
- the default section name "unknown";
- the prose phrases "damage flags AND mask", "length", "capacity", "rounds" and "current";
- this spec's own notation "danger", "self", "target", "cost" and "start";
- the synthetic fixture packet names "alpha" and "guard".
