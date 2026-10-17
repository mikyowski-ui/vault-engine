# SPEC-008 archive fixtures

- `synthetic.dat.hex`: the complete 96-byte archive of section 4.1 (one root
  directory, a `0x40` block-compressed `PACK.TXT` and a raw `PLAIN.TXT`).

Section 4.2's outer blocks and the two-block payload are built inline by the
tests. All bytes are synthetic; no original archive is used.
