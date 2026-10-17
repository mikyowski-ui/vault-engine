# SPEC-007: Critter Art, Hex Routing, and Combat Resolution

## Clean-Room Rules for Research Agents
- Document ONLY binary file layouts (byte offsets, endianness, data types), mathematical formulas, state transitions, and observable behavior.
- DO NOT copy C/C++ code, struct definitions, function names, internal variable names, or implementation-source paths from the inspected implementation.
- Provide small synthetic hex/text test vectors at the bottom of the specification.

## 1. Overview & Scope

This specification describes the reference implementation, targeting the 32-bit Windows engine. Evidence is static inspection of art lookup, movement, ray traversal, combat, item/stat consumers, and numeric critical-effect data. Synthetic checks use independent arithmetic/state models, not original game assets or a live game session.

SPEC-001 and SPEC-003 define sprite bytes, prototype/map files, and screen/hex projection; SPEC-006 defines effective stat/skill and random-check calculations. This document adds the complete art-identifier codec and combat/routing rules. Public host mnemonics such as `obj_can_see_obj` identify observable interfaces, not native routine names.

Important distinctions: a directional-file selector is not a critter's zero-based facing; a six-character critter stem normally produces eight filename characters but six characters are not enforced; the route search marks hexes seen on insertion and does not perform full cost relaxation. The inspected damage/hit pipeline does not consume the stored ammunition AC/DR/multiplier/divisor modifiers. Recovered omissions and defects below are not replaced with assumed rules from another engine version.

## 2. Binary Layout / Data Format (Byte Offset Tables)

### 2.1 Complete 32-Bit Art Identifier

An art identifier, abbreviated FID, occupies four bytes. In prototype/map/save fields serialized through the resource integer readers/writers it is a **big-endian 32-bit word**; it is not four independent little-endian bytes. Bit positions below refer to the numeric word, with bit 0 least significant.

| Bits | Mask | Meaning |
| --- | --- | --- |
| 0-11 | `0x00000FFF` | Zero-based base-art list index |
| 12-15 | `0x0000F000` | Critter weapon-animation code, or head fidget variant; otherwise not used by ordinary filename lookup |
| 16-23 | `0x00FF0000` | Animation code; head animation selector for the head category |
| 24-27 | `0x0F000000` | Art category |
| 28-30 | `0x70000000` | Critter directional-file selector; not the sprite's in-file direction index |
| 31 | `0x80000000` | Not used by valid category/filename decoding; normal construction leaves it clear |

For ordinary supported values, numeric encoding is base index plus `4096 * weapon/variant + 65536 * animation + 16777216 * category + 268435456 * file selector`. Construction masks base to 12 bits, weapon/variant to 4, animation to 8, and selector to 3. It does not validate every semantic field before packing. Filename decoding ignores bit 31; alias reconstruction can discard it.

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

Thus the animation/variant bits do not universally append suffixes to every art category. Normal non-critter construction clears the directional selector. A raw identifier of another category can retain such bits, but ordinary filename lookup does not use them. Invalid categories/list indexes are not comprehensively bounds-checked before all accesses.

Head animation selectors 0-11 append, respectively, `gv, gf, gn, ng, nf, nb, bn, bf, bv, gp, np, bp`, then `.frm`. Selectors 1, 4, 7 additionally append the decimal value of bits 12-15 after their two letters. There is no zero padding and no guarantee of an eight-character head filename. Other head selector values are not safely bounded by the suffix lookup.

### 2.2 Critter List Rows, Weapon Letters, and Animation Suffixes

The logical list is `art/critters/critters.lst`. A successful text-line read consumes one zero-based index; blank/comment-like lines are not skipped. Reads are bounded to 199 bytes including a returned LF, so an overlong physical line can consume multiple indexes. CRLF is converted to LF in text mode; a final unterminated line still counts.

The base token ends at the first space, comma, semicolon, TAB, or LF and is truncated to 12 bytes. Leading delimiters produce an empty stem. There is no general leading-whitespace trim, hash-comment removal, or extension stripping. Case of the stored stem is preserved.

In a separate pass, the first comma introduces one signed decimal alias index, allowing ordinary leading whitespace/sign and stopping at the first nondigit; no digits means zero. Additional comma fields do not form an alias array and are not independently consumed. Without a comma, the default character base index at list initialization is used, initially zero. A conventional row is `six-character stem,alias index`, but neither six characters nor a particular total line width is enforced.

Alias substitution occurs only for animation codes `27,29,30,33,55,57,58,64`. Substitute the selected row's alias, masked to 12 bits, while preserving category 1, animation, weapon code, and directional selector. This is one substitution, not recursive alias chasing, and is not a generic missing-file fallback.

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

Codes 20-35 name fall-back, fall-front, bad landing, big hole, charred body, chunks, autofire dance, electrify, sliced-in-half, burned-to-nothing, electrified-to-nothing, exploded-to-nothing, melted-to-nothing, fire dance, bloody fall-back, bloody fall-front. Codes 48-63 are their final/static counterparts in that same order.

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

The weapon-code validity check precedes every animation branch: weapon codes 11-15 fail even for a death animation whose suffix otherwise ignores the weapon. Animation 0-64 is the supported domain; the arithmetic suffix branches do not provide a comprehensive safe rejection rule for arbitrary larger bytes.

### 2.3 Directional File Selection and Fallback

