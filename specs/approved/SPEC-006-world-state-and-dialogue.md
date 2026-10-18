# SPEC-006: World Variables, Character Statistics, Map Events, and Dialogue

## Clean-Room Rules for Research Agents
- Document ONLY binary file layouts (byte offsets, endianness, data types), mathematical formulas, state transitions, and observable behavior.
- DO NOT copy C/C++ code, struct definitions, function names, or internal variable names from the reference implementation.
- Provide small synthetic hex/text test vectors at the bottom of the specification.

## 1. Overview & Scope

This specification covers campaign/map `.GAM` initialization, player `.GCD` sheets, SPECIAL-derived statistics and skills, random checks, map entry and action dispatch, and the reply/choice/talking-head dialogue state machine. The evidence is static inspection of reference revision `R1`, targeting 32-bit Windows, supplemented by the adjacent local asset corpus. It is not an assertion that every recovered edge case was verified in the original executable.

Only public file tokens, public script vocabulary, mathematical notation, and observable state are named. The template's repository-name reference is replaced with generic wording to respect this document's stricter identifier restriction. Public names such as `map_enter_p_proc`, `start_gdialog`, and `giq_option` describe the requested script interface, not implementation routines.

The corpus has six premade `.GCD` files, each 428 bytes, and 36 `.GAM` files. The observed campaign initializer allocates 618 variables. Those counts describe these assets, not format limits. Map/prototype geometry, compiled-script instruction encodings, bitmap frames, and localized message syntax have separate specifications; only their interfaces needed for world state and dialogue are described here.

**Compatibility tags.** Every documented quirk carries one tag:

- `[Must-match]`: gameplay-visible behavior, data grammar, formula, ordering, or random-draw sequence. A clean-room engine must reproduce it exactly.
- `[May-improve]`: reference memory-safety defects, undefined behavior, uninitialized reads, stale state, presentation-only transients, or a lookup that reads the wrong table in an obviously unintended way. A clean-room engine may apply the stated safe rule. It should keep every other observable behavior. An engine that wants bit-exact parity may offer a compatibility switch that keeps the reference behavior.

Plain layout tables and formula tables carry one tag for the whole table, on the sentence that introduces or follows them.

## 2. Binary Layout / Data Format (Byte Offset Tables)

### 2.1 `.GAM`: Ordered Text Initializers

Campaign initialization reads `data/vault13.gam`, seeking `GAME_GLOBAL_VARS:`. Fresh-map initialization attempts `maps/<mapname>.gam`, seeking `MAP_GLOBAL_VARS:`. These are raw byte-oriented text files with ASCII syntax and no binary header, version, checksum, declared encoding, or binary integer byte order. Numeric initializers are signed decimal text, not little-endian or big-endian integers. `[Must-match]`

The usual record spelling is `descriptive_name = decimal_value;`. Names are documentary: the loader stores **only an ordered array of signed 32-bit values**, not a name-to-value dictionary. `[Must-match]` The byte rules below are data grammar. `[Must-match]`

| Token/position | Byte rule |
| --- | --- |
| Section selection | Compare the first 16 bytes of each input chunk with the requested marker, case-sensitively, without trimming |
| Campaign match | The compared bytes are `GAME_GLOBAL_VARS`; its following colon is not part of the 16-byte comparison |
| Map match | All 16 bytes of `MAP_GLOBAL_VARS:`, including the colon, participate |
| Empty chunk-line | Skip only when its first byte is LF, `0A` |
| Full-line comment | Skip only when its first two bytes are `2F 2F`, two slashes at the start |
| Semicolon | First `3B` terminates the text considered for this record; it does not by itself make the record skippable |
| Assignment | First `3D` before that semicolon begins the initializer |
| No assignment | Allocate a variable with value zero |
| Assignment value | Skip leading numeric-conversion whitespace, accept an optional sign and decimal digits; trailing text is not required to be empty |

The matched marker chunk is consumed and does not allocate a variable. Thereafter reading continues to end of file: there is **no section-end recognition**. A later heading, bare word, or semicolon-only comment therefore allocates a zero if it has no assignment. `[Must-match]` A duplicate documentary name creates another variable rather than replacing an earlier one. `[Must-match]`

Indices are zero-based and assigned at allocation time: first accepted chunk is index 0, next is 1, and so on. Blank LF lines and column-zero slash comments consume no index. Leading spaces before a slash comment prevent comment recognition. Space/tab-only lines, a leading carriage return without LF, and a line reduced to empty text by a semicolon still consume an index. `[Must-match]`

Reads hold at most **257 bytes per chunk**, followed by an internal terminator. A long physical line can become multiple separately processed chunks and allocate multiple indices; a leftover LF-only chunk is skipped. `[Must-match]` Section matching also acts on these chunks. Ordinary CRLF input is translated to LF before these tests. A byte-order mark or indentation before a marker prevents a normal marker match. No Unicode decoding or name escape processing occurs. `[Must-match]`

An initializer such as `12tail` converts to 12; `0x10` converts its decimal prefix to zero, not hexadecimal 16. `[Must-match]` A present assignment with no convertible number leaves the newly allocated element uninitialized: its value is not a specified zero. Decimal overflow is likewise not a reliable wraparound contract. `[May-improve]` Safe rule: store zero when no number converts, and clamp an overflowing decimal to the signed 32-bit range. Fixtures avoid those unsafe cases.

If a file opens successfully, any prior array is cleared before marker search. A missing marker then yields an empty array and a successful result. A file-open failure instead reports failure and leaves the prior array intact. `[Must-match]` A map's companion-file failure is not propagated by the successful map-load path. `[Must-match]` Attempting this initializer also refreshes player inventory-derived bookkeeping before opening the file. `[Must-match]`

Campaign and map-global reads outside the valid index range yield zero; out-of-range writes fail. The numeric index, not its documentary spelling, is the script-facing contract. `[Must-match]`

### 2.2 `.GCD`: Complete Player Sheet Layout

A complete player `.GCD` sheet is **428 bytes, `0x01AC`**, with no magic or version. All numeric fields below are **signed 32-bit two's-complement, big-endian**, except that the first word is used as a bitfield. The name is a fixed 32-byte byte string. There are no native pointers, alignment gaps, variable-length records, or trailing checksums in this layout. `[Must-match]`

| File offset | Bytes | Meaning |
| --- | ---: | --- |
| `0x0000` | 4 | Critter/player flags, 32-bit big-endian bitfield |
| `0x0004` | 140 | 35 base-stat words, in the index order below |
| `0x0090` | 140 | 35 additive bonus-stat words in the same order |
| `0x011C` | 72 | 18 invested-skill-point words, in the skill order in Section 3.3 |
| `0x0164` | 4 | Body category |
| `0x0168` | 4 | Critter experience reward; not the player's accumulated experience |
| `0x016C` | 4 | Kill-count category |
| `0x0170` | 32 | Player name, normally NUL-terminated and padded; raw single-byte encoding |
| `0x0190` | 16 | Four tagged-skill indices; -1 means unused |
| `0x01A0` | 8 | Two trait indices; -1 means unused |
| `0x01A8` | 4 | Unspent character-creation attribute points |

For stat index J, its base word is at `4 + 4 x J`, and its bonus word is at `144 + 4 x J`. These are serialized values, not guaranteed-valid or already-recomputed effective statistics. `[Must-match]`

