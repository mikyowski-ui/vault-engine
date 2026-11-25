# SPEC-007 Oracle Tables

The critical tables of `approved/SPEC-007-critter-fid-pathfinding-combat.md`, as the build reads them. Nothing here came from the research vault: these tables are transcribed from the approved spec text in this repository, so no generator stands behind them.

| File | Contents |
| --- | --- |
| `critical-success.tsv` | 864 rows: section 2.5, one row per entry of the matrices, by victim, location, and tier |
| `critical-failure.tsv` | 7 rows: section 2.4, the failure masks by table and column |

`crates/vault-core/build.rs` compiles these tables, and fails at the row's `file:line` on anything its schema does not allow. No build reads the spec. A change to the spec must be made in the table by hand: `crates/vault-core/tests/table_sync.rs` reads the Markdown, compares every row, and fails naming the spec section until the two agree.

Each file's `#` comments give its columns and its section. Victim 15 is row `P` of section 2.5, and an `=` of that section is written out as the entry it repeats.
