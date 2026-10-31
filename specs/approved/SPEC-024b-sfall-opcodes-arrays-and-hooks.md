# SPEC-024b: sfall Extended Opcodes, Dynamic Arrays, and Hook Scripts

**Status:** research hand-off, Revision 3, 2026-10-30. Revision 1 was ingested on the owner's brief of 2026-10-30, which approves Decision D-3, and Revision 2 (Team A) made that amendment. Revision 3 comes from the research side, is built on Revision 2's text with Team A's prose correction merged, and needs ingestion. See "Revision history".

**Provenance:** written on the research side from the published sfall scripting documentation at tag v4.5.1 (reference revision S1): the opcode list, the function reference, the function notes, the hook reference, the arrays guide, the compiler guide, and the public script headers. The engine's own approved specs supplied the anchors. sfall's implementation code was **not** read, and nothing here was checked against a running sfall or against Et Tu or Restoration Project scripts (Decisions D-1 and D-26).

**Provenance of Revision 2.** Author: the Spec-Author sub-agent (Team A), 2026-10-30, on the owner's brief of 2026-10-30. Sources: the owner's brief; this spec and `oracle/SPEC-024/`; SPEC-017, SPEC-023; and `docs/REFERENCE.md` (the enhanced words, mod manifests, and what a replay reads). No decompilation, disassembly, re-implementation source, game asset, or engine Rust code was used, and no sfall material was consulted on this side.

**Provenance of Revision 3.** Written on the research side from three sources: the published sfall documents and headers of revision S1, as bundled with *Fallout Et Tu* 2.0.3911; compiled `.int` scripts and script source from *Fallout Et Tu* and from *Restoration Project Updated* 2.4.34; and the engine's approved specs. sfall's implementation code was still **not** read, and nothing was checked against a running sfall. Scripts supplied word numbers, operand constants, counts, and observed call patterns. No script text is reproduced here.

**Oracle:** `oracle/SPEC-024/` holds the complete tables and computed vectors. Section 7 says how they relate to this text. Revision 3 changes three oracle files and adds a fourth.

## Revision history

- **Revision 1 (2026-10-30).** Research hand-off.
- **Revision 2 (2026-10-30, Team A).** Approves Decision D-3 as an owner instruction, and makes the amendment it asked for in SPEC-017 (Revision 4).
  - **Section 3.1.** Sfall mode now has a defined manifest key, and a command-line flag for replays (D-28).
  - **Section 3.2.** The shadowing covers every enhanced word in the overlap. Revision 1 named only `0x8200`, but the four camera words `0x8210..=0x8213` are enhanced words too (D-27).
  - **Section 5.8.** Separates its named delete of `VAULTSFL.DAT` from SPEC-023's stale `.SAV` sweep (D-29). The rule itself does not change.
  - Every other rule, and every oracle file, is unchanged.
- **Prose correction to Revision 2 (2026-10-30, Team A).** On the owner's brief of 2026-10-30, from the implementer's report in `05608f9`, sections 4.1, 5.4, 6.4 and 6.5 now agree with `sfall-opcodes.tsv` (D-T1). `scan_array`'s value is class `N` and `set_sfall_arg`'s value is class `I`, and section 4.1 says what `ret_class` means. No oracle file, vector or word number changed.
- **Revision 3 (2026-10-30, research).** Closes Decision D-26 with an audit of the two target mods (section 1.1), and promotes what they use most.
  - **Section 4.11, new.** 25 behaviors move from tier P2 to P1. Section 4.3 and section 4.2 change to match.
  - **Sections 6.14 to 6.18, new.** Six hooks gain boundary text, so P1 now holds 14 hooks and P2 holds 36.
  - **Section 5.8.** The sidecar becomes format revision 2 (Decision D-32). Its layout and vectors F and F3 change. A revision 1 sidecar migrates on load.
  - **Section 4.2.** `sfall_funcN`'s one-push rule is no longer provisional.
  - **Section 4.10.** Records that Et Tu's base scripts use tier X words (Decision D-40).
  - Revision 2's text is otherwise unchanged, and Decisions D-27 to D-29 stand.
- **Amendment to Revision 3 (2026-10-30, research).** On a later owner's brief of 2026-10-30, before ingestion.
  - **`sfall-opcodes.tsv`.** `scan_array`'s value and `set_sfall_arg`'s value become class `A`, which reverses D-T1's direction for these two words (Decision D-46). Only those two rows change. No vector, word number, or stack contract changes.
  - **Sections 4.1, 5.4, and 6.4.** Team A's prose correction is merged, including what `ret_class` means, except its class `N` and class `I` rules.
  - **Section 6.5.** A value `set_sfall_arg` cannot convert leaves the argument unchanged.
  - **Sections 1 and 4.10.** X words are no-op stubs, not refused words (Decisions D-8 and D-40). This does not change what they do.
- **Second amendment to Revision 3 (2026-10-30, research).** On the owner's brief of 2026-10-30, which settles Decision D-32.
  - **Sections 5.8 and 5.9.** A revision 1 sidecar now migrates instead of being refused: it loads, each field revision 2 adds takes a listed default, `sfall_hash` is the migrated state's, and the next save writes revision 2. The premise that no sidecar exists yet is gone.
  - **Section 6.5.** States that a string converts only to a `mixed` argument.
  - **Section 7 and the oracle.** New vectors MG1 and MG2 (migration), and SS1 and SS2 (string values), add 29 rows to `sfall-array-vectors.tsv`. No existing row changes. New tests 15 and 16.

## Clean-Room Rules for Research Agents

- Document only binary layouts, formulas, opcode numbers, stack transitions, state rules, and observable behavior.
- Do not copy code, struct definitions, function signatures, or internal variable names from `sfall`, `fallout2-ce`, `fallout1-re`, or any decompilation.
- Public SSL opcode names, public hook names, and public script file names are permitted. They are what compiled scripts and mod files contain.
- Provide synthetic test vectors.

## 1. Overview & Scope

This specification lets the engine run compiled scripts that use sfall's extended script interface, as *Fallout Et Tu 2.0* and *Restoration Project Updated* do. It has three parts:

- **Part 1 (section 4):** the extended instruction words `0x8156..=0x8281`, the `sfall_func0` to `sfall_func8` dispatchers, the `metarule3` sub-functions, and their stack contracts (`vault_vm::extensions`).
- **Part 2 (section 5):** dynamic arrays, their storage classes, deterministic iteration, and the save sidecar (`vault_vm` and `vault_core`).
- **Part 3 (section 6):** `hs_*.int` hook scripts, registration, argument and return handling, and fourteen fully specified hooks (`vault_core::host`).

Section 3 comes first because these word numbers collide with SPEC-017.

Out of scope: `ddraw.ini` engine patches, shaders, the graphics and sound words, and the 36 hooks of tier P2. They keep a stack contract only (sections 4.10 and 6.19).

**Tiers.** Every word, function, and hook in the oracle carries one tier. `[Must-match]`

| Tier | Meaning |
| --- | --- |
| P1 | Fully specified here |
| P2 | Number and stack contract are specified; the effect is a stub (section 4.10) |
| X | No-op stub by design, with no memory access (section 4.10) |
| R | Reserved word with no published contract (section 3.3) |

**Compatibility tags.**

| Tag | Meaning here |
| --- | --- |
| `[Must-match]` | The published documentation states it, or a published example directly implies it |
| `[May-improve]` | The documentation acknowledges a defect; the rule gives a safe replacement |
| `[Safe rule]` | The documentation is silent, or its behavior cannot be deterministic; the rule is this vault's definition. It is deterministic and never undefined |

A `[Must-match]` tag means "matches the published documentation". It does not mean "confirmed against a running sfall" (Decision D-1). The sidecar migration rules of sections 5.8 and 5.9 are the one exception: nothing published states them, and they carry the tag by owner instruction because every build must follow them (Decision D-32).

**Invariant.** With sfall mode off, no word, hook, array, file, or hash in this specification exists, and every existing behavior and golden is unchanged. The one exception is how a slot's old `VAULTSFL.DAT` is treated (section 5.8). With sfall mode on and no hook registered, every formula in SPEC-007, SPEC-009, SPEC-011, and SPEC-014 gives the same value as with sfall mode off. `[Must-match]`

### 1.1 Mod audit (Revision 3)

This section is evidence, not requirements. `oracle/SPEC-024/mod-opcode-usage.tsv` holds the ranked results, and it reports numbers and public names only, never script bytes.

| Corpus | Form | Scripts | Method |
| --- | --- | ---: | --- |
| *Fallout Et Tu* 2.0.3911 (`Fo1in2`), `mods` folder | Compiled `.int` | 1060 | SPEC-017 section 8.2's stack model, which also reads operand constants |
| `sfall.dat` and `ce.dat`, shipped with Et Tu | Compiled `.int` | 3 | The same. Counted in their own column, not in the ranking |
| *Restoration Project Updated* 2.4.34 (RPU) | Source only | 1513 | RPU's own build selection and preprocessing, then a token scan of the procedures `-O2` keeps |

Et Tu ships sfall v4.5.1, which is revision S1, and its bundled documents generate this table byte for byte. RPU pins sfall 4.4.8. RPU's tree has no sfall headers, so the v4.5.1 headers stood in for them. No compiler was run, so RPU's numbers come from its source: its 1496 listed scripts with source, and 17 global scripts. Its other 48 listed scripts have no source in the tree, and are not counted.

**Results.**

- **Stack model.** The port reproduces the recorded Fallout 1 model check: 0 findings and the one `gatedemo.int` exception (SPEC-017 section 8.4), over the 881 listed scripts present in the unpacked corpus. On the mods it gives **0 findings** over 1063 scripts, with all 4748 sfall words of the linear decode reached, none above `0x8281`, and no reserved word.
- **Selectors.** Every `sfall_funcN` name, `metarule3` sub-function, and hook ID was read from a constant. None was unresolved.
- **Signatures.** SPEC-017 section 8.3's inference over each promoted word with a compiled call site finds the table's signature among the consistent candidates. Several candidates can fit, because the model accepts values left on the stack until a frame ends, so this confirms each signature rather than proving it.
- **`sfall_funcN` always pushes.** Functions documented as having no result are followed by the discard word at every statement call that was sampled: `set_unique_id` 84 of 84, `message_box` 8 of 8, `create_win` 6 of 6, `signal_close_game` 5 of 5. `get_terrain_name`, whose result is used, is followed by it at 0 of 24.
- **Source calibration.** The source scan ran over the 1045 Et Tu scripts that exist in both forms. Over Tier P2 rows it matches the binary on all 148 script and row pairs, with no extra and no missing pair. Over hooks it matches on all 36 pairs. Its only misses are 460 pairs for P1 array words that the compiler generates from array syntax, plus one extra pair for `load_array`. Neither touches a P2 row. RPU's source calls all match the table's operand counts.
- **Use.** 86 of the 280 Tier P2 opcode rows, and 22 of the 42 Tier P2 hooks, are used. Of the eight hooks already in P1, seven are used, and `HOOK_TOHIT` is used by neither mod.
- **Hook files.** Neither mod ships an `hs_*.int`. Every registration in both goes through `register_hook_proc` or `register_hook_proc_spec`.
- **Names outside the table.** `rest_option_msgs`, `set_rest_option`, and `set_party_member_cc_msg_ids` are fallout2-ce extensions behind Et Tu's own engine test, and `metarule3` 999 appears in one `sfall.dat` script. Section 4.2's unknown-name rule and SPEC-017 section 5.2's unknown rule cover them.

**Ranking.** Rows are ordered by scripts, then call sites (Decision D-31). Ties at the cut-off are real, so the tie rule matters: nine opcode rows tie for the last four places, and nineteen hooks tie for the last three.

## 2. Conventions

SPEC-004 and SPEC-005 conventions apply unless a rule here says otherwise. `[Must-match]`

- Instruction words are big-endian. No word here has inline operands.
- **Pop order is first popped to last popped.** A script pushes operands in public-call order, so the call's last argument is on top and is popped first.
- `[a, b] -> [r]` is a stack transition, with the top of the stack on the right.
- A word with `none` in the push column leaves nothing on the stack.
- All new binary layouts are little-endian, using SPEC-021 section 2.2's primitive names (`u8`, `u32`, `i32`, `tag`, `opt`, `seq`). This differs from the big-endian `.INT` and `SAVE.DAT` layouts.
- Diagnostics follow SPEC-017 section 1: one line in the debug log that names the word and the reason, with no effect on the program or the stack.

**Tag scope.** A tag covers its own table row, bullet, or sentence. A table or list with no tag of its own takes the tag of the sentence that introduces it or the line straight after it. A heading statement such as "all of this section" covers everything under that heading unless a line says otherwise. Sentences that give a reason, or evidence, are not requirements. `[Must-match]`

## 3. Word Sets and Dispatch

### 3.1 Activation

- **sfall mode** is a session setting, off by default. `[Safe rule]`, as an owner instruction (Decision D-3).
- **Settings file.** `sfall = true` under `[scripting]` in `vault-engine.toml`. The key name is a default. `[Safe rule]`
- **Mod manifest.** The same table and key in a layer's `vault-mod.toml`, which adds them to the manifest's grammar. They work as `[enhanced]` keys do: only enabled layers count, they apply over the settings file with the lowest layer first, and the highest layer that sets the key wins. `[Safe rule]`
- **Command line.** `--sfall` turns it on and wins over both files. A replay reads neither file, so a replay of an sfall session needs this flag. `[Safe rule]`
- It is accepted only under the `fallout2` profile. *Et Tu 2.0* and *Restoration Project Updated* both run on Fallout 2 data. Requesting it under `fallout1` is refused when the session is built, with an explicit error. `[Safe rule]`
- It is fixed when the session is built. Loading a save does not change it. `[Safe rule]`
- As SPEC-017 section 2.3 requires, a word's meaning comes from the session and never from inspecting a script. `[Must-match]`

### 3.2 The collision and its resolution

sfall's published numbering begins at `0x8156`. SPEC-017 gives Fallout 2 the set `0x8000..=0x8183` and keeps `0x8200..=0x82FF` as the engine's enhanced range. The three cannot share one table. The published numbers below are `[Must-match]`:

| Word | SPEC-017 / engine meaning | sfall meaning |
| --- | --- | --- |
| `0x8156` | `inven_cmds` | `read_byte` |
| `0x8157` | `reg_anim_change_fid` | `read_short` |
| `0x8158` | `reg_anim_take_out` | `read_int` |
| `0x8159` | `reg_anim_turn_towards` | `read_string` |
| `0x815A..=0x8183` | Reserved | Stats, perks, and more |
| `0x8184..=0x81FF` | Refused at load | Various |
| `0x8200` | `engine_api_version` (enhanced) | `fs_size` |
| `0x8210` | `op_cam_pan_to` (enhanced) | `sfall_ver_major` |
| `0x8211` | `op_cam_set_zoom` (enhanced) | `sfall_ver_minor` |
| `0x8212` | `op_cam_shake` (enhanced) | `sfall_ver_build` |
| `0x8213` | `op_cam_reset` (enhanced) | `hero_select_win` |

Sfall mode resolves this with one new set, `WordSet::FALLOUT2_SFALL`, which is `0x8000..=0x8281`. `[Safe rule]`, as an owner instruction (Decision D-3).

| Range | Sfall mode off | Sfall mode on |
| --- | --- | --- |
| `0x8000..=0x8155` | SPEC-017 | SPEC-017, unchanged |
| `0x8156..=0x8281` | SPEC-017, as today | This specification, with or without the enhanced preset. The SPEC-017 section 4.1 rows and every enhanced word in this range (today `0x8200` and `0x8210..=0x8213`) are unreachable |
| `0x8282..=0x82FF` | SPEC-017, as today | SPEC-017, as today: enhanced preset only, otherwise refused at load |

Evidence that this costs retail play nothing: SPEC-017 Revision 3's audit of 1248 retail scripts found no use of any word above `0x8155`.

The three `reg_anim_*` names of SPEC-017 section 4.1 are sfall words, published at `0x825E`, `0x825F`, and `0x8260`. Under sfall mode they keep their SPEC-017 effect and pop order at those numbers. `[Must-match]` for the numbers; `[Safe rule]` for reusing the effect text. `inven_cmds` has no number in this table, so it cannot be reached under sfall mode. `[Safe rule]`

The dispatch table under sfall mode has **642 entries**, indices 0 to 641 (`0x8000` to `0x8281`). The SPEC-004 section 3.8 safe rule applies above it. The load-time rule on bits 10 to 14 is unchanged. `[Safe rule]`

The mod-script stack check (SPEC-017 section 8) uses this table under sfall mode. The `pops` and `push` columns of `sfall-opcodes.tsv` are its model. `[Safe rule]`

**SPEC-017 amendment** (Decision D-3): SPEC-017 Revision 4 makes it. Section 2.1 gains the third set; section 2.2's statement that `0x8184..=0x81FF` belongs to no set gains "except under sfall mode"; section 2.2's enhanced range gains the shadowing above; and section 7.14 gives the vectors. With sfall mode off, no SPEC-017 word changes.

### 3.3 Reserved words

`0x8252`, `0x8258`, `0x8259`, `0x8268`, `0x8269`, and `0x826A` have no published contract, so their operand counts are unknown. They are in the set: a program containing one loads. Executing one consumes nothing and raises the undefined-instruction VM error. `opcode_exists` reports 0 for them. `[Safe rule]`

## 4. Part 1: Extended Opcodes

### 4.1 Value model and coercion

A value is an int (tag `0xC001`), a float (`0xA001`), or a string (`0x9001` or `0x9801`). Object references and array IDs are ints. Operand checks first clear bit `0x0800`, as SPEC-004 section 2.5 says; any other tag is a failed operand. `[Must-match]`

Each operand of a word has a class, given in the `arg_classes` column of `sfall-opcodes.tsv`:

| Class | Int operand | Float operand | String operand |
| --- | --- | --- | --- |
| `I` integer | As given | Truncated toward zero, saturating at the i32 limits | Failed |
| `F` float | Converted to the nearest binary32 | As given | Failed |
| `N` number | As given | As given | Failed |
| `S` string | Failed | Failed | As given |
| `K` key | As given | Failed | As given |
| `A` any | As given | As given | As given |
| `O` object | As given; not a live object means null (SPEC-017 section 1) | Failed | Failed |
| `P` procedure | Procedure index; 0 means none | Failed | Failed |

The whole table is `[Safe rule]`, with one documented basis: the public hook example passes a float to `set_sfall_arg` and `set_sfall_return`, so a float must be accepted where an int is declared.

**Failed operand.** Every operand is popped first. The word then has no effect, writes a diagnostic, and pushes its default: int 0 for return class `I`, `O`, `N`, or `A`; float 0.0 for `F`; the empty string for `S`. **Unlike SPEC-017, a failed operand is not a VM error**, because scripts written for sfall expect its functions not to stop the script. `[Safe rule]`

**Return class.** The `ret_class` column of `sfall-opcodes.tsv` chooses that default, the value pushed when an operand fails. It does not limit what a word returns, because a pushed value keeps its own type. So `scan_array` and `array_key`, with `ret_class` `I`, can push a float or string map key (vector B step 9 pushes `F:5.0`), `get_sfall_arg` pushes what the argument holds, and `list_next` pushes an object reference. The Push column of a word table gives what the word can push. `[Safe rule]`

Stack underflow stays a VM error (SPEC-004 section 3.8). `[Must-match]`

A returned string is allocated in the calling program's runtime string storage and carries tag `0x9801`. A returned float carries `0xA001`, and anything else `0xC001`. `[Must-match]`

### 4.2 `sfall_funcN` dispatch

| Word | Mnemonic | Arguments N | Stack transition |
| --- | --- | ---: | --- |
| `0x8276` | `sfall_func0` | 0 | `[name] -> [r]` |
| `0x8277` | `sfall_func1` | 1 | `[name, a1] -> [r]` |
| `0x8278` | `sfall_func2` | 2 | `[name, a1, a2] -> [r]` |
| `0x8279` | `sfall_func3` | 3 | `[name, a1, a2, a3] -> [r]` |
| `0x827A` | `sfall_func4` | 4 | `[name, a1 .. a4] -> [r]` |
| `0x827B` | `sfall_func5` | 5 | `[name, a1 .. a5] -> [r]` |
| `0x827C` | `sfall_func6` | 6 | `[name, a1 .. a6] -> [r]` |
| `0x8280` | `sfall_func7` | 7 | `[name, a1 .. a7] -> [r]` |
| `0x8281` | `sfall_func8` | 8 | `[name, a1 .. a8] -> [r]` |

The numbers have a gap: 7 and 8 sit after `0x827F`. There is **no `sfall_func9`** in the published list; `0x8282` is not an sfall word. `[Must-match]`

- Pop order is `aN`, then down to `a1`, then the name. `[Must-match]`
- Each call pushes **exactly one** value, including functions documented as having no result, which push int 0. `[Must-match]` The mod audit confirms it (section 1.1).
- The name is matched as exact bytes. An unknown name pushes int 0 with a diagnostic. `[Safe rule]`
- A known name called with fewer arguments takes the documented default; extra arguments are ignored. `[Safe rule]`

Functions of tier P1, `[Must-match]` unless a row says otherwise:

| Name | Arguments | Result |
| --- | --- | --- |
| `get_metarule_table` | none | Temporary list of the P1 function names, ascending by bytes. `[Safe rule]` for limiting it to P1 |
| `metarule_exist` | name | Int 1 for a P1 name, else 0. `[Safe rule]` for limiting it to P1 |
| `opcode_exists` | word | Int 1 for a P1 word, else 0. `[Safe rule]` for limiting it to P1 |
| `get_sfall_arg_at` | index | Hook argument at that index, section 6.4 |
| `get_ini_section`, `get_ini_sections`, `get_ini_config`, `set_ini_setting` | See section 4.5 | See section 4.5 |
| `string_find`, `string_format`, `string_to_case`, `string_compare` | See section 4.6 | See section 4.6 |
| `floor2` | number | See section 4.7 |
| `get_object_data`, `set_object_data`, `get_flags`, `set_flags`, `critter_inven_obj2` | See section 4.8 | See section 4.8 |
| `set_unique_id`, `get_terrain_name`, `add_extra_msg_file`, `message_box`, `create_win`, `dialog_obj`, `signal_close_game` | See section 4.11 | See section 4.11 |

Limiting the existence queries to P1 means a script that checks before relying on a function takes its fallback path rather than trusting a stub.

Et Tu tells sfall from fallout2-ce by `opcode_exists(0x823B)`: 1 means sfall. `modified_ini` is P1, so here the answer is 1 and Et Tu takes its sfall paths. `0x823B` must stay P1. `[Safe rule]` The three fallout2-ce function names of section 1.1 stay unknown names here, and Et Tu reaches them only on its fallout2-ce path.

### 4.3 `metarule`, `metarule3`, and `metarule2_explosions`

- **`metarule`** (`0x810B`). The published documentation adds no rule numbers. SPEC-017 section 5.1 applies unchanged. `[Must-match]`
- **`metarule3`** (`0x80E1` under Fallout 2). SPEC-017 section 5.2's pop order and its rules 100 to 111 apply unchanged. sfall adds the following sub-functions. `[Must-match]` for numbers and operands.

| Sub | Public macro | p1 | p2 | p3 | Tier | Effect |
| ---: | --- | --- | --- | --- | --- | --- |
| 200 | `set_horrigan_days` | days | 0 | 0 | P2 | Stub |
| 201 | `clear_keyboard_buffer` | 0 | 0 | 0 | P1 | Empties the logical key queue, including any `tap_key` still queued; int 0. `[Must-match]` |
| 210 | `get_current_save_slot` | 0 | 0 | 0 | P2 | Stub |
| 211 | `set_current_save_slot` | page | slot | 0 | P2 | Stub |
| 212 | `get_current_quick_save_page` | 0 | 0 | 0 | P2 | Stub |
| 213 | `get_current_quick_save_slot` | 0 | 0 | 0 | P2 | Stub |
| 214 | `set_current_quick_save_slot` | page | slot | check | P2 | Stub |

Any other sub-function follows SPEC-017 section 5.2's unknown rule. `[Must-match]`

- **`metarule2_explosions`** (`0x8261`) pops three operands and pushes one value, as `[a, b, c] -> [r]`, where a is the sub-function. Sub-function 6, `get_explosion_damage`, is P1 (section 4.11). Sub-functions 1 to 5 and 7 to 9 are P2, and any other sub-function pushes int 0 with a diagnostic. The opcode list shows a single operand, but the headers and function notes show three, and the compiled and source calls in the audit use three. `[Must-match]`; `[Safe rule]` for other sub-functions.

### 4.4 Environment words

| Word | Mnemonic | Pop order | Push | Effect |
| --- | --- | --- | --- | --- |
| `0x8210` | `sfall_ver_major` | none | I | 4. `[Must-match]` |
| `0x8211` | `sfall_ver_minor` | none | I | 5. `[Must-match]` |
| `0x8212` | `sfall_ver_build` | none | I | 1. `[Must-match]` |
| `0x8164` | `game_loaded` | none | I | 1 on a script's first call after a new game or load, else 0, tracked per script. `[Must-match]` |
| `0x81AF` | `get_game_mode` | none | I | Bit set below. `[Must-match]` |
| `0x8170` | `in_world_map` | none | I | 1 while the world map is open, else 0. `[Must-match]` |
| `0x8163` | `get_year` | none | I | Year of the game calendar. `[Must-match]` |
| `0x81B3` | `get_uptime` | none | I | Session step count times 16. `[Safe rule]`: the published behavior reads the wall clock, which cannot be replayed |

`get_game_mode` bits: `0x1` world map, `0x2` local map, `0x4` dialogue, `0x8` escape menu, `0x10` save, `0x20` load, `0x40` combat, `0x80` options, `0x100` help, `0x200` character screen, `0x400` Pip-Boy, `0x800` player's combat turn, `0x1000` inventory, `0x2000` automap, `0x4000` Skilldex, `0x8000` use-item window, `0x10000` loot, `0x20000` barter, `0x40000` hero window, `0x80000` dialogue review, `0x100000` counter window, `0x200000` pause window, `0x80000000` special. `[Must-match]`

### 4.5 INI configuration

| Word | Mnemonic | Pop order | Push | Effect |
| --- | --- | --- | --- | --- |
| `0x81AC` | `get_ini_setting` | setting S | I | Integer value, or -1 |
| `0x81EB` | `get_ini_string` | setting S | S, or I -1 | String value; empty when the file or key is absent; **int** -1 when the setting is malformed |
| `0x823B` | `modified_ini` | none | I | The `ModifiedIni` value, or 0 when none is set. `[Safe rule]` for the 0 |

The setting text has exactly three fields split by `|`: file, section, key. The file name is at most 63 bytes, and the section at most 32. Anything else is malformed. A missing file or key, or a malformed setting, gives -1 from `get_ini_setting`. `[Must-match]`

- Integer values parse by the `atoi` rule of section 4.6. `[Safe rule]`
- **`get_ini_section`** (file, section) gives a temporary map of string keys to string values, or an empty one when the file is absent. Pairs are in file order. `[Must-match]`; `[Safe rule]` for the order.
- **`get_ini_sections`** (file) gives a temporary list of section names in file order, or an empty one. `[Must-match]`; `[Safe rule]` for the order.
- **`get_ini_config`** (file, 0 or 1) gives a permanent map of section name to permanent sub-map. A repeated call for the same file gives the same array unless it was freed. `[Must-match]`
- **`set_ini_setting`** (setting, value) writes an int or string. A missing file is created. `[Must-match]`

Vault limits, all `[Safe rule]`:

- A file name holding `..`, an absolute path, or a drive prefix is malformed.
- Reads go through the `Vfs` layers, then the profile's writable folder. Writes go to the writable folder only, never to an archive.
- `ddraw.ini` is an ordinary file name. Without such a file, its queries give the absent result.
- Section and key names compare without case. The file is read once and cached for the session, and `set_ini_setting` updates that cache. INI files are inputs: a replay needs the same files.

### 4.6 Strings and conversion

| Word | Mnemonic | Pop order | Push | Effect |
| --- | --- | --- | --- | --- |
| `0x8235` | `string_split` | separator, text | I array | Temporary list of the pieces; an empty separator splits into single bytes. `[Must-match]` |
| `0x8237` | `atoi` | text S | I | See below |
| `0x8238` | `atof` | text S | F | See below |
| `0x824E` | `substr` | length, start, text | S | See below |
| `0x824F` | `strlen` | text S | I | Length in bytes. `[Must-match]` |
| `0x8250` | `sprintf` | value A, format S | S | One-value format, below. `[Must-match]` |
| `0x8251` | `charcode` | text S | I | First byte, unsigned. 0 for the empty string. `[Must-match]`; `[Safe rule]` for the 0 |
| `0x8253` | `typeof` | value A | I | 1 int, 2 float, 3 string. `[Must-match]` |