| Stat index | Meaning |
| --- | --- |
| 0, 1, 2, 3, 4, 5, 6 | Strength, Perception, Endurance, Charisma, Intelligence, Agility, Luck |
| 7, 8, 9 | Maximum HP, maximum AP, armor class |
| 10, 11, 12 | Unarmed-damage field, melee damage, carry weight |
| 13, 14, 15, 16 | Sequence, healing rate, critical chance, better-critical modifier |
| 17-23 | Damage thresholds: normal, laser, fire, plasma, electrical, EMP, explosion |
| 24-30 | Damage resistances in the same seven-type order |
| 31, 32, 33, 34 | Radiation resistance, poison resistance, age, gender |

Current HP, current poison, current radiation, level, accumulated player experience, inventory, equipped items, selected portrait, and perk ranks are **not** serialized by this sheet. The first 368 bytes are the reusable critter-stat portion; the remaining 60 bytes add player name, tags, traits, and remaining creation points. Player flag masks include `0x00000001` for sneaking, `0x00000008` for pending level advancement, and `0x00000010` for addiction. `[Must-match]`

Trait indices are fixed, in order zero through fifteen. `[Must-match]`

| Index | Trait | Index | Trait |
| ---: | --- | ---: | --- |
| 0 | Fast Metabolism | 8 | Bloody Mess |
| 1 | Bruiser | 9 | Jinxed |
| 2 | Small Frame | 10 | Good Natured |
| 3 | One Hander | 11 | Chem Reliant |
| 4 | Finesse | 12 | Chem Resistant |
| 5 | Kamikaze | 13 | Night Person |
| 6 | Heavy Handed | 14 | Skilled |
| 7 | Fast Shot | 15 | Gifted |

### 2.3 Sheet Loading and Normalization

Full player initialization first resets player statistics, invested skills, tags, traits, and perks, then reads the sheet. The player loader forces base EMP resistance to 100 and forces body category, critter experience reward, and kill-count category to zero, regardless of their file values. Generic critter-stat loading does not perform those player-only normalizations. `[Must-match]`

After the full player sheet is read, gender-dependent art and inventory bookkeeping are refreshed, movement/blocking presentation flags are normalized, derived statistics are recomputed, and a healing request of 10000 HP is applied subject to the current maximum. The sheet has no current-HP field. Loading can therefore replace cached derived values; merely decoding the file's arrays is not equivalent to fully initializing a player. `[Must-match]`

Numeric reads check for truncation, but the fixed-name read is not separately checked or forced to terminate. There is no comprehensive range validation for stored values, trait/tag indices, or trailing bytes. A safe independent loader should require the full 428-byte layout, a terminator within the name, and valid index/range constraints before exposing the data. `[May-improve]` Safe rule: reject a sheet that fails any of these checks with a diagnostic, before changing any player state. Extra bytes are not another defined sheet section.

## 3. Behavioral Rules & Edge Cases

### 3.1 SPECIAL and Derived-Stat Pipeline

Integer division truncates toward zero throughout this specification unless a formula explicitly uses floor or ceiling. For nonnegative attributes, division by two or three is therefore ordinary floor division. `[Must-match]`

For each primary attribute X, distinguish its raw stored base, its player-trait adjustment, and its additive stored bonus. Let R(X) be **raw base plus trait adjustment**, without the bonus or clamping. Let E(X) be the effective attribute after adding its stored bonus and situational adjustments, then clamping to 1 through 10. Non-player critters do not receive the player's selected-trait adjustments. `[Must-match]`

Primary stat queries add base, trait adjustment, and bonus; blindness additionally subtracts 5 from Perception before clamping. `[Must-match]` Derived-stat recomputation stores the following base values. `[Must-match]`

| Derived base | Recalculation formula |
| --- | --- |
| Maximum HP | `15 + R(Strength) + 2 x R(Endurance)` |
| Maximum AP | `5 + floor(E(Agility) / 2)` |
| Armor class | `E(Agility)` |
| Melee damage | `max(E(Strength) - 5, 1)` |
| Carry weight | `25 + 25 x E(Strength)` |
| Sequence | `2 x E(Perception)` |
| Healing rate | `max(floor(E(Endurance) / 3), 1)` |
| Critical chance | `E(Luck)` |
| Better-critical modifier | 0 |
| Radiation resistance | `2 x E(Endurance)` |
| Poison resistance | `5 x E(Endurance)` |

**HP intentionally uses R, not E.** Temporary/permanent Strength or Endurance bonuses in the bonus array do not enter the initial HP formula; trait changes do. An unclamped trait-adjusted base above 10 can affect HP even though the effective SPECIAL query is capped at 10. Other derived bases above use the effective, clamped attributes. `[Must-match]`

An effective derived-stat query then adds that stat's own trait adjustment and additive bonus. It does not add the primary-attribute bonus a second time. `[Must-match]` In combat, a critter not currently taking its turn also adds its remaining combat AP to armor class. This is current unspent AP, not necessarily maximum AP. `[Must-match]`

Final bounds are 0 through signed-32-bit maximum for HP/AP/AC/unarmed damage/melee damage/carry/sequence/healing; 0-100 for critical chance, better-critical modifier, thresholds, EMP resistance, radiation resistance, and poison resistance; 0-90 for other damage resistances; 16-35 for age; and 0-1 for gender. Age additionally gains the whole number of elapsed 365-day game years. These query bounds do not imply that the file loader validated its raw arrays. `[Must-match]`

Only the listed derived bases are rewritten by recomputation. The unarmed-damage field, armor thresholds/resistances, age, and gender otherwise retain stored data. Derived bases are cached: changing primary bases or primary bonuses invokes recalculation, but an ordinary derived query is not a universal recalculation of every dependency. `[Must-match]`

### 3.2 Trait, Perk, and Advancement Adjustments

The following trait adjustments apply only to the player, and duplicate selection of the same trait does not apply it twice. `[Must-match]`

| Trait | Adjustment to covered statistics |
| --- | --- |
| Gifted | +1 to each raw primary attribute; -10 to every skill percentage |
| Bruiser | +2 raw Strength; -2 maximum AP after the Agility formula |
| Small Frame | +1 raw Agility; subtract `10 x raw stored Strength` from carry weight, excluding Gifted/Bruiser and bonus-array Strength from this subtraction |
| Kamikaze | Subtract the stored base AC, leaving armor/perk/other bonuses and situational AP intact; +5 sequence |
| Heavy Handed | +4 melee damage; -30 better-critical modifier before final query clamping |
| Fast Metabolism | +2 healing rate; recovered resistance behavior below |
| Finesse | +10 critical chance |
| Night Person | Perception and Intelligence adjustment determined by the clock rule below |
| Skilled | +10 to every skill percentage; perk-selection interval becomes four levels instead of three |
| Good Natured | -10 to the six combat skills; +15 First Aid, Doctor, Speech, and Barter |

For Night Person, express the current clock as `100 x hour + minute`. The recovered test subtracts 600 and compares the result with 1200. Consequently the adjustment is **-1 from 00:00 through 17:59 and +1 from 18:00 through 23:59**, including -1 before 06:00. This is not a symmetric conventional day/night rule. `[Must-match]`

For Fast Metabolism, the recovered resistance adjustment **adds** each stored base resistance rather than canceling it. At recalculation Endurance 5, base radiation resistance 10 becomes 20 and base poison resistance 25 becomes 50 before other bonuses/clamping. This double-negative behavior is recorded as a reference quirk, not replaced by a trait-description interpretation. `[Must-match]` The remaining traits have no additional direct adjustment in the base-stat/skill calculations covered here; attack costs, critical-damage tables, and drug rules are separate concerns. `[Must-match]`

