# SPEC-021 state hash and trace fixtures

Every file here is synthetic and was computed by the Spec-Author sub-agent
(Team A) from SPEC-021 alone. None holds game data. The RNG values follow from
SPEC-006 section 3.4 with seed 1, and the VM traces from SPEC-004 section
4.1's synthetic program (`tests/fixtures/vm/program.int.hex`). These goldens
live here, not in `oracle/`, which is reserved for research-side vectors.

- `fnv1a64.tsv`: FNV-1a 64 known answers (SPEC-021 section 2.1). The input is
  uppercase hexadecimal, `-` for the empty input.
- `rev2/`: the current world vectors of SPEC-021 revision 2, where W0 holds
  its inventory item as entity `3:1`: `w0.stream.hex` to `w4.stream.hex` (the
  complete canonical world streams, as offset-addressed hex in the format of
  `vm/program.int.hex`), `hashes.tsv` (every vector of section 5 with its
  base, the change, the stream lengths and both hashes) and
  `combat-trace-example.tsv` (the combat hash trace of the snapshots S0 to
  S3 of section 5.6, as a format example). It also holds the AI fixture and
  the critter art fixture that the seeded combat trace installs. See
  `rev2/README.md`. The revision 1 versions of these vectors, superseded by
  revision 2, were deleted (SPEC-021 section 3.9).
- `w1.combat.hex`, `w4.combat.hex`: the complete combat streams of W1 and W4,
  unchanged by revision 2. W0's is the nine bytes
  `56 43 48 31 43 4D 42 54 00`.
- `vm-trace-program.tsv`: the VM opcode trace (section 3.5) of SPEC-004
  section 4.1's program, 44 rows.
- `vm-trace-program-branch.tsv`: the same with SPEC-004 section 4.1's branch
  mutation (the payload at `0x00AE` set to 1), 42 rows.

The implementer adds `combat-trace-synthetic-seed1.tsv`, the seeded combat
golden of SPEC-021 test T11, and a line here naming the synthetic map it was
recorded on.

- `combat-trace-synthetic-seed1.tsv`: the seeded combat golden (SPEC-021
  test T11), recorded by the implementer (Team B) on 2026-10-27 with
  `asset-audit --dump-combat-trace maps/synthetic.map --seed 1
  --max-rounds 5` over the `cargo xtask fixtures` corpus (its first
  directory). Map used: `maps/synthetic.map`, which has a defender (the
  synthetic critter, handle `3:1`). Re-recorded on 2026-10-27 under SPEC-021
  revision 3 sections 3.7 and 3.9 (SPEC-020 section 3.4's rule), with the AI
  fixture (`rev2/ai.txt`, `rev2/combatai.msg`) and the critter art fixture
  (the four `rev2/sample*.frm` files) installed in that corpus. The NPC
  punches the player on each of its turns, ending each at 1 AP of 7. The
  combat does not end on its own, so the trace stops after five complete
  rounds without an `end` row.
