# SPEC-019 loot layout fixture

Synthetic. Team A computed `loot-layout.tsv` from SPEC-019 section 3.7 and SPEC-013
section 3.16. Coordinates are relative to the window, which sits at screen (80, 0), and
`screen_x` and `screen_y` add that origin.

`kind` is `window`, `box` (a drawn element or a hit box) or `drop` (a release zone over
a list). Under `fallout2` no two `box` rows overlap. Under `fallout1` the rows are
SPEC-013's, which puts the two body-cycling buttons inside `target_row_2` and
`target_row_3`, and the enhanced Take All button is not listed.
