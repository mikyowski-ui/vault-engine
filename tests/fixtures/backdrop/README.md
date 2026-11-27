# Backdrop fixtures

`support.rs` builds synthetic pre-rendered backdrops (`.bkd` index plus `.bkc`
chunks) from in-memory colour, depth and fade arrays, by hand, so it has no
dependency on `vault-formats`. Layout: `docs/prerendered-backdrop-cutaway.md`
sections 3.1-3.3 (big-endian; 24-byte header, 32-byte elevation records, then
per elevation the presence table and suppressed PIDs; chunk files have a
16-byte header and a stored or zlib payload of colour, depth and fade planes).

- `build(chunk_size, zlib, palette_hash, &[Elevation]) -> Backdrop` returns the
  index bytes and `(elevation, column, row, bytes)` for every chunk.
- `vector_b()` is the brief's vector B (4 by 1 pixels).

zlib streams use stored DEFLATE blocks with a correct Adler-32. No game data.
Parser tests are in `crates/vault-formats/src/backdrop_tests.rs`.
