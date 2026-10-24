# SPEC-012: Movies (.MVE), Movie Fades, and Subtitles (.SVE)

## Clean-Room Rules for Research Agents
- Document ONLY binary file layouts (byte offsets, endianness, data types), mathematical formulas, state transitions, and observable behavior.
- DO NOT copy C/C++ code, struct definitions, function names, internal variable names, or implementation-source paths from the inspected implementation.
- Provide small synthetic hex/text test vectors at the bottom of the specification.

## 1. Overview & Scope

This specification describes reference revision R1, targeting the 32-bit Windows engine. Evidence comes from two sources:

- **Static inspection** of the movie library (container reader, frame timer, audio buffering, video block decoder, palette handling), the engine-side movie player (presentation, subtitles), the movie-fade module, the game movie table, and the `play_gmovie` opcode and other callers.
- **Measurements** over the local unpacked corpus: 13 `.MVE` files under `art/cuts`, 7 fade files (`art/cuts/*.cfg`), and 15 `.SVE` files (5 per language for English, French and Spanish). A research-side oracle decoder that follows the rules below produced the vectors in `oracle/SPEC-012/` (section 4.9). A scratch harness compiled the reference block decoder and compared it frame by frame against the oracle. All **8613** corpus video frames and all synthetic frames matched byte for byte.

No original executable was run. No corpus pixels, PCM samples, or subtitle texts are reproduced.

This document covers:

- the `.MVE` file header, chunk framing with its read-ahead rule, and opcode framing;
- every opcode the reference interprets, and the ones it skips;
- the 8-bit block video codec: buffers, swap, decoding map, all sixteen block types, and the motion tables;
- palette loading and the device-palette push at frame display;
- the DPCM audio codec, silence frames, track selection, and output buffering;
- frame pacing, frame counting, dropping, and display placement;
- movie fade files and their palette effects;
- `.SVE` subtitle parsing, timing, and drawing;
- the 14-entry game movie table, file paths, the playback flow and its exit rules, and the `play_gmovie` opcode.

Relationship to other specifications: SPEC-001 defines `.PAL` color tables and 6-bit palette levels. SPEC-002 defines fonts and text wrapping. SPEC-005 defines opcode stack contracts. SPEC-006 defines the dialogue background-processing gate. SPEC-008 defines DAT/patch lookup and the 14-byte played-movie save component (component 22). SPEC-009 §2.2 defines the INI grammar reused by fade files. SPEC-010 owns other audio formats.

**Compatibility tags.** Every documented quirk carries one tag:

- `[Must-match]`: decoded pixels and PCM, file layouts, timing formulas, ordering, and gameplay-visible playback rules. A clean-room engine must reproduce it exactly.
- `[May-improve]`: reference memory-safety defects, unchecked indexes, undefined initial contents, presentation-device scheduling details, or a reference bug that is unreachable with shipped data. A clean-room engine may apply the stated safe rule. It should keep every other observable behavior. Where the corpus never triggers a `[May-improve]` case, this is stated.

Terminology (descriptive, not reference identifiers):

| Term | Meaning |
| --- | --- |
| Chunk | Top-level container unit: a 4-byte header and a body of opcodes |
| Opcode | One typed record inside a chunk body |
| Current buffer | The 8-bit frame buffer being decoded and displayed |
| Other buffer | The second frame buffer, used as the "previous frame" |
| Block | An 8 × 8 pixel cell; the frame is a grid of blocks |
| Palette table | The 256 × 3 table of 6-bit levels filled by palette opcodes |
| Device palette | The palette entries actually pushed to the display |
| Frame counter | The number of show-frame opcodes processed so far (section 3.7) |
| Period | Frame duration in microseconds (section 3.3) |

## 2. Binary Layout / Data Format (Byte Offset Tables)

### 2.1 Numeric Conventions

All multi-byte integers in `.MVE` files are **little-endian**: u16 is unsigned 16-bit and u32 is unsigned 32-bit. This is the opposite of the big-endian prototype, map and save formats in SPEC-003 and SPEC-008. `[Must-match]` `.SVE` and fade `.cfg` files are byte-oriented text, so endianness does not apply to them. Pixel values are 8-bit palette indexes. Palette levels are 6-bit values (0-63) stored one per byte. PCM samples are signed 16-bit values; a decoder's output buffer stores them little-endian.

### 2.2 File Header (26 bytes)

| Offset | Size | Type | Value / meaning |
| --- | ---: | --- | --- |
| `0x00` | 20 | bytes | `Interplay MVE File` followed by `0x1A 0x00` |
| `0x14` | 2 | u16 | Header size; must be 26 |
| `0x16` | 2 | u16 | Version; must be 256 (`0x0100`) |
| `0x18` | 2 | u16 | Check value; must satisfy `version + check = 0x1233`, so 4403 (`0x1133`) |
| `0x1A` | 4 | u16, u16 | Header of the first chunk (length, type); see 2.3 |

All twenty signature bytes are compared, including the trailing NUL. `[Must-match]` The reference's check is `(bitwise-NOT version) - check = 0xFFFFEDCC` in 32-bit arithmetic, which is equivalent to the sum rule. `[Must-match]` A file that fails any test is rejected and nothing is played. `[Must-match]`

### 2.3 Chunk Framing and the Read-Ahead Rule

| Chunk-relative offset | Size | Type | Meaning |
| --- | ---: | --- | --- |
| `+0` | 2 | u16 | Body length L in bytes (0-65535) |
| `+2` | 2 | u16 | Chunk type; **ignored** by the player |
| `+4` | L | bytes | Opcodes, back to back |

- The reader fetches each chunk as one record of **L + 4 bytes**: the body plus the **next** chunk's 4-byte header. The first chunk's header comes from header offset `0x1A`. `[Must-match]`
- A record that cannot be read in full ends playback with an error. `[Must-match]`
- A well-formed file therefore ends with an extra 4-byte chunk header after the last chunk. The corpus files all end with a zero-length header of type 5. `[Must-match]`
- Opcodes never straddle chunks. `[Must-match]`
- The reference does not check opcode lengths against L. `[May-improve]` Safe rule: treat an opcode whose 4-byte header or payload runs past the body as a stream error that ends playback.

### 2.4 Opcode Framing

| Opcode-relative offset | Size | Type | Meaning |
| --- | ---: | --- | --- |
| `+0` | 2 | u16 | Payload length N |
| `+2` | 1 | u8 | Opcode type |
| `+3` | 1 | u8 | Opcode version |
| `+4` | N | bytes | Payload |

### 2.5 Opcode Table

Offsets are relative to the payload. Every opcode is `[Must-match]`.

