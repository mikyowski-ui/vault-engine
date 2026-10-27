# Synthetic fixtures

All files here were authored for Vault Engine. None contain original game assets.

- `formats/`: all section-4 fixtures from approved SPEC-001 and SPEC-002, including
  a materializer for a valid synthetic preview/audit corpus; see its README.
- `world/`: SPEC-003 prototypes, map streams, inventory, geometry, and complete
  synthetic map-preview recipes.
- `vm/`: SPEC-004 complete program, isolated fragments, error vectors, and VM
  context/scheduling recipes.
- `host/`: all SPEC-005 extension bytecode fragments and expected stack bytes.
- `state/`: SPEC-006 GAM/GCD vectors, world/RNG/stat/dialogue tests, and interactive
  synthetic corpus generation.
- `sound/`: SPEC-010 ACM vectors A-H as offset-addressed hex streams.
- `fallout2/`: SPEC-018 §8's synthetic data tables (`maps.txt`, `city.txt`,
  `worldmap.txt`, `party.txt`, the line tables and `elevators.txt`), the
  grammar sample of §8.2, and a builder that adds them and a synthetic
  `maps/artemple.map` to the synthetic corpus for a Fallout 2 install.
- `gallery/`: the documentation gallery's showcase (`cargo xtask
  capture-gallery`): procedural tile, wall, critter, item and scenery art
  drawn in code over the synthetic corpus, and a showcase map made from the
  camera demo's map without its script.
- `archive/`: SPEC-008 and SPEC-015 archive builders, and placeholder game
  folders the launcher accepts; see its README.
- `movie/`: SPEC-012 header, chunk-walk, subtitle, and fade vectors; the
  synthetic movies of Fixture H are read from `oracle/SPEC-012/synthetic/`.
- `loose/`: plain-text markers with mixed-case game-style paths and extensions.
  All eight top-level game directories and root palette/font paths are represented.
  The markers test VFS access and audit traversal; they are **not valid legacy
  binary assets**. This is the original Phase 0 filename/count fixture; later
  format validators intentionally reject its binary marker contents.
- `audit-expected.txt`: expected CLI counts for that loose directory.
- `core/entities.json`: synthetic entity payloads for generational handle tests.
- `core/sequences.txt`: interleaved command/event allocation and restore scenario.
- `core/save-v1.hex`: hand-authored version-1 header followed by ASCII `TEST`.
  Hex encoding keeps the complete wire representation reviewable.
- `render/indices.hex`: a 16×16, row-major sprite using each index 0–255 once.
  The renderer pairs this with a generated RGB332 palette.
- `input-commands.jsonl`: expected structured records for synthetic input.

Tests copy loose fixtures into temporary directories when exercising namespace
changes, collisions, or symlink attacks. The committed fixture directory is never
modified by tests.

The `support.rs` builders under `fallout2/`, `formats/`, `gallery/`, `movie/`, `sound/`,
`state/`, `vm/` and `world/` need only `std`, so they are compiled once as the `vault-fixtures`
crate (`crates/vault-fixtures`), a dev-dependency of the crates that use them
and a dependency of `cargo xtask fixtures`. `items/support.rs` implements
`vault-core` types, so `vault-core` includes it as its own test module.
