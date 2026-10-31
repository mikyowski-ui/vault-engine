# SPEC-024b: sfall Extended Opcodes, Dynamic Arrays, and Hook Scripts

**Status:** research hand-off, Revision 1, 2026-10-30. Ingested on the owner's brief of 2026-10-30, which approves Decision D-3. Revision 2 (Team A, 2026-10-30) makes that amendment; see "Revision history".

**Provenance:** written on the research side from the published sfall scripting documentation at tag v4.5.1 (reference revision S1): the opcode list, the function reference, the function notes, the hook reference, the arrays guide, the compiler guide, and the public script headers. The engine's own approved specs supplied the anchors. sfall's implementation code was **not** read, and nothing here was checked against a running sfall or against Et Tu or Restoration Project scripts (Decisions D-1 and D-26).

**Provenance of Revision 2.** Author: the Spec-Author sub-agent (Team A), 2026-10-30, on the owner's brief of 2026-10-30. Sources: the owner's brief; this spec and `oracle/SPEC-024/`; SPEC-017, SPEC-023; and `docs/REFERENCE.md` (the enhanced words, mod manifests, and what a replay reads). No decompilation, disassembly, re-implementation source, game asset, or engine Rust code was used, and no sfall material was consulted on this side.

**Oracle:** `oracle/SPEC-024/` holds the complete tables and computed vectors. Section 7 says how they relate to this text. Revision 2 changes no oracle file.

## Revision history

- **Revision 1 (2026-10-30).** Research hand-off.
- **Revision 2 (2026-10-30, Team A).** Approves Decision D-3 as an owner instruction, and makes the amendment it asked for in SPEC-017 (Revision 4).
  - **Section 3.1.** Sfall mode now has a defined manifest key, and a command-line flag for replays (D-28).
  - **Section 3.2.** The shadowing covers every enhanced word in the overlap. Revision 1 named only `0x8200`, but the four camera words `0x8210..=0x8213` are enhanced words too (D-27).
  - **Section 5.8.** Separates its named delete of `VAULTSFL.DAT` from SPEC-023's stale `.SAV` sweep (D-29). The rule itself does not change.
  - Every other rule, and every oracle file, is unchanged.
- **Prose correction to Revision 2 (2026-10-30, Team A).** On the owner's brief of 2026-10-30, from the implementer's report in `05608f9`, sections 4.1, 5.4, 6.4 and 6.5 now agree with `sfall-opcodes.tsv` (D-T1). `scan_array`'s value is class `N` and `set_sfall_arg`'s value is class `I`, and section 4.1 says what `ret_class` means. No oracle file, vector or word number changed.

## Clean-Room Rules for Research Agents

- Document only binary layouts, formulas, opcode numbers, stack transitions, state rules, and observable behavior.
- Do not copy code, struct definitions, function signatures, or internal variable names from `sfall`, `fallout2-ce`, `fallout1-re`, or any decompilation.
- Public SSL opcode names, public hook names, and public script file names are permitted. They are what compiled scripts and mod files contain.
- Provide synthetic test vectors.

## 1. Overview & Scope

This specification lets the engine run compiled scripts that use sfall's extended script interface, as *Fallout Et Tu 2.0* and *Restoration Project Updated* do. It has three parts:

- **Part 1 (section 4):** the extended instruction words `0x8156..=0x8281`, the `sfall_func0` to `sfall_func8` dispatchers, the `metarule3` sub-functions, and their stack contracts (`vault_vm::extensions`).
- **Part 2 (section 5):** dynamic arrays, their storage classes, deterministic iteration, and the save sidecar (`vault_vm` and `vault_core`).
- **Part 3 (section 6):** `hs_*.int` hook scripts, registration, argument and return handling, and eight fully specified hooks (`vault_core::host`).

Section 3 comes first because these word numbers collide with SPEC-017.

Out of scope: `ddraw.ini` engine patches, shaders, the graphics and sound words, and the 42 hooks of tier P2. They keep a stack contract only (sections 4.10 and 6.14).

**Tiers.** Every word, function, and hook in the oracle carries one tier. `[Must-match]`

| Tier | Meaning |
| --- | --- |
| P1 | Fully specified here |
| P2 | Number and stack contract are specified; the effect is a stub (section 4.10) |
| X | Refused by design (section 4.10) |
| R | Reserved word with no published contract (section 3.3) |

**Compatibility tags.**

