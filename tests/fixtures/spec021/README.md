# SPEC-021 state hash and trace fixtures

Every file here is synthetic and was computed by the Spec-Author sub-agent
(Team A) from SPEC-021 alone. None holds game data. The RNG values follow from
SPEC-006 section 3.4 with seed 1, and the VM traces from SPEC-004 section
4.1's synthetic program (`tests/fixtures/vm/program.int.hex`). These goldens
live here, not in `oracle/`, which is reserved for research-side vectors.

- `fnv1a64.tsv`: FNV-1a 64 known answers (SPEC-021 section 2.1). The input is
  uppercase hexadecimal, `-` for the empty input.
- `hashes.tsv`: every vector of section 5 (W0 to W4 and the single-field
  mutations), with its base, the change, the stream lengths and both hashes.
- `w0.stream.hex` to `w4.stream.hex`: the complete canonical world streams of
  W0 to W4, as offset-addressed hex (the format of `vm/program.int.hex`).
- `w1.combat.hex`, `w4.combat.hex`: the complete combat streams of W1 and W4.
  W0's is the nine bytes `56 43 48 31 43 4D 42 54 00`.
- `vm-trace-program.tsv`: the VM opcode trace (section 3.5) of SPEC-004
  section 4.1's program, 44 rows.
- `vm-trace-program-branch.tsv`: the same with SPEC-004 section 4.1's branch
  mutation (the payload at `0x00AE` set to 1), 42 rows.
- `combat-trace-example.tsv`: a combat hash trace (section 3.6) built from the
  snapshots S0 to S3 of section 5.6, as a format example.

The implementer adds `combat-trace-synthetic-seed1.tsv`, the seeded combat
golden of SPEC-021 test T11, and a line here naming the synthetic map it was
recorded on.
