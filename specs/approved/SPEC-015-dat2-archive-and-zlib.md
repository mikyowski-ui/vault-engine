# SPEC-015: Fallout 2 Archive (DAT Version 2) and zlib Members

**Status:** approved by the project owner, 2026-10-25.

**Sources.** This specification has exactly two sources:

1. The project owner's written brief of 2026-10-25 for the archive layout, and
   the owner's answers to three follow-up questions on the same day:
   - the path length prefix is a 4-byte little-endian byte count;
   - stored paths are read with either slash as the separator, folding case;
   - compressed members are zlib streams (RFC 1950).
2. The public IETF standards RFC 1950 (ZLIB Compressed Data Format
   Specification version 3.3) and RFC 1951 (DEFLATE Compressed Data Format
   Specification version 1.3).

Nothing here comes from game executables, their decompilations or any other
engine. A point neither source settles is either an **engine safe rule**,
marked `[Safe rule]`, which refuses what it does not define, or an open
question in section 7.

## 1. Scope and coexistence

This specification defines a second archive format, used by Fallout 2's data
files. It does not change SPEC-008. The engine selects the archive format by
the game profile (`game = "fallout1"` or `"fallout2"`):

- Under `fallout1`, archives are read only as SPEC-008 archives, and every
  SPEC-008 rule is unchanged. `[Must-match]`
- Under `fallout2`, archives are read only as the format below. A SPEC-008
  archive is refused there, and a version 2 archive is refused under
  `fallout1`. `[Safe rule]`

All integers in this format are **little-endian**. (SPEC-008's are
big-endian.) `[Must-match]`

## 2. Footer

The last 8 bytes of the archive are the footer:

| Offset from the footer | Size | Field |
|---|---|---|
| 0 | 4 | `tree_size`: u32, the directory tree's length in bytes |
| 4 | 4 | `file_size`: u32, the archive's total length in bytes |

`[Must-match]`

- An archive shorter than 8 bytes is refused. `[Safe rule]`
- `file_size` must equal the archive's actual length. A mismatch is refused
  (a truncated or extended file). `[Safe rule]`
- `tree_size` must leave room for the tree and the footer:
  `tree_size + 8 <= file_size`. Otherwise refused. `[Safe rule]`
- A tree larger than 64 MiB (the engine's per-asset limit) is refused
  before it is read. `[Safe rule]`

## 3. Directory tree

The tree starts at byte `T = file_size - 8 - tree_size` and is exactly
`tree_size` bytes long. `[Must-match]`

| Size | Field |
|---|---|
| 4 | `count`: u32, the number of entries |
| per entry | the entries, in stored order |

Each entry:

| Size | Field |
|---|---|
| 4 | `name_length`: u32, the path's length in bytes |
| `name_length` | the path bytes |
| 1 | `flags`: u8, `1` = compressed, `0` = stored |
| 4 | `real_size`: u32, the member's logical length |
| 4 | `packed_size`: u32, the member's stored length |
| 4 | `offset`: u32, the stored bytes' position from the archive's start |

`[Must-match]` for the field order and widths; the rules below are safe
rules.

- The entries must consume exactly `tree_size` bytes. A tree that ends
  early, or that has bytes left over after `count` entries, is refused.
  `[Safe rule]`
- `count` is bounded by what the tree can hold: each entry takes at least 17
  bytes, so `count * 17 + 4 > tree_size` is refused before any entry is
  read. `[Safe rule]`
- `flags` is 0 (stored) or 1 (zlib-compressed). Any other value is refused.
  `[Must-match: owner decision, 2026-10-25]`
- A member's stored bytes must lie wholly before the tree:
  `offset + packed_size <= T`. Otherwise refused. `[Safe rule]`
- A stored member (`flags = 0`) must have `real_size == packed_size`.
  `[Safe rule]`
- `real_size` above the engine's per-asset limit (64 MiB, as SPEC-008's
  extraction limit) is refused when the member is extracted, before any
  decompression. `[Safe rule]`

## 4. Paths

- A path is 1 to 1024 bytes of printable ASCII (`0x20` to `0x7E`). Anything
  else is refused. `[Safe rule]`
