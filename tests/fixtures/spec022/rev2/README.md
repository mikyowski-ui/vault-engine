# SPEC-022 revision 2 fixtures

Every file here is synthetic. The Spec-Author sub-agent (Team A) computed
each one from SPEC-022 revision 2 alone. None holds game data. The scenes use
the formula tables of SPEC-022 section 5.1 instead of a palette. See SPEC-022
sections 3.15 and 5.2 to 5.7, and decisions D-10, D-16 and D-25 to D-32.

Formats are those of the parent folder's README: TSV files have one header
row, hexadecimal values are uppercase, and `-` means "not used".

- `sort-keys.tsv`: every revision 1 key (floor and roof rows relabelled as
  ranks), plus the shroud (layer 3, primary 10000), ground-item highlights
  (outline positions 100 and 105) and the largest list index.
- `square-order.tsv`: nine floor squares, their SPEC-003 section 3.6 floor
  and roof corners at OX = -3680, OY = -430, and their rank by
  (corner y, corner x, index).
- `peek-scale.tsv`: the peek radii and the thresholds Q[0] and Q[15] for
  eight zooms (zoom 0.0625 shows the 1024-pixel cap).
- `peek-thresholds.tsv`: the revision 2 scene's 16 ring thresholds (inner 2,
  ring 4): `threshold_q4 = 16 x inner + (b + 1) x ring`, and its square.
- `peek-samples.tsv`: the revision 2 peek test at 16 pixels of the scene
  (center pixel (5, 2), OX = -4, OY = 2). `b` is the Bayer value, `dx` and
  `dy` are whole pixels.
- `scene-uniforms.hex`: the parent folder's scene with the revision 2 peek
  fields (offsets `0x30` to `0x7F`).
- `scene-expected.hex`: the parent folder's scene (`scene.tsv`,
  `scene-sprites.txt`) drawn with the revision 2 peek. It differs from the
  revision 1 canvas at (1, 0) and (0, 1) only.
- `scene2.tsv`, `scene2-sprites.txt`: the second scene (section 5.6), an
  unlit 12-by-8 scene with a cutout drawn with light source `none`, the
  enhanced shroud as a `remap` instance, and a ground-item highlight at
  outline position 100. For the `remap` row, `levels` names the remap table
  (`shr_tbl`: 256 entries per row, row c - 1 for code c).
- `scene2-order.txt`, `scene2-batches.tsv`, `scene2-uniforms.hex`,
  `scene2-expected.hex`: its key order, batch plan, uniform block and
  expected canvas (from a canvas filled with `40`).
- `blend-clamp.tsv`: the gray-level bound (section 5.7) with a synthetic
  gray rule that exceeds 7, as only a hand-built ink can (palette gray
  levels never exceed 7): `gray_raw` is the unclamped level,
  `gray_used` the clamped one, and `blend` is `F_t[gray_used][D]`.

Until the implementer's follow-up adopts revision 2, the merged tests still
read the revision 1 files in the parent folder. That follow-up switches them
to this folder and deletes the superseded parent files: `peek-scale.tsv`,
`peek-thresholds.tsv`, `peek-samples.tsv`, `scene-uniforms.hex`,
`scene-expected.hex` and `sort-keys.tsv` (SPEC-022 section 3.15).