After aliasing and suffix selection, concatenate the selected base stem and the two letters. A six-character stem gives eight characters before the dot. Selector 0 appends `.frm`; selectors 1-6 append `.fr0` through `.fr5`. Physical orientations 0-5 are northeast, east, southeast, southwest, west, northwest; ordinary directional callers pass **orientation plus 1** as the file selector. A raw selector 7 formats `.fr6`, not a seventh valid sprite-facing direction.

Filename resolution itself simply honors the encoded selector. The normal FID constructor additionally applies resource-existence fallback:

1. For non-critters, animation below 20 or above 35, or animation 33 fire dance, encode selector 0 immediately; do not probe directional files.
2. For critter animation 20-35 except 33, probe the requested selector's filename, using alias substitution when applicable. If present, retain that selector.
3. If missing and the requested selector was 1, encode 0. Otherwise probe selector 1 (`.fr0`); if present retain 1, else encode 0.

The final `.frm` fallback is not guaranteed to exist. Requested selector 0 can also be probed first for eligible death animation; if that `.frm` is missing the constructor can select `.fr0`. It does not scan all six directions, switch weapon letters, substitute standing art, or chase another alias when a resource is missing. The returned FID retains the original base index even when filename lookup resolves through its alias.

### 2.4 Combat Encodings and Numeric Table Domains

Combat state bits are 32-bit values: `0x01` active combat, `0x02` player-input gate, `0x08` accepted end-combat request. Initial/out-of-combat state normally has the input-gate bit set, so it is not independently proof that combat is active.

| Hit mode | Meaning |
| ---: | --- |
| 0 / 1 | Left-hand primary / secondary |
| 2 / 3 | Right-hand primary / secondary |
| 4 / 5 | Punch / kick |
| 6 / 7 | Left-hand reload / right-hand reload |

The inspected item path does not provide a separate advanced-unarmed AP/damage ladder for the additional enumerated mode values 8-19. Ordinary punch/kick costs and weapon selection below must not be replaced with such a ladder.

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

For unarmed/melee use half the listed adjustment, truncating toward zero. Uncalled is normalized to torso after its initial accuracy calculation and before the normal critical-effect lookup; the stored uncalled effect rows nevertheless differ from torso in some NPC tables.

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

All masks in that table are hexadecimal, including digit-only values. Persistent critter status retains only knockout, knockdown, cripple/blind, dead, and lose-turn bits. Maneuver `0x01` means alerted/participating, `0x02` stop-attacking, `0x04` fleeing; these are not the effect-flag meanings.

Critical-success tier selection uses integer `D100 + effective Better Criticals stat`: values at most 20, 45, 70, 90, 100, or greater select tiers 0-5 respectively. The failure selection score is `D100 - 5 * (effective Luck - 5)`: values at most 20, 50, 75, 95, or greater select columns 0-4 of this complete hexadecimal mask table:

| Failure table number | 0 | 1 | 2 | 3 | 4 |
| ---: | --- | --- | --- | --- | --- |
| 0 | 0 | 8000 | 8000 | 80002 | 200000 |
| 1 | 0 | 8000 | 4000 | 100000 | 10000 |
| 2 | 0 | 20000 | 4000 | 100000 | 2000 |
| 3 | 8000 | 28000 | C000 | 100000 | 9000 |
| 4 | 40000 | 4000 | 84000 | 100000 | 1000 |
| 5 | 8000 | 40000 | 2000 | 100000 | 9002 |
| 6 | 0 | 8000 | 100000 | 2000 | 9400 |

The weapon prototype selects failure-table number; no weapon/selector -1 uses table 0. There is no safe universal clamp for invalid table numbers.

### 2.5 Complete Critical-Success Effect Matrices

These are built-in mathematical lookup data, not another on-disk resource format or a serialized native-memory record. Each slash-separated entry contains seven values in order: damage scale numerator, ordinary effect mask, saving-stat number, saving-stat adjustment, extra mask on failed save, ordinary message number, failed-save message number. **Only the second and fifth values are hexadecimal**; all other values are decimal. `=` repeats the entire entry immediately to its left in the same row.

Damage scale is divided by 2 in the damage formula: numerator 3 means 1.5 times before subsequent integer truncation. Saving-stat -1 means no save; otherwise roll D10 against effective stat plus the adjustment, applying the extra mask and alternate message on failure. Stat numbers used here are 0 strength, 2 endurance, 5 agility, 6 luck. Flags/messages are preserved exactly, including anomalous rows; no symmetry or message-number correction is implied.

Victim table numbers 0-14 are man, woman, child, super mutant, ghoul, brahmin, radscorpion, rat, floater, centaur, robot, dog, mantis, deathclaw, plant. NPC selection uses prototype kill category, not inferred gender/body shape. P selects the separate player matrix irrespective of gender. Location numbers are those in section 2.4. All 864 entries, including uncalled location 8, are specified below.

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

Hex numbering is `column + 200 * row`, with coordinates 0-199. Directions 0-5 are northeast, east, southeast, southwest, west, northwest. The one-step numeric changes for even stored columns are `-1,199,200,201,1,-200`; for odd columns, `-201,-1,200,1,-199,-200`. The directional helper refuses to step from any outer-border hex, even inward. These are the stored column's parity rules, not row parity or mirrored screen-column parity.