| Tag | Meaning here |
| --- | --- |
| `[Must-match]` | The published documentation states it, or a published example directly implies it |
| `[May-improve]` | The documentation acknowledges a defect; the rule gives a safe replacement |
| `[Safe rule]` | The documentation is silent, or its behavior cannot be deterministic; the rule is this vault's definition. It is deterministic and never undefined |

A `[Must-match]` tag means "matches the published documentation". It does not mean "confirmed against a running sfall" (Decision D-1).

**Invariant.** With sfall mode off, no word, hook, array, file, or hash in this specification exists, and every existing behavior and golden is unchanged. The one exception is how a slot's old `VAULTSFL.DAT` is treated (section 5.8). With sfall mode on and no hook registered, every formula in SPEC-007, SPEC-009, SPEC-011, and SPEC-014 gives the same value as with sfall mode off. `[Must-match]`

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

**Return class.** The `ret_class` column of `sfall-opcodes.tsv` chooses that default. It does not limit what a word returns, because a pushed value keeps its own type. So `scan_array` and `array_key`, with `ret_class` `I`, can push a float or string map key (vector B step 9 pushes `F:5.0`), `get_sfall_arg` pushes what the argument holds, and `list_next` pushes an object reference. The Push column of a word table gives what the word can push. `[Safe rule]`

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
- Each call pushes **exactly one** value, including functions documented as having no result, which push int 0. `[Must-match]`, provisional until the stack check confirms it on real mod scripts (Decision D-26).
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

Limiting the existence queries to P1 means a script that checks before relying on a function takes its fallback path rather than trusting a stub.

### 4.3 `metarule`, `metarule3`, and `metarule2_explosions`

- **`metarule`** (`0x810B`). The published documentation adds no rule numbers. SPEC-017 section 5.1 applies unchanged. `[Must-match]`
- **`metarule3`** (`0x80E1` under Fallout 2). SPEC-017 section 5.2's pop order and its rules 100 to 111 apply unchanged. sfall adds the following sub-functions. `[Must-match]` for numbers and operands.

| Sub | Public macro | p1 | p2 | p3 | Tier | Effect |
| ---: | --- | --- | --- | --- | --- | --- |
| 200 | `set_horrigan_days` | days | 0 | 0 | P2 | Stub |
| 201 | `clear_keyboard_buffer` | 0 | 0 | 0 | P1 | Empties the logical key queue; int 0. `[Must-match]` |
| 210 | `get_current_save_slot` | 0 | 0 | 0 | P2 | Stub |
| 211 | `set_current_save_slot` | page | slot | 0 | P2 | Stub |
| 212 | `get_current_quick_save_page` | 0 | 0 | 0 | P2 | Stub |
| 213 | `get_current_quick_save_slot` | 0 | 0 | 0 | P2 | Stub |
| 214 | `set_current_quick_save_slot` | page | slot | check | P2 | Stub |

Any other sub-function follows SPEC-017 section 5.2's unknown rule. `[Must-match]`

- **`metarule2_explosions`** (`0x8261`) pops three operands and pushes one value, as `[a, b, c] -> [r]`. Its sub-functions 1 to 9 are P2. The opcode list shows a single operand, but the headers and function notes show three, and this spec follows those. `[Must-match]`

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
- **X words** are `read_byte`, `read_short`, `read_int`, `read_string`, `write_byte`, `write_short`, `write_int`, `write_string`, `call_offset_v0` to `call_offset_v4`, and `call_offset_r0` to `call_offset_r4`. They address the original executable's memory, which does not exist here. They follow the P2 rule. `[Safe rule]` Their addresses are never used to reach host memory.
- **R words** follow section 3.3.

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
| `0x8239` | `scan_array` | value N, ID | A | First match: list index or map key, else int -1 | Int -1 |
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

**Caution, a string value.** `scan_array`'s value has class `N`, so a string is a failed operand (section 4.1): nothing is searched, and the push is int 0, not -1. A script can read that 0 as a match at index 0. `[Safe rule]`

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

This is a vault format, deliberately **not** byte-compatible with sfall's `sfallgv.sav`, whose layout is unpublished (Decision D-15). All of section 5.8 is `[Safe rule]`.

**File.** `VAULTSFL.DAT`, beside the slot's `SAVE.DAT`, through the storage path that SPEC-023 gives other slot files.