**`substr(text, start, length)`.** `[Must-match]`

- The first position is 0. A negative start counts from the end.
- A negative length omits that many bytes from the end.
- Length 0 runs to the end of the string.

Positions are then clamped to the text; an empty range gives the empty string. `[Safe rule]`

**`atoi` and `atof`.** Skip leading ASCII whitespace, accept one optional sign, and read decimal digits (for `atof`, also a fraction and an exponent) up to the first byte that does not fit. No digits gives 0. `atoi` saturates at the i32 limits. `[Safe rule]`

**`string_find`** (haystack, needle, optional position) gives the first match's position, or -1. A negative position counts from the end. `[Must-match]`

**`string_to_case`** (text, mode) gives lowercase for 0 and uppercase for 1, for A-Z and a-z only. `[Must-match]`

**`string_compare`** (a, b, optional code page) gives 1 when the strings match ignoring case, else 0. `[Must-match]` Case folding covers ASCII only, and the code page is accepted and ignored, so bytes 128 to 255 compare exactly. `[Safe rule]`

**Format strings** (`sprintf`, `string_format`). The usual percent syntax applies. `sprintf` takes one value, and `string_format` up to seven; its format is limited to 1024 bytes. `[Must-match]` The conversions are `d i u x X o c s f e g E G %`, with flags, width, and precision. `%c` gives the byte with that code. `[Must-match]`

Coercions, `[Safe rule]`: a `%d`-family conversion truncates a float; a `%f`-family conversion converts an integer; `%s` gives a number's plain decimal text; and a number conversion given a string fails the operand. Floats print with a fixed `.` decimal mark in every locale. Missing values leave the conversion text empty.

### 4.7 Math

| Word | Mnemonic | Pop order | Push | Effect |
| --- | --- | --- | --- | --- |
| `0x81EC` | `sqrt` | x F | F | Square root |
| `0x81ED` | `abs` | x N | N | Absolute value, same type. `abs` of the i32 minimum stays the minimum. `[Safe rule]` for that case |
| `0x81EE` | `sin` | x F | F | Sine, radians |
| `0x81EF` | `cos` | x F | F | Cosine, radians |
| `0x81F0` | `tan` | x F | F | Tangent, radians |
| `0x81F1` | `arctan` | y F, x F | F | Two-argument arctangent of `x / y`; pass y = 1 for the plain form. Public call order is x, then y |
| `0x8264` | `log` | x F | F | Natural logarithm |
| `0x8265` | `exponent` | x F | F | `e` to the x |
| `0x8266` | `ceil` | x F | I | Least integer not below x |
| `0x8267` | `round` | x F | I | Nearest integer. Ties go away from zero. `[Safe rule]` for ties |
| `0x8263` | `^` | exponent N, base N | N | See below |
| `0x827F` | `div` | divisor N, dividend N | N | See below |

The rows are `[Must-match]` unless tagged.

- **`^`.** Two ints give an int. An int exponent allows a negative base. A float exponent with a negative base gives NaN. `[Must-match]` Int overflow wraps at 32 bits, and a negative int exponent gives 0 unless the base is 1 or -1. `[Safe rule]`
- **`div`.** Two ints divide as unsigned 32-bit values, and the result bits are pushed as an int. If either is a float, it is ordinary signed division. `[Must-match]` Division by zero follows SPEC-004 section 3.8. `[Safe rule]`
- **`floor2`** (function) rounds toward negative infinity: `floor2(-1.5)` is -2. `[Must-match]`
- **Determinism.** `sqrt` uses IEEE-754 rounding. The other functions (`sin cos tan arctan log exponent ^`) must not use platform math libraries. They use one pure-software implementation pinned in `Cargo.lock`, computing in binary64 and rounding to binary32, so every platform gives identical bits. `[Safe rule]`

### 4.8 Critter and object properties

Stat, skill, and perk numbers are those of SPEC-006. Object operands that are null give the default result and do nothing. `[Safe rule]`

| Word | Mnemonic | Pop order | Push | Effect |
| --- | --- | --- | --- | --- |
| `0x815A` | `set_pc_base_stat` | value, stat | none | Same as `set_critter_base_stat` for the player |
| `0x815B` | `set_pc_extra_stat` | value, stat | none | Same as `set_critter_extra_stat` for the player |
| `0x815C` | `get_pc_base_stat` | stat | I | Same as `get_critter_base_stat` for the player |
| `0x815D` | `get_pc_extra_stat` | stat | I | Same as `get_critter_extra_stat` for the player |
| `0x815E` | `set_critter_base_stat` | value, stat, critter O | none | Stores the base value |
| `0x815F` | `set_critter_extra_stat` | value, stat, critter O | none | Stores the bonus value |
| `0x8160` | `get_critter_base_stat` | stat, critter O | I | Reads the base value |
| `0x8161` | `get_critter_extra_stat` | stat, critter O | I | Reads the bonus value |
| `0x8191` | `get_critter_current_ap` | critter O | I | Current AP |
| `0x8192` | `set_critter_current_ap` | ap, critter O | none | Sets current AP |
| `0x8242` | `set_critter_skill_points` | value, skill, critter O | none | Sets stored skill points |
| `0x8243` | `get_critter_skill_points` | skill, critter O | I | Reads stored skill points |
| `0x8217` | `get_weapon_ammo_pid` | weapon O | I | Loaded ammunition PID |
| `0x8218` | `set_weapon_ammo_pid` | pid, weapon O | none | Sets loaded ammunition PID |
| `0x8219` | `get_weapon_ammo_count` | weapon O | I | Loaded rounds |
| `0x821A` | `set_weapon_ammo_count` | count, weapon O | none | Sets loaded rounds |
| `0x8248` | `get_last_target` | critter O | O | The critter's last target, or null |
| `0x8249` | `get_last_attacker` | critter O | O | The critter's last attacker, or null |
| `0x8193` | `active_hand` | none | I | 0 left, 1 right. `[Safe rule]` for the numbering |
| `0x8194` | `toggle_active_hand` | none | none | Switches the active hand |
| `0x818C` | `get_kill_counter` | category | I | Player's kill count |
| `0x818D` | `mod_kill_counter` | amount, category | none | Adds to the count |
| `0x8204` | `get_proto_data` | offset, pid | I | Field below; -1 for an invalid PID |
| `0x8205` | `set_proto_data` | value, offset, pid | none | Changes the in-memory prototype |
| `0x8206` | `set_self` | object O | none | See below |

Numbers, operands, and pushes are `[Must-match]`. The stat words apply no perks and no clamping. `[Must-match]`

**`set_self`.** The object replaces `self_obj` for the next word only, then the original returns. Object 0 reverts it at once. `source_obj` and `target_obj` do not work straight after it. A script's first `set_self` affects the next call, and a second consecutive call is what the docs require for `use_obj`, `use_obj_on_obj`, and the object's local variables. `[Must-match]`

**`self_obj` in a global script without `set_self`.** The documentation says this crashes. `[May-improve]`: it gives null, with a diagnostic.

**Prototype offsets.** Offsets address fields by meaning, not by the `.pro` file layout of SPEC-003. Each offset below reads or writes the named SPEC-003 field. An offset not listed gives 0 and writes nothing, with a diagnostic. Prototype edits last until the prototype reloads and are not saved. `[Must-match]` for the offsets; `[Safe rule]` for unlisted offsets.

| Class | Offset: field |
| --- | --- |
| All | 0 PID; 4 text ID; 8 art ID; 12 light distance; 16 light intensity; 20 flags; 24 extended flags; 28 script ID |
| Item | 32 item type; 108 material; 112 size; 116 weight; 120 cost; 124 inventory art; 128 sound (one byte) |
| Weapon | 36 animation code; 40 minimum damage; 44 maximum damage; 48 damage type; 52 primary range; 56 secondary range; 60 projectile PID; 64 minimum Strength; 68 primary AP; 72 secondary AP; 76 critical-failure table; 80 perk; 84 burst size; 88 caliber; 92 ammunition PID; 96 magazine size; 100 sound |
| Armor | 36 AC; 40-64 damage resistance in order normal, laser, fire, plasma, electrical, EMP, explosion, by 4; 68-92 damage threshold in the same order, by 4; 96 perk; 100 male art; 104 female art |
| Container | 36 maximum size; 40 flags |
| Ammunition | 36 caliber; 40 pack size; 44 AC modifier; 48 DR modifier; 52 damage multiplier; 56 damage divisor |
| Misc item | 36 power PID; 40 caliber |
| Critter | 32 critter flags; `36 + 4s` base stat s, 0 to 34; `176 + 4s` bonus stat s, 0 to 34; `316 + 4k` skill k, 0 to 17; 388 body type; 392 kill experience; 396 kill type; 400 damage type; 404 head art; 408 AI packet; 412 team |

**`get_object_data`** (object, offset) and **`set_object_data`** (object, offset, value) use the offsets below. An unlisted offset gives 0 and writes nothing, with a diagnostic. Writing a value that SPEC-021 covers goes through the ordinary state, so the hash sees it. `[Must-match]` for the offsets; `[Safe rule]` for the rest.

| Class | Offset: field |
| --- | --- |
| Any object | `0x00` object ID; `0x04` hex; `0x18` current frame; `0x1C` rotation; `0x20` art ID; `0x24` flags; `0x28` elevation; `0x38` misc flags; `0x64` PID; `0x68` combat ID; `0x6C` light distance; `0x70` light intensity; `0x78` script ID; `0x80` script index |
| Item | `0x3C` current charges (key code for keys) |
| Critter | `0x3C` combat state flags; `0x40` current AP; `0x44` damage flags; `0x48` damage last turn; `0x54` who hit it; `0x58` HP |

**`get_flags`** and **`set_flags`** read and write the `0x24` flags word. **`critter_inven_obj2`** (critter, type) acts as SPEC-005 `critter_inven_obj`, and also reports the player's inactive hand correctly. `[Must-match]`

**Lists.**

| Word | Mnemonic | Pop order | Push | Effect |
| --- | --- | --- | --- | --- |
| `0x820D` | `list_begin` | type | I | New list ID |
| `0x820E` | `list_next` | list ID | O | Next object, or 0 at the end |
| `0x820F` | `list_end` | list ID | none | Ends the list |
| `0x8236` | `list_as_array` | type | I array | Temporary list of the objects |

Types: 0 critters, 1 ground items, 2 scenery, 3 walls, 5 miscellaneous, 6 spatial, 9 all. Numbers, operands, and pushes are `[Must-match]`. Objects come in SPEC-021's entity order, the list is a snapshot taken at the call, and the end of a list gives 0. `[Safe rule]`

The documentation warns that a missing `list_end` leaks memory. `[May-improve]`: open lists are released at the end of the Simulation phase. `list_end` on one is a no-op.

### 4.9 sfall globals and global scripts

**Globals.**

| Word | Mnemonic | Pop order | Push | Effect |
| --- | --- | --- | --- | --- |
| `0x819D` | `set_sfall_global` | value N, key K | none | Stores the value |
| `0x819E` | `get_sfall_global_int` | key K | I | The value, or 0 when unset |
| `0x819F` | `get_sfall_global_float` | key K | F | The value, or 0.0 when unset |

A string key is **exactly 8 bytes** and case-sensitive; an int key is any int. `[Must-match]` A string key of another length makes the word a failed operand. The value keeps its type. `get_sfall_global_int` truncates a float, and `get_sfall_global_float` converts an int. `[Safe rule]` Globals are saved in the sidecar (section 5.8). `[Must-match]`

**Global scripts.** `[Safe rule]` unless tagged. This is the minimum that hooks depend on, and it yields to SPEC-024a if that document defines it (Decision D-21).

- A global script is a `.INT` under `scripts/` whose base name begins with `gl`, without case. It has no attached object. `[Must-match]`
- Scripts are ordered ascending by lowercased base name, compared as bytes. Registration order is observable (section 6.2), so this order must be fixed.
- `start` runs once after a new game or a load, in the session setup of section 6.1. `[Must-match]`
- `map_enter_p_proc`, `map_exit_p_proc`, and `map_update_p_proc` run at those events, in the same order. `[Must-match]` for the procedures.

| Word | Mnemonic | Pop order | Push | Effect |
| --- | --- | --- | --- | --- |
| `0x816A` | `set_global_script_repeat` | steps | none | Runs `start` every n steps, 1 meaning every step and 0 disabling it. No effect outside a global script. `[Must-match]`; a frame is one step, section 5.3 |
| `0x819B` | `set_global_script_type` | type | none | 0 local map; 1 whenever the cursor is shown, including world map and dialogue; 2 world map; 3 local and world map, not dialogue. `[Must-match]` |
| `0x819C` | `available_global_script_types` | none | I | 15, for all four types. `[Safe rule]` |

Repeating global scripts run in phase 2 of the step, in the order above.

### 4.10 Tiers P2, X, and R

- **P2 words and functions.** The operands are popped as the table says. The word has no effect and writes one diagnostic per program. If it pushes, the push is int 0, or the empty string for return class `S`. Scripts keep running. `[Safe rule]`
- **X words** are `read_byte`, `read_short`, `read_int`, `read_string`, `write_byte`, `write_short`, `write_int`, `write_string`, `call_offset_v0` to `call_offset_v4`, and `call_offset_r0` to `call_offset_r4`. They address the original executable's memory, which does not exist here, so they are no-op stubs under the P2 rule. Each pops its operands, has no effect, pushes the default return value if it pushes (int 0, or the empty string for `read_string`), writes the standard diagnostic once per program, and never halts the script. `[Safe rule]` Their addresses are never used to reach host memory.
- **R words** follow section 3.3.

**X words in the target mods.** Et Tu's base scripts use them, so these effects are lost here (Decision D-40). `gl_0_settings.int` holds `write_int` and `write_byte`, for its rest-timer patch. `gl_fo1mechanics.int` holds `write_byte`, for its Dogmeat dialogue patch. Two optional mini-mods also hold them: `gl_footprints.int` (`read_int`) and `gl_character_randomizer.int` (`read_int`, `write_int`, `write_string`). The scripts keep running, because the P2 rule keeps the stack balanced. RPU uses no X word.

### 4.11 Words and functions promoted in Revision 3

These are the 25 behaviors the mod audit (section 1.1) put first. Numbers, operands, and pushes are `[Must-match]`. An effect is `[Must-match]` only where the row or sentence says so. The published text for many of these is a signature alone, so most effects are `[Safe rule]`, and the "Evidence" sentences say what the real scripts show. Evidence sentences are not requirements.