The route builder takes actor, source, goal, an optional output direction-byte sequence, a strict-goal Boolean, and a blocker query. Output bytes are unsigned one-byte direction numbers, so no multi-byte endianness applies to them. The search's transient working records are not an additional serialized map format.

The normal blocker query first examines objects on the candidate hex, then neighboring hexes in direction order 0-5 for multihex objects. An eligible blocker is on the actor's elevation, is not the actor, lacks basic hidden bit `0x01` and movement-nonblocking bit `0x10`, and has art category critter/scenery/wall. Items and miscellaneous art are not movement blockers. Neighboring blockers additionally require multihex bit `0x800`; this expands that obstacle's occupied footprint by one hex. Light/projectile-through flags alone do not remove a movement blocker.

Search order and costs:

1. If strict-goal is true and the goal blocker query is non-null, return zero immediately, even for an otherwise usable door. With a permissive goal, intermediate blocking is still tested but the goal's blocker is not.
2. Clear the 40000-bit seen map, mark source seen, and put source in frontier slot zero. Its accumulated cost is zero and incoming direction is initialized to **0**, not the actor's facing.
3. Heuristic between projected reference corners is `absolute DX + absolute DY - floor(min(absolute DX, absolute DY) / 2)`, in screen-pixel units. Choose frontier entry with smallest heuristic plus accumulated cost; strict comparisons retain the lowest occupied slot on ties.
4. Remove that slot. If its hex is the goal, backtrack. Otherwise append it to the expanded list, then examine neighbors in direction order 0-5.
5. Skip seen neighbors. For a non-goal neighbor, query its blocker; skip it unless the actor is allowed to auto-use that particular door. Put an accepted neighbor into the **first vacant frontier slot**, mark it seen immediately, record its parent hex/direction, and calculate cost.
6. Each hex adds 50 cost units. Outside combat, add another 10 when the incoming direction changes, including a first step other than direction 0. During combat there is no turn surcharge. There is no AP, cripple, ammunition, terrain, or door-opening cost in this search metric.
7. Continue until goal selected, frontier empty, or a capacity failure. Backtrack by finding expanded entries with the recorded parent hex, collect incoming directions, then reverse the collected bytes.

Seen is set on insertion, not expansion. There is no cheaper-route update, decrease-key, reopening, or alternate parent for an already seen hex. This is a heuristic best-first route search with fixed edge costs, not a guarantee of fully relaxed optimal A*. Reusing low-numbered frontier holes also means ties are not FIFO or simply lexicographic direction order.

Frontier and expanded storage each have 2000 slots, but reaching a count of 2000 returns failure; at most 1999 live frontier entries or expanded non-goal entries are accepted. Backtracking stops after 800 direction bytes. If a longer route was found, that truncation can return the **last** 800 steps reversed into forward order without reaching the original source, rather than rejecting or returning the first 800 steps. Zero also represents an already-at-goal route, not just failure. Invalid hexes/buffer sizes are not universally validated before indexing.

Auto-door permission requires a non-player actor with critter art, biped or robotic body, an available scenery-door prototype, and an unlocked door. Quadrupeds, the player, locked doors, and non-doors do not receive this exception. A usable intermediate door is traversable to the planner, but a strict blocked goal is rejected before door permission is considered.

Movement animation applies frame displacements until the next hex boundary, then queries the next hex again. A newly blocking non-door triggers route rebuilding from the actor's current hex to its saved destination with strict-goal enabled. Successful rebuilding resets frame, facing, displacement, and route cursor; failure ends movement. A usable door instead receives an open/use request and the move proceeds; the door action's return value is not checked before the subsequent move attempt.

Move-to-object temporarily hides the destination object while planning, then restores its prior hidden state. It shortens the selected path by one step for a normal mover or two for a multihex mover; only the moving actor's multihex flag controls that subtraction. Core search does **not** dilate every obstacle for a multihex mover's full body. The saved replan destination is computed by stepping forward from the original target along the removed final direction, and for a multihex mover also along the preceding removed direction, rather than simply storing the actor's intended stopping hex. This can produce unintuitive replans.

Move-to-tile similarly removes the last step if the destination is occupied, and updates that saved replan destination. Ordinary move request limits truncate the scheduled route length; the core search's 50/60 costs are not the remaining AP budget. Actual AP is charged when a hex transition commits.

### 3.2 Perception, LOS, Shootability, and Ray Traversal

Three distinct tests must not be collapsed:

| Test | Eligibility / meaning |
| --- | --- |
| Movement blocker | Visible, blocking critter/scenery/wall on the hex or multihex neighbor footprint |
| Dedicated sight-blocker query | Only scenery/wall on the exact hex; ignore hidden and light-through bit `0x20000000`; no neighboring multihex expansion |
| Combat/host straight-ray query | Uses the **movement** blocker, then applies projectile-through filtering only for sampling stride 32 |

These are hex/object tests, not per-sprite-pixel alpha/outline tests. A first eligible object can mask another object sharing the same hex. In particular, skipping a projectile-through object does not make the callback search a second stacked object on that same hex.

Perception first measures hex distance reduced by one for each multihex participant, with a minimum of zero. The observer's forward sector is its orientation and either adjacent orientation, including wraparound. In that sector, maximum perception distance is five times effective Perception, halved for a target with glass-translucency bit `0x20000`, then quartered for a player target whose sneaking is working. Outside that sector, the maximum is Perception outside combat or twice Perception in combat, quartered for working player sneak. The rear-sector branch does not apply the glass-halving step. All divisions discard fractions. No obstacle ray is part of this perception predicate.