- A save under sfall mode **always** writes it, even when empty.
- A save with sfall mode off removes any old one, as for `VAULTENG.TOML`. This is one named delete in that save's commit. SPEC-023 section 3.11's stale `.SAV` sweep never touches this file (Decision D-29).
- `SAVE.DAT` itself is unchanged.

| Offset | Bytes | Content |
| ---: | --- | --- |
| 0 | 4 | Tag `VSFL` |
| 4 | 4 | u32 format revision, 1 |
| 8 | 4 | u32 next array ID |
| 12 | variable | seq(global record) |
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
| Sfall mode on, sidecar present and valid | Restores globals, arrays, and the next ID |
| Sfall mode on, no sidecar | Empty state, next ID 1. A vanilla save can start using sfall |
| Sfall mode off, sidecar present | Refuses the slot, with an explicit message: its programs were written for other word meanings |
| Bad tag, unknown revision, truncation, trailing bytes, unknown type byte, a duplicate ID, or a shape byte outside 0-1 | Refuses the slot, with an explicit message. Saved data is never dropped silently |

### 5.9 `sfall_hash` and SPEC-021

**`WorldState::state_hash` does not change.** The stream stays `VSH1`, and no golden is re-recorded. SPEC-021 section 2.3 requires a new magic for any change to coverage, and arrays add nothing that vanilla replays can see. `[Safe rule]`

`state_hash` stays deterministic because arrays can only reach covered state through scripts, and section 5.6 makes every script-visible array result deterministic. `[Must-match]` for that requirement.

Array divergence that has not yet reached covered state is caught by a companion: `[Safe rule]`

- `WorldState::sfall_hash(&self) -> u64` is `fnv1a64` (SPEC-021 section 2.1) of the **extension stream**.
- That stream is the section 5.8 layout with tag `VSX1`, and with **every live array**, of any storage class. The storage byte is significant.
- It obeys SPEC-021 section 3.3's rules: it takes `&self`, draws no random number, and changes no state.
- With an empty store and no globals, it is the fixed 20-byte stream of vector F3.

Hook registrations, global-script counters, open lists, and the last-created record are not covered, because scripts rebuild them. `[Safe rule]`

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
| `0x823D` | `set_sfall_arg` | value I, index I | none | Replaces argument `index`, 0-based | Ignored |
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

- `set_sfall_arg` takes its value by class `I`, so a float truncates and a string is a failed operand (section 4.1). The resulting int replaces the argument whatever its declared type, `mixed` included. An index out of range is ignored.
- A return is converted by the hook's declared return type: `int`, `critter`, `item`, and `obj` by class `I`, and `mixed` unchanged. A value that cannot convert leaves that return unset, with a diagnostic.
- A member that ends in a VM error is unloaded from every chain, and its writes up to that point stay. The chain continues with the next member.
- A member runs to completion, by SPEC-004 section 3.7's synchronous call. A word that would suspend it (a dialogue, a wait, or a timer yield) ends that member, with a diagnostic.
- Chains nest when a member causes another hook. Each chain has its own context, and the outer one resumes untouched. Past **8 nested chains**, a chain is skipped with a diagnostic and the engine's values stand.
- `self_obj` is null in a member, unless `set_self` changes it.

### 6.6 Phases and previews

Hooks must not break two engine rules: drawing never changes the world (SPEC-022 section 3.13), and the same inputs give the same states (SPEC-021 section 3.10). All of section 6.6 is `[Safe rule]`.

- **A chain runs only in phase 1 (Input) or phase 2 (Simulation)**, never in phase 3 (Checkpoint) or 4 (Presentation).
- **A chain runs once per engine query**, even if an identical query came just before. Query order is the order of the host specs. A chain may draw random numbers: the draws are part of the deterministic sequence, because the queries are.
- **Preview queries.** sfall also runs hooks for values that are only displayed, such as the to-hit percentage under the targeting cursor or the AP cost on the interface bar. Here, such a value is queried in phase 2 from the state of that step, and presentation reads the stored result.
  - A preview runs at most once per step, and only when its inputs differ from the previous step's.
  - A preview whose inputs are not reachable from the input records shows the unhooked value.

This differs from sfall in how many times a hook fires (Decision D-17). A script that counts firings can notice; a script that only computes a value cannot.

### 6.7 Common numbering

These agree with the engine's own tables. `[Must-match]` The argument and return tables of sections 6.8 to 6.13 are also `[Must-match]` unless a row says otherwise.

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
| ret1 | V_P, for section 3.19 step 3 and display | Negative becomes 0. `[Safe rule]` |

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

