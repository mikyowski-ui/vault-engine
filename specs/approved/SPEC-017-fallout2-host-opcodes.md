# SPEC-017: Fallout 2 Host Words and VM Deltas

**Status:** approved by the Spec-Author sub-agent (Team A), 2026-10-25, under AGENTS.md "Spec Authoring (Team A)". Section 8 revised (Revision 2) by the coordinator at the owner's request, 2026-10-25. Revision 3 (retail audit recorded; script gate open) by the coordinator, 2026-10-25. Revision 4 (the opt-in sfall word set, section 2) by Team A, 2026-10-30, on the owner's instruction.

**Provenance.** Author: the Spec-Author sub-agent (Team A), 2026-10-25.
Sources:

- the owner's brief for this spec, relayed by the coordinator on 2026-10-25
  (the range `0x8156..=0x8183`, `vault_vm::WordSet`, and the separate
  enhanced range);
- the project's approved specs: SPEC-003 to SPEC-016, and SPEC-018 §1;
  for the reconciliation pass of the same day, SPEC-018 and SPEC-019 in full;
- `docs/fallout2-and-qol-roadmap.md`;
- Team A's general knowledge of publicly documented Fallout 2 scripting:
  public SSL procedure, opcode and constant names, and the argument order of
  public script calls.

No decompilation, disassembly or re-implementation source was consulted, and
no game asset was opened. Team A's general
knowledge comes from model training data whose sources cannot be audited, so
this spec cannot certify that none of its content traces back to
reverse-engineered material (see `PROVENANCE.md`, "In-session spec
authoring"). Section 8 defines a corpus audit that checks the word table
against the user's own Fallout 2 scripts, using counts only.

**Revision 2 (§8 audit model), 2026-10-25.** The coordinator wrote this
revision at the owner's request, after the first revision's §8.4 model check
reported 1082 findings over the owner's Fallout 1 corpus. Its sources are:

- the approved SPEC-004 and SPEC-005;
- the counts the §8 audit reports;
- the instruction words of a few Fallout 1 procedures in the owner's local
  corpus, read only to classify those findings.

No script byte is reproduced here, and nothing outside section 8, Decisions
31 to 35 and this note changes. The implementing agent is not this
revision's author.

**Revision 3 (retail Fallout 2 audit), 2026-10-25.** The coordinator wrote this
revision at the owner's request. It ran section 8's audit over the owner's
retail Fallout 2 install (the GOG release).

- **What was opened.** Only SPEC-018 §1's three archives, mounted as SPEC-018
  says. sfall's archive and DLL, `f2_res.dat` and the `mods` folder were
  neither mounted nor opened.
- **What changed.**
  - section 3.2 gains `0x8109`;
  - section 8.4 records the audit;
  - section 8.5's gate is **open**;
  - Decisions 36 to 41 are new.
- **What it cites.** Counts, word numbers, two script names and constant
  selector values only. No script byte is reproduced.

**Revision 4 (the sfall word set), 2026-10-30.** The Spec-Author sub-agent
(Team A) wrote this revision on the owner's brief of 2026-10-30, which approves
SPEC-024b Decision D-3.

- **What changed.**
  - section 2.1 gains the third set, `FALLOUT2_SFALL`;
  - sections 2.2 to 2.4 say how that set sits beside the enhanced range;
  - section 4.1 says when its rows can be reached;
  - section 8.5 names the set Play loads with;
  - section 7.14 and Decisions 42 to 46 are new, and Decision 1 is amended.
- **What did not change.** With sfall mode off, which is the default, no word
  changes number or meaning under either profile, with or without the enhanced
  preset. Sections 3, 5 and 8.1 to 8.4, the vectors of sections 7.1 to 7.13 and
  8.6, and the audit results are unchanged.
- **Sources.**
  - the owner's brief;
  - SPEC-024b and `oracle/SPEC-024/sfall-opcodes.tsv`;
  - `docs/REFERENCE.md` (the enhanced words, and mod-script verification).

No decompilation, disassembly, re-implementation source or game asset was used,
and no engine Rust code was read.

## Clean-Room Rules for Research Agents
- Document ONLY binary file layouts (byte offsets, endianness, data types), mathematical formulas, state transitions, and observable behavior.
- DO NOT copy C/C++ code, struct definitions, function names, internal variable names, or implementation-source paths from any inspected implementation.
- Provide small synthetic hex/text test vectors at the bottom of the specification.

## 1. Overview & Scope

This spec defines what compiled scripts (`.INT`, SPEC-004) may execute under
the Fallout 2 profile (`game = "fallout2"`). It covers:

1. the instruction-word set of each profile, and how the enhanced range sits
   beside them (section 2);
2. the VM-core deltas from SPEC-004 (section 2.4);
3. which SPEC-005 words change under Fallout 2, and which keep their meaning
   but read Fallout 2 data (section 3);
4. the 46 words `0x8156..=0x8183` (section 4);
5. the multiplexed words `metarule` (Fallout 2 rules) and `metarule3`
   (section 5);
6. what SPEC-018 and SPEC-019 provide (section 6);
7. synthetic test vectors (section 7);
8. a corpus audit that gates Fallout 2 **script execution**, and what Play
   does while scripts are gated (section 8).

**The main finding.** Team A knows of few script facilities that Fallout 2
adds, and in Team A's knowledge most of them sit in **Fallout 1 word slots
whose Fallout 1 meaning was a stub or a defective routine**. A few more are
sub-functions of the multiplexed words. So section 3.2 reassigns six SPEC-005
words under Fallout 2. Section 4 gives four words full contracts and reserves
the other 42 words of the requested range. A reserved word is in the word
set, so a program that contains one loads, but executing it raises the
undefined-instruction VM error. A wrong table therefore fails closed, and the
section 8 audit finds it.

**Compatibility tags.** Each rule carries one tag:

- `[Must-match]`: script-visible behavior that compiled Fallout 2 scripts
  depend on: the word number, the pop order, the push, the operand type
  checks, and the stated host effect. The engine must implement it exactly
  as written. Every `[Must-match]` statement here is Team A's statement of
  Fallout 2 behavior. It stays **provisional** until the section 8 audit of a
  real corpus confirms it.
- `[May-improve]`: a known reference defect, with a safe rule. Team A knows
  of no Fallout 2 defect in the words defined here, so this tag appears only
  where a SPEC-005 rule is carried over.
- `[Safe rule]`: an engine rule where Team A does not know the reference
  behavior, or where the reference is undefined. It is deterministic, never
  undefined behavior, and refuses or ignores what it does not define.

**Conventions inherited from SPEC-005 §2.1.** `[Must-match]`

- Instruction words are big-endian; no word here has inline operands.
- **Pop order is first popped to last popped.** A script pushes operands in
  the reverse of the listed order, so the public call's **first** argument is
  popped **last**.
- Unmarked operands are `I` (integer tag `0xC001`, compared after clearing
  bit `0x0800`). `S` is a string (static `0x9001` or dynamic `0x9801`). `U`
  is popped with no tag check. A failed type check is a VM error (SPEC-004
  §3.8), not a pushed status.
- `none` in the push column means nothing is pushed. A statement never leaves
  an invented status.
- `I bool` is integer 0 or 1. `I object` is an opaque object reference, and
  zero is null.

**References.** An object operand is an opaque reference (SPEC-005 §2.1).
Zero is null. Under this spec a **nonzero reference that does not name a live
object** behaves exactly as null, with a diagnostic. `[Safe rule]`

**Diagnostics.** A diagnostic is one line in the engine's debug log (the
`vault-debug` trace) naming the word and the reason. It never stops the
program and never changes the stack contract. `[Safe rule]`

## 2. Word Sets and Dispatch

### 2.1 The two profiles

| Profile | `vault_vm::WordSet` | Host words |
| --- | --- | --- |
| `fallout1` | `FALLOUT1` = `0x8000..=0x8155` (unchanged) | SPEC-005 |
| `fallout2` | `FALLOUT2` = `0x8000..=0x8183` | SPEC-005 as amended by section 3, plus section 4 |
| `fallout2` with sfall mode on | `FALLOUT2_SFALL` = `0x8000..=0x8281` | `0x8000..=0x8155` as under `FALLOUT2`, then SPEC-024b's table for `0x8156..=0x8281` |

The rules below are `[Must-match]` unless tagged.

- **Loading.** A program is checked against the active profile's set exactly
  as `FALLOUT1` checks today. Slot-1 pushes (`0x8001 + n * 0x0400`) are
  always accepted. Any other word must lie in the set.
- **The same words under both profiles.** `0x8000..=0x804B` (SPEC-004 core)
  and `0x804C..=0x80A0` (SPEC-005 library) are identical under both
  profiles. The game words `0x80A1..=0x8155` are identical except for the
  six words of section 3.2.
- **Fallout 1 is unchanged.** Under `fallout1`, words `0x8156..=0x8183` are
  still outside the set, so a program containing one is refused at load, as
  today. Under `fallout1`, every SPEC-005 row keeps its meaning, including
  the six rows section 3.2 replaces under Fallout 2.
- **Reserved words.** Under `fallout2`, the words that section 4 reserves are
  in the set: a program containing one loads. Executing one consumes nothing
  and raises the undefined-instruction VM error, like SPEC-005's
  `0x807D`/`0x807E`. `[Safe rule]`
- **Dispatch index.** SPEC-004 §3.8 masks a word to its low ten bits. The
  Fallout 2 dispatch table has **388 entries**, indices 0-387
  (`0x8000`-`0x8183`). The SPEC-004 §3.8 safe rule applies above it: an
  index from 388 to 1023 is an unregistered slot and raises the
  undefined-instruction VM error. The engine's existing load-time rule that
  rejects every non-push word with bits 10-14 set (SPEC-004 §2.5,
  `[May-improve]`) is unchanged under both profiles.

**Sfall mode (Revision 4).** `[Safe rule]`, as an owner instruction (Decision 42).

- **Off by default.** Sfall mode is the opt-in session setting of SPEC-024b
  section 3.1: from the settings file, a mod manifest or the command line, and
  under `fallout2` only.
- **Off.** `FALLOUT1`, `FALLOUT2`, their dispatch tables and the enhanced range
  are exactly as the rules above and section 2.2 state them. No word changes
  number or meaning, and every existing test and script behaves as before.
- **On.** A program is checked against `FALLOUT2_SFALL`, and every word in
  `0x8156..=0x8281` routes to SPEC-024b's table. That covers section 4.1's words,
  `0x8184..=0x81FF` (which no other set holds), and the part of the enhanced
  range section 2.2 names. `0x8000..=0x8155` stays as under `FALLOUT2`, including
  the seven words of section 3.2 and the multiplexed words of section 5.
- **Dispatch index.** The `FALLOUT2_SFALL` table has **642 entries**, indices
  0-641 (`0x8000`-`0x8281`). An index from 642 to 1023 is an unregistered slot, as
  above. The load-time rule on bits 10-14 is unchanged.

### 2.2 The enhanced range

`0x8200..=0x82FF` stays separate from both game sets. `[Safe rule]`