The host `obj_can_see_obj` interface first requires non-null objects and a placed target, then that perception test. It casts with stride 16 and reports success only if the first reported movement blocker is the target itself. A nonblocking/hidden target can therefore fail this test despite being within perception. The inspected wrapper also leaves the input/output obstacle slot uninitialized, although the ray reads its input as an ignored object; this is an unsafe recovered path, not a universally deterministic Boolean guarantee. A controlled independent ray test must explicitly supply the initial ignored reference.

The straight-ray traversal is pixel Bresenham between projected **hex centers**, corner plus `(16,8)`, on the actor's elevation:

1. With an obstacle-output slot, first test the source hex. Ignore its supplied initial ignored object; otherwise an eligible source blocker stops immediately with zero samples. At stride 32, also ignore a returned blocker carrying basic projectile-through bit `0x80000000`.
2. Choose Y as major axis when absolute X difference is at most absolute Y difference; otherwise X is major. Initialize error to `2 * absolute minor difference - absolute major difference`. The signs of X/Y differences choose pixel step -1, 0, or 1.
3. At each current pixel, map it to a hex using the exact pixel-boundary rules in SPEC-003. Increment the sample counter, counting the initial pixel as 1. At every stride pixels, record a sample and reset that counter.
4. If current major coordinate equals the target major coordinate, clear the obstacle output and finish **before** the normal changed-hex blocker test.
5. If error is nonnegative, step the minor coordinate and subtract twice the major difference. Step the major coordinate and add twice the minor difference.
6. If the hex computed before that pixel step differs from the previous tested hex, query its blocker. A nonignored blocker stops traversal, except projectile-through filtering at stride 32. Otherwise remember that hex and continue.
7. Append a final partial-stride sample if necessary. A sample contains hex, elevation, and pixel displacement from that hex's center.

Sampling returns at most 200 samples; trying to append sample 201 returns zero. The limit applies even when no output sample array was requested. For an unobstructed ray of major-axis pixel span L, sample count is `ceil((L + 1) / stride)`. A missing obstacle-output slot disables blocker checks entirely, not just obstacle reporting. Ray return zero is consequently not a universal "clear line" answer.

Shooting/cover repeatedly casts at stride 32, initially ignoring the shooter. A returned non-critter is a hard obstruction. A returned critter is not a hard obstruction: count it as one cover body unless it is the requested defender, restart at that critter's **center**, and ignore that previous critter on the next cast. Thus later segments can differ from the original single straight line. Each intervening critter subtracts 10 accuracy points; cover is not a percentage or fractional wall exposure. Alive/dead status is not separately checked here; normal corpse nonblocking flags often remove corpses from consideration.

Weapon attack preflight rejects, in order: dead defender; both crippled arms when using a weapon; one crippled arm with a two-handed weapon; insufficient ordinary AP; distance beyond effective range; empty ammunition when capacity is positive; hard ray obstruction for ranged/throw. A high accuracy score alone does not prove shootability: hit-chance calculation counts cover but ignores the hard-obstruction Boolean. Long/corrupt rays that hit the sample cap can leave obstacle state unchanged; callers that only inspect that state do not thereby acquire a safe termination guarantee.

### 3.3 Combat Initialization, Participation, Turns, and Ending

Normal initialization sets inactive state with input-gate bit `0x02`, no participants, no pending attack, and player AP equal to effective maximum AP. Starting a fresh combat stops animations/fidgeting, fixes the combat elevation, collects visible critters there, assigns combat identifiers, clears accumulated damage/opponents/AP, and retains only maneuver alert bit `0x01`. All collected critters initially form the waiting partition; active count is zero. Active bit `0x01` is set and combat UI/AI presentation is initialized.

For distinct initiating participants, initial active order is attacker, defender, then player if neither is the player. The first pass uses that order **before** sequence sorting. Attacker/defender become each other's last opponents. There is no explicit deduplication for an invalid same-object initiating pair. A resumed combat instead first completes the player's resumed turn without refilling AP, then continues after that player's existing list position.

The outer entry guard is permissive: either participant being absent/on the current elevation is sufficient, rather than requiring both actors to match. Ordinary vectors below use distinct actors on one elevation. "Active critter" excludes knockout, dead and lose-turn status; "prone" includes knockout/knockdown or art animation 20-35/48-63. The separate dead query also recognizes nonpositive HP. These predicates are not interchangeable with every direct status-bit test.

At the round sweep, scan waiters in list order. Joining requires not hidden, player's elevation, not dead/knocked out. Recent damage admits immediately; otherwise invoke its combat script with parameter 5. Alert admits; stop/flee rejects; otherwise an AI danger target must exist. An admitted waiter swaps into the active partition. Admitted NPCs receive an immediate turn before subsequent sorting; the player does not receive that extra admission turn. Swapping and incrementing the scan can defer a swapped-in waiter to a later pass.

Dead active participants leave the active/waiting partitions. Non-player knockout, or maneuver value **exactly** 2, moves an active participant to waiting and clears alert. This equality is not a general test of stop bit 2 combined with other bits. Sort remaining active actors by descending effective Sequence, then descending Luck. Equal keys compare equal: no stable tie order or player-first tie rule is promised. Add five game seconds after the sweep.

