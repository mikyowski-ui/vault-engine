# Extended art fixtures

`support.rs` builds the synthetic inputs of `docs/extended-art-namespaces.md`
by hand, so it has no dependency on `vault-formats`.

- `art_list(prefix, rows) -> String`: `<prefix>0000.frm` and on, one per line
  (vector A is `art_list("s", 4100)`).
- `manifest(name, art, protos, remaps) -> String`: `vault-mod.toml` text with
  `[[art]]` and `[[protos]]` rows `(category, file, index)` and `[[tile_remap]]`
  rows `(map, file)`.
- `tile_remap(pairs) -> Vec<u8>`: a `VTLX` sidecar from `(local index, row)`
  pairs (vector E is `tile_remap(&[(4000, 5000)])`). Pairs are written as
  given, so an invalid sidecar can be built.

Parser checks that use these builders are in `tools/asset-audit/tests/cli.rs`.
No game data.
