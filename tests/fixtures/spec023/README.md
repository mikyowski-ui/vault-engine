# SPEC-023 in-memory VFS and portable storage fixtures

Every file here is synthetic. The Spec-Author sub-agent (Team A) wrote or
computed each one from SPEC-023 alone. No archive, member, slot or manifest
holds game data; file names such as `MASTER.DAT` are used only as the
public names the engine looks for. See SPEC-023 section 5. These vectors
live here, not in `oracle/`, which is reserved for research-side vectors.

Text formats: TSV files have one header row. Hexadecimal values are
uppercase. A `-` means "not used" (or, in `expect`, an empty list).

Archives, as `OFFSET: bytes` lines of 16 bytes (SPEC-008 section 4.1 form):

- `f1-master.dat.hex` (217 bytes) and `f1-critter.dat.hex` (132 bytes):
  version 1 archives (SPEC-008, big-endian). `DATA\PACK.TXT` is one
  LZSS block that decodes to `ABCABCABC`; every other member is raw.
- `f2-master.dat.hex` (142 bytes), `f2-patch.dat.hex` (94 bytes) and
  `f2-critter.dat.hex` (55 bytes): version 2 archives (SPEC-015,
  little-endian). `data\zip.txt` is a zlib stream with one stored DEFLATE
  block of `HELLO` and Adler-32 `044C0175`.

Mounts and drops:

- `f1-loose.tsv`, `f2-loose.tsv`: the loose override trees (path as given,
  ASCII content).
- `lookups.tsv`: requests against the mounts `f1`, `f2` and `f2-nopatch`
  (SPEC-023 section 5.3): the content, or `NOT_FOUND`, and the part that
  served it.
- `refusals.tsv`: 13 inputs that must be refused, with single-byte
  mutations given by offset.
- `drops.tsv`: dropped file lists and the mount, report or refusal they
  give (SPEC-023 section 3.5).

Storage:

- `keys.tsv`: storage keys, their canonical form and their key-value record
  key, or the refusal.
- `storage-ops.tsv`: a 20-step conformance script that every backend must
  pass with the same results. A commit's `data` is a `;`-separated list of
  `put KEY=VALUE` and `del KEY`; the value runs from the first `=` to the
  next `;` or the end of the field.
- `quota.tsv`: the reference in-memory backend under a 20-byte quota
  (usage counts canonical key bytes plus value bytes).
- `mods-entries.tsv`, `mods-expected.tsv`: entries of the `mods` namespace,
  and the manifest discovery, layer finding and layer mounting results.