Words with no binary call site have no compiled evidence for their stack shape: `force_encounter`, `set_car_current_town`, `is_iface_tag_active`, `fs_seek`, `fs_read_short`, and `fs_write_short`. Their source calls all match the table's operand count (section 1.1).

**Party and explosives.**

| Word | Mnemonic | Pop order | Push | Effect |
| --- | --- | --- | --- | --- |
| `0x8271` | `party_member_list` | include hidden | I array | Temporary list of party entries. `[Must-match]` for the two modes |
| `0x8261` sub 6 | `get_explosion_damage` | 0, PID, 6 | I array | Temporary list `[min, max]`. `[Must-match]` |

- **`party_member_list`.** 0 gives the entries that are alive critters with object flag `0x01` (hidden) clear, and 1 gives every entry. `[Must-match]` Any other value acts as 1. `[Safe rule]` Entries are in SPEC-019 section 3.14 order, so the player comes first, as that section's count includes the player. `[Safe rule]`
- **`get_explosion_damage`.** PIDs 51 and 206 give `[30, 50]`, and PIDs 85 and 209 give `[40, 80]`, from SPEC-013 section 3.10. Any other weapon PID gives its prototype's minimum and maximum damage (section 4.8 offsets 40 and 44). Any other PID gives `[0, 0]`. `[Safe rule]` for every PID but the four explosives. Evidence: RPU indexes the result as two elements for a trap's damage draw.

**Object IDs.** `set_unique_id` (object, optional flag) gives int. `[Must-match]`

- The unique range is `0x10000000..=0x7FFFFFFF`. An object whose `0x00` ID (section 4.8) is in that range keeps it, and that ID is returned without reassignment. Otherwise it takes the next unique ID, which is then returned. `[Must-match]`
- The next unique ID starts at `0x10000000`, rises by 1 for each assignment, and is saved in the sidecar (section 5.8). A released ID is not reused. `[Safe rule]`
- Flag -1 releases a unique ID: the object takes a new ordinary ID from the engine's allocator, which is returned. `[Must-match]` On an object without a unique ID, flag -1 returns its ID unchanged. Any other flag is ignored. `[Safe rule]`
- At the range's end, no ID is assigned and the current ID is returned, with a diagnostic. A null object gives 0. `[Safe rule]`
- **Stacking.** Under sfall mode, SPEC-011 section 3.13 gains one condition: neither item has an ID in the unique range. `[Must-match]`

**Message files.**

| Word | Mnemonic | Pop order | Push | Effect |
| --- | --- | --- | --- | --- |
| `0x826B` | `message_str_game` | message, file ID | S | As `message_str`, from a game message file. `[Must-match]` |

| File ID | File |
| ---: | --- |
| 0 to 4 | `combat`, `combatai`, `scrname`, `misc`, `custom` |
| 5 to 9 | `inventry`, `item`, `lsgame`, `map`, `options` |
| 10 to 14 | `perk`, `pipboy`, `quests`, `proto`, `script` |
| 15 to 20 | `skill`, `skilldex`, `stat`, `trait`, `worldmap`, `editor` |
| `0x1000` to `0x1005` | `pro_item`, `pro_crit`, `pro_scen`, `pro_wall`, `pro_tile`, `pro_misc` |
| `0x2000` to `0x2FFF` | `ExtraGameMsgFileList` entries |
| `0x3000` to `0x3FFF` | `add_extra_msg_file` entries |

The IDs are `[Must-match]`. Files are `.msg` files under `text/<language>/game/`. The published names give the file stems; ID 1 is `combatai.msg`, because that is the file's name in the game data. `[Safe rule]` for the stems.

- A nonpositive message number, an absent message, an unassigned file ID, or an absent file gives `Error`, as `message_str` does (SPEC-005). `[Must-match]`
- It starts no speech and no lip sync, unlike `message_str` in a talking-head dialogue. `[Safe rule]`
- **`ExtraGameMsgFileList`** is read from `ddraw.ini`, section `[Misc]`, as a comma list. Files take consecutive IDs from `0x2000`. An entry written `name:n` takes `0x2000 + n`, and the entries after it continue from there. `[Must-match]`

**`add_extra_msg_file`** (name, optional ignored number) gives int. `[Must-match]`

- It loads the named `.msg` from the current language's game folder, then from `text/English/game/`, and gives the next ID from `0x3000`. `[Must-match]`
- The same file name, without case, gives the same ID. An absent file, or a table past `0x3FFF`, gives -1 and uses no ID. `[Safe rule]`
- The table clears at a new game and at a load, so scripts rebuild it during setup in the same order. Nothing about it is saved. `[Safe rule]` Evidence: both RPU callers do this under `game_loaded`.

**World map and car.**

| Word | Mnemonic | Pop order | Push | Effect |
| --- | --- | --- | --- | --- |
| `0x81B6` | `set_car_current_town` | town | none | Sets `car_area` (SPEC-019 section 3.19.1). `[Must-match]` |
| `0x8171` | `force_encounter` | map | none | Forced encounter, flags 0. `[Must-match]` |
| `0x8229` | `force_encounter_with_flags` | flags, map | none | Forced encounter with flags. `[Must-match]` |

- **`set_car_current_town`** writes `car_area` whether or not `has_car` is 1. A number that is not an area in SPEC-018 section 4.3 changes nothing, with a diagnostic. `[Safe rule]`
- **`get_terrain_name`** (no arguments, or x and y) gives string. `[Must-match]` With no arguments it uses the party's cell. Otherwise x and y are world pixels, as for `metarule3` 105 (SPEC-018 section 5.3). `[Safe rule]`
  - The result is `worldmap.msg` message `1000 + terrain index`, where the index is the cell's position in `terrain_types` (SPEC-018 section 4.4). A point outside the world gives the empty string. `[Safe rule]`
  - Evidence: Et Tu compares the result with `worldmap.msg` 1001, 1002, and 1003 to detect mountain, city, and ocean, so a name taken from `worldmap.txt` would fail on a translated install.

**Forced encounters.** One encounter can be pending, with its map and flags. `[Safe rule]` except where tagged.

- The word stores the map and flags. A later call replaces them, unless the pending one has flag `0x2`, which then ignores the call. `[Must-match]`
- A map number with no row in SPEC-018 section 5.1 changes nothing, with a diagnostic. Map 0 is valid. Evidence: RPU calls `force_encounter(0)`.
- It may be called on the world map or on a local map. It takes effect at the next encounter check of SPEC-018 section 5.6 after the party moves on the world map. `[Must-match]`
- At that check, the pending encounter replaces steps 1 to 8: no random number is drawn, no counter changes, and no Outdoorsman check applies. `[Must-match]` for the check.
- The map loads through SPEC-018 section 5.7's step 1. No group is spawned.
- The pending encounter clears, and is saved in the sidecar until then (section 5.8). `HOOK_ENCOUNTER` does not fire (section 6.17).

| Flag | Name | Effect |
| ---: | --- | --- |
| `0x1` | `ENCOUNTER_FLAG_NO_CAR` | The car stays where it is. `[Must-match]` |
| `0x2` | `ENCOUNTER_FLAG_LOCK` | Blocks new forced encounters until this one occurs. `[Must-match]` |
| `0x4` | `ENCOUNTER_FLAG_NO_ICON` | No flashing icon (SPEC-019 section 3.12.7). `[Must-match]` |
| `0x8` | `ENCOUNTER_FLAG_ICON_SP` | The special icon. `[Must-match]` |
| `0x10` | `ENCOUNTER_FLAG_FADEOUT` | Fades to black as `gfade_out`. The map load does not fade back in, and a script restores it with `gfade_in`. `[Must-match]` |

**Virtual files.** A session holds a table of virtual files, each with a canonical path (SPEC-023 section 2.2), bytes, and a position. All of this is `[Safe rule]` unless tagged, because the published text is signatures plus one paragraph.

| Word | Mnemonic | Pop order | Push | Effect |
| --- | --- | --- | --- | --- |
| `0x81F8` | `fs_copy` | source, path | I | Handle, or -1 |
| `0x8202` | `fs_seek` | position, handle | none | Sets the position |
| `0x820A` | `fs_read_short` | handle | I | Next 16 bits |
| `0x81FB` | `fs_write_short` | value, handle | none | Writes 16 bits |

- **Precedence.** A virtual file takes precedence over the same path in the `Vfs` layers, for every later read in the session. `[Must-match]`
- **`fs_copy`** reads the source's bytes, from a virtual file if one exists, else through the `Vfs`. An invalid path or an absent source gives -1 and creates nothing.
  - On success the file at `path` holds those bytes, with position 0, and the call gives its handle. Handles start at 1 and rise.
  - An existing `path` keeps its handle. Evidence: RPU's `has_head` calls `fs_copy` on every conversation and treats only -1 as failure.
- **`fs_seek`.** A position from 0 to the file size, inclusive, is stored. Anything else leaves it, with a diagnostic.
- **`fs_read_short` and `fs_write_short`** are **big-endian**. Evidence: RPU writes a small frame rate straight into an FRM header field, and reads its frame count for a numeric test. FRM fields are big-endian (SPEC-001), so no other order gives those scripts their intended result.
  - A read sign-extends the 16 bits. A write stores the low 16 bits. Each advances the position by 2.
  - With fewer than 2 bytes left, a read gives 0, a write stores nothing, and the position stays, with a diagnostic.
  - An unknown handle is a failed operand (section 4.1).
- **Cached data.** A write drops any cached decode of that path, so the next load sees the new bytes.
- **Lifetime.** The table clears at a new game and at a load, and nothing about it is saved. Saving a virtual file takes `fs_resize(id, -1)`, which stays tier P2, so none is saved. Evidence: both RPU frame-rate patches run under `game_loaded`.
- The other `fs_*` words stay tier P2 and do not touch this table.

**Appearance.**

| Word | Mnemonic | Pop order | Push | Effect |
| --- | --- | --- | --- | --- |
| `0x8215` | `set_hero_style` | style | none | Stores the style |
| `0x8227` | `refresh_pc_art` | none | none | Rebuilds the player's art |

- The stored style is an int, 0 at session start. It is not saved. Evidence: Et Tu keeps its style in an sfall global and sets it again. `[Safe rule]`
- `refresh_pc_art` applies SPEC-005 section 3.8's player art rule again, at once, to the player's current armor and sex. It costs no AP. `[Safe rule]`
- **Limit.** Hero appearance archives are not defined here, so the stored style changes no art (Decision D-42). `[Safe rule]`

**Interface tags.**

| Word | Mnemonic | Pop order | Push | Effect |
| --- | --- | --- | --- | --- |
| `0x81DC` | `show_iface_tag` | tag | none | Sets the tag's script flag |
| `0x81DD` | `hide_iface_tag` | tag | none | Clears the tag's script flag |
| `0x81DE` | `is_iface_tag_active` | tag | I | 1 when the box is lit, else 0 |

| Tag | Box | `show` and `hide` | `is_iface_tag_active` |
| ---: | --- | --- | --- |
| 0 | SNEAK | Yes | Yes |
| 1, 2 | POISONED, RADIATED | No | Yes |
| 3, 4 | LEVEL, ADDICT | Yes | Yes |
| 5 to `4 + BoxBarCount` | Custom | Yes | Yes |

The tag numbers are `[Must-match]`. `BoxBarCount` comes from `ddraw.ini`, and is 5 when absent. Any other tag does nothing and reads as 0. `[Safe rule]`

- A box is lit when SPEC-019 section 3.5.2's condition holds, or its script flag is set. `hide_iface_tag` clears only the flag, so it cannot hide a box the engine has lit. `[Safe rule]` Evidence: RPU hides ADDICT only after testing that the player has no real addiction.
- Custom boxes follow the five named boxes, in ascending tag order, and SPEC-019's five-slot limit still applies. Tag t reads `intrface.msg` message `100 + t`. `[Safe rule]`
- Script flags are not saved, and clear at a new game and at a load. Evidence: RPU's repeating global scripts set them again. `[Safe rule]`

**Screen and windows.**

| Word | Mnemonic | Pop order | Push | Effect |
| --- | --- | --- | --- | --- |
| `0x8220` | `get_screen_width` | none | I | 640. `[Safe rule]` |
| `0x8221` | `get_screen_height` | none | I | 480. `[Safe rule]` |

- These are constants on every host. SPEC-005 window geometry is in 640 by 480 units, and a value taken from the host window would make scripts differ between machines and break replays. `[Safe rule]`
- **`create_win`** (name, x, y, width, height, optional flags) acts as `createwin` (SPEC-005, `0x8062`), and pushes int 0. `[Must-match]` Without flags, the flags are `0x4`. `[Must-match]` The flags are `0x2` do not move to the front, `0x4` on top of other windows, `0x8` hidden, `0x10` exclusive, and `0x20` transparent. `[Must-match]`
- Windows are presentation state, and do not affect the world. The sfall drawing functions stay tier P2, so a window from `create_win` shows only what SPEC-005's window words draw. `[Safe rule]`

**Dialogue queries.** `dialog_obj` (no arguments) gives the critter the player is talking to or bartering with (SPEC-006, SPEC-011), else null. `[Must-match]`

**`message_box`** (text, optional flags, color 1, color 2) gives int. `[Must-match]`

- 1 for Yes or Done (Enter), 0 for No (Escape), and -1 when no box can be created. `[Must-match]`
- `\n` starts a new line. Color 1 is the first line's palette index, and color 2 the other lines'. Both default to 145. Flags -1, or absent, mean `0x11`. `[Must-match]`
- The flags are `0x0` auto-size, `0x1` normal, `0x2` small, `0x4` align left, `0x8` align top, `0x10` Yes and No buttons instead of Done, and `0x20` no buttons. `[Must-match]`
- The box is SPEC-014 section 3.13's generic dialog box, as a modal screen. The program waits at the next instruction, and the result is pushed when the box closes. The world does not advance, as for any modal screen in SPEC-021. `[Safe rule]`
- Where a program cannot wait, the call gives -1 with no box: in a hook member (section 6.5), and in session setup (section 6.1). `[Safe rule]`
- The closing key comes from the input log, so a replay gives the same result. With `0x20` set, Enter and Escape still close the box. `[Safe rule]`

**`signal_close_game`** (no arguments) pushes int 0. `[Must-match]`

- It acts as `metarule` rule 13, then ends the session instead of returning to the main menu. `[Must-match]`
- The session ends after the current step's Simulation phase. `HOOK_GAMEMODECHANGE` then runs once with arg0 1. Play returns to the operating system with no prompt, and a headless run stops as a finished run. `[Safe rule]`
- Evidence: Et Tu's `gl_0.ssl` shows a box and calls this when sfall's major version is 5 or more, or when a required `ddraw.ini` key reads 0. Here the version is 4, and an absent `ddraw.ini` gives -1 rather than 0, so neither test closes the game. Its deliberate invalid-object call afterwards gives null by section 4.1.

