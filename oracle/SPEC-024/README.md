# SPEC-024 Oracle Vectors

Vectors backing `SPEC-024b-sfall-opcodes-arrays-and-hooks.md` section 7. These hand-off files hold word numbers, stack contracts, hook signatures, and synthetic vectors. They hold no game data.

| File | Contents |
| --- | --- |
| `sfall-opcodes.tsv` | 449 rows: all 300 words `0x8156` to `0x8281`, 133 `sfall_funcN` rows, 7 `metarule3` rows, 9 `metarule2_explosions` rows |
| `sfall-hooks.tsv` | 50 hooks plus the reserved range 49-60 |
| `sfall-array-vectors.tsv` | 94 rows: array vectors A to F3 and hook-chain vectors H1 to H11 |

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

## `sfall-hooks.tsv`

`arg_types` and `ret_types` list the types of arg0 up, and ret0 up. `critter`, `item`, and `obj` are object references. `tier` is P1 for the eight hooks with boundary text, and P2 for hooks that load but do not fire (SPEC-024b section 6.14).

## `sfall-array-vectors.tsv`

Columns are `vector`, `step`, `operation`, `push`, `state_after`, and `rule`. Each vector starts from an empty store with the next ID at 1. Steps run in file order.

- Values are written `I:5`, `F:0.5`, and `S:"text"`.
- A state lists live arrays as `#ID shape storage key=K [elements]`. Map elements are written `K=>V`.
- A D-vector shuffle takes its random draws from the operation text, so it needs no generator.
- Rows named `sidecar`, `stream`, and `stream-after-step` hold bytes in `push`, as spaced hexadecimal pairs. Rows named `sidecar-fnv` and `sfall_hash` hold 16 uppercase hexadecimal digits, FNV-1a 64 as in SPEC-021 section 2.1.
- H rows give arguments, returns, the final value, and the member order. `rets=[None]` means no member set a return. A `[75.0]` return is a float converted when applied.
- H1 and H2 are the published chain example. The others are synthetic.

## Verification

The generator's checks stop it from writing a table on any failure:

- Exactly 294 published opcode rows, from `0x8156` to `0x8281`, with no parse failure.
- Exactly 50 hook IDs, each matched to a hook block.
- SPEC-021's three FNV-1a known answers.
- The sidecar bytes parse back and re-serialize identically.
- The published chain example gives 75 in both registration orders.

The published opcode list and function reference were also compared with each other: they agree, apart from `^` and `register_hook_proc_spec`, which the function reference lists without a number. The list's single-operand form of `metarule2_explosions` conflicts with the public headers and function notes, which give three operands, and the table uses three.

These vectors encode SPEC-024b's own rules. They prove the spec is self-consistent, and they do not prove agreement with a running sfall.

Eleven public names in `sfall-opcodes.tsv` also occur as identifiers in reference revision R1, so the spec linter flags them if pointed at this table. They are public names from sfall's script headers, not taken from R1. The spec text itself avoids them.

## Regenerating

The generator under `tools/spec024/` is research-side and must not be copied to the implementation repository. Only this directory and the spec cross the wall.

From the vault root, with `D` set to a folder holding the published documents of revision S1:

```sh
python3 -I tools/spec024/gen_oracle.py "$D" oracle/SPEC-024
```