| Type | Name (descriptive) | Payload | Reference action |
| ---: | --- | --- | --- |
| 0 | End of stream | none | Playback finishes normally (section 3.2) |
| 1 | End of chunk | none | Fetch the next chunk record |
| 2 | Create timer | `+0` u32 rate (µs); `+4` u16 subdivision | Set the frame period (section 3.3) |
| 3 | Configure audio | `+0` u16 unused; `+2` u16 flags; `+4` u16 sample rate; `+6` buffer size: **u32 if version ≥ 1, else u16** | Flags: bit 0 stereo, bit 1 16-bit, bit 2 compressed (honored only if version ≥ 1). Other bits are ignored |
| 4 | Frame sync point | none | Wait for the frame deadline; start or stop audio output (section 3.3) |
| 5 | Configure video | `+0` u16 width in blocks; `+2` u16 height in blocks; `+4` u16 height multiplier (version ≥ 1, else 1); `+6` u16 high-color flag (version ≥ 2, else 0) | Reallocate both frame buffers (section 3.4) |
| 7 | Show frame | `+0` u16 palette start; `+2` u16 palette count; `+4` u16 field-mode flag (version ≥ 1, else 0) | Push the palette range, display the current buffer, count the frame (section 3.7) |
| 8 | Audio data | `+0` u16 sequence; `+2` u16 track mask; `+4` u16 decoded length in bytes; `+6` codec bytes | Decode into the output stream if the mask selects the track (section 3.8) |
| 9 | Audio silence | `+0` u16 sequence; `+2` u16 track mask; `+4` u16 length in bytes | Emit silence if the mask selects the track |
| 10 | Video mode | any | Ignored |
| 11 | Synthesize palette | six u16: base1, rows1, cols1, base2, rows2, cols2 | Section 3.6; never present in the corpus |
| 12 | Load palette | `+0` u16 start; `+2` u16 count; `+4` 3 × count bytes | Copy into the palette table (section 3.6) |
| 14 | Suppress display | any | Marks the chunk: its show-frame opcode does not display (section 3.7) |
| 15 | Decoding map | W × H / 2 bytes | The block-type map for the next video data opcode in the same chunk |
| 17 (`0x11`) | Video data | `+0` u16, `+2` u16 (both unused); `+4` u16 x; `+6` u16 y; `+8` u16 w; `+10` u16 h (all in blocks); `+12` u16 flags; `+14` block parameter stream | Decode one frame (sections 3.4-3.5). **Version must be ≥ 3**, or playback ends with an error |
| other (6, 13, 16, 19, 21, …) | — | any | Skipped without effect |

The corpus uses types 0, 1, 2, 3, 4, 5, 7, 8, 9, 10, 12, 15, 17, 19 and 21, with versions 3:1, 5:2, 7:1 and 17:3 (all others version 0). `[Must-match]`

### 2.6 Decoding Map

The decoding map holds one 4-bit block type per block of the decode region, in raster order (row by row, left to right). Each byte holds two blocks: the **low nibble first**, then the high nibble. `[Must-match]` The map is cleared at the start of every chunk; a video data opcode uses the most recent map opcode **in the same chunk**. `[Must-match]` The reference has no check for a missing map. `[May-improve]` Safe rule: a video data opcode with no map in its chunk is a stream error that ends playback.

### 2.7 Block Parameter Stream

Block parameters are consumed in block order from the single stream at video payload `+14`. The table gives the bytes each type consumes. "P0 ≤ P1" compares unsigned bytes. All rows are `[Must-match]`.

