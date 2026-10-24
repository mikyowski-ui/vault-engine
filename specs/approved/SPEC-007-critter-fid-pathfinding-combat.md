# SPEC-007: Critter Art, Hex Routing, and Combat Resolution

## Clean-Room Rules for Research Agents
- Document ONLY binary file layouts (byte offsets, endianness, data types), mathematical formulas, state transitions, and observable behavior.
- DO NOT copy C/C++ code, struct definitions, function names, internal variable names, or implementation-source paths from the inspected implementation.
- Provide small synthetic hex/text test vectors at the bottom of the specification.

## 1. Overview & Scope

This specification describes reference revision R1, targeting the 32-bit Windows engine. Evidence is static inspection of art lookup, movement, ray traversal, combat, item/stat consumers, and numeric critical-effect data. Synthetic checks use independent arithmetic/state models, not original game assets or a live game session.

SPEC-001 and SPEC-003 define sprite bytes, prototype/map files, and screen/hex projection; SPEC-006 defines effective stat/skill and random-check calculations. This document adds the complete art-identifier codec and combat/routing rules. Public host mnemonics such as `obj_can_see_obj` identify observable interfaces, not native routine names.

Important distinctions: a directional-file selector is not a critter's zero-based facing; a six-character critter stem normally produces eight filename characters but six characters are not enforced; the route search marks hexes seen on insertion and does not perform full cost relaxation. The inspected damage/hit pipeline does not consume the stored ammunition AC/DR/multiplier/divisor modifiers. `[Must-match]` Recovered omissions and defects below are not replaced with assumed rules from another engine version.

**Compatibility tags.** Every documented quirk carries one tag:

- `[Must-match]`: gameplay-visible behavior, data grammar, formula, ordering, or random-draw sequence. A clean-room engine must reproduce it exactly.
- `[May-improve]`: reference memory-safety defects, undefined behavior, uninitialized reads, stale state, presentation-only transients, or a lookup that reads the wrong table in an obviously unintended way. A clean-room engine may apply the stated safe rule and should keep every other observable behavior. An engine that wants bit-exact parity may offer a compatibility switch that keeps the reference behavior.

### Revision history

- **R1-rev2 (2026-10-18).** Section 3.6 now gives the exact draw range and position of the punch-to-kick draw. Section 3.7 now gives the complete missed-shot bystander algorithm: the line-of-fire stage, the impact fallback, victim selection, hit location, damage, and draw order. Fixtures D and E are added. The raw revision hash was replaced by the label R1.
- **R1-rev3 (2026-10-19).** Section 3.6 now holds the single authoritative intended-target, onlooker and retaliation rule, which SPEC-009 cites. A surviving conscious unintended victim, whether a missed-shot bystander or a critical-failure random target, makes the retaliation check only when its team differs from the attacker's. Section 3.7 now states that the disabled splash enumeration serves every explosion, so no explosion damages beyond its center target, which matches SPEC-013 §3.10.
- **R1-rev9 (2026-10-22).** Section 3.3: the combat-request AP bonus is always 0 in R1, because no request source sets it. Section 3.4: the UI aiming flag in reload mode, and the called-shot window of SPEC-014 §3.38. New Fixture I gives the spray draw order with Sniper, Jinxed, an extra-only hit and a critical failure.
- **R1-rev10 (2026-10-23).** New section 3.12 Combat Message Lines: the full line order, names and message bases, the status list, and the Brief/Verbose difference (only the struck-target tail of a critical hit). The status list's last flag is looked up at the bare bit number and prints nothing (`[May-improve]` safe rule). New Fixture J.
- **R1-rev8 (2026-10-21).** Resolves two engine-audit errata:
  - Section 3.9: a throw whose projectile phase does not run still registers step 11, which always changes the attacker to weapon code 0 because there is no refill item; the dodge is registered only for an invalid projectile, never for a critical failure that keeps its critical bit. A `[May-improve]` keeps the held item's weapon code. Section 3.10.4 step 11 points to the new rule.
  - Section 3.7: the Stage A pass inside a spray, Jinxed/Sniper post-processing once per spray on the final result (no Slayer, no per-target processing), the extra-hits-only outcome with a `[May-improve]`, and the full spray draw order. Section 3.6 states that Sniper is ranged-subtype only, never throw.
- **R1-rev7 (2026-10-20).** Answers four Builder questions from the movement and interaction pass:
  - Section 3.1: a move to an object never tests the target's hex for a blocker, so a target that shares its hex with a wall is reached from an adjacent hex.
  - Section 3.10.1: new "One sequence per owner" rule.
  - Section 3.11.3: the container reach timing rule now matches SPEC-013 §3.16. A new "Adjacency check" paragraph states when `misc.msg` 2000 prints.
  - Section 3.11.3 also points to SPEC-014 §3.27 for player clicks while a sequence runs.
- **R1-rev6 (2026-10-19).**
  - New section 3.11 covers non-combat animation: walking and running art, when the engine runs, and the per-frame step model with its hex crossing rule; idle fidgets; interaction reaches for pickup, use, containers and ladders; door and container opening and closing, with `use_p_proc` order and when the blocking flags change; and knockback and getting up outside combat.
  - The section also corrects several suffix assumptions: walking is `ab`, `am` is the high reach, `af` is falling, and `ap` is hit from behind.
  - New Fixture H.
- **R1-rev5 (2026-10-19).**
  - New section 3.10 gives the attack animation choreography: the animation start rule and its delays; the attack animation, sounds and action frame for each attack mode; the melee and ranged sequences; projectile flight timing; and every reaction, from dodge, flinch, knockdown and knockback to death, blood and fire.
  - Section 3.9 answers two Builder questions. The projectile hit-sound delay needs no clamp because nothing intercepts a projectile in flight. A landed item keeps the light it was given at launch, where the requested radius 9 becomes 8, or 0 without intensity.
  - Section 3.8 now states that blood plays after a fall rather than replacing it.
  - New Fixture G; Fixture F gains row F7.
- **R1-rev4 (2026-10-19).** New section 3.9 gives thrown-weapon consumption (one unit, hand refill), the projectile flight sequence from the attacker's adjacent hex to the defender or impact hex, and what remains on the map. New Fixture F.

## 2. Binary Layout / Data Format (Byte Offset Tables)

### 2.1 Complete 32-Bit Art Identifier

An art identifier, abbreviated FID, occupies four bytes. In prototype/map/save fields serialized through the resource integer readers/writers it is a **big-endian 32-bit word**; it is not four independent little-endian bytes. `[Must-match]` Bit positions below refer to the numeric word, with bit 0 least significant.

| Bits | Mask | Meaning |
| --- | --- | --- |
| 0-11 | `0x00000FFF` | Zero-based base-art list index |
| 12-15 | `0x0000F000` | Critter weapon-animation code, or head fidget variant; otherwise not used by ordinary filename lookup |
| 16-23 | `0x00FF0000` | Animation code; head animation selector for the head category |
| 24-27 | `0x0F000000` | Art category |
| 28-30 | `0x70000000` | Critter directional-file selector; not the sprite's in-file direction index |
| 31 | `0x80000000` | Not used by valid category/filename decoding; normal construction leaves it clear |

For ordinary supported values, numeric encoding is base index plus `4096 * weapon/variant + 65536 * animation + 16777216 * category + 268435456 * file selector`. Construction masks base to 12 bits, weapon/variant to 4, animation to 8, and selector to 3. It does not validate every semantic field before packing. Filename decoding ignores bit 31; alias reconstruction can discard it. `[Must-match]`

| Category | Art directory / list stem | Filename interpretation |
| ---: | --- | --- |
| 0 | `items` | Use selected list filename verbatim |
| 1 | `critters` | Base stem plus two-letter codec and selected extension |
| 2 | `scenery` | List filename verbatim |
| 3 | `walls` | List filename verbatim |
| 4 | `tiles` | List filename verbatim |
| 5 | `misc` | List filename verbatim |
| 6 | `intrface` | List filename verbatim |
| 7 | `inven` | List filename verbatim |
| 8 | `heads` | Stem plus head suffix and, for fidgets, decimal variant |
| 9 | `backgrnd` | List filename verbatim |
| 10 | `skilldex` | Additional supported category; list filename verbatim |

Thus the animation/variant bits do not universally append suffixes to every art category. Normal non-critter construction clears the directional selector. A raw identifier of another category can retain such bits, but ordinary filename lookup does not use them. `[Must-match]` Invalid categories/list indexes are not comprehensively bounds-checked before all accesses: alias lookup and the list-length test can read a category's list before the category range is checked. `[May-improve]`: produce no filename for a category above 10 or a base index at or beyond that category's list length, before any list access.

Head animation selectors 0-11 append, respectively, `gv, gf, gn, ng, nf, nb, bn, bf, bv, gp, np, bp`, then `.frm`. Selectors 1, 4, 7 additionally append the decimal value of bits 12-15 after their two letters. `[Must-match]` There is no zero padding and no guarantee of an eight-character head filename. `[Must-match]` Other head selector values are not safely bounded by the suffix lookup. `[May-improve]`: produce no filename for a head selector above 11.

### 2.2 Critter List Rows, Weapon Letters, and Animation Suffixes

The logical list is `art/critters/critters.lst`. A successful text-line read consumes one zero-based index; blank/comment-like lines are not skipped. Reads are bounded to 199 bytes including a returned LF, so an overlong physical line can consume multiple indexes. CRLF is converted to LF in text mode; a final unterminated line still counts. `[Must-match]`

The base token ends at the first space, comma, semicolon, TAB, or LF and is truncated to 12 bytes. Leading delimiters produce an empty stem. There is no general leading-whitespace trim, hash-comment removal, or extension stripping. Case of the stored stem is preserved. `[Must-match]`

In a separate pass, the first comma introduces one signed decimal alias index, allowing ordinary leading whitespace/sign and stopping at the first nondigit; no digits means zero. Additional comma fields do not form an alias array and are not independently consumed. Without a comma, the default character base index at list initialization is used, initially zero. A conventional row is `six-character stem,alias index`, but neither six characters nor a particular total line width is enforced. `[Must-match]`

Alias substitution occurs only for animation codes `27,29,30,33,55,57,58,64`. Substitute the selected row's alias, masked to 12 bits, while preserving category 1, animation, weapon code, and directional selector. This is one substitution, not recursive alias chasing, and is not a generic missing-file fallback. `[Must-match]`

Weapon-animation codes must be 0-10 for the critter filename codec:

| Code | Meaning | Weapon letter when applicable |
| ---: | --- | --- |
| 0 | No weapon animation | No weapon letter |
| 1 | Knife | d |
| 2 | Club | e |
| 3 | Hammer | f |
| 4 | Spear | g |
| 5 | Pistol | h |
| 6 | SMG | i |
| 7 | Shotgun | j |
| 8 | Laser rifle | k |
| 9 | Minigun | l |
| 10 | Launcher | m |

Let the weapon letter be the corresponding character above. The exact two-letter critter suffix is:

| Animation code | Suffix rule |
| --- | --- |
| 0 stand | `aa` with weapon 0; weapon letter then `a` otherwise |
| 1 walk | `ab` with weapon 0; weapon letter then `b` otherwise |
| 2-12 | `a` then consecutive letter `c` through `m`; weapon code does not alter it |
| 13 dodge | `an` with weapon 0; weapon letter then `e` otherwise |
| 14-17 | `ao, ap, aq, ar`, respectively |
| 18 throw | `dm` for knife 1; `gm` for spear 4; `as` for every other supported weapon code |
| 19 run | `at` |
| 20-35 | `b` then letters `a` through `p`, in animation-code order |
| 36 prone-to-standing | `ch` |
| 37 back-to-standing | `cj` |
| 38-47 | Weapon letter then letters `c` through `l`; weapon 0 is rejected |
| 48-63 | `r` then letters `a` through `p`, in animation-code order |
| 64 called-shot image | `na`, taking precedence over the 48-and-higher rule |

Codes 20-35 name fall-back, fall-front, bad landing, big hole, charred body, chunks, autofire dance, electrify, sliced-in-half, burned-to-nothing, electrified-to-nothing, exploded-to-nothing, melted-to-nothing, fire dance, bloody fall-back, bloody fall-front. Codes 48-63 are their final/static counterparts in that same order. `[Must-match]` The weapon-letter, suffix, and weapon-action tables in this section are exact data. `[Must-match]`

| Animation code | Weapon action | Second suffix letter |
| ---: | --- | --- |
| 38 | Take out | c |
| 39 | Put away | d |
| 40 | Parry | e |
| 41 | Thrust | f |
| 42 | Swing | g |
| 43 | Point | h |
| 44 | Unpoint | i |
| 45 | Single fire | j |
| 46 | Burst | k |
| 47 | Continuous fire | l |

The weapon-code validity check precedes every animation branch: weapon codes 11-15 fail even for a death animation whose suffix otherwise ignores the weapon. `[Must-match]` Animation 0-64 is the supported domain; the arithmetic suffix branches do not provide a comprehensive safe rejection rule for arbitrary larger bytes, and codes above 64 produce second letters past `p`. `[May-improve]`: produce no filename for a critter animation code above 64.

### 2.3 Directional File Selection and Fallback

After aliasing and suffix selection, concatenate the selected base stem and the two letters. A six-character stem gives eight characters before the dot. Selector 0 appends `.frm`; selectors 1-6 append `.fr0` through `.fr5`. Physical orientations 0-5 are northeast, east, southeast, southwest, west, northwest; ordinary directional callers pass **orientation plus 1** as the file selector. A raw selector 7 formats `.fr6`, not a seventh valid sprite-facing direction. `[Must-match]`

Filename resolution itself simply honors the encoded selector. The normal FID constructor additionally applies resource-existence fallback:

1. For non-critters, animation below 20 or above 35, or animation 33 fire dance, encode selector 0 immediately; do not probe directional files. `[Must-match]`
2. For critter animation 20-35 except 33, probe the requested selector's filename, using alias substitution when applicable. If present, retain that selector. `[Must-match]`
3. If missing and the requested selector was 1, encode 0. Otherwise probe selector 1 (`.fr0`); if present retain 1, else encode 0. `[Must-match]`

The final `.frm` fallback is not guaranteed to exist. Requested selector 0 can also be probed first for eligible death animation; if that `.frm` is missing the constructor can select `.fr0`. `[Must-match]` It does not scan all six directions, switch weapon letters, substitute standing art, or chase another alias when a resource is missing. The returned FID retains the original base index even when filename lookup resolves through its alias. `[Must-match]`

### 2.4 Combat Encodings and Numeric Table Domains

Combat state bits are 32-bit values: `0x01` active combat, `0x02` player-input gate, `0x08` accepted end-combat request. Initial/out-of-combat state normally has the input-gate bit set, so it is not independently proof that combat is active. `[Must-match]`

| Hit mode | Meaning |
| ---: | --- |
| 0 / 1 | Left-hand primary / secondary |
| 2 / 3 | Right-hand primary / secondary |
| 4 / 5 | Punch / kick |
| 6 / 7 | Left-hand reload / right-hand reload |

The inspected item path does not provide a separate advanced-unarmed AP/damage ladder for the additional enumerated mode values 8-19. Ordinary punch/kick costs and weapon selection below must not be replaced with such a ladder. `[Must-match]`

| Hit location | Meaning | Accuracy adjustment, ranged/throw |
| ---: | --- | ---: |
| 0 | Head | -40 |
| 1 | Left arm | -30 |
| 2 | Right arm | -30 |
| 3 | Torso | 0 |
| 4 | Right leg | -20 |
| 5 | Left leg | -20 |
| 6 | Eyes | -60 |
| 7 | Groin | -30 |
| 8 | Uncalled | 0 |

For unarmed/melee use half the listed adjustment, truncating toward zero. Uncalled is normalized to torso after its initial accuracy calculation and before the normal critical-effect lookup; the stored uncalled effect rows nevertheless differ from torso in some NPC tables. `[Must-match]`

Effect flags are separate from the object's basic flags and maneuver flags:

| Hex mask | Effect |
| --- | --- |
| `1 / 2` | Knocked out / knocked down |
| `4 / 8` | Cripple left leg / right leg |
| `10 / 20 / 40 / 80` | Cripple left arm / right arm / blind / dead |
| `100 / 200` | Attack hit / critical |
| `400 / 800` | On fire / armor bypass |
| `1000 / 2000 / 4000` | Weapon explodes / destroyed / dropped |
| `8000` | Lose turn |
| `10000 / 20000 / 40000` | Hit self / lose ammunition / dud |
| `80000 / 100000 / 200000` | Additional self injury / random target / random cripple |
| `400000 / 800000` | Explosion backwash / reverse-animation marker |

All masks in that table are hexadecimal, including digit-only values. Persistent critter status retains only knockout, knockdown, cripple/blind, dead, and lose-turn bits. Maneuver `0x01` means alerted/participating, `0x02` stop-attacking, `0x04` fleeing; these are not the effect-flag meanings. `[Must-match]`

Critical-success tier selection uses integer `D100 + effective Better Criticals stat`: values at most 20, 45, 70, 90, 100, or greater select tiers 0-5 respectively. The failure selection score is `D100 - 5 * (effective Luck - 5)`: values at most 20, 50, 75, 95, or greater select columns 0-4 of this complete hexadecimal mask table. `[Must-match]`

| Failure table number | 0 | 1 | 2 | 3 | 4 |
| ---: | --- | --- | --- | --- | --- |
| 0 | 0 | 8000 | 8000 | 80002 | 200000 |
| 1 | 0 | 8000 | 4000 | 100000 | 10000 |
| 2 | 0 | 20000 | 4000 | 100000 | 2000 |
| 3 | 8000 | 28000 | C000 | 100000 | 9000 |
| 4 | 40000 | 4000 | 84000 | 100000 | 1000 |
| 5 | 8000 | 40000 | 2000 | 100000 | 9002 |
| 6 | 0 | 8000 | 100000 | 2000 | 9400 |

The weapon prototype selects failure-table number; no weapon/selector -1 uses table 0. `[Must-match]` Other table numbers are not range-checked before the lookup. `[May-improve]`: treat any failure-table number outside 0-6 as table 0.

### 2.5 Complete Critical-Success Effect Matrices

These are built-in mathematical lookup data, not another on-disk resource format or a serialized native-memory record. Each slash-separated entry contains seven values in order: damage scale numerator, ordinary effect mask, saving-stat number, saving-stat adjustment, extra mask on failed save, ordinary message number, failed-save message number. **Only the second and fifth values are hexadecimal**; all other values are decimal. `=` repeats the entire entry immediately to its left in the same row. `[Must-match]`

Damage scale is divided by 2 in the damage formula: numerator 3 means 1.5 times before subsequent integer truncation. Saving-stat -1 means no save; otherwise roll D10 against effective stat plus the adjustment, applying the extra mask and alternate message on failure. Stat numbers used here are 0 strength, 2 endurance, 5 agility, 6 luck. Flags/messages are preserved exactly, including anomalous rows; no symmetry or message-number correction is implied. `[Must-match]`

Victim table numbers 0-14 are man, woman, child, super mutant, ghoul, brahmin, radscorpion, rat, floater, centaur, robot, dog, mantis, deathclaw, plant. NPC selection uses prototype kill category, not inferred gender/body shape. P selects the separate player matrix irrespective of gender. Location numbers are those in section 2.4. All 864 entries, including uncalled location 8, are specified below. `[Must-match]`

