# SPEC-024 Oracle Vectors

Vectors backing `SPEC-024b-sfall-opcodes-arrays-and-hooks.md` section 7. These hand-off files hold word numbers, stack contracts, hook signatures, and synthetic vectors. They hold no game data.

| File | Contents |
| --- | --- |
| `sfall-opcodes.tsv` | 449 rows: all 300 words `0x8156` to `0x8281`, 133 `sfall_funcN` rows, 7 `metarule3` rows, 9 `metarule2_explosions` rows |
| `sfall-hooks.tsv` | 50 hooks plus the reserved range 49-60 |
| `sfall-array-vectors.tsv` | 182 rows: array vectors A to F3, hook-chain vectors H1 to H17, vectors V1 to V4, U1, M1, T1, and E1 for the rows promoted in Revision 3, sidecar migration vectors MG1 and MG2, and string-value vectors SS1 and SS2 |
| `mod-opcode-usage.tsv` | 108 rows: every Tier P2 opcode row and hook the two target mods use, ranked. This is evidence, not a test input |

## `sfall-opcodes.tsv`

| Column | Meaning |
| --- | --- |
| `kind` | `word`, `sfall_func`, `metarule3`, or `metarule2_explosions` |
| `id` | Instruction word, function name string, or sub-function number |
| `mnemonic` | Public name; `-` for a reserved word |
| `dispatch_word` | The instruction that reaches this row |
| `pops` | Operands popped, including the name or sub-function operand; `-` for a reserved word |
| `push` | 1 when a value is pushed, else 0 |
| `arg_classes` | Operand classes in public-call order (SPEC-024b section 4.1). Pop order is the reverse |
| `ret_class` | Return class; `-` for none |
| `category` | Group from the published function reference |
| `tier` | P1, P2, X, or R (SPEC-024b section 1) |
| `tag` | Covers number, stack contract, and push. For tier P2 the effect is `[Safe rule]` |

Where a function has two published argument counts, it has one row per count.

`scan_array` and `set_sfall_arg` take their value as class `A` (SPEC-024b Decision D-46), although the published opcode list declares `int/float` and `int`. The generator sets those two classes by name.

## `sfall-hooks.tsv`

`arg_types` and `ret_types` list the types of arg0 up, and ret0 up. `critter`, `item`, and `obj` are object references. `tier` is P1 for the fourteen hooks with boundary text, and P2 for hooks that load but do not fire (SPEC-024b section 6.19).

## `sfall-array-vectors.tsv`

Columns are `vector`, `step`, `operation`, `push`, `state_after`, and `rule`. Each vector starts from an empty store with the next ID at 1. Steps run in file order.

- Values are written `I:5`, `F:0.5`, and `S:"text"`.
- A state lists live arrays as `#ID shape storage key=K [elements]`. Map elements are written `K=>V`.
- A D-vector shuffle takes its random draws from the operation text, so it needs no generator.
- Rows named `sidecar`, `sidecar-r1`, `sidecar-r1-empty`, `stream`, and `stream-after-step` hold bytes in `push`, as spaced hexadecimal pairs. Rows named `sidecar-fnv`, `sfall_hash`, and `sfall_hash-empty` hold 16 uppercase hexadecimal digits, FNV-1a 64 as in SPEC-021 section 2.1.
- MG2 rows, named `refused-*`, hold the bytes in `push` and the reason in `state_after`. MG1's `defaults` row states the three migration defaults in `push`.
- H rows give arguments, returns, the final value, and the member order. `rets=[None]` means no member set a return. A `[75.0]` return is a float converted when applied.
- H1 and H2 are the published chain example. The others are synthetic.
- V, U, M, T, and E rows use the same columns, with the state in `state_after`. Vector V's bytes are a synthetic FRM-shaped header, and a step's bytes are spaced hexadecimal pairs after `bytes=`.
- Vector U's ordinary ID 900 stands for whatever the engine's allocator gives.
- SS2 rows use the H columns. Their final value gives arguments 2 and 12 and the diagnostic count.
- Vectors F and F3 changed in Revision 3, because the sidecar is now format revision 2 (SPEC-024b section 5.8).
- Vector names are unique across this file and SPEC-024b section 7.1. M1 already names message files, and S1 and S2 name stack vectors, so migration is MG and string values are SS.