Perk effects on stored stat bonuses are applied when a rank/effect is gained, and removed inversely. They must **not** be added again on every query if already present in the bonus array. `[Must-match]`

| Perk | Additive effect per acquired rank |
| --- | --- |
| Bonus HtH Damage | +2 melee damage |
| Earlier Sequence | +2 sequence |
| Faster Healing | +1 healing rate |
| More Criticals | +5 critical chance |
| Strong Back | +50 carry weight |
| Action Boy | +1 maximum AP |
| Dodger | +5 AC |
| Better Criticals | +20 better-critical modifier |
| Rad Resistance | +15 radiation resistance |
| Toughness | +10 normal damage resistance |
| Snakeater | +25 poison resistance |

Equipment/addiction effects use the same bonus mechanism: powered armor adds +3 Strength and +30 radiation resistance; the combat-armor effect adds +20 radiation resistance. Buffout addiction subtracts 2 Strength, 2 Endurance, and 3 Agility; Mentats addiction subtracts 3 Intelligence and 2 Agility; Psycho addiction subtracts 2 Intelligence; RadAway addiction subtracts 20 radiation resistance. Primary-bonus changes recalculate the non-HP derived bases as described above. `[Must-match]`

Each actual level advancement adds a stored maximum-HP bonus of `2 + trunc(R(Endurance) / 2) + 4 x Lifegiver ranks at that advancement`. This is accumulated history, not a retroactive formula using only today's rank or level. The healing request is issued before increasing that maximum-HP bonus, so a previously full character need not gain the same amount of current HP immediately. `[Must-match]` Maximum player level is 21. `[Must-match]`

The editor's level-reconciliation step grants `5 + 2 x R(Intelligence) + 2 x Educated ranks`, minus 5 for Gifted, per newly reconciled level, retaining unspent skill points up to 99 and flooring the Gifted-adjusted total at zero. The recovered allocator does not add a separate extra five for Skilled. `[Must-match]` Perk selection normally occurs at level multiples of three, or four with Skilled, with at most seven selected perks in this path. Gaining a rank and meeting its selection prerequisites are distinct operations. `[Must-match]`

### 3.3 Skill Percentages

All attribute terms in the table use E. The `.GCD` skill word is **invested points**, not a final percentage. `[Must-match]`

| Index | Skill | Untrained base percentage |
| ---: | --- | --- |
| 0 | Small Guns | `35 + Agility` |
| 1 | Big Guns | `10 + Agility` |
| 2 | Energy Weapons | `10 + Agility` |
| 3 | Unarmed | `65 + floor((Agility + Strength) / 2)` |
| 4 | Melee Weapons | `55 + floor((Agility + Strength) / 2)` |
| 5 | Throwing | `40 + Agility` |
| 6 | First Aid | `30 + floor((Perception + Intelligence) / 2)` |
| 7 | Doctor | `15 + floor((Perception + Intelligence) / 2)` |
| 8 | Sneak | `25 + Agility` |
| 9 | Lockpick | `20 + floor((Perception + Agility) / 2)` |
| 10 | Steal | `20 + Agility` |
| 11 | Traps | `20 + floor((Perception + Agility) / 2)` |
| 12 | Science | `25 + 2 x Intelligence` |
| 13 | Repair | `20 + Intelligence` |
| 14 | Speech | `25 + 2 x Charisma` |
| 15 | Barter | `20 + 2 x Charisma` |
| 16 | Gambling | `20 + 3 x Luck` |
| 17 | Outdoorsman | `5 + floor((Endurance + Intelligence) / 2)` |

For nonnegative invested points P, an untagged skill adds P. A tagged player skill instead adds **20 + 2 x P** to the untrained base. Any match among the four tag entries activates this once; repeated copies do not multiply it. Then add player trait, skill-perk, and difficulty adjustments, and cap at **200**, without a general lower clamp. `[Must-match]` A negative stored invested-point value is returned directly rather than treated as a normal investment. `[Must-match]`

| Skill-perk condition | Percentage adjustment |
| --- | --- |
| Medic present | +20 First Aid and Doctor |
| Ghost present, player's visible light at most 45875 | +20 Sneak |
| Master Thief present | +10 Sneak, Lockpick, Steal, and Traps |
| Mr. Fixit present | +20 Science and Repair |
| Speaker present | +20 Speech and Barter |

These skill bonuses test presence, not rank multiplication. Sneak includes the Master Thief adjustment as well as the independent Ghost test. `[Must-match]` Easy difficulty adds 20 and hard difficulty subtracts 10 to the twelve noncombat skills, indices 6-17; normal adds zero. These player-only adjustments are not applied to NPC skill queries. `[Must-match]` Other perks do not automatically acquire a displayed-skill adjustment merely because their description concerns a skill. `[Must-match]`

Spending one unspent skill point increments its invested-point word by one: +1 percentage point untagged or +2 tagged, before the cap. There is no tiered high-percentage purchase price in this path. `[Must-match]`

### 3.4 Random Generator and Check Results

The generator is shared by gameplay and visual behavior. It uses one recurrence state, a 32-entry shuffle array, and a shuffle selector. These are runtime state, not serialized `.GAM` or `.GCD` fields. `[Must-match]`

For a draw, compute `Q = 16807 x (N modulo 127773) - 2836 x trunc(N / 127773)`. If Q is negative, add 2147483647; repeat that correction if still negative. Select shuffle entry `selector modulo 32`, save its old value U as the output, replace that entry with Q, set both the next recurrence state to Q and selector to U. For a valid inclusive range A through B, return `A + (U modulo (B - A + 1))`. Modulo bias is retained; there is no rejection sampling. `[Must-match]`

The recovered reversed-range wrapper ultimately returns its **first** bound after its range guard, rather than providing a reliable normalized reversed range; it still consumes one generator draw. Equal bounds still consume a generator draw. `[Must-match]` Arithmetic overflow or a zero/invalid modulus is outside the defined valid-input contract. `[May-improve]` Safe rule: when the range width does not fit in 1 through 2147483647, consume one draw and return the first bound.

Explicit seeding clamps a supplied seed below 1 to 1, except that the special seed -1 first requests an ambient host-generated seed. Iterate the same recurrence 40 times with loop indices 40 down to 1; during seeding a negative result is corrected by clearing its sign bit, equivalent to adding 2147483648, **not** the draw-time correction above. Store results in shuffle entries 31 down to 1 when the loop index is below 32. Entry zero is not initialized by this loop and retains its previous value; then initialize the selector from entry zero. Thus reseeding alone is history-dependent. `[Must-match]` Initial startup also consumes 100000 draws for a randomness diagnostic; an externally supplied seed is not a full-state snapshot. `[Must-match]`

The ambient seed combines two host-library random draws as `first x 65536 + second`, retaining the low 31 bits. On the reference Windows target, initial host-library seeding uses the millisecond clock. This ambient path is not a portable deterministic seed source, and the host entropy source itself is not part of the contract. `[Must-match]`

For a skill check, let C be effective skill percentage plus the caller's modifier. Draw D in the inclusive range 1-100 using the generator and compute margin `C - D`. Nonnegative margin succeeds; negative margin fails. There is no general 5%/95% skill-check cap. `[Must-match]` Invalid skill indices immediately return ordinary failure. `[Must-match]`

Before game tick 864000, return only ordinary success or failure, consuming no critical-upgrade draw. At or after that tick:

