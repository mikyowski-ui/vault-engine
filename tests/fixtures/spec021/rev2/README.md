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
- `sampleaq.frm`, `sampleao.frm`, `sampleba.frm`, `samplecj.frm` (revision
  3): synthetic critter art for the corpus's only critter base, `sample`:
  punch (animation 16), hit flinch (14), fall back (20) and stand-up (37).
  Without them the NPC's punch cannot register its animation sequence, so
  the attack is refused before any AP is spent (SPEC-021 section 5.8). The
  four files are identical, 75 bytes each: one 1-by-1 frame of palette index
  `0x10` shared by all six directions, 10 fps, action frame 0. Every multi-byte
  field is big-endian (SPEC-001):

  ```text
  0000: 00 00 00 04 00 0A 00 00 00 01 00 00 00 00 00 00
  0010: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
  0020: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
  0030: 00 00 00 00 00 00 00 00 00 00 00 00 00 0D 00 01
  0040: 00 01 00 00 00 01 00 00 00 00 10
  ```

The combat streams `../w1.combat.hex` and `../w4.combat.hex`, `../fnv1a64.tsv`
and the VM traces are unchanged and stay in the parent folder.

Before a seeded combat trace (SPEC-021 test T11), the test copies `ai.txt` to
`data/ai.txt` and `combatai.msg` to `text/english/game/combatai.msg` in its
own copy of the `cargo xtask fixtures` corpus. Since revision 3 it also
copies each of the four `sample*.frm` files to `art/critters/` under the same
name (for example `sampleaq.frm` to `art/critters/sampleaq.frm`). It never
changes this folder.

The implementer's follow-up adopted revision 2 (SPEC-021 section 3.9): the
tests read the world vectors, `hashes.tsv` and `combat-trace-example.tsv`
from this folder, the superseded revision 1 files in the parent folder were
deleted, and `../combat-trace-synthetic-seed1.tsv` was re-recorded with the
AI fixture and the art fixture installed.