- It belongs to neither `FALLOUT1` nor `FALLOUT2`.
- It is accepted only when the enhanced preset is active, under either
  profile, and it dispatches through the enhanced table by its full word, as
  today.
- `0x8184..=0x81FF` belongs to no set and no extension, except under sfall
  mode. A program containing one of these words is refused at load under both
  profiles.
- No word in this spec is renumbered into the enhanced range, and the
  enhanced range never shadows a game word.

**Under sfall mode (Revision 4).** `FALLOUT2_SFALL` overlaps this range, and the
sfall word always wins, whatever the preset. (Decisions 42 to 44)

| Range | Sfall mode off | Sfall mode on |
| --- | --- | --- |
| `0x8000..=0x8155` | Sections 2.1 and 3 | Unchanged |
| `0x8156..=0x8183` | Section 4.1 | SPEC-024b |
| `0x8184..=0x81FF` | Refused at load | SPEC-024b |
| `0x8200..=0x8281` | Enhanced preset only, otherwise refused at load | SPEC-024b, with or without the enhanced preset |
| `0x8282..=0x82FF` | Enhanced preset only, otherwise refused at load | Unchanged |

The enhanced words defined today inside the overlap cannot be reached under
sfall mode. Each number means the sfall word instead, and the stack contracts
differ:

| Word | Enhanced word | Sfall word |
| --- | --- | --- |
| `0x8200` | `engine_api_version` | `fs_size` |
| `0x8210` | `op_cam_pan_to` | `sfall_ver_major` |
| `0x8211` | `op_cam_set_zoom` | `sfall_ver_minor` |
| `0x8212` | `op_cam_shake` | `sfall_ver_build` |
| `0x8213` | `op_cam_reset` | `hero_select_win` |

So a script written for those enhanced words does not do its enhanced work under
sfall mode. Mod-script verification runs with SPEC-024b's table there, so it
judges such a script by the sfall contracts. No enhanced word is defined in
`0x8282..=0x82FF` today, so the enhanced preset adds no script word under sfall
mode. Its other settings are unaffected.

### 2.3 Dispatch per profile

The engine keeps one dispatch table per profile. A word's meaning is chosen
by the active profile when the program runs, never by inspecting the script.
A save made under one profile never loads under the other (roadmap, "Saves";
SPEC-018 §7.2),
so a running program never changes profile. `[Safe rule]`

Under `fallout2`, sfall mode selects a second table (Revision 4). The session
setting chooses it, never the script. The setting is fixed when the session is
built, and loading a save does not change it, so a running program never changes
word set either. `[Safe rule]` (Decision 45)

### 2.4 VM-core deltas from SPEC-004

**No VM-core change.** Under Fallout 2, SPEC-004 holds unchanged:

- the file layout, the bootstrap and the tags;
- the stacks (2048 bytes each), the frames and the instruction budget of 10;
- the return and context words, the arithmetic and the errors.

The only change is the end of the registration range: SPEC-004 §3.7 says the
game words extend through `0x8155`. Under Fallout 2 they extend through
`0x8183`, and under sfall mode through `0x8281`. SPEC-004 §4.3's note that word
`0x8156` reads outside the dispatch array applies under `fallout1` only.
`[Must-match]`

**Lifecycle procedures.** Fallout 2 adds four public procedure names after
SPEC-004 §3.7's table. They follow the same lookup and action-code rules
(SPEC-004 §3.7, "Action code"): `script_action` returns the event number
during the request.

| Public entry name | Event number | Requesting host path |
| --- | ---: | --- |
| `push_p_proc` | 24 | The player's Push action on a party member (SPEC-019 §3.6). Source is the player, target the pushed critter. An override suppresses the default push. |
| `is_dropping_p_proc` | 25 | Never requested in this revision. `[Safe rule]` |
| `combat_is_starting_p_proc` | 26 | Never requested in this revision: SPEC-019 defines no path. `[Safe rule]` |
| `combat_is_over_p_proc` | 27 | Never requested in this revision: SPEC-019 defines no path. `[Safe rule]` |

The names and numbers are `[Must-match]`. The paths are as tagged. Under
`fallout1` no event above 23 exists.

## 3. SPEC-005 Words Under Fallout 2

### 3.1 The general rule

Under Fallout 2, every SPEC-005 row of `0x804C..=0x8155`, except the six of
section 3.2, keeps:

- its public name and pop order;
- its push;
- its operand type checks;
- its `[Must-match]` host effect;
- its `[May-improve]` safe rule.

The host effect acts on Fallout 2 world state. `[Must-match]` (owner default:
unchanged unless stated, with a reason).

Some rows keep their meaning but take their constants from Fallout 2 data.
The row's mechanism is unchanged; only the data source changes:

| Word | Name | Fallout 2 data source |
| --- | --- | --- |
| `0x80C5`, `0x80C6` | `global_var`, `set_global_var` | The campaign array from Fallout 2's `data/vault13.gam` (SPEC-018 §6.2) |
| `0x80E4` | `load_map` | SPEC-018 §5.1's map table replaces SPEC-008 §3.10's `map.msg` 0-65, for numbers and for name matching. The entrance parameter is still popped, but it is stored in **no** campaign global: SPEC-005's write of element 32 does not happen under Fallout 2 (SPEC-018 §5.1, Decision 25). The map loads at its header's default entrance. `[Safe rule]` |
| `0x8101` | `cur_map_index` | SPEC-018 §5.1's map numbers |
| `0x80EA`, `0x80EB`, `0x80F6`, `0x8118`, `0x8119` | Clock and calendar | The clock rules of SPEC-006 §3.7 and SPEC-005 §3.5, with SPEC-018 §6.1's calendar (tick 0 is 25 July 2241) and reset tick |
| `0x80A1` | `give_exp_points` | SPEC-011's level table and perk timing, provisionally, until a later spec gives Fallout 2's (SPEC-018 §4.7). Party-member level-ups follow SPEC-019 §3.17 |
| `0x8121` | `giq_option` | The Smooth Talker perk's SPEC-011 index, provisionally (SPEC-018 §4.7) |
| `0x8124`, `0x8125`, `0x814B` | `party_add`, `party_remove`, `party_member_obj` | Fallout 2's party-member table (SPEC-018 §4.5) and party membership (SPEC-019 §3.14) |
| `0x8104` | `proto_data` | Prototype fields read under SPEC-016. The selector table of SPEC-005 §3.6 is unchanged. |
| `0x810B` | `metarule` | Rule 15 uses the engine-owned Fallout 2 elevator table (SPEC-018 §4.9) in place of SPEC-013 §2.7. Section 5.1 adds the Fallout 2 rules. |
| `0x8108`, `0x8109` | `world_map`, `town_map` | Each queues the same request kind as in SPEC-005. The Fallout 2 world-map and town-map screens are SPEC-019 §3.12 and §3.13. |
| `0x8115` | `play_gmovie` | Fallout 2's 17-entry movie table, indexes 0-16 (SPEC-018 §6.4), with its per-movie flags (SPEC-019 §3.21). The SPEC-005 safe rule ignores, with a diagnostic, a number outside 0-16, in place of 0-13. |
| `0x8146`, `0x8148` | `endgame_slideshow`, `endgame_movie` | SPEC-019 §3.20, with the slides of SPEC-018 §4.6's `endgame.txt` |
| `0x810F` | `reg_anim_animate` | The violence-filter special case keeps PID `0x0100002F` and animation 20 unchanged. `[Safe rule]` |

**Fallout 2 spellings of existing words.** These public Fallout 2 names add
no word. Each one compiles to an existing word or sub-function, so the engine
needs nothing more for it. This table is Team A's knowledge of the public
script headers. It is checked only indirectly, through the section 8 audit.

| Fallout 2 call | Compiles to |
| --- | --- |
| `gdialog_mod_barter(modifier)` | `0x8129` (`gdialog_barter`): the activation parameter is the barter modifier (SPEC-011 §3.17) |
| `obj_is_visible_flag(who)` | `has_trait(1, who, 666)` (`0x80F3`, selector 666, SPEC-005 §3.5) |
| `rm_fixed_timer_event(who, fixed)` | `metarule3(100, who, fixed, 0)` |
| `art_change_fid_num(who, fid)` | `metarule3(107, who, fid, 0)` |
| `tile_set_center(hex)` | `metarule3(108, hex, 0, 0)` |
| `mark_map_entrance_state(map, state, elevation)` | `metarule3(104, map, state, elevation)` |
| `wm_subtile_state(x, y)`, `tile_get_next_critter(...)`, `get_kill_count`-style calls, `chem_use_level`, `car_out_of_fuel`, `map_get_load_area` | The `metarule3` sub-functions of section 5.2 |
| `town_known`, `map_known`, `cur_town`, `car_current_town`, `is_skill_tagged`, `obj_drop_everything`, `inven_unwield_who`, `worldmap_xpos`, `worldmap_ypos`, `language_filter`, `violence_filter`, `weapon_dmg_type`, `critter_barters`, `critter_kill_type` and the car calls | The `metarule` rules of section 5.1 |

### 3.2 Words reassigned under Fallout 2