**Keys.** `tap_key` (`0x8162`) pops a scan code and pushes nothing. `[Must-match]`

- The key is queued, and is pressed and released as a logical key at the start of the next phase 1, after that step's input records. `[Safe rule]`
- Scan codes are those of `HOOK_KEYPRESS`'s arg1. A value outside 1 to 255 does nothing, with a diagnostic. `[Safe rule]`
- It does not run `HOOK_KEYPRESS`, and is not written to the input log, because a replay queues it again. `[Safe rule]`

## 5. Part 2: Dynamic Arrays

### 5.1 Model

The array store belongs to the world, not to a program, and arrays are shared by all scripts. `[Must-match]` Each array has an ID, a **shape** (list or map), a **lookup flag** on maps, a **storage class**, an optional save key, and its elements.

Elements are ints, floats, and strings, mixed freely. A string is copied into the store as bytes, so it outlives the program that made it. `[Must-match]` An object reference is stored as its int; a stale one reads as null (SPEC-017 section 1). `[Safe rule]`

| Shape | Behavior |
| --- | --- |
| List | Elements at indices 0 to length - 1. An index outside that range is never created by assignment. `[Must-match]` |
| Map | Key and value pairs, with no fixed size. Keys may be any type. `[Must-match]` |
| Lookup map | A map whose existing values are read-only and which may store int 0. `[Must-match]` |

### 5.2 Array IDs

- IDs are ints starting at 1. Each new array takes the next ID, and 0 is never an ID. `[Must-match]` for 0 meaning none; `[Safe rule]` for the rest.
- An ID is never reused while a session lasts, even after `free_array`. `[Safe rule]`
- A new game empties the store and sets the next ID to 1. `[Safe rule]`
- A bad ID never stops a script. Each word's bad-ID result is in section 5.4. `[Must-match]`

### 5.3 Storage classes

| Class | Created by | Ends |
| --- | --- | --- |
| Temporary | `temp_array`, array expressions, and every word or function that returns an array, unless section 4 says permanent | End of the current frame |
| Permanent | `create_array`, or `fix_array` on a temporary array | `free_array`, new game, or load |
| Saved | `save_array` on any array | `free_array`, new game, or load, after which `load_array` finds it in the save |

`[Must-match]`

**A frame is one headless step** of SPEC-021 section 3.10: the temporary arrays alive at the end of phase 2 (Simulation) are freed there, before phase 3 (Checkpoint). So a checkpoint never sees one. A step that runs several world ticks is still one frame, and a modal screen that suspends the world still ends its frames. `[Safe rule]`

Loading a slot deletes every array first, then restores the saved ones under their stored IDs (section 5.8). A permanent array that was not saved is gone, as documented. `[Must-match]`

### 5.4 Word contracts

| Word | Mnemonic | Pop order | Push | Effect | Bad ID |
| --- | --- | --- | --- | --- | --- |
| `0x822D` | `create_array` | flags I, size I | I | New permanent array | n/a |
| `0x8233` | `temp_array` | flags I, size I | I | New temporary array | n/a |
| `0x8234` | `fix_array` | ID | none | Temporary becomes permanent; other classes unchanged | Ignored |
| `0x822E` | `set_array` | value A, key A, ID | none | Assignment, below | Ignored |
| `0x822F` | `get_array` | key A, ID | A | Value, or int 0 for an absent key or an index out of range | Int 0 |
| `0x8232` | `resize_array` | size I, ID | none | Resize or order operation, below | Ignored |
| `0x8230` | `free_array` | ID | none | Deletes the array, including from the save set | Ignored |
| `0x8231` | `len_array` | ID | I | Elements or pairs | -1 |
| `0x8239` | `scan_array` | value A, ID | A | First match: list index or map key, else int -1 | Int -1 |
| `0x8254` | `save_array` | ID, key A | none | Section 5.5 | Ignored |
| `0x8255` | `load_array` | key A | I | Section 5.5 | n/a |
| `0x8256` | `array_key` | index I, ID | A | Below | Int 0 |
| `0x8257` | `arrayexpr` | value A, key A | I | Section 5.7. Always int 0 | n/a |

Number, pop order, push, and each effect the documentation names are `[Must-match]`. **Note `save_array`'s order**: the key is the first argument and the ID the second, the opposite of every other array word.

The operand classes here are the `arg_classes` column's. Section 4.1 relates the Push column to `ret_class`.

**Creation.**

- `size >= 0` makes a list of that many int 0 values. `[Must-match]`
- `size == -1` makes a map, and `flags == 2` makes it a lookup map. `[Must-match]`
- `flags` is ignored for a list. **Compiler output passes nonzero values here**, such as `temp_array(4, 2)` for a list. `[Must-match]`
- Any other flags on a map give an ordinary map. `size < -1` makes no array and pushes 0. `[Safe rule]`
- Every creation records the array as the **last created**, for `arrayexpr`.

**Assignment on a list.** A numeric key in range replaces that element. A float key truncates toward zero. A key out of range or a string key is ignored. Assigning 0 stores 0. `[Must-match]`; `[Safe rule]` for float and string keys.

**Assignment on a map.**

| Case | Result |
| --- | --- |
| New key, value not int 0 | Pair appended at the end of iteration order. `[Must-match]`; `[Safe rule]` for the position |
| Existing key, value not int 0 | Value replaced; position unchanged. `[Must-match]`; `[Safe rule]` for the position |
| Value is int 0, key exists | Pair removed; later pairs move up one. `[Must-match]`; `[Safe rule]` for the shift |
| Value is int 0, key absent | Nothing changes. `[Must-match]` |
| Value is float 0.0 | Stored as a value. `[Must-match]` |

**Assignment on a lookup map.** An absent key is appended, even with int 0. An existing key does not change. `[Must-match]`

**`resize_array`.**

| Size | Operation |
| --- | --- |
| Above current length, list | Appends int 0 values. `[Must-match]` |
| Above current length, map | Nothing changes. `[Must-match]` |
| 0 up to current length | Keeps the first `size` entries of iteration order. `[Must-match]`; `[Safe rule]` for which entries |
| -2 | Sort ascending; a list by value, a map by key. `[Must-match]` |
| -3 | Sort descending; a list by value, a map by key. `[Must-match]` |
| -4 | Reverse the order. `[Must-match]` |
| -5 | Shuffle, section 5.6. `[Must-match]` |
| -6 | Sort a map ascending by value. `[Must-match]`. On a list it acts as -2. `[Safe rule]` |
| -7 | Sort a map descending by value. `[Must-match]`. On a list it acts as -3. `[Safe rule]` |
| -1, or below -7 | Nothing changes, with a diagnostic. `[Safe rule]` |

**`array_key`.** With index -1 it reports the shape: 0 list, 1 map. For a list it returns the index unchecked. For a map it returns the key at that iteration position, or int 0 outside the range. `[Must-match]`; `[Safe rule]` for out of range.

**Caution kept as published.** `scan_array`'s -1 is ambiguous on a map with a -1 key. `[Must-match]`

**A string value.** `scan_array`'s value has class `A`, so a string is searched like any other value, by section 5.6's equality (Decision D-46, vector SS1). `[Must-match]`

### 5.5 Save keys

`save_array(key, ID)`:

- Any key other than int 0 makes the array saved under that key, and a temporary array becomes permanent first. `[Must-match]`
- Key int 0 makes a saved array permanent again. On an array that is not saved, it does nothing. `[Must-match]`
- A key held by another array moves to this one; the other array stays permanent. An array holds one key, so a new key replaces its old key. `[Safe rule]`

Save keys compare like map keys (section 5.6), so int 0 is the only unsave key; float 0.0 is an ordinary key. `[Safe rule]`

`load_array(key)` gives the ID of the array holding the key, or 0. This also finds a live saved array before any game load. `[Must-match]`

**Reserved key.** `load_array("...all_arrays...")` gives a new temporary list of all saved keys, ascending by the order of section 5.6. `[Must-match]`; `[Safe rule]` for a key that is not a string.

### 5.6 Determinism

The documentation says not to rely on key order. Determinism requires one anyway, so this order is defined: `[Safe rule]`

1. **Iteration order is the array's element order.** A list runs by index. A map runs in the order the rules above produce: first insertion, unchanged by updates and by later removals, until an order operation rewrites it.
2. **No operation may depend on** a hash-map's iteration, a memory address, a pointer value, the wall clock, or a locale. A map may keep an index for lookup, but iteration, `array_key`, `scan_array`, sorting, and serialization must follow the element order.
3. `foreach` compiles to `len_array`, then `get_array` or `array_key` by position, so a change during a loop takes effect at the next position. The rules above define the outcome.

**Key and value equality.** Two values are equal when they have the same type and the same content: int by value, float by its 32 bits, string by exact bytes. So `1` and `1.0` are distinct keys, `0.0` and `-0.0` are distinct, and a NaN equals itself. `[Safe rule]`

**Sort order.** A total order: numbers first, then strings. Numbers compare by value; equal values put the int first. NaN sorts after every other number. Strings compare as unsigned bytes, case-sensitive. A stable sort is used, and descending order is the reverse of the comparison. `[Safe rule]`

**Shuffle.** For i from length - 1 down to 1, draw j uniformly in 0 to i, inclusive, from the SPEC-006 gameplay generator, and swap positions i and j. It therefore draws length - 1 numbers. A length below 2 draws none. `[Safe rule]`

### 5.7 Compiler-generated words

The compiler lowers array syntax to these words, so a compiled script contains them directly. `[Must-match]`

| Source form | Words used |
| --- | --- |
| `list[5] := v` | `set_array` |
| `display_msg(list[5])` | `get_array` |
| `m.name` | `get_array` or `set_array` with a string key |
| `[a, b]`, `{k: v}` | `temp_array`, then `arrayexpr` per element |
| `foreach` | `len_array`, `array_key`, `get_array` |

`arrayexpr(key, value)` assigns to the last created array, with the section 5.4 rules, and pushes int 0. If that array no longer exists, it does nothing and still pushes 0. `[Must-match]`; `[Safe rule]` for the missing array.

The last-created record is not saved, and a load clears it. `[Safe rule]`

### 5.8 The save sidecar

This is a vault format, deliberately **not** byte-compatible with sfall's `sfallgv.sav`, whose layout is unpublished (Decision D-15). All of section 5.8 is `[Safe rule]`, except the migration rules, which carry their own tags.

**File.** `VAULTSFL.DAT`, beside the slot's `SAVE.DAT`, through the storage path that SPEC-023 gives other slot files.

- A save under sfall mode **always** writes it, even when empty.
- A save with sfall mode off removes any old one, as for `VAULTENG.TOML`. This is one named delete in that save's commit. SPEC-023 section 3.11's stale `.SAV` sweep never touches this file (Decision D-29).
- `SAVE.DAT` itself is unchanged.

| Offset | Bytes | Content |
| ---: | --- | --- |
| 0 | 4 | Tag `VSFL` |
| 4 | 4 | u32 format revision, 2 |
| 8 | 4 | u32 next array ID |
| 12 | 4 | u32 next unique object ID, starting at `0x10000000` (section 4.11) |
| 16 | 4 | i32 pending forced-encounter map, or -1 for none (section 4.11) |
| 20 | 4 | u32 pending forced-encounter flags, 0 for none |
| 24 | variable | seq(global record) |
| then | variable | seq(array record) |

**Global record:** key value, then stored value. Ascending by the section 5.6 order of the keys.

**Array record:** ascending by ID. Only saved arrays appear in the sidecar.

| Field | Bytes | Content |
| --- | --- | --- |
| ID | 4 | u32 |
| Shape | 1 | u8: 0 list, 1 map |
| Lookup | 1 | u8: 0 or 1 |
| Storage | 1 | u8: 0 temporary, 1 permanent, 2 saved |
| Reserved | 1 | u8: 0 |
| Save key | variable | opt(value) |
| Count | 4 | u32 elements or pairs |
| Elements | variable | Iteration order. A list writes value; a map writes key value, then value |

**Value:**

| Type byte | Payload |
| ---: | --- |
| 1 | i32 |
| 2 | u32 holding the binary32 bits unchanged |
| 3 | u32 byte length, then the bytes, with no terminator |

The type bytes equal `typeof`'s results.

**Loading.** The sidecar is parsed whole and validated before any working file changes, with SPEC-018 section 7.8's order.

| Situation | Result |
| --- | --- |
| Sfall mode on, revision 2 sidecar present and valid | Restores globals, arrays, the next array ID, the next unique ID, and the pending forced encounter |
| Sfall mode on, revision 1 sidecar present and valid | Migrates, as below |
| Sfall mode on, no sidecar | Empty state, next array ID 1, next unique ID `0x10000000`, no pending encounter. A vanilla save can start using sfall |
| Sfall mode off, sidecar of either revision present | Refuses the slot, with an explicit message: its programs were written for other word meanings |
| Bad tag, a revision other than 1 or 2, truncation, trailing bytes, unknown type byte, a duplicate ID, or a shape byte outside 0-1 | Refuses the slot, with an explicit message. Saved data is never dropped silently |

**Migrating a revision 1 sidecar (Decision D-32).** The engine has written format revision 1 since this specification's Revision 1, so revision 1 sidecars exist beside players' saves. Revision 1 differs from revision 2 only in its header, which stops at offset 12. Its global, array, and value records are the ones above.

| Offset | Bytes | Content |
| ---: | --- | --- |
| 0 | 4 | Tag `VSFL` |
| 4 | 4 | u32 format revision, 1 |
| 8 | 4 | u32 next array ID |
| 12 | variable | seq(global record) |
| then | variable | seq(array record) |

A reader checks the tag, then the revision, and only then chooses the layout.

1. A revision 1 sidecar loads. Everything it holds carries over unchanged: the next array ID, every global, and every array with its ID, shape, lookup byte, storage, save key, and element order. `[Must-match]`
2. Each field that revision 2 adds takes this default on migration. These three are the only fields it adds. `[Must-match]`

   | Field | Default on migration |
   | --- | --- |
   | Next unique object ID | `0x10000000`, where a new game starts it |
   | Pending forced-encounter map | -1, so no encounter is pending |
   | Pending forced-encounter flags | 0 |