For each turn:

- Set current actor and reset the pending attack descriptor. Knockout, dead, or lose-turn status skips the action branch and clears lose-turn.
- Otherwise, except for resumed turns, refill ordinary AP from effective maximum AP plus any initial scripted combat-request AP bonus.
- Player receives a separate free-movement pool of twice Bonus Move rank; NPCs do not. Invoke the actor's combat script with parameter 4 and clear invocation source/target.
- A script override suppresses automatic stand-up and ordinary player-input/AI action processing, but queued animations still finish. Otherwise a prone actor stands for 3 AP, then player input or NPC AI runs.
- Player input stops on end-turn/space, input gate clear, accepted end-combat request, knockout/dead/lose-turn, ordinary AP at most zero, quit/load state, or scripted interruption. Ordinary AP alone gates this loop, not AP plus free movement.
- Drain queued animation completions, reset damage received this turn, clear free movement at turn end. Player death or player elevation changing from the fixed combat elevation ends the loop.

An explicit end-combat request is refused if, on the same elevation, an active relevant opponent does not want to stop, or a relevant waiter wants to join. Relevant means a non-player on a different team, or one whose last opponent is on its own team. Stop preference is true for stop/flee maneuver, knockout/death, no AI danger target, danger outside perception, or AP still equal to maximum AP. Successful explicit ending sets bit `0x08`; player input consumes/clears that bit and exits combat.

After a complete round, automatic ending occurs if active count is at most one, player is absent, or every active actor is on player's team and no active actor has a last opponent on that team. Otherwise another round begins. A special scripted ending actor can also break the loop.

Cleanup clears actor damage/opponents/maneuvers/AP/outlines, restores player's maximum AP and normal UI, clears active combat and restores input-gate state, frees the combat list/AI context, and dispatches map-update scripts. Accumulated kill experience is awarded only for a living player when no quit request suppresses it. Non-scripted knockout recovery can wake a prone surviving player at cleanup. Load interruption uses its separate restoration path instead of assuming normal cleanup.

### 3.4 AP Costs, Range, and Ammunition Availability

Movement commits cost 1 AP per hex with sound legs, 4 with one crippled leg, 8 with both crippled legs. Spend the free-movement pool first, then ordinary AP, saturating ordinary AP at zero. Moving/running changes animation speed, not this per-hex cost. Bonus Move does not pay for attacks, inventory, or reload. An already queued movement can continue using free movement after ordinary AP reaches zero, although ordinary player input no longer stays open for that reason alone.

| Action | Cost / rule |
| --- | --- |
| Bare punch or kick | Base 3 AP, same cost and same basic damage interval |
| Weapon primary | Prototype primary AP value |
| Weapon secondary | Prototype secondary AP value |
| Aimed attack | Add 1 after weapon/player reductions |
| Reload through normal combat/interface path | 2 AP, returned before aiming/perk/trait reductions |
| Nonweapon generic use | 2 AP in the generic item-cost query |
| Stand up | 3 AP, or reduce remaining AP to zero when below 3 |
| Open player inventory in combat | `4 - Quick Pockets rank`, once on entry; wrapper does not clamp a negative malformed cost |
| Use inventory item on a target in combat | 2 AP on successful use, in addition to applicable entry cost |

For a player weapon with effective range greater than 2, Fast Shot subtracts 1 AP. Presence of Bonus HtH Attacks subtracts 1 for melee/unarmed subtype, and presence of Bonus Rate of Fire subtracts 1 for ranged subtype. These are presence tests, not one reduction per rank. Then add aiming cost and clamp attack cost to at least 1. Bare unarmed attacks do not enter the weapon Fast Shot branch. Attack debit occurs after attack resolution and successful animation sequencing; it saturates at zero rather than requiring every direct caller to have passed preflight.

Normal inventory drag-to-reload does not independently debit the sidebar's 2-AP reload cost; the inventory entry cost is already charged. Sidebar reload charges only after successful reload. Ammunition must be a compatible caliber; a nonempty weapon also requires the same ammunition PID, rather than silently mixing types.

AI successful reload also debits 2 with saturation at zero; unlike the sidebar gate, that debit need not imply at least 2 AP existed beforehand. For attack debit, non-torso/non-uncalled locations imply aiming; torso/uncalled use the player's current UI aiming flag, or false for an NPC.

Bare punch/kick range is 1. Weapon primary/secondary uses its corresponding prototype range. Throwing range is capped to three times Strength, or for player three times `Strength + 2 * Heave Ho rank`. Object distance subtracts one for each multihex participant and clamps at zero.

Normal aimed-shot availability rejects player Fast Shot, burst/continuous animation, and explosion/fire/EMP damage. The recovered additional plasma-throw condition compares incompatible values and never supplies a real plasma-throw restriction. Availability and AP calculation are separate: forcing an aiming flag into the cost helper does not repair availability.

Unspent ordinary AP is added to effective AC when that defender is not the current combat actor, before the stat's own clamp. Free-movement points do not supply that AC bonus. Derived maximum AP normally starts at `5 + floor(Agility / 2)` and Sequence at `2 * Perception`, with effective trait/perk/bonus adjustments as in SPEC-006.

### 3.5 Exact Hit-Chance Calculation

