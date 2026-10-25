# SPEC-008 archive fixtures

- `synthetic.dat.hex`: the complete 96-byte archive of section 4.1 (one root
  directory, a `0x40` block-compressed `PACK.TXT` and a raw `PLAIN.TXT`).

Section 4.2's outer blocks and the two-block payload are built inline by the
tests. All bytes are synthetic; no original archive is used.

# SPEC-015 archive fixtures

- `dat2.rs` (`vault_fixtures::dat2`): builds synthetic version 2 archives
  (little-endian; member data, tree, footer). It also reads the zlib vectors
  in `../zlib/` with their regenerated plain data.