- On failure, draw another 1-100; upgrade to critical failure when that draw is at most `floor((-margin) / 10)`. `[Must-match]`
- On success, draw another 1-100; upgrade to critical success when it is at most `floor(margin / 10) + effective critical chance`. `[Must-match]`

Thresholds are not independently capped at 100. Result codes are 0 critical failure, 1 failure, 2 success, and 3 critical success. A stat check instead draws 1-10 and compares with `effective stat + modifier`, returning only code 2 or 1 and the corresponding margin; it never performs the percentage critical-upgrade step. `[Must-match]`

For a skill contest, first check the attacker. Only a successful attacker causes a defender check. If the defender also succeeds, subtract the defender's margin from the attacker's; otherwise retain the attacker's margin. Reclassify that margin with critical modifier zero, which can consume another critical-upgrade draw. This is not a single shared opposed die roll. `[Must-match]`

### 3.5 Map Load and Entry Request Order

Requesting a `.MAP` first probes its saved `.SAV` counterpart and prefers a successful saved-map load. Otherwise the original map is loaded. The saved-state bit `0x01` distinguishes restoration from fresh initialization. The following is the host-side order for a successful load; scripts can perform additional actions during their requests. `[Must-match]`

1. Save/leave the old map, stop animations, suspend normal map background processing, prepare party members, disable scrolling, rendering refresh, and script execution, and clear the current map-script association. `[Must-match]`
2. Read and validate the map header version 19. Remove old objects. Clamp negative global/local variable counts to zero, allocate the arrays, and read the map's binary global and local values. `[Must-match]`
3. Read tile data, script instances, and objects. Object loading rebuilds local lighting contributions. For a fresh map, normalize critter combat data. `[Must-match]`
4. Set the entrance elevation and viewport center. Initially there is no current map script, so this elevation assignment does not take the ordinary elevation-change update path. `[Must-match]`
5. Set ambient light to **65536**, then place the player at the centered tile, set the entrance facing, and identify the current map. This full ambient precedes the map's final desired scripted lighting. For a fresh map only, attempt the companion `.GAM` based on the header's map name and replace the map-global array with its initialization values. Successful saved-state restoration does not reinitialize those values from `.GAM`. `[Must-match]`
6. Enable scripts. If the map declares a map script, create its hidden temporary owner and request `map_enter_p_proc`, with fixed parameter 1 for fresh entry or 0 for saved entry, while spatial checking is disabled. `[Must-match]`
7. Recover party members, show the interface, finalize player placement/standing art, install player light radius 4 and intensity 65536, and resume map background processing. Player light is established **after** the initial map-script entry request. `[Must-match]`
8. Request startup for all script instances in stored script-type/list order. Then broadcast `map_enter_p_proc` to present named entries on other scripts, excluding the map script already requested in step 6, again supplying the fresh/saved fixed parameter and suppressing spatial checks. `[Must-match]`
9. Broadcast `map_update_p_proc`: request the map script first, then present named entries on other scripts in stored order, with spatial checks disabled around the broadcast. `[Must-match]`
10. Enable tile refresh, apply any pending entry-facing override or redraw, enqueue midnight/periodic map timers, remove the loading callback, re-enable scrolling, and restore the normal cursor. `[Must-match]`

The generic dispatcher resets the action's override flag on each request, sets its action category, and uses the owner as target only if no target was supplied. It clears the source reference after completion. `[Must-match]` On a script's first request, it loads and runs the compiled program's initial stream rather than atomically jumping into the requested named procedure. `[Must-match]` Later requests use cached procedure indices. A missing cached entry can fall back to procedure index 1; map broadcasts explicitly skip missing entries on non-map scripts. `[Must-match]` Host request order must not be confused with a guarantee that every requested name immediately executes a distinct procedure body.

Later elevation changes request map updates when a current map script exists. Dialogue exit and several completed object-use paths also request updates. Map update is therefore not solely a once-per-minute timer. `[Must-match]`

On fresh-map companion-file open failure, already-read binary globals remain. An opened `.GAM` with no matching marker replaces them with an empty array. `[Must-match]` A failed map read is not a transaction restoring the old scene: the error path resets to a new map, then still enters shared post-load cleanup/startup handling. `[May-improve]` Safe rule: report the failed read with a diagnostic and leave the engine on the reset empty map; restoring the old scene is also acceptable.

Before leaving an existing map, map-exit queue cleanup precedes the map script's `map_exit_p_proc` request. The map is then marked as saved and its last-visit time recorded. Poison and radiation categories are not selected for ordinary exit clearing; other categories use their configured clear handlers, which may retain an event unless the handler returns removal approval. Script and periodic map-timer entries have no retaining clear handler and are removed. `[Must-match]`

### 3.6 Lighting and Spatial Triggers

Ambient light is an integer in 16384 through 65536. Setting ambient to a requested intensity V stores:

`clamp(V + 6553 x Night Vision ranks, 16384, 65536)`

Every setter call applies that perk addition. `[Must-match]` Rebuilding local lighting first initializes every tile on all three elevations to 655, then adds enabled, visible object-light contributions with blocking/occlusion rules. A valid light has radius at most 8 and intensity at most 65536. For an unobstructed ring at hex distance D, its contribution is `intensity - D x trunc((intensity - 655) / (radius + 1))`, through its radius; the source tile receives the full intensity. `[Must-match]`

Visible light for an object is the greater of ambient and accumulated light at its tile, capped at 65536. For the player, subtract the player's own source intensity from the tile accumulation before this comparison. This is the quantity used by the Ghost skill-perk threshold. `[Must-match]`

The public percentage-light operation uses recovered, discontinuous conversion rather than one linear percentage:

| Percentage P | Requested intensity before perk/clamp |
| --- | --- |
| Below 50 | `16384 + trunc(P x 24576 / 100)` |
| Exactly 50 | 40960 |
| Above 50 | `40960 + trunc(P x 24576 / 100)` |

Thus 49 gives 28426, 50 gives 40960, and 51 gives 53493. A map-entry script may replace the initial full ambient using this operation. Its 50 breakpoint must not be silently smoothed into a conventional interpolation. `[Must-match]`

Normal animated hex movement records the old tile, commits the movement step, then checks spatials only if the tile changed. The straight-line animation path and a bare object-tile assignment do not themselves take this same trigger path. `[Must-match]` The check ignores the two cursor objects, hidden or flat objects, tile indices below 10, and calls made while spatial checks are disabled. `[Must-match]`

The check temporarily disables spatial reentry and iterates enabled spatial scripts on the mover's elevation, excluding script instances with disabled bit `0x02`. It requests `spatial_p_proc` if the mover is exactly on the spatial center, or if the radius is nonzero and the hex distance is at most that radius. Source is the moving object; the target is left for the dispatcher's owner fallback. At the end checking is re-enabled. This is **occupancy on each movement step**, not an outside-to-inside edge detector; moving between two tiles in one region can trigger it twice, and overlapping regions can all fire. `[Must-match]`

For valid tiles on the 200-by-200 hex grid, let `X = tile modulo 200`, `Y = floor(tile / 200)`, and define axial coordinates `Q = X`, `R = Y - floor((X + 1) / 2)`. For coordinate differences U and V, hex distance is `max(abs(U), abs(V), abs(U + V))`. A negative sentinel tile -1 produces distance 9999 in the reference distance query. Spatial matches are elevation-specific; this is not screen-pixel or Euclidean distance. `[Must-match]`