Art weapon class, hit-mode number, and prototype attack-mode nibble are separate quantities. Primary attack mode uses the low extended-flag nibble; secondary uses the next nibble. Values 1/2 select punch/kick and Unarmed skill, 3/4 swing/thrust and Melee skill, 5 throw and Throwing skill, 6/7/8 single/burst/continuous ranged fire. Ranged uses Energy Weapons for laser/plasma/electrical damage; otherwise Big Guns if the weapon's big-gun flag is set, Small Guns otherwise. Mode 0 has no valid skill; 9-15 are not safely indexed by the inspected mode tables.

Start with effective Unarmed skill for no weapon, otherwise the effective selected weapon skill. Let d be object distance including multihex reductions and P be effective Perception. Effective Perception already includes the blinded-stat reduction/clamp.

For ranged/throw attacks in the range-aware calculation:

- Set range factor to 4 for a Long Range weapon property, 2 otherwise.
- Set R to the greater of `d - range factor * P` and `-2 * P`.
- For a player attacker, subtract twice Sharpshooter rank from R **after** that clamp.
- Add `-12 * R` when R is nonnegative and attacker is blind; otherwise add `-4 * R`. Negative R therefore gives a close-range bonus.
- Count intervening critters with the shot query and subtract 10 per body.

Then apply, in order:

1. For a player using any weapon with One Hander: -40 for two-handed, +20 otherwise.
2. For any weapon user: subtract 20 per point by which weapon minimum Strength exceeds effective Strength. Add 20 for Accurate weapon property.
3. Subtract defender effective AC, including its unused-AP contribution when applicable.
4. Add the location adjustment: full table value for ranged/throw, half for melee/unarmed.
5. Add 15 for a multihex defender, for all attack types.
6. For a player attacker, apply target-light penalty from the table below.
7. Add any initial scripted combat-request accuracy adjustment.
8. Subtract another 25 if attacker is blind. Add 40 if defender is knocked out or knocked down.
9. If attacker team differs from player team, apply combat difficulty -20 easy, 0 normal, +20 hard.
10. Cap only the upper result at 95. There is no lower cap at zero or a guaranteed 5-percent floor.

No-range calculation omits the R term, but still includes cover, Strength, AC, location, lighting and other adjustments. Unarmed bypasses weapon-specific handling; thrown attacks still use the ranged-style location/range/cover adjustments.

| Visible target light, full intensity 65536 | Player accuracy penalty |
| --- | ---: |
| At most 26214 | -40 |
| 26215-39321 | -25 |
| 39322-52428 | -10 |
| Above 52428 | 0 |

Visible target light is the greater of ambient light and true tile light, capped at 65536 on the tile-light path. When the target is the player, subtract that player's emitted light from the tile-light value before the comparison. NPC attackers do not receive this darkness penalty.

The stored ammunition armor-class modifier is not read by this hit-chance pipeline. Likewise, no fractional cover percentage, target sprite transparency, or automatic unseen-target penalty is added beyond the actual tests above.

### 3.6 Attack Rolls, Critical Effects, and Damage Arithmetic

For a single shot or ordinary melee/unarmed attack, roll D100 and set margin to hit chance minus that roll. Negative margin is failure, otherwise success. Only when absolute game timestamp divided by 864000 is at least 1, a second D100 can upgrade: failed margin becomes critical failure when the second roll is at most `trunc(-margin / 10)`; success becomes critical success when it is at most `trunc(margin / 10) + effective Critical Chance - full location adjustment`. Use the full location adjustment here even for melee/unarmed, not its halved accuracy penalty.

These random result values are 0 critical failure, 1 failure, 2 success, 3 critical success. The first-day gate does not prohibit later forced upgrades: global player Jinxed changes any ordinary failure to critical failure on a 50-percent coin flip; player Slayer promotes successful melee/unarmed to critical success; player Sniper promotes a ranged success when D10 is at most Luck. Silent Death sets normal-hit scale numerator to 4 for a successful sneaking player melee/unarmed strike from behind against a defender not already targeting the player. A critical lookup replaces that numerator rather than multiplying it again.

Critical success sets the attack's critical marker, chooses the success tier in section 2.4, and uses the player or NPC/location entry in section 2.5. Apply ordinary flags/message. If a saving stat is present, a D10 greater than effective victim stat plus its signed adjustment applies the extra flags and alternate message. There is no second D100 critical-upgrade mechanism on this D10 save. Random-cripple flags are removed and replaced by one uniform choice of left leg, right leg, left arm, right arm, without rerolling already crippled limbs.

Critical failure clears hit, chooses the weapon's failure table/tier, and does nothing further when the selected mask is zero. Otherwise set critical plus those attacker flags:

- Hit-self computes ordinary self damage, using scheduled ammunition count for ranged or one round otherwise; explosion computes one ordinary self-damage round.
- Additional self-injury adds D5. Lose-turn sets ordinary AP to zero; it does not independently clear the free-movement pool.
- Lose-ammunition schedules the entire current magazine for consumption for ranged subtype; on other subtypes that flag is removed.
- Random cripple uses the same four-limb substitution. Random-target selects another AI target, switches to torso, clears the critical marker, marks hit, and calculates normal damage if a target exists.
- Weapon drop/destruction/explosion effects are also presented through animation callbacks. A mask is not proof that every intended HP/equipment effect later commits; see the application guards below.

Protected PID `0x01000008` bypasses victim critical augmentation and the ordinary death/status/HP application paths. The same PID as attacker is exempt from critical-failure effects. This is a literal PID exception, not a general invulnerability test.