| Type | Bytes | Layout |
| ---: | ---: | --- |
| 0, 1 | 0 | — |
| 2, 3, 4 | 1 | Motion code B |
| 5 | 2 | Signed x offset, signed y offset (two's-complement bytes) |
| 6 | 0 | Section 3.5 |
| 7 | 10 if P0 ≤ P1, else 4 | P0, P1, then 8 bitmap bytes (or 2) |
| 8 | 16 if P0 ≤ P1, else 12 | Quadrants: four groups of (P0, P1, 2 bitmap bytes). Halves: (P0, P1, 4 bytes, P2, P3, 4 bytes) |
| 9 | 4 + 16, 4 + 4, or 4 + 8 | P0-P3, then a bitmap sized by subcase (section 3.5) |
| 10 | 32 if P0 ≤ P1, else 24 | Quadrants: four groups of (P0-P3, 4 bitmap bytes). Halves: (P0-P3, 8 bytes, P4-P7, 8 bytes) |
| 11 | 64 | 64 raw pixels, row-major |
| 12 | 16 | 16 pixels, one per 2 × 2 cell, row-major |
| 13 | 4 | One pixel per 4 × 4 quadrant: top-left, top-right, bottom-left, bottom-right |
| 14 | 1 | One pixel |
| 15 | 2 | P0, P1 |

The reference never checks the stream against the payload length. `[May-improve]` Safe rule: reading past the payload is a stream error that ends playback. The corpus never over-reads (section 4.9).

### 2.8 Audio Delta Table

The compressed audio codec adds `D[code]` to a 16-bit predictor. D has 256 u16 entries, listed row-major (row r holds codes 16r … 16r+15):

```text
0000 0001 0002 0003 0004 0005 0006 0007 0008 0009 000A 000B 000C 000D 000E 000F
0010 0011 0012 0013 0014 0015 0016 0017 0018 0019 001A 001B 001C 001D 001E 001F
0020 0021 0022 0023 0024 0025 0026 0027 0028 0029 002A 002B 002F 0033 0038 003D
0042 0048 004F 0056 005E 0066 0070 007A 0085 0091 009E 00AD 00BD 00CE 00E1 00F5
010B 0124 013E 015C 017B 019E 01C4 01ED 021A 024B 0280 02BB 02FB 0340 038C 03DF
0439 049C 0508 057D 05FE 0689 0722 07C9 087F 0945 0A1E 0B0A 0C0C 0D25 0E58 0FA8
1115 12A4 1458 1633 183A 1A6F 1CD9 1F7B 225A 257D 28E8 2CA4 30B7 3529 3A03 3F4E
4515 4B62 5244 59C5 61F6 6AE7 74A8 7F4D 8AEB 9798 A56E B486 C4FF D6F9 EA97 FFFF
0001 0001 1569 2907 3B01 4B7A 5A92 6868 7515 80B3 8B58 9519 9E0A A63B ADBC B49E
BAEB C0B2 C5FD CAD7 CF49 D35C D718 DA83 DDA6 E085 E327 E591 E7C6 E9CD EBA8 ED5C
EEEB F058 F1A8 F2DB F3F4 F4F6 F5E2 F6BB F781 F837 F8DE F977 FA02 FA83 FAF8 FB64
FBC7 FC21 FC74 FCC0 FD05 FD45 FD80 FDB5 FDE6 FE13 FE3C FE62 FE85 FEA4 FEC2 FEDC
FEF5 FF0B FF1F FF32 FF43 FF53 FF62 FF6F FF7B FF86 FF90 FF9A FFA2 FFAA FFB1 FFB8
FFBE FFC3 FFC8 FFCD FFD1 FFD5 FFD6 FFD7 FFD8 FFD9 FFDA FFDB FFDC FFDD FFDE FFDF
FFE0 FFE1 FFE2 FFE3 FFE4 FFE5 FFE6 FFE7 FFE8 FFE9 FFEA FFEB FFEC FFEE FFED FFEF
FFF0 FFF1 FFF2 FFF3 FFF4 FFF5 FFF6 FFF7 FFF8 FFF9 FFFA FFFB FFFC FFFD FFFE FFFF
```

The table is engine data and is `[Must-match]` exactly. Read as signed 16-bit, codes 120-127 are negative (`8AEB` = -29973 at code 120 … `FFFF` = -1 at code 127), codes 128 and 129 are both +1, codes 130-136 are positive, and codes 137-255 are negative, starting at `80B3` = -32589. Codes 237 and 238 hold `FFEE` and `FFED`, out of monotonic order, and must be reproduced as listed.

### 2.9 `.SVE` Subtitle File

A `.SVE` file is plain text. Each line has the form `<frame number>:<text>`. The corpus uses CR LF line ends. Parsing rules are in section 3.10. `[Must-match]`

### 2.10 Movie Fade File (`.cfg`)

A fade file uses the SPEC-009 §2.2 INI grammar: `;` starts a comment, section names and values are whitespace-trimmed, and the last duplicate key wins. Keys:

| Section | Key | Value |
| --- | --- | --- |
| `info` | `total_effects` | Integer E |
| `info` | `effect_frames` | E comma-separated integers: the start frame of each effect |
| `<start frame>` (decimal, no sign or padding) | `fade_type` | `in` or `out`, case-insensitive |
| `<start frame>` | `fade_color` | Three comma-separated integers R, G, B (6-bit levels; stored modulo 256) |
| `<start frame>` | `fade_steps` | Integer S: the number of frames the fade lasts |

The key names and value grammar above are `[Must-match]`. `art/cuts/movie.cfg` is a commented example that no movie uses. `[Must-match]`

### 2.11 Game Movie Table and Saved State

| Index | File (under `art\cuts\`) | `play_gmovie` flags | Index | File | `play_gmovie` flags |
| ---: | --- | --- | ---: | --- | --- |
| 0 | `iplogo.mve` | 0x0B | 7 | `ovrrun.mve` | 0x09 |
| 1 | `mplogo.mve` | 0x0B | 8 | `walkm.mve` | 0x0B |
| 2 | `intro.mve` | 0x0B | 9 | `walkw.mve` | 0x0B |
| 3 | `vexpld.mve` | 0x09 | 10 | `dipedv.mve` | 0x09 |
| 4 | `cathexp.mve` | 0x09 | 11 | `boil1.mve` | 0x0B |
| 5 | `ovrintro.mve` | 0x0B | 12 | `boil2.mve` | 0x0B |
| 6 | `boil3.mve` | 0x09 | 13 | `raekills.mve` | 0x09 |

Flag bits: `0x01` fade to black before; `0x02` fade back to the game palette after; `0x04` stop background music; `0x08` pause background music for the duration. `[Must-match]` `raekills.mve` is listed but absent from the corpus (section 3.12). `[Must-match]` The played-movie record is 14 bytes, one per index, saved as SAVE.DAT component 22 (SPEC-008). A byte is 1 once that movie has played to its end or been skipped. `[Must-match]`

## 3. Behavioral Rules & Edge Cases

### 3.1 Opening a Movie

1. Open the file through DAT/patch lookup (SPEC-008). If it cannot be opened, nothing plays. `[Must-match]`
2. Validate the header (section 2.2), then read the first chunk record (section 2.3). Failure of either ends playback before any frame. `[Must-match]`
3. Reset the frame counter and dropped-frame counter to 0. Select audio track 0, i.e. track-mask bit `0x0001`. `[Must-match]`

### 3.2 Processing Loop

Playback advances in **steps**. Each step processes opcodes in stream order until one of these happens:

- A show-frame opcode completes. The step returns "frame shown". `[Must-match]`
- An end-of-stream opcode is reached. The step returns "finished", and the movie closes. `[Must-match]`
- An error occurs: a record read failure, a video data opcode with version < 3, or a failed buffer or audio-device setup. The library stops. `[Must-match]`

Opcodes 2, 3, 4, 5, 8, 9, 11, 12, 14, 15 and 17 take effect immediately in stream order. An end-of-chunk opcode fetches the next record and continues the same step. `[Must-match]`

After a library error, the engine-side player only treats "finished" as end of movie. On an error it keeps its playing state, and every later step returns "not active" without closing the movie. The game's playback loop (section 3.12) then waits for a key, a mouse click, or a quit request. `[May-improve]` Safe rule: treat any library error as end of movie. The corpus never triggers an error.

### 3.3 Frame Timer and Pacing

- **Create timer:** the period is `T = rate_lo × sub + (sub >> 1)` µs, where `rate_lo` is the **low 16 bits** of the u32 rate and `sub` is the u16 subdivision. `[Must-match]` A rate above 65535 is therefore truncated. `[May-improve]` Safe rule: use all 32 bits. The corpus uses rate 8341 or 8928 with subdivision 8, so T = 66732 µs (about 14.99 frames/s) or 71428 µs (cathexp.mve).
- If a timer is already running with the same T, the opcode does nothing. Otherwise the player first waits for the current deadline (if one is running) and then sets the deadline to `now + T`. `[Must-match]`
- **Frame sync point (opcode 4):** busy-wait until `now ≥ deadline + T/4`. Let `late = now - (deadline + T/4)`. Then advance `deadline += T`. The upcoming frame is **late** if `late > T/2` and the previous frame was not dropped. `[Must-match]`
- The wait is a spin on a millisecond clock scaled to microseconds. `[May-improve]` Any wait that meets the same deadlines is acceptable.
- At the end of playback the player waits for the current deadline once more. `[Must-match]`

### 3.4 Video Buffers, Region, and Swap

- **Configure video** sets the frame size to `8 × width_blocks` by `8 × height_blocks × multiplier` pixels and creates **two** 8-bit buffers of that size. Any earlier buffers are discarded. `[Must-match]` Every corpus movie uses 54 × 40 blocks, multiplier 1 and 8-bit color, i.e. **432 × 320** pixels.
- New buffer contents are undefined in the reference. `[May-improve]` Safe rule: zero both buffers. Filling them with 0x00 versus 0xFF changes no corpus frame, so the initial contents are never visible in shipped data.
- High-color mode (flag ≠ 0) uses a different decoder that is absent from this revision, and the player refuses it when the display is not high-color. `[May-improve]` Safe rule: reject high-color movies.
- A multiplier other than 1 breaks the reference's block stepping. `[May-improve]` Safe rule: reject it.
- The reference also refuses a frame wider or taller than the 640 × 480 output surface. `[Must-match]`
- **Video data:**
  1. If flags bit 0 is set, swap the roles of the two buffers first. The old "other" buffer becomes current. `[Must-match]`
  2. Decode the region of w × h blocks whose top-left block is (x, y) into the current buffer, reading the "other" buffer as the previous frame. Blocks outside the region keep their contents. `[Must-match]` Every corpus frame is full-size: x = y = 0, w = 54, h = 40.
  3. Record the region. Display uses the most recent region, even for show-frame opcodes that had no video data. `[Must-match]`
- Width w should be even, because map bytes cover two blocks. `[May-improve]` Safe rule: reject an odd w.

### 3.5 Block Types

Pixel (i, j) of a block is column i and row j, with (0, 0) at the top left. "Bit k" of a byte sequence means bit `k mod 8` of byte `floor(k / 8)`, least-significant first. Motion copies address the source **linearly**: for a destination at buffer coordinates (X, Y), the source byte is at linear offset `(Y + dy) × width + (X + dx)`. An x position outside 0..width-1 therefore wraps into the neighboring row. `[Must-match]` A source outside the whole buffer is a reference memory-safety defect. `[May-improve]` Safe rule: treat it as a stream error. It never occurs in the corpus.

| Type | Rule |
| ---: | --- |
| 0 | Copy the 8 × 8 block at the same position from the other buffer `[Must-match]` |
| 1 | Leave the block unchanged `[Must-match]` |
| 2 | Copy from the **current** buffer at offset (dx, dy) given by B: if B < 56, dx = 8 + (B mod 7) and dy = floor(B / 7); otherwise dx = -14 + ((B - 56) mod 29) and dy = 8 + floor((B - 56) / 29) `[Must-match]` |
| 3 | As type 2 with dx and dy both negated `[Must-match]` |
| 4 | Copy from the **other** buffer with dx = (B AND 15) - 8 and dy = (B >> 4) - 8 `[Must-match]` |
| 5 | Copy from the **other** buffer with dx = first signed byte and dy = second signed byte `[Must-match]` |
| 7 | Two colors. If P0 ≤ P1: pixel (i, j) = P1 if bit `8j + i` of the 8 bitmap bytes is set, else P0. If P0 > P1: the block is a 4 × 4 grid of 2 × 2 cells, and cell (u, v) uses bit `4v + u` of the 2 bitmap bytes `[Must-match]` |
| 8 | If P0 ≤ P1: four 4 × 4 quadrants in the order **top-left, bottom-left, top-right, bottom-right**, each with its own two colors (the first pair is P0, P1) and 2 bitmap bytes; quadrant pixel (i, j) uses bit `4j + i`. If P0 > P1, compare the second color pair at block-parameter offset 6. If **P2 > P3**: top half (8 × 4) then bottom half, each using bit `8j + i` of its 4 bytes. Otherwise: left half (4 × 8) then right half, each using bit `4j + i` of its 4 bytes. Set bit = second color of the pair `[Must-match]` |
| 9 | Four colors; 2-bit values v select P[v], cells are row-major, and cell c uses bits `2c` and `2c+1`. The subcase is chosen by (P0 ≤ P1, P2 ≤ P3): (yes, yes) gives 8 × 8 single pixels from 16 bytes; (yes, no) gives 4 × 4 cells of 2 × 2 pixels from 4 bytes; (no, yes) gives 4 × 8 cells of 2 × 1 (two pixels wide) from 8 bytes; (no, no) gives 8 × 4 cells of 1 × 2 (two pixels tall) from 8 bytes `[Must-match]` |
| 10 | If P0 ≤ P1: four 4 × 4 quadrants in the order top-left, bottom-left, top-right, bottom-right, each with four colors and 4 bytes of 2-bit values (the first group's colors are P0-P3). If P0 > P1, compare the second color set's first two colors at block-parameter offset 12. If greater: top then bottom 8 × 4 halves; otherwise left then right 4 × 8 halves. Each half has four colors and 8 bytes `[Must-match]` |
| 11 | 64 raw pixels, row-major `[Must-match]` |
| 12 | Cell (u, v) of the 4 × 4 grid of 2 × 2 cells = byte `4v + u` `[Must-match]` |
| 13 | Quadrant colors: top-left, top-right, bottom-left, bottom-right `[Must-match]` |
| 14 | All 64 pixels = P0 `[Must-match]` |
| 15 | Pixel (i, j) = P0 if i + j is even, else P1 `[Must-match]` |

Copies proceed row by row from the top, four bytes at a time. For types 2 and 3 the table's offsets always have |dx| ≥ 8 or |dy| ≥ 8, so the source never overlaps the destination block, and the copy order has no visible effect. `[Must-match]`

**Type 6** never occurs in the corpus. In the reference it skips blocks by a count taken from the **low nibble of the current map byte**, plus 2. A type-6 low nibble skips 8; a type-6 high nibble skips (low nibble + 2), in pairs of blocks. The skip can run past the end of a block row and decrement the remaining-row count. When both nibbles are 6, the count wraps around. `[May-improve]` Safe rule: treat type 6 as a stream error that leaves the rest of the frame undecoded. Corpus count of type 6: 0.

### 3.6 Palette Table

- **Load palette (12):** copy `3 × count` bytes to palette-table entries `start … start + count - 1`. Values are stored unchanged as 6-bit levels. The table persists across chunks and segments. `[Must-match]` The reference does not bounds-check `start + count ≤ 256`. `[May-improve]` Safe rule: reject the opcode if the range exceeds 256 entries.
- **Synthesize palette (11):** never present in the corpus. For entries `base1 + j` (j < cols1), the reference writes R = 63 (from the final row iteration), G = 0 and B = `5 × floor(63j / (cols1 - 1)) / 8`. For the second group it writes R = 0 and G = 63 at `base2 + j`, but writes B at `base1 + j`. It divides by zero when rows or cols equal 1. `[May-improve]` Safe rule: ignore opcode 11.

### 3.7 Show Frame, Frame Counting, and Dropping

Processing a show-frame opcode with palette start s and count n:

1. Increment the frame counter. Corpus frame numbers are therefore 1-based. `[Must-match]`
2. **Palette push.** If n > 0, push palette-table entries `s … s+n-1` to the device. Entries outside the range keep their previous device values. `[Must-match]` The corpus pushes s = 1, n = 254 once per segment, so device entries 0 and 255 are never set by any movie and keep whatever the game palette held (black after a fade-in). If n = 0, nothing is pushed.
3. The reference first pushes the same range from an all-zero table before display, then from the real table after display. `[May-improve]` A single push of the real values is equivalent.
4. **Display.** If the chunk contained a suppress-display opcode (14), the frame is not displayed. Otherwise, if the frame is late (section 3.3) and n = 0, the frame is **dropped**: not displayed, and the dropped counter increments. Otherwise display the current buffer (section 3.9). `[Must-match]` Frame dropping is a pacing aid. `[May-improve]` An engine may always display, but must never drop a frame that carries a palette push.
5. Suppressed and dropped frames still count. Subtitle and fade timing use the frame counter (sections 3.10 and 3.11). `[Must-match]`
6. A field-mode flag ≠ 0 selects a display path that is missing from this revision. `[May-improve]` Safe rule: display normally. The corpus always has 0.

Show-frame opcodes without a preceding video data opcode redisplay the unchanged current buffer. The corpus has 365 such frames. `[Must-match]`

### 3.8 Audio

- **Configure audio:** create an output buffer of `floor((size + size/2) / 4) × 4` bytes for the given rate, channels (stereo flag) and sample width (16-bit flag). Without an audio device, all audio opcodes are accepted and ignored. `[Must-match]` Every corpus movie with sound uses 22050 Hz, stereo, 16-bit, compressed, with a buffer size of 88280 or 88196. walkm.mve and walkw.mve have no audio opcodes.
- **Track selection:** an audio data or silence opcode is used only if `track mask AND 0x0001 ≠ 0`. `[Must-match]` In the corpus, data opcodes use mask 1. Silence opcodes use `0xFFFE` (other tracks only, ignored here) or `0xFFFF`; each file has 14 initial `0xFFFF` silences (28 in ovrintro.mve).
- **Silence:** append `length` bytes of 0x00 (16-bit) or 0x80 (8-bit). `[Must-match]`
- **Compressed data**, decoded length L. Every audio data opcode restarts the predictors from its own first bytes:
  - Mono: the first u16 is the initial predictor p and is also output sample 0. Then each of the next `(L - 2) / 2` code bytes c gives `p = (p + D[c]) mod 65536`, output as the next sample. `[Must-match]`
  - Stereo: the first two u16 values are the left and right predictors and are output as the first sample pair. Then `(L - 4) / 4` pairs of code bytes (left code, right code) each update their own channel and are output interleaved left, right. `[Must-match]`
  - Arithmetic **wraps** modulo 2^16 and is never clamped. `[Must-match]` The corpus never overflows a signed 16-bit range, so a clamping decoder matches on shipped files. Synthetic vectors A and B exercise the wrap.
- **Uncompressed data:** the reference copies L bytes from the wrong address (the location of its own source pointer instead of the payload). `[May-improve]` Safe rule: copy the L payload bytes unchanged. Not present in the corpus.
- **Output buffering:** decoded bytes are appended to a circular device buffer. The write position wraps at its end, and the decoder state carries across the wrap. The player tracks the last 60 write positions; when all 60 are pending, a new audio opcode is discarded. At each frame sync point it starts playback when the current frame's audio is inside the buffered span, and stops it on underrun, so audio restarts in step with video. `[May-improve]` These device-cursor details may be replaced by any scheduler that plays the decoded stream in order and in step with the frame timer. The decoded stream content and order are `[Must-match]`.
- **Volume:** movie audio volume is `0.94 ×` the background-music volume, truncated, or 0 when music is disabled (SPEC-008 §3.10). It is converted to device attenuation as `-1000 × log2(32767 / v)` hundredths of a decibel, clamped to -10000..0 and truncated toward zero, with v = 0 giving -10000. Unlike every other game sound (SPEC-010 §3.13), it is not scaled by master volume; master volume 0 silences it only by disabling music. The level is updated whenever the music volume or the music on/off state changes, even when no movie is playing. Pan is centered. `[Must-match]`

### 3.9 Display Placement

The game plays movies in a 640 × 480 window at (0, 0) in **direct** mode. Each displayed frame is copied unscaled and centered: left = `(640 - width) / 2`, top = `(480 - height) / 2`. The source is the recorded region's offset with the full frame size, and the destination size is the region size. `[Must-match]` For the corpus that is 432 × 320 placed at (104, 80). The device palette (section 3.7) maps pixel indexes to colors through the SPEC-001 6-bit levels and gamma. `[Must-match]`

### 3.10 Subtitles

1. **Path.** The subtitle path is `text\<language>\cuts\<movie name>.SVE`, where `<language>` is the `fallout.cfg` `[system]` `language` value. To build it, take the movie path from its last backslash onward and replace everything from its last `.` with `.SVE`. `[Must-match]`
2. **When subtitles are used.** For indexes 6, 11 and 12 (the three boil movies), subtitles are always on. For all other movies the `fallout.cfg` `[preferences]` `subtitles` setting decides. Either way, subtitles are used only if the `.SVE` file exists. `[Must-match]`
3. **Parsing.** Read lines of at most 258 bytes, text mode. Stop at the first read that returns nothing. In each line, cut at the first LF, then at the first CR. If the line contains a `:`, the part before the first colon is converted with leading-integer rules (sign allowed, no digits gives 0) and the rest, which may contain further colons, is the text. A line without a colon is ignored. `[Must-match]` A longer line is split across reads, and the continuation is parsed as its own line. `[May-improve]` Safe rule: read whole lines. The longest corpus line is 106 bytes. Entries keep **file order**; they are not sorted. `[Must-match]`
4. **Timing.** After every step that did not finish the movie, let f be the frame counter. While the first remaining entry has frame number ≤ f, draw it and remove it. `[Must-match]` Several entries can be drawn in one step, and the last one drawn stays visible. An entry with a smaller number placed after a larger one waits for the larger one. An entry with empty text blanks the subtitle band; the corpus uses this 12 times. A drawn subtitle stays until the next one replaces it. `[Must-match]`
5. **Drawing.**
   - The band spans the full 640-pixel width. Its height is `text height + 4`, clipped to the screen bottom.
   - Its top is `y0 = (480 - H - Y - th) / 2 + H + Y`, where H and Y are the last displayed frame's height and top, and th is the font's text height. For the corpus, `y0 = (80 - th) / 2 + 400`.
   - Each draw fills the band with color index 0, then wraps and centers the text in it (SPEC-002).
   - The text color is the RGB555 value (31, 31, 31) looked up in the subtitle color table. The game loads `art\cuts\subtitle.pal` as the color table for the duration and uses font 101 (SPEC-002). Both are restored afterwards. `[Must-match]`
6. **Wrapping and the lines after the first.** The entry text is broken into lines for a width of 640 pixels:
   - Scan the characters, adding each character's width (SPEC-002 §3.1; a space counts as the space width). While the sum is at most 640, add the letter spacing and continue.
   - At the first character that makes the sum exceed 640, step back to the last space before it. The line ends just before that space, and the following spaces are skipped. The next line starts at the first non-space character.

   Each line i would be drawn at `y0 + i × th`, but a line is drawn only when its bottom, `y0 + (i + 1) × th`, does not pass the band bottom, `y0 + band height`. The band is `th + 4` high, so only line 0 is ever drawn. The rest of the text is computed and thrown away: it is not shown later, and the next entry replaces the band. Line 0 is centered at x = `trunc((640 - w) / 2)`, where w is its width including its final letter spacing (SPEC-002 §3.1). It is drawn transparently over the filled band.

   No corpus entry wraps. The widest English entry is 534 pixels and the widest French one 636, so every shipped subtitle shows in full. `[May-improve]`: for an entry wider than 640 pixels, grow the band downward by one text height per extra line, to the screen bottom, and draw every line. Text with no space before the overflow point makes the reference wrapper loop forever. `[May-improve]`: break such a line before the overflowing character.

### 3.11 Movie Fades

1. **Loading.** At playback start the fade file is looked up at the movie path with everything from its last `.` replaced by `.cfg`, for example `art\cuts\intro.cfg`. `[Must-match]`
   - Read `info.total_effects` (E) and `info.effect_frames`. If E ≥ 2, `effect_frames` must contain at least E - 1 commas; otherwise **no** effect loads. With more values, the E-th value is the leading integer of the remainder. `[Must-match]`
   - For each start frame F, the effect section is named by F in decimal. It is skipped if `fade_type` is missing or not in/out, or if `fade_color` lacks two commas, or if `fade_steps` is missing. `[Must-match]`
   - A loaded effect covers frames `F … F + S - 1`. Effects are searched **newest-loaded first**, so with overlapping effects the later-listed one wins. `[Must-match]`
   - When at least one effect loads, the fade hook replaces the palette-push path. Initially the movie is "inside a fade" if any effect starts at frame ≤ 1. `[Must-match]`
2. **Palette pushes.** While a fade hook is active, each palette push (section 3.7) is stored into a source palette. It reaches the device only when not inside a fade. `[Must-match]`
3. **Per-frame update.** After each step, with frame counter f:
   - Find an effect with `F ≤ f ≤ F + S - 1`.
   - If one is found, let `k = f - F + 1`. Fade in: `level = C - trunc(k × (C - src) / S)`. Fade out: `level = src - trunc(k × (src - C) / S)`. Both use truncating division toward zero, for each of the 768 levels, with C the fade color component and src the source palette level. Store the result modulo 256 and set the **whole** device palette to it.
   - "Inside a fade" becomes true if an effect was found, false otherwise. `[Must-match]`
4. **Consequences.**
   - Frame numbers start at 1, so an effect starting at frame 0 shows its first visible step as k = 2. `[Must-match]`
   - When a fade-out ends, the device palette stays at the fade color until the next palette push. In intro.mve, for example, the screen stays black from frame 70 until the fade-in at frame 101. `[Must-match]`
   - The last step of a fade-in equals the source palette, and the last step of a fade-out equals C. `[Must-match]`

The parsed effects for every corpus fade file are in `oracle/SPEC-012/moviefx.tsv`, and the post-fade palette hash for every frame is in `mve-frames.tsv`.

### 3.12 Game Movie Playback Flow

Playing game movie index m with flags g:

1. Build the path `art\cuts\<file>`. If DAT/patch lookup does not find it, return failure. Nothing is shown and the played byte is unchanged. `[Must-match]` This always happens for index 13.
2. If g has `0x01`, fade the game palette to black. `[Must-match]`
3. Open a 640 × 480 window at (0, 0). If g has `0x04`, stop background music; otherwise, if g has `0x08`, pause it. `[Must-match]`
4. Decide on subtitles (section 3.10). If they are on, load the subtitle color table and font. `[Must-match]`
5. Wait until no mouse button is held. Hide the cursor and disable palette color cycling. Start fades (section 3.11) and the movie. `[Must-match]`
6. **Loop.** Each iteration polls input once; one poll runs exactly one movie step. Stop when any of these holds:
   - the movie is no longer playing;
   - a quit request is pending;
   - a key or other input event arrives;
   - a mouse button was pressed at some point during the movie and no button is held now. `[Must-match]`
   Holding a button does not skip; releasing it does. `[Must-match]`
7. **Cleanup.**
   - Stop the movie and the fades, run one final update, and set the palette to black. `[Must-match]`
   - Mark index m as played, even when the movie was skipped. `[Must-match]`
   - Re-enable color cycling, restore the arrow cursor (and its visibility), and if subtitles were on, restore `color.pal`, the font and the text color. `[Must-match]`
   - Close the window. If g has `0x08`, unpause the music. If g has `0x02`, reload `color.pal` (when subtitles were off) and fade to the game palette. `[Must-match]`

Engine callers and their flags:

| Caller | Movies | Flags |
| --- | --- | --- |
| Startup | iplogo, then intro | 0x01, then 0 |
| Main-menu intro button, or the main-menu idle timeout when no demo is selected | intro | 0x08 |
| New game | ovrintro | 0x04 |
| World-map water/vault events (SPEC-008) | boil3; vexpld; cathexp | 0x0B each |
| Endgame | walkm or walkw by player gender | 0 |
| Pip-Boy video archive | selected movie | 0x0B |
| Midnight water countdown, outside dialogue, each movie only if not yet played | boil1 (water ≤ 100), boil2 (≤ 50), boil3 (0) | boil3: 0x05; others: 0x07 |

All these flags are `[Must-match]`.

**Water countdown.** The remaining water is **campaign global 10**, the eleventh `GAME_GLOBAL_VARS` initializer of `data/vault13.gam` (SPEC-006 §2.1), whose shipped value is 150 days. The water chip quest is campaign global 101. Both are ordinary campaign globals, read and written by scripts with `global_var`/`set_global_var`; neither is an exported variable (SPEC-005 §2.2), and the engine does not need any script to supply them. The SPEC-014 §3.41.1 vault-water note reads the same two globals. Each midnight event (SPEC-006 §3.7) runs these steps first: `[Must-match]`

1. If a countdown movie from step 4 is still playing, the midnight handler returns at once: none of its daily work runs and no next midnight event is queued.
2. Unjam all locks.
3. If global 101 is not 2 and global 10 is above 0, subtract 1 from global 10. Then, only when no dialogue is active, choose a movie from the new value: boil1 if the value is at most 100 and the boil1 bit of the water-movie byte is clear (then set that bit); otherwise boil2 if the value is at most 50 and the boil2 bit is clear (then set that bit); otherwise boil3 if the value is 0. One midnight chooses at most one movie, so when scripts have set the value to 50 or less with neither bit set, boil1 plays that midnight and boil2 the next.
4. A chosen movie plays only outside dialogue and only if its played bit is clear (section 3.12, step 7). After boil3 plays, the game ends with a quit request. A chosen movie that was already played does nothing.

The water-movie byte is 0 at a new game and is saved and loaded with the script state. A countdown that reaches 0 while dialogue is active chooses no movie, and later midnights never decrement a value of 0, so boil3 and the water game over never come from the countdown in that game. `[May-improve]` Safe rule: when global 10 is already 0 and global 101 is not 2 at a midnight outside dialogue, choose boil3.

**Pip-Boy selection.** For selection a ≥ 0, scan indexes 3..13 for played movies. At each played movie decrement a, and stop when a ≤ 0. Selections 0 and 1 therefore both pick the first played movie. If no movie qualifies, the scan ends at index 14, and the reference plays index 14, which is out of range. `[May-improve]` Safe rule: play nothing.

### 3.13 `play_gmovie` Opcode

`play_gmovie` (`0x8115`, SPEC-005) pops one integer, the movie index m.

1. A non-integer operand raises the SPEC-005 script error. `[Must-match]`
2. Background processing for dialogue is suspended (SPEC-006). The flags are taken from the `play_gmovie` column of section 2.11. For indexes 3 and 4, `0x02` is added when no map is loaded (empty map name). Then the movie plays through section 3.12, and background processing resumes. A failed play prints a diagnostic only. `[Must-match]`
3. The index is not range-checked before it is used to index the flags and file tables. `[May-improve]` Safe rule: treat an m outside 0..13 as a failed play.
4. The opcode pushes no result. `[Must-match]`

## 4. Synthetic Test Fixtures (Hex / Text)

### 4.1 Fixture A: Header Validation

```text
49 6E 74 65 72 70 6C 61 79 20 4D 56 45 20 46 69 6C 65 1A 00   signature
1A 00 00 01 33 11                                             26, 256, 4403
08 00 03 00                                                   first chunk: length 8, type 3
```

Valid. Variants:

- A2: check value `34 11` (4404) is rejected.
- A3: version `01 01` with check `32 11` is rejected, because the version must be 256 even though the sum is 0x1233.
- A4: signature byte 19 equal to `20` instead of `00` is rejected.
- A5: header size `1C 00` is rejected.

### 4.2 Fixture B: Chunk Walk and Read-Ahead

The stream after the 26-byte header (replacing Fixture A's first-chunk header):

```text
0E 00 03 00                         first chunk header: L = 14, type 3 (ignored)
06 00 02 00 8D 20 00 00 08 00       opcode N = 6, type 2 (timer), version 0: rate 8333, subdivision 8
00 00 01 00                         opcode type 1 (end of chunk)
08 00 04 00                         chunk 2 header: L = 8
00 00 00 00 00 00 01 00             opcode type 0 (end of stream), then opcode type 1
00 00 00 00                         trailing header required by the read-ahead rule
```

1. Record 1 is 14 + 4 = 18 bytes: both opcodes plus chunk 2's header. The timer gives T = 8333 × 8 + (8 >> 1) = 66668 µs.
2. Record 2 is 8 + 4 = 12 bytes: the step returns "finished" at its type-0 opcode.
3. Dropping the final 4 bytes makes record 2 unreadable, so playback ends with a read error before the end-of-stream opcode is seen.
4. Changing the first chunk's type word to any other value changes nothing.

### 4.3 Fixture C: Motion Codes

| Type | B | (dx, dy) |
| ---: | --- | --- |
| 2 | `00` | (8, 0) |
| 2 | `37` (55) | (14, 7) |
| 2 | `38` (56) | (-14, 8) |
| 2 | `FF` | (11, 14) |
| 3 | `00` | (-8, 0) |
| 3 | `FF` | (-11, -14) |
| 4 | `00` | (-8, -8) |
| 4 | `88` | (0, 0) |
| 4 | `FF` | (7, 7) |
| 5 | `FD 02` | (-3, 2) |

Linear wrap: in a 432-wide frame, a type-2 block at x = 424 with B = `00` reads source columns 432..439 of the same rows. Those addresses are columns 0..7 of the **next** row.

### 4.4 Fixture D: Pattern Blocks

1. Type 7, parameters `10 20 01 02 04 08 10 20 40 80` (P0 ≤ P1): pixel (i, i) = `20` for i = 0..7 and every other pixel = `10`. The block consumes 10 bytes.
2. Type 7, parameters `20 10 21 84` (P0 > P1): the bits are `0x8421`, so the 2 × 2 cells (0,0), (1,1), (2,2) and (3,3) are `10` and the rest `20`. The block consumes 4 bytes.
3. Type 9, parameters `00 01 03 02 E4 E4 E4 E4` (P0 ≤ P1, P2 > P3): 2 × 2 cells. Each byte `E4` gives the values 0, 1, 2, 3, so every pixel row is `00 00 01 01 03 03 02 02`. The block consumes 8 bytes.
4. Type 8, parameters `05 09 0F 00 …` (P0 ≤ P1): the top-left quadrant's rows 0-3 are `09 09 09 09`, `05 05 05 05`, `05 05 05 05`, `05 05 05 05`. Bits 0-3 of `0F` (set) give row 0, bits 4-7 (clear) give row 1, and byte `00` gives rows 2-3. The **second** group of four bytes paints the **bottom-left** quadrant.
5. Type 15, `05 09`: row 0 is `05 09 05 09 05 09 05 09` and row 1 is `09 05 09 05 …`.
6. Type 13, `01 02 03 04`: rows 0-3 are `01×4 02×4` and rows 4-7 are `03×4 04×4`.
7. Map byte `E7` puts type 7 in the left block and type 14 in the right block.

### 4.5 Fixture E: Audio

1. **Mono.** Opcode 8 payload with track mask 1, length 8, codec bytes `00 01 05 FF 82`. The predictor starts at 256. Output samples are 256, 261, 260, 5741 (256 + 5 = 261; + `FFFF` = 260; + `1569` = 5741). Output bytes: `00 01 05 01 04 01 6D 16`.
2. **Stereo.** Length 12, codec bytes `9C FF 64 00 01 FF 82 81`. The samples are (L, R) = (-100, 100), (-99, 99), (5382, 100). Output bytes: `9C FF 64 00 9D FF 63 00 06 15 64 00`.
3. **Wrap.** Predictor 32000 with code `6F` (`3F4E` = 16206) gives **-17330** (`4E BC`). A clamping decoder would give 32767.
4. **Track mask.** The same opcode with mask `0002` produces nothing. Opcode 9 with mask `FFFF` and length 16 appends 16 zero bytes. With mask `FFFE` it appends nothing.

### 4.6 Fixture F: Subtitles

File (CR LF line ends):

```text
10:First line
not a subtitle line
25:Second: with colon
5:Earlier number, later in file
:Zero
```

1. Parsing yields four entries in file order: (10, `First line`), (25, `Second: with colon`), (5, `Earlier number, later in file`), (0, `Zero`). The second line has no colon and is ignored.
2. After the step that produces frame 10, entry 1 is drawn. Frames 11-24 draw nothing.
3. After frame 25, entries 2, 3 and 4 are each drawn in turn within that one step, and `Zero` remains visible.
4. Variant: with a 640 × 480 window, a 432 × 320 frame at y = 80, and text height 10, the band top is `(480 - 320 - 80 - 10) / 2 + 400 = 435` and the band height is 14.

### 4.7 Fixture G: Fades

Source palette levels (63, 40, 1) for every entry. Effect A: `in`, color 0,0,0, start 0, 10 steps. Effect B: `out`, color 0,0,0, start 35, 35 steps.

| Frame | Effect, k | Levels |
| ---: | --- | --- |
| 1 | A, k = 2 | (12, 8, 0) |
| 5 | A, k = 6 | (37, 24, 0) |
| 9 | A, k = 10 | (63, 40, 1) |
| 10-34 | none | Device follows palette pushes |
| 35 | B, k = 1 | (62, 39, 1) |
| 36 | B, k = 2 | (60, 38, 1) |
| 52 | B, k = 18 | (31, 20, 1) |
| 69 | B, k = 35 | (0, 0, 0) |
| 70 onward | none | Stays (0, 0, 0) until the next push |

`effect_frames=0` with `total_effects=2` loads nothing, because there is no comma.

### 4.8 Fixture H: Synthetic Movies

`oracle/SPEC-012/synthetic/synthetic-A.mve` (mono) and `synthetic-B.mve` (stereo) are 64 × 64 movies (8 × 8 blocks).

- Frame 1 uses only types 11-15 and loads a 256-entry palette with the push (0, 256). Later frames cycle through every block type except 6, alternate the buffer swap, and push (1, 254).
- Each chunk carries a track-0 audio opcode, a track-1-only audio opcode (to be ignored), a `0xFFFE` silence, and an unknown opcode type 19.
- Frame 3 adds a `0xFFFF` silence. The last frame carries a suppress-display opcode.
- Audio wraps 24 times (A) and 19 times (B).

`*.expected.tsv` gives the SHA-256 of every frame's device palette and pixels, every audio opcode's PCM, and the whole PCM stream. `*.frames.hex` lists every decoded frame. A conforming decoder must reproduce all of them. That includes the last frame (frame 5 of A, frame 4 of B), marked `suppressed=1`: its buffer is decoded and counted but not displayed. Every subcase of types 7-10 occurs in both movies.

### 4.9 Corpus Oracle Vectors

`oracle/SPEC-012/` contains `mve-summary.tsv`, `mve-frames.tsv` (8978 frames), `mve-audio.tsv` (8085 audio opcodes), `moviefx.tsv` (18 effects), and `sve-subtitles.tsv` (147 entries across 3 languages). The research oracle reported:

- 13 movies: 432 × 320; 8978 show-frame opcodes; 8613 video frames; 0 suppressed frames.
- Timer rate 8341 everywhere except cathexp.mve (8928), always with subdivision 8.
- 11 movies have 22050 Hz stereo compressed 16-bit audio, 47,712,956 decoded PCM bytes in total (track 0, silence included). Two movies are silent.
- Block-type 6 count 0. Out-of-buffer motion reads 0. Parameter over-reads 0. Stream errors 0. Audio wraps 0.
- Every movie ends with opcode 0 in a type-4 chunk, followed by a zero-length trailing header.
- Every corpus subtitle entry is drawn (none waits past the last frame). 12 entries have empty text.

Implementations should match every per-frame pixel and palette hash and every per-file PCM hash. Fade palette hashes apply when the fade file is honored.

## 5. Clean-Room Verification Checklist
- [x] Zero C/C++ code snippets or struct definitions included
- [x] All multi-byte integer endianness explicitly stated
- [x] Synthetic test vectors included
- [x] Every documented quirk tagged `[Must-match]` or `[May-improve]`
- [x] No internal reference symbol names; only public SSL opcode names and file-format key literals
- [x] Spec linter run against `fallout1-re` symbols with no findings

Verification notes:
- **Linter.** `tools/spec_lint.py` gives 0 errors. The movie-fade key `fade_steps` is a public `.cfg` key literal that coincides with an unrelated reference variable, so it was added to the linter allowlist (CLAUDE.md rule 2). The 42 plain-word warnings were reviewed and are benign: they are English words in prose, notation or fixture text ("version", "code", "text", "frame", "number", "start", "count", "width", "height", "mask", "length", "file", "line"), the INI section name "info", and the `fallout.cfg` keys "language" and "subtitles" with the section name "system".
- **Code.** The fenced blocks hold only hex dumps, the audio delta table, and sample `.SVE` text.
- **Oracle.** The block decoder matched the reference decoder byte for byte on all 8613 corpus video frames and on both synthetic movies.
- **Tag count.** 102 `[Must-match]` and 23 `[May-improve]` tags.

Update notes (2026-10-23): section 3.8 states that movie audio is not scaled by master volume and gives its truncation and update rule. Section 3.10 gains step 6: subtitles wrap at 640 pixels, but only the first line is drawn, because the band is one text height plus 4. No corpus entry wraps. There are two `[May-improve]` safe rules, for growing the band and for the endless loop on a line with no space. Net tags: +2 `[May-improve]`. `tools/spec_lint.py` gives 0 errors.

Update notes (2026-10-23, second pass): section 3.12 now defines the water countdown. The remaining water is campaign global 10 (`data/vault13.gam`, 150 days) and the quest state is campaign global 101; neither is an exported variable. It gives the midnight steps, the saved water-movie byte, and a `[May-improve]` safe rule for a countdown that reaches 0 during dialogue. Net tags: +1 `[May-improve]`. `tools/spec_lint.py` gives 0 errors.