| Victim | Location | Tier 0 | Tier 1 | Tier 2 | Tier 3 | Tier 4 | Tier 5 |
| --- | ---: | --- | --- | --- | --- | --- | --- |
| 0 | 0 | 4/0/-1/0/0/5001/5000 | 4/800/2/0/1/5002/5003 | 5/800/2/-3/1/5002/5003 | 5/802/2/-3/1/5004/5003 | 6/801/6/0/40/5005/5006 | 6/80/-1/0/0/5007/5000 |
| 0 | 1 | 3/0/-1/0/0/5008/5000 | 3/8000/-1/0/0/5009/5000 | 4/0/2/-3/10/5010/5011 | 4/810/-1/0/0/5012/5000 | = | 4/810/-1/0/0/5013/5000 |
| 0 | 2 | 3/0/-1/0/0/5008/5000 | 3/8000/-1/0/0/5009/5000 | 4/0/2/-3/20/5014/5000 | 4/820/-1/0/0/5015/5000 | = | 4/820/-1/0/0/5013/5000 |
| 0 | 3 | 3/0/-1/0/0/5016/5000 | 3/800/-1/0/0/5017/5000 | 4/802/-1/0/0/5019/5000 | = | 6/801/-1/0/0/5020/5000 | 6/80/-1/0/0/5021/5000 |
| 0 | 4 | 3/2/-1/0/0/5023/5000 | 3/2/2/0/8/5023/5024 | 4/2/2/-3/8/5023/5024 | 4/80A/-1/0/0/5025/5000 | 4/80A/2/0/1/5025/5026 | 4/809/-1/0/0/5026/5000 |
| 0 | 5 | 3/2/-1/0/0/5023/5000 | 3/2/2/0/4/5023/5024 | 4/2/2/-3/4/5023/5024 | 4/806/-1/0/0/5025/5000 | 4/806/2/0/1/5025/5026 | 4/805/-1/0/0/5026/5000 |
| 0 | 6 | 4/0/6/4/40/5027/5028 | 4/800/6/3/40/5029/5028 | 6/800/6/2/40/5029/5028 | 6/8840/-1/0/0/5030/5000 | 8/841/-1/0/0/5031/5000 | 8/80/-1/0/0/5032/5000 |
| 0 | 7 | 3/0/-1/0/0/5033/5000 | 3/800/2/-3/2/5034/5035 | 3/2/2/-3/1/5035/5036 | 3/1/-1/0/0/5036/5000 | 4/802/2/0/1/5035/5036 | 4/801/-1/0/0/5037/5000 |
| 0 | 8 | 3/0/-1/0/0/5016/5000 | 3/800/-1/0/0/5017/5000 | 4/0/-1/0/0/5018/5000 | 4/802/-1/0/0/5019/5000 | 6/801/-1/0/0/5020/5000 | 6/80/-1/0/0/5021/5000 |
| 1 | 0 | 4/0/-1/0/0/5101/5100 | 4/800/2/0/1/5102/5103 | 6/800/2/-3/1/5102/5103 | 6/802/2/-3/1/5104/5103 | 6/801/6/0/40/5105/5106 | 6/80/-1/0/0/5107/5000 |
| 1 | 1 | 3/0/-1/0/0/5108/5100 | 3/8000/-1/0/0/5109/5100 | 4/0/2/-2/10/5110/5111 | 4/0/2/-4/10/5110/5111 | 4/810/-1/0/0/5112/5100 | 4/810/-1/0/0/5113/5100 |
| 1 | 2 | 3/0/-1/0/0/5108/5100 | 3/8000/-1/0/0/5109/5100 | 4/0/2/-2/20/5114/5100 | 4/0/2/-4/20/5114/5100 | 4/820/-1/0/0/5115/5100 | 4/820/-1/0/0/5113/5100 |
| 1 | 3 | 3/0/-1/0/0/5116/5100 | 3/800/-1/0/0/5117/5100 | 4/802/-1/0/0/5119/5100 | = | 6/801/-1/0/0/5120/5100 | 6/80/-1/0/0/5121/5100 |
| 1 | 4 | 3/2/-1/0/0/5123/5100 | 4/2/2/0/8/5123/5124 | 4/2/2/-3/8/5123/5124 | 4/80A/-1/0/0/5125/5100 | 4/80A/2/0/1/5125/5126 | 4/809/-1/0/0/5126/5100 |
| 1 | 5 | 3/2/-1/0/0/5123/5100 | 4/2/2/0/4/5123/5124 | 4/2/2/-3/4/5123/5124 | 4/806/-1/0/0/5125/5100 | 4/806/2/0/1/5125/5126 | 4/805/-1/0/0/5126/5100 |
| 1 | 6 | 4/0/6/4/40/5127/5128 | 4/800/6/3/40/5129/5128 | 6/800/6/2/40/5129/5128 | 6/8840/-1/0/0/5130/5100 | 8/841/-1/0/0/5131/5100 | 8/80/-1/0/0/5132/5100 |
| 1 | 7 | 3/0/-1/0/0/5133/5100 | 3/0/2/0/2/5133/5134 | 3/2/2/0/1/5134/5135 | 3/1/-1/0/0/5135/5100 | 4/802/2/0/1/5134/5135 | 4/801/-1/0/0/5135/5100 |
| 1 | 8 | 3/0/-1/0/0/5116/5100 | 3/800/-1/0/0/5117/5100 | 4/802/-1/0/0/5119/5100 | = | 6/801/-1/0/0/5120/5100 | 6/80/-1/0/0/5121/5100 |
| 2 | 0 | 4/0/2/0/1/5200/5201 | 4/800/2/-2/1/5202/5203 | = | 6/801/-1/0/0/5203/5000 | = | 6/80/-1/0/0/5204/5000 |
| 2 | 1 | 3/0/-1/0/0/5205/5000 | 4/8000/2/0/10/5206/5207 | 4/8000/2/-2/10/5206/5207 | 4/810/-1/0/0/5208/5000 | = | = |
| 2 | 2 | 3/0/-1/0/0/5209/5000 | 4/8000/2/0/20/5206/5207 | 4/8000/2/-2/20/5206/5207 | 4/820/-1/0/0/5208/5000 | = | = |
| 2 | 3 | 3/0/-1/0/0/5210/5000 | 4/800/-1/0/0/5211/5000 | 4/802/-1/0/0/5212/5000 | = | 4/801/-1/0/0/5213/5000 | 6/80/-1/0/0/5214/5000 |
| 2 | 4 | 3/0/-1/0/0/5215/5000 | 3/A/-1/0/1460/5000/0 | = | 4/80A/-1/0/0/5217/5000 | = | = |
| 2 | 5 | 3/0/-1/0/0/5215/5000 | 3/6/-1/0/1460/5000/0 | = | 4/806/-1/0/0/5217/5000 | = | = |
| 2 | 6 | 4/0/6/5/40/5218/5219 | 4/800/6/2/40/5220/5221 | 6/800/6/-1/40/5220/5221 | 6/8840/-1/0/0/5222/5000 | 8/841/-1/0/0/5223/5000 | 8/80/-1/0/0/5224/5000 |
| 2 | 7 | 3/0/-1/0/0/5225/5000 | = | 3/2/-1/0/0/5226/5000 | = | = | 4/2/-1/0/0/5226/5000 |
| 2 | 8 | 3/0/-1/0/0/5210/5000 | 3/800/-1/0/0/5211/5000 | 4/800/-1/0/0/5211/5000 | 4/802/-1/0/0/5212/5000 | 4/801/-1/0/0/5213/5000 | 6/80/-1/0/0/5214/5000 |
| 3 | 0 | 4/0/-1/0/0/5300/5000 | 4/800/2/-1/2/5301/5302 | 5/800/2/-4/2/5301/5302 | 5/802/2/-3/1/5302/5303 | 6/802/2/0/1/5302/5303 | 6/80/-1/0/0/5304/5000 |
| 3 | 1 | 3/0/-1/0/0/5300/5000 | 3/0/5/0/8000/5300/5306 | 4/8800/2/-1/10/5307/5308 | 4/8800/2/-3/10/5307/5308 | 4/810/-1/0/0/5308/5000 | = |
| 3 | 2 | 3/0/-1/0/0/5300/5000 | 3/0/5/0/8000/5300/5006 | 4/8800/2/-1/20/5307/5309 | 4/8800/2/-3/20/5307/5309 | 4/820/-1/0/0/5309/5000 | = |
| 3 | 3 | 3/0/-1/0/0/5300/5000 | 3/800/-1/0/0/5301/5000 | 4/802/-1/0/0/5302/5000 | = | 4/801/-1/0/0/5310/5000 | 6/80/-1/0/0/5311/5000 |
| 3 | 4 | 3/0/-1/0/0/5300/5000 | 3/0/5/0/2/5300/5312 | 4/2/2/-3/8/5312/5313 | 4/A/-1/0/0/5313/5000 | 4/80A/-1/0/0/5314/5000 | 4/809/-1/0/0/5315/5000 |
| 3 | 5 | 3/0/-1/0/0/5300/5000 | 3/0/5/0/2/5300/5312 | 4/2/2/-3/4/5312/5313 | 4/6/-1/0/0/5313/5000 | 4/806/-1/0/0/5314/5000 | 4/805/-1/0/0/5315/5000 |
| 3 | 6 | 4/0/-1/0/0/5300/5000 | 4/800/6/5/40/5316/5317 | 6/800/6/3/40/5316/5317 | 6/8800/6/0/40/5318/5319 | 8/841/-1/0/0/5320/5000 | 8/80/-1/0/0/5321/5000 |
| 3 | 7 | 3/0/-1/0/0/5300/5000 | 3/0/6/0/800/5300/5017 | 3/800/2/-2/2/5301/5302 | 3/2/-1/0/0/5312/5000 | 4/802/2/0/1/5302/5303 | 4/801/-1/0/0/5303/5000 |
| 3 | 8 | 3/0/-1/0/0/5300/5000 | 3/800/-1/0/0/5301/5000 | 4/802/-1/0/0/5302/5000 | = | 4/801/-1/0/0/5310/5000 | 6/80/-1/0/0/5311/5000 |
| 4 | 0 | 4/0/-1/0/0/5001/5000 | 4/800/2/0/1/5400/5003 | 5/800/2/-1/1/5400/5003 | 5/802/2/-2/1/5004/5005 | 6/801/0/0/0/5005/5000 | 6/80/-1/0/0/5401/5000 |
| 4 | 1 | 3/0/-1/0/0/5016/5000 | 3/0/5/0/C000/5001/5402 | 4/C000/2/0/10/5402/5012 | 4/C800/2/0/10/5403/5404 | 4/4810/-1/0/0/5404/5000 | = |
| 4 | 2 | 3/0/-1/0/0/5016/5000 | 3/0/5/0/C000/5001/5402 | 4/C000/2/0/20/5402/5015 | 4/C800/2/0/20/5403/5404 | 4/4820/-1/0/0/5404/5000 | = |
| 4 | 3 | 3/0/-1/0/0/5001/5000 | 3/800/-1/0/0/5017/5000 | 3/0/-1/0/0/5018/5000 | 4/802/-1/0/0/5004/5000 | 4/801/-1/0/0/5003/5000 | 6/80/-1/0/0/5007/5000 |
| 4 | 4 | 3/0/-1/0/0/5001/5000 | 3/0/5/0/2/5001/5023 | 4/2/2/0/8/5023/5024 | 4/A/-1/0/0/5024/5000 | 4/80A/2/0/1/5024/5026 | 4/809/-1/0/0/5026/5000 |
| 4 | 5 | 3/0/-1/0/0/5001/5000 | 3/0/5/0/2/5001/5023 | 4/2/2/0/4/5023/5024 | 4/6/-1/0/0/5024/5000 | 4/806/2/0/1/5024/5026 | 4/805/-1/0/0/5026/5000 |
| 4 | 6 | 4/0/6/3/40/5001/5405 | 4/800/6/0/40/5406/5407 | 6/800/6/-3/40/5406/5407 | 6/8840/-1/0/0/5030/5000 | 8/841/-1/0/0/5031/5000 | 8/80/-1/0/0/5408/5000 |
| 4 | 7 | 3/0/-1/0/0/5001/5000 | 3/0/6/0/800/5001/5033 | 3/800/2/0/2/5033/5035 | 3/2/-1/0/0/5004/5000 | 4/802/2/0/1/5035/5036 | 4/801/-1/0/0/5036/5000 |
| 4 | 8 | 3/0/-1/0/0/5001/5000 | 3/800/-1/0/0/5017/5000 | 3/0/-1/0/0/5018/5000 | 4/802/-1/0/0/5004/5000 | 4/801/-1/0/0/5003/5000 | 6/80/-1/0/0/5007/5000 |
| 5 | 0 | 4/0/-1/0/0/5016/5000 | = | 5/0/2/2/2/5016/5500 | 5/0/2/-1/2/5016/5500 | 6/1/0/0/0/5501/5000 | 6/80/-1/0/0/5502/5000 |
| 5 | 1 | 3/0/-1/0/0/5001/5000 | = | 4/0/2/0/4/5016/5503 | = | 4/4/-1/0/0/5503/5000 | = |
| 5 | 2 | 3/0/-1/0/0/5001/5000 | = | 4/0/2/0/8/5016/5503 | = | 4/8/-1/0/0/5503/5000 | = |
| 5 | 3 | 3/0/-1/0/0/5504/5000 | = | 4/0/-1/0/0/5504/5000 | 4/800/-1/0/0/5505/5000 | = | 6/80/-1/0/0/5506/5000 |
| 5 | 4 | 3/0/-1/0/0/5001/5000 | = | 4/0/2/0/8/5016/5503 | = | 4/8/-1/0/0/5503/5000 | = |
| 5 | 5 | 3/0/-1/0/0/5001/5000 | = | 4/0/2/0/4/5016/5503 | = | 4/4/-1/0/0/5503/5000 | = |
| 5 | 6 | 4/0/-1/0/0/5001/5000 | 4/800/6/0/40/5029/5507 | 6/800/6/-3/40/5029/5507 | 6/8840/-1/0/0/5508/5000 | 8/841/-1/0/0/5509/5000 | 8/80/-1/0/0/5510/5000 |
| 5 | 7 | 3/0/-1/0/0/5001/5000 | 3/800/-1/0/0/5511/5000 | = | 4/800/-1/0/0/5512/5000 | = | 6/800/-1/0/0/5513/5000 |
| 5 | 8 | 3/0/-1/0/0/5504/5000 | = | 4/0/-1/0/0/5504/5000 | 4/800/-1/0/0/5505/5000 | = | 6/80/-1/0/0/5506/5000 |
| 6 | 0 | 4/0/-1/0/0/5001/5000 | 4/0/2/3/2/5001/5600 | 5/0/2/0/2/5001/5600 | 5/0/2/-3/2/5001/5600 | 6/2/-1/0/0/5600/5000 | 6/80/-1/0/0/5601/5000 |
| 6 | 1 | 3/0/-1/0/0/5001/5000 | = | 4/0/-1/0/0/5016/5000 | 4/0/2/0/10/5016/5602 | 4/10/-1/0/0/5602/5000 | = |
| 6 | 2 | 3/0/-1/0/0/5001/5000 | = | 4/0/-1/0/0/5016/5000 | 4/0/2/2/20/5016/5603 | 4/0/2/0/20/5016/5603 | 4/20/-1/0/0/5603/5000 |
| 6 | 3 | 3/0/-1/0/0/5001/5000 | 3/800/-1/0/0/5604/5000 | 4/0/-1/0/0/5016/5000 | 4/800/-1/0/0/5605/5000 | 4/800/5/0/2/5605/5606 | 4/80/-1/0/0/5607/5000 |
| 6 | 4 | 3/0/-1/0/0/5001/5000 | 3/0/5/2/0/5001/5600 | 4/2/2/0/8/5600/5608 | 4/808/-1/0/0/5609/5000 | 4/80A/-1/0/0/5608/5000 | = |
| 6 | 5 | 3/0/-1/0/0/5001/5000 | 3/0/5/2/0/5001/5600 | 4/2/2/0/4/5600/5008 | 4/804/-1/0/0/5609/5000 | 4/806/-1/0/0/5608/5000 | = |
| 6 | 6 | 4/0/-1/0/0/5001/5000 | 4/0/5/3/40/5001/5610 | 6/0/5/0/40/5016/5610 | 6/0/5/-3/40/5016/5610 | 8/0/5/-3/40/5611/5612 | 8/80/-1/0/0/5613/5000 |
| 6 | 7 | 3/0/-1/0/0/5614/5000 | = | 4/0/-1/0/0/5614/5000 | 4/1/-1/0/0/5615/5000 | = | 4/80/-1/0/0/5616/5000 |
| 6 | 8 | 3/0/-1/0/0/5001/5000 | 3/800/-1/0/0/5604/5000 | 4/0/-1/0/0/5016/5000 | 4/800/-1/0/0/5605/5000 | 4/800/5/0/2/5605/5606 | 4/80/-1/0/0/5607/5000 |
| 7 | 0 | 4/800/-1/0/0/5700/5000 | = | 4/80/-1/0/0/5701/5000 | = | 6/80/-1/0/0/5701/5000 | = |
| 7 | 1 | 3/10/-1/0/0/5703/5000 | = | = | 4/10/-1/0/0/5703/5000 | = | = |
| 7 | 2 | 3/20/-1/0/0/5705/5000 | = | = | 4/20/-1/0/0/5705/5000 | = | = |
| 7 | 3 | 3/0/-1/0/0/5706/5000 | 3/2/-1/0/0/5707/5000 | = | 4/2/-1/0/0/5707/5000 | = | 4/80/-1/0/0/5708/5000 |
| 7 | 4 | 3/8/-1/0/0/5709/5000 | = | = | 4/8/-1/0/0/5709/5000 | = | = |
| 7 | 5 | 3/4/-1/0/0/5710/5000 | = | = | 4/4/-1/0/0/5710/5000 | = | = |
| 7 | 6 | 4/800/-1/0/0/5711/5000 | 4/80/-1/0/0/5712/5000 | = | 6/80/-1/0/0/5712/5000 | = | = |
| 7 | 7 | 3/0/-1/0/0/5001/5000 | = | 4/800/-1/0/0/5711/5000 | = | 6/80/-1/0/0/5712/5000 | = |
| 7 | 8 | 3/0/-1/0/0/5706/5000 | 3/2/-1/0/0/5707/5000 | = | 4/2/-1/0/0/5707/5000 | = | 4/80/-1/0/0/5708/5000 |
| 8 | 0 | 4/0/-1/0/0/5001/5000 | 4/0/5/0/2/5001/5800 | 5/0/5/-3/2/5016/5800 | 5/2/2/0/1/5800/5801 | 6/2/2/-3/1/5800/5801 | 6/80/-1/0/0/5802/5000 |
| 8 | 1 | 3/0/-1/0/0/5001/5000 | 3/0/2/0/8000/5001/5803 | 4/0/2/-2/8000/5001/5803 | 3/8800/-1/0/0/5804/5000 | 4/8800/-1/0/0/5804/5000 | 4/80/-1/0/0/5805/5000 |
| 8 | 2 | 3/0/-1/0/0/5001/5000 | 3/0/2/0/8000/5001/5803 | 4/0/2/-2/8000/5001/5803 | 3/8800/-1/0/0/5804/5000 | 4/8800/-1/0/0/5804/5000 | 4/80/-1/0/0/5805/5000 |
| 8 | 3 | 3/0/-1/0/0/5001/5000 | 3/0/5/0/2/5001/5800 | 3/0/5/-2/2/5001/5800 | 4/2/-1/0/0/5800/5000 | 4/802/-1/0/0/5804/5000 | 4/80/-1/0/0/5805/5000 |
| 8 | 4 | 3/0/-1/0/0/5001/5000 | 3/0/5/1/2/5001/5800 | 4/0/5/-1/2/5001/5800 | 4/2/2/-1/C/5800/5806 | 4/802/2/-3/C/5804/5806 | 6/480/-1/0/0/5807/5000 |
| 8 | 5 | 3/0/-1/0/0/5001/5000 | = | 4/8000/-1/0/0/5803/5000 | = | 4/8030/-1/0/0/5808/5000 | = |
| 8 | 6 | 4/0/-1/0/0/5001/5000 | 4/800/-1/0/0/5809/5000 | 5/0/2/0/40/5016/5810 | 5/800/2/-3/40/5809/5810 | 6/840/-1/0/0/5810/5000 | 6/842/-1/0/0/5801/5000 |
| 8 | 7 | 3/0/-1/0/0/5001/5000 | = | 3/0/2/0/2/5001/5800 | 3/0/2/-3/2/5001/5800 | 3/802/-1/0/0/5800/5000 | = |
| 8 | 8 | 3/0/-1/0/0/5001/5000 | 3/0/5/0/2/5001/5800 | 3/0/5/-2/2/5001/5800 | 4/2/-1/0/0/5800/5000 | 4/802/-1/0/0/5804/5000 | 4/80/-1/0/0/5805/5000 |
| 9 | 0 | 4/0/-1/0/0/5016/5000 | 4/0/2/0/8002/5016/5900 | 5/0/2/-3/8002/5016/5900 | 5/800/2/-3/8002/5901/5900 | 6/800/2/-3/8002/5901/5900 | 6/80/-1/0/0/5902/5000 |
| 9 | 1 | 3/0/-1/0/0/5001/5000 | = | 4/0/2/0/8000/5016/5903 | 4/0/2/0/10/5016/5904 | 4/10/-1/0/0/5904/5000 | 4/810/-1/0/0/5905/5000 |
| 9 | 2 | 3/0/-1/0/0/5001/5000 | = | 4/0/2/0/8000/5016/5903 | 4/0/2/0/20/5016/5904 | 4/20/-1/0/0/5904/5000 | 4/820/-1/0/0/5905/5000 |
| 9 | 3 | 3/0/-1/0/0/5001/5000 | 4/0/-1/0/0/5001/5000 | 4/800/-1/0/0/5901/5000 | 4/800/2/2/0/5901/5900 | 5/802/-1/0/0/5900/5000 | 5/80/-1/0/0/5902/5000 |
| 9 | 4 | 3/0/-1/0/0/5001/5000 | 3/2/-1/0/0/5900/5000 | 4/2/2/0/8/5900/5906 | 4/A/-1/0/0/5906/5000 | = | 4/800A/-1/0/0/5907/5000 |
| 9 | 5 | 3/0/-1/0/0/5001/5000 | 3/2/-1/0/0/5900/5000 | 4/2/2/0/4/5900/5906 | 4/6/-1/0/0/5906/5000 | = | 4/8006/-1/0/0/5907/5000 |
| 9 | 6 | 4/0/-1/0/0/5001/5000 | 4/0/2/1/40/5001/5908 | 6/800/2/-1/40/5901/5908 | 6/8840/-1/0/0/5909/5000 | 8/841/-1/0/0/5910/5000 | 8/80/-1/0/0/5911/5000 |
| 9 | 7 | 2/0/-1/0/0/5912/5000 | = | = | = | = | = |
| 9 | 8 | 3/0/-1/0/0/5001/5000 | 4/0/-1/0/0/5001/5000 | 4/800/-1/0/0/5901/5000 | 4/800/2/2/0/5901/5900 | 5/802/-1/0/0/5900/5000 | 5/80/-1/0/0/5902/5000 |
| 10 | 0 | 4/0/-1/0/0/6000/5000 | = | 5/0/-1/0/0/6000/5000 | 5/2/-1/0/0/6001/5000 | 6/802/-1/0/0/6002/5000 | 6/80/-1/0/0/6003/5000 |
| 10 | 1 | 3/0/-1/0/0/6000/5000 | = | 3/0/2/0/10/6000/6004 | 3/0/2/-3/10/6000/6004 | 4/10/-1/0/0/6004/5000 | 4/10/2/0/20/6004/6005 |
| 10 | 2 | 3/0/-1/0/0/6000/5000 | = | 3/0/2/0/20/6000/6004 | 3/0/2/-3/20/6000/6004 | 4/20/-1/0/0/6004/5000 | 4/20/2/0/10/6004/6005 |
| 10 | 3 | 3/0/-1/0/0/6000/5000 | 3/800/-1/0/0/6006/5000 | 4/0/-1/0/0/6007/5000 | 4/800/-1/0/0/6008/5000 | 6/800/-1/0/0/6009/5000 | 6/80/-1/0/0/6010/5000 |
| 10 | 4 | 3/0/-1/0/0/6000/5000 | 4/0/-1/0/0/6007/5000 | 3/0/2/0/8/6000/6004 | 4/0/2/-4/8/6007/6004 | 4/8/2/0/2/6004/6011 | 4/8/2/-3/6/6004/6012 |
| 10 | 5 | 3/0/-1/0/0/6000/5000 | 4/0/-1/0/0/6007/5000 | 3/0/2/0/4/6000/6004 | 4/0/2/-4/4/6007/6004 | 4/4/2/0/2/6004/6011 | 4/4/2/-3/A/6004/6012 |
| 10 | 6 | 3/0/-1/0/0/6000/5000 | 3/0/2/0/40/6000/6013 | 3/0/2/-2/40/6000/6013 | 3/0/2/-4/40/6000/6013 | 3/0/2/-6/40/6000/6013 | 3/40/-1/0/0/6013/5000 |
| 10 | 7 | 3/0/-1/0/0/6000/5000 | = | 3/0/2/-1/8002/6000/6002 | 3/0/2/-4/8002/6000/6002 | 3/8002/2/0/0/6002/6003 | 3/8002/2/-4/0/6002/6003 |
| 10 | 8 | 3/0/-1/0/0/6000/5000 | 3/800/-1/0/0/6006/5000 | 4/0/-1/0/0/6007/5000 | 4/800/-1/0/0/6008/5000 | 6/800/-1/0/0/6009/5000 | 6/80/-1/0/0/6010/5000 |
| 11 | 0 | 4/0/-1/0/0/5016/5000 | 4/0/2/0/2/5016/6100 | 4/0/2/-3/2/5016/6100 | 4/0/2/-6/30/5016/6101 | 4/2/2/-3/1/6100/6102 | 4/80/-1/0/0/6103/5000 |
| 11 | 1 | 3/0/-1/0/0/5001/5000 | 3/0/2/-1/4/5001/6104 | 3/0/2/-3/4/5001/6104 | 3/0/2/-5/6/5001/6105 | 3/4/5/-1/2/6104/6105 | 3/6/-1/0/0/6105/5000 |
| 11 | 2 | 3/0/-1/0/0/5001/5000 | 3/0/2/-1/8/5001/6104 | 3/0/2/-3/8/5001/6104 | 3/0/2/-5/A/5001/6105 | 3/8/5/-1/2/6104/6105 | 3/A/-1/0/0/6105/5000 |
| 11 | 3 | 3/0/-1/0/0/5001/5000 | 3/0/5/-1/2/5001/6100 | 4/0/-1/0/0/5016/5000 | 4/0/5/-3/2/5016/6100 | 4/2/-1/0/0/6100/5000 | 6/80/-1/0/0/6103/5000 |
| 11 | 4 | 3/0/2/1/8/5001/6104 | 3/0/2/0/8/5001/6104 | 3/0/2/-2/8/5001/6104 | 3/0/2/-4/A/5001/6105 | 3/8/5/-1/2/6104/6105 | 3/A/-1/0/0/6105/5000 |
| 11 | 5 | 3/0/2/1/4/5001/6104 | 3/0/2/0/4/5001/6104 | 3/0/2/-2/4/5001/6104 | 3/0/2/-4/6/5001/6105 | 3/4/5/-1/2/6104/6105 | 3/6/-1/0/0/6105/5000 |
| 11 | 6 | 4/0/-1/0/0/5001/5000 | 4/800/-1/0/0/5018/5000 | 6/800/-1/0/0/5018/5000 | 6/800/2/3/40/5018/6106 | 8/800/2/0/40/5018/6106 | 8/80/-1/0/0/6107/5000 |
| 11 | 7 | 3/0/-1/0/0/5001/5000 | 3/0/5/-2/2/5001/6100 | 4/0/-1/0/0/5016/5000 | 4/0/5/-5/2/5016/6100 | 4/2/-1/0/0/6100/5000 | 6/80/-1/0/0/6103/5000 |
| 11 | 8 | 3/0/-1/0/0/5001/5000 | 3/0/5/-1/2/5001/6100 | 4/0/-1/0/0/5016/5000 | 4/0/5/-3/2/5016/6100 | 4/2/-1/0/0/6100/5000 | 6/80/-1/0/0/6103/5000 |
| 12 | 0 | 4/0/-1/0/0/5001/5000 | 4/0/2/0/2/5001/6200 | 5/0/2/-3/2/5016/6200 | 5/2/2/-1/1/6200/6201 | 6/2/2/-3/1/6200/6201 | 6/80/-1/0/0/6202/5000 |
| 12 | 1 | 3/0/2/0/10/5001/6203 | = | 3/0/2/-2/10/5001/6203 | 4/0/2/-4/10/5016/6203 | = | 4/8010/-1/0/0/6204/5000 |
| 12 | 2 | 3/0/2/0/20/5001/6203 | = | 3/0/2/-2/20/5001/6203 | 4/0/2/-4/20/5016/6203 | = | 4/8020/-1/0/0/6204/5000 |
| 12 | 3 | 3/0/-1/0/0/1000/5000 | 3/0/2/0/800/5001/6205 | 3/0/2/-2/800/5001/6205 | 4/0/2/-2/800/5016/6205 | 4/0/2/-4/800/5016/6205 | 6/80/-1/0/0/6206/5000 |
| 12 | 4 | 3/0/5/0/2/5001/6201 | 3/0/5/-2/2/5001/6201 | 4/0/5/-4/2/5001/6201 | 3/2/2/0/8/6201/6203 | 4/2/2/-3/8/6201/6203 | 4/A/-1/0/0/6207/5000 |
| 12 | 5 | 3/0/5/0/2/5001/6201 | 3/0/5/-3/2/5001/6201 | 3/2/2/-2/4/6201/6208 | = | 3/2/2/-5/4/6201/6208 | 3/6/-1/0/0/6208/5000 |
| 12 | 6 | 4/0/-1/0/0/5001/5000 | 4/800/2/0/8000/6205/6209 | 6/800/2/-3/8000/6205/6209 | 6/8800/2/-3/40/6209/6210 | 8/8802/2/-3/40/6209/6210 | 8/80/-1/0/0/6202/5000 |
| 12 | 7 | 3/0/-1/0/0/5001/5000 | = | 3/800/-1/0/0/6205/5000 | 4/0/-1/0/0/5016/5000 | = | 4/802/-1/0/0/6209/5000 |
| 12 | 8 | 3/0/-1/0/0/1000/5000 | 3/0/2/0/800/5001/6205 | 3/0/2/-2/800/5001/6205 | 4/0/2/-2/800/5016/6205 | 4/0/2/-4/800/5016/6205 | 6/80/-1/0/0/6206/5000 |
| 13 | 0 | 4/0/-1/0/0/5016/5000 | 4/0/2/0/2/5016/5023 | 5/0/2/-3/2/5016/5023 | 5/0/2/-5/2/5016/5023 | 6/0/2/-4/8002/5016/5004 | 6/0/2/-5/8002/5016/5004 |
| 13 | 1 | 3/0/-1/0/0/5001/5000 | 3/0/2/0/10/5001/5011 | 3/0/2/-2/10/5001/5011 | 3/0/2/-4/10/5001/5011 | 3/0/2/-6/10/5001/5011 | 3/0/2/-8/10/5001/5011 |
| 13 | 2 | 3/0/-1/0/0/5001/5000 | 3/0/2/0/20/5001/5014 | 3/0/2/-2/20/5001/5014 | 3/0/2/-4/20/5001/5014 | 3/0/2/-6/20/5001/5014 | 3/0/2/-8/20/5001/5014 |
| 13 | 3 | 3/0/-1/0/0/5001/5000 | 3/0/2/-1/800/5001/6300 | 4/0/-1/0/0/5016/5000 | 4/0/2/-1/800/5016/6300 | 5/802/-1/0/0/5004/5000 | 5/8802/-1/0/0/5005/5000 |
| 13 | 4 | 3/0/5/0/2/5001/5004 | 3/0/2/0/8/5001/5004 | 3/2/2/-2/8/5001/5004 | 4/2/2/-4/8/5016/5022 | 4/2/2/-5/8/5023/5024 | 4/2/2/-6/8/5023/5024 |
| 13 | 5 | 3/0/5/0/2/5001/5004 | 3/0/2/0/8/5001/5004 | 3/2/2/-2/8/5001/5004 | 4/2/2/-4/8/5016/5022 | 4/2/2/-5/8/5023/5024 | 4/2/2/-6/8/5023/5024 |
| 13 | 6 | 4/0/-1/0/0/5001/5000 | 4/0/2/-3/8000/5001/6301 | 6/800/2/-6/8000/6300/6301 | 6/800/2/-2/40/6301/6302 | 8/840/-1/0/0/6302/5000 | = |
| 13 | 7 | 3/0/-1/0/0/5001/5000 | = | 4/0/-1/0/0/5001/5000 | 4/0/-1/0/0/5016/5000 | 5/0/5/0/2/5016/5004 | 5/0/5/-3/2/5016/5004 |
| 13 | 8 | 3/0/-1/0/0/5001/5000 | 3/0/2/-1/800/5001/6300 | 4/0/-1/0/0/5016/5000 | 4/0/2/-1/800/5016/6300 | 5/802/-1/0/0/5004/5000 | 5/8802/-1/0/0/5005/5000 |
| 14 | 0 | 4/0/-1/0/0/6405/5000 | 4/800/-1/0/0/6400/5000 | 5/0/-1/0/0/6401/5000 | 5/800/-1/0/0/6402/5000 | 6/800/2/-3/8000/6402/6403 | 6/800/2/-6/8000/6402/6403 |
| 14 | 1 | 3/0/-1/0/0/6405/5000 | = | = | = | = | = |
| 14 | 2 | 3/0/-1/0/0/6405/5000 | = | = | = | = | = |
| 14 | 3 | 3/0/-1/0/0/6405/5000 | = | 3/800/-1/0/0/6400/5000 | 4/0/-1/0/0/6401/5000 | = | 4/800/-1/0/0/6402/5000 |
| 14 | 4 | 3/0/-1/0/0/6405/5000 | = | = | = | = | = |
| 14 | 5 | 3/0/-1/0/0/6405/5000 | = | = | = | = | = |
| 14 | 6 | 4/0/-1/0/0/6405/5000 | 4/800/-1/0/0/6400/5000 | 5/0/-1/0/0/6401/5000 | 5/800/-1/0/0/6402/5000 | 6/800/2/-4/40/6402/6406 | 6/840/-1/0/0/6406/6404 |
| 14 | 7 | 3/0/-1/0/0/6405/5000 | = | 4/800/-1/0/0/6402/5000 | = | 5/800/2/-3/8000/6402/6403 | 5/800/2/-6/8000/6402/6403 |
| 14 | 8 | 3/0/-1/0/0/6405/5000 | = | 3/800/-1/0/0/6400/5000 | 4/0/-1/0/0/6401/5000 | = | 4/800/-1/0/0/6402/5000 |
| P | 0 | 3/0/-1/0/0/6500/5000 | 3/800/2/3/2/6501/6503 | 3/800/2/0/2/6501/6503 | 3/802/2/2/1/6503/6502 | 3/801/6/2/40/6502/6504 | 6/800/2/-2/80/6501/6505 |
| P | 1 | 2/0/-1/0/0/6506/5000 | 2/8000/-1/0/0/6507/5000 | 3/0/2/0/10/6508/6509 | 3/800/-1/0/0/6501/5000 | 3/810/-1/0/0/6510/5000 | = |
| P | 2 | 2/0/-1/0/0/6506/5000 | 2/8000/-1/0/0/6507/5000 | 3/0/2/0/20/6508/6509 | 3/800/-1/0/0/6501/5000 | 3/820/-1/0/0/6511/5000 | = |
| P | 3 | 3/0/-1/0/0/6512/5000 | = | 3/800/-1/0/0/6508/5000 | 3/802/-1/0/0/6503/5000 | 4/802/-1/0/0/6503/5000 | 4/802/6/2/80/6503/6513 |
| P | 4 | 3/0/-1/0/0/6512/5000 | 3/2/-1/0/0/6514/5000 | 3/2/2/0/8/6514/6515 | 4/80A/-1/0/0/6516/5000 | = | 4/809/-1/0/0/6517/5000 |
| P | 5 | 3/0/-1/0/0/6512/5000 | 3/2/-1/0/0/6514/5000 | 3/2/2/0/4/6514/6515 | 4/806/-1/0/0/6516/5000 | = | 4/805/-1/0/0/6517/5000 |
| P | 6 | 3/0/-1/0/0/6518/5000 | 3/0/6/3/40/6518/6519 | 3/800/6/3/40/6501/6519 | 4/8800/-1/0/0/6520/5000 | 4/8840/-1/0/0/6521/5000 | 6/80/-1/0/0/6522/5000 |
| P | 7 | 3/0/-1/0/0/6523/5000 | 3/0/2/0/2/6523/6524 | 3/2/-1/0/0/6524/5000 | 3/2/2/4/1/6524/6525 | 4/2/2/2/1/6524/6525 | 4/1/-1/0/0/6526/5000 |
| P | 8 | 3/0/-1/0/0/6512/5000 | = | 3/800/-1/0/0/6508/5000 | 3/802/-1/0/0/6503/5000 | 4/802/-1/0/0/6503/5000 | 4/802/6/2/80/6503/6513 |