All following divisions truncate toward zero. For each round that hits, choose base random damage:

| Attack form | Inclusive base damage interval |
| --- | --- |
| Bare unarmed | 1 through effective Melee Damage plus 2 |
| Weapon melee/unarmed subtype | Prototype minimum through prototype maximum plus effective Melee Damage |
| Other weapon subtype | Prototype minimum through prototype maximum |

Melee bonus expands the upper endpoint; it is **not** added to both endpoints after the random draw. For a player ranged subtype add `2 * Bonus Ranged Damage rank` to that round's chosen value. Let M be normal numerator 2, Silent Death numerator 4, or selected critical-table numerator. Let difficulty percentage be 75/100/125 for easy/normal/hard only when attacker team differs from player team, otherwise 100.

The intervals assume ordered endpoints. Reversed prototype damage bounds use the RNG's recovered reversed-bound fallback to the first supplied bound, not an assumed symmetric swapped interval; this matters after the weapon writer's bound reversal described in SPEC-003.

Determine victim DT/DR by weapon damage type, ordered normal, laser, fire, plasma, electrical, EMP, explosion:

- Without armor bypass, use effective threshold/resistance; Penetrate weapon property sets DT to zero.
- For a player attacker, Finesse adds 30 to DR on this armor-processing path.
- Armor bypass makes both DT and DR zero **except for EMP**, which still uses armor processing and any Finesse/Penetrate changes.

For each hitting round, take base damage plus its ranged bonus, multiply by M, divide by 2, multiply by difficulty percentage, divide by 100, then subtract DT. If positive, subtract `trunc(remaining * DR / 100)`; add it to total only if still positive. DT/DR are applied separately to each round, not once to the summed burst. Excessive DR cannot turn this accumulator into healing.

**Ammunition multiplier, divisor, and DR modifier are not used here.** Their prototype values remain stored data, but this inspected combat path supplies no corresponding multiplier/divisor/DR term. Combined with the absent ammunition AC term, using a conventional ammunition-adjusted formula would change observable results. Loading/caliber/round-count behavior remains separate and does use ammunition identity/quantity.

Before sequencing, a scripted initial combat request can add/clamp defender damage and replace flags; its two supplied override words are written to the same defender-flags field, with the last winning. This is not a valid separate attacker/defender override layout. Those damage clamps do not rerun the already-computed knockback or death-flag calculations. A punch request has a one-in-four cosmetic switch to kick when kick art exists.

Attack resolution initially calculates damage/flags/ammunition-to-spend without immediately debiting the magazine or HP. Successful action sequencing debits attack AP. When the tracked combat-animation count reaches zero, cleanup reselects the current weapon by hit mode, consumes its scheduled ammunition, displays results, and applies target damage/status. Changing the equipped weapon in between can therefore affect which weapon is debited.

For ordinary victims, positive damage at least current HP sets dead. Persistent status applies mask `0x80FF`; armor bypass, hit, critical and weapon-effect bits are not all stored as permanent critter status. Dead removes queued events; knockout queues wake-up after `10 * (35 - 3 * Endurance)` ticks. HP application records damage received, invokes damage/destroy scripts, can credit player/team kills unless overridden, detaches dead scripts and removes dead party members.

The inspected attacker-self-damage commit tests whether the attacker is **already marked dead**, rather than the normal live-attacker condition. Calculating or displaying critical-failure self damage therefore does not guarantee that a living attacker's HP receives it. A table dead flag can also mark a victim dead independently of HP arithmetic. Do not repair these state guards by assuming that all calculated damage is immediately authoritative HP.

### 3.7 Burst, Continuous Fire, Miss Retargeting, and Splash

For burst/continuous animation, scheduled ammunition is the smaller of current magazine and prototype burst count. Make one hit-chance roll with effective Critical Chance as its critical modifier, without the single-shot location subtraction; critical failure returns early. A critical-success spray adds 20 to its local direct-fire accuracy without reapplying the ordinary 95 cap.

For a burst, set center budget to integer ammunition divided by 3, at least 1; left budget to ammunition divided by 3; right budget to the remainder. Direct main-target trials are center divided by 2, at least 1. When that direct count was raised to 1, reduce center budget by 1. Roll direct trials against the spray accuracy. Then cast center with budget center minus actual direct hits, left with its budget, and right with its budget. Continuous fire instead initializes each of these budgets to 1.

Direct trials use the full D100 result/upgrade check with critical modifier zero and count either success result as a hit; along-ray trials below use a plain D100 comparison. The additional RNG draws of those two mechanisms therefore differ.

The center ray extends from attacker through target to effective range. For side rays, use target hex as spread center unless target distance is at most 3, in which case use the hex three line transitions from attacker. Take the direction from spread center back to attacker; side points are its adjacent directions plus 1 and plus 5 modulo 6. Extend attacker-to-side-point lines to weapon range. Line extension uses pixel Bresenham and counts changed hexes until the requested count or an outer-border hex, not repeated angular-neighbor steps.

Each ray stops at hard non-critter obstruction. At a critter, recompute ordinary torso accuracy for that critter, not the direct-fire accuracy boosted by the spray critical. Consume consecutive successful D100 trials until the first miss or ray budget is exhausted; a miss does not decrement that budget before the ray continues to another critter. The trial comparison is evaluated before the remaining-budget test, so exhausted-budget loop checks can still consume an extra random draw.

