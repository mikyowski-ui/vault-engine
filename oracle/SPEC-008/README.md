# SPEC-008 Oracle Tables

The fixed world and Pip-Boy catalogs of `approved/SPEC-008-dat-saves-and-worldmap.md`, as the builds read them. Nothing here came from the research vault: these tables are transcribed from the approved spec text in this repository, so no generator stands behind them.

| File | Contents |
| --- | --- |
| `world-grids.tsv` | 30 rows: section 3.6, the terrain, frequency, and region digits of each grid row |
| `towns.tsv` | 12 rows: section 3.7, the town locations |
| `hotspots.tsv` | 42 rows: section 3.7, the listed slots of the hotspot catalog |
| `maps.tsv` | 66 rows: section 3.10, the map numbers, with town and city music |
| `holodisks.tsv` | 18 rows: section 3.8, the campaign index of each holodisk |
| `quests.tsv` | 12 rows: section 3.8, the quest campaign indices of each town |

`crates/vault-core/build.rs` compiles the first four and `crates/vault-guidance/build.rs` the last two, and each fails at the row's `file:line` on anything its schema does not allow. No build reads the spec. A change to the spec must be made in the table by hand: the `tests/table_sync.rs` test of each crate reads the Markdown, compares every row, and fails naming the spec section until the two agree.

Each file's `#` comments give its columns and its section.