### 3.7 World Clock and Timed Queue

Game time is measured in **ten ticks per second**: 600 per minute, 36000 per hour, and 864000 per day. Setting absolute game time zero changes it to one. Queue insertion records `due time = current ticks + delay`; script-timer delays use ticks, not milliseconds or the core interpreter's millisecond timers. `[Must-match]`

Events are ordered by due time, with new equal-time events placed after existing ones. Processing snapshots the current game time once and removes due events with `due time <= snapshot`. Each event is removed before calling its handler; its payload and queue node are released afterwards. A nonzero handler result stops this processing pass, leaving later events pending. Newly scheduled events due by the same snapshot may run in the same pass. `[Must-match]`

Script timer type 3 carries the script instance identifier and a fixed parameter. At firing, if the instance still exists, install that fixed parameter and request `timed_event_p_proc`. The queue event is one-shot unless a script enqueues another. The queue's event type numbers are not VM opcodes or procedure-table indices. `[Must-match]`

At the end of map load, enqueue a midnight event and, for a named map, a map-update event delayed 600 ticks. Enqueueing does not first deduplicate existing timers. `[Must-match]` The map-update handler requests `map_update_p_proc`, removes other queued map-update events, and reschedules one 600 ticks after the current processing time. This is sixty game seconds, not sixty rendered frames. `[Must-match]`

The midnight delay, in ticks, is `10 x (60 x (60 - minute - 1) + 3600 x (24 - hour - 1) + 60)`. It uses only hour/minute, ignoring the current seconds/tick remainder. `[Must-match]` Midnight processing includes daily lock unjamming, campaign/radiation maintenance, and replacement of the midnight/map-update scheduling chain. `[Must-match]`

Outside active dialogue, the background clock checker normally processes the queue outside combat. At most one tick is added when at least 100 wall-clock milliseconds have elapsed and combat is inactive; it does not add all missed ticks after a long frame. The 100-ms branch also enables queue processing during combat, without adding that real-time tick. Combat rounds separately advance five game seconds. An additional map-update request occurs after 30000 wall-clock milliseconds unless the host mode has numeric value 4. That mode also suppresses the initial outside-combat queue permission, but the later 100-ms branch can still enable processing. `[Must-match]`

Active dialogue suppresses this normal critter/timed background checker; talking-head animation still uses wall-clock timing. Explicit time changes or queue-processing calls elsewhere are not forbidden by that suppression. `[Must-match]`

### 3.8 Object Actions and Override Boundaries

The ordinary sequence is input request, optional movement/animation and proximity/AP checks, script dispatch at the interaction stage, then a default action unless the relevant override/result suppresses it. An override is not the same as an integer script return result. Each dispatch clears the override flag, but does not universally clear the separate integer result. `[Must-match]`

| Requested action | Public procedure/event | Source, target, and default behavior |
| --- | --- | --- |
| `look_at` | `look_at_p_proc`, event 21 | Source is actor, target is examined object; script runs first, then ordinary object-name/dead-critter short text unless overridden |
| `description` | `description_p_proc`, event 3 | Source is actor, target is examined object; script can suppress the base description/fallback text |
| `use` | `use_p_proc`, event 6 | For scenery/doors/containers, source is actor and target is used object; override suppresses that branch's default operation |
| `use_skill_on` | `use_skill_on_p_proc`, event 8 | Source is actor, target is skill target, action detail is zero-based skill index; default skill use follows unless overridden |
| `use_obj_on` | `use_obj_on_p_proc`, event 7 | May consult the used item's script and then the target's script, with different target references; routing below |
| `pickup` | `pickup_p_proc`, event 4 | Source is actor, target is item; default adds it to inventory and disconnects it from the map only after inventory insertion succeeds |
| `talk` | `talk_p_proc`, event 11 | Deferred dialogue request enters the target script; the talk entry path does not install the same actor/target tuple as ordinary item actions |

Look and description reject a dead actor or tile-art target. `[Must-match]` Description override suppresses only the base description: player-specific condition, weapon/ammunition, and Awareness HP/equipment information can still be appended afterwards. An ordinary condition description categorizes a live target as full health, or `floor(3 x current HP / maximum HP) + 1`; dead/nonpositive HP uses category zero. `[Must-match]`

Pickup approaches to object distance at most 1 and performs its callback at the interaction animation's action frame when available. It normally adds one item; money uses the stored amount, with a minimum of one, then clears that amount after successful addition. Failed weight/capacity insertion leaves the map item connected. A container that is not pickup-enabled instead takes the container-open/loot route; that is not an item pickup callback. `[Must-match]`

Scenery use and pickup's shared combat interaction check charge **3 AP**, in addition to movement, and fail when fewer remain. Container use checks its lock and can fail before calling its use script; door use can play locked feedback before its use script and then conditionally attempt its own open/close operation. Normal non-door scenery use and direct inventory-item use request map updates after their operation. Direct inventory-item use has item-type defaults and is not a universal dispatch of `use_p_proc` for every item. `[Must-match]`

Skill use normally approaches within object distance 1. First Aid/Doctor require a critter and are blocked in combat; Lockpick requires an item/scenery and is blocked in combat; Steal is blocked in combat and cannot target oneself; Traps is blocked in combat and rejects critters. Science/Repair are blocked in combat and restrict organic critter targets, with the Science-on-brahmin exception. Sneak toggles the player's sneak state immediately rather than calling a target script. The final skill-on stage rejects a jammed lock before the script callback. `[Must-match]`

For `use_obj_on`, distinguish actor A, used item I, and target T:

1. A Doctor's Bag or First Aid Kit takes its special skill route rather than the general two-script route. It is blocked in combat and passes kit bonus 20 into medical use. Medical use supplies `effective critical chance + kit bonus` as the skill check's chance modifier; the skill check still separately uses effective critical chance for critical upgrading. A one-in-ten follow-up draw can consume the kit when medical use returns ordinary completion, even if no healing roll succeeded. `[Must-match]`
2. If I has no script, consult T's script if present, with source A and **target I**. Without its override, perform the item-type default on T; otherwise use that script's integer result. `[Must-match]`
3. If I has a script, invoke it first with source A and **target T**. A nonzero integer result stops routing. A zero result permits T's script with source A and target I; without that target script's override, perform the default. `[Must-match]`
4. When an item script existed, the retained script result after an overridden target call is still the item's result, not automatically the target's result. This differs from the no-item-script path. `[Must-match]`
5. The item-on-object wrapper destroys I when the operation returns 1, converts that return to ordinary success, and requests a map update. `[Must-match]`

Talk is restricted to a player actor and critter target in the ordinary action path. It may approach/run first; the post-movement check requires object distance below 9 and an unobstructed shot line, then schedules dialogue rather than immediately displaying choices. Dialogue entry refuses combat or a target without a script. Its additional reachability/path test and tile-distance limit of 12 apply to ordinary non-item, non-spatial targets. It suspends map background activity, establishes the session target, and requests the talk event. Generic source clearing/owner fallback applies; do not assume this request supplies the player as script source. `[Must-match]`

### 3.9 Dialogue Construction and Choice State Machine

Dialogue has orthogonal state: an open/closed session, reply-and-option processing, and head idle/speech/transition animation. The head updater can continue while the choice loop waits for input. The state transitions in the table below are reference behavior. `[Must-match]`

