# SPEC-005 Oracle Table

The host opcode table of `approved/SPEC-005-host-opcodes.md`, as the build reads it. Nothing here came from the research vault: this table is transcribed from the approved spec text in this repository, so no generator stands behind it. It holds no game data.

| File | Contents |
| --- | --- |
| `host-opcodes.tsv` | 266 rows: every word `0x804C` to `0x8155`, in word order, with its section, name, operand types, and push kind |

`crates/vault-vm/build.rs` compiles this table, and fails at the row's `file:line` on anything its schema does not allow. No build reads the spec. A change to the spec must be made in the table by hand: `crates/vault-vm/tests/table_sync.rs` reads the Markdown, compares every row, and fails naming the spec section until the two agree.

The file's `#` comments give its columns and its sections. A type in `operands` is always named. Section 2.1 makes an unmarked operand `Integer`, so 347 of the table's operands have no suffix in `spec_operands`, the Pop cell kept verbatim.

Word `0x80DD` has the name ``attack` alias``, with its backtick, because that is the name the engine's catalog has always held for that row.