Under `fallout2` these seven words replace their SPEC-005 rows (six from
the first revision, and `0x8109` from Revision 3's audit). In Fallout 1
each was a stub, or a routine with a defect so severe that its slot was
reusable: barter stubs, reaction stubs, an unsupported jump, and the
non-terminating area-visibility loops. Fallout 2's public script calls of
these names have the argument orders below. Under `fallout1` the SPEC-005
rows are unchanged. `[Must-match]` for each row's pop order, push and checks.

| Word | Fallout 1 meaning (SPEC-005) | Fallout 2 name | Pop order | Push | Host effect |
| --- | --- | --- | --- | --- | --- |
| `0x80B2` | `reaction_roll` stub | `mark_area_known` | state, index, mark type | none | See section 4.2. `[Must-match]` |
| `0x80CF` | `animate_jump`, pops nothing | `tile_in_tile_rect` | test hex, lower-right hex, lower-left hex, upper-right hex, upper-left hex | I bool | See section 4.3. `[Must-match]` |
| `0x80E1` | `turn_off_objs_in_area` | `metarule3` | third parameter U, second parameter U, first parameter U, sub-function | I value | See section 5.2. `[Must-match]` |
| `0x80E2` | `turn_on_objs_in_area` | `set_map_music` | track name S, map number | none | See section 4.4. `[Must-match]` |
| `0x80E5` | `barter_offer` stub | `wm_area_set_pos` | y, x, area | none | See section 4.5. `[Must-match]` |
| `0x80E6` | `barter_asking` stub | `set_exit_grids` | orientation, destination hex, destination elevation, destination map, marked elevation | none | See section 4.6. `[Must-match]` |
| `0x8109` | `town_map`, pops nothing | *(unnamed; Revision 3)* | third operand U, second operand I, first operand U | I 0 | No effect, with one diagnostic per program. `[Safe rule]` (Decision 37) |

The SPEC-005 `[May-improve]` rules of `0x80E1` and `0x80E2` (the area-loop
termination) do not apply under Fallout 2, because those routines do not
exist there.

The other Fallout 1 stubs keep their SPEC-005 rows under Fallout 2:
`skill_contest` (`0x80AD`), `reaction_influence` (`0x80B3`), `roll_dice`
(`0x80B5`) and `make_daytime` (`0x80D1`). The section 8 audit reports how
the Fallout 2 corpus uses each of them. `[Safe rule]`

## 4. Fallout 2 Words

### 4.1 The range `0x8156..=0x8183`

| Word | Public name | Pop order | Push | Checks and host effect |
| --- | --- | --- | --- | --- |
| `0x8156` | `inven_cmds` | index, command, owner | I object | Command 13 returns the owner's inventory entry at the zero-based index, as its representative object. Entries are counted in SPEC-003 §2.12 inventory order, one per stack, so equipped items count too: the same entries that `critter_inven_obj` selector -2 counts. An index below 0, or at or beyond the entry count, gives I 0. Any other command gives I 0 with a diagnostic. A null owner gives I 0 with a diagnostic. The owner may be any object with an inventory. `[Must-match]` for command 13; `[Safe rule]` for the rest. |
| `0x8157` | `reg_anim_change_fid` | delay, art identifier, object | none | Outside combat, append one **set-art** entry (SPEC-007 §3.10.1, instant) to the sequence being registered. The entry's owner is the object, its start delay the delay, and its new art the complete 32-bit identifier, used as given. In combat the operands are consumed and nothing happens. A null object only diagnoses. Registration follows the same rules as SPEC-005 §2.6's registration words, including what happens with no sequence open. An identifier whose art cannot be loaded makes the whole sequence fail (SPEC-007 §3.10.1, "Failure"). `[Must-match]` |
| `0x8158` | `reg_anim_take_out` | delay, weapon code, object | none | Outside combat, append one **take-out-weapon** entry (SPEC-007 §3.10.1, timed) for the object: it plays the take-out animation for that weapon code (art bits 12-15, SPEC-007 §2.1), and the object's art then keeps the new weapon code. The delay works as in `reg_anim_change_fid`, and the combat gate and null rule are the same. A weapon code outside 1-15 registers nothing for this entry, with a diagnostic. `[Must-match]`; `[Safe rule]` for the code range. |
| `0x8159` | `reg_anim_turn_towards` | delay, hex, object | none | Outside combat, append one **turn-toward-a-hex** entry (SPEC-007 §3.10.1, instant): the object faces the hex, using SPEC-005 `rotation_to_tile` from its own hex. The combat gate and null rule are the same. A hex outside 0-39999 registers nothing for this entry, with a diagnostic. `[Must-match]`; `[Safe rule]` for the hex range. |
| `0x815A`-`0x8183` | Reserved (42 words) | none | none | Each of these 42 words is in the Fallout 2 set, so a program containing one loads. Executing one consumes nothing and raises the undefined-instruction VM error (SPEC-004 §3.8). The program ends with the error state. `[Safe rule]` |

The range holds 46 words: four defined and 42 reserved. Team A placed the
four defined words at `0x8156`-`0x8159`; their numbers are the least certain
part of this spec (Decisions 5 and 6).

**Under sfall mode (Revision 4).** This table applies to `FALLOUT2` only. Under
`FALLOUT2_SFALL`, `0x8156..=0x8183` are SPEC-024b's words. The three
registration words keep the effects and pop orders above at sfall's numbers:
`reg_anim_change_fid` at `0x825E`, `reg_anim_take_out` at `0x825F` and
`reg_anim_turn_towards` at `0x8260`. `inven_cmds` has no number in SPEC-024b's
table, so it cannot be reached under sfall mode. `[Safe rule]` (Decision 42)

### 4.2 `mark_area_known` (`0x80B2` under Fallout 2)

Public call: `mark_area_known(mark type, index, state)`. Pop order: state,
index, mark type. No push. `[Must-match]`

| Mark type | Index names | Effect |
| ---: | --- | --- |
| 0 | An area: one `city.txt` record, which SPEC-019 calls a city (SPEC-018 §4.3) | The area's known state (SPEC-018 §5.8) becomes the given state: 0 unknown, 1 known, 2 known and visited. The area's world-map marker and town-map entry follow SPEC-019 §3.12. |
| 1 | A map number (SPEC-018 §5.1) | State 0 clears the map's known and visited flags (SPEC-018 §5.2). State 1 sets the known flag. State 2 sets both. These flags are what `metarule` rule 19 reads. |

- The first two rows' meanings are `[Must-match]`.
- Any other mark type, any other state value, or an index outside its table
  changes nothing, with a diagnostic. `[Safe rule]`
- Marking an area known neither moves the party nor reveals world-map
  subtiles. Subtiles change only through `metarule3` 101, through travel
  (SPEC-018 §5.5) and at a new game (SPEC-018 §6.5). `[Safe rule]`

### 4.3 `tile_in_tile_rect` (`0x80CF` under Fallout 2)

Public call: `tile_in_tile_rect(upper-left, upper-right, lower-left,
lower-right, test hex)`. Pop order: test hex, lower-right, lower-left,
upper-right, upper-left. Push I bool. `[Must-match]`

- Each hex is split into column `hex mod 200` and row `trunc(hex / 200)`, the
  numbering of SPEC-005 `set_map_start` (hex = 200 x row + column).
- The rectangle is every hex whose column lies between the smallest and the
  largest corner column, and whose row lies between the smallest and the
  largest corner row, bounds included.
- The result is 1 when the test hex is in the rectangle, otherwise 0.
- The order of the four corners does not change the result. `[Safe rule]`
- Any of the five operands outside 0-39999 gives 0. No diagnostic is
  needed. `[Safe rule]`

### 4.4 `set_map_music` (`0x80E2` under Fallout 2)

Public call: `set_map_music(map number, track name)`. Pop order: track name
S, map number. No push. `[Must-match]`

- The map's city track (SPEC-008 §3.10, "City music") becomes the given
  name. A name means `sound/music/<name>.acm`, looked up case-insensitively.
  The override replaces SPEC-018 §5.2's track for that map from then on.
- When the map is the current map, the background track is stopped and the
  new track starts from its beginning, looping, at once.
- The override lasts until the next full game reset: a new game, a game
  load, or a return to the main menu. It is not written to the save.
  `[Safe rule]`
- A map number with no `maps.txt` record (SPEC-018 §4.2) changes nothing,
  with a diagnostic. A name longer than 15 bytes is cut to 15 bytes. An empty name
  removes the override, so the map's table track applies again.
  `[Safe rule]`
- A missing track file is handled as SPEC-010 handles any missing music
  file: no sound, no error. `[Safe rule]`

### 4.5 `wm_area_set_pos` (`0x80E5` under Fallout 2)

Public call: `wm_area_set_pos(area, x, y)`. Pop order: y, x, area. No push.
`[Must-match]`

- The area's current world-map position (SPEC-018 §5.8: the world-pixel
  point that its marker, its circle and arrival use) becomes `(x, y)`.
- The change persists in the saved world-map state (SPEC-018 §7.4, per-area
  X and Y).
- The party does not move, even when it is in that area.
- An area outside the table, or a point outside the world (SPEC-018 §5.3),
  changes nothing, with a diagnostic. `[Safe rule]`

### 4.6 `set_exit_grids` (`0x80E6` under Fallout 2)

Public call: `set_exit_grids(marked elevation, destination map, destination
elevation, destination hex, orientation)`. Pop order: orientation,
destination hex, destination elevation, destination map, marked elevation.
No push. `[Must-match]`

- Every exit-grid object on the current map whose elevation equals the
  marked elevation is changed. Exit grids are the misc PIDs
  `0x05000010`-`0x05000017` (SPEC-013 §3.22), on any hex, whether hidden or
  not.
- Each one's four destination words (SPEC-003 §2.11: map, hex, elevation,
  orientation) are replaced by the operands, as given.
- The objects keep their art. The SPEC-003 art rebuild for destinations of 0
  or below happens only when a map is read.
- The new destinations are object state, so they are saved with the map's
  working copy (SPEC-014 §3.35) like any other object field.
- A marked elevation outside 0-2 changes nothing. No diagnostic is needed.
  `[Safe rule]`
- The values are not validated beyond that: the transition rules of SPEC-013
  §3.22 interpret them when the player steps on the grid. `[Must-match]`

## 5. Multiplexed Words

### 5.1 `metarule` (`0x810B`): Fallout 2 rules

Pop order and push are unchanged: parameter, rule; push I value. The rule
operand must be I, and so must the parameter. A rule that ignores its
parameter still pops it. `[Must-match]`