| Public operation/event | State transition and effects |
| --- | --- |
| `start_gdialog` | Refuse combat or a null object; choose head/background/mood, establish the session target from the issuing script's owner, create the shell, and enable the head updater |
| `gsay_start` | Reset option count and review cursor; it does not create a reply or automatically erase the previous reply |
| `gsay_reply` | Install a message-backed or literal reply, associate its script program, reset the reply paging offset, record review history, and **discard all previously built options** |
| `gsay_option` | Append an ordinary option: message/literal text, integer callback procedure index, and reaction category |
| `giq_option` | Apply the Intelligence filter in Section 3.10, then append the same option only if it passes |
| `gsay_end` | Enter the synchronous reply/choice processing loop; resolve text and speech, realize option buttons, and wait for a choice |
| Choice selected | Record it in review history, stop speech, perform its head reaction, clear the current option count, and invoke its nonzero callback procedure |
| Callback builds another reply/options | Refresh the content and continue the choice loop; a new reply again clears older options |
| Callback leaves no options, or callback index is zero | End this reply/choice processing loop; this is not by itself the explicit destruction of the whole dialogue session |
| Explicit session-end request | Stop speech/updater, free review content and locked head art, destroy dialogue windows, restore font/palette cycling/audio/scrolling/cursor state, and clear session state |

`start_gdialog` consumes five arguments in public source order: message-list number, object reference, initial mood, head index, background index. All use integer-tagged values, including the object reference. The message-list argument is consumed but not used to select the reply. `[Must-match]` The supplied object is validated, but the final session target is the object owning the issuing script; ordinary use therefore requires that script owner and intended dialogue target agree. `[Must-match]` With a head, the existing session target's NPC reaction overrides the supplied initial mood. `[Must-match]`

`gsay_reply` consumes message-list number followed by message number **or literal string**. Option construction consumes message-list number, message number or literal string, numeric callback procedure index, and reaction. `giq_option` adds its threshold as the first argument. These are public argument descriptions, not native source declarations. `[Must-match]`

Numeric callback indices are the operational interface. String callback names are not a working alternative: the adapters reject them before dormant name-handling paths could help, and the lower named-option path records callback zero. Zero callbacks end the current node without executing another procedure. `[Must-match]`

Message-list numbers are one-based references into the script/message-list catalog. Positive reply lists are resolved when the node is displayed, not merely when `gsay_reply` is called. Replies request speech playback from the message's sound field only when a talking head and active dialogue are present; option messages do not request speech. Literal replies use their stored characters directly and do not supply a message sound. `[Must-match]`

If `gsay_end` has a pending reply but no options, it inserts a neutral continuation option with callback zero. A negative reply-list sentinel of -1 instead returns without processing. Special option pairs -2/-2 select the localized Done label; -1/-1 selects the continuation convention, whose first option receives a localized label below effective Intelligence 4 but otherwise remains blank. Literal text uses a content sentinel -4, not a localized message lookup. Displayed options normally receive leading bytes `95 20`, the single-byte bullet and space. `[Must-match]`

There are 30 logical option records and 80 review replies. Message-backed reply text is bounded to 899 content bytes in its 900-byte display buffer. `[Must-match]` Raw literal-reply copying is not bounded in the reference; a safe limit is 899 bytes. `[May-improve]` Safe rule: truncate a literal reply to 899 bytes. Raw option text must leave room for the two-byte prefix and terminator, giving 897 content bytes; literal option text is not bounded in the reference either. `[May-improve]` Safe rule: truncate literal option text to 897 bytes. A thirty-first option can write its callback beyond the option array before the later capacity check; do not treat overflow as a guaranteed clean rejection. `[May-improve]` Safe rule: reject the thirty-first and later options without writing any option state.

The reply rectangle is 379 by 58 at screen position (135,225); the option rectangle is 393 by 117 at (127,335). The number of logical options is not a guarantee that all fit visibly. `[Must-match]` Reply paging advances on Space or a ten-second page timeout, with up/down navigation over remembered pages. `[Must-match]` Ten page-offset slots exist without a comprehensive overflow guard. `[May-improve]` Safe rule: keep a remembered-page list large enough for every page of the reply, or stop advancing after the tenth page; never write past it. Ordinary digit selection maps digits 1 through 9 to options zero through eight. `[Must-match]` The recovered handling of digit 0 produces index -1, an out-of-range option read, and substitutes a dummy no-callback choice rather than a valid tenth entry. `[May-improve]` Safe rule: treat digit 0 as a choice with no callback and no reaction that ends the current reply/choice loop, without reading option storage at index -1.

Processing windows are created/destroyed only at the outermost reply loop. Nested processing depth two or greater suppresses further option-callback execution in the choice path. Barter and Ask About are deferred presentation submodes; a pending switch to barter can refuse an immediate session-end request. Leaving the overarching talk operation restores map background processing and requests map updates, including early talk-return paths. `[Must-match]`

### 3.10 Intelligence Filtering and NPC Reaction

Let I be `effective player Intelligence + Smooth Talker ranks`. The effective attribute has already been clamped to 1-10, but the added perk ranks are **not clamped back to 10**. `[Must-match]`

| Threshold T supplied to `giq_option` | Include the option exactly when |
| --- | --- |
| T at least zero | `I >= T` |
| T below zero | `I <= -T` |

Positive thresholds are minimum-Intelligence choices; negative thresholds are maximum-Intelligence/low-INT choices. Equality is included on both sides. Smooth Talker can unlock a positive-threshold option and simultaneously remove a negative-threshold one. It does not change the underlying SPECIAL stat or all Intelligence-derived skills. There is no Speech roll or random test in this filter. `[Must-match]`

Filtering occurs immediately when each option is appended, in script order. Rejected options do not consume option indices or remain as disabled placeholders. A later attribute change does not retroactively re-filter an already built list. `[Must-match]`

Initial NPC reaction is cached in that NPC's persistent script-local data. With the standard layout, local index zero is the numeric reaction, one is its category, two is the initialized flag, and three controls reputation sign. `[Must-match]` On first initialization:

- Start at `25 + 5 x effective player Charisma + 10 x Presence ranks`. `[Must-match]`
- Add player reputation, or subtract it when NPC local index three equals 1. Cult of Personality instead adds the absolute reputation, independent of that sign control. `[Must-match]`
- Subtract 30 if the campaign child-killer reputation counter exceeds 2. `[Must-match]`
- Add 20 if bad-monster reputation exceeds three times good-monster reputation, or the champion flag equals 1. `[Must-match]`
- Independently subtract 20 if good-monster reputation exceeds twice bad-monster reputation, or the berserker flag equals 1. `[Must-match]`

If the initialized flag is already nonzero, the stored reaction is reused rather than recomputed from current Charisma and reputation. `[Must-match]` Missing required local storage can yield reaction -1. `[Must-match]`

Numeric reaction at most 25 is bad, 26-75 is neutral, and above 75 is good. Its stored categories are respectively 1, 2, and 3; head fidget selectors are respectively **7, 4, and 1**. Do not confuse these with option reaction codes **49 good, 50 neutral, 51 bad**. `[Must-match]`

Selecting an option maps codes 49/50/51 to an improving/unchanged/worsening head reaction. This visual reaction does not itself rewrite the NPC's persistent numeric reaction. `[Must-match]` Empathy changes option coloring by reaction: good blue, neutral green, bad red through the active palette mappings. `[Must-match]`

### 3.11 Talking-Head and Background Art Selection