3. `sfall_hash` after the load is the hash of the migrated revision 2 state (section 5.9), never a hash of the revision 1 bytes. `[Must-match]`
4. The next save writes revision 2. The engine never writes revision 1 again. `[Must-match]`
5. A load rewrites no file, so the slot keeps its revision 1 sidecar until that next save. `[Safe rule]`
6. A missing sidecar keeps the rule in the table above. A truncated or oversized revision 1 sidecar is refused as revision 1 refuses it today. Any revision other than 1 and 2 is refused with the existing unknown-revision error. `[Safe rule]`

Vectors MG1 and MG2 pin these rules.

### 5.9 `sfall_hash` and SPEC-021

**`WorldState::state_hash` does not change.** The stream stays `VSH1`, and no golden is re-recorded. SPEC-021 section 2.3 requires a new magic for any change to coverage, and arrays add nothing that vanilla replays can see. `[Safe rule]`

`state_hash` stays deterministic because arrays can only reach covered state through scripts, and section 5.6 makes every script-visible array result deterministic. `[Must-match]` for that requirement.

Array divergence that has not yet reached covered state is caught by a companion: `[Safe rule]`

- `WorldState::sfall_hash(&self) -> u64` is `fnv1a64` (SPEC-021 section 2.1) of the **extension stream**.
- That stream is the section 5.8 layout with tag `VSX1`, and with **every live array**, of any storage class. The storage byte is significant.
- It obeys SPEC-021 section 3.3's rules: it takes `&self`, draws no random number, and changes no state.
- With an empty store and no globals, it is the fixed 32-byte stream of vector F3.
- After a revision 1 sidecar loads, it is the stream of the migrated state, in revision 2's layout with section 5.8's defaults (vector MG1). So an empty revision 1 sidecar gives vector F3's value. `[Must-match]`

Hook registrations, global-script counters, open lists, the last-created record, virtual files, added message files, interface tag flags, and the stored hero style are not covered, because scripts rebuild them. `[Safe rule]`

## 6. Part 3: Hook Scripts

### 6.1 Discovery and session setup

A **hook script** is a `.INT` under `scripts/` named as in `sfall-hooks.tsv`. The name is compared without case. `[Must-match]`

After a new game or a slot load finishes (map loaded, sidecar restored), and before the first phase 2 that follows, the engine runs this setup once. `[Safe rule]` for the order and the moment. The published note that `HOOK_CALCAPCOST` misses the initial interface draw supports running it after the load.

1. Clear the hook registry.
2. For each hook ID ascending, if the file exists, load it and run its `start`, with `init_hook` giving 1. A hook file shared by two IDs loads once.
3. For each global script, in section 4.9 order, run `start`.

Hook-script instances keep their program state between runs. Setup runs again on each load. `[Must-match]`

A script refused by the mod-script check takes SPEC-006 section 3.7's cannot-load path and is not part of any chain. `[Safe rule]`

**Published defect.** Because of this timing, the first interface draw can show a wrong AP cost. `[May-improve]`: the engine requests a preview (section 6.6) at the end of setup, so the first frame is already right.

### 6.2 Registration

| Word | Mnemonic | Pop order | Push | Effect |
| --- | --- | --- | --- | --- |
| `0x8207` | `register_hook` | hook ID | none | Registers the calling script's `start` |
| `0x8262` | `register_hook_proc` | procedure P, hook ID | none | Registers that procedure; procedure 0 unregisters |
| `0x827D` | `register_hook_proc_spec` | procedure P, hook ID | none | Registers at the end of the chain |

`[Must-match]`

- A script has at most one registration per hook. Registering again changes its procedure and keeps its position. `[Safe rule]`
- Procedure 0 removes the calling script's registration for that hook. `[Must-match]`
- A call from a script that is neither a global script nor a hook script is ignored, with a diagnostic. `[Safe rule]`
- A hook ID without a row in `sfall-hooks.tsv`, or in 49 to 60, is ignored, with a diagnostic. `[Safe rule]`
- Registration is not saved. Scripts register again during setup. `[Must-match]`

### 6.3 The chain

For one hook, members run in this order: `[Must-match]`

1. Procedures from `register_hook` and `register_hook_proc`, **newest first**.
2. The `hs_<name>.int` script's `start`.
3. Procedures from `register_hook_proc_spec`, **oldest first**.

An empty chain costs nothing and changes nothing. `[Must-match]`

### 6.4 Hook context

Running a chain creates one context, holding the argument array, the return array, and a set flag per return. `[Must-match]`

| Word | Mnemonic | Pop order | Push | Inside a chain | Outside a chain |
| --- | --- | --- | --- | --- | --- |
| `0x81E4` | `get_sfall_arg` | none | A | Next argument; advances the cursor. Int 0 past the end | Int 0 |
| `0x823C` | `get_sfall_args` | none | I array | New temporary list of all current arguments | Empty temporary list |
| `0x823D` | `set_sfall_arg` | value A, index I | none | Replaces argument `index`, 0-based | Ignored |
| `0x81E5` | `set_sfall_return` | value A | none | Writes the next return; does nothing once all are written | Ignored |
| `0x81EA` | `init_hook` | none | I | 0 | 1 during setup, else 0 |

`[Must-match]`; `[Safe rule]` for the "Outside a chain" column. The operand classes here are the `arg_classes` column's, and section 4.1 relates the Push column to `ret_class`.

`get_sfall_arg_at(n)` returns argument n without moving the cursor, and int 0 when n is out of range. `[Must-match]`

### 6.5 Composition

These rules define how members compose. `[Must-match]` They follow the published chain description and its worked example.

1. **Each member starts fresh.** Its argument cursor and its return cursor both begin at 0.
2. **Arguments carry forward.** `set_sfall_arg` changes what later members read.
3. **Returns carry forward.** A return one member sets stays set, and a later member overrides it only by writing that index.
4. **Last writer wins, per index.**
5. **Only returns reach the engine.** A changed argument never changes the engine's value by itself.
6. **An unset return leaves the engine's value** in place, and a hook needs no return to run.

The published example chains "divide by 2" and "multiply by 1.5" on a hit chance of 100. With both members also writing `set_sfall_arg`, the result is 75 in either registration order (vectors H1 and H2).

Further rules, `[Safe rule]`:

- `set_sfall_arg` takes any value (class `A`, Decision D-46), then converts it to the argument's declared type: `int`, `critter`, `item`, and `obj` by class `I`, and `mixed` unchanged. A value that cannot convert leaves the argument unchanged, with one diagnostic. So a string converts only to a `mixed` argument, because class `I` never parses a string, even one made of digits (vector SS2). An index out of range is ignored.
- A return is converted by the hook's declared return type, the same way. A value that cannot convert leaves that return unset, with a diagnostic.
- A member that ends in a VM error is unloaded from every chain, and its writes up to that point stay. The chain continues with the next member.
- A member runs to completion, by SPEC-004 section 3.7's synchronous call. A word that would suspend it (a dialogue, a wait, or a timer yield) ends that member, with a diagnostic.
- Chains nest when a member causes another hook. Each chain has its own context, and the outer one resumes untouched. Past **8 nested chains**, a chain is skipped with a diagnostic and the engine's values stand.
- `self_obj` is null in a member, unless `set_self` changes it.

### 6.6 Phases and previews

Hooks must not break two engine rules: drawing never changes the world (SPEC-022 section 3.13), and the same inputs give the same states (SPEC-021 section 3.10). All of section 6.6 is `[Safe rule]`.

- **A chain runs only in phase 1 (Input), in phase 2 (Simulation), or in session setup (section 6.1)**, never in phase 3 (Checkpoint) or 4 (Presentation).
- **A chain runs once per engine query**, even if an identical query came just before. Query order is the order of the host specs. A chain may draw random numbers: the draws are part of the deterministic sequence, because the queries are.
- **Preview queries.** sfall also runs hooks for values that are only displayed, such as the to-hit percentage under the targeting cursor or the AP cost on the interface bar. Here, such a value is queried in phase 2 from the state of that step, and presentation reads the stored result.
  - A preview runs at most once per step, and only when its inputs differ from the previous step's.
  - A preview whose inputs are not reachable from the input records shows the unhooked value.

This differs from sfall in how many times a hook fires (Decision D-17). A script that counts firings can notice; a script that only computes a value cannot.

### 6.7 Common numbering

These agree with the engine's own tables. `[Must-match]` The argument and return tables of sections 6.8 to 6.18 are also `[Must-match]` unless a row says otherwise.

| Hook value | Engine value |
| --- | --- |
| Attack type 0 to 7 | SPEC-007 hit mode 0 to 7 |
| Attack type 8 to 19 | Named unarmed attacks, which SPEC-007 maps to punch or kick. They are passed as stored |
| Body part 0 to 8 | SPEC-007 hit location 0 to 8 |
| Roll 0 to 3 | SPEC-007 section 3.6 result: critical failure, failure, success, critical success |
| Special-effect flags | SPEC-007 damage flag bits, unchanged |

### 6.8 `HOOK_TOHIT` (0, `hs_tohit.int`)

**Boundary.** Runs after SPEC-007 section 3.5 step 10 (the upper cap of 95), and before any use of the result: the section 3.6 roll, section 3.7 recomputations, AI hit-chance tests, and previews. It runs for both the range-aware and no-range calculations, and for AI evaluation of a hypothetical fire position. `[Must-match]`

| Index | Type | Value |
| ---: | --- | --- |
| arg0 | int | Hit chance after the cap |
| arg1 | critter | Attacker |
| arg2 | critter | Target |
| arg3 | int | Body part |
| arg4 | int | Source hex; can differ from the attacker's hex during AI evaluation |
| arg5 | int | Attack type |
| arg6 | int | 1 for the range-aware calculation, else 0. It does not mean the attack is ranged |
| arg7 | int | Hit chance before the cap |

| Return | Overrides | Limits |
| --- | --- | --- |
| ret0 | The hit chance | Clamped to -99 through 999. `[Must-match]` |

### 6.9 `HOOK_AFTERHITROLL` (1, `hs_afterhitroll.int`)

**Boundary.** Runs when the attack result is final, including the Jinxed, Slayer, and Sniper post-processing of SPEC-007 section 3.6, and before any critical effect is applied and before section 3.7.1. `[Must-match]` for "after the hit or miss decision"; `[Safe rule]` for the placement after post-processing. It runs once per attack, including once for a burst, on its single hit-chance roll. `[Safe rule]`

| Index | Type | Value |
| ---: | --- | --- |
| arg0 | int | Roll, 0 to 3 |
| arg1 | critter | Attacker |
| arg2 | critter | Target |
| arg3 | int | Body part |
| arg4 | int | Hit chance |

| Return | Overrides | Limits |
| --- | --- | --- |
| ret0 | The roll | 0 to 3, else ignored. `[Safe rule]` |
| ret1 | The body part | 0 to 8, else ignored. `[Safe rule]` |
| ret2 | The target | A live critter, else ignored. `[Safe rule]` |

Later steps use the overridden values. A roll changed to a hit calculates damage, and a changed target becomes the attack's defender for the rest of the attack, including `HOOK_COMBATDAMAGE`. `[Safe rule]`

### 6.10 `HOOK_CALCAPCOST` (2, `hs_calcapcost.int`)

**Boundary.** Runs at the end of every SPEC-007 section 3.4 item-cost query, on its final value: after the weapon or punch base, the player reductions, the aim cost, and the clamp to at least 1, and likewise for the 2 AP reload and generic-use results. It does not run for movement. `[Must-match]` It runs before the value is compared with the critter's AP or debited.

| Index | Type | Value |
| ---: | --- | --- |
| arg0 | critter | Critter acting |
| arg1 | int | Attack type |
| arg2 | int | 1 for an aimed attack, else 0 |
| arg3 | int | Default AP cost |
| arg4 | item | Weapon, or 0 |

| Return | Overrides | Limits |
| --- | --- | --- |
| ret0 | The AP cost | Negative becomes 0. `[Safe rule]` |

The debit still saturates at zero. A return of 0 is free; the SPEC-007 clamp to 1 does not apply after the hook. `[Safe rule]`

### 6.11 `HOOK_COMBATDAMAGE` (5, `hs_combatdamage.int`)

**Boundary.** Runs when the attack's damage has been calculated and before it is applied, in the phase SPEC-007 section 3.6 calls resolution, which is **before the attack animation**. Nothing has been debited yet: not HP, ammunition, or AP. It runs once per victim, the primary target first, then each burst or splash extra in SPEC-007 section 3.7 order. It also runs when the AI tests whether an area attack is safe for allies, on that simulated result. It does not run for a miss, or for non-combat damage such as a dynamite explosion. `[Must-match]`; `[Safe rule]` for the extras' order.

| Index | Type | Value |
| ---: | --- | --- |
| arg0 | critter | Target |
| arg1 | critter | Attacker |
| arg2 | int | Damage to the target |
| arg3 | int | Damage to the attacker; 0 for an extra. `[Safe rule]` for extras |
| arg4 | int | Target's special-effect flags |
| arg5 | int | Attacker's special-effect flags; 0 for an extra. `[Safe rule]` for extras |
| arg6 | item | Weapon |
| arg7 | int | Body part struck |
| arg8 | int | Damage multiplier M, in halves: 2 is normal, 3 is 1.5 times, 8 is 4 times |
| arg9 | int | Rounds that hit; 1 for melee |
| arg10 | int | Knockback to the target |
| arg11 | int | Attack type |
| arg12 | mixed | Int 0 in this revision. `[Safe rule]`; Decision D-25 |

| Return | Overrides | Limits |
| --- | --- | --- |
| ret0 | Damage to the target | Negative becomes 0. `[Safe rule]` |
| ret1 | Damage to the attacker | Negative becomes 0. `[Safe rule]` |
| ret2 | Target's special-effect flags | 32 bits, as given |
| ret3 | Attacker's special-effect flags | 32 bits, as given |
| ret4 | Knockback to the target | Negative becomes 0. `[Safe rule]` |

Later application uses the returned values, with its SPEC-007 guards and persistent-status mask unchanged. `[Must-match]`

### 6.12 `HOOK_BARTERPRICE` (10, `hs_barterprice.int`)

**Boundary.** Runs after step 4 of SPEC-011 section 3.18 yields Price, and before section 3.19's comparison or any display. On entering barter and after a transaction, it runs twice: first for the player, then for the NPC. `[Must-match]`

| Index | Type | Value |
| ---: | --- | --- |
| arg0 | critter | Player-side barterer |
| arg1 | critter | Critter being bartered with |
| arg2 | int | Price |
| arg3 | critter | Merchant offer table T |
| arg4 | int | C_T: caps on that table |
| arg5 | int | V_T: its value before skill modifiers. `[Safe rule]` for choosing V_T |
| arg6 | critter | Player offer table P |
| arg7 | int | V_P: the value of the player's offer |
| arg8 | int | 1 if the offer control was pressed and this is not a party member, else 0 |
| arg9 | int | 1 if trading with a party member, else 0 |

