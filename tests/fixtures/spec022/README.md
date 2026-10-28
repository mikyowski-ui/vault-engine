# SPEC-022 instanced rendering and hex light fixtures

Every file here is synthetic. The Spec-Author sub-agent (Team A) computed
each one from SPEC-022 alone. None holds game data, and the scene uses the
formula tables of SPEC-022 section 5.1 instead of a palette. The light values
follow from SPEC-006 section 3.6's falloff formula, and the digests use
SPEC-021 section 2.1's FNV-1a 64. These vectors live here, not in `oracle/`,
which is reserved for research-side vectors.

Text formats: TSV files have one header row. Hexadecimal values are uppercase.
A `-` means "not used".

- `sort-keys.tsv`: sort keys (section 2.1) from layer, primary and secondary.
- `scene.tsv`: the 16 instances of the section 5.4 scene, deliberately out of
  key order. `flags` is decimal (bit 0 EXEMPT, bit 1 PEEK). `aux` is 4
  hexadecimal digits (bits 0-2 blend table, bits 8-10 gray level). `clip` is
  the half-open rectangle `x0,y0,x1,y1`. `light` is `none`, `hex`,
  `ambient`, `fixed` or `map` (a level image, named in `levels`). `src` names
  an image of `scene-sprites.txt`.
- `scene-order.txt`: the instances in ascending key order, as `key<TAB>id`.
- `scene-batches.tsv`: the batch plan after culling (`flat_x` is culled). The
  `snapshot` column is the bounding box of a read batch's clipped
  rectangles.
- `scene-sprites.txt`: the scene's images: sprites, the ring images `ring_o`
  (final indexes) and `ring_t` (paint counts), the level image `lvl_m`, and
  the synthetic cutout mask `egg`. Each starts with `image NAME WIDTH HEIGHT`,
  followed by one line per row of hexadecimal bytes.
- `scene-uniforms.hex`: the 128-byte layer uniform block of the scene
  (section 2.4), as offset-addressed hex.
- `scene-expected.hex`: the 24-by-16 canvas after the scene, starting from a
  canvas filled with `40`. Each line is one canvas row, prefixed with the
  row's byte offset.
- `light-falloff.tsv`: every hex of the two-light world of section 5.3 whose
  accumulated light differs from 655, and three that do not (hexes 0, 19600
  and 39999). It gives distances, the accumulation, the packed entry, and the
  effective light and level at ambient 32768. The packed buffer's digest is
  `D45003EF0BA78613`.
- `light-pack.tsv`: the clamp of an accumulated light to `[0, 65536]`.
- `light-levels.tsv`: effective light E and level L for each light source,
  using the falloff world's buffer for the `hex` rows.
- `peek-thresholds.tsv`: the scene's 16 peek thresholds (inner radius 32,
  ring 48, in 1/16 pixel).
- `peek-scale.tsv`: the default peek's radii for five canvas scales.
- `peek-samples.tsv`: the peek test at 14 pixels of the scene (center
  (88, 40) in 1/16 pixel, OX = -4, OY = 2).