A head index is a zero-based row of `art/heads/heads.lst`; it is not an object prototype ID. `[Must-match]` A background index is a zero-based row of `art/backgrnd/backgrnd.lst`. The saved default background selector begins at 2. Passing background -1 retains the previous selector rather than choosing no background. `[Must-match]` Passing head -1 selects a no-head dialogue view of the game scene and disables message-based talking-head speech. `[Must-match]`

For valid indices, the packed head art identifier is:

`0x08000000 + animation x 65536 + (variant modulo 16) x 4096 + (head index modulo 4096)`

The selected background identifier is `0x09000000 + (background index modulo 4096)`. Its frame zero, direction zero supplies the 388-by-200 scene behind the head. The dialogue shell separately uses interface art identifier `0x06000067` (index 103), and its normal lower panel uses `0x06000063` (index 99). The selector is not one of those interface-art indices. `[Must-match]`

Head list rows conventionally spell `stem,good-count,neutral-count,bad-count`. Filename selection uses the stem with the following suffixes; only fidget suffixes append the decimal variant number before `.frm`. The packed identifier above and the suffix table below are `[Must-match]`.

| Animation selector | Role | Filename suffix |
| ---: | --- | --- |
| 0 | Very good reaction | `gv.frm` |
| 1 | Good idle/fidget | `gf<variant>.frm` |
| 2 | Good to neutral | `gn.frm` |
| 3 | Neutral to good | `ng.frm` |
| 4 | Neutral idle/fidget | `nf<variant>.frm` |
| 5 | Neutral to bad | `nb.frm` |
| 6 | Bad to neutral | `bn.frm` |
| 7 | Bad idle/fidget | `bf<variant>.frm` |
| 8 | Very bad reaction | `bv.frm` |
| 9 | Good speech phonemes | `gp.frm` |
| 10 | Neutral speech phonemes | `np.frm` |
| 11 | Bad speech phonemes | `bp.frm` |

For example, synthetic stem `synth` and good variant 2 select `art/heads/synthgf2.frm`; neutral speech selects `art/heads/synthnp.frm`. The animation and variant are different packed-ID fields, not suffixes inferred from an ordinary critter's weapon/rotation animation. `[Must-match]`

The recovered head-list metadata loader populates **all three mood counts from the first numeric column**, rather than independently using the last two columns. The local corpus includes a row whose declared counts differ, so this distinction can affect actual art selection. This is a wrong-column lookup. `[May-improve]` Safe rule: read the good, neutral, and bad counts from their own columns; an engine that does so must still verify that every selectable fidget file exists, and otherwise keep the first-column count.

To choose a fidget, draw 1-100 and add half the accumulated idle-second counter, truncating:

| Available count | Chosen variant |
| ---: | --- |
| 1 | 1 |
| 2 | 1 when the adjusted draw is below 68; otherwise 2 |
| 3 | 1 below 52, 2 from 52 through 76, otherwise 3; reset the idle counter after computing the draw |
| Other count | The recovered selection retains that count as the variant rather than drawing uniformly among all variants |

At zero idle counter, the two-variant probabilities are 67%/33%; the three-variant probabilities are 51%/25%/24%, subject to the generator's modulo bias. The draw, thresholds, counter reset, and count-retaining fallback are `[Must-match]`. Invalid indices/counts or absent art do not have a guaranteed safe recovery; the reference's head-index bounds test also admits one index past the end of the list. `[May-improve]` Safe rule: treat a head index outside the list, a fidget count of zero or less, or a missing art file as no fidget, keep the current frame, and log a diagnostic.

Fidget and reaction transitions advance frames with delay `floor(1000 / frame rate)` wall-clock milliseconds; a zero file frame rate uses 10 FPS. On completion, an idle fidget returns to frame zero. The first idle waiting threshold is ten seconds; later thresholds are randomly 4-7 seconds. Waiting is measured against the retained fidget timestamp, not universally from speech completion. User choice resets the idle counter; speech completion sets it to 3. `[Must-match]`

| Current mood | Improving option | Worsening option |
| --- | --- | --- |
| Good | Play selector 0, remain good | Play selector 2, become neutral |
| Neutral | Play selector 3, become good | Play selector 5, become bad |
| Bad | Play selector 6, become neutral | Play selector 8, remain bad |

A neutral option leaves mood unchanged. Before a reaction transition, finish the current fidget; then play all transition frames and set up the destination mood's fidget and speech art. `[Must-match]`

During speech, idle animation pauses and the speech-timing system selects frames from the corresponding selector 9, 10, or 11. Phoneme indices 0-41 map to frame numbers as follows, in index order. `[Must-match]`

```text
0-13:  0 3 1 1 3 1 1 1 7 8 7 3 1 8
14-27: 1 7 7 6 6 2 2 2 2 4 4 5 5 2
28-41: 2 2 2 2 6 2 2 5 8 2 2 2 2 8
```

When speech finishes, release its resources, draw speech frame zero, and allow a new fidget. Choosing an option stops any remaining speech before its reaction. With no head, the viewport supplies the scene image instead of a selected head/background animation. Lip/sound binary formats themselves are outside this specification. `[Must-match]`

## 4. Synthetic Test Fixtures (Hex / Text)

### 4.1 `.GAM`: Index Assignment and Parser Boundaries

This complete synthetic campaign initializer uses ASCII and LF line endings. The line beginning with two spaces and slashes is deliberately not a recognized full-line comment.

```text
Preamble ignored during section search
GAME_GLOBAL_VARS:
// A column-zero comment consumes no index.
alpha = 7;

 beta = -2;
alpha = 9;
bare_label
 ; semicolon-only text still allocates
  // indented text still allocates
decimal_prefix = 0x10;
signed_prefix = +12tail;
MAP_GLOBAL_VARS:
after_heading = 3;
```

Expected campaign array, in index order zero through nine:

```text
7 -2 9 0 0 0 0 12 0 3
```

The second `alpha` does not replace the first. The later map heading does not end campaign parsing. CRLF physical endings yield the same array after text-stream translation.

Independent map initializer:

```text
MAP_GLOBAL_VARS:
first = 4;
second = -1;
// not_an_entry = 99;
```

Expected map globals are index 0 value 4 and index 1 value -1.

| Mutation/input | Expected result |
| --- | --- |
| Replace the campaign marker's final colon with `X` | Still matches the same first 16 bytes |
| Replace the map marker's colon with `X` | No matching section; successful open yields an empty map array |
| Indent or lowercase the only marker | No normal marker match; empty array |
| A retained line made of one space followed by LF, bytes `20 0A` | Allocate one zero |
| A retained semicolon-only line, bytes `3B 0A` | Allocate one zero |
| A retained slash-comment line, bytes `2F 2F 3D 39 0A` | Skip it, despite its assignment-looking text |
| After the marker, 257 copies of byte `58` followed by LF | One zero-valued entry; the leftover LF-only read is skipped |
| After the marker, 258 copies of byte `58` followed by LF | Two zero-valued entries from two nonempty chunks |
| A retained line `bad = ;` | Uninitialized numeric result in the reference; no deterministic value assertion. Under the Section 2.1 `[May-improve]` safe rule the value is zero |

### 4.2 Complete Synthetic `.GCD`

Construct exactly 428 zero bytes, then overwrite the following nonoverlapping ranges. Left-column values are hexadecimal file offsets, not file bytes. All listed numeric words are big-endian; every unlisted byte remains zero. This fully specifies the file without a generator program.