| Rule | Public name | Parameter | Result under Fallout 2 |
| ---: | --- | --- | --- |
| 13 | `METARULE_SIGNAL_END_GAME` | ignored | As SPEC-005: end-game request, I 0 |
| 14 | `METARULE_TEST_FIRSTRUN` | ignored | As SPEC-005 |
| 15 | `METARULE_ELEVATOR` | elevator type | As SPEC-005, with the elevator table of SPEC-018 §4.9; a type without a record does nothing, with a diagnostic; I 0 |
| 16 | `METARULE_PARTY_COUNT` | ignored | As SPEC-005 |
| 17 | `METARULE_AREA_KNOWN` | area | I 1 when the area's known state (SPEC-018 §5.8) is 1 or 2, else 0. An area outside the table gives 0 |
| 18 | `METARULE_WHO_ON_DRUGS` | critter | I 1 when the timed queue holds at least one drug-stage event (queue type 0, SPEC-011 §2.5) owned by the critter, else 0. Null gives 0 |
| 19 | `METARULE_MAP_KNOWN` | map number | I 1 when the map's known flag (SPEC-018 §5.2) is set, else 0. A map outside the table gives 0 |
| 22 | `METARULE_IS_LOADGAME` | ignored | As SPEC-005 |
| 30 | `METARULE_CAR_CURRENT_TOWN` | ignored | I `car_area` while `has_car` is 1, else -1 (SPEC-019 §3.19.1) |
| 31 | `METARULE_GIVE_CAR_TO_PARTY` | ignored | `has_car` becomes 1, `car_area` the current map's area and `car_map` the current map (SPEC-019 §3.19.1); I 0 |
| 32 | `METARULE_GIVE_CAR_GAS` | amount | `car_fuel` becomes `clamp(car_fuel + amount, 0, 80000)` (SPEC-019 §3.19.1); I 0. Without a car, nothing changes |
| 40 | `METARULE_SKILL_CHECK_TAG` | skill | I 1 when the skill is one of the player's tagged skills (SPEC-006 §2.2, offset `0x0190`), else 0. Skills outside 0-17 give 0 |
| 42 | `METARULE_DROP_ALL_INVEN` | critter | Every inventory entry, in inventory order, is removed and placed on the critter's own hex and elevation. Worn and held flags are cleared. For the player, the armor's effects are removed (SPEC-011 §3.10), the default player art applies as for `rm_obj_from_inven` (SPEC-005 §3.8), and the interface bar is refreshed; I 0. Null gives I 0 with a diagnostic |
| 43 | `METARULE_INVEN_UNWIELD_WHO` | critter | Unwield, as SPEC-005 `inven_unwield` does for its owner: the critter's right hand, or the player's active hand; I 0. Null gives I 0 with a diagnostic |
| 44 | `METARULE_GET_WORLDMAP_XPOS` | ignored | I the party's world-map X position, in world pixels (SPEC-018 §7.4) |
| 45 | `METARULE_GET_WORLDMAP_YPOS` | ignored | I the party's world-map Y position |
| 46 | `METARULE_CURRENT_TOWN` | ignored | I the current area (SPEC-018 §7.4; SPEC-019's current city), or -1 for none |
| 47 | `METARULE_LANGUAGE_FILTER` | ignored | I the `language_filter` preference, 0 or 1 (SPEC-014 §2.3) |
| 48 | `METARULE_VIOLENCE_FILTER` | ignored | I the `violence_level` preference, 0-3 (SPEC-014 §2.3) |
| 49 | `METARULE_WEAPON_DAMAGE_TYPE` | item | I the weapon prototype's damage type (SPEC-011 numbering). A null or non-weapon item gives -1 with a diagnostic |
| 50 | `METARULE_CRITTER_BARTERS` | critter | I 1 when the critter's prototype has critter flag `0x00000002`, "barter enabled" (SPEC-003 §2.4), else 0. Null or non-critter gives 0 |
| 51 | `METARULE_CRITTER_KILL_TYPE` | critter | I the prototype's kill category (SPEC-003 §2.4, offset `0x198`; under Fallout 2 numbered 0-18 by SPEC-018 §7.3). Null or non-critter gives -1 with a diagnostic |
| any other | — | ignored | I 0, as in SPEC-005. No diagnostic is needed |

- Rules 13-16 and 22 are `[Must-match]` exactly as SPEC-005.
- The rule numbers and results of 17-51 are `[Must-match]`.
- The null and out-of-range results of 17-51 are `[Safe rule]`.
- Under `fallout1`, rules 17-51 are "any other" and give I 0. `[Must-match]`

### 5.2 `metarule3` (`0x80E1` under Fallout 2)

Public call: `metarule3(sub-function, p1, p2, p3)`. Pop order: p3 U, p2 U,
p1 U, sub-function. Push I value. `[Must-match]`

- **Typing.** The sub-function must be I; any other tag is a VM error. The
  three parameters are popped without a tag check. Each sub-function reads
  the parameters it uses: as integers when their masked tag is I, and p1 of
  sub-function 102 as text when it is S. When a parameter that a
  sub-function uses has another tag, nothing happens, with a diagnostic, and
  the result is I 0. Unused parameters are ignored whatever their tag.
  `[Safe rule]`
- **Unknown sub-functions** pop all four operands, do nothing and push I 0,
  with a diagnostic. `[Safe rule]`
- **The table.** The sub-function numbers, their operand meanings and their
  normal results below are `[Must-match]`. The null, invalid and
  out-of-range results are `[Safe rule]`.

| Sub | Public name | p1 | p2 | p3 | Effect and result |
| ---: | --- | --- | --- | --- | --- |
| 100 | `METARULE3_CLR_FIXED_TIMED_EVENTS` | object | fixed parameter | unused | Remove every queued script-timer event (queue type 3, SPEC-006 §3.7) whose script instance is the object's script and whose fixed parameter equals p2. Events of other types and timers with other parameters stay. I 0. A null object, or one without a script, removes nothing. |
| 101 | `METARULE3_MARK_SUBTILE` | world X | world Y | radius | Find the world-map subtile that holds world pixel (X, Y) (SPEC-018 §5.3). Every subtile whose column and row each differ from that subtile's by at most the radius rises to state 1 (known) when it is state 0. A subtile is never lowered. I 0. A point outside the world, or a negative radius, changes nothing. |
| 102 | `METARULE3_SET_WM_MUSIC` | track name S | unused | unused | The world-map screen's music track (default `03wrldmp`, SPEC-018 §5.2) becomes the name, cut to 15 bytes. When the world map is open, the track restarts at once. The override lasts until the next full game reset, as in section 4.4. An empty name restores the default. I 0. |
| 103 | `METARULE3_GET_KILL_COUNT` | kill category | unused | unused | I the player's kill count for that category: the counts that SPEC-019 §3.9's Kills folder shows, over SPEC-018 §7.3's categories 0-18. A category outside 0-18 gives 0. |
| 104 | `METARULE3_MARK_MAP_ENTRANCE` | map number | state | elevation | Every entrance slot that SPEC-018 §5.8 "Entrances by map" selects for the map and elevation becomes known when the state is nonzero, and unknown when it is 0. Elevation -1 selects every elevation. I 0 when the map has a `maps.txt` record, else I -1. |
| 105 | `METARULE3_WM_SUBTILE_STATE` | world X | world Y | unused | I the discovery value, 0 unknown, 1 seen or 2 visited (SPEC-018 §5.5), of the subtile that holds world pixel (X, Y). A point outside the world gives -1. |
| 106 | `METARULE3_TILE_GET_NEXT_CRITTER` | hex | elevation | previous critter | Consider every critter-category object placed at that hex and elevation, in the map's object order for that elevation (SPEC-003), including the player and dead or hidden critters. With p3 null, return the first one. Otherwise return the first one after p3. Return I 0 when there is none, when p3 is not among them, or when the hex is outside 0-39999 or the elevation outside 0-2. |
| 107 | `METARULE3_ART_SET_BASE_FID_NUM` | object | art identifier or base index | unused | Let B be the low 12 bits of p2 (SPEC-007 §2.1). For the player, set the default player art (SPEC-005 §3.8) to base B; if no armor is worn, replace the base field of the player's art at once, keeping every other field, with no animation. For any other object, replace the base field of its art at once, keeping every other field. Redraw either way. I 0. A null object changes nothing. |
| 108 | `METARULE3_TILE_SET_CENTER` | hex | unused | unused | Center the view on the hex, on the current elevation, as the player recenter does. Scroll limits apply (SPEC-014 §3.29). I 0. A hex outside 0-39999 changes nothing. |
| 109 | `METARULE3_CHEM_USE_LEVEL` | critter | unused | unused | I the `chem_use` field of the critter's control record (SPEC-019 §2.5), as its index in SPEC-018 §4.5's vocabulary. A null critter, or one without a control record, gives -1. Packets' own `chem_use` keys are not read in this revision (SPEC-018 §4.7). |
| 110 | `METARULE3_CAR_OUT_OF_FUEL` | unused | unused | unused | I 1 when `has_car` is 1 and `car_fuel` is 0 (SPEC-019 §3.19.1), else 0. |
| 111 | `METARULE3_MAP_GET_LOAD_AREA` | unused | unused | unused | I the area of the current map (SPEC-018 §5.2). While a map loads, it gives the area of the map being loaded. A map without an area gives -1. |

## 6. What SPEC-018 and SPEC-019 Provide

The words above read or write named world state. The three Fallout 2 specs
were reconciled on 2026-10-25 (Decisions 25-30), and each item below names
the section that now provides it. The engine implements each word against a
host interface for that state, and the tests drive that interface with mock
values (section 7). Changing an owner spec never changes a word's stack
contract.

**From SPEC-018 (mounting, saves, data tables, new-game flow):**

1. **Areas** (SPEC-019's cities, SPEC-018 §4.3): area numbers; each area's
   current world position, written by `wm_area_set_pos` (SPEC-018 §5.8);
   its known state, 0-2 (`mark_area_known` type 0, `metarule` 17); the
   current area (`metarule` 46). All saved in SPEC-018 §7.4.
2. **Maps** (SPEC-018 §4.2, §5.1-§5.2): map numbers and file names
   (`load_map`, `cur_map_index`); each map's area (`metarule3` 111); its
   music track (`set_map_music`); its known and visited flags
   (`mark_area_known` type 1, `metarule` 19), saved in SPEC-018 §7.4; its
   entrances by map and elevation (`metarule3` 104, SPEC-018 §5.8).
3. **World grid** (SPEC-018 §5.3, §5.5): the world size, 50-pixel subtiles,
   the pixel-to-subtile mapping, the discovery values 0-2 (`metarule3` 101
   and 105); the party's world position (`metarule` 44 and 45, SPEC-018
   §7.4); the world-map music `03wrldmp` (`metarule3` 102, SPEC-018 §5.2).
4. **Campaign variables:** the `data/vault13.gam` array (SPEC-018 §6.2).
   `load_map`'s entrance parameter is written to **no** element (SPEC-018
   §5.1).
5. **Kill categories:** 0-18, with 19 saved counts (SPEC-018 §7.3).
6. **AI packets:** the packets' Fallout 2 control keys are not read
   (SPEC-018 §4.7). `metarule3` 109 reads the control record instead.
7. **Progression:** SPEC-011's level and perk tables, provisionally
   (SPEC-018 §4.7); the elevator table (SPEC-018 §4.9); the start date and
   tick (SPEC-018 §6.1).
8. **Party-member table** (SPEC-018 §4.5), and the saved party state
   (SPEC-018 §7.5).

**From SPEC-019 (interface, party control, car):**

1. **Car state** (SPEC-019 §2.4, §3.19.1): `has_car`, `car_area`, `car_map`
   and `car_fuel`, maximum 80000 (`metarule` 30-32, `metarule3` 110).
2. **Party control:** the Push action that requests `push_p_proc` (SPEC-019
   §3.6). `combat_is_starting_p_proc` and `combat_is_over_p_proc` are never
   requested: SPEC-019 defines no path for them.
3. **Screens:** the world-map and town-map screens for `world_map` and
   `town_map` (SPEC-019 §3.12, §3.13); the city markers that
   `mark_area_known` affects.
4. **Media:** the movie flags (SPEC-019 §3.21) and the ending (SPEC-019
   §3.20).
5. **Preferences:** `language_filter` and `violence_level` keep SPEC-014
   §2.3's keys and ranges (SPEC-019 §3.11; `metarule` 47 and 48).

### 6.1 Answers to SPEC-019 §6 ("Coordination With SPEC-017 and SPEC-018")

An **area** in this spec is SPEC-019's **city**: one `[Area NN]` record of
SPEC-018's `city.txt` (SPEC-018 §4.3).

1. **Party counts.** `metarule` rule 16 returns all entries, the player
   included (SPEC-019 §3.14), unchanged from SPEC-005. No word returns the
   visible-entry count. SPEC-019 uses that count only in its own rules.
   `[Safe rule]`
2. **The car.**
   - `metarule` 31 gives the car to the party, and rule 32 gives fuel, with
     a signed amount.
   - Rule 30 returns the car's current town.
   - `metarule3` 110 is the out-of-fuel test.
   - There is no carry amount: SPEC-019 has no such state (Decision 26).
3. **The world map.**
   - `mark_area_known` type 0 sets a city's known and visited states.
   - `metarule3` 104 sets a city entrance's state, and is what scripts use in
     place of Fallout 1's destroyed-area rows.
   - `metarule` rules 44 and 45 read the world position, and rule 46 the
     current city.
4. **`push_p_proc`** is host event 24 (section 2.4). Like every lifecycle
   name, it has no fixed procedure-table index: the host finds it by name,
   case-insensitively (SPEC-004 §3.7). The other added names are events
   25-27.
