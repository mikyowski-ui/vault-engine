# SPEC-021 revision 4 fixtures

The folder is named `rev3` by the owner's brief; it holds revision 4 of
SPEC-021, because revision 3 of the spec had already used `rev2/` for its art
fixture (SPEC-021 decision D-40).

Every file here is synthetic. The Spec-Author sub-agent (Team A) wrote or
computed each one from SPEC-021 alone. None holds game data. These vectors
live here, not in `oracle/`, which is reserved for research-side vectors. See
SPEC-021 sections 3.10 to 3.12, 5.9 and 5.10, and decisions D-40 to D-49.

- `checkpoints-example.tsv`: a format example of the checkpoint stream (section
  3.10), built from the revision 2 worlds and their hashes in
  `../rev2/hashes.tsv`, not from a run. It has the version line, the column
  header and seven rows.
- `compare-cases.tsv`: 15 comparisons against `checkpoints-example.tsv` as the
  expected stream. Each row describes one change to a copy of that file, which
  is the actual stream, and gives the verdict and the exact report. `line` is
  the one-based line of the file, `value` is the new text, with `\t` standing
  for a tab, and `-` means "not used". The changes are:
  - `none`: no change.
  - `set`: replace the named column on the named line.
  - `set2`: the same, and also replace line 7's `state_hash` with `value`, so
    the stream differs at two rows.
  - `setboth`: the same, and also replace the same line's `combat_hash` with
    the expected row's `state_hash`, so one row differs in two columns.
  - `truncate`: drop every line after the named line.
  - `append`: add `value` as a last row.
  - `line1`, `line2`: replace that line with `value`.
  - `drop`: remove the named column from the named line.
- `scenario-explore-talk.jsonl`: the input log of replay scenario (a), for
  `maps/synthetic.map`. 403 records, 387 frames.
- `scenario-world-travel.jsonl`: the input log of replay scenario (b), for
  `maps/synthetic.map`. 622 records, 615 frames.

Both logs were written by a throwaway script. They use only the records
`surface`, `tick`, `pointer_moved`, `pointer_button`, `key` and `quit`, on a
640-by-480 surface with a 16 ms frame.

The implementer adds these goldens (SPEC-021 section 3.11), recorded once and
then frozen:

- `scenario-explore-talk.checkpoints.tsv`
- `scenario-world-travel.checkpoints.tsv`
- `scenario-seeded-combat.checkpoints.tsv`
- `scenario-world-travel.final.tsv`: the party's final position and game tick
  in scenario (b), which the state hash does not cover.

Scenario (c) has no log. It uses the driver of SPEC-021 section 3.7, over the
corpus that test T11 prepares.