Ray hits on the main target add to its round count. Other critters become up to six extra targets, each computed at normal damage scale numerator 2. Repeated burst hits on an extra target accumulate damage/flags/knockback. Continuous fire resets remaining budget to 1 at each encountered critter, ignores later duplicate-extra accumulation, and returns zero main-target hits from each ray; its main-target damage comes from the direct trial rather than those ray-return counts.

A failed initial spray with any main/extra hits is promoted to success; a nonfailed spray with no hits becomes failure. Main-target critical augmentation occurs after extra-target damage was already calculated, so a main critical multiplier must not be applied to every extra target.

The accidental-target helper used on a ranged miss has an inverted final projectile-through test: an ordinarily selected non-through blocker fails its final acceptance condition. The later miss-impact fallback can still hit something at the extended impact path or grenade-scatter position, so this does not establish that all stray hits are impossible. Grenades scatter by a random direction and nominal D(half target distance), with the generator's reversed-bound quirk and a minimum displacement of 1 for zero result.

The weapon-attack splash helper has its radius/initial-hex setup commented out in the inspected revision. Its first hex remains -1 and it exits without adding splash victims. This omission does not describe the separate scripted-explosion action path. Do not supply an invented radius/falloff formula for this incomplete weapon-splash path.

### 3.8 Knockback, Death Animation, and Corpse Presentation

Computed knockback is zero for a multihex defender. Otherwise explosion, bare unarmed, or melee subtype permits `floor(total damage / 10)`, or `/ 5` for a Knockback weapon property. A non-null weapon classified as unarmed does not enter the bare-unarmed exception unless another eligible condition applies. Other normal ranged/throw damage does not automatically produce displacement. The prototype behavioral no-knockback bit is not consulted by this computation.

Direction is attacker-to-defender hex orientation. The displacement executor checks movement blockers at increasing distance. With no blocker it uses the computed maximum. A blocker at distance j first decrements distance before breaking, then decrements it again after the loop; actual displacement is therefore at most `j - 2`, clamped to no displacement, not always the expected `j - 1`. Fall-front without art falls back to fall-back. A fall can animate without movement when the resulting distance is nonpositive.

Ordinary lethal animation uses violence levels 0 none, 1 minimal, 2 normal, 3 maximum. "From front" is true when the absolute difference between stored attacker/defender orientations is 2, 3 or 4, not 0, 1 or 5.

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

1. Victim PIDs `0x01000017,0x01000032,0x01000031` request animation 31 with minimum violence 2.
2. Bloody Mess applies only for a player attacker. At violence 3, Bloody Mess with punch/kick/thrust/swing/throw requests 23; single normal-damage fire with Bloody Mess or damage at least 45 also requests 23.
3. Otherwise at violence above 2, Bloody Mess or damage at least 45 selects the maximum column, falling back to the moderate column if the maximum choice cannot pass its violence/art test.
4. Otherwise at violence above 1, Bloody Mess or damage at least 15 selects the moderate column. Otherwise choose 20.
5. A rear hit changes a remaining plain 20 choice to 21 fall-front, then performs final art/violence fallback.

Minimum-violence/art validation returns the desired animation if allowed and found. Otherwise a front hit falls back to 20. The recovered rear fallback is inverted: when fall-front art exists it returns 20, and when it does not exist it returns 21. Preserve this distinction instead of assuming every returned fallback resource exists.

Before the ordinary victim-based choice tree, special attacker art `0x0500000A` requests explosive 31, attacker PID `0x020001EB` requests electrical 30, and attacker art `0x020001F5` uses the supplied attack-animation selector, each under maximum-violence validation. Ordinary fall selection checks one/two hexes in the falling direction, can reverse fall-front/back around obstacles, then replaces unavailable fall-front with fall-back.

Blood presentation replaces fall-back/front with 34/35 when violence is not zero and the corresponding art exists. Fire-dance death can animate a random unobstructed 2-5-hex run, then burned-to-nothing 29. A living on-fire result can use 33 and return to standing. These presentation changes do not turn every transient fire/critical flag into permanent critter status.

Corpse presentation sets movement-nonblocking and flattens except for victim PIDs `0x01000032,0x01000031,0x01000008`; it removes the outline. Final animation 30 or 31 drops inventory except for victim PIDs `0x01000031,0x01000032,0x01000017`. Public direct/static-death requests have a separate correction: static codes 51-63 revert to forced fall-back 20 in these callers if maximum violence/art is unavailable. They are not the same decision tree as attack-driven damage selection.

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

For a controlled ray test on the original source/goal, put a blockable defender on 20298 and one ordinary blockable critter on 20299. Explicitly initialize the ignored-object reference to null, avoiding the host wrapper's uninitialized-slot defect:

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
- A separate melee result of 35 damage gives computed knockback 3. Clear space permits 3; a wall at displacement 2 produces actual displacement 0 because of the double decrement; a wall at displacement 3 permits 1.
- Before timestamp 864000, without a forced-upgrade trait/perk, the normal D100 critical-upgrade draw is not made. Slayer/Sniper/Jinxed postprocessing must not be disabled merely by calling it the first day.

## 5. Clean-Room Verification Checklist
- [x] Zero C/C++ code snippets or struct definitions included
- [x] All multi-byte integer endianness explicitly stated
- [x] Synthetic test vectors included