5. **Control records and dispositions.** No word writes a party member's
   control-record fields or disposition. `metarule3` 109 reads the record's
   `chem_use`, as SPEC-018 §4.5's vocabulary index, which is SPEC-019 §2.5's
   panel position.

## 7. Synthetic Test Fixtures (Hex / Text)

These are bytecode **fragments**, run as SPEC-005 §4 runs its fixtures:

- start with empty stacks, enter at byte 0 and stop at the end;
- the harness supplies the script context, the string storage and mock
  hosts for the section 6 state;
- run under the `fallout2` profile unless a vector says otherwise.

Words and payloads are big-endian. A typed push is a two-byte tag followed by
a four-byte payload. Stack bytes put the payload first (SPEC-004 §2.5).

### 7.1 Vector A: word sets (load-time)

Build minimal programs with the `vault-fixtures` builders. Each contains one
listed word in its initial stream.

| Word | Under `fallout1` | Under `fallout2` | Enhanced preset |
| --- | --- | --- | --- |
| `0x8155` | Loads | Loads | Same as without |
| `0x8156` | Refused at load | Loads | Same as without |
| `0x8183` | Refused at load | Loads | Same as without |
| `0x8184` | Refused at load | Refused at load | Still refused |
| `0x81FF` | Refused at load | Refused at load | Still refused |
| `0x8200` | Refused at load | Refused at load | Loads under either profile |

### 7.2 Vector B: a reserved word fails closed

```text
0000: C0 01 00 00 00 05 81 5A
```

The push runs. `815A` then raises the undefined-instruction VM error without
popping, so the value stack still holds I 5 (`00 00 00 05 C0 01`). The low
status bits are ended and error, `0x0005`. Under `fallout1` the fragment is
refused at load instead. Repeat for `8183`: same result.

### 7.3 Vector C: the profile decides `0x80E1`

```text
0000: C0 01 00 00 00 00 C0 01 00 00 00 00 C0 01 00 00
0010: 00 00 C0 01 00 00 00 00 80 E1
```

- Under `fallout2`, `80E1` is `metarule3` with sub-function 0, an unknown
  sub-function. It pops four values and pushes I 0, with a diagnostic. Final
  stack: `00 00 00 00 C0 01`.
- Under `fallout1`, `80E1` is `turn_off_objs_in_area` over elevations 0-0
  and hexes 0-0, with SPEC-005's terminating safe rule. The mock map has no
  object at hex 0, so nothing changes. Final stack: empty.

### 7.4 Vector D: `metarule3` 100 and an unknown sub-function

Mock world:

- self is reference `0x00010000`, a critter with script instance 7;
- the timed queue holds, in due order:
  - script timer (type 3), instance 7, fixed parameter 1;
  - script timer, instance 7, fixed parameter 2;
  - drug stage (type 0) owned by self;
  - script timer, instance 7, fixed parameter 1;
  - script timer, instance 9, fixed parameter 1.

```text
0000: C0 01 00 00 00 64 80 BC C0 01 00 00 00 01 C0 01
0010: 00 00 00 00 80 E1 C0 01 00 00 00 63 C0 01 00 00
0020: 00 07 C0 01 00 00 00 08 C0 01 00 00 00 09 80 E1
```

| Executed word | Pops, first to last | Effect | Stack afterward, bottom to top |
| --- | --- | --- | --- |
| `80BC` | none | I self | I 100, I self |
| First `80E1` | I 0, I 1, I self, I 100 | Both instance-7 timers with parameter 1 are removed. The parameter-2 timer, the drug event and the instance-9 timer stay, in order. | I 0 |
| Second `80E1` | I 9, I 8, I 7, I 99 | Unknown sub-function 99: nothing happens, with a diagnostic | I 0, I 0 |

Final stack bytes: `00 00 00 00 C0 01 00 00 00 00 C0 01`.

### 7.5 Vector E: `metarule3` 106 walks a hex

Mock map, elevation 0, hex 12345 (`0x3039`), in object order:

- item `0x00050000`;
- critter A `0x00010000`;
- critter B `0x00020000`.

Elevation 1, hex 12345, holds critter C `0x00060000`.

```text
0000: C0 01 00 00 00 6A C0 01 00 00 30 39 C0 01 00 00
0010: 00 00 C0 01 00 00 00 6A C0 01 00 00 30 39 C0 01
0020: 00 00 00 00 C0 01 00 00 00 6A C0 01 00 00 30 39
0030: C0 01 00 00 00 00 C0 01 00 00 00 00 80 E1 80 E1
0040: 80 E1
```

The ten pushes build three calls whose previous-critter operand is the
result of the call above it:

| Call | Pops | Result |
| --- | --- | --- |
| First `80E1` | I 0 (previous), I 0 (elevation), I 12345, I 106 | A |
| Second | A, I 0, I 12345, I 106 | B |
| Third | B, I 0, I 12345, I 106 | I 0, because there is no critter after B. The item is never returned, and C is on another elevation. |

Final stack bytes: `00 00 00 00 C0 01`.

### 7.6 Vector F: `metarule3` 107 sets the art base

Self is a non-player critter with art `0x01004003`.

```text
0000: C0 01 00 00 00 6B 80 BC C0 01 01 00 00 05 C0 01
0010: 00 00 00 00 80 E1
```

`80E1` pops I 0, I `0x01000005`, I self and I 107, and pushes I 0. Self's art
becomes `0x01004005`: base 5, with weapon code 4 kept.

Player variant: replace self with the player, wearing no armor, with art
`0x0100400B`. The default player art becomes base 5 and the art
`0x01004005`. With armor worn, the default player art becomes base 5 but the
art is unchanged.

### 7.7 Vector G: `tile_in_tile_rect`

The corners are 2020 (column 20, row 10), 2010, 4020 and 4010. The test
hexes are 3015 (column 15, row 15, inside) and 3025 (column 25, outside).

```text
0000: C0 01 00 00 07 E4 C0 01 00 00 07 DA C0 01 00 00
0010: 0F B4 C0 01 00 00 0F AA C0 01 00 00 0B C7 80 CF
0020: C0 01 00 00 07 E4 C0 01 00 00 07 DA C0 01 00 00
0030: 0F B4 C0 01 00 00 0F AA C0 01 00 00 0B D1 80 CF
```

- Each `80CF` pops five values: the test hex first, the upper-left corner
  last.
- Final stack bytes: `00 00 00 01 C0 01 00 00 00 00 C0 01`.
- Mutation: replace the first fragment's test hex with `00 00 9C 40`
  (40000). That call then gives 0.
- Under `fallout1`, `80CF` is `animate_jump`, which pops nothing. The same
  fragment then leaves all ten pushed values on the stack.

### 7.8 Vector H: music, area position and area knowledge

Mock world:

- static string offset 6 is `testtrk`;
- the current map is 3;
- areas 0-9 and maps 0-9 exist, all unknown;
- area 2 is at (100, 100).

```text
0000: C0 01 00 00 00 03 90 01 00 00 00 06 80 E2 C0 01
0010: 00 00 00 02 C0 01 00 00 01 5E C0 01 00 00 02 58
0020: 80 E5 C0 01 00 00 00 00 C0 01 00 00 00 04 C0 01
0030: 00 00 00 01 80 B2 C0 01 00 00 00 01 C0 01 00 00
0040: 00 07 C0 01 00 00 00 02 80 B2 C0 01 00 00 00 02
0050: C0 01 00 00 00 00 C0 01 00 00 00 01 80 B2
```

| Executed word | Pops, first to last | Effect |
| --- | --- | --- |
| `80E2` | S `testtrk`, I 3 | Map 3's track becomes `testtrk`. It is the current map, so the background track restarts as `testtrk`, looping |
| `80E5` | I 600, I 350, I 2 | Area 2 is now at (350, 600) |
| First `80B2` | I 1, I 4, I 0 | Area 4 is known |
| Second `80B2` | I 2, I 7, I 1 | Map 7 is known and visited |
| Third `80B2` | I 1, I 0, I 2 | Mark type 2 is unknown: nothing changes, with a diagnostic |

Both stacks finish empty.

### 7.9 Vector I: the Fallout 2 `metarule` rules

This vector continues from Vector H's world.

```text
0000: C0 01 00 00 00 11 C0 01 00 00 00 04 81 0B C0 01
0010: 00 00 00 13 C0 01 00 00 00 07 81 0B C0 01 00 00
0020: 00 11 C0 01 00 00 00 05 81 0B C0 01 00 00 00 63
0030: C0 01 00 00 00 00 81 0B
```

| Call | Result |
| --- | --- |
| Rule 17, area 4 | I 1 |
| Rule 19, map 7 | I 1 |
| Rule 17, area 5 | I 0 |
| Rule 99 | I 0 |

- Under `fallout2` the final stack is I 1, I 1, I 0, I 0.
- Under `fallout1` it is four I 0 values: rules 17 and 19 are unknown there.

### 7.10 Vector J: `set_exit_grids`

Mock map:

- elevation 0 holds exit grids G1 (PID `0x05000010`, hidden) and G2 (PID
  `0x05000011`), each with destination map 9, hex 100, elevation 0,
  orientation 0;
- elevation 1 holds exit grid G3 with the same destination.

```text
0000: C0 01 00 00 00 00 C0 01 00 00 00 05 C0 01 00 00
0010: 00 01 C0 01 00 00 4E 84 C0 01 00 00 00 02 80 E6
```

- `80E6` pops I 2, I 20100, I 1, I 5 and I 0.
- G1 and G2 now lead to map 5, hex 20100, elevation 1, orientation 2. G3 is
  unchanged, and no art changes.
- Both stacks finish empty.

### 7.11 Vector K: `inven_cmds`

Self `0x00010000` holds two entries, in order: a stack of three, represented
by `0x00030000`, then `0x00040000`.

```text
0000: 80 BC C0 01 00 00 00 0D C0 01 00 00 00 01 81 56
0010: 80 BC C0 01 00 00 00 0D C0 01 00 00 00 02 81 56
0020: 80 BC C0 01 00 00 00 0C C0 01 00 00 00 00 81 56
```

| Call | Result |
| --- | --- |
| Command 13, index 1 | `0x00040000` |
| Command 13, index 2 | I 0: there are only two entries |
| Command 12, index 0 | I 0, with a diagnostic |

Final stack bytes: `00 04 00 00 C0 01 00 00 00 00 C0 01 00 00 00 00 C0 01`.

### 7.12 Vector L: the three registration words

The mock is outside combat, and self is a live critter facing 0. Every art
loads.

```text
0000: C0 01 00 00 00 01 C0 01 00 00 00 01 81 0E 80 BC
0010: C0 01 01 00 00 05 C0 01 00 00 00 00 81 57 80 BC
0020: C0 01 00 00 30 39 C0 01 FF FF FF FF 81 59 80 BC
0030: C0 01 00 00 00 04 C0 01 FF FF FF FF 81 58 C0 01
0040: 00 00 00 03 C0 01 00 00 00 00 81 0E
```

