# SPEC-008 archive fixtures

- `synthetic.dat.hex`: the complete 96-byte archive of section 4.1 (one root
  directory, a `0x40` block-compressed `PACK.TXT` and a raw `PLAIN.TXT`).

Section 4.2's outer blocks and the two-block payload are built inline by the
tests. All bytes are synthetic; no original archive is used.

- `dat.rs` (`vault_fixtures::dat`): builds raw (uncompressed) archives from
  directories and members, the placeholder `MASTER.DAT` and `CRITTER.DAT`
  the launcher's checks accept, and a game folder holding both
  (`fallout1_install`). The launcher tests and `cargo xtask capture-gallery`
  use them.

# SPEC-015 archive fixtures

- `dat2.rs` (`vault_fixtures::dat2`): builds synthetic version 2 archives
  (little-endian; member data, tree, footer). It also reads the zlib vectors
  in `../zlib/` with their regenerated plain data. `launcher_install` writes
  a Fallout 2 folder the launcher's checks accept, from the required
  member names its caller passes.