## 3. Behavioral Rules & Edge Cases

### 3.1 Hex Route Search, Blockers, Doors, and Replanning

Hex numbering is `column + 200 * row`, with coordinates 0-199. Directions 0-5 are northeast, east, southeast, southwest, west, northwest. The one-step numeric changes for even stored columns are `-1,199,200,201,1,-200`; for odd columns, `-201,-1,200,1,-199,-200`. The directional helper refuses to step from any outer-border hex, even inward. These are the stored column's parity rules, not row parity or mirrored screen-column parity. `[Must-match]`

The route builder takes actor, source, goal, an optional output direction-byte sequence, a strict-goal Boolean, and a blocker query. Output bytes are unsigned one-byte direction numbers, so no multi-byte endianness applies to them. The search's transient working records are not an additional serialized map format. `[Must-match]`

The normal blocker query first examines objects on the candidate hex, then neighboring hexes in direction order 0-5 for multihex objects. An eligible blocker is on the actor's elevation, is not the actor, lacks basic hidden bit `0x01` and movement-nonblocking bit `0x10`, and has art category critter/scenery/wall. Items and miscellaneous art are not movement blockers. Neighboring blockers additionally require multihex bit `0x800`; this expands that obstacle's occupied footprint by one hex. Light/projectile-through flags alone do not remove a movement blocker. `[Must-match]`

Search order and costs:

1. If strict-goal is true and the goal blocker query is non-null, return zero immediately, even for an otherwise usable door. With a permissive goal, intermediate blocking is still tested but the goal's blocker is not. `[Must-match]`
2. Clear the 40000-bit seen map, mark source seen, and put source in frontier slot zero. Its accumulated cost is zero and incoming direction is initialized to **0**, not the actor's facing. `[Must-match]`
3. Heuristic between projected reference corners is `absolute DX + absolute DY - floor(min(absolute DX, absolute DY) / 2)`, in screen-pixel units. Choose frontier entry with smallest heuristic plus accumulated cost; strict comparisons retain the lowest occupied slot on ties. `[Must-match]`
4. Remove that slot. If its hex is the goal, backtrack. Otherwise append it to the expanded list, then examine neighbors in direction order 0-5. `[Must-match]`
5. Skip seen neighbors. For a non-goal neighbor, query its blocker; skip it unless the actor is allowed to auto-use that particular door. Put an accepted neighbor into the **first vacant frontier slot**, mark it seen immediately, record its parent hex/direction, and calculate cost. `[Must-match]`
6. Each hex adds 50 cost units. Outside combat, add another 10 when the incoming direction changes, including a first step other than direction 0. During combat there is no turn surcharge. There is no AP, cripple, ammunition, terrain, or door-opening cost in this search metric. `[Must-match]`
7. Continue until goal selected, frontier empty, or a capacity failure. Backtrack by finding expanded entries with the recorded parent hex, collect incoming directions, then reverse the collected bytes. `[Must-match]`

Seen is set on insertion, not expansion. There is no cheaper-route update, decrease-key, reopening, or alternate parent for an already seen hex. This is a heuristic best-first route search with fixed edge costs, not a guarantee of fully relaxed optimal A*. Reusing low-numbered frontier holes also means ties are not FIFO or simply lexicographic direction order. `[Must-match]`

Frontier and expanded storage each have 2000 slots, but reaching a count of 2000 returns failure; at most 1999 live frontier entries or expanded non-goal entries are accepted. `[Must-match]` Backtracking stops after 800 direction bytes. If a longer route was found, that truncation can return the **last** 800 steps reversed into forward order without reaching the original source, rather than rejecting or returning the first 800 steps. `[May-improve]`: return the first 800 steps counted from the source. Zero also represents an already-at-goal route, not just failure. `[Must-match]` Invalid hexes/buffer sizes are not universally validated before indexing. `[May-improve]`: return zero for a source or goal hex outside 0-39999.

Auto-door permission requires a non-player actor with critter art, biped or robotic body, an available scenery-door prototype, and an unlocked door. Quadrupeds, the player, locked doors, and non-doors do not receive this exception. A usable intermediate door is traversable to the planner, but a strict blocked goal is rejected before door permission is considered. `[Must-match]`

Movement animation applies frame displacements until the next hex boundary, then queries the next hex again. A newly blocking non-door triggers route rebuilding from the actor's current hex to its saved destination with strict-goal enabled. Successful rebuilding resets frame, facing, displacement, and route cursor; failure ends movement. A usable door instead receives an open/use request and the move proceeds; the door action's return value is not checked before the subsequent move attempt. `[Must-match]`

Move-to-object temporarily hides the destination object while planning, then restores its prior hidden state. It shortens the selected path by one step for a normal mover or two for a multihex mover; only the moving actor's multihex flag controls that subtraction. Core search does **not** dilate every obstacle for a multihex mover's full body. The saved replan destination is computed by stepping forward from the original target along the removed final direction, and for a multihex mover also along the preceding removed direction, rather than simply storing the actor's intended stopping hex. This can produce unintuitive replans. `[Must-match]`

**Move-to-object goal.** `[Must-match]` Move-to-object plans with a non-strict goal: the target's own hex is never tested for a blocker. A wall, scenery or any other object sharing that hex does not stop the route. Corpus examples are wall lockers whose hex also holds a wall.
- There is no separate search for a free hex next to the target. The mover stops on the route's next-to-last hex: the neighbor from which the search reached the target hex, which is always reachable.
- A one-step route (the mover is already adjacent) moves nothing. A mover on the target's own hex and elevation registers no move entry at all.
- When no route exists, because every reachable approach to the target hex is blocked, the move entry fails as it starts. Its sequence then ends (section 3.10.1), and the forced adjacency check that follows the move fails (section 3.11.3).
- In combat the route is also cut to the AP limit. A cut route can stop short, and the adjacency check then fails the same way.

Move-to-tile similarly removes the last step if the destination is occupied, and updates that saved replan destination. Ordinary move request limits truncate the scheduled route length; the core search's 50/60 costs are not the remaining AP budget. Actual AP is charged when a hex transition commits. `[Must-match]`

### 3.2 Perception, LOS, Shootability, and Ray Traversal

Three distinct tests must not be collapsed:

| Test | Eligibility / meaning |
| --- | --- |
| Movement blocker | Visible, blocking critter/scenery/wall on the hex or multihex neighbor footprint |
| Dedicated sight-blocker query | Only scenery/wall on the exact hex; ignore hidden and light-through bit `0x20000000`; no neighboring multihex expansion |
| Combat/host straight-ray query | Uses the **movement** blocker, then applies projectile-through filtering only for sampling stride 32 |

These are hex/object tests, not per-sprite-pixel alpha/outline tests. A first eligible object can mask another object sharing the same hex. In particular, skipping a projectile-through object does not make the callback search a second stacked object on that same hex. `[Must-match]`

Perception first measures hex distance reduced by one for each multihex participant, with a minimum of zero. The observer's forward sector is its orientation and either adjacent orientation, including wraparound. In that sector, maximum perception distance is five times effective Perception, halved for a target with glass-translucency bit `0x20000`, then quartered for a player target whose sneaking is working. Outside that sector, the maximum is Perception outside combat or twice Perception in combat, quartered for working player sneak. The rear-sector branch does not apply the glass-halving step. All divisions discard fractions. No obstacle ray is part of this perception predicate. `[Must-match]`

The host `obj_can_see_obj` interface first requires non-null objects and a placed target, then that perception test. It casts with stride 16 and reports success only if the first reported movement blocker is the target itself. A nonblocking/hidden target can therefore fail this test despite being within perception. `[Must-match]` The inspected wrapper also leaves the input/output obstacle slot uninitialized, although the ray reads its input as an ignored object; this is an unsafe recovered path, not a universally deterministic Boolean guarantee. `[May-improve]`: initialize the ignored-object input to none before the cast. A controlled independent ray test must explicitly supply the initial ignored reference.

The straight-ray traversal is pixel Bresenham between projected **hex centers**, corner plus `(16,8)`, on the actor's elevation: `[Must-match]`

1. With an obstacle-output slot, first test the source hex. Ignore its supplied initial ignored object; otherwise an eligible source blocker stops immediately with zero samples. At stride 32, also ignore a returned blocker carrying basic projectile-through bit `0x80000000`. `[Must-match]`
2. Choose Y as major axis when absolute X difference is at most absolute Y difference; otherwise X is major. Initialize error to `2 * absolute minor difference - absolute major difference`. The signs of X/Y differences choose pixel step -1, 0, or 1. `[Must-match]`
3. At each current pixel, map it to a hex using the exact pixel-boundary rules in SPEC-003. Increment the sample counter, counting the initial pixel as 1. At every stride pixels, record a sample and reset that counter. `[Must-match]`
4. If current major coordinate equals the target major coordinate, clear the obstacle output and finish **before** the normal changed-hex blocker test. `[Must-match]`
5. If error is nonnegative, step the minor coordinate and subtract twice the major difference. Step the major coordinate and add twice the minor difference. `[Must-match]`
6. If the hex computed before that pixel step differs from the previous tested hex, query its blocker. A nonignored blocker stops traversal, except projectile-through filtering at stride 32. Otherwise remember that hex and continue. `[Must-match]`
7. Append a final partial-stride sample if necessary. A sample contains hex, elevation, and pixel displacement from that hex's center. `[Must-match]`

Sampling returns at most 200 samples; trying to append sample 201 returns zero. The limit applies even when no output sample array was requested. For an unobstructed ray of major-axis pixel span L, sample count is `ceil((L + 1) / stride)`. A missing obstacle-output slot disables blocker checks entirely, not just obstacle reporting. Ray return zero is consequently not a universal "clear line" answer. `[Must-match]`

Shooting/cover repeatedly casts at stride 32, initially ignoring the shooter. A returned non-critter is a hard obstruction. A returned critter is not a hard obstruction: count it as one cover body unless it is the requested defender, restart at that critter's **center**, and ignore that previous critter on the next cast. Thus later segments can differ from the original single straight line. Each intervening critter subtracts 10 accuracy points; cover is not a percentage or fractional wall exposure. Alive/dead status is not separately checked here; normal corpse nonblocking flags often remove corpses from consideration. `[Must-match]`