| Offset | Replacement bytes |
| --- | --- |
| `0x0004` | `00 00 00 05 00 00 00 05 00 00 00 05 00 00 00 05 00 00 00 05 00 00 00 05 00 00 00 05` |
| `0x0020` | `00 00 00 1E 00 00 00 07 00 00 00 05 00 00 00 00` |
| `0x0030` | `00 00 00 02 00 00 00 C8 00 00 00 0A 00 00 00 01` |
| `0x0040` | `00 00 00 05 00 00 00 00` |
| `0x0078` | `00 00 00 64` |
| `0x0080` | `00 00 00 0A 00 00 00 19 00 00 00 19` |
| `0x0090` | `00 00 00 02` |
| `0x011C` | `00 00 00 04` |
| `0x0154` | `00 00 00 02` |
| `0x0170` | `46 69 78 74 75 72 65 20 53 69 78 00` |
| `0x0190` | `00 00 00 00 00 00 00 0E FF FF FF FF FF FF FF FF` |
| `0x01A0` | `FF FF FF FF FF FF FF FF` |

Expected raw fields: all seven SPECIAL bases 5; Strength bonus +2; invested Small Guns 4 and Speech 2; tags 0 and 14, then two unused entries; no selected traits; name `Fixture Six`; age 25; gender 0; remaining character points zero. Body category, critter experience reward, kill category, and flags are zero. No perk ranks are encoded.

After full player initialization on normal difficulty, effective SPECIAL is `(7,5,5,5,5,5,5)`. Maximum HP is **30, not 32**; AP 7, AC 5, melee damage 2, carry weight 200, sequence 10, healing 1, critical chance 5. Player EMP resistance is 100. The final skill percentages, by indices zero through seventeen, are:

```text
68 15 15 71 61 45 35 20 30 25 25 25 35 25 59 30 35 10
```

For example, Small Guns is `35 + 5 + 20 + 2 x 4 = 68`; Speech is `25 + 2 x 5 + 20 + 2 x 2 = 59`. Decoding the bonus array as another set of bases, or treating the skill words as ready-to-use percentages, produces different incorrect results.

Mutation checks:

- Replacing the HP base word at `0x0020` with `00 00 03 09` stores 777, but full derived recomputation restores the base to 30.
- Replacing the body/experience/kill words at `0x0164` with three nonzero big-endian integers demonstrates player-only normalization to zero.
- Removing the final byte truncates the remaining-points word. The low-level player-sheet read reports failure; full outer initialization can still continue with partial state, so this is not a transactional load. Under the Section 2.3 `[May-improve]` safe rule the sheet is rejected before any state changes.
- Replacing every byte of the 32-byte name with nonzero text violates the safe name-termination requirement; the reference does not supply a dependable clean rejection. Under the Section 2.3 `[May-improve]` safe rule it is rejected with a diagnostic.

### 4.3 Worked Stat, Trait, and Perk Derivations

These examples assume no combat unspent-AP AC addition, normal difficulty, no blindness, no prior level-up HP bonus, and no other equipment/drug effects.

| State | Maximum HP | AP | AC | Melee | Carry | Sequence | Healing | Critical chance |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| All raw SPECIAL 5, no traits/bonuses | 30 | 7 | 5 | 1 | 150 | 10 | 1 | 5 |
| Same raw SPECIAL, Strength bonus +2, as in the sheet fixture | 30 | 7 | 5 | 2 | 200 | 10 | 1 | 5 |
| All raw SPECIAL 5, Gifted and Small Frame, no bonuses | 33 | 8 | 7 | 1 | 125 | 12 | 2 | 6 |
| Previous trait state plus the perk effects below | 33 | 10 | 12 | 3 | 175 | 14 | 3 | 11 |

For the Gifted/Small Frame row, effective SPECIAL is `(6,6,6,6,6,7,6)`. HP is `15 + 6 + 2 x 6 = 33`. Carry weight is `25 + 25 x 6 - 10 x 5 = 125`: the subtraction uses original raw Strength 5, not effective Strength 6. With zero invested points, tagged Small Guns is `35 + 7 + 20 - 10 = 52`; untagged Science and Speech are each `25 + 2 x 6 - 10 = 27`.

For the last row, assume granted effects of Action Boy rank 2, Dodger rank 1, Bonus HtH Damage rank 1, Strong Back rank 1, Earlier Sequence rank 1, Faster Healing rank 1, and More Criticals rank 1. Add respectively 2 AP, 5 AC, 2 melee damage, 50 carry weight, 2 sequence, 1 healing, and 5 critical chance. These are effect calculations, not a claim that this is a legal starting character or that a `.GCD` stores those ranks.

At a later single advancement with trait-adjusted Endurance 6 and Lifegiver rank 1, the added maximum-HP bonus is `2 + 3 + 4 = 9`. If no previous advancement bonuses exist, maximum HP becomes 42. Current HP need not jump from 33 to 42 because the healing request occurs before that maximum increase.

### 4.4 RNG, IQ, Art, and Event State Vectors

For a fresh, zero-initialized shuffle array, seed 1 with the precise seeding rule in Section 3.4, before any startup diagnostic or other random draw. The first six raw shuffled outputs and corresponding 1-100 draws are:

```text
Raw:  0 812987216 1817129560 1441282327 16531729 1998097157
D100: 1 17        61         28         30       58
```

This fixture's cleared shuffle entry zero is essential; merely reseeding a running game need not reproduce it.

For an independent check-result example, skill chance 68 with first draw 42 gives margin 26. At/after tick 864000 with critical chance 5, the upgrade threshold is 7: second draw 7 gives code 3 and second draw 8 gives code 2. First draw 93 instead gives margin -25 and critical-failure threshold 2: second draw 2 gives code 0. Before tick 864000, omit that second draw and return ordinary success/failure. A stat 5 with modifier +1 and d10 result 6 yields success code 2 and margin zero without a critical draw.

For IQ filtering, effective Intelligence 3 and no Smooth Talker rejects threshold +4 and accepts -4 and -3. With one Smooth Talker rank, I becomes 4: +4 and -4 both pass at equality, while -3 fails. Intelligence 10 plus three ranks accepts threshold +12; the adjusted filter value is 13, not re-clamped to 10.

For synthetic head-list row 7 with stem `synth`, neutral fidget variant 2 has art identifier `0x08042007` and filename `art/heads/synthnf2.frm`. Neutral phoneme art is `0x080A0007`. An improving choice from neutral plays transition `0x08030007`, then chooses a good fidget. Background row 5 gives `0x09000005`; a subsequent background argument -1 retains row 5.

For a spatial centered at tile 2010, elevation zero, radius 1, moves to tiles 2011 and 2211 each match and each can trigger a callback, even without leaving the region between them. Tile 2412 is distance 3 and does not match. The same tile numbers at another elevation do not match this spatial. A nested movement during the disabled spatial scan does not recursively trigger that scan.

For the queue, enqueue A, B, then C at current tick 1000 with delays 10 each. A processing pass at 1009 dispatches none. At 1010, A returning zero is followed by B; if B returns nonzero, C remains pending until another pass, even though C is already due. A periodic map update processed late at tick 2633 reschedules for 3233, not for an earlier nominal cadence boundary.

## 5. Clean-Room Verification Checklist
- [x] Zero C/C++ code snippets or struct definitions included
- [x] All multi-byte integer endianness explicitly stated
- [x] Synthetic test vectors included
- [x] Every documented quirk tagged `[Must-match]` or `[May-improve]`
- [x] No internal reference symbol names; only public SSL opcode names and file-format key literals
- [x] Spec linter run against `fallout1-re` symbols with no findings