- Either `\` or `/` separates directories, and ASCII letters are folded to
  lowercase. `[Must-match: owner decision, 2026-10-25]`
- A path is looked up and listed in the engine's canonical form: lowercase,
  `/`-separated, with the same component rules as every other loose or
  archived path (no empty, `.` or `..` component). A path that has no
  canonical form is refused. `[Safe rule]`
- Two entries whose canonical paths are equal are refused (an ambiguous
  archive), not resolved by order. `[Safe rule]`

## 5. Members

- A **stored** member's logical bytes are its `packed_size` bytes at
  `offset`. `[Must-match]`
- A **compressed** member's `packed_size` bytes at `offset` are exactly one
  zlib stream (RFC 1950) whose decompressed data is the member's logical
  bytes. `[Must-match]`

### 5.1 The zlib wrapper (RFC 1950)

- `CMF`: compression method (low 4 bits) must be 8 (DEFLATE), and the window
  information (high 4 bits) at most 7. `[Must-match: RFC 1950 §2.2]`
- `FLG`: `CMF * 256 + FLG` must be a multiple of 31.
  `[Must-match: RFC 1950 §2.2]`
- A preset dictionary (`FDICT`, bit 5 of `FLG`) is refused: no dictionary is
  defined for these archives. `[Safe rule]`
- The DEFLATE data follows (section 5.2). After its final block, the stream
  is aligned to a byte boundary, and the next 4 bytes are the Adler-32
  checksum of the decompressed data, **big-endian** (RFC 1950's own byte
  order). A checksum mismatch is refused. `[Must-match: RFC 1950 §2.2, §9]`
- The stream must end exactly at `packed_size`. Bytes after the checksum, or
  a stream that needs bytes past `packed_size`, are refused. `[Safe rule]`

### 5.2 DEFLATE (RFC 1951)

The decoder follows RFC 1951 §3.2 exactly: stored blocks (with the `LEN` /
`NLEN` complement check), fixed Huffman codes, and dynamic Huffman codes,
with the length and distance tables of §3.2.5 and the code-length alphabet
of §3.2.7. `[Must-match: RFC 1951]` It applies these bounds:

- The output is limited to `real_size` bytes. Producing a byte past it is
  refused at once, with no further growth. `[Safe rule]`
- At the end of the final block the output must be exactly `real_size`
  bytes. `[Safe rule]`
- These are refused (RFC 1951 leaves their effect undefined):
  - block type 3;
  - length symbols 286 and 287, and distance symbols 30 and 31;
  - a distance farther back than the output produced so far;
  - a repeat code 16 with no previous length;
  - code lengths that run past the counts `HLIT` and `HDIST` give;
  - an over-subscribed Huffman code;
  - a literal/length code with no end-of-block symbol;
  - running out of input. `[Safe rule]`

## 6. Errors and the VFS

- A refused archive fails to open, naming the rule. A refused member fails
  its read, naming the member. No partial output is returned. `[Safe rule]`
- The archive is a read-only VFS provider: its file list is the canonical
  paths in canonical order, and a read of a listed path extracts that
  member. `[Safe rule]`

## 7. Decisions

The open questions of the first revision, resolved by the project owner on
2026-10-25:

1. **Path form.** Both `/` and `\` separate directories, and ASCII case
   folds to lowercase (section 4).
2. **Flag values.** Only 0 (stored) and 1 (zlib-compressed) are valid; any
   other byte is refused (section 3).
3. **Archive roles.** Which archive serves which database is part of mounting
   a Fallout 2 game, so it is specified in SPEC-018: `MASTER.DAT` is the
   master database, `CRITTER.DAT` the critter-art database, and an optional
   `patch000.dat` sits above `MASTER.DAT`.

## 8. Test vectors

`tests/fixtures/zlib/vectors.hex` holds zlib streams produced by CPython's
standard `zlib` module (a third-party RFC 1950 implementation, used only as
an independent encoder). Each vector's plain data is generated
deterministically by the tests. The vectors cover:

- an empty stream;
- fixed-Huffman blocks;
- dynamic-Huffman blocks;
- stored blocks at compression level 0;
- long back-references (repetitive data);
- data larger than one stored block.

The tests also build synthetic version 2 archives from these members, and
damage them to exercise every safe rule in sections 2 to 5.
