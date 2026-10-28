# SPEC-021 revision 2 fixtures

Every file here is synthetic and was computed by the Spec-Author sub-agent
(Team A) from SPEC-021 revision 2 alone. None holds game data. See SPEC-021
sections 3.9, 5.2 to 5.7 and decisions D-28 to D-33.

- `w0.stream.hex` to `w4.stream.hex`: the canonical world streams of the
  revision 2 worlds W0 to W4. W0 now holds its inventory item as its own
  entity `3:1` (PID 4, no position, orientation 0), which the engine needs,
  because it keeps every inventory item as an entity. Each stream is the
  revision 1 stream with the entity count raised to 3 and the 23-byte item
  record inserted after entity `2:3`.
- `hashes.tsv`: every revision 2 vector (W0 to W4 and the single-field
  mutations), with the same columns as the revision 1 table. The
  `combat_bytes` and `combat_hash` columns are unchanged from revision 1.
- `combat-trace-example.tsv`: the combat hash trace of snapshots S0 to S3,
  rebuilt on the revision 2 worlds. Only the `state_hash` column changed.
- `ai.txt`: a one-packet SPEC-009 `data/ai.txt` that makes the NPC fight on
  its turns in the seeded combat trace (SPEC-021 section 5.7).
- `combatai.msg`: a one-record SPEC-002 catalog. AI initialization needs
  this file to exist; the packet never shows a taunt.

The combat streams `../w1.combat.hex` and `../w4.combat.hex`, `../fnv1a64.tsv`
and the VM traces are unchanged and stay in the parent folder.

Before a seeded combat trace (SPEC-021 test T11), the test copies `ai.txt` to
`data/ai.txt` and `combatai.msg` to `text/english/game/combatai.msg` in its
own copy of the `cargo xtask fixtures` corpus. It never changes this folder.

Until the implementer's follow-up adopts revision 2, the merged tests still
read the revision 1 files in the parent folder. That follow-up switches them
to this folder, deletes the superseded parent files (`w0.stream.hex` to
`w4.stream.hex`, `hashes.tsv`, `combat-trace-example.tsv`), and re-records
`../combat-trace-synthetic-seed1.tsv` with the AI fixture installed.