## `mod-opcode-usage.tsv`

The audit of SPEC-024b section 1.1. Rows with `group` `opcode` come first, then `group` `hook`. Each group ranks from 1, by the tie rule of Decision D-31. A row's tier in this table is the tier before Revision 3 promoted anything, so every row was Tier P2 when audited.

| Column | Meaning |
| --- | --- |
| `group` | `opcode` or `hook` |
| `rank` | Position in its group |
| `kind` | `word`, `sfall_func`, `metarule3`, `metarule2_explosions`, or `hook` |
| `id` | Instruction word, function name, sub-function number, or hook ID |
| `mnemonic` | Public name |
| `dispatch_word` | The instruction that reaches this row; `-` for a hook |
| `scripts_total` | `scripts_etu` plus `scripts_rpu`. This is the ranking count |
| `scripts_etu` | Compiled scripts in Et Tu's `mods` folder that use the row |
| `scripts_rpu` | RPU source scripts that use the row |
| `scripts_sfall_dat` | Scripts inside `sfall.dat` and `ce.dat` that use the row. Not in the total |
| `sites_etu`, `sites_rpu` | Call sites. `-` for a hook |
| `call_arity` | Argument counts seen, for `sfall_func` rows |
| `handler_tokens` | Largest source size of a handler, with the procedures it reaches. A hook tie-break only |
| `evidence` | `int`, `ssl`, `dat`, joined by `+` |
| `rev3_tier` | P1 when Revision 3 promoted the row, else P2 |

Rank 1 (`0x8261`) and rank 2 (its sub-function 6) are the same call sites, so the 26 opcode rows with `rev3_tier` P1 are 25 behaviors. `0x8261` shows one more script than sub-function 6, for its sub-functions 3 and 4.

## Verification

The generator's checks stop it from writing a table on any failure:

- Exactly 294 published opcode rows, from `0x8156` to `0x8281`, with no parse failure.
- Exactly 50 hook IDs, each matched to a hook block.
- SPEC-021's three FNV-1a known answers.
- The sidecar bytes parse back and re-serialize identically.
- A revision 1 sidecar's next save equals its own bytes with revision 2's header, its hash equals that of the revision 2 file's state, and every MG2 byte string is refused.
- Each SS2 row's argument and diagnostic count match its expected result.
- The published chain example gives 75 in both registration orders.

The audit has no such stop, so its results were checked before use:

- The stack model reproduces SPEC-017 section 8.4's Fallout 1 result before it reads a mod: 0 findings and the one recorded exception.
- The usage table is byte-identical before and after the tier change, because the audit reports promoted rows as they were audited.
- The source scan was calibrated against compiled scripts (SPEC-024b section 1.1).

The published opcode list and function reference were also compared with each other: they agree, apart from `^` and `register_hook_proc_spec`, which the function reference lists without a number. The list's single-operand form of `metarule2_explosions` conflicts with the public headers and function notes, which give three operands, and the table uses three.

These vectors encode SPEC-024b's own rules. They prove the spec is self-consistent, and they do not prove agreement with a running sfall.

Eleven public names in `sfall-opcodes.tsv` also occur as identifiers in reference revision R1, so the spec linter flags them if pointed at this table. They are public names from sfall's script headers, not taken from R1. The spec text itself avoids them.

## Regenerating

The generator under `tools/spec024/` is research-side and must not be copied to the implementation repository. Only this directory and the spec cross the wall.

From the vault root, with `D` set to a folder holding the published documents of revision S1:

```sh
python3 -I tools/spec024/gen_oracle.py "$D" oracle/SPEC-024
```

The usage table also needs both mods, and the host-word catalog that the engine generates from SPEC-005:

```sh
python3 -I tools/spec024/mod_audit.py audit --table oracle/SPEC-024 --catalog "$C" \
    --fo1in2 "$F" --rpu "$R" --work "$W" --out oracle/SPEC-024/mod-opcode-usage.tsv
```