| Executed word | Pops, first to last | Registered entry |
| --- | --- | --- |
| `810E` | I 1, I 1 | Begin an unreserved sequence |
| `8157` | I 0, I `0x01000005`, I self | Set art `0x01000005`, delay 0 |
| `8159` | I -1, I 12345, I self | Turn toward hex 12345, delay -1 |
| `8158` | I -1, I 4, I self | Take out weapon code 4, delay -1 |
| `810E` | I 0, I 3 | End: the sequence holds the three entries, in this order |

- Both stacks finish empty.
- In combat, the same fragment consumes every operand and registers nothing.
- With `8158`'s weapon code changed to 0 (bytes `0x30`-`0x35` becoming
  `C0 01 00 00 00 00`), only the set-art and turn entries are registered,
  with one diagnostic.

### 7.13 Vector M: lifecycle names

Build a synthetic program whose procedure table has `start` and
`Push_P_Proc` (mixed case on purpose).

- A host request for event 24 finds `Push_P_Proc` case-insensitively and
  runs it.
- During that request, `script_action` (`80C7`) pushes I 24.
- Under `fallout1`, the host never requests event 24.

### 7.14 Vector P: sfall mode routes the overlap (Revision 4)

**Load time.** Build minimal programs as in Vector A. Vector A itself is
unchanged: its columns are sfall mode off. All columns here are `fallout2`.

| Word | Off | Off, enhanced preset | On | On, enhanced preset |
| --- | --- | --- | --- | --- |
| `0x8155` | Loads | Loads | Loads | Loads |
| `0x8156` | Loads | Loads | Loads | Loads |
| `0x8183` | Loads | Loads | Loads | Loads |
| `0x8184` | Refused at load | Refused at load | Loads | Loads |
| `0x81FF` | Refused at load | Refused at load | Loads | Loads |
| `0x8200` | Refused at load | Loads | Loads | Loads |
| `0x8213` | Refused at load | Loads | Loads | Loads |
| `0x8281` | Refused at load | As today | Loads | Loads |
| `0x8282` | Refused at load | As today | Refused at load | As today |

"As today" means the result an engine without Revision 4 gives for that word.
Neither word is a defined enhanced word, so this vector does not state it, but
the two "As today" cells of `0x8282` must agree. A session that asks for sfall
mode under `fallout1` is refused when it is built, so no `fallout1` column exists.

**Execution.** Each fragment runs under the columns named. Stack bytes put the
payload first, as in section 7.

Fragment 1, the `0x8156` of Vector K:

```text
0000: 80 BC C0 01 00 00 00 0D C0 01 00 00 00 01 81 56
```

- Off: `inven_cmds` pops I 1, I 13 and self. With Vector K's mock world the
  final stack bytes are `00 04 00 00 C0 01`.
- On: `read_byte` is tier X (SPEC-024b section 4.10). It pops I 1 and pushes I 0,
  so the final stack is self, I 13, I 0, with self `0x00010000`:
  `00 01 00 00 C0 01 00 00 00 0D C0 01 00 00 00 00 C0 01`.

Fragment 2, `0x8200`:

```text
0000: C0 01 00 00 00 07 82 00
```

- Off, enhanced preset: `engine_api_version` pops nothing and pushes the API
  version, 2 today, so the final stack is I 7, I 2:
  `00 00 00 07 C0 01 00 00 00 02 C0 01`.
- On, with or without the enhanced preset: `fs_size` is tier P2. It pops I 7 and
  pushes I 0, with a diagnostic. Final stack bytes: `00 00 00 00 C0 01`.

Fragment 3, `0x8210`:

```text
0000: C0 01 00 00 00 64 C0 01 00 00 00 00 82 10
```

- Off, enhanced preset: `op_cam_pan_to` pops both operands (hex 100, duration 0)
  and pushes nothing. The stack finishes empty.
- On, with or without the enhanced preset: `sfall_ver_major` pops nothing and
  pushes I 4, so the final stack is I 100, I 0, I 4:
  `00 00 00 64 C0 01 00 00 00 00 C0 01 00 00 00 04 C0 01`.

## 8. Corpus Audit and the Fallout 2 Gate

Every word number in sections 3.2 and 4 comes from Team A's knowledge.
This section defines the check against the user's own Fallout 2 scripts. It
reports counts and signatures only, never script bytes, like the existing
hash-only audits (`PROVENANCE.md`).

### 8.1 What the audit reads

The audit reads every `.INT` that `scripts/scripts.lst` names in the mounted
Fallout 2 databases (SPEC-018 §1). Each is decoded by SPEC-004's layout. A
word outside the `FALLOUT2` set is a **refused-word** finding.

### 8.2 The stack model

Each program is analysed in units:

- its initial stream (SPEC-004 §2.2), starting at depth 0;
- each distinct procedure body address, starting at depth `declared argument
  count + 1`: the arguments plus the count that `0x802B` pops.

Within a unit, abstract execution tracks the value-stack depth, and for each
slot one of these:

- a **known constant**: a slot-1 push whose tag masks to `0xC001`;
- a **host result**: the value a host word's row pushes (rows with push 1,
  in the host-word row of the table below);
- **unknown**: everything else, including a procedure call's result
  (`0x8005`) and every core word's result.

Duplicate (`0x801B`) and swap (`0x8018`) move slots unchanged, marks
included. Successor addresses are explored by a worklist. (Revision 2)

