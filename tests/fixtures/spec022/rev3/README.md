# SPEC-022 revision 3 fixtures

Every file here is synthetic. The Spec-Author sub-agent (Team A) computed
each one from SPEC-022 revision 3 alone. None holds game data. See SPEC-022
sections 3.16 and 5.8, and decisions D-33 to D-41.

Formats are those of the parent folder's README: TSV files have one header
row, hexadecimal values are uppercase, and `-` means "not used".

- `square-rank.tsv`: nine floor squares and the four grid corners, at SPEC-003 section 3.6's example translation. `rank` is
  the square's position among all 10000 squares (revision 3), and
  `drawn_rank_rev2` is the rank revision 2 gave it among the nine.
- `highlight-positions.tsv`: ground-item highlight keys. `list_len` is the
  number of entries the outline list holds, `k` the highlight's position in
  the highlight list, and `position` is `list_len + k`.
- `scene2.tsv`, `scene2-order.txt`: the second scene with the highlight `hl`
  at position 1, because its outline list holds one entry. Only `hl`'s key
  and secondary differ from `../rev2/`. The batches, uniforms and canvas of
  `../rev2/` still apply.
- `peek-center.tsv`: the pointer's position, the tile's corner, and the peek
  center `(floor(pointer x) - tile x, floor(pointer y) - tile y)`.
- `peek-ring.tsv`: the ring test at zoom 1.0 (inner 96, ring 48), on and
  beside its boundaries. `q_b` is `Q[b]`, and `floor_16t` is `floor(16t)`.
- `peek-seam.tsv`: the peek test at 12 view pixels, drawn in tile 0 (corner
  (0, 0)) and tile 1 (corner (16, 0)), with center pixel (20, 2) in the view,
  inner 2, ring 4, OX = -4 and OY = 2. Both tiles give the same `cut`.
- `shroud-mask.tsv`: the enhanced shroud's pixel test. `hex_state` is
  `off_grid`, `allowed` or `outside`. `nearest_d_sq` is the squared distance
  from the pixel to the allowed center nearest its hex, or `-` when no allowed
  center lies within 104 pixels. `dark` is 1 when the pixel takes the dark
  index.
- `unlit-modes.tsv`: the draw mode and light source of each item in a lit
  scene and in an unlit scene.

The files of `../rev2/` stay current. The implementer's follow-up deleted the
three that this folder superseded: `square-order.tsv`, by `square-rank.tsv`, and
`scene2.tsv` and `scene2-order.txt`, by the files of the same names here
(SPEC-022 section 3.16).
