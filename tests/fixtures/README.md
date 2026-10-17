# Synthetic fixtures

All files here were authored for Vault Engine. None contain original game assets.

- `formats/`: all section-4 fixtures from approved SPEC-001 and SPEC-002, including
  a materializer for a valid synthetic preview/audit corpus; see its README.
- `loose/`: plain-text markers with mixed-case game-style paths and extensions.
  All eight top-level game directories and root palette/font paths are represented.
  The markers test VFS access and audit traversal; they are **not valid legacy
  binary assets**. There is one of each audited extension plus two other files.
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