Weapon attack preflight rejects, in order: dead defender; both crippled arms when using a weapon; one crippled arm with a two-handed weapon; insufficient ordinary AP; distance beyond effective range; empty ammunition when capacity is positive; hard ray obstruction for ranged/throw. A high accuracy score alone does not prove shootability: hit-chance calculation counts cover but ignores the hard-obstruction Boolean. `[Must-match]` Long/corrupt rays that hit the sample cap can leave obstacle state unchanged (still holding the caller's ignored object); callers that only inspect that state do not thereby acquire a safe termination guarantee. `[May-improve]`: when the sample cap is reached, clear the obstacle output and end the caller's cover/retry loop.

### 3.3 Combat Initialization, Participation, Turns, and Ending

Normal initialization sets inactive state with input-gate bit `0x02`, no participants, no pending attack, and player AP equal to effective maximum AP. Starting a fresh combat stops animations/fidgeting, fixes the combat elevation, collects visible critters there, assigns combat identifiers, clears accumulated damage/opponents/AP, and retains only maneuver alert bit `0x01`. All collected critters initially form the waiting partition; active count is zero. Active bit `0x01` is set and combat UI/AI presentation is initialized. `[Must-match]`

For distinct initiating participants, initial active order is attacker, defender, then player if neither is the player. The first pass uses that order **before** sequence sorting. Attacker/defender become each other's last opponents. `[Must-match]` There is no explicit deduplication for an invalid same-object initiating pair. `[May-improve]`: list a same-object initiating pair once. A resumed combat instead first completes the player's resumed turn without refilling AP, then continues after that player's existing list position. `[Must-match]`

The outer entry guard is permissive: either participant being absent/on the current elevation is sufficient, rather than requiring both actors to match. `[May-improve]`: begin combat only when every supplied participant is on the current elevation. Ordinary vectors below use distinct actors on one elevation. "Active critter" excludes knockout, dead and lose-turn status; "prone" includes knockout/knockdown or art animation 20-35/48-63. The separate dead query also recognizes nonpositive HP. These predicates are not interchangeable with every direct status-bit test. `[Must-match]`

At the round sweep, scan waiters in list order. Joining requires not hidden, player's elevation, not dead/knocked out. Recent damage admits immediately; otherwise invoke its combat script with parameter 5. Alert admits; stop/flee rejects; otherwise an AI danger target must exist. An admitted waiter swaps into the active partition. Admitted NPCs receive an immediate turn before subsequent sorting; the player does not receive that extra admission turn. Swapping and incrementing the scan can defer a swapped-in waiter to a later pass. `[Must-match]`

Dead active participants leave the active/waiting partitions. Non-player knockout, or maneuver value **exactly** 2, moves an active participant to waiting and clears alert. This equality is not a general test of stop bit 2 combined with other bits. `[Must-match]` Sort remaining active actors by descending effective Sequence, then descending Luck. `[Must-match]` Equal keys compare equal and the reference sort is unstable: no stable tie order or player-first tie rule is promised. `[May-improve]`: keep equal-key actors in their pre-sort list order. Add five game seconds after the sweep. `[Must-match]`

For each turn:

- Set current actor and reset the pending attack descriptor. Knockout, dead, or lose-turn status skips the action branch and clears lose-turn. `[Must-match]`
- Otherwise, except for resumed turns, refill ordinary AP from effective maximum AP. `[Must-match]` A combat request also carries an AP bonus. It is added to this refill only during the first turn of the combat the request starts, the same window as the request's accuracy adjustment (section 3.5 step 7). Every R1 source of a combat request supplies 0: the `attack` and `attack_setup` opcodes (SPEC-005), the player's attack command, and the requests the engine makes after an attributed explosion or a dropped primed explosive (SPEC-009 §3.4). No opcode or data file sets it, so the refill is always exactly effective maximum AP. `[Must-match]`
- Player receives a separate free-movement pool of twice Bonus Move rank; NPCs do not. Invoke the actor's combat script with parameter 4 and clear invocation source/target. `[Must-match]`
- A script override suppresses automatic stand-up and ordinary player-input/AI action processing, but queued animations still finish. Otherwise a prone actor stands for 3 AP, then player input or NPC AI runs. `[Must-match]`
- Player input stops on end-turn/space, input gate clear, accepted end-combat request, knockout/dead/lose-turn, ordinary AP at most zero, quit/load state, or scripted interruption. Ordinary AP alone gates this loop, not AP plus free movement. `[Must-match]`
- Drain queued animation completions, reset damage received this turn, clear free movement at turn end. Player death or player elevation changing from the fixed combat elevation ends the loop. `[Must-match]`

An explicit end-combat request is refused if, on the same elevation, an active relevant opponent does not want to stop, or a relevant waiter wants to join. Relevant means a non-player on a different team, or one whose last opponent is on its own team. Stop preference is true for stop/flee maneuver, knockout/death, no AI danger target, danger outside perception, or AP still equal to maximum AP. Successful explicit ending sets bit `0x08`; player input consumes/clears that bit and exits combat. `[Must-match]`

After a complete round, automatic ending occurs if active count is at most one, player is absent, or every active actor is on player's team and no active actor has a last opponent on that team. Otherwise another round begins. A special scripted ending actor can also break the loop. `[Must-match]`

Cleanup clears actor damage/opponents/maneuvers/AP/outlines, restores player's maximum AP and normal UI, clears active combat and restores input-gate state, frees the combat list/AI context, and dispatches map-update scripts. Accumulated kill experience is awarded only for a living player when no quit request suppresses it. Non-scripted knockout recovery can wake a prone surviving player at cleanup. Load interruption uses its separate restoration path instead of assuming normal cleanup. `[Must-match]`

### 3.4 AP Costs, Range, and Ammunition Availability

Movement commits cost 1 AP per hex with sound legs, 4 with one crippled leg, 8 with both crippled legs. Spend the free-movement pool first, then ordinary AP, saturating ordinary AP at zero. Moving/running changes animation speed, not this per-hex cost. Bonus Move does not pay for attacks, inventory, or reload. An already queued movement can continue using free movement after ordinary AP reaches zero, although ordinary player input no longer stays open for that reason alone. `[Must-match]`

Action costs are exact `[Must-match]`:

| Action | Cost / rule |
| --- | --- |
| Bare punch or kick | Base 3 AP, same cost and same basic damage interval |
| Weapon primary | Prototype primary AP value |
| Weapon secondary | Prototype secondary AP value |
| Aimed attack | Add 1 after weapon/player reductions |
| Reload through normal combat/interface path | 2 AP, returned before aiming/perk/trait reductions |
| Nonweapon generic use | 2 AP in the generic item-cost query |
| Stand up | 3 AP, or reduce remaining AP to zero when below 3 |
| Open player inventory in combat | `4 - Quick Pockets rank`, once on entry; wrapper does not clamp a negative malformed cost. `[May-improve]`: clamp the entry cost at 0 |
| Use inventory item on a target in combat | 2 AP on successful use, in addition to applicable entry cost |

For a player weapon with effective range greater than 2, Fast Shot subtracts 1 AP. Presence of Bonus HtH Attacks subtracts 1 for melee/unarmed subtype, and presence of Bonus Rate of Fire subtracts 1 for ranged subtype. These are presence tests, not one reduction per rank. `[Must-match]` Then add aiming cost and clamp attack cost to at least 1. Bare unarmed attacks do not enter the weapon Fast Shot branch. Attack debit occurs after attack resolution and successful animation sequencing; it saturates at zero rather than requiring every direct caller to have passed preflight. `[Must-match]`

Normal inventory drag-to-reload does not independently debit the sidebar's 2-AP reload cost; the inventory entry cost is already charged. Sidebar reload charges only after successful reload. Ammunition must be a compatible caliber; a nonempty weapon also requires the same ammunition PID, rather than silently mixing types. `[Must-match]`

AI successful reload also debits 2 with saturation at zero; unlike the sidebar gate, that debit need not imply at least 2 AP existed beforehand. For attack debit, non-torso/non-uncalled locations imply aiming; torso/uncalled use the player's current UI aiming flag, or false for an NPC. The UI aiming flag is false while the active slot is in reload mode or another action that reports no attack (SPEC-014 §3.23). The player picks a called location in the called-shot window (SPEC-014 §3.38); torso picked there is location 3 and is debited as aimed. `[Must-match]`

Bare punch/kick range is 1. Weapon primary/secondary uses its corresponding prototype range. Throwing range is capped to three times Strength, or for player three times `Strength + 2 * Heave Ho rank`. Object distance subtracts one for each multihex participant and clamps at zero. `[Must-match]`

Normal aimed-shot availability rejects player Fast Shot, burst/continuous animation, and explosion/fire/EMP damage. `[Must-match]` The recovered additional plasma-throw condition compares the damage type against an animation code and never supplies a real plasma-throw restriction. `[May-improve]`: also reject aiming for a throw-animation weapon with plasma damage. Availability and AP calculation are separate: forcing an aiming flag into the cost helper does not repair availability. `[Must-match]`

Unspent ordinary AP is added to effective AC when that defender is not the current combat actor, before the stat's own clamp. Free-movement points do not supply that AC bonus. Derived maximum AP normally starts at `5 + floor(Agility / 2)` and Sequence at `2 * Perception`, with effective trait/perk/bonus adjustments as in SPEC-006. `[Must-match]`

### 3.5 Exact Hit-Chance Calculation

Art weapon class, hit-mode number, and prototype attack-mode nibble are separate quantities. Primary attack mode uses the low extended-flag nibble; secondary uses the next nibble. Values 1/2 select punch/kick and Unarmed skill, 3/4 swing/thrust and Melee skill, 5 throw and Throwing skill, 6/7/8 single/burst/continuous ranged fire. Ranged uses Energy Weapons for laser/plasma/electrical damage; otherwise Big Guns if the weapon's big-gun flag is set, Small Guns otherwise. Mode 0 has no valid skill. `[Must-match]` Values 9-15 are not safely indexed by the inspected mode tables. `[May-improve]`: treat attack-mode values 9-15 like mode 0 (no valid skill or attack subtype).

Start with effective Unarmed skill for no weapon, otherwise the effective selected weapon skill. Let d be object distance including multihex reductions and P be effective Perception. Effective Perception already includes the blinded-stat reduction/clamp. `[Must-match]`

For ranged/throw attacks in the range-aware calculation:

- Set range factor to 4 for a Long Range weapon property, 2 otherwise. `[Must-match]`
- Set R to the greater of `d - range factor * P` and `-2 * P`. `[Must-match]`
- For a player attacker, subtract twice Sharpshooter rank from R **after** that clamp. `[Must-match]`
- Add `-12 * R` when R is nonnegative and attacker is blind; otherwise add `-4 * R`. Negative R therefore gives a close-range bonus. `[Must-match]`
- Count intervening critters with the shot query and subtract 10 per body. `[Must-match]`

Then apply, in order:

1. For a player using any weapon with One Hander: -40 for two-handed, +20 otherwise. `[Must-match]`
2. For any weapon user: subtract 20 per point by which weapon minimum Strength exceeds effective Strength. Add 20 for Accurate weapon property. `[Must-match]`
3. Subtract defender effective AC, including its unused-AP contribution when applicable. `[Must-match]`
4. Add the location adjustment: full table value for ranged/throw, half for melee/unarmed. `[Must-match]`
5. Add 15 for a multihex defender, for all attack types. `[Must-match]`
6. For a player attacker, apply target-light penalty from the table below. `[Must-match]`
7. Add any initial scripted combat-request accuracy adjustment. `[Must-match]`
8. Subtract another 25 if attacker is blind. Add 40 if defender is knocked out or knocked down. `[Must-match]`
9. If attacker team differs from player team, apply combat difficulty -20 easy, 0 normal, +20 hard. `[Must-match]`
10. Cap only the upper result at 95. There is no lower cap at zero or a guaranteed 5-percent floor. `[Must-match]`

No-range calculation omits the R term, but still includes cover, Strength, AC, location, lighting and other adjustments. Unarmed bypasses weapon-specific handling; thrown attacks still use the ranged-style location/range/cover adjustments. `[Must-match]`

| Visible target light, full intensity 65536 | Player accuracy penalty |
| --- | ---: |
| At most 26214 | -40 |
| 26215-39321 | -25 |
| 39322-52428 | -10 |
| Above 52428 | 0 |

Visible target light is the greater of ambient light and true tile light, capped at 65536 on the tile-light path. When the target is the player, subtract that player's emitted light from the tile-light value before the comparison. NPC attackers do not receive this darkness penalty. `[Must-match]`

The stored ammunition armor-class modifier is not read by this hit-chance pipeline. Likewise, no fractional cover percentage, target sprite transparency, or automatic unseen-target penalty is added beyond the actual tests above. `[Must-match]`

### 3.6 Attack Rolls, Critical Effects, and Damage Arithmetic

For a single shot or ordinary melee/unarmed attack, roll D100 and set margin to hit chance minus that roll. Negative margin is failure, otherwise success. Only when absolute game timestamp divided by 864000 is at least 1, a second D100 can upgrade: failed margin becomes critical failure when the second roll is at most `trunc(-margin / 10)`; success becomes critical success when it is at most `trunc(margin / 10) + effective Critical Chance - full location adjustment`. Use the full location adjustment here even for melee/unarmed, not its halved accuracy penalty. `[Must-match]`

These random result values are 0 critical failure, 1 failure, 2 success, 3 critical success. The first-day gate does not prohibit later forced upgrades: global player Jinxed changes any ordinary failure to critical failure on a 50-percent coin flip; player Slayer promotes successful melee/unarmed to critical success; player Sniper promotes a success to critical success when the hit mode's weapon subtype is ranged (never throw, melee or unarmed) and a D10 is at most Luck; the D10 is drawn only when the attacker is the player, the result is success, the subtype is ranged and the player has the perk. Silent Death sets normal-hit scale numerator to 4 for a successful sneaking player melee/unarmed strike from behind against a defender not already targeting the player. A critical lookup replaces that numerator rather than multiplying it again. `[Must-match]`

Critical success sets the attack's critical marker, chooses the success tier in section 2.4, and uses the player or NPC/location entry in section 2.5. Apply ordinary flags/message. If a saving stat is present, a D10 greater than effective victim stat plus its signed adjustment applies the extra flags and alternate message. There is no second D100 critical-upgrade mechanism on this D10 save. Random-cripple flags are removed and replaced by one uniform choice of left leg, right leg, left arm, right arm, without rerolling already crippled limbs. `[Must-match]`

Critical failure clears hit, chooses the weapon's failure table/tier, and does nothing further when the selected mask is zero. `[Must-match]` Otherwise set critical plus those attacker flags:

- Hit-self computes ordinary self damage, using scheduled ammunition count for ranged or one round otherwise; explosion computes one ordinary self-damage round. `[Must-match]`
- Additional self-injury adds D5. Lose-turn sets ordinary AP to zero; it does not independently clear the free-movement pool. `[Must-match]`
- Lose-ammunition schedules the entire current magazine for consumption for ranged subtype; on other subtypes that flag is removed. `[Must-match]`
- Random cripple uses the same four-limb substitution. Random-target selects another AI target, switches to torso, clears the critical marker, marks hit, and calculates normal damage if a target exists. That target is an unintended victim for the retaliation rule below. `[Must-match]`
- Weapon drop/destruction/explosion effects are also presented through animation callbacks. A mask is not proof that every intended HP/equipment effect later commits; see the application guards below. `[Must-match]`

Protected PID `0x01000008` bypasses victim critical augmentation and the ordinary death/status/HP application paths. The same PID as attacker is exempt from critical-failure effects. This is a literal PID exception, not a general invulnerability test. `[Must-match]`

All following divisions truncate toward zero. For each round that hits, choose base random damage:

| Attack form | Inclusive base damage interval |
| --- | --- |
| Bare unarmed | 1 through effective Melee Damage plus 2 |
| Weapon melee/unarmed subtype | Prototype minimum through prototype maximum plus effective Melee Damage |
| Other weapon subtype | Prototype minimum through prototype maximum |

Melee bonus expands the upper endpoint; it is **not** added to both endpoints after the random draw. For a player ranged subtype add `2 * Bonus Ranged Damage rank` to that round's chosen value. Let M be normal numerator 2, Silent Death numerator 4, or selected critical-table numerator. Let difficulty percentage be 75/100/125 for easy/normal/hard only when attacker team differs from player team, otherwise 100. `[Must-match]`

The intervals assume ordered endpoints. Reversed prototype damage bounds use the RNG's recovered reversed-bound fallback to the first supplied bound, not an assumed symmetric swapped interval; this matters after the weapon writer's bound reversal described in SPEC-003. `[Must-match]`

Determine victim DT/DR by weapon damage type, ordered normal, laser, fire, plasma, electrical, EMP, explosion:

- Without armor bypass, use effective threshold/resistance; Penetrate weapon property sets DT to zero. `[Must-match]`
- For a player attacker, Finesse adds 30 to DR on this armor-processing path. `[Must-match]`
- Armor bypass makes both DT and DR zero **except for EMP**, which still uses armor processing and any Finesse/Penetrate changes. `[Must-match]`

For each hitting round, take base damage plus its ranged bonus, multiply by M, divide by 2, multiply by difficulty percentage, divide by 100, then subtract DT. If positive, subtract `trunc(remaining * DR / 100)`; add it to total only if still positive. DT/DR are applied separately to each round, not once to the summed burst. Excessive DR cannot turn this accumulator into healing. `[Must-match]`

**Ammunition multiplier, divisor, and DR modifier are not used here.** Their prototype values remain stored data, but this inspected combat path supplies no corresponding multiplier/divisor/DR term. Combined with the absent ammunition AC term, using a conventional ammunition-adjusted formula would change observable results. Loading/caliber/round-count behavior remains separate and does use ammunition identity/quantity. `[Must-match]`

Before sequencing, a scripted initial combat request can add/clamp defender damage and replace flags. `[Must-match]` Its two supplied override words (attacker flags, then defender flags) are both written to the same defender-flags field, with the last winning. This is not a valid separate attacker/defender override layout. `[May-improve]`: write the first override word to the attacker's flags and the second to the defender's. Those damage clamps do not rerun the already-computed knockback or death-flag calculations. `[Must-match]` A punch request can switch to kick before resolution; the paragraph below gives its draw. `[Must-match]`

**Punch-to-kick substitution.** The ordinary attack request is used by the player's attack command, called or uncalled, and by the NPC AI attack. When its requested hit mode is 4 (punch), it first makes exactly one draw from the shared generator over the inclusive range 1 through 4. This is a range of width 4, so the result is `1 + (U modulo 4)` in the SPEC-006 section 3.4 notation. `[Must-match]`

- **Position.** This is the first random draw of the attack request. It precedes attack-record initialization and the range check, and every draw of this section and section 3.7: hit-chance calculation, attack D100, critical-upgrade D100, Jinxed coin, critical tables, damage, and miss resolution. Hit-chance calculation itself draws nothing. For an NPC it follows that NPC's called-shot draws (SPEC-009). For the player it follows the preflight of section 3.2, which draws nothing. `[Must-match]`
- **Result 1.** Build a critter art identifier with the attacker's current base index (bits 0-11), its current weapon-animation code (bits 12-15), animation 17 (kick, suffix `ar`), and file selector 0. Section 2.3 rule 1 never probes directional files for animation 17. If that resource exists, the request's hit mode becomes 5 (kick) for the rest of the attack. That includes resolution, the AP-cost query, the animation and its sounds, and the recorded hit mode. If it is missing, the mode stays 4. The existence probe consumes no draw. `[Must-match]`
- **Results 2-4.** The mode stays 4 and no art probe is made. `[Must-match]`
- **No draw.** A request for any other hit mode, including a direct kick request (mode 5), makes no draw. A drawn value is never returned to the generator: the draw stays consumed when the attack then fails the range check or sequencing. Attack records built directly by explosion and other action paths with hit mode 4 do not pass through the attack request and make no draw. `[Must-match]`
- Punch and kick share base cost 3 AP, range 1, the Unarmed skill, and the damage interval (sections 3.4 and 3.6). The substitution therefore changes presentation and the recorded mode, not accuracy, cost, or damage. `[Must-match]`

Attack resolution initially calculates damage/flags/ammunition-to-spend without immediately debiting the magazine or HP. Successful action sequencing debits attack AP. When the tracked combat-animation count reaches zero, cleanup reselects the current weapon by hit mode, consumes its scheduled ammunition, displays results, and applies target damage/status. `[Must-match]` Changing the equipped weapon in between can therefore affect which weapon is debited. `[May-improve]`: consume the scheduled ammunition from the weapon that made the attack.

For ordinary victims, positive damage at least current HP sets dead. Persistent status applies mask `0x80FF`; armor bypass, hit, critical and weapon-effect bits are not all stored as permanent critter status. Dead removes queued events; knockout queues wake-up after `10 * (35 - 3 * Endurance)` ticks. HP application records damage received, invokes damage/destroy scripts, can credit player/team kills unless overridden, detaches dead scripts and removes dead party members. `[Must-match]`

**Intended target, unintended victims, onlookers and retaliation.** This is the single authoritative rule; SPEC-009 §3.3 and §3.4 refer to it. Every attack records its **intended target**: the defender named when the attack began. The recorded defender can later change to an **unintended victim**: a missed-shot bystander (section 3.7.1) or a critical-failure random target (above). When the attack's damage is applied, these steps run in order. `[Must-match]`

1. If the intended target exists and differs from the final recorded defender, onlookers are alerted for the intended target, even though it took no damage.
2. If the final recorded defender is a critter not already marked dead, its new status is applied. Then, when the attacker is a critter:
   - if the defender is now dead or knocked out, it records the attacker as its last attacker (SPEC-009 §3.4 standard setter);
   - otherwise it makes the SPEC-009 §3.4 retaliation rating check against the attacker **only if it is the intended target, or its team differs from the attacker's**.

   Its damage is then applied, and onlookers are alerted for it.
3. Each splash or burst extra that is a critter not already marked dead follows step 2. The intended-target exception never applies to extras, so a surviving extra makes the rating check only when its team differs from the attacker's.

An unintended victim on the attacker's own team that survives conscious therefore never changes its last-attacker reference. An intended target on the attacker's team does make the rating check. Attacks by a non-critter source, such as a timed or scripted explosion, skip both the setter and the rating check here. Explosion attribution follows SPEC-013 §3.10. `[Must-match]`

The inspected attacker-self-damage commit tests whether the attacker is **already marked dead**, rather than the normal live-attacker condition. Calculating or displaying critical-failure self damage therefore does not guarantee that a living attacker's HP receives it. `[May-improve]`: commit attacker self-damage and its flags when the attacker is **not** marked dead. A table dead flag can also mark a victim dead independently of HP arithmetic. `[Must-match]` Apart from that safe rule, do not assume that all calculated damage is immediately authoritative HP.

### 3.7 Burst, Continuous Fire, Miss Retargeting, and Splash

For burst/continuous animation, scheduled ammunition is the smaller of current magazine and prototype burst count. Make one hit-chance roll with effective Critical Chance as its critical modifier, without the single-shot location subtraction; critical failure returns early. A critical-success spray adds 20 to its local direct-fire accuracy without reapplying the ordinary 95 cap. `[Must-match]`

For a burst, set center budget to integer ammunition divided by 3, at least 1; left budget to ammunition divided by 3; right budget to the remainder. Direct main-target trials are center divided by 2, at least 1. When that direct count was raised to 1, reduce center budget by 1. Roll direct trials against the spray accuracy. Then cast center with budget center minus actual direct hits, left with its budget, and right with its budget. Continuous fire instead initializes each of these budgets to 1. `[Must-match]`

Direct trials use the full D100 result/upgrade check with critical modifier zero and count either success result as a hit; along-ray trials below use a plain D100 comparison. The additional RNG draws of those two mechanisms therefore differ. `[Must-match]`

**Stage A inside a spray.** `[Must-match]` When no direct trial hit, the spray runs Stage A of section 3.7.1 against the intended defender right after the direct trials and before the center ray, whatever the (non-critical-failure) spray roll was. Its draws are consumed and the hit location becomes torso. In the reference it never commits, so the main-target count stays 0 and the center budget is unchanged. Do not apply the section 3.7.1 Stage A safe rule to this pass: committing here would replace the recorded defender before the rays are cast. A spray whose final result is failure therefore runs Stage A twice, once here and once in the miss resolution of section 3.7.1.

The center ray extends from attacker through target to effective range. For side rays, use target hex as spread center unless target distance is at most 3, in which case use the hex three line transitions from attacker. Take the direction from spread center back to attacker; side points are its adjacent directions plus 1 and plus 5 modulo 6. Extend attacker-to-side-point lines to weapon range. Line extension uses pixel Bresenham and counts changed hexes until the requested count or an outer-border hex, not repeated angular-neighbor steps. `[Must-match]`

Each ray stops at hard non-critter obstruction. At a critter, recompute ordinary torso accuracy for that critter, not the direct-fire accuracy boosted by the spray critical. Consume consecutive successful D100 trials until the first miss or ray budget is exhausted; a miss does not decrement that budget before the ray continues to another critter. The trial comparison is evaluated before the remaining-budget test, so exhausted-budget loop checks can still consume an extra random draw. `[Must-match]`

Ray hits on the main target add to its round count. Other critters become up to six extra targets, each computed at normal damage scale numerator 2. Repeated burst hits on an extra target accumulate damage/flags/knockback. Continuous fire resets remaining budget to 1 at each encountered critter, ignores later duplicate-extra accumulation, and returns zero main-target hits from each ray; its main-target damage comes from the direct trial rather than those ray-return counts. `[Must-match]`

A failed initial spray with any main/extra hits is promoted to success; a nonfailed spray with no hits becomes failure. Main-target critical augmentation occurs after extra-target damage was already calculated, so a main critical multiplier must not be applied to every extra target. `[Must-match]`

**Post-processing of a spray.** `[Must-match]` The section 3.6 forced upgrades run exactly once per attack, on the spray's final result after the adjustment above, never per round or per target:

- Jinxed: a final ordinary failure draws the coin. A critical failure from the spray roll returns before the rays and never draws it.
- Slayer and Silent Death never apply, because a spray's subtype is ranged.
- Sniper: a final success, including a failure promoted by hits, draws the D10 under the section 3.6 conditions and can promote it to critical success.

Extra targets always use numerator 2 and never receive Jinxed, Sniper, the critical tables or the D10 save.

**Extra hits only.** `[Must-match]` When the spray roll was success or critical success and only extra targets were hit, the result stays success or critical success. The intended target then gets the hit bit and takes 0 damage, because zero main-target rounds make no damage draws. On a critical success, including one from Sniper, it still receives the section 3.6 critical-success processing (tier draw, save, random-cripple substitution and flags), so a critical-table dead or knockdown flag applies to a target no round struck. `[May-improve]`: when no main-target round hit, do not set the hit bit or run critical processing for the intended target; keep the extras' damage.

**Draw order for a burst or continuous-fire attack.** `[Must-match]` A draw is made only when its condition holds.

1. Spray roll: D100, then the critical-upgrade D100 when the first-day gate allows it, with effective Critical Chance as the modifier. On critical failure, skip to step 7.
2. Each direct trial in turn: D100, then the gated critical-upgrade D100 with modifier 0.
3. When no direct trial hit: the Stage A D100s in encounter order, up to and including the first success.
4. For the center, left and right rays in that order, at each critter the ray reaches: the trial D100s (hits plus one). Then, when that critter is not the intended target and was hit, one base-damage draw per hit round.
5. The Jinxed coin, for a final ordinary failure.
6. The Sniper D10, for a final success.
7. By final result:
   - Critical success: the section 3.6 critical-success draws, then one base-damage draw per main-target round.
   - Success: one base-damage draw per main-target round.
   - Failure: the second Stage A pass, then one Stage B base-damage draw when it selects a critter (a spray is never a grenade, so no scatter draws).
   - Critical failure: the section 3.6 critical-failure draws. Self and random-target damage use the spray's scheduled ammunition as the round count.
8. Explosion-on-extras draws, when they apply (none in R1).

#### 3.7.1 Missed-shot bystander resolution

This procedure runs after the attack result is final, including the Jinxed, Slayer and Sniper post-processing of section 3.6. It applies only when the hit mode's weapon subtype is ranged or throw. It covers single shots and throws, and also burst or continuous sprays whose result became failure under the spray rules above. A melee or unarmed miss never selects a bystander. Stage A runs here only for an ordinary failure, result 1, of the final result. A burst or continuous spray has also already run one Stage A pass inside the spray (section 3.7), whose draws come before this one. Stage B runs when, after the result branch, the attacker flags contain neither the hit bit nor the critical bit. That happens after an ordinary failure that Stage A did not commit, after a critical failure whose selected failure mask is zero, and after a critical failure by protected attacker PID `0x01000008`, which returns before setting the critical marker. A critical failure that sets any mask skips both stages. `[Must-match]`

Let the **extension hex** E be the hex reached by counting the weapon's effective range for this hit mode (section 3.4, including the throw cap) along the pixel line from the attacker's hex center through the intended defender's hex center. The count uses the line-extension rule in this section: count changed hexes, and stop early at an outer-border hex. When the range is at most 0, or attacker and defender share a hex, E is the attacker's hex. `[Must-match]`

**Stage A: line-of-fire candidate.** `[Must-match]`

1. Set the ignored object to the attacker and the current hex to the attacker's hex.
2. While the current hex differs from E, cast the section 3.2 ray at stride 32 from the current hex to E on the attacker's elevation, supplying the ignored object. Let B be the reported blocker.
3. If B is none, stop. If B is not a critter, stop with an obstruction and no candidate.
4. If B is a critter other than the intended defender, let H be the section 3.5 range-aware hit chance of this attacker against B. Use the attack's current hit location (an uncalled request has already become torso) and the same hit mode. The line-of-fire chance is `trunc(H / 3)`, or exactly 5 when B is dead by the dead query of section 3.3. Draw D100. If the draw is at most that chance, stop with B as the candidate. A chance of 0 or less still consumes its draw.
5. Otherwise, including whenever B is the intended defender, which never draws, set the ignored object to B and the current hex to B's hex, and repeat from step 2.
6. After stopping, set the attack's hit location to torso whatever the outcome.

Stage A never commits a hit in the reference. Its final acceptance test is inverted: it accepts only a candidate carrying projectile-through bit `0x80000000`, and the stride-32 ray never reports such an object. Stage A's draws are still consumed, and Stage B then runs. `[May-improve]`: when Stage A stops with a critter candidate, commit it. Make it the victim, set the impact hex to its hex, set the hit bit, and compute damage as below. An obstruction or no candidate commits nothing. A committed Stage A skips Stage B because the hit bit is now set. A ray that reaches the sample cap leaves its previous object in place; the cap rule of section 3.2 applies to this loop as well.

**Stage B: impact fallback.** `[Must-match]`

1. Choose the impact hex T. For a grenade, meaning throw animation with explosion, plasma or EMP damage, draw the scatter distance first. It is nominally D(trunc(d / 2)), where d is the object distance of section 3.4, with the generator's reversed-bound quirk and a minimum of 1 for a zero result. Then draw a direction from 0 through 5. T is the hex reached by stepping that distance from the intended defender's hex in that direction. For every other weapon, T is E and no draws are made.
2. Cast the section 3.2 ray at stride 32 from the intended defender's hex to T on the defender's elevation, ignoring the intended defender. If it reports a blocker other than the intended defender, that blocker is the candidate and the impact hex becomes the candidate's hex. Otherwise the candidate is the first movement blocker of section 3.2 at T on the defender's elevation, with no ignored object, and the impact hex stays T.
3. If a candidate exists and lacks projectile-through bit `0x80000000`, set the hit bit, record the candidate as the attack's defender, and compute damage as below. Otherwise nothing is hit, and the impact hex is used only for projectile presentation.

Consequences of Stage B, all `[Must-match]`:

- Stage B never considers critters between the attacker and the intended defender. That region belongs to Stage A, which does not commit in the reference.
- T need not lie beyond the defender. If the pixel line needs more than the range in changed hexes to reach the defender, E lies between the attacker and the defender, and the Stage B ray runs back toward the attacker. If it needs exactly the range, E is the defender's own hex. With no other blocker there, the "miss" then selects the intended defender itself for one round at numerator 2, with no critical.
- A non-critter candidate, such as a wall or scenery, receives the hit bit and becomes the recorded defender. It takes zero damage and causes no damage draw. No HP, status or script hit notification follows, and the combat message is the attacker's "missed" text (message 515 for the player, 615 or 715 by NPC gender).
- The originally intended target remains recorded as the intended target. A critter victim other than it produces a "was hit instead" message. Its number is a base plus 8 when the intended target is the player, or plus 9 otherwise. The base is chosen by the victim: 500 for the player, 600 for a male NPC, 700 for a female NPC. Onlookers, last-attacker recording and retaliation follow the intended-target rule in section 3.6. A bystander is an unintended victim, so a surviving conscious bystander makes the retaliation rating check only when its team differs from the attacker's. A bystander left dead or knocked out records the attacker as its last attacker whatever its team.

**Bystander damage and hit location.** Every committed bystander, from Stage B or the Stage A safe rule, is computed as follows. `[Must-match]`

- A non-critter victim receives 0 damage and no draw.
- A critter victim receives exactly one round of the section 3.6 per-round formula with numerator M = 2. Silent Death, critical-table numerators, the critical-success tables, the D10 save and Better Criticals never apply. Make one base-damage draw from the hit mode's interval in the section 3.6 table. Add the player's ranged-subtype Bonus Ranged Damage. Apply the difficulty percentage selected by the attacker's team against the player's team, not by the victim's team. Use the victim's effective DT/DR for the weapon damage type: Penetrate sets DT to 0, and a player attacker's Finesse adds 30 DR. The armor-bypass branch never applies, because a miss carries no critical victim flags.
- Knockback follows section 3.8 with the victim as defender. It is zero for a multihex victim. For a non-null ranged or throw weapon, only explosion damage type produces it: `floor(damage / 10)`, or `/ 5` for a Knockback weapon. Death checks then use the victim's HP, and the usual explosion-on-extras step follows.
- Ammunition already scheduled for the attack (one round for a single shot, the spray amount otherwise) is consumed whether or not a bystander is hit.
- Hit location is torso after Stage A. The critical-failure paths that reach Stage B never pass through Stage A, so the location there stays the requested called location (uncalled is already torso). Location is not a damage input here. It only selects the "hit in the <location>" message text. `[May-improve]`: report torso for every Stage B victim.

**Draw order for a ranged or throw miss.** `[Must-match]`

1. Attack D100, then the critical-upgrade D100 when the first-day gate allows it.
2. The Jinxed coin, for global player Jinxed with an ordinary failure.
3. On ordinary failure, the Stage A D100s in encounter order, up to and including the first success. On critical failure, the section 3.6 critical-failure draws instead.
4. For a grenade, the Stage B scatter distance, then the scatter direction.
5. One base-damage draw, only for a critter victim.
6. Explosion-on-extras draws, when they apply.

The splash-victim enumeration has its radius and initial-hex setup commented out in reference revision R1. Its first hex stays -1, so it exits on its first iteration without adding any victim and without drawing. This one enumeration serves every explosion: explosive weapon attacks, grenades, critical-failure explosions, and the timed and scripted explosions of SPEC-013 §3.10, including the dropped-explosive notice check. In R1 **no explosion damages anything beyond its center target**: for a weapon, the struck defender; for a timed or scripted explosion, the critter blocking the center hex. Do not supply an invented radius or falloff formula. `[Must-match]` The surrounding explosion art, and the separate radius-3 scenery-script notification of SPEC-013 §3.10, are presentation and script effects, not damage.

### 3.8 Knockback, Death Animation, and Corpse Presentation

Computed knockback is zero for a multihex defender. Otherwise explosion, bare unarmed, or melee subtype permits `floor(total damage / 10)`, or `/ 5` for a Knockback weapon property. A non-null weapon classified as unarmed does not enter the bare-unarmed exception unless another eligible condition applies. Other normal ranged/throw damage does not automatically produce displacement. The prototype behavioral no-knockback bit is not consulted by this computation. `[Must-match]`

Direction is attacker-to-defender hex orientation. The displacement executor checks movement blockers at increasing distance. With no blocker it uses the computed maximum. `[Must-match]` A blocker at distance j first decrements distance before breaking, then decrements it again after the loop; actual displacement is therefore at most `j - 2`, clamped to no displacement, not always the expected `j - 1`. `[May-improve]`: stop one hex short of the first blocker (at most `j - 1`). Fall-front without art falls back to fall-back. A fall can animate without movement when the resulting distance is nonpositive. `[Must-match]`

Ordinary lethal animation uses violence levels 0 none, 1 minimal, 2 normal, 3 maximum. "From front" is true when the absolute difference between stored attacker/defender orientations is 2, 3 or 4, not 0, 1 or 5. `[Must-match]`

| Damage type | Moderate severe-death animation | Maximum severe-death animation |
| --- | ---: | ---: |
| Normal | 26 autofire dance | 25 chunks |
| Laser | 28 sliced-in-half | 28 sliced-in-half |
| Fire | 24 charred | 33 fire dance |
| Plasma | 24 charred | 32 melted |
| Electrical | 27 electrify | 30 electrified-to-nothing |
| EMP | 20 fall-back | 20 fall-back |
| Explosion | 23 big hole | 31 exploded-to-nothing |

Death choice is ordered:

1. Victim PIDs `0x01000017,0x01000032,0x01000031` request animation 31 with minimum violence 2. `[Must-match]`
2. Bloody Mess applies only for a player attacker. At violence 3, Bloody Mess with punch/kick/thrust/swing/throw requests 23; single normal-damage fire with Bloody Mess or damage at least 45 also requests 23. `[Must-match]`
3. Otherwise at violence above 2, Bloody Mess or damage at least 45 selects the maximum column, falling back to the moderate column if the maximum choice cannot pass its violence/art test. `[Must-match]`
4. Otherwise at violence above 1, Bloody Mess or damage at least 15 selects the moderate column. Otherwise choose 20. `[Must-match]`
5. A rear hit changes a remaining plain 20 choice to 21 fall-front, then performs final art/violence fallback. `[Must-match]`

Minimum-violence/art validation returns the desired animation if allowed and found. Otherwise a front hit falls back to 20. `[Must-match]` The recovered rear fallback is inverted: when fall-front art exists it returns 20, and when it does not exist it returns 21, so the returned fallback resource can be missing. `[May-improve]`: for a rear hit return 21 when fall-front art exists and 20 otherwise.

Before the ordinary victim-based choice tree, special attacker art `0x0500000A` requests explosive 31, attacker PID `0x020001EB` requests electrical 30, and attacker art `0x020001F5` uses the supplied attack-animation selector, each under maximum-violence validation. Ordinary fall selection checks one/two hexes in the falling direction, can reverse fall-front/back around obstacles, then replaces unavailable fall-front with fall-back. `[Must-match]`

Blood presentation adds 34/35 after a fall-back/front when violence is not zero and the corresponding art exists; it plays after the fall rather than replacing it (section 3.10.6). Fire-dance death can animate a random unobstructed 2-5-hex run, then burned-to-nothing 29. A living on-fire result can use 33 and return to standing. These presentation changes do not turn every transient fire/critical flag into permanent critter status. `[Must-match]`

Corpse presentation sets movement-nonblocking and flattens except for victim PIDs `0x01000032,0x01000031,0x01000008`; it removes the outline. Final animation 30 or 31 drops inventory except for victim PIDs `0x01000031,0x01000032,0x01000017`. Public direct/static-death requests have a separate correction: static codes 51-63 revert to forced fall-back 20 in these callers if maximum violence/art is unavailable. They are not the same decision tree as attack-driven damage selection. `[Must-match]`

### 3.9 Thrown Weapons: Consumption and Projectile Flight

This section applies to a hit mode whose attack animation is the throw (animation 18), after the attack result is final, including section 3.7.1. It covers removing the thrown item from the attacker and flying it to the target. SPEC-013 §3.10 gives the explosion art, sounds and damage for explosive throws. Non-thrown ranged weapons with a projectile use the same flight steps but create a new projectile object and remove nothing from the inventory.

The animation start delays below follow the start rule of section 3.10.1, which SPEC-013 §3.10 summarizes: -1 waits until every earlier entry of the sequence has finished, 0 starts together with the entry before it, and N > 0 waits N examinations of the sequence while an earlier entry is still running. Section 3.10 places these steps in the whole attack sequence.

**Corpus thrown weapons.** `[Must-match]`

| PID | Item | Throw mode | Weapon code | Damage type | Projectile art (`misc.lst` index, identifier) | After arrival |
| ---: | --- | --- | ---: | --- | --- | --- |
| 7 | Spear | secondary | 4 | 0 normal | `spear.frm` (13, `0x0500000D`) | Lies at the arrival hex |
| 19 | Rock | primary | 0 | 0 normal | `rockthro.frm` (28, `0x0500001C`) | Lies at the arrival hex |
| 25 | Frag Grenade | primary | 0 | 6 explosion | `grndtro.frm` (27, `0x0500001B`) | Explodes and is erased |
| 26 | Plasma Grenade | primary | 0 | 3 plasma | `grndtro.frm` (27, `0x0500001B`) | Explodes and is erased |
| 27 | Pulse Grenade | primary | 0 | 5 EMP | `grndtro.frm` (27, `0x0500001B`) | Explodes and is erased |
| 45 | Throwing Knife | primary | 1 | 0 normal | `knife.frm` (4, `0x05000004`) | Lies at the arrival hex |
| 79, 205 | Flare | primary | 0 | 0 normal | `fthro.frm` (26, `0x0500001A`) | Lies at the arrival hex |
| 159 | Molotov Cocktail | primary | 0 | 6 explosion | `molotov.frm` (32, `0x05000020`) | Explodes and is erased |

The projectile art is the art of the weapon prototype's projectile PID (SPEC-003 weapon payload offset `0x18`), which names a miscellaneous prototype. A throw is **explosive** when its damage type is explosion, plasma or EMP.

**When the projectile phase runs.** `[Must-match]` Both conditions must hold:

1. The attack is a hit, or it does not carry the critical bit. A critical failure with a nonzero effect mask keeps the critical bit and does not fly (section 3.6). A critical failure whose mask is zero is an ordinary miss, and a critical failure that redirects to a random target clears the critical bit and sets hit; both of these fly.
2. The projectile PID names a loadable prototype whose art identifier is not -1. Every corpus thrown weapon qualifies.

When the phase does not run, nothing is removed from the attacker and nothing flies. A defender dodge is registered only in the case of section 3.10.4 step 7: the projectile is invalid (condition 2), the attack is not a hit, the critical bit is clear, and the defender is neither knocked out nor knocked down. A critical failure that keeps its critical bit registers no dodge. Critical-failure weapon effects (drop, destroy, explode) act on the item still in the hand. `[Must-match]`

**Consumption.** `[Must-match]` Consumption happens at once, while the attack's animation sequence is being built, before any of it plays. In this order:

1. Record the thrown item's own art identifier and its hand flags.
2. Remove exactly **one unit** from the attacker's inventory, searching nested containers as well.
   - When the entry held exactly one unit, the entry is deleted and the hand is now empty.
   - When it held more, the inventory entry is replaced by a copy of the thrown object with the quantity reduced by 1. The copy gets a new object ID and a new script instance. It keeps the hand flag, so the rest of the stack stays in the same hand.
   - The removed object loses its hand and worn flags and no longer has an owner.
3. **Refill the hand.** Search the attacker's inventory, in order and including nested containers, for the first entry identical to the thrown item:
   - same PID and script;
   - neither object equipped or marked used;
   - not a container, and both inventories empty;
   - equal item data.

   Take one unit of that entry, give it the recorded hand flags, and add it back as its own entry. That unit is the **refill item**. If none is found, there is no refill item. An equipped copy left by step 2 never matches, so a partly used hand stack is never refilled from itself.
4. For the player, refresh the interface bar's item slots without animation.
5. The removed object **is** the projectile. Its art becomes the projectile art, and it is placed on the map at the attacker's hex and elevation.

A hand stack with units left can still be refilled from a separate unequipped stack of the same item. The hand then holds two hand-flagged entries. `[May-improve]`: skip the refill when units remain in the hand.

**Flight.** `[Must-match]` Every entry that has a delay is registered in the attack's single reserved animation sequence. Step 3 happens at once, while the sequence is being built.

1. The attacker turns toward the final defender's hex, delay -1. Section 3.7.1 can replace the defender with a bystander.
2. The AI attack taunt (SPEC-009). Then the attack sound on the attacker, delay -1: the weapon attack sound (SPEC-010) when the attacker's art weapon code is nonzero, otherwise the character sound for the throw animation. Then the throw animation, delay 0.
3. The projectile is hidden, and its light is set with a requested radius of 9 at its own light intensity ("Projectile light" below gives the result). It is moved to the **origin hex**: the hex adjacent to the attacker in the hex direction from the attacker's hex to the defender's hex, on the attacker's elevation. It faces that direction.
4. The projectile is unhidden with delay A. A is the action frame of the attacker's throw art for its current base, weapon code and facing (taken before the turn of step 1), or 0 when that art cannot be loaded.
5. The flight sound (SPEC-010 "ammo flying" weapon sound) plays on the projectile, delay 0.
6. The projectile moves in a straight line, using the walk animation, delay 0. On a hit it goes to the defender's hex on the defender's elevation. On a miss it goes to the impact hex of section 3.7.1 on the defender's elevation. On a hit, N is the stride-32 straight-ray sample count from the origin hex to the defender's hex (section 3.2), minus 1. On a miss, N is 0. The flight path never tests for blockers, and its nodes take the projectile's own elevation, which is the attacker's; the supplied elevation is not used. Section 3.10.5 gives the node count, the flight speed and the value of N.
7. **Explosive arrival:** SPEC-013 §3.10 ("Order", steps 2-3). The grenade first shows its own item art, then the explosion art plays and the six adjacent arts follow.
8. **Non-explosive arrival:** the weapon hit sound plays on the projectile, delay N. Section 3.10.5 gives when it starts.
9. The defender's reaction and damage display, delay 0 (section 3.10.6). Then the AI hit or miss taunt.
10. An explosive projectile is erased. A non-explosive thrown projectile changes back to the recorded item art, delay -1, and **stays on the map** at its arrival hex as an ordinary item, where it can be picked up. The six explosion neighbors are erased.
11. Unless the attack left the attacker knocked out, knocked down or dead: if the refill item exists and has a nonzero weapon code, the attacker takes out that weapon code, delay -1. Otherwise the attacker's art changes to standing with weapon code 0, delay -1.

When the rest of a stack stays in the hand (step 2), there is no refill item. The attacker then shows weapon code 0 while still holding the item. Its next throw uses weapon-code-0 throw art (section 2.2) and the character attack sound. This affects the Spear and the Throwing Knife; the other corpus thrown weapons have weapon code 0 anyway. `[May-improve]`: when units remain in the hand, take out that hand item's weapon code instead.

**Attacker weapon code when the projectile phase does not run.** `[Must-match]`

- A throw never substitutes a special weapon code. The throw art is animation 18 for the attacker's current base and current weapon code (section 2.2), and consumption removes the unit without changing the attacker's art, so the attacker keeps the thrown weapon's code while the throw plays. There is no saved "pre-throw" code to restore.
- Step 11 is the only place a throw changes the attacker's art. It is registered at the end of **every** throw sequence, whether or not the projectile phase ran, and is skipped only when this attack's resolution gave the attacker the knocked-out, knocked-down or dead flag.
- A refill item exists only when the projectile phase ran and consumption step 3 found one. When the phase did not run there is never a refill item, so step 11 always changes the attacker to standing with weapon code 0.
- A critical failure whose effect drops, destroys or explodes the weapon takes it out of the active hand, and that removal already sets the attacker's weapon code to 0, so the result is consistent. A drop that the item's script overrides leaves the item in the hand.
- Every other effect that keeps the critical bit (lose turn, hit self, self-injury without knockdown, random cripple, dud, or a lose-ammunition flag removed because the subtype is throw) leaves the item in the hand while step 11 shows weapon code 0. The code stays 0 until the attacker's art is next rebuilt from the equipped item, for example when the player closes the inventory, switches hands or wields an item. Until then the next throw uses the weapon-code-0 throw art, its action frame and the character attack sound, as in the stacked-hand case above.
- When the attack sequence fails to register (section 3.10.1) after consumption, nothing changes the attacker's art, so a hand emptied by the throw still shows the thrown weapon's code.

`[May-improve]`: in step 11, when the thrown item (or the rest of its stack) is still in the attacker's hand, take out that item's weapon code instead of weapon code 0; when a throw's sequence fails to register, apply the weapon code of whatever the hand then holds.

**Hit-sound delay when a blocker intercepts the line.** `[Must-match]` N is never clamped, and no clamp is needed. Nothing intercepts a projectile in flight. The flight path and N are both built without a blocker test, from the same origin hex to the same destination, so the projectile passes over anything on the line and N always equals its node count minus 1. Interception is decided earlier, during attack resolution (section 3.7.1). When a bystander, wall or scenery object is struck there, it becomes the recorded defender and gets the hit bit, so both the flight and N end at its hex. The start rule of section 3.10.1 also bounds the wait: a positive delay never holds an entry after every earlier entry has finished. The hit sound therefore never starts after the flight ends; it can start earlier, because the attacker's still-running throw animation also counts N down (section 3.10.5).

**Projectile light.** `[Must-match]` Setting an object's light keeps its intensity and clamps the radius to at most 8. An intensity of 0 or less instead sets both the radius and the intensity to 0. The requested radius 9 therefore becomes:

- radius 8 when the projectile's own intensity is positive;
- radius 0 with intensity 0 otherwise.

While hidden, the projectile casts no light. From the unhide of step 4 its light moves with it. Arrival changes only its art, so a landed item **keeps** the radius and intensity it was given at launch. In the corpus, no thrown-item prototype and no miscellaneous projectile prototype has a nonzero light value. Only a lit flare carries light: lighting a Flare (PID 79) turns it into PID 205 with radius 8 and intensity 65536, and starts its burn-out event. A thrown lit flare therefore lands with radius 8 and intensity 65536, the same as before the throw. Its burn-out event stays queued and destroys the landed flare when it fires (SPEC-008 event type 7). An unlit Flare, and every other corpus thrown item, flies and lands with no light.

The throw does not change the thrower's own light. Wielding a lit flare raises the wielder's light to the larger of its own and the flare's values, and this stays raised after the flare is thrown. `[May-improve]`: when the throw leaves no lit flare in either hand, restore the thrower's light the way the reference does when another item replaces a lit flare in the hand. That is radius 4 and intensity 65536 for the player, and the prototype's radius and intensity for any other critter.

### 3.10 Attack Animation Choreography

This section gives the order and timing of everything an attack shows and plays: the attacker's animations and sounds, the projectile, and the reactions of the defender, the extra victims and the attacker. The attack sequence is built once, when the attack is sequenced after resolution (section 3.6), and plays afterwards. The damage it shows was already computed. HP and status change when the combat-animation count returns to zero (section 3.6), not when a reaction plays.

#### 3.10.1 Animation sequences and the start rule

`[Must-match]` An attack registers one reserved, prioritized animation sequence: an ordered list of entries, each with an owner object and a start delay. The reference holds at most 40 entries per sequence. The longest attack in this section, an explosive throw that kills its knocked-back defender, uses 37.

- **Instant entries** finish as soon as they start: turn toward a hex, play a sound, show a taunt, hide, unhide, set art, set light, and the forced callbacks of section 3.10.6. The turn sets the facing and the standing pose for the current weapon code at once; it does not animate.
- **Timed entries** run over several frame steps: animate (play an animation once), animate and hide, straight move, and take out a weapon.

Frame steps (`[Must-match]`):

- A timed entry takes its frame steps every `trunc(1000 / fps)` milliseconds, with the fps of SPEC-001 §3.4 (stored 0 means 10).
- In combat, an entry that uses the walk animation (1) adds the `combat_speed` preference (SPEC-014) to its fps. This applies to every owner except the player, and to the player only when `player_speedup` is 1. The projectile flight uses animation 1, so its rate always includes `combat_speed`.
- An animation's first frame step comes at the first scheduler pass after it starts, including the pass in which it started when that pass has not yet reached it. Step k shows zero-based frame k − 1. The step after the last frame finishes the entry.
- A straight move's step k places the object on path node k − 1, and the step that reaches the last node finishes the entry.

**Start rule.** `[Must-match]` The sequence is **examined** at these moments:

- once when it is registered;
- right after any of its entries starts;
- at the start of every frame step of each of its running timed entries;
- whenever one of its timed entries finishes.

Each examination repeats the following:

1. If no entry is left, stop.
2. If every started entry has finished, start the next entry whatever its delay, and repeat.
3. Otherwise read the next entry's delay:
   - negative: stop and wait;
   - positive: decrease it by 1 and stop;
   - 0: start the entry and repeat.

The first entry's delay is forced to 0. Consequences:

- **-1** starts once every earlier entry has finished.
- **0** starts in the same examination as the entry before it.
- **N > 0** starts at the N + 1-th examination after it becomes the next entry. The first of those examinations is the one in which the entry before it started.
  - When that entry is an animation and the only running entry, the delayed entry starts on that animation's N-th frame step, the one that shows its zero-based frame N − 1.
  - Several running timed entries of the sequence count the same delay down, so it runs out sooner.
  - A finishing step examines twice: once at its start and once when it finishes.
- A positive delay never holds an entry after every earlier entry has finished. This is the only bound on a delay.

A sound entry is built when the sequence is registered. Its relative volume (SPEC-010 §3.11) is fixed then. If no effect file is found, the entry stays in place as a silent entry with the same delay. `[Must-match]`

**Failure.** `[Must-match]` If any entry cannot be registered, because the capacity is full or the art of an animate, set-art or take-out entry cannot be loaded, the whole sequence is discarded before anything plays.

- The attack request then fails before the AP debit of section 3.6, plays nothing, and applies no damage.
- Random draws already made stay consumed.
- An explosive or non-thrown projectile is erased. A non-explosive thrown item has already been consumed (section 3.9) and stays on the map at the origin hex, hidden. `[May-improve]`: when a thrown attack's sequence fails, return the unit to the attacker's inventory.

A sequence that stops while playing, because an entry fails as it starts, ends as follows:

- every unstarted entry is dropped, except forced callbacks and forced erases, which run at once;
- unstarted sounds are discarded;
- each critter owner that is not prone is reset to its standing pose.

`[Must-match]`

The standing reset also happens when a sequence ends normally. Every critter owner that is not prone is set to its standing pose for its current weapon code, unless the sequence was registered without that reset; ladder climbing is registered that way (SPEC-013 §3.23). Hidden objects whose hide entry has run, and every forced erase, are removed from the map when the sequence ends. `[Must-match]`

**One sequence per owner.** `[Must-match]` A new sequence cannot add an entry owned by an object that has a non-callback entry, started or not, in another sequence that has not ended.
- Such an entry fails, so the whole new sequence is discarded (Failure, above). The request is dropped, not queued.
- If that other sequence is an idle fidget (section 3.11.2), it is ended at once and registration continues.
- Callback entries are not checked. A sequence made only of callbacks therefore always registers.
- Cancelling an object's sequence (SPEC-014 §3.27) ends the first sequence that has not ended and holds a non-callback entry owned by that object. A prioritized sequence, such as an attack, is never cancelled this way. The ending follows the rules above: unstarted entries are dropped except forced ones, and owners are reset to their standing pose.
- A use-item-on-object entry is a three-argument callback. The reference records its target object as the owner of that entry and checks it like any other owned entry. While such a sequence runs, the target cannot register an entry of its own, and cancelling the target's sequence ends the user's sequence. `[May-improve]`: treat this entry as having no owner, like every other callback.

#### 3.10.2 Attack animation by hit mode

The attack animation comes from the weapon's attack-mode nibble for the hit mode (SPEC-003). Hit mode 5 (kick) always uses animation 17. A hit mode with no weapon uses animation 16. The suffixes are those of section 2.2. `[Must-match]`

| Attack mode | Animation | Suffix | Sound on the attacker at start | Contact or hit sound |
| --- | ---: | --- | --- | --- |
| Punch | 16 | `aq` | Character sound, animation 16 | Character sound, animation 16, contact extra (SPEC-010 first letter `z`) |
| Kick | 17 | `ar` | Character sound, animation 17 | Character sound, animation 17, contact extra |
| Swing | 42 | weapon letter, `g` | Weapon attack sound (`A`) | Weapon hit sound (`H`) |
| Thrust | 41 | weapon letter, `f` | Weapon attack sound | Weapon hit sound |
| Throw | 18 | `dm` knife, `gm` spear, `as` otherwise | Weapon attack sound when the attacker's art weapon code is nonzero, else character sound, animation 18 | Section 3.9 arrival |
| Single fire | 45 | weapon letter, `j` | Same rule as throw | Weapon hit sound only with a projectile (section 3.10.4) |
| Burst | 46 | weapon letter, `k` | Same rule as throw | Same as single fire |
| Continuous fire | 47 | weapon letter, `l` | Same rule as throw | None |

Animations 16 and 17 use the character sounds even when a weapon such as Brass Knuckles makes the punch. `[Must-match]` A ranged attack other than a throw also plays point (43, weapon letter then `h`) before its attack animation and unpoint (44, weapon letter then `i`) after it.

`[Must-match]` The **action frame** A is the action-frame value in the attack art's FRM header (SPEC-001 offset `0x06`, used as in §3.4). It is read from the attack animation for the attacker's current base and weapon code, and is 0 when that art cannot be loaded. Animations 16-18 and 41-47 always encode file selector 0 (section 2.3 rule 1), so the facing does not matter. A is a start delay in the sense of section 3.10.1. The contact sound, the projectile's appearance and the reactions are all timed from it. Corpus examples for the player stem `hmjmps`, all at 10 fps:

| Art | Animation | A | Frames |
| --- | --- | ---: | ---: |
| `aq` | punch | 3 | 11 |
| `ar` | kick | 6 | 17 |
| `as` | throw | 4 | 12 |
| `an` | dodge | 1 | 8 |
| `eg` | club swing | 7 | 12 |
| `ef` | club thrust | 4 | 10 |
| `hj` | pistol single fire | 1 | 5 |
| `ik` | SMG burst | 1 | 9 |

#### 3.10.3 Melee and unarmed sequence

This applies to attack animations below 45 other than 18. Let A be the attacker's action frame. Let B be the action frame of the defender's dodge art: animation 13 for the defender's current base and weapon code, suffix `an` with weapon code 0, otherwise the weapon letter then `e`. The entries are, in order: `[Must-match]`

1. The attacker turns toward the recorded defender's hex.
2. The AI attack taunt (SPEC-009 §3.17), delay 0.
3. One branch, chosen by the attack result:
   - **Hit, or critical bit set.** The attacker's flags contain the hit bit `0x100` or the critical bit `0x200`, which includes every critical failure that kept its critical bit.
     - Attack sound, delay 0.
     - Attack animation, delay 0.
     - Contact or hit sound on the attacker, delay A.
     - Then the reactions of section 3.10.6 with delay 0, so they begin on the attacker's action frame.

     A critical failure therefore plays the contact or hit sound although nothing was struck. The attacker's own reaction follows.
   - **Miss, defender knocked out or knocked down** (status bit `0x01` or `0x02`): attack sound with delay -1, then the attack animation with delay 0. There is no dodge and no contact sound.
   - **Miss, dodge art exists, B ≤ A.**
     - Attack sound, delay -1.
     - Attack animation, delay 0.
     - Dodge sound on the defender (character sound for animation 13), delay A − B.
     - Dodge animation, delay 0.
   - **Miss, dodge art exists, B > A.**
     - Dodge sound, delay -1.
     - Dodge animation, delay 0.
     - Attack sound, delay B − A.
     - Attack animation, delay 0.

     Either dodge branch lines the two action frames up to within one frame step.
   - **Miss, no dodge art:** nothing more. The attacker does not animate and no sound plays. `[May-improve]`: register the attack sound (delay -1) and the attack animation (delay 0), as for a knocked-down defender.
4. The attacker's hit taunt (delay -1) when the attack hit and the defender did not die, or its miss taunt (delay -1) when the attack missed (SPEC-009 §3.17).

A melee miss never selects a bystander (section 3.7.1), so the recorded defender is the intended target.

#### 3.10.4 Ranged and thrown sequence

This applies to animations 18 and 45-47. The entries are, in order: `[Must-match]`

1. The attacker turns toward the final recorded defender's hex, which section 3.7.1 may have replaced.
2. Unless the animation is 18, point (43), delay -1. It plays in full before the attack.
3. The AI attack taunt, delay 0.
4. The attack sound on the attacker (section 3.10.2), delay -1.
5. The attack animation, delay 0.
6. The **projectile phase**, under the conditions of section 3.9. It never runs for animation 47. It follows section 3.9 steps 3-8; a non-thrown weapon creates a new object with the projectile art instead of consuming an item. The arrival is one of:
   - **Explosive:** a throw with damage type explosion, plasma or EMP, or any other weapon with damage type explosion. SPEC-013 §3.10 gives the explosion entries.
   - **Thrown, non-explosive:** the weapon hit sound on the projectile, delay N.
   - **Non-thrown, non-explosive** (lasers, plasma weapons and the Alien Blaster): the projectile is erased with delay -1, then the weapon hit sound plays with delay N. The sound plays on the weapon object, which SPEC-010 §3.11 positions at its owner, the attacker. The erase waits for both the flight and the attacker's animation to finish. The hit sound then starts at once, because nothing earlier is still running, so N has no effect.
7. For animations 18, 45 and 46, when the projectile phase does not run because the weapon has no valid projectile (section 3.9 condition 2), the attack is not a hit, the critical bit is clear, and the defender is neither knocked out nor knocked down: the defender's dodge animation, delay A, with **no** dodge sound. Its art is not checked first. A missing dodge art makes the registration fail, and so the whole attack fails (section 3.10.1). `[May-improve]`: skip the dodge when its art does not exist.
8. The reactions of section 3.10.6. Their delay is 0 after a projectile phase and A otherwise. Without a projectile, the defender therefore reacts on the attacker's action frame.
9. The defender's hit taunt (delay -1) when the attack hit and the defender did not die, otherwise its miss taunt (delay -1). The taunt is spoken by the defender, not the attacker (SPEC-009 §3.17).
10. Clean-up: section 3.9 step 10 (an explosive projectile and its six neighbors are erased; a thrown non-explosive item gets its art back), all with delay -1.
11. Unless the attacker's flags hold knocked out, knocked down or dead: for a throw, section 3.9 step 11, which is registered whether or not the projectile phase of step 6 ran (section 3.9, "Attacker weapon code when the projectile phase does not run"); otherwise unpoint (44), delay -1.

**Corpus projectile weapons.** `[Must-match]` Every corpus bullet, burst and flame weapon has projectile PID -1, so it shows no projectile and plays no hit sound at all. Its target reacts on the attacker's action frame. The corpus weapons that do fire a projectile are listed below; the thrown weapons are in section 3.9.

| PID | Weapon | Projectile art (`misc.lst` index, identifier) | Arrival |
| ---: | --- | --- | --- |
| 13 | Rocket Launcher | `rocket.frm` (9, `0x05000009`) | Explosive |
| 15, 24, 233 | Plasma Rifle, Plasma Pistol, Turbo Plasma Rifle | `plazpis.frm` (7, `0x05000007`) | Non-explosive |
| 16, 118 | Laser Pistol, Laser Rifle | `lazrpis.frm` (5, `0x05000005`) | Non-explosive |
| 120 | Alien Blaster | `elect.frm` (14, `0x0500000E`) | Non-explosive |

Plasma damage counts as explosive only for a throw. The plasma rifles and the Plasma Pistol therefore never explode. `[Must-match]`

#### 3.10.5 Projectile flight timing

`[Must-match]`

- **Path.** Let L be `max(|dx|, |dy|)` in screen pixels between the centers of the origin hex and the destination hex (SPEC-003 projection). The flight path has `floor(L / 32) + 1` nodes: one every 32 major-axis pixels, plus a final node on the destination. It always ends on the destination hex, and it never tests for blockers. When the origin hex is the destination, because the defender is adjacent, the path has one node.
- **Speed.** The projectile reaches one node per frame step: `trunc(1000 / (fps + combat_speed))` milliseconds. The fps is the projectile art's own rate, with stored 0 meaning 10.
  - Corpus rates: `knife.frm` 29 fps.
  - `spear.frm` and `rockthro.frm` store 0, so they use 10.
  - The other projectile arts store 10.

  The projectile art's frames advance once per step.
- **N.** On a hit, N is `floor(L / 32)`, the node count minus 1. On a miss with nothing struck, N is 0.
- **Hit sound (thrown, non-explosive).** The sound entry becomes next in the examination that starts the flight, which happens on the attacker's action frame. It counts down from there. With the flight as the only running entry, the sound starts on the flight step that reaches the second-to-last node. While the attacker's throw animation is still running, its steps count down too, so the sound starts earlier. Fixture G gives a worked case. The defender's reaction starts in the same examination as the sound.
- **Miss.** With N = 0 the hit sound starts together with the flight, when the projectile appears, not when it lands. On a miss the sound's target is the intended defender, which selects the SPEC-010 material letter.

#### 3.10.6 Reactions

Reactions are registered at one point in the sequence, in this order:

1. One placeholder for each extra victim that is a critter (section 3.10.7). The first placeholder carries the reaction delay D, and the rest carry 0.
2. Then, by attack result:
   - **Not a hit, and the critical or backwash bit (`0x400000`) is set:** the attacker's own reaction.
   - **Hit:**
     - the recorded defender's reaction, when that defender is a critter;
     - then, when the attacker's dud bit (`0x40000`) is set, the attacker's own reaction.

`[Must-match]`

The defender's reaction uses delay D only when there is no placeholder; otherwise its delay is 0. The attacker's own reaction always has delay -1, counts as from the front, and has no knockback. A struck wall or scenery object shows no reaction.

**From the front.** Section 3.8 compares stored orientations. For the recorded defender, the attacker's orientation is read while the sequence is being built, which is before the turn entry runs. Extra victims are evaluated after the main sequence has started, so the turn has already happened for them. `[May-improve]`: for the recorded defender, use the direction from the attacker's hex to the defender's hex as the attacker's orientation. Knockback direction always uses that hex direction (section 3.8).

**Reaction of critter R** with flags F, knockback distance k, direction d and delay D. `[Must-match]` The first matching row applies.

| Case | Entries |
| --- | --- |
| R is prone when the sequence is built (status bit `0x01` or `0x02`, or current animation 20-35 or 48-63) | If F has the dead bit `0x80` and R is not already dead: blood (below) with delay D, then the weapon effects and death callback below. Otherwise nothing at all, including no weapon effects. |
| F has the dead bit; the death animation (section 3.8) is 33 fire dance, and R has its art | Sound for 33, delay D. Animation 33 in place, delay 0. Then draw a distance `random(2, 5)` and a direction `random(0, 5)`. For distance j from that value down to 1, test a stride-4 straight path from R's hex to the hex j steps away in that direction. At the first path with no blocker, add a turn toward that hex (delay -1, so after the first fire dance) and a straight move to it using animation 33 (delay 0). When no path is free, R does not move. Then the sound for 29, delay -1, and animation 29 (burned to nothing), delay 0. |
| F has the dead bit; k ≠ 0 and the death animation is 20 or 21 | Knockback executor (section 3.8), delay D. It plays the sound for the fall with the knockdown extra, then animates in place or animates while sliding to the final hex, delay 0. Then blood, delay -1. No die sound plays. |
| F has the dead bit, otherwise | Die sound: the character sound for the chosen animation with the die extra, first letter `z` for 20 and 21 (SPEC-010), delay D. Apply the fall choice of section 3.8, which reverses 20 and 21 around obstacles. Death animation, delay 0. For 20 or 21, blood, delay -1. |
| F has knocked out (`0x01`) or knocked down (`0x02`) | Animation 20 from the front, else 21. Sound for that animation with no extra, delay D. If k ≠ 0, the knockback executor with delay 0, which plays the same effect name again with the knockdown extra. Otherwise apply the fall choice, then the animation with delay 0. R stays down; it stands up on its own turn (section 3.4): 37 after 20, 36 otherwise. |
| F has the on-fire bit `0x400` and R has art for 33 | Animation 33, delay D, with no sound. Then R's standing art, delay -1. |
| k ≠ 0 | Animation 20 from the front, else 21. Knockback executor, delay D. Then 37 if the final animation is 20, otherwise 36, delay -1. |
| Otherwise (hit flinch) | Animation 14 (hit from front, `ao`) when from the front or when R has no art for 15, otherwise 15 (hit from back, `ap`). Sound for that animation, delay D. The animation, delay 0. |

**Blood.** `[Must-match]` When violence is above 0 and the art exists, blood adds animation 34 after a fall of 20, or 35 after a fall of 21. It is a second animation that plays once the fall has finished, not a replacement for the fall; the corpse keeps its last frame. Without that art, nothing is added.

In the knocked-out or knocked-down case with knockback, the same effect name plays twice at once, because extras 0 and 1 build the same name. `[May-improve]`: play it once.

**Weapon effects and the death callback.** `[Must-match]` These follow the case entries, and they act on the attack's weapon, which is the attacker's. They matter for the attacker's own reaction after a critical failure.

- Explode bit `0x1000`:
  - a forced callback drops the weapon at R's hex, delay -1;
  - the weapon's art becomes `roktxpd.frm` (`0x0500000A`), delay 0;
  - it animates and hides, delay 0;
  - the weapon hit sound plays for the right-hand primary mode with R as the target, delay 0;
  - a forced erase follows, delay -1.
- Destroy bit `0x2000`: a forced callback destroys the weapon, delay -1.
- Drop bit `0x4000`: a forced callback drops the weapon, delay -1.
- Dead bit: a forced callback applies the corpse presentation of section 3.8, delay -1.

**Reaction animation summary.** Codes and suffixes are from section 2.2.

| Reaction | Codes |
| --- | --- |
| Dodge | 13 (`an`, or weapon letter then `e`) |
| Hit flinch | 14 `ao` front, 15 `ap` back |
| Knockdown, knockout, knockback slide | 20 `ba`, 21 `bb` |
| Stand up | 37 `cj` after 20, 36 `ch` after 21 |
| Bloody falls | 34 `bo`, 35 `bp` |
| Gore deaths | 23 `bd` big hole, 24 `be` charred, 25 `bf` chunks, 26 `bg` autofire dance, 27 `bh` electrify, 28 `bi` sliced in half, 30 `bk` electrified to nothing, 31 `bl` exploded to nothing, 32 `bm` melted |
| Fire | 33 `bn` fire dance, 29 `bj` burned to nothing |
| Static corpse frames | 48-63 (`ra`-`rp`), used by direct death requests and map loading, not by this sequence |

#### 3.10.7 Extra victims and draws made while building

`[Must-match]`

- **Extras.** After the main sequence is registered, and so has started, each extra victim that is a critter gets its own reserved, prioritized sequence holding only its reaction. Its from-the-front value compares the attacker's orientation after the turn with the extra's own. Its knockback direction runs from the attacker's hex to its hex, and its delay is 0. Each extra sequence is held until the main sequence reaches its placeholder (section 3.10.6). All critter extras therefore begin reacting when the recorded defender does. Extras that are not critters get nothing. In R1 no explosion has extras (section 3.7), so extras come only from burst and continuous fire.
- **Draws.** Building the sequence makes random draws after every draw of sections 3.6 and 3.7, in this order:
  1. the attack taunt (SPEC-009 §3.17);
  2. the fire-dance distance and direction of each reaction in the main sequence, in registration order;
  3. the hit or miss taunt;
  4. the fire-dance draws of each extra, in extras order.

  A draw is made only when its condition holds: taunts enabled and the speaker eligible, and the fire-dance death chosen.

### 3.11 Non-Combat Animation

This section covers every animation the engine itself starts outside an attack: walking and running, idle fidgets, interaction reaches, ladders, doors and containers, and knockback or getting up outside combat. Sequences, delays and frame steps follow section 3.10.1. Scripts can start any animation through the SPEC-005 animation opcodes; this section only covers what the engine starts.

**Suffix corrections.** `[Must-match]` These follow from the section 2.2 suffix table:

- `aa` is standing (0), and walking (1) is `ab`, or the weapon letter then `b`.
- `al` is the middle-height reach (11), and the high reach (12) is `am`.
- `ae` is the ladder climb (4), and `af` is falling (5).
- `ap` is hit from behind (15); knockdown falls are 20 (`ba`) and 21 (`bb`).

#### 3.11.1 Walking and running

**Art.** `[Must-match]` A walking step uses animation 1, whose filename depends on the weapon code: `ab` for code 0, otherwise the weapon letter then `b`. Running uses animation 19, `at`, whose filename ignores the weapon code, so an armed runner shows unarmed running art. A run request walks instead (animation 1) when any of these holds:

- the mover is a critter with a crippled leg (bit `0x04` or `0x08`);
- the mover is the player, sneaking, without the Silent Running perk;
- the mover has no art for animation 19.

A player run request made by a click also clears sneaking unless the player has Silent Running.

**When the engine runs.** `[Must-match]`

| Request | Out of combat | In combat |
| --- | --- | --- |
| Player click to move (SPEC-014 §3.27) | Run when the `running` preference is on; Shift inverts it | Same rule, AP-limited |
| Pick up an item, use an object or scenery, climb a ladder, open a container's loot | Run when the object distance is 5 or more, otherwise walk | Walk, limited to current AP |
| Use a skill on a target | Run at distance 5 or more, otherwise walk; no movement when the target is the player | Walk, AP-limited |
| Talk | Run when the distance is 9 or more, or the line to the target is shot-blocked (section 3.2); otherwise no approach movement | Walk, AP-limited |

A move toward an object is followed by a turn toward that object's hex, with delay -1. A move toward a hex is not followed by a turn. If the player is already walking or running when a new interaction starts, the player's current sequence is cancelled first.

**Step model.** `[Must-match]` A walk or run entry plans its route when it starts (section 3.1). It then takes one frame step per `trunc(1000 / fps)` milliseconds, with the `combat_speed` rule of section 3.10.1. On each step:

1. **First step:** re-place the mover on its own hex, which zeroes its pixel displacement. Set frame 0, face the route's first direction, and switch to the walk or run art. **Later steps:** advance one frame, wrapping from the last frame to 0.
2. Add the shown frame's per-frame displacement for the current facing to the mover's displacement (SPEC-001 §3.5).
3. Let (dx, dy) be the pixel step to the next hex in the facing direction. NE is (16, -12), E (32, 0), SE (16, 12), SW (-16, 12), W (-32, 0) and NW (-16, -12). The mover **crosses** when its displacement reaches the step on either axis: x ≥ dx for positive dx, x ≤ dx for negative dx, and the same test for y.
4. On a crossing, the residual is the displacement minus (dx, dy). Let H be the neighboring hex in that direction.
   - If H holds a movement blocker that the mover may not auto-open (section 3.1), the route is rebuilt from the current hex with a strict goal. On success the mover is re-placed on its hex (displacement 0) at frame 0, facing the new first direction, and the route cursor restarts. On failure the move ends.
   - If H holds a door the mover may auto-open, the door is used as in section 3.11.4 and the move continues into H at once.
   - Otherwise the mover moves to H, which zeroes its displacement. In combat, AP is charged for the hex (section 3.4). The move ends when the route is complete, or in combat when AP plus free movement reach 0. Otherwise the mover faces the next direction and its displacement is set to the residual.

There is no interpolation between hexes. The visible speed comes entirely from the art's per-frame displacements and frame rate. The frame counter continues across hexes and direction changes, and a residual from one hex carries into the next. When the move ends, the last residual is dropped, so the mover stands exactly on its hex. Fixture H gives a worked case.

**Sounds.** `[Must-match]` Walking and running play no footstep or movement sound. Exit grids under the player's new hex are tested whenever the player moves onto a hex (SPEC-013 §3.22).

#### 3.11.2 Idle fidgets

`[Must-match]` Outside combat, a background task runs while a map is loaded. It is removed when combat starts and added again when combat ends. It does nothing while a quit request is pending, during self-run playback, or while the player is hidden. Otherwise, when the time since its last run exceeds the current wait W (initially 0):

1. Collect up to 100 candidates on the player's elevation, in object order. A candidate is a visible critter whose current animation is 0 (standing), that is not dead, and whose bounding rectangle intersects the 640-by-480 screen.
2. If there is at least one candidate, draw `random(0, count - 1)` to pick one. Register an unreserved, insignificant sequence: when the candidate is the player, or its critter art stem starts with `m` or `M` and its distance to the player is below twice the player's Perception, first the character sound for animation 0 with delay 0. Then animation 0 once, with delay 0. Any later sequence that names the same critter ends this one.
3. Let k be `trunc(20 / count)` with candidates, or 7 without, clamped to 1 through 7. Set W to `random(0, 3000) + 1000 × k` milliseconds.

These draws use the shared generator (SPEC-006 §3.4), so idle fidgets interleave with other draws outside combat.

#### 3.11.3 Interaction animations

`[Must-match]` unless tagged.

| Action | Entries after the approach |
| --- | --- |
| Pick up an item (SPEC-006 §3.8) | Adjacency check, forced, -1. The 3-AP combat check, -1. Reach to the ground: animation 10 (`ak`), delay 0. A sound named after the item's art stem, delay A. The pickup, delay A. |
| Use a container that cannot be picked up | Adjacency and AP checks. If armed, the put-away sound (-1) and put-away animation 39 (-1). Reach (below), delay 0. The container use of section 3.11.4, delay -1. If armed, take out the weapon again, -1. If the container was closed when the sequence was built, or its PID is 213, the loot window request (SPEC-013 §3.16), -1. |
| Use scenery or an object, or use an item on one | Adjacency and AP checks. If armed, the put-away sound (-1) and animation 39 (delay 0). Reach (below), delay -1. The use (section 3.11.4, or SPEC-006 §3.8 for an item used on a target), delay -1. If armed, take out the weapon again, -1. |
| Climb a ladder up | SPEC-013 §3.23: approach the hex southeast of the ladder, turn, the AP check, the put-away, then the climb sound and animation 4 (`ae`), then the use. |

A is the action frame of the reach art for the mover's current base and weapon code, or -1 when that art cannot be loaded. With -1, the pickup waits until the reach has finished.

**Reach height.** `[Must-match]` For a prone critter target, or scenery whose prototype extended flags have bit `0x01` set, the reach is the ground reach (10, `ak`); otherwise the middle reach (11, `al`).

- For a container, prototype payload word `0x04` bit 0 **set** selects the ground reach. Clear selects the middle reach.
- In the corpus, the bit is set on the Ice Chests, Bones, Footlockers and the Crate, and clear on bookcases, desks, lockers, shelves, boxes and every other container.
- The high reach (12, `am`) is never started by the engine. Animations 2, 3 and 5-9 (jump, falling, stairs) are never started by it either; a script must request them.

**Container reach timing.** This rule is authoritative, and SPEC-013 §3.16 states the same rule.
- The container branch looks up the chosen reach art, ground or middle, with weapon code 0, because the weapon has been put away by then. The lookup is meant to time the container use from that art's action frame.
- Its art test is inverted. When the art loads, the action frame is not read, so the delay of the container use is -1. Only when the art fails to load does it try to read an action frame, from art that does not exist.
- Every corpus critter has both reach arts. The container use therefore always waits until the reach animation has finished. `[Must-match]`
- `[May-improve]`: when the reach art loads, read its action frame A and give the container use delay A, as the pickup does. The container then opens on the reach's action frame, and its sound and animation overlap the rest of the reach. When the art does not load, use delay -1 and read nothing.
- The entries after the container use keep delay -1 under either rule: the weapon take-out and the loot window request (SPEC-013 §3.16). The loot window therefore still opens only after the reach and the container animation have both finished.

**Adjacency check.** `[Must-match]` Every engine interaction that approaches a target follows the approach with the same forced check: object distance at most 1, otherwise the sequence ends. This covers pickup, container use, scenery or object use, using an item on an object, ladder climbing, skill use (SPEC-013 §3.2) and looting a critter (SPEC-013 §3.16).
- The check prints `misc.msg` 2000 ("You cannot get there.") only when the **target** is the player. It never prints because the actor is the player.
- A player whose approach fails therefore sees no message, for ladders and every other interaction. The player sees 2000 only when another critter's interaction aimed at the player fails.
- `[May-improve]`: print it when the actor is the player, and not because the target is.
- Talk is the one exception, and its check is correct. It prints 2000 when the actor is the player, after the approach, if the line to the target is shot-blocked or the object distance is 9 or more. Out of combat, a failed talk is therefore the only case where the player is told "You cannot get there."

**Player clicks during a sequence.** What a click does while the player's reach, use or other sequence is still playing is given in SPEC-014 §3.27 ("Clicks while the player is busy"), using the "One sequence per owner" rule of section 3.10.1.

PID 213 (Remains of Gizmo) skips the container use and always requests the loot window.

#### 3.11.4 Doors and containers

**Door use.** `[Must-match]` unless tagged. Using a door, by a click, a script request or an NPC auto-opening it during a walk, runs in this order:

1. If the door is locked, play its locked sound at once, outside any sequence: open/close name with action `L` (SPEC-010 §3.8).
2. Run the door's `use_p_proc` **before** any animation, with the user as source and the door as target. If the script sets its override, stop here; the door does not move.
3. If the door's frame is not 0, **close** it. Register a reserved sequence:
   - set the open bit `0x01` of the door's instance open-flags word to clear, -1;
   - play the close sound (action `C`), -1;
   - animation 0 of the door's art in reverse, delay 0.
4. If the frame is 0 and the open bit is already set, do nothing. Otherwise **open** it:
   - set the open bit, -1;
   - play a sound, -1;
   - animation 0 forward, delay 0.

   The sound is again the close sound (`C`), not the open sound. `[May-improve]`: play the open sound (`O`), as the script open request does.
5. Last, a forced finishing callback, -1 (below).

A locked door is not refused here. Unless its script overrides, it plays the locked sound and then opens. Corpus door scripts enforce locks through the override. Nothing tests for an object standing in the doorway; a door closes on a critter in it. Scenery that is not a door runs its `use_p_proc` with the same override rule. Without an override the player sees `proto.msg` 480. Then the map-update scripts run.

**Animation.** `[Must-match]` A door or container uses animation 0 of its own art. Forward play starts from the current frame and stops on the last frame; reverse play starts from the current frame and stops on frame 0. Each frame adds its per-frame displacement going forward and subtracts it in reverse (SPEC-001 §3.5).

**Finishing callback.** `[Must-match]` unless tagged.

- **Open bit clear:** clear the open-door flag group `0xA0000010` (movement-nonblocking `0x10`, light-through `0x20000000`, projectile-through `0x80000000`) and rebuild lighting. If the frame is not 0, subtract displacements and set frame 0. The reference subtracts the displacements of frames from (current − 1) down to 0, which is one frame off. `[May-improve]`: subtract those of the current frame down to frame 1, the exact reverse of forward play.
- **Open bit set:** set that flag group and rebuild lighting. If the frame is not the last, add the displacements of each later frame and set the last frame.

Blocking, sight and projectile passage therefore change only **after** the animation:

- an opening door blocks movement, sight and shots until it is fully open;
- a closing door stays passable until it is fully shut.

The open bit itself flips as the sequence starts. Section 3.1 and section 3.2 give how these flags affect routes and rays.

**Container use.** `[Must-match]`

1. If the container is locked: play its locked sound (`L`); for the player, show `proto.msg` 487 ("It is locked."); and fail.
2. Run its `use_p_proc`; an override stops here.
3. Register a sequence: at frame 0, the open sound (`O`) with delay 0 and animation 0 forward; otherwise the close sound (`C`) with delay 0 and animation 0 in reverse.
4. For the player, show `proto.msg` 485 ("You search the %s.") when the frame was 0, or 486 ("You close the %s.") otherwise.

No flag word changes. An open container is one whose frame is not 0 (SPEC-005 `obj_is_open`).

**Script open and close.** `[Must-match]` `obj_open` and `obj_close` (SPEC-005) act only on a door or container that is in the opposite state.

- A locked object is refused, with no sound.
- Otherwise its jam is cleared, and no `use_p_proc` runs.
- The sequence is: set or clear the open bit as a forced callback (-1); the open sound (`O`) or close sound (`C`), -1; the animation forward or reversed, delay 0; and the finishing callback.
- For a container, the open bit is kept in a word its saved record does not hold, and the flag group has no blocking effect, because items never block.

#### 3.11.5 Knockback and getting up outside combat

`[Must-match]`

- **Script damage.** `critter_damage` (SPEC-005) with animation enabled uses a hidden temporary attacker at the victim's hex, with art `0x020001F5` and facing 0. One sequence plays `whc1xxx1` and then the section 3.10.6 reaction with delay 0.
  - From the front: the victim's facing is SE, SW or W.
  - Knockback is `trunc(damage / 10)` hexes for a victim that is not multihex and damage that is not electrical, or 0 when armor is bypassed. The direction is SE, because the direction from a hex to itself is SE.
  - A death uses the maximum-violence column of section 3.8 for the damage type, under maximum-violence validation.
  - Without animation, a lethal amount kills the victim at once, with no animation.
- **Knockback alive.** The victim slides and falls (20 or 21), then stands up at once with 37 or 36 (section 3.10.6). The knocked-down status is not set.
- **Knockout wake-up.** When a knockout's wake event fires outside combat, it replaces knocked out with knocked down and immediately plays the stand-up: 37 when the current animation is 20, otherwise 36. That clears knocked down. In combat it only marks the critter, which stands up on its turn (section 3.4).
- **End of combat.** A prone player who is not dead has the wake event removed and stands up at once, unless a script ended combat while the player was knocked out. A player still knocked out at the end of a main-loop pass ends the game (SPEC-014 §3.5).

### 3.12 Combat Message Lines

The same routine prints the message-monitor lines (SPEC-014 §3.20) for three things: every combat attack, once its attack animation has finished and just before its damage is applied; every explosion; and scripted damage (`critter_damage`, SPEC-005). Text comes from `combat.msg`. "Dead" below means the critter's stored damage flags already had the dead bit `0x80` before this attack's damage was applied, so a critter that this attack kills still gets its lines. Every line ends with a `.` appended by the engine. `[Must-match]`

**Names and bases.** For the player the name is `combat.msg` 506 "You" and the base is 500, whatever the player's gender. For any other critter the name is its object name, and the base is 600 for a male and 700 for a female. The **main critter** is the struck target when the attack hit, and the attacker otherwise. B is the main critter's base. `[Must-match]`

Lines are printed in this order, each only when its condition holds. `[Must-match]`

1. **Strength.** When the player attacks with a weapon whose minimum strength is above the player's strength: 107 "You are not strong enough to use this weapon properly." (no extra `.`).
2. **Wrong target.** When the attack hit, and the struck target differs from the critter aimed at:
   - If the struck object is a critter and the aimed-at critter is the player: B + 8 "Oops! %s was hit instead of you!" with the struck critter's name.
   - If the struck object is a critter and the aimed-at critter is someone else: B + 9 with the struck critter's name, then the aimed-at critter's name.
   - If the struck object is not a critter: 515 "%s missed" with "You" when the player attacked, otherwise 615 or 715 by the attacker's gender with the attacker's name.

   The appended `.` follows the message's own `!`, giving for example "Oops! Ian was hit instead of you!.".
3. **Struck target.** When the attack hit, the struck object is a critter, and it is not dead:
   - **Torso** (location 3; uncalled attacks are normalized to torso, section 2.4): a critical hit uses B + 28 (no damage), B + 24 (1) or B + 20 (otherwise); a normal hit uses B + 27, B + 23 or B + 13.
   - **Other locations:** the location name is `combat.msg` 1000 + 10 × the critter's art alias + location, as in the called-shot window (SPEC-014 §3.38). A critical hit uses B + 25, B + 21 or B + 11; a normal hit uses B + 26, B + 22 or B + 12. If that name is missing, the line starts empty.
   - Damage 0 and 1 use the wording without a number. Any other value, including a negative one, is formatted with `%d`.
   - Then the **tail** is added, which is where Brief and Verbose differ (below).
4. **Attacker.** When the attacker exists and is not dead:
   - On a miss: a critical miss uses B + 14 (attacker damage 0), B + 33 (1) or B + 34 (otherwise), and a normal miss uses B + 15. The attacker's status list (below) for its own damage flags is appended.
   - On a hit, or on a miss that is not critical, a second attacker line follows when the attacker took damage above 0: the normal-hit torso wording for the attacker (its own base, 13, 23 or 27 by damage) followed by its status list.
5. **Extra victims.** For each additional critter hit by a spray, an explosion or a knock-on, in order and unless dead: the normal-hit torso wording for its damage, followed by its status list. Damage 0 prints "… was hit for no damage".

**Status list** for a critter C and damage flags F. The base S is 200 for the player, whatever the player's gender, 300 for a male and 400 for a female. `[Must-match]`

- F = 0: nothing.
- F includes dead (`0x80`): 108 " and " followed by S + 7 ("were killed" or "was killed"). Nothing else is listed.
- Otherwise, take every set bit of F except hit (`0x100`) and critical (`0x200`), from bit 0 upward. Each bit but the last adds ", " and then S + the bit number (for example 301 "knocked down" for bit 1). The last bit adds 108 " and " and then the message numbered by the **bare bit number**, 0 to 31. `combat.msg` has no such entries, so nothing follows " and ". A single knocked-down flag therefore reads "Raider was hit for 5 hit points and ." `[May-improve]` Safe rule: use S + the bit number for the last flag as for the others ("… and knocked down.").

**Brief and Verbose.** `[Must-match]` The `fallout.cfg` key `combat_messages` is read at every struck-target line; a missing key counts as 1. Value 1 is Verbose and 0 is Brief (SPEC-014 §3.18). The setting changes only the tail of line 3, and only for a critical hit whose critical-effect message was chosen by section 3.6 from the matrices of section 2.5:

- **Verbose, critical hit:** append the critical-effect message text, which is the ordinary message or, after a failed save, the failed-save message, for example 5004 ". The blow knocks him to the ground". The target's status list is **not** appended. If the target's damage flags include dead, the line is printed, and then a second line follows: the main critter's name, a space, and "were killed" (207 for a male player, 257 for a female player) or "was killed" (307 for a male, 407 for a female).
- **Brief, or any non-critical hit:** append the target's status list.

Every other line is identical in both settings. The strength, wrong-target, attacker and extra-victim lines always use the status list, and a non-critical hit prints the same line in both.

## 4. Synthetic Test Fixtures (Hex / Text)

These vectors are invented and require no shipped artwork or live host execution. Resource-existence sets, blocker objects, effective stats and random outcomes are explicit test inputs. Hex dumps contain data only; none is a complete map/prototype file.

### 4.1 Fixture A: Critter FID-to-Filename Codec

Use these three zero-based critter-list rows, all LF-terminated:

```text
ranger,1
mutant,0
slimex,1,999
```

Rows 0/1 alias each other, but resolution does not recurse. Row 2 aliases row 1; its trailing 999 is not another alias selection. Directly resolving the following numeric FIDs, without running constructor existence fallback, must give:

| Input FID | Expected filename within `art/critters/` | Purpose |
| --- | --- | --- |
| `0x01000000` | `rangeraa.frm` | Unarmed stand |
| `0x01005000` | `rangerha.frm` | Pistol stand |
| `0x01015000` | `rangerhb.frm` | Pistol walk |
| `0x010D5000` | `rangerhe.frm` | Pistol dodge |
| `0x01121000` | `rangerdm.frm` | Knife throw exception |
| `0x01124000` | `rangergm.frm` | Spear throw exception |
| `0x01122000` | `rangeras.frm` | Club throw uses ordinary throw |
| `0x01241000` | `rangerch.frm` | Prone-to-standing |
| `0x012D5000` | `rangerhj.frm` | Pistol single fire |
| `0x011B5000` | `mutantbh.frm` | Electrify aliases row 0 to row 1 once |
| `0x011B5002` | `mutantbh.frm` | Row 2 uses first alias only |
| `0x01400000` | `mutantna.frm` | Called-shot image exception |
| `0x013D0000` | `rangerrn.frm` | Static fire dance 61 does not use the alias rule for 33 |
| `0x31145000` | `rangerba.fr2` | File selector 3, death ignores valid weapon letter |
| `0x61155000` | `rangerbb.fr5` | File selector 6 |
| `0x311B5000` | `mutantbh.fr2` | Direction preserved across alias |
| `0x31215000` | `mutantbn.fr2` | Raw resolver honors selector even though normal fire-dance construction clears it |
| `0x71145000` | `rangerba.fr6` | Raw selector 7 formatting, not a valid seventh direction |
| `0x012D0000` | No filename | Weapon action 45 with weapon code 0 rejected |
| `0x0100B000` | No filename | Weapon code 11 rejected before suffix selection |

Three consecutive FID words, big-endian, can be represented by:

```text
0000: 01 2D 50 00 31 14 50 00 01 40 00 00
```

They select `rangerhj.frm`, `rangerba.fr2`, and `mutantna.frm`, respectively.

Constructor tests below use category 1/base 0/weapon 5, requested physical facing 2, hence requested selector 3. Only the listed resources exist:

| Animation | Existing resources in critter directory | Returned FID | Resolved filename |
| ---: | --- | --- | --- |
| 20 | `rangerba.fr2` | `0x31145000` | `rangerba.fr2` |
| 20 | `rangerba.fr0` | `0x11145000` | `rangerba.fr0` |
| 20 | `rangerba.frm` | `0x01145000` | `rangerba.frm` |
| 20 | None | `0x01145000` | `rangerba.frm`, subsequently missing |
| 20 | `rangerba.fr5` only | `0x01145000` | `rangerba.frm`, not a scan to direction 5 |
| 27 | `mutantbh.fr2` | `0x311B5000` | `mutantbh.fr2`, original base index retained |
| 33 | `mutantbn.fr2` and `mutantbn.frm` | `0x01215000` | `mutantbn.frm` without directional probing |
| 45 | `rangerhj.fr2` only | `0x012D5000` | `rangerhj.frm`, subsequently missing |

As a separate category-overlay check, head-list row 0 stem `talker` with FID `0x08014000` gives `talkergf4.frm`: head selector 1, variant 4. It does not use the critter pistol-letter rule.

### 4.2 Fixture B: Exact Routes Around Blockers

All hexes are on elevation 0. Unless a case overrides it, actor is the player, biped, non-multihex, facing 5; source is 20100 and goal is 20298, with strict-goal enabled. Every unlisted hex is empty. A wall means visible blocking wall art with basic flags zero. A door means visible blocking scenery-door art with an available prototype. Multihex obstacle means visible blocking critter with bit `0x800`. These are core planner tests, not promises that a permissive occupied-goal animation later finishes.

| Case | Combat state / blockers | Expected direction bytes | Length | Forward hex sequence |
| --- | --- | --- | ---: | --- |
| Clear | Combat, no blockers | `01 01` | 2 | `20100,20299,20298` |
| Wall | Combat, wall 20299 | `02 01 00` | 3 | `20100,20300,20499,20298` |
| Wall with turn surcharge | Outside combat, same wall | `00 01 02` | 3 | `20100,20099,20098,20298` |
| Player door | Combat, unlocked door 20299 | `02 01 00` | 3 | `20100,20300,20499,20298` |
| NPC biped door | Combat, non-player biped, same unlocked door | `01 01` | 2 | `20100,20299,20298` |
| NPC robotic door | Combat, non-player robotic, same unlocked door | `01 01` | 2 | `20100,20299,20298` |
| NPC locked door | Combat, non-player biped, locked door 20299 | `02 01 00` | 3 | `20100,20300,20499,20298` |
| NPC quadruped door | Combat, non-player quadruped, unlocked door 20299 | `02 01 00` | 3 | `20100,20300,20499,20298` |
| Strict blocked goal | Combat, wall 20298 | Empty | 0 | No route; reject before search |
| Permissive blocked goal | Combat, wall 20298, strict-goal false | `01 01` | 2 | `20100,20299,20298` |
| Multihex footprint | Combat, goal changed to 20496, multihex obstacle 20298 | `02 02 01 01 00 00` | 6 | `20100,20300,20500,20699,20698,20697,20496` |
| Multihex footprint with surcharge | Outside combat, same changed goal/obstacle | `00 00 01 01 02 02` | 6 | `20100,20099,19898,20097,20096,20296,20496` |
| Moving multihex actor | Combat, same changed goal/obstacle, actor also multihex | `02 02 01 01 00 00` | 6 | Same core search; no extra mover-footprint dilation |
| Outer border | Combat, source 0, goal 1, no blockers | Empty | 0 | Directional helper never leaves source 0 |

In the wall cases, the combat path is not the lexicographically first NE route despite examining NE first. Low-slot frontier reuse affects the tie. Outside combat, initial incoming direction 0 and turn penalties select the NE route even though actor facing was 5.

For a controlled ray test on the original source/goal, put a blockable defender on 20298 and one ordinary blockable critter on 20299. Explicitly initialize the ignored-object reference to null, as the section 3.2 `[May-improve]` safe rule does, avoiding the host wrapper's uninitialized-slot defect:

- Stride-16 LOS reports the intervening critter, not defender: controlled LOS answer false.
- Stride-32 shooting resumes through that critter: hard obstruction false, cover count 1, accuracy cost 10.
- Replace the intervening critter by an ordinary wall: hard obstruction true.
- Give that wall projectile-through bit `0x80000000`: stride 32 skips it, while stride 16 still regards it as a movement blocker.
- Give the defender movement-nonblocking bit `0x10` and remove intermediate blockers: stride 16 does not report defender, so controlled LOS is false despite clear geometry/perception.

### 4.3 Fixture C: Worked Aimed Rifle Attack and Critical Variant

Assume successful animation/art setup, no scripted overrides, no initial request bonuses, no events changing state during the attack, and absolute timestamp at least 864000. Shooter is player at hex 20100 facing east; defender is a non-player man-category critter at 21685, fifteen east steps away, facing west. An ordinary intervening critter at 20892 provides one cover body but not hard obstruction. All are non-multihex on elevation 0.

| Input | Value |
| --- | --- |
| Effective shooting skill / Perception / Strength / Critical Chance / Better Criticals | `80 / 6 / 5 / 5 / 20` |
| Player AP / loaded rounds | `8 / 6` |
| Weapon | Right primary, single fire, normal damage, range 20, minimum Strength 4, primary cost 5, damage interval 8-20 |
| Attack | Aimed right leg, location 4 |
| Target HP / base-and-bonus AC / unused AP / effective AC | `60 / 10 / 2 / 12` |
| Target normal DT / DR / Endurance | `4 / 25 / 5` |
| Ambient / true target tile light / visible light | `50000 / 60000 / 60000` |
| Stored ammunition modifiers | AC -10, DR -20, damage multiplier 2, divisor 3; all four ignored by this attack path |
| Other modifiers | No accuracy/range/penetration/knockback weapon property, relevant player trait, Sniper, ranged-damage or rate-of-fire bonus; normal difficulty |

Range term is `15 - 2 * 6 = 3`, hence -12 accuracy. Cover is -10, effective target AC -12, leg adjustment -20; visible light is above 52428 and supplies zero. Other terms are zero. Final hit chance is **26**, not 36 from applying ammunition AC -10. Aiming costs `5 + 1 = 6 AP`.

For the normal-hit run, supply these random outcomes in order: attack D100 20, critical-upgrade D100 50, base-damage draw 18. Margin is 6; critical threshold is `trunc(6 / 10) + 5 - (-20) = 25`, so roll 50 does not upgrade. Normal numerator 2 and difficulty 100 leave 18; subtract DT 4 to get 14; subtract truncated `14 * 25 / 100 = 3` to get **11 damage**. Multiplying 14 by 75 percent and truncating instead would incorrectly give 10.

After successful sequencing/cleanup: attacker AP 2, magazine 5, defender HP 49, no new persistent status, knockback zero. The stored ammunition multiplier/divisor/DR modifier do not change that result.

For an independent critical variant reset the same starting state and use outcomes: attack D100 20, upgrade D100 1, severity D100 80, saving D10 8, base damage 18. Upgrade succeeds; severity is `80 + 20 = 100`, tier 4. Victim table 0/location 4/tier 4 is `4/80A/2/0/1/5025/5026`. Endurance 5 fails against 8, so resolved defender mask is `0x80B`, with alternate message 5026.

Normal-damage bypass removes DT and DR, so damage is `18 * 4 / 2 = 36`. Final HP is 24; AP 2 and ammunition 5 remain the same as the normal run. Persistent status is `0x0000000B` (knockout, knockdown, right-leg cripple), not the transient armor-bypass bit. Knockout wake-up is queued for `10 * (35 - 3 * 5) = 200` ticks. Rifle damage still supplies no knockback displacement just because the critical has a knockdown flag.

Example final field bytes, big-endian, in the order AP, ammunition, HP, persistent status; these are individual field values, not a complete instance record:

```text
normal:   00 00 00 02 00 00 00 05 00 00 00 31 00 00 00 00
critical: 00 00 00 02 00 00 00 05 00 00 00 18 00 00 00 0B
```

Additional boundary checks on the same rules:

- A critical-failure weapon table 2, Luck 7, failure draw 60 gives score 50, column 1, mask `0x20000`; ranged failure schedules the whole six-round magazine for consumption.
- A separate melee result of 35 damage gives computed knockback 3. Clear space permits 3; a wall at displacement 2 produces actual displacement 0 because of the double decrement; a wall at displacement 3 permits 1. These are reference values; the section 3.8 `[May-improve]` safe rule gives 1 and 2.
- Before timestamp 864000, without a forced-upgrade trait/perk, the normal D100 critical-upgrade draw is not made. Slayer/Sniper/Jinxed postprocessing must not be disabled merely by calling it the first day.

### 4.4 Fixture D: Punch-to-Kick Draw Position

Shared inputs: an NPC attacker whose AI makes no called-shot draw, with bare hands, art weapon code 0 and effective Melee Damage 1, so the bare interval is 1-3. It attacks an adjacent defender. Hit chance is 60, effective Critical Chance 5, torso location adjustment 0, and absolute timestamp at least 864000. Random outcomes are listed in draw order.

| Case | Requested mode | Kick art present | Outcomes | Result |
| --- | ---: | --- | --- | --- |
| D1 | 4 | Yes | `1, 40, 70, 3` | Kick draw 1 selects mode 5. Attack 40 gives margin 20; upgrade threshold `2 + 5 - 0 = 7`, so 70 does not upgrade. Damage 3 |
| D2 | 4 | No | `1, 40, 70, 3` | Same draws consumed; mode stays 4; damage 3 |
| D3 | 4 | Yes | `2, 40, 70, 3` | No probe; mode 4; damage 3 |
| D4 | 5 | Yes | `40, 70, 3` | No kick draw; the first draw is the attack D100 |
| D5 | 4 | Yes | `4` | Defender out of range 1: the attack fails after consuming only the kick draw |

D1 and D2 differ only in recorded mode, animation 17 against animation 16, and sounds. AP debit is 3 in D1 through D4; D5 debits nothing.

### 4.5 Fixture E: Missed Single Shot and Bystanders

The player fires a single-fire rifle: normal damage, range 10, damage 8-20, no weapon perk, normal difficulty, no relevant traits or perks, timestamp at least 864000. Ray results are explicit test inputs. Along the line from the attacker toward E are a living critter X1, then the intended defender, then a living critter X2. A stride-32 cast reports them in that order and reports nothing beyond X2 before E. Section 3.5 hit chances are 50 against the defender, 45 against X1 and 30 against X2. X2 has normal DT 2 and DR 20; X1 has DT 0 and DR 0.

| Case | Outcomes | Reference result | Stage A safe-rule result |
| --- | --- | --- | --- |
| E1 | `90, 30, 10, 12` | Attack fails (margin -40); 30 exceeds `trunc(40 / 10) = 4`, so no critical failure. Stage A: X1 chance 15, draw 10 selects X1, which is not committed. Stage B: the ray from the defender reports X2, and damage draw 12 gives `12 - 2 = 10`, then `10 - trunc(10 * 20 / 100) = 8`. **X2 takes 8**; X1 is untouched | X1 committed; draw 12 gives **X1 12**; Stage B skipped; same draw count |
| E2 | `90, 30, 50, 60, 12` | Stage A: X1 draw 50 fails; the defender is passed with no draw; X2 chance 10, draw 60 fails; the cast then reaches E. Stage B selects X2: **X2 takes 8** | Same as reference: no Stage A candidate |
| E3 | `90, 30, 50` | X2 replaced by a wall at the same place. Stage A: X1 draw 50 fails, defender passed, wall gives an obstruction. Stage B: the ray reports the wall, which gets the hit bit and the "missed" message, with **0 damage and no damage draw** | Same as reference |

Boundary check: when E is the defender's own hex and no other blocker is present, outcomes `90, 30, 12` (with no critter on the line in Stage A) give the intended defender one round at numerator 2. The fixture's damage interval and the defender's DT/DR then apply as in section 3.6.

### 4.6 Fixture F: Thrown-Weapon Consumption

Player attacker; the right hand is the active hand. Each row is independent. "Hand" lists hand-flagged entries after the attack. `[Must-match]` except where tagged.

| ID | Before | Attack result | Hand after | Other inventory after | Map after | Attacker art after |
| --- | --- | --- | --- | --- | --- | --- |
| F1 | Throwing Knife ×3 in hand | Hit | Throwing Knife ×2 (copy, new object ID) | none | One Throwing Knife at the defender's hex, item art restored | Standing, weapon code 0 (`[May-improve]`: take out code 1) |
| F2 | Frag Grenade ×1 in hand; Frag Grenade ×3 unequipped | Miss, impact hex T | Frag Grenade ×1 (refill) | Frag Grenade ×2 | Nothing: the grenade explodes at T and is erased | Standing, weapon code 0 (refill code is 0) |
| F3 | Spear ×1 in hand; Spear ×1 unequipped | Hit | Spear ×1 (refill) | none | One Spear at the defender's hex | Takes out weapon code 4 |
| F4 | Rock ×1 in hand | Critical failure, nonzero mask | Rock ×1, unchanged unless the failure effect drops or destroys it | unchanged | No projectile | Per section 3.6 critical failure |
| F5 | Throwing Knife ×2 in hand; Throwing Knife ×1 unequipped | Hit | Throwing Knife ×1 (copy) and Throwing Knife ×1 (refill): two hand-flagged entries, `[May-improve]` | none | One Throwing Knife at the defender's hex | Takes out weapon code 1 |
| F6 | Flare ×1 in hand | Miss, impact hex T | empty | none | One Flare at T, with no light | Standing, weapon code 0 |
| F7 | Lit flare (PID 205, radius 8, intensity 65536) ×1 in hand; the player's light was raised to radius 8 by wielding it | Hit | empty | none | One lit flare at the defender's hex, still radius 8 and intensity 65536, its burn-out event still queued | Standing, weapon code 0; the player's light stays at radius 8 (`[May-improve]`: restored to radius 4) |

### 4.7 Fixture G: Attack Sequence Timing

Every animation and the projectile run at 10 fps with `combat_speed` 0, so each running entry takes one frame step per scheduler pass. Within a pass, the entry that started earlier steps first. An entry that starts during a pass also takes its first step in that pass. Pass 0 is registration. The rows follow section 3.10.1. `[Must-match]`

| ID | Setup | Timeline |
| --- | --- | --- |
| G1 | Punch hit. Attack art 11 frames, A = 3. Defender flinches. | Pass 0: turn, attack sound and attack animation start. Passes 1-2: attack frames 0-1. Pass 3: contact sound, flinch sound and flinch animation start, and the attacker shows frame 2 (A − 1). |
| G2 | Melee miss. A = 3, dodge B = 1. | Pass 0: attack sound and attack animation start. Dodge sound delay 2: the dodge sound and dodge animation start in pass 2, while the attacker shows frame 1. |
| G3 | Melee miss. A = 3, dodge B = 5. | Pass 0: dodge sound and dodge animation start. Attack sound delay 2: the attack sound and attack animation start in pass 2, and attack frame 0 shows in the same pass. |
| G4 | Thrown knife hit. Throw art 12 frames, A = 4. L = 200, so N = 6 and the flight has 7 nodes. | Pass 4: unhide, flight sound and flight start. The attacker shows frame 3 and the knife reaches node 0. The attacker's steps and the flight's steps both count N down. Pass 7: the hit sound, the defender's reaction sound and its animation start, as the knife reaches node 3. Pass 10: the knife reaches node 6 and the flight finishes. |
| G5 | As G4, but the throw art has 4 frames. | Pass 5: the attacker's finishing step examines twice. Pass 7: the hit sound starts, at node 3, as in G4. |
| G6 | As G4, but the attack misses and nothing is struck, so N = 0. | Pass 4: the hit sound starts together with the flight. |
| G7 | Laser hit. Point art 4 frames; single-fire art 5 frames, A = 1. L = 64, so the flight has 3 nodes. | Pass 5: point finishes; the attack sound, attack animation, unhide, flight sound and flight start; attack frame 0 and node 0. Pass 7: the flight finishes. Pass 10: the attack animation finishes; the projectile is erased, and the hit sound and the reaction start. |

Path check: L = 0 (adjacent defender) gives 1 node and N = 0. L = 31 gives 1 node; L = 32 gives 2 nodes and N = 1.

### 4.8 Fixture H: Walking Steps

A synthetic walk art has 8 frames. Every SE frame has per-frame displacement (5, 4), and every E frame has (8, 0). The route is [SE, E], out of combat. Each row is one frame step (section 3.11.1). `[Must-match]`

| Step | Frame | Facing | Displacement after adding | Event |
| ---: | ---: | --- | --- | --- |
| 1 | 0 | SE | (5, 4) | Re-placed at displacement 0 first; no crossing |
| 2 | 1 | SE | (10, 8) | — |
| 3 | 2 | SE | (15, 12) | y reaches 12: cross. Residual (-1, 0). Move to the SE neighbor, face E, displacement (-1, 0) |
| 4 | 3 | E | (7, 0) | — |
| 5 | 4 | E | (15, 0) | — |
| 6 | 5 | E | (23, 0) | — |
| 7 | 6 | E | (31, 0) | — |
| 8 | 7 | E | (39, 0) | x reaches 32: cross. Move to the E neighbor. The route is complete, so the residual (7, 0) is dropped and the walker ends at displacement (0, 0). When its sequence ends, it is reset to its standing pose (section 3.10.1) |

With a blocker on the E neighbor at step 8 that is not an auto-usable door, the walker stays on the SE neighbor. It then replans from there, or stops if no route exists.

### 4.9 Fixture I: Spray Draw Order

Shared inputs: the player fires a burst (animation 46) with 3 rounds in the magazine and a burst count of 10, so 3 rounds are scheduled. The center budget is `trunc(3 / 3) = 1`, the left budget 1 and the right budget 1. The direct-trial count `trunc(1 / 2) = 0` is raised to 1, so the center budget drops to 0. Spray accuracy is 70, effective Critical Chance 5, Luck 6, Better Criticals 0, difficulty normal and timestamp at least 864000. The weapon does normal damage, 10-20, and has no explosion effect. Ray contents are explicit test inputs. Random outcomes are listed in draw order (section 3.7, "Draw order for a burst or continuous-fire attack"). `[Must-match]`

| Case | Perk or trait | Ray contents | Outcomes | Result |
| --- | --- | --- | --- | --- |
| I1 | Sniper | No critter on any ray | `40, 50, 20, 90, 4, 30, 15` | Spray roll 40: margin 30, success. Its upgrade threshold is `3 + 5 = 8`, so 50 does not upgrade. Direct trial 20: margin 50, success. Its upgrade threshold is `5 + 0 = 5`, so 90 does not upgrade: 1 main-target hit. No Stage A pass. The center budget is `0 - 1 = -1`, so the center ray draws nothing; the side rays meet no critter. Final success, so the Sniper D10 is drawn: 4 is at most Luck 6, giving critical success. Critical tier draw 30 (tier 1). The selected entry in this fixture has no saving stat and no random cripple. Then one base-damage draw, 15, for the single main-target round. 7 draws |
| I2 | Sniper | The line to E holds only the intended defender. The left ray reaches critter X, not the intended target, with torso hit chance 60; nothing else | `80, 50, 95, 60, 30, 10, 12, 9` | Spray roll 80: margin -10, failure; 50 exceeds `trunc(10 / 10) = 1`. Direct trial 95: margin -25; 60 exceeds 2, so no hit. The Stage A pass inside the spray passes the defender with no draw. Center budget 0: no draws. Left ray at X: 30 is at most 60, a hit, and the budget is now 0; 10 is still drawn and ends the trials. Base-damage draw 12 for X's one round. Right ray: nothing. A failure with an extra hit is promoted to success. Sniper D10 9 exceeds 6, so it stays success. Zero main-target rounds make no damage draw: the intended target gets the hit bit and 0 damage ("Extra hits only"). 8 draws |
| I3 | Jinxed, no Sniper | As I2 but with no critter on any ray; Stage B finds no candidate | `80, 50, 95, 60, 0` | Spray roll and direct trial fail as in I2. Neither Stage A pass draws, and no ray draws. The final result is an ordinary failure, so the Jinxed coin is drawn; 0 keeps the failure (only 1 makes it a critical failure). Miss resolution: the second Stage A pass draws nothing, and Stage B is not a grenade and finds nothing, so there is no damage draw. 5 draws |
| I4 | Jinxed | Any | `100, 2, 40` | Spray roll 100: margin -30; 2 is at most 3, giving critical failure. The spray returns at once: no direct trial, no Stage A, no ray and no Jinxed coin. 40 is the section 3.6 critical-failure table draw; any further draws depend on the selected effect |

A clean-room engine that skips the spray post-processing would make 5 draws in I1, losing the Sniper D10 and the critical tier, and 7 in I2. Placing the Sniper D10 before the rays would also shift every later draw.

### 4.10 Fixture J: Combat Message Lines

A male NPC named "Raider" (alias 0, so the head is `combat.msg` 1110 "the head") is struck in the head by the player. Texts are the English corpus `combat.msg`.

| Attack result | Verbose (`combat_messages` = 1) | Brief (0) |
| --- | --- | --- |
| Critical, 18 damage, effect 5/802/2/-3/1/5004/5003 with the save passed (flags `0x802`) | "Raider was critically hit in the head for 18 hit points. The blow knocks him to the ground." | "Raider was critically hit in the head for 18 hit points, knocked down and ." (safe rule: "…, knocked down and had his armor bypassed.") |
| Critical, 40 damage, effect 6/80/-1/0/0/5007/5000 (flags `0x80`) | Two lines: "Raider was critically hit in the head for 40 hit points, death is instantaneous." then "Raider was killed." | One line: "Raider was critically hit in the head for 40 hit points and was killed." |
| Normal hit, 5 damage, no flags | "Raider was hit in the head for 5 hit points." | Same |
| Normal hit, 1 damage, torso | "Raider was hit for 1 hit point." | Same |
| Player's critical miss, 3 self-damage, flags lose turn (`0x8000`) | "You critically missed and took 3 hit points and ." (safe rule: "… and lost your next turn.") | Same |

## 5. Clean-Room Verification Checklist
- [x] Zero C/C++ code snippets or struct definitions included
- [x] All multi-byte integer endianness explicitly stated
- [x] Synthetic test vectors included
- [x] Every documented quirk tagged `[Must-match]` or `[May-improve]`
- [x] No internal reference symbol names; only public SSL opcode names and file-format key literals
- [x] Spec linter run against `fallout1-re` symbols with no findings

Verification notes (R1-rev8): `tools/spec_lint.py` gives 0 errors, and no source path or raw revision hash appears. Its 18 plain-word warnings were reviewed and are benign: the public art directory/list stems ("items", "critters", "scenery", "tiles", "misc"), the public `fallout.cfg` key "running", and this spec's own formula notation ("index", "stat", "column", "rank", "range", "count"). The only fenced blocks are a synthetic list file and big-endian hex dumps. Fixture G timelines were cross-checked with an independent model of the section 3.10.1 start rule, written from this spec's text rather than reference code. The section 3.11 container reach bits were read from the corpus item prototypes. Fixture H was recomputed by hand from the section 3.11.1 step rule. The R1-rev7 container reach timing and adjacency rules were cross-checked with SPEC-013 §3.16, §3.2 and §3.23, and the busy-click rules with SPEC-014 §3.27. Tag count, excluding the two tag definitions: 213 `[Must-match]`, 39 `[May-improve]`.

Update notes (2026-10-22, R1-rev9): sections 3.3 and 3.4 and new Fixture I. Fixture I was stepped by hand through the section 3.7 draw order; the combat-request AP bonus was checked at all six request sources. Tag count is now 215 `[Must-match]`, 39 `[May-improve]`. `tools/spec_lint.py` gives 0 errors.

Update notes (2026-10-23, R1-rev10): section 3.12 and Fixture J. All message numbers and texts were checked against the English corpus `combat.msg`, which has no entries 0-31. The Fixture J critical effects were taken from the section 2.5 man/head row. Net tags: +5 `[Must-match]`, +1 `[May-improve]`. `tools/spec_lint.py` gives 0 errors.
