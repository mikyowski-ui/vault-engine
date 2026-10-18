# SPEC-012 movie fixtures

Synthetic vectors from SPEC-012 section 4, authored from the specification:

- `fixture-a.hex`: Fixture A, the 26-byte header and the first chunk header.
- `fixture-b.hex`: Fixture B, the chunk walk after the header (timer, end of
  chunk, end of stream, and the trailing header the read-ahead rule needs).
- `fixture-f.sve`: Fixture F subtitles (CR LF line ends).
- `fixture-g.cfg`: Fixture G fades (effect A `in` from frame 0 for 10 steps,
  effect B `out` from frame 35 for 35 steps).

`support.rs` loads them, builds the A2-A5 header variants, and exposes the
two synthetic movies of Fixture H, which live in `oracle/SPEC-012/synthetic/`
with their expected hashes and decoded frames. Tests that use them are in
`crates/vault-formats/src/movie_tests.rs` and `crates/vault-core/src/movie_tests.rs`.
None of these files contain game data.