### 6.14 The other 42 hooks

Tier P2 hooks are listed with full argument and return types in `sfall-hooks.tsv`. Their scripts load, register, and run during setup, but **no chain runs**, because their engine boundaries are not specified here. The engine's own behavior stands. `[Safe rule]`

Each must get its own boundary text before it fires. IDs 49 to 60 are reserved. `[Must-match]` `hs_stdprocedure.int` serves both IDs 40 and 41. `[Must-match]`

## 7. Synthetic Test Fixtures

All data is synthetic. `oracle/SPEC-024/README.md` explains the files.

| File | Coverage |
| --- | --- |
| `sfall-opcodes.tsv` | All 300 words, 133 `sfall_funcN` rows, 7 `metarule3` rows, 9 `metarule2_explosions` rows |
| `sfall-hooks.tsv` | All 50 hooks and the reserved range |
| `sfall-array-vectors.tsv` | Arrays (A to F3) and hook chains (H1 to H11), computed by a reference model |

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
4. The sidecar bytes of vector F round-trip, and each mutation in section 5.8's refusal row refuses the slot.
5. `sfall_hash` equals the vector values. It equals vector F3's value on a fresh session.
6. A map with the same operations in two allocator states serializes to identical bytes.
7. Two runs of one input log with a hook that draws random numbers give identical checkpoint streams on two platforms.
8. A hook registered on each of the eight hooks, returning nothing, changes no SPEC-021 hash from the unhooked run, apart from any random draws of its own.

## 8. Clean-Room Verification Checklist

- [x] Zero C/C++ code snippets, signatures, or struct definitions included
- [x] No file path under a source directory and no raw 40-character hash
- [x] All multi-byte integer endianness explicitly stated
- [x] Every normative rule tagged `[Must-match]`, `[May-improve]`, or `[Safe rule]`
- [x] Synthetic test vectors included, computed by a reference model
- [x] Opcode and hook tables generated from the published documents, not typed by hand
- [x] Spec linter run against `fallout1-re` symbols with 0 errors
- [x] Owner approval of Decision D-3 (brief of 2026-10-30), and the SPEC-017 amendment (SPEC-017 Revision 4)
- [x] Revision 2, by Team A: the ingestion grep finds no implementation-source path and no 40-character hexadecimal string, and this text has no code listing. The research linter was not re-run on this side, because Team A has no access to it, so the linter line above describes Revision 1

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
| D-8 | X words are refused by design | Default | They address another program's memory |
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
| D-26 | Et Tu and Restoration Project scripts were not audited | Research limit | Neither mod is in the local corpus. The stack check should run on them before these contracts are final |
| D-27 | The shadowing covers the enhanced camera words `0x8210..=0x8213` as well as `0x8200` | Team A default, applying D-3 | Revision 1 named only `0x8200`, but `docs/REFERENCE.md` defines all five. Their numbers are in the owner's range, and their operand counts differ from the sfall words', so leaving them out would make dispatch ambiguous |
| D-28 | The manifest key is `sfall` under `[scripting]`, by the `[enhanced]` keys' rules, and `--sfall` turns the mode on from the command line | Team A default | A manifest refuses unknown tables, so the key must be defined for the owner's manifest route to work. A replay reads neither file, so without a flag no replay could run under sfall mode, and section 7.3's test 7 needs one |
| D-29 | A save with sfall mode off keeps section 5.8's named delete of an old `VAULTSFL.DAT`. SPEC-023's stale `.SAV` sweep leaves the file untouched | Team A default; the sweep's scope is an owner instruction (SPEC-023 D-18, D-38) | Without the delete, a slot just saved with sfall mode off would hold an old sidecar, and section 5.8's loading table would refuse it |
| D-T1 | Where the prose and `sfall-opcodes.tsv` disagree on an operand class, the table's class stands: `scan_array`'s value is `N`, and `set_sfall_arg`'s value is `I`. `ret_class` chooses a failed operand's default and does not limit the pushed value | Owner instruction for the direction (brief of 2026-10-30); Team A default for the meaning of `ret_class` | The engine compiles its word table from this file. Vector B step 9 pushes `F:5.0` from a row with `ret_class` `I`, so `ret_class` cannot limit the value. Vectors H1 and H2 hold under class `I`. This ID stays outside the numbered series because a research revision is expected, and its numbers are not known on this side |