| Return | Overrides | Limits |
| --- | --- | --- |
| ret0 | Price | -1 leaves it unchanged. `[Must-match]` Any other negative value becomes 0. `[Safe rule]` |
| ret1 | V_P, for SPEC-011 section 3.19 step 3 and display | Negative becomes 0. `[Safe rule]` |

### 6.13 `HOOK_MOVECOST`, `HOOK_INVENTORYMOVE`, and `HOOK_KEYPRESS`

**`HOOK_MOVECOST` (11, `hs_movecost.int`).**

**Boundary.** Runs whenever hex count becomes an AP cost, after the SPEC-007 section 3.4 per-hex rate (1, 4, or 8) is applied, and before the free-movement pool or AP are spent. A committed step runs it with arg1 = 1, so an NPC's combat movement fires it per hex, as the public guidance relies on. A planning query for N hexes runs it with arg1 = N. It runs only where movement costs AP. `[Must-match]`; `[Safe rule]` for "only where movement costs AP".

| Index | Type | Value |
| ---: | --- | --- |
| arg0 | critter | Critter moving |
| arg1 | int | Hexes |
| arg2 | int | Original AP cost |

| Return | Overrides | Limits |
| --- | --- | --- |
| ret0 | The AP cost | Negative becomes 0. `[Safe rule]` |

**`HOOK_INVENTORYMOVE` (24, `hs_inventorymove.int`).**

**Boundary.** Runs when a drag in the player's inventory (SPEC-014 section 3.34) resolves to a target, before the move and before any validity test. The hook can only prevent a move. It cannot force an item into an invalid slot. `[Must-match]`

Dropping a stack on the ground runs it once per item. For caps it runs once only, and for ammunition each run covers one pack. `[Must-match]`

| Index | Type | Value |
| ---: | --- | --- |
| arg0 | int | Target: 0 backpack, 1 left hand, 2 right hand, 3 armor, 4 weapon being reloaded by dropping ammunition, 5 container, 6 ground, 7 pickup, 8 character portrait |
| arg1 | item | Item moved |
| arg2 | item | Item replaced, weapon reloaded, or container filled; or 0 |

| Return | Overrides | Limits |
| --- | --- | --- |
| ret0 | -1 or unset: engine handler. Any other value: the move is prevented | None |

A prevented move leaves the item where it was, as a refused drop does, with no AP charged. `[Safe rule]`

**`HOOK_KEYPRESS` (19, `hs_keypress.int`).**

**Boundary.** Runs in phase 1, once for each physical press and each release, before any game or UI handling of the key. It does not run for engine key repeat. `[Must-match]`; `[Safe rule]` for excluding repeat.

| Index | Type | Value |
| ---: | --- | --- |
| arg0 | int | 1 pressed, 0 released |
| arg1 | int | DirectInput scan code: set-1 make code, with `0x80` added for extended keys. A is 30 |
| arg2 | int | Windows virtual-key code. A is 65 |

| Return | Overrides | Limits |
| --- | --- | --- |
| ret0 | The key seen by the game, as a scan code | 0 means no override |

- The input log records the **physical** key, so a replay runs the hook again and gets the same logical key. `[Safe rule]`
- The documentation tells scripts to return the same override for press and release. As a guard against stuck keys, releasing a physical key also releases whatever its press produced (vector H11). `[Safe rule]`

### 6.14 `HOOK_GAMEMODECHANGE` (31, `hs_gamemodechange.int`)

**Boundary.** Runs once when the `get_game_mode` bit set (section 4.4) has changed. `[Must-match]` The value is compared once per step, at the end of phase 2 and before temporary arrays are freed. Changes that cancel out inside one step run nothing, and several in a row run it once. The first value is the one at the end of session setup, which runs nothing. `[Safe rule]`

| Index | Type | Value |
| ---: | --- | --- |
| arg0 | int | 1 when the player exits the game, else 0 |
| arg1 | int | The previous game mode |

It has no returns. Members read the new mode with `get_game_mode`. `signal_close_game` runs it with arg0 1, even when the mode did not change. Returning to the main menu is not exiting the game. `[Safe rule]`

### 6.15 `HOOK_USEOBJON` (8, `hs_useobjon.int`) and `HOOK_USEOBJ` (18, `hs_useobj.int`)

**`HOOK_USEOBJON` boundary.** Runs before the item is used and before any `use_obj_on_p_proc` request, when: `[Must-match]`

1. A critter uses an item on a critter or on itself: at SPEC-006 section 3.8's interaction stage, before its routing list, and before SPEC-013 section 3.13's kit route.
2. A critter uses an inventory item that has no Use action flag and is not an active flare or explosive. The target is the user.
3. The player or an AI critter uses a drug (SPEC-011).

Using an item on a target that is not a critter does not run it. AP charged before the interaction stage stays charged. `[Safe rule]`

| Index | Type | Value |
| ---: | --- | --- |
| arg0 | critter | The target |
| arg1 | critter | The user |
| arg2 | int | The item used, as an object reference |

**`HOOK_USEOBJ` boundary.** Runs before the item is used and before its `use_p_proc` request, when a critter uses an inventory item that has the Use action flag or is an active flare or Dynamite, and when the player uses an object from the main interface. `[Must-match]` It runs before SPEC-013 section 3.10's arming, and before the drug path. `[Safe rule]` for the order.

| Index | Type | Value |
| ---: | --- | --- |
| arg0 | critter | The user |
| arg1 | obj | The object used |

Both hooks return the same value.

| Return | Overrides | Limits |
| --- | --- | --- |
| ret0 | 0: place it back. 1: remove it. -1 or unset: engine handler | Any other value counts as unset. `[Safe rule]` |

- 0 ends the action: no procedure is requested, no default runs, and the item stays where it is. `[Must-match]`
- 1 ends the action the same way, and destroys the item, as SPEC-006 section 3.8's wrapper does when an operation returns 1. `[Must-match]`
- **Published defect.** The documentation says removing or destroying the item inside the hook crashes. `[May-improve]`: a member's destroy request for the used item is ignored, with a diagnostic. It returns 1 instead.

### 6.16 `HOOK_STDPROCEDURE` (40, `hs_stdprocedure.int`)

**Boundary.** Runs when the host requests a named procedure of a script (SPEC-004 section 3.7), for the script of any object, before the procedure runs, after the source, target, and fixed parameter are installed. `[Must-match]`; `[Safe rule]` for the exact moment.

It never runs for `start` (event 1), `critter_p_proc` (12), `timed_event_p_proc` (22), or `map_update_p_proc` (23). `[Must-match]` It does not run when the script has no such procedure. `[Safe rule]`

| Index | Type | Value |
| ---: | --- | --- |
| arg0 | int | The event number, as `script_action` reports it (SPEC-004 section 3.7) |
| arg1 | obj | The object that owns the procedure (`self_obj`) |
| arg2 | obj | The caller (`source_obj`), or 0 |
| arg3 | int | 0. The value 1 belongs to ID 41, which stays tier P2 |
| arg4 | obj | The object acted upon (`target_obj`), or 0 |
| arg5 | int | The fixed parameter |

| Return | Overrides | Limits |
| --- | --- | --- |
| ret0 | -1 cancels the procedure | Any other value lets it run. `[Must-match]` |

A cancelled procedure counts as run with no override, so the action's default still follows (SPEC-006 section 3.8). `[Safe rule]`

### 6.17 `HOOK_ENCOUNTER` (43, `hs_encounter.int`)

**Boundary.** `[Must-match]` for the two events.

- **Event 0.** Runs when SPEC-018 section 5.6 selects a random encounter, after its step 8 and before the flash and the map load.
- **Event 1.** Runs when the player enters a local map from the world map: for SPEC-018 section 5.7's dropping to terrain, and for entering an area, before the map loads.

It does not run for a forced encounter, or for any other scripted one. `[Must-match]` Evidence: Et Tu's handler itself calls `force_encounter_with_flags` and returns -1, which works only if the forced encounter does not run the handler again.

| Index | Type | Value |
| ---: | --- | --- |
| arg0 | int | Event type: 0 random encounter, 1 entry from the world map |
| arg1 | int | The map that will load |
| arg2 | int | 1 for a Special entry (SPEC-018 section 4.4), else 0 |
| arg3 | int | Encounter table number, or -1 for event 1 |
| arg4 | int | Entry number in the table, or -1 for event 1 |

| Return | Overrides | Limits |
| --- | --- | --- |
| ret0 | The map. For event 0, -1 cancels the encounter and travel continues | For event 1, -1 is ignored. `[Must-match]`; `[Safe rule]` for ignoring it. A map with no SPEC-018 section 5.1 row is ignored, with a diagnostic. `[Safe rule]` |
| ret1 | 1 cancels the encounter and loads ret0's map, for event 0 | Any other value does nothing. `[Must-match]` |

- A replaced map still spawns the entry's groups, unless ret1 is 1. `[Safe rule]`
- Draws and counter changes already made by SPEC-018 section 5.6 stand when the hook cancels. `[Safe rule]`
- SPEC-018 section 5.6's statement that every selected encounter is entered holds only with no member cancelling it. `[Must-match]`

### 6.18 `HOOK_SETGLOBALVAR` (29, `hs_setglobalvar.int`)

**Boundary.** Runs in `set_global_var` (`0x80C6`), after the index is found valid and before the value is stored. `[Must-match]`; `[Safe rule]` for the order. An invalid index runs nothing. Writes the engine itself makes to a global do not run it. `[Safe rule]`

| Index | Type | Value |
| ---: | --- | --- |
| arg0 | int | The index of the global variable |
| arg1 | int | The value being set |

| Return | Overrides | Limits |
| --- | --- | --- |
| ret0 | The value stored | None |

A member that calls `set_global_var` runs a nested chain, within section 6.5's depth limit. Past that limit the nested write still stores its value, and only its chain is skipped. `[Safe rule]` Evidence: Et Tu's handler sets other globals from inside the hook.

### 6.19 The other 36 hooks

Tier P2 hooks are listed with full argument and return types in `sfall-hooks.tsv`. Their scripts load, register, and run during setup, but **no chain runs**, because their engine boundaries are not specified here. The engine's own behavior stands. `[Safe rule]`

Each must get its own boundary text before it fires. IDs 49 to 60 are reserved. `[Must-match]` `hs_stdprocedure.int` serves both IDs 40 and 41. `[Must-match]`

## 7. Synthetic Test Fixtures

All data is synthetic. `oracle/SPEC-024/README.md` explains the files.

| File | Coverage |
| --- | --- |
| `sfall-opcodes.tsv` | All 300 words, 133 `sfall_funcN` rows, 7 `metarule3` rows, 9 `metarule2_explosions` rows |
| `sfall-hooks.tsv` | All 50 hooks and the reserved range |
| `sfall-array-vectors.tsv` | Arrays (A to F3), hook chains (H1 to H17), and the promoted rows: virtual files (V1 to V4), unique IDs (U1), message files (M1), interface tags (T1), and explosive damage (E1); sidecar migration (MG1, MG2); and string values (SS1, SS2). All are computed by a reference model. MG and SS are not M and S because M1 already names message files here, and S1 and S2 name stack vectors in section 7.1 |
| `mod-opcode-usage.tsv` | The ranked mod audit of section 1.1. It is evidence, not a test input |

### 7.1 Stack vectors

| Vector | Stack before, top right | Word | Stack after | Rule |
| --- | --- | --- | --- | --- |
| S1 | `"string_find", "abc", "c"` | `0x8278` | `I:2` | `a2` is popped first |
| S2 | `"no_such_fn", I:1` | `0x8277` | `I:0` | Unknown name, with a diagnostic |
| S3 | `"set_flags", I:0, I:1` | `0x8278` | `I:0` | A null object does nothing, and one value is still pushed |
| S4 | `"traps", I:7` | `0x8254` | empty | `save_array`: the key is below the ID |
| S5 | `I:1, "x", I:9` | `0x822E` | empty | `set_array`: 3 pops, 0 pushes |
| S6 | `I:1, I:0` | `0x822F` | `I:0` | `get_array` of an absent element |
| S7 | `I:-1, I:0` | `0x822D` | `I:1` | `create_array`: flags on top |
| S8 | `"abc", "x", I:0` | `0x824E` | `""` | `substr` with a string start: 3 pops, the default pushed |
| S9 | `I:5`, then `0x8252` | `0x8252` | `I:5`, VM error | Reserved word |
| S10 | `I:7, I:2` | `0x827F` | `I:3` | Unsigned division |
| S11 | `I:-1, I:2` | `0x827F` | `I:2147483647` | `0xFFFFFFFF` divided by 2 |
| S12 | empty | `0x8220` | `I:640` | `get_screen_width` is a constant |
| S13 | `"dialog_obj"` | `0x8276` | `I:0` | Outside dialogue: null, and one value is pushed |
| S14 | `"set_unique_id", I:0` | `0x8277` | `I:0` | A null object |
| S15 | `"art/none.frm", "art/none.frm"` | `0x81F8` | `I:-1` | `fs_copy` of an absent source: 2 pops, 1 push |
| S16 | `I:6, I:41, I:0` | `0x8261` | array ID | `get_explosion_damage`: 3 pops, 1 push; the list is `[0, 0]` |
| S17 | `I:2, I:0, I:0` | `0x8261` | `I:0` | Sub-function 2 is still a stub |
| S18 | `"message_box", "x"`, inside a hook member | `0x8277` | `I:-1` | A member cannot wait |
| S19 | `I:161, I:16` | `0x8229` | empty | `force_encounter_with_flags`: flags on top |

### 7.2 Published string and math vectors

| Input | Result |
| --- | --- |
| `substr("test", -2, 2)` | `"st"` |
| `substr("test", 0, -2)` | `"te"` |
| `substr("test", 1, 0)` | `"est"` |
| `string_split("a,b", ",")` | `["a", "b"]` |
| `string_split("ab", "")` | `["a", "b"]` |
| `floor2(-1.5)` | `-2` |
| `2 ^ 3` | `I:8` |
| `(-8.0) ^ 2` | `F:64.0` |
| `(-8.0) ^ 0.5` | NaN |
| `get_ini_setting("a.ini")` | `-1` |
| `get_ini_string("a.ini")` | `I:-1` |
| `get_ini_string("absent.ini\|s\|k")` | `""` |