| Words | Effect on the model |
| --- | --- |
| Slot-1 pushes | +1 |
| `0x8000`, `0x8002`, `0x8003`, `0x8019`, `0x801E`, `0x8029`, `0x802C`, `0x8049`, `0x804A`, `0x804B`, `0x800F` | 0 |
| `0x800C`, `0x8012`, `0x8014`, `0x8028`, `0x802D`, `0x8032`, `0x8043`-`0x8046` | Pop as listed in SPEC-004 §3.2-3.5 and push 1: `0x800C` pops none; the others pop 1 |
| `0x800D`, `0x801A`, `0x8016`, `0x8047`, `0x8048`, `0x8008`, `0x800A`, `0x800B` | Pop 1 |
| `0x8013`, `0x8015`, `0x8017`, `0x8027`, `0x8031`, `0x8006`, `0x8007` | Pop 2 |
| `0x8018` | Pop 2, push 2 (swapped) |
| `0x801B` | Pop 1, push 2 |
| `0x8033`-`0x8042` | Pop 2, push 1 |
| `0x801F` | Pop 3 |
| `0x802B` | Pop 1. The first one in a body sets the depth to the declared argument count. Any later one is an **unexpected-frame** finding |
| `0x802A` | Depth becomes 0 relative to the frame base |
| `0x8004` | Pop the target. Continue only at the target. A target equal to a procedure body address ends the path |
| `0x802F` | Pop the condition, then the target. Continue at the next instruction and at the target, both at the new depth |
| `0x8030` | Continue at the next instruction at depth - 1 (the target stays), and at the target at depth - 2 |
| `0x8005` | The top is the procedure index and, below it, the argument count c, which must be known. Pop both and c more values, push 1 (the procedure's result, unknown), and continue at the next instruction. The index need not be known: an index computed by a name lookup (`0x8028`) does not change the call's stack effect (Revision 2) |
| `0x802E` | Ends the path, recorded as **data-dependent** |
| `0x8009` | Pop 1 and end the path |
| `0x800E`, `0x8010`, `0x8011`, `0x801C`, `0x801D`, `0x8020`-`0x8026` | End the path |
| Host words | Pop and push by the active profile's table: the SPEC-005 rows, as amended by sections 3 and 4. A row that sometimes omits its push (for example `critter_attempt_placement` on null) counts its normal-path push. `addregion` (`0x807F`) pops a known count n and then n more values. Each pushed value is a host result. Reserved and unregistered words are a **reserved-word** finding and end the path |

**Leaked results.** The compiler emits a host word called as a statement
without discarding its result, so the value stays on the stack until the
procedure's frame is cleared (`0x802A`). A procedure call's result is
discarded explicitly (`0x801A`). An address can therefore be reached with
host results left over on one path and not on another. This is how the
corpus is built, not a defect, and the model accepts it. (Revision 2,
Decision 31)

When a path arrives at an address that already has a recorded state:

1. Let D be the deeper of the two states and S the shallower, with
   k = depth(D) - depth(S). (When the depths are equal, D is the recorded
   state and k = 0.)
2. If D holds fewer than k host results, the arrival is a **merge
   conflict**. The recorded state is kept, and this arrival ends.
3. Otherwise remove D's k lowest host results, that is, those nearest the
   bottom of the stack. A leaked result lies below everything the current
   statement has pushed since. What remains has S's depth.
4. Meet the remainder with S slot by slot, from the bottom. Two equal slots
   keep their value. Any other pair becomes unknown.
5. If the met state differs from the recorded state, it replaces it, and the
   address is explored again from it. Otherwise this arrival ends.

This terminates: a state only ever gets shallower, or a slot only ever
becomes unknown. A unit in which step 3 removed at least one result is
counted as a **unit with leaked results**. That count is reported, but it is
not a finding.

**Findings.** Each of these is reported:

- **Underflow:** the depth falls below 0 before a word's pushes.
- **Overflow:** the depth rises above 341.
- **Merge conflict:** step 2 above.
- **Unresolved:** a jump operand, or a call's argument count, is not a known
  constant, or a jump target is not an instruction boundary of the linear
  decode. This ends the path.
- The refused-word, reserved-word, unexpected-frame and data-dependent
  findings above.

A unit reports each finding kind at most once per instruction address, however
often re-exploration reaches it. A merge conflict is reported at the address
of the arrival. (Revision 2)

### 8.3 Signature inference

Take each host word W in `0x80A1..=0x8183` that occurs in at least one unit
with a finding. Re-analyse every unit containing W once for each candidate
signature: pops 0-8, push 0 or 1, with every other word unchanged. Report
the candidates under which none of those units has a finding. A recorded
exception (section 8.4) does not make a unit count as one with a finding.

A host result that is never consumed is accepted as a leak, so the corpus
cannot tell push 0 from push 1 for a word whose result is never used. Both
candidates are then consistent, and the report shows both. (Revision 2,
Decision 34)

The output per word is:

- its occurrence count;
- its table signature;
- the consistent candidates.

Operand-type evidence is reported too: for each operand position of W, how
often a known constant pushed directly for it was a string push (`0x9001`).
This tells, for example, whether `0x80E2`'s top operand really is text.

### 8.4 The model check

Before any Fallout 2 result is trusted, run the same audit with the
`fallout1` table over the user's Fallout 1 corpus. It must report zero
findings, apart from the recorded exceptions below. If it does not, the
model of section 8.2 is wrong, not the host table: report the findings for a
revision of this section.

**Recorded exceptions.** An exception is a finding in game data that the
model correctly reports. It is a defect in that script, not in the model or
the table, and a spec revision records each one with its reason. An exception
is matched exactly by:

- the profile table;
- the script's name, lowercase, as `scripts/scripts.lst` lists it;
- the finding kind;
- the word;
- the count.

A matching finding is reported on its own line as a recorded exception, and
is not counted among the findings. If the observed count differs from the
recorded count, or the script is not in the mount, that is a **stale
exception**. A stale exception counts as one finding, and its recorded
findings count as findings. `[Safe rule]` (Revision 2, Decision 33)

| Table | Script | Finding | Word | Count | Reason |
| --- | --- | --- | --- | --- | --- |
| `fallout1` | `gatedemo.int` | underflow | `0x80DE` | 1 | Its one call to `start_gdialog` pushes four operands, not five. The bottommost operand is the unused list number of SPEC-005 §3. The other 430 occurrences in the Fallout 1 corpus are consistent with five pops. |

The `fallout2` table has no recorded exception in this revision.

**The retail Fallout 2 audit (Revision 3, 2026-10-25).** This is the audit the
gate of section 8.5 asks for, run over the owner's retail install with the
`fallout2` table of Revision 3:

- 1248 scripts analysed, 40 listed but absent, 0 undecodable, 29956 units,
  0 empty procedure bodies, 74 units with leaked results.
- **0 findings** of every kind, and no recorded exception.
- Before Revision 3's `0x8109` row, it gave 3 merge conflicts. All three
  were in the units that hold `0x8109`'s 4 occurrences, in two scripts.
  Inference found two consistent candidates, pops 2 with push 0 and pops 3
  with push 1.
- Every one of those call sites pushes an object, the constant 13 and a
  value. Pops 2 is consistent only because the object, a host result, would
  then be absorbed as a leak. Pops 3 with push 1 is the reading that uses
  every pushed operand (Decision 37).
- The Fallout 1 model check of the same run passed: 0 findings, with the one
  recorded exception of `gatedemo.int`.

The per-word summary that section 8.5 asks to be recorded:

| Word | Occurrences | Table | Consistent candidates | Operand evidence |
| --- | ---: | --- | --- | --- |
| `0x80B2` `mark_area_known` | 124 | pops 3, push 0 | pops 3 with push 0 or 1 | all three operands are integer constants in every call |
| `0x80CF` `tile_in_tile_rect` | 1053 | pops 5, push 1 | pops 4, push 0; pops 5, push 1 | the four rectangle hexes are integer constants in every call |
| `0x80E1` `metarule3` | 1188 | pops 4, push 1 | pops 3, push 0; pops 4, push 1 | the sub-function is an integer constant in every call |
| `0x80E2` `set_map_music` | 0 | pops 2, push 0 | not inferred | — |
| `0x80E5` `wm_area_set_pos` | 96 | pops 3, push 0 | pops 3 with push 0 or 1; pops 4, push 1 | — |
| `0x80E6` `set_exit_grids` | 104 | pops 5, push 0 | pops 5 with push 0 or 1; pops 6, push 1 | integer constants; destination map -2 (the world map) is common |
| `0x8156`-`0x8159` | 0 each | as section 4 | not inferred | — |
| `0x815A`-`0x8183` (reserved) | 0 | — | — | — |
| Stubs `0x80AD`, `0x80B3`, `0x80B5`, `0x80D1` | 0 each | as SPEC-005 | — | — |

Each table signature is among its consistent candidates, so the audit
confirms sections 3.2 and 4 as far as stack shape can.

- **Words in use.** No retail script uses a word above `0x8155`. Section 4's
  four words and the reserved range are never executed by the retail game.
- **Selectors in use.**
  - `metarule3` sub-functions: 100, 101, 105, 106, 108, 109, 110 and 111,
    all inside section 5.2's range.
  - `metarule` rules: 13 to 19, 22, 30, 31, 40, 42 to 49, and 51 to 53.
  - Rules 52 and 53 (2 calls each) are not defined, so they take section
    5.1's "any other" row (I 0). (Decision 39)

**What the audit does not check.** It checks the stack shape of every word,
not what a word does. Section 3.2 and 4 host effects, section 5's rules and
sub-functions, and the SPEC-005 rows that Fallout 2 keeps are Team A's or
SPEC-005's statements. They stay `[Must-match]` as written, and are
confirmed by play, not by this audit. (Decision 38)

**Result of Revision 2.** With this model and the exception above, the
Fallout 1 corpus gives zero findings. The coordinator measured this on
2026-10-25 against a prototype of the model (884 scripts, 16148 units). The
report also prints a final line, `Script stack model check: passed` or
`Script stack model check: failed`, when it runs the `fallout1` table.

### 8.5 The gate

Fallout 2 **script execution** stays gated until both hold:

1. the Fallout 2 audit reports zero findings;
2. its per-word summary (occurrence counts and consistent candidates, with no
   script bytes) has been recorded in a revision of this spec, which confirms
   or corrects sections 3.2 and 4.

The gate is part of the specification, not a setting: no configuration key,
command-line flag or preset opens it. A spec revision that records the audit
opens it. In the first two revisions it was closed. **Revision 3 records the
audit (section 8.4) and opens it.** `[Safe rule]` (Decisions 28, 36)

With the gate open, Fallout 2 Play loads every `.INT` with the `FALLOUT2`
word set and runs it through the Fallout 2 host table, as map preview and the
headless harness already did. Section 8.7's rules 2 to 7 no longer apply.
Under sfall mode it uses `FALLOUT2_SFALL` and SPEC-024b's table instead. The
audit itself keeps the `fallout2` table whatever the setting. (Decision 46)

Only script execution is gated. Play is not: section 8.7 says what a Play
session does while the gate is closed. Map preview, the VM test harness of
section 7 and other headless tests run scripts or words directly, and are not
gated.

### 8.6 Audit vector

Vector N is one procedure body at offset 0, with declared argument count 0,
so the entry depth is 1.

```text
0000: 80 2B C0 01 00 00 00 30 C0 01 00 00 00 01 80 2F
0010: C0 01 00 00 00 00 C0 01 00 00 00 05 C0 01 00 00
0020: 00 01 C0 01 00 00 4E 84 C0 01 00 00 00 02 80 E6
0030: 80 2A 80 29 80 1C
```

- `802B` brings the depth to 0.
- The branch at `0x0E` pops the condition and target `0x30`. Both successors
  start at depth 0.
- The then-arm pushes five values and runs `80E6`.
- With the Fallout 2 table (`set_exit_grids`, pops 5, push 0), it reaches
  `0x30` at depth 0: no finding.
- With the Fallout 1 table (`barter_asking`, pops 3), it reaches `0x30` at
  depth 2: one merge conflict at `0x30`.
- With the Fallout 1 table (`barter_asking` pushes nothing), D holds two
  known constants and no host result, with k = 2, so the leaked-result rule
  cannot absorb them.
- Inference then reports two consistent candidates for `0x80E6`: pops 5 with
  push 0, and pops 5 with push 1. The push-1 candidate's result is never
  consumed, so it is accepted as a leaked result (Decision 34). The
  candidate pops 6, push 1 underflows before its push.

Vector O (Revision 2) is one procedure body at offset 0, with declared
argument count 0, so the entry depth is 1:

```text
0000: 80 2B C0 01 00 00 00 24 C0 01 00 00 00 01 80 2F
0010: C0 01 00 00 00 01 C0 01 00 00 00 02 C0 01 00 00
0020: 00 03 80 B6 C0 01 00 00 00 00 90 01 00 00 00 00
0030: 80 28 80 05 80 1A 80 2A 80 29 80 1C
```

- `802B` brings the depth to 0. The branch at `0x0E` pops the condition and
  the target `0x24`.
- The then-arm pushes three values and runs `80B6` (`move_to`, pops 3,
  push 1) as a statement. It reaches `0x24` holding one host result.
- The other successor reaches `0x24` at depth 0.
- At `0x24` the code pushes the argument count 0 and a static string, looks
  the string up as a procedure (`8028`, so the index is unknown), calls it
  (`8005`), and discards the call's result (`801A`).
- Under the first revision's model, the table `fallout1` gives one merge
  conflict and one unresolved call.
- Under Revision 2's model it gives no finding, and one unit with leaked
  results. That holds whichever successor is explored first.

### 8.7 Play while scripts are gated

While the gate of section 8.5 is closed, a Fallout 2 Play session runs as
follows. SPEC-018 §6.7 and SPEC-019 §3.4 follow this section. `[Safe rule]`
(Decision 28)

1. **Launcher.** Play is available under `game = "fallout2"` once SPEC-018
   §2 accepts the install. The roadmap's Fallout 2 launch refusal no longer
   applies to Play.
2. **No script runs.** No `.INT` file is opened, no program enters the VM
   run list, and the VM's time slice finds an empty list. Every script
   request the host would make is handled exactly as SPEC-006 §3.7 handles a
   record whose script file cannot be loaded: nothing runs, no override is
   set, and the host's default action follows. This covers the map-load
   requests of SPEC-006 §3.5 steps 6, 8 and 9, the critter-script cycle,
   spatial checks, script timers, object actions (SPEC-006 §3.8), combat
   requests, `map_exit_p_proc` and `push_p_proc`. No load is attempted, so
   there is no per-request diagnostic; one diagnostic per session records
   that scripts are gated.
3. **What follows from 2.** Talking to a critter does nothing, because
   dialogue entry refuses a target without a script (SPEC-006 §3.8). Use,
   look, pickup and skills take their default actions. No script calls
   `party_add`, so the party stays the player alone. No `metarule` 15
   elevator request is made.
4. **Script records are kept.** Script instances read from maps, and those
   attached to spawned encounter critters (SPEC-018 §5.7), keep every stored
   field and local variable. They are written to working `.SAV` files and
   save slots unchanged.
5. **Everything else runs.** The new-game hand-off (SPEC-018 §6.6, with its
   movie and music), map loading and rendering, exit grids, movement,
   looking, the interface bar, inventory, the character screen, the Pip-Boy,
   the world map, travel and encounters, combat that the player starts (the
   combat AI is engine code, SPEC-009), and save and load.
6. **Notice.** After each completed new-game hand-off and each successful
   game load, the message monitor prints this line, once:
   `Scripts are off: the Fallout 2 script audit is not recorded yet.`
   It is an engine string, not a message file entry. The launcher shows the
   same line under Play.
7. **Saves** made in such a session carry the gated mark of SPEC-018 §7.2.

When a revision opens the gate, rules 2 to 7 stop applying, and a gated save
no longer loads (SPEC-018 §7.2).

## 9. Decisions

1. **The Fallout 2 word set is `0x8000..=0x8183`, and the enhanced range
   `0x8200..=0x82FF` stays separate** (owner instruction). Revision 4 keeps this
   exactly with sfall mode off. Decision 42 says what sfall mode changes.
2. **SPEC-005 words keep their meaning under Fallout 2 unless this spec
   states a difference with a reason** (owner instruction). Section 3.1
   lists the rows that only change data source, and section 3.2 the six
   reassigned rows, each with its reason.
3. **Every word number here is provisional until the section 8 audit
   confirms it, and Fallout 2 script execution stays gated until then**
   (Team A default; Decision 28 says what Play does meanwhile). Team A's knowledge of Fallout 2's numbering cannot be
   checked against a source here. A wrong number would corrupt the stack
   silently, so the spec fails closed and measures.
4. **Six Fallout 1 slots are reassigned under Fallout 2:** `0x80B2`
   `mark_area_known`, `0x80CF` `tile_in_tile_rect`, `0x80E1` `metarule3`,
   `0x80E2` `set_map_music`, `0x80E5` `wm_area_set_pos` and `0x80E6`
   `set_exit_grids` (Team A default, from Team A's knowledge). Each Fallout 1
   meaning is a stub or a routine that never returns, so no working Fallout 2
   script could depend on the Fallout 1 meaning. Confidence is highest for
   `0x80E1`, `0x80E2`, `0x80E5` and `0x80E6`, lower for `0x80CF`, and lowest
   for `0x80B2`.
5. **`inven_cmds`, `reg_anim_change_fid`, `reg_anim_take_out` and
   `reg_anim_turn_towards` are placed at `0x8156`-`0x8159`** (Team A
   default). Team A is confident that these names and argument orders are
   Fallout 2 script calls, but not of their numbers. They may instead occupy
   Fallout 1 stub slots: `skill_contest` `0x80AD` and `reaction_influence`
   `0x80B3` have three operands and one result, like `inven_cmds`. The
   section 8.3 inference reports which.
6. **The other 42 words of the range are reserved:** accepted at load, a VM
   error when executed (Team A default). Team A knows no further Fallout 2
   word, and will not invent one. If Fallout 2 has fewer than 46 additional
   words, as Team A suspects, these rows stay reserved for good.
7. **Several public Fallout 2 names are header macros over existing words,
   and add no word** (Team A default, section 3.1's second table).
8. **The Fallout 2 `metarule` rules are 17-19, 30-32, 40 and 42-51, and
   every other unknown rule still gives I 0** (Team A default for the rule
   set; the zero rule is SPEC-005's).
9. **`metarule3` checks only its sub-function's tag. Each sub-function
   checks the parameters it uses; a wrong tag there gives a diagnostic and
   I 0, not a VM error** (Team A default). An unknown reference check would
   otherwise end a quest script.
10. **The `metarule3` operand meanings are Team A's knowledge** (Team A
    default). Sub-functions 101 (raise to known, not visited), 102 (one name
    operand), 104 (result -1 for an unknown map) and 109 (position in the
    `chem_use` list) are the least certain.
11. **A nonzero reference that names no live object behaves as null**
    (Team A default, the SPEC-005 §2.1 `[May-improve]` option made the rule).
12. **The `set_map_music` and world-map music overrides are not saved, and
    last until the next full game reset** (Team A default). This keeps
    SPEC-018 §7's save layout unchanged.
13. **`mark_area_known` accepts states 0, 1 and 2 and mark types 0 and 1
    only; anything else is ignored** (Team A default).
14. **`tile_in_tile_rect` uses the column and row bounding box of the four
    corners** (Team A default). This needs no screen orientation, and the
    order of the corners does not matter.
15. **The lifecycle names are 24 `push_p_proc`, 25 `is_dropping_p_proc`, 26
    `combat_is_starting_p_proc` and 27 `combat_is_over_p_proc`. Only
    `push_p_proc` has a requesting path here, through SPEC-019** (Team A
    default).
16. **No VM-core change from SPEC-004** (Team A default).
17. *(Replaced by Decision 25.)* The first revision kept campaign element 32
    for `load_map`'s entrance parameter.
18. **`WEAPON_DAMAGE_TYPE` and `CRITTER_KILL_TYPE` give -1 for an invalid
    object, rather than a value that collides with category 0** (Team A
    default).
19. **`TILE_GET_NEXT_CRITTER` lists every critter, including the player
    and dead or hidden critters, in map object order** (Team A default).
20. **`ART_SET_BASE_FID_NUM` uses only the low 12 bits of its operand. For
    the player it sets the default player art, and shows it only when no
    armor is worn** (Team A default, so armor never visually disappears).
21. **The three new registration words follow SPEC-005 §2.6's combat gate
    and null rule** (Team A default).
22. **`inven_cmds` defines command 13 only** (Team A default). It is the
    only command Team A knows scripts to use.
23. **No word returns the visible party count, reads or sets a car carry
    amount, or writes party control records, in this revision** (Team A
    default, answering SPEC-019 §6; see Decision 26). Team A knows no Fallout 2 word
    for these that it is confident of, and will not invent one.
24. **The audit model of section 8.2, and its Fallout 1 self-check of
    section 8.4** (Team A default). The audit turns the unverifiable
    knowledge into measured counts without moving any game data across the
    wall.

**Reconciliation decisions** (2026-10-25). These resolve conflicts between
SPEC-017, SPEC-018 and SPEC-019. Each is written the same way in every spec
it touches, and SPEC-018 §10 lists them as R1-R6.

25. **`load_map`'s entrance parameter is stored in no campaign global under
    Fallout 2** (Team A default, reconciliation 2026-10-25; SPEC-018 R1).
    Fallout 2's `vault13.gam` gives element 32 another meaning, so writing
    there would silently corrupt a campaign variable. SPEC-018 and SPEC-019
    already switched Fallout 1's indexes 32 and 65 off. A later revision may
    name a global once the section 8 audit gives evidence for one.
26. **No word is added for a visible party count or a car carry amount**
    (Team A default, reconciliation 2026-10-25; SPEC-018 R2). SPEC-019 now
    uses the visible count only internally, and has no carry amount.
27. **`metarule` 15 uses SPEC-018 §4.9's engine-owned elevator table**
    (Team A default, reconciliation 2026-10-25; SPEC-018 R3).
28. **Only script execution is gated, not Play** (Team A default,
    reconciliation 2026-10-25; SPEC-018 R4). Section 8.7 defines a gated Play
    session. The gate is opened only by a spec revision recording the audit.
29. **Areas are SPEC-019's cities; the area, map, car and party state these
    words touch is SPEC-018's** (sections 6 and 6.1), and `metarule3` 109
    reads the control record (Team A default, reconciliation 2026-10-25;
    SPEC-018 R5, R6).
30. **Cross-references point at sections that exist** (Team A default,
    reconciliation 2026-10-25): the level and perk tables are SPEC-011's,
    provisionally; the movie table is SPEC-018 §6.4 with SPEC-019 §3.21's
    flags; the kill categories are 0-18; and `push_p_proc` is event 24 in all
    three specs.

**Revision 4 decisions** (2026-10-30, Team A, on the owner's brief)

42. **Sfall mode routes `0x8156..=0x8281` to SPEC-024b; with it off, nothing
    changes** (owner instruction, approving SPEC-024b Decision D-3). sfall's
    published numbering collides with this spec's Fallout 2 words and with the
    enhanced range. Under the default `fallout1` and `fallout2` profiles every
    word behaves as it did before this revision, so no existing test or script
    breaks. Opt-in sfall mode under `fallout2`, set by the settings file or a mod
    manifest, uses the new set.
43. **The shadowing covers every enhanced word in the overlap** (Team A default,
    applying Decision 42). SPEC-024b's draft named only `0x8200`, but
    `docs/REFERENCE.md` defines the camera words `0x8210`-`0x8213` too. Their
    numbers are in the owner's range, so they route to sfall as well, and
    section 2.2 lists them.
44. **`0x8282..=0x82FF` keeps its meaning under sfall mode** (Team A default,
    from SPEC-024b section 3.2). It is outside the owner's range. Future enhanced
    words belong there, or they will not work under sfall mode.
45. **Sfall mode is fixed when the session is built** (Team A default), for the
    same reason as section 2.3's profile rule: a program must never see a word
    change meaning while it runs.
46. **The retail audit is not repeated for the new set** (Team A default). Section
    8.4's audit found no retail word above `0x8155`, so `FALLOUT2_SFALL` runs
    retail scripts through the same rows. It does not confirm SPEC-024b's words,
    which SPEC-024b Decision D-26 leaves unaudited.

**Revision 3 decisions** (2026-10-25, coordinator, at the owner's request)

36. **The gate is open** (owner instruction, with the audit recorded). The
    retail audit gives zero findings, and its per-word summary is recorded in
    section 8.4, which is what section 8.5 asks for.
37. **`0x8109` pops three operands and pushes I 0, with no effect under
    Fallout 2** (coordinator default, measured). The audit shows that
    Fallout 2 does not use Fallout 1's zero-operand `town_map` there. It does
    not show what the word does. A diagnostic marks each program that
    reaches it, so play can find its four call sites (two scripts). The
    second operand is checked as I, since it is a direct integer constant at
    every site. The other two are unchecked. A later revision that knows the
    word replaces this row.
38. **Stack shape is the audit's whole claim** (coordinator default). The
    host effects stay provisional statements, now exercised by Play.
39. **`metarule` rules 52 and 53 stay undefined, and push 0** (coordinator
    default). The retail scripts use each twice.
40. **Section 4's words stay defined, although the retail game never uses
    them** (coordinator default). They cost nothing, and mods may use them.
41. **Saves made while gated stay refused once the gate opens**, as SPEC-018
    §7.2 says (unchanged).

**Revision 2 decisions** (2026-10-25, coordinator, at the owner's request)

31. **Host results left on the stack by statement calls are accepted at
    merges** (coordinator default). The first revision's model counted 1080
    merge conflicts in the Fallout 1 corpus, all of this kind. Only host
    results are absorbed. Pushed operands and procedure-call results are
    not, so a row that pops too few operands is still a merge conflict
    (Vector N).
32. **A call's procedure index need not be known** (coordinator default).
    The index does not change the stack effect, and one Fallout 1 call takes
    its index from a name lookup.
33. **A finding in game data is recorded as an exception, never modelled
    away** (coordinator default). Each exception is matched exactly, and a
    stale one counts as a finding. Fallout 1 has one: `gatedemo.int`'s
    four-operand `start_gdialog`.
34. **Push 0 and push 1 are both consistent when a word's result is never
    consumed** (coordinator default). This is a measurement limit of a
    corpus audit, so a word's push stays as its table states until a
    consumed use decides it.
35. **The model check prints a pass/fail line** (coordinator default). It
    stays a report outside the audit categories, which are unchanged.

## 10. Clean-Room Verification Checklist
- [x] Zero C/C++ code snippets or struct definitions included
- [x] All multi-byte integer endianness explicitly stated (big-endian words and payloads, SPEC-004)
- [x] Synthetic test vectors only (section 7); no game asset was read
- [x] Every rule tagged `[Must-match]`, `[May-improve]` or `[Safe rule]`
- [x] Only public SSL opcode, procedure and constant names; no internal symbol of any reference binary
- [x] No implementation-source file path and no 40-character hexadecimal string
- [x] No decompilation, disassembly or re-implementation source consulted

Verification notes (2026-10-25, revised in the reconciliation pass of the
same day): Team A has no access to the research
vault's `spec_lint.py`. The checks above were made by reading the text. Every
hex vector was assembled and offset-checked by hand: each push is 6 bytes,
each word 2. The cross-references are to section titles of SPEC-003 to
SPEC-016 as they stand on this date.

Verification notes for Revision 4 (2026-10-30): the sfall names, pop counts and
pushes in sections 2.2 and 7.14 were read from `oracle/SPEC-024/sfall-opcodes.tsv`.
A throwaway script read section 7.14 back from this file. It decoded each hex
fragment, recomputed each sfall stack result from the oracle's rows, and
recomputed the load table from the ranges in section 2. The ingestion grep finds
no implementation-source path and no 40-character hexadecimal string.