### 7.3 Required tests

1. Every row of `sfall-opcodes.tsv`: under sfall mode, a call pops `pops` operands and pushes `push` values. Tier R rows raise the VM error.
2. With sfall mode off, `0x8156` keeps its SPEC-017 row, `0x8200` and `0x8210..=0x8213` keep their enhanced rows, and all SPEC-021 goldens are byte-identical. SPEC-017 section 7.14 gives the vectors.
3. Every array vector, step by step, compares the push and the state.
4. The sidecar bytes of vector F round-trip, and each mutation in section 5.8's refusal row refuses the slot. A revision mutation uses a value other than 1 and 2, as vector MG2 does.
5. `sfall_hash` equals the vector values. It equals vector F3's value on a fresh session.
6. A map with the same operations in two allocator states serializes to identical bytes.
7. Two runs of one input log with a hook that draws random numbers give identical checkpoint streams on two platforms.
8. A hook registered on each of the fourteen hooks, returning nothing, changes no SPEC-021 hash from the unhooked run, apart from any random draws of its own.
9. Vectors V1 to V4, U1, M1, T1, and E1, step by step, compare the push and the state.
10. After vector V1's write, a load of the path reads the patched rate, and a new game or a load removes the virtual file.
11. A forced encounter draws no random number, changes no counter, spawns no group, and runs no `HOOK_ENCOUNTER` chain. Saved between the call and the check, it still occurs after a load.
12. An item with an ID in the unique range does not stack with an otherwise identical item. After `set_unique_id(item, -1)`, it stacks again.
13. `opcode_exists(0x823B)` gives 1 under sfall mode.
14. Every row of `mod-opcode-usage.tsv` with `rev3_tier` P1 is tier P1 in `sfall-opcodes.tsv` or `sfall-hooks.tsv`.
15. Vector MG1's `sidecar-r1` bytes load to its state and `defaults`, `sfall_hash` equals its value, and the next save writes exactly its `sidecar` bytes. Its empty sidecar gives vector F3's value. Every byte string of vector MG2 refuses the slot.
16. Vectors SS1 and SS2, step by step, compare the push, the state, the final arguments, and the diagnostic count.

## 8. Clean-Room Verification Checklist

- [x] Zero C/C++ code snippets, signatures, or struct definitions included
- [x] No file path under a source directory and no raw 40-character hash
- [x] All multi-byte integer endianness explicitly stated
- [x] Every normative rule tagged `[Must-match]`, `[May-improve]`, or `[Safe rule]`
- [x] Synthetic test vectors included, computed by a reference model
- [x] Opcode and hook tables generated from the published documents, not typed by hand, except the two operand classes of Decision D-46, which the generator sets by name
- [x] Spec linter run against `fallout1-re` symbols with 0 errors
- [x] Owner approval of Decision D-3 (brief of 2026-10-30), and the SPEC-017 amendment (SPEC-017 Revision 4)
- [x] Revision 2, by Team A: the ingestion grep finds no implementation-source path and no 40-character hexadecimal string, and this text has no code listing. The research linter was not re-run on this side, because Team A has no access to it, so the linter line above describes Revision 1
- [x] Revision 3: the opcode, hook, and vector tables regenerated from the documents bundled with Et Tu, which give the Revision 2 tables byte for byte before the tier change
- [x] Revision 3: the stack model reproduces SPEC-017 section 8.4's Fallout 1 result before any mod result was used
- [x] Revision 3: the source scan was calibrated against compiled scripts before its RPU counts were used
- [x] Revision 3 amendment: owner decision on D-40 (brief of 2026-10-30)
- [x] Revision 3 amendment: regenerating the oracle changes exactly the two `sfall-opcodes.tsv` rows of Decision D-46
- [x] Revision 3 amendment: owner decision on D-32 (brief of 2026-10-30): revision 1 sidecars migrate
- [x] Revision 3 amendment: regenerating the oracle adds 29 rows to `sfall-array-vectors.tsv` and changes no existing row

## 9. Decisions

Every decision is a research default unless the Class column says otherwise.

| ID | Decision | Class | Why |
| --- | --- | --- | --- |
| D-1 | Sources are sfall's public documentation at v4.5.1 only. `[Must-match]` means "as documented", not "verified against sfall" | Owner brief; research limit | The brief names public documentation. A fetch of the implementation was blocked, and it was not pursued |
| D-2 | Sfall mode is off by default and exists only under `fallout2` | Default | Both target mods run on Fallout 2 data; Fallout 1 stays untouched |
| D-3 | New set `FALLOUT2_SFALL`, `0x8000..=0x8281`, which shadows SPEC-017 section 4.1 and the enhanced words in that range. With sfall mode off, under either profile, every SPEC-017 word number behaves as it does today, with no breaking change to a test or script. With opt-in sfall mode on under `fallout2`, set by the settings file or a mod manifest, `0x8156..=0x8281` routes to this specification. SPEC-017 Revision 4 makes the amendment | **Owner instruction** (brief of 2026-10-30) | The numberings collide, and SPEC-017 Decision 1 is an owner instruction. Retail scripts use no word above `0x8155` |
| D-4 | Six unpublished words fail closed at execution | Default | Their operand counts are unknown, so a stub could unbalance the stack |
| D-5 | `sfall_func9` is not specified, although the brief named it | Default | The published list ends at `sfall_func8` (`0x8281`) |
| D-6 | A wrong operand type is a diagnostic and a default result, not a VM error | Default | sfall scripts expect functions not to stop them. This differs from SPEC-017 |
| D-7 | Tier P1 is the foundational set. P2 keeps stack balance with stubs | Default | Stack balance keeps scripts alive; undefined effects are not guessed |
| D-8 | X words are no-op stubs by design, and never reach memory (Decision D-40) | Default | They address another program's memory |
| D-9 | Map iteration is first-insertion order | Default | The docs leave it open. It is fully deterministic and needs no sort on each read |
| D-10 | A frame is one headless step, and temporary arrays end before the checkpoint | Default | A render frame is not a replayable unit. Checkpoints stay free of transient arrays |
| D-11 | IDs start at 1, rise, are not reused in a session, and are stored in the sidecar | Default | Stale IDs fail safely and allocation is deterministic |
| D-12 | Values are equal only with the same type and content | Default | One simple, total rule |
| D-13 | The sort order is numbers, then strings, with stable sorting | Default | The docs do not define a mixed-type order |
| D-14 | Shuffle uses the SPEC-006 gameplay generator | Default | Any other source would break replay |
| D-15 | The sidecar is a vault format, not `sfallgv.sav` | Default | That layout is unpublished. Saves from sfall itself do not import |
| D-16 | `state_hash` and `VSH1` stay unchanged; `sfall_hash` is added beside them | Default | No golden changes. The owner can later promote arrays into a `VSH2` |
| D-17 | Hooks never run in Presentation, and preview queries run in Simulation | Default | SPEC-022 requires drawing not to change the world. Firing counts differ from sfall |
| D-18 | Chain composition follows the published example; conversions and error handling are vault rules | Default | The docs define the happy path only |
| D-19 | A member that errors is unloaded, and the chain continues | Default | One broken mod must not disable the others |
| D-20 | Chains nest to a depth of 8 | Default | Bounded and deterministic |
| D-21 | Section 4.9's global-script rules are a minimum and yield to SPEC-024a | Default | SPEC-024a was not found in either repository |
| D-22 | Non-exact math uses one pinned software implementation | Default | Platform libraries differ in the last bit |
| D-23 | `get_uptime` is derived from the step count | Default | The wall clock is not replayable |
| D-24 | P2 hooks load but do not fire | Default | Their boundaries are not specified |
| D-25 | `HOOK_COMBATDAMAGE` arg12 is 0 | Default | The published attack-data offsets describe another engine's memory |
| D-26 | Et Tu and Restoration Project scripts were not audited | Research limit; **closed by Revision 3** (D-30) | Neither mod was in the local corpus at Revision 1 |
| D-27 | The shadowing covers the enhanced camera words `0x8210..=0x8213` as well as `0x8200` | Team A default, applying D-3 | Revision 1 named only `0x8200`, but `docs/REFERENCE.md` defines all five. Their numbers are in the owner's range, and their operand counts differ from the sfall words', so leaving them out would make dispatch ambiguous |
| D-28 | The manifest key is `sfall` under `[scripting]`, by the `[enhanced]` keys' rules, and `--sfall` turns the mode on from the command line | Team A default | A manifest refuses unknown tables, so the key must be defined for the owner's manifest route to work. A replay reads neither file, so without a flag no replay could run under sfall mode, and section 7.3's test 7 needs one |
| D-29 | A save with sfall mode off keeps section 5.8's named delete of an old `VAULTSFL.DAT`. SPEC-023's stale `.SAV` sweep leaves the file untouched | Team A default; the sweep's scope is an owner instruction (SPEC-023 D-18, D-38) | Without the delete, a slot just saved with sfall mode off would hold an old sidecar, and section 5.8's loading table would refuse it |
| D-30 | The audit reads Et Tu's compiled scripts through SPEC-017 section 8.2's model, and RPU's source through its own build selection, because RPU ships no `.int` files. No downloaded compiler was run | Research limit | A token scan of source is weaker evidence than a binary, so it was calibrated on the 1045 Et Tu scripts that exist in both forms, with no error on any P2 row or hook. RPU has no stack evidence of its own, and its 48 listed scripts with no source are retail scripts the mod leaves unchanged |
| D-31 | Opcodes rank by scripts, then call sites, with ties broken by need of a higher-ranked row, then use in both mods, then number. `0x8261` and its sub-function 6 are the same call sites, so they count as one of the 25. Hooks rank by scripts, then need, then handler size | Default | Nine rows tie for the last four opcode places, and nineteen for the last three hook places, so a sort alone would decide them. `fs_read_short` and `fs_write_short` are needed for `fs_seek` to do anything, `add_extra_msg_file` supplies `message_str_game`'s file IDs in RPU, and `HOOK_USEOBJ` is registered beside `HOOK_USEOBJON` |
| D-32 | The sidecar becomes format revision 2, with the next unique ID and the pending forced encounter. A revision 1 sidecar migrates on load: section 5.8 gives each added field's default (next unique ID `0x10000000`, pending map -1, pending flags 0), `sfall_hash` is the migrated state's, and the next save writes revision 2. The engine never writes revision 1 again | Owner instruction (brief of 2026-10-30) | Published text says unique IDs are saved, and a counter cannot be rebuilt from objects on unloaded maps. Et Tu sets a pending encounter that a save could otherwise lose. The engine has written revision 1 since this specification's Revision 1, so players have those sidecars, and refusing them would strand their saves. The defaults lose nothing: `set_unique_id` and the forced-encounter words were tier P2 stubs when revision 1 was specified, so a revision 1 game has no assigned unique ID and no pending encounter to keep. Vectors F and F3 changed, and vectors MG1 and MG2 pin the migration |
| D-33 | `fs_read_short` and `fs_write_short` are big-endian | Default, from script evidence | The documents are silent. RPU writes a small number into a big-endian FRM field, and only this order gives the result the script wants |
| D-34 | `get_terrain_name` reads `worldmap.msg` `1000 + terrain index` | Default, from script evidence | The documents are silent. Et Tu compares against those messages, so `worldmap.txt`'s names would fail on a translated install |
| D-35 | `get_screen_width` and `get_screen_height` are 640 and 480 on every host | Default | A host-dependent value would reach script logic and break replays. Et Tu's layouts work at these values |
| D-36 | Virtual files are not saved, and a write drops the path's cached decode | Default | Saving needs `fs_resize`, which stays P2, and both RPU patches run again on each load |
| D-37 | `message_box` waits as a modal screen, and gives -1 where a program cannot wait | Default | -1 is the published failure value, so a script that cannot show the box takes its documented path |
| D-38 | `tap_key` is a queued logical key. It runs no `HOOK_KEYPRESS` and is not logged | Default | A replay queues it again, and logging it would apply it twice |
| D-39 | A forced encounter runs no `HOOK_ENCOUNTER` chain and makes no draw | Default, from published text | The hook's text excludes scripted encounters, and Et Tu's handler depends on that |
| D-40 | X words are no-op stubs, although Et Tu's base scripts use them. Each pops its operands, pushes the default return value if it pushes, writes the standard diagnostic once per program, and never halts the script | Owner instruction (brief of 2026-10-30) | Three memory writes have no effect: two for the rest timer, and one for Dogmeat's dialogue. Engine rules could replace them, but none is specified here, and the words address memory that does not exist |
| D-41 | The three fallout2-ce function names stay unknown | Default | They are not sfall functions, and Et Tu takes that path only when `opcode_exists(0x823B)` is 0 |
| D-42 | `set_hero_style` stores a number and changes no art | Research limit | Et Tu ships appearance archives, but their mounting rule is unpublished. The word keeps its stack contract and its stored value until that is specified |
| D-43 | `HOOK_GAMEMODECHANGE` compares once per step | Default | Modes can change several times inside a step, and one fixed comparison point keeps firing replayable. A script that counts firings can notice |
| D-44 | A cancelled `HOOK_STDPROCEDURE` counts as run with no override | Default | The documents say only that execution is cancelled |
| D-45 | An interface box is lit by the engine's condition or by a script flag, and a script cannot hide what the engine lit | Default | SPEC-019's lit conditions stand, and RPU tests for a real addiction before hiding ADDICT |
| D-46 | `scan_array`'s value and `set_sfall_arg`'s value are class `A`, in `sfall-opcodes.tsv` and in sections 5.4 and 6.4. This reverses D-T1's direction for these two words | Owner instruction (later brief of 2026-10-30) | For `scan_array`, the arrays guide declares the value `mixed` and the published array test script searches for strings, so the opcode list's `int/float` is the narrower statement. For `set_sfall_arg`, every published signature says `int`, so class `A` is this project's rule, and it lets a `mixed` argument hold a float or a string. Vectors H1 and H2 hold, because an `int` argument still converts by class `I` |
| D-T1 | Superseded in part by D-46. Where the prose and `sfall-opcodes.tsv` disagree on an operand class, the table's class stands: `scan_array`'s value is `N`, and `set_sfall_arg`'s value is `I`. **The part that stands:** `ret_class` chooses a failed operand's default and does not limit the pushed value | Owner instruction for the direction (brief of 2026-10-30); Team A default for the meaning of `ret_class` | The engine compiles its word table from this file. Vector B step 9 pushes `F:5.0` from a row with `ret_class` `I`, so `ret_class` cannot limit the value. This ID stays outside the numbered series because Team A wrote it before the research revision's numbers were known on that side |
