# SPEC-010: Sound Effect Names and ACM Audio Decoding

## Clean-Room Rules for Research Agents
- Document ONLY binary file layouts (byte offsets, endianness, data types), mathematical formulas, state transitions, and observable behavior.
- DO NOT copy C/C++ code, struct definitions, function names, or internal variable names from the reference implementation.
- Provide small synthetic hex/text test vectors at the bottom of the specification.

## 1. Overview & Scope

This specification describes the compressed `.ACM` audio format used for sound effects, speech, and music, the exact decoding of that format to 16-bit PCM, the playback format assigned to decoded effect and speech audio, the construction of sound-effect file names from game state, the lookup and alias chain used to find an effect file, speech file paths, distance-based effect volume, the `.LIP` phoneme file that goes with each dialogue speech file, and the lip-sync rule that turns playback position into talking-head speech frames.

Evidence comes from two sources:

- Static inspection of reference revision R1, targeting 32-bit Windows.
- Measurements over the local unpacked corpus: all **2511** `.ACM` files (864 under `sound/sfx`, 1646 under `sound/speech`, 1 under `data`), decoded by a research-side oracle that follows the rules below. The oracle tables are in `oracle/SPEC-010/` (section 4.6). No original executable was run; corpus agreement shows these rules are self-consistent with the shipped assets, not that the original game produced identical bytes.
- A parse of all **1601** `.LIP` files under `sound/speech` with the section 2.6 layout, checked against the header of each companion `.ACM` (section 5).

Compatibility tags follow project policy. **[Must-match]** marks behavior an implementation must reproduce exactly: decoded PCM, file layouts, name strings, and formulas. **[May-improve]** marks reference behavior that comes from memory-safety defects or unbounded loops. An implementation must not reproduce that behavior as undefined behavior; it must instead do what the tag text says (an explicit error, or a bounded result). Where a [May-improve] case never occurs in the corpus, this is stated.

Out of scope: background-music streaming and fade control, mapping volume values to mixer attenuation, the decoded-effect cache, and the script-opcode wrappers. Talking-head art selection, fidgets, and the phoneme-to-frame table are in SPEC-006 section 3.11; this document gives only the lip-sync timing that drives them. The wrappers are covered by SPEC-005 by opcode number (`0x80A3`, `0x80A5`, `0x813D`-`0x8142`); this document gives the name builders those opcodes call. Animation suffix letters are defined in SPEC-007 section 2.2, prototype layouts in SPEC-003, archive lookup in SPEC-008, and message sound fields in SPEC-002.

## 2. Binary Layout / Data Format (Byte Offset Tables)

### 2.1 Numeric and Bit-Order Conventions

All multi-byte fields of the ACM header are **little-endian**. This is the opposite of the big-endian prototype, map, and archive formats in SPEC-003 and SPEC-008.

After the header, an ACM file is one continuous **LSB-first bitstream**. Bit *i* of the stream is bit `i mod 8` (weight `2^(i mod 8)`) of byte `floor(i / 8)`. A *w*-bit field read at stream position *p* has value `sum(bit[p+j] * 2^j for j in 0..w-1)`. Equivalently, the reader keeps a little-endian accumulator, appends each new byte above the bits it already holds, and takes fields from the low end. Fields are not byte-aligned, and blocks begin wherever the previous block ended. **[Must-match]**

The header itself is also read through this bit reader. Because every header field is a whole number of bytes, it is identical to the plain byte layout in section 2.2.

Decoded samples are signed 16-bit two's-complement values, emitted in **little-endian** byte order.

### 2.2 ACM Header (14 bytes)

| Offset | Bytes | Type | Meaning |
| --- | ---: | --- | --- |
| `0x00` | 3 | bytes | Signature `97 28 03` (24-bit LSB-first value `0x032897`) |
| `0x03` | 1 | u8 | Version; must be `01` |
| `0x04` | 4 | u32 LE | Total sample count *N*: number of 16-bit samples to emit, counting all channels together |
| `0x08` | 2 | u16 LE | Header channel count |
| `0x0A` | 2 | u16 LE | Header sample rate in Hz |
| `0x0C` | 2 | u16 LE | Packed word *W*: levels `L = W & 0x000F`; rows `R = W >> 4` (0-4095) |

Derived values: column count `C = 2^L`; block size `R x C` samples.

Signature or version mismatch is a load failure, and the file plays nothing. **[Must-match]**

The sample count is stored as two 16-bit halves, low half first. That is the same as a single little-endian u32, and implementations may read it as one.

Corpus: every file has signature and version as above, `R = 16`, and `L = 7` (871 files) or `L = 8` (1640 files). Every file declares 22050 Hz. All 864 effect files declare 1 channel; all 1646 speech files and `data/kill13b.acm` declare **2 channels** (see section 3.6).

### 2.3 Block Bitstream Layout

The bitstream after the header is a sequence of blocks. Each block encodes `R x C` samples, stored row-major: sample `(row r, column c)` is block element `r x C + c`.

| Field | Width (bits) | Meaning |
| --- | ---: | --- |
| Step exponent *k* | 4 | Step-table half size is `2^k` |
| Step *s* | 16 | Unsigned step value 0-65535 |
| Then, for each column `c = 0 .. C-1` in order: |  |  |
| Column format *f* | 5 | Selects the coding of this column's *R* values (section 2.4) |
| Column payload | variable | Exactly *R* values for column *c*, rows 0 to R-1 in order |

All of column 0's payload comes before column 1's format field, and so on. Blocks continue until *N* samples have been emitted (section 3.5). There is no block count, length prefix, or end marker. **[Must-match]**

### 2.4 Column Format Codes

The step table `T` is defined in section 3.2; `T[j]` is a signed table entry. In the prefix-code rows below, "bit *b*" means the *b*-th not-yet-consumed bit of the column payload, numbering from bit 0. A code consumes only the bits listed for its case.

| *f* | Coding of one column (repeat until *R* values are produced) |
| ---: | --- |
| 0 | No payload bits. All *R* values are 0. |
| 1, 2 | Reserved: decoding fails (section 3.7). |
| 3-16 | Each value is an *f*-bit field *v*; the value is `T[v - 2^(f-1)]`. |
| 17 | bit0=0: two zeros (1 bit). bit0=1, bit1=0: one zero (2 bits). bit0=1, bit1=1: one value, `T[+1]` if bit2=1 else `T[-1]` (3 bits). |
| 18 | bit0=0: one zero (1 bit). bit0=1: `T[+1]` if bit1=1 else `T[-1]` (2 bits). |
| 19 | 5-bit field *v*. Base-3 digits `d0 = v mod 3`, `d1 = floor(v/3) mod 3`, `d2 = floor(v/9)` give three values `T[d0-1]`, `T[d1-1]`, `T[d2-1]`, in that order. |
| 20 | bit0=0: two zeros (1 bit). bit0=1, bit1=0: one zero (2 bits). bit0=1, bit1=1: one value from bits 2-3 (4 bits): `(bit3,bit2)` = `00`, `01`, `10`, `11` give `T[-2]`, `T[-1]`, `T[+1]`, `T[+2]`. |
| 21 | bit0=0: one zero (1 bit). bit0=1: one value from bits 1-2 (3 bits): `(bit2,bit1)` = `00`, `01`, `10`, `11` give `T[-2]`, `T[-1]`, `T[+1]`, `T[+2]`. |
| 22 | 7-bit field *v*. Base-5 digits `d0 = v mod 5`, `d1 = floor(v/5) mod 5`, `d2 = floor(v/25)` give `T[d0-2]`, `T[d1-2]`, `T[d2-2]`. |
| 23 | bit0=0: two zeros (1 bit). bit0=1, bit1=0: one zero (2 bits). bits 0-2 = 1,1,0: `T[+1]` if bit3=1 else `T[-1]` (4 bits). bits 0-2 = 1,1,1: one value from `t = (bit4,bit3)` (5 bits): t = 0, 1, 2, 3 give `T[-3]`, `T[-2]`, `T[+2]`, `T[+3]`. |
| 24 | bit0=0: one zero (1 bit). bit0=1, bit1=0: `T[+1]` if bit2=1 else `T[-1]` (3 bits). bit0=1, bit1=1: one value from `t = (bit3,bit2)` (4 bits): t = 0, 1, 2, 3 give `T[-3]`, `T[-2]`, `T[+2]`, `T[+3]`. |
| 25 | Reserved: decoding fails. |
| 26 | bit0=0: two zeros (1 bit). bit0=1, bit1=0: one zero (2 bits). bit0=1, bit1=1: one value from `t = (bit4,bit3,bit2)` (5 bits): t = 0..7 give `T[-4]`, `T[-3]`, `T[-2]`, `T[-1]`, `T[+1]`, `T[+2]`, `T[+3]`, `T[+4]`. |
| 27 | bit0=0: one zero (1 bit). bit0=1: one value from `t = (bit3,bit2,bit1)` (4 bits), mapped as in format 26. |
| 28 | Reserved: decoding fails. |
| 29 | 7-bit field *v*. Base-11 digits `d0 = v mod 11`, `d1 = floor(v/11)` give `T[d0-5]`, `T[d1-5]`. |
| 30, 31 | Reserved: decoding fails. |

Rules shared by all formats **[Must-match]**:

- When a column needs only one more value, a two-zeros code still consumes its 1 bit but stores a single zero. A grouped code (formats 19, 22, 29) still consumes its whole field, and the digits that would fall past row *R-1* are discarded. Nothing carries over to the next column.
- The zero produced by a zero code is literal 0, not a table lookup.
- Out-of-range grouped fields decode as all-minimum digits: format 19 with `v >= 27` gives three `T[-1]`; format 22 with `v >= 125` gives three `T[-2]`; format 29 with `v >= 121` gives two `T[-5]`. The corpus never contains such fields.

Corpus usage of formats (column count over all blocks): 0, 3-14, 17-24, 26, 27 and 29 occur. Formats 15 and 16 and all reserved codes never occur.

### 2.5 Prototype Bytes Used by Sound Names

These one-byte fields are listed as "uninterpreted" or "sound code" in SPEC-003. Their sound meaning is:

| Prototype | File offset | Meaning |
| --- | --- | --- |
| Item (any subtype) | `0x38` | Item sound byte; used as the container sound letter |
| Weapon item | `0x79` (weapon payload `0x40`) | Weapon sound byte |
| Scenery (any subtype) | `0x28` | Scenery sound byte; used as the door sound letter |

The corpus confirms these offsets. All 17 containers with a nonzero item sound byte use `A`-`D`, and every name built from them exists. All 42 doors use scenery sound bytes `A`-`R`, and every resulting open/close name exists. The other 60 containers and all non-container items hold zero, as do non-door scenery prototypes. Weapon sound bytes are ASCII digits and uppercase letters (`oracle/SPEC-010/sfx-proto-bytes.tsv`).

### 2.6 `.LIP` Phoneme File

A `.LIP` file gives the mouth shapes for one dialogue speech file. It holds a header, a phoneme table, and a marker table. Every multi-byte integer is a **big-endian** 32-bit two's-complement value, as in the archive and prototype formats. This is the opposite of the little-endian ACM header in section 2.2. The tables in this section are `[Must-match]`.

**Version 2 header (44 bytes).** All corpus files use this version.

| Offset | Bytes | Type | Meaning |
| --- | ---: | --- | --- |
| `0x00` | 4 | s32 BE | Version: 2 |
| `0x04` | 4 | s32 BE | Not used. Corpus: always 22528 (`0x5800`) |
| `0x08` | 4 | s32 BE | Flags. Bit 0 (value 1) is the repeat flag (section 3.12); no other bit is used. Corpus: 0 in 1501 files, 4 in 100 files |
| `0x0C` | 4 | s32 BE | Not used. Corpus: 0 |
| `0x10` | 4 | s32 BE | Speech length in bytes. Not used at playback. Corpus: 2N in 6 files and 2N + 1 in 1595 files, where N is the companion ACM sample count (section 2.2) |
| `0x14` | 4 | s32 BE | Phoneme count P |
| `0x18` | 4 | s32 BE | Not used. Corpus: 0 |
| `0x1C` | 4 | s32 BE | Marker count M |
| `0x20` | 8 | bytes | Audio stem: the companion ACM name with no directory or extension, padded with zero bytes. All 8 bytes may be name characters, with no terminator |
| `0x28` | 4 | bytes | Not used. Corpus: `VOC` and a zero byte |

**Phoneme table**, at `0x2C`: P bytes. Each byte is a phoneme number 0-41. SPEC-006 section 3.11 maps phoneme numbers to speech frames.

**Marker table**, at `0x2C + P`: M entries of 8 bytes each.

| Entry offset | Bytes | Type | Meaning |
| --- | ---: | --- | --- |
| +0 | 4 | s32 BE | Marker type, 0 or 1. Checked when loading (section 3.12), otherwise not used |
| +4 | 4 | s32 BE | Position: a byte offset into the decoded speech PCM |

The file is `44 + P + 8M` bytes long. Nothing after the marker table is read.

**Units.** Speech plays as mono 16-bit samples at 22050 Hz (section 3.6). A position therefore counts **bytes of decoded PCM**, two per sample: sample *s* starts at byte `2s`, and one second is 44100 bytes. The time of a marker is `position / 44100` seconds after the speech starts. Positions are not sample counts, ACM block numbers, or milliseconds. `[Must-match]` Corpus evidence: the length word at `0x10` is always 2N or 2N + 1, and the last marker always lies within -3019 to +24093 bytes of 2N, that is, -68 ms to +546 ms of the audio end. 36417 marker positions are odd numbers; that is valid, because the playback position is a byte count.

**Pairing.** Phoneme *i* is shown from marker *i* to marker *i* + 1. The last marker is the end-of-speech marker and has no phoneme of its own, so a well-formed file has `P = M - 1`. `[Must-match]` Corpus: `P = M - 1` in all 1601 files.

**Version 1 header (360 bytes)** `[May-improve]`. No corpus file uses it. The reference reads it as follows, and the phoneme table then starts at `0x168`:

| Offset | Bytes | Meaning |
| --- | ---: | --- |
| `0x00` | 4 | Version: 1 |
| `0x04` | 4 | Not used |
| `0x08` | 4 | Flags, as in version 2 |
| `0x0C`, `0x14`, `0x18`, `0x30` | 4 each | Values the reference stores where it keeps its own audio handle and table addresses |
| `0x10`, `0x1C`, `0x20`, `0x28`, and the 7 words at `0x34`-`0x4F` | 4 each | Not used (`0x20` is a start offset that loading always resets to 0) |
| `0x24` | 4 | Phoneme count P |
| `0x2C` | 4 | Marker count M |
| `0x50` | 8 | Audio stem, as in version 2 |
| `0x58`-`0x167` | 272 | Not used |

The reference later releases or deletes whatever the words at `0x0C` and `0x14` hold, as if they were its own resources. Safe rule: ignore the four words at `0x0C`, `0x14`, `0x18`, and `0x30`. An implementation may instead reject version 1 files and handle them as a missing `.LIP` (section 3.12).

## 3. Behavioral Rules & Edge Cases

### 3.1 Bit Supply and End of File

The bit reader takes bytes in file order. Once the file is exhausted, it supplies **zero bytes** without limit. **[Must-match]**

The reader often requests more bits than it will consume. Fixed-width fields need all their bits, but prefix-code formats look ahead by their longest code length: format 17 looks ahead 3 bits, formats 20 and 24 look ahead 4 bits, and formats 23 and 26 look ahead 5 bits. A one-bit code at the end of the file can therefore need up to 4 bits beyond the last stored byte. Those look-ahead bits must read as zero, and running out of data during look-ahead must not be treated as an error. Corpus: 2178 files need 1 zero byte past EOF and 135 need 2; only 198 end exactly on a byte. Every stored byte is consumed in all 2511 files.

A file truncated in the middle of real block data decodes by the same zero-supply rule. Zero bits decode deterministically: step exponent 0, step 0, format 0 in every column. That produces silent coefficients, but the carry state of section 3.4 still evolves, so the output is not necessarily silent. **[May-improve]**: an implementation may instead reject a file whose decoding needs a block header to start at or after EOF. It must not read beyond its buffer. No corpus file starts a block past EOF.

### 3.2 Step Table

At the start of each block, after reading *k* and *s*, set:

- `T[j] = wrap16(j x s)` for `j = 0 .. 2^k - 1`
- `T[-j] = wrap16(-j x s)` for `j = 1 .. 2^k`

Here `wrap16(x)` keeps the low 16 bits of the two's-complement value and reinterprets them as signed (`x mod 65536`, then subtract 65536 if the result is at least 32768). Entries hold 16-bit values even though block elements are 32-bit. For example, step 40000 gives `T[+1] = -25536` and `T[-1] = +25536`. **[Must-match]** (No corpus file reaches a wrapped entry, but the rule is cheap and exact.)

The table is asymmetric: it is filled for indexes `-2^k .. 2^k - 1`, so `T[+2^k]` is never written.

A column code that indexes outside the filled range reads a table entry left over from an earlier block, or from another file (the reference shares one table across all decoders), or from uninitialized memory. **[May-improve]**: an implementation must report a decode error for that file. For example, a format-29 column in a block with `k = 0` needs `T[±5]`. Format *f* in 3-16 is in range exactly when `f - 1 <= k`. No corpus file performs an out-of-range access.

### 3.3 Decoding One Block

1. Read *k* and *s*, then rebuild `T`.
2. For each column `c = 0 .. C-1`: read *f*, then produce *R* values by section 2.4, storing value *i* at block element `i x C + c`.
3. If any column format is reserved, the block fails (section 3.7).
4. Apply the inverse transform of section 3.4 to the block (skipped when `L = 0`).
5. Emit samples as in section 3.5.

### 3.4 Inverse Transform (Lifting)

When `L = 0`, block elements are output directly, with no transform and no bias. **[Must-match]**

When `L >= 1`, the decoder keeps carry pairs that **persist from block to block for the whole file**, all starting at zero. Level 1 has `C/2` pairs, level 2 has `C/4`, and so on down to level *L*, which has one pair. **[Must-match]**

**Row chunks.** Let `Q = max(1, floor(2048 / C) - 2)`. Each block is processed in consecutive chunks of `r = min(Q, rows remaining)` rows, starting at row 0. With `R = 16`, `L = 7` (Q = 14) gives chunks 14 and 2, and `L = 8` (Q = 6) gives chunks 6, 6, and 4.

**One lifting pass** over a region viewed as `M` rows by `K` columns (row-major, contiguous), with one carry pair `(a, b)` per column: for each column *x* independently, load `(a, b)` from that column's carry, then for each row pair `(2i, 2i+1)`, `i = 0 .. M/2 - 1`, with inputs `x0 = e[2i][x]` and `x1 = e[2i+1][x]`:

- `e[2i][x]   = x0 + 2b + a`
- `e[2i+1][x] = 2 x0 - b - x1`
- then `a = x0` and `b = x1` (the **input** values, not the outputs)

After the last pair, store `(a, b)` back into the column's carry. All arithmetic is 32-bit two's-complement with wraparound.

**Per chunk** of *r* rows, which is `r x C` contiguous elements:

1. View the chunk as `2r` rows by `C/2` columns and run one lifting pass with the level-1 carries. Then store each level-1 carry as `wrap16(a)`, `wrap16(b)`; the level-1 carries hold signed 16-bit values. **[Must-match]**
2. Add 1 to the element in column 0 of each of those `2r` rows. **[Must-match]**
3. For level `j = 2 .. L`: view the same chunk as `2^j x r` rows by `C / 2^j` columns and run one lifting pass with the level-*j* carries. These carries keep full 32-bit values.

These views reinterpret the same contiguous elements with different row widths. They are not transposes.

Corpus: no level-1 carry ever needs truncation, and decoding with full 32-bit level-1 carries and a 32-bit step table gives byte-identical PCM for all 2511 files.

**Odd chunks [May-improve].** The pairwise rule above needs an even number of rows, `2r` (always even) at level 1 and `2^j x r` at deeper levels. For odd *r*, the reference's level-1 code processes rows in groups of four. If *r* is 1, it skips the level-1 pass entirely. If *r* is odd and at least 3, it leaves the last two rows untouched and reads carry state that was never loaded. An implementation must apply the pairwise rule to all `2r` rows for every *r*. No corpus file produces an odd chunk, since every file has `R = 16` and the possible chunk sizes are 14, 2, 6, and 4.

### 3.5 Output Conversion and Sample Count

Each block yields up to `R x C` samples in block-element order (row-major). Each output sample is `wrap16(e >> L)`, where `>>` is an arithmetic (sign-propagating) right shift. Values outside the 16-bit range **wrap; they are not saturated**. **[Must-match]** Corpus: 32 effect files contain wrapped samples (124 in total, all listed in `oracle/SPEC-010/acm-wrap.tsv`). An example is `sound/sfx/mafeyebl.acm` sample 3985, where the shifted value -32828 is emitted as **+32708**. Saturating instead would give -32768 there and change the PCM hash.

Let *E* be the samples emitted so far. A block emits `min(R x C, N - E)` samples. Decoding stops once `E = N`, and no further block header is read. The rest of the final block is decoded and then discarded. **[Must-match]** The decoded byte length is `2N`.

### 3.6 Playback Format

Header channel count and sample rate do not configure effect or speech playback. **[Must-match]**

- Effects and speech are played as **mono 16-bit at the mixer rate of 22050 Hz**, whatever the header says. Each decoded sample is one mono sample.
- Background music streams are played as interleaved **stereo** at the mixer rate, whatever the header says.

The 1646 speech files declare 2 channels but are mono data. 1301 of them have an **odd** total sample count, which interleaved stereo could not have. Treating their samples as stereo frames would halve the duration and garble the speech.

### 3.7 Decode Failure and Degenerate Headers

**Reserved column format (1, 2, 25, 28, 30, 31).** The reference ends the stream at that block: samples from earlier blocks remain emitted, and nothing from the failing block is emitted. **[Must-match]** for the samples that come before the failing block. **[May-improve]**: an implementation must also report a decode error rather than a silently short sound. No corpus file contains a reserved format.

**R = 0.** Each block emits zero samples and the remaining count never decreases, so the reference never terminates. **[May-improve]**: an implementation must reject such a file with `N > 0`. A file with `N = 0` emits nothing for any *R*.

**Large L.** *L* may be up to 15 (`C = 32768`), in which case `Q = 1`. With `Q = 1`, every chunk is a single row and falls under the odd-chunk rule of section 3.4. Corpus files use only `L = 7` and `L = 8`.

### 3.8 Effect Name Construction

All builders produce a name **without** a directory or extension. Each converts ASCII `a`-`z` to uppercase, leaving every other byte unchanged. **[Must-match]** A zero byte inserted by a `%c`-style sound byte ends the name at that point. Builders return "no name" on the failures listed, and no sound is played then.

**Character effects** (object, animation code, extra selector):

1. Stem: the base token of the art list for the object's art category, at the object's art index (low 12 bits of its art identifier). For critters this is `critters.lst`. The **unaliased** row is used; the SPEC-007 alias substitution does not apply. If the index is beyond the list, there is no name. **[Must-match]**
2. Two letters: the SPEC-007 section 2.2 suffix for the animation code. The weapon code is normally bits 12-15 of the object's art identifier. For animation 38 (take out) the weapon code is the **extra selector** instead. If the suffix rule fails (weapon code 11-15, or weapon 0 for animations 38-47), there is no name. **[Must-match]**
3. Overrides of the **first** suffix letter only:
   - Animations 20 and 21 (fall back, fall front) with extra 2 (pass out) use `y`; with extra 3 (die) they use `z`.
   - Animations 16 and 17 (punch, kick) with extra 4 (contact) use `z`.
   - The other extra values are 0 (none) and 1 (knockdown); they change nothing. **[Must-match]**
4. Name = stem + two letters, uppercased. For example, the punch-contact name for stem `hmjmps` is `HMJMPSZQ`, and the pass-out fall-front name is `HMJMPSYB`.

**Weapon effects** (effect type, weapon, hit mode, target). Effect types 0-4 have letters `R` ready, `A` attack, `O` out of ammo, `F` projectile in flight, `H` hit.

- Weapon letter = the weapon sound byte (section 2.5). With no weapon it is zero, which ends the name after the second character. **[Must-match]**
- Variant digit = `1` for effect types R and O. For A, F, and H it is `1` when the hit mode is 0, 2, or 4 (left primary, right primary, punch) and `2` for every other hit mode, **including kick (5)**. **[Must-match]**
- Material letter = `X` unless the effect is H, a target is given, and the weapon damage type is not plasma (3), EMP (5), or explosion (6). Otherwise it comes from the target's prototype material (items, scenery, and walls only; other target categories count as no material). Glass, metal, or plastic (0-2) give `M`; wood (3) gives `W`; dirt, stone, or cement (4-6) give `S`; leather (7) and no material give `F`. **[Must-match]**
- Name = `W`, effect letter, weapon letter, variant digit, material letter, `XX`, `1`. Examples: `WHE1MXX1`, `WAH2XXX1`, `WRA1XXX1`.

**Open/close names** (object, action). Action letters are `O` open, `C` close, `L` lock, `N` unlock, `U` use, for actions 0-4.

- A scenery object gives `S`, action letter, `DOORS`, scenery sound byte. If the prototype cannot be loaded, the sound byte defaults to `A`. **[Must-match]**
- Any other object is treated as an item and gives `I`, action letter, `CNTNR`, item sound byte. A zero sound byte gives a 7-character name such as `IOCNTNR`, which normally matches no file. **[Must-match]**

**Formatted base names** (used only by script opcodes `0x813E`-`0x8140` and `0x8142`, whose wrappers are broken per SPEC-005):

- Ambient: `A`, base right-justified with **spaces** to at least 6 characters, `1`. For example, base `wind` gives `A  WIND1`.
- Interface/item: the same with `N`.
- Scenery: `S`, then `P` for selector 1 (passive) or `A` otherwise, action letter, base right-justified with spaces to at least 4 characters, `1`.
- Padding spaces are part of the name and of the file looked up. Bases longer than the minimum width are not truncated. **[Must-match]**

**Name length [May-improve].** The reference builds every name in a 13-byte buffer, so a name longer than 12 characters overruns it. This happens with a stem longer than 10, or an ambient/interface base longer than 10. An implementation must build the complete name in adequately sized storage. No corpus critter stem is longer than 6 characters.

**Literal names.** Fixed interface buttons use, for press/release: small red `ib1p1xx1`/`ib1lu1x1`; medium `ib2p1xx1`/`ib2lu1x1`; large `ib3p1xx1`/`ib3lu1x1`; toggle `toggle` for both. These are passed lowercase. They are not uppercased by a builder, but lookup is case-insensitive (section 3.9).

### 3.9 Effect Lookup and Alias Chain

Given name *n* and an optional owning object, try paths in order. The first archive entry that opens wins (SPEC-008 selected-database lookup; case-insensitive keys). The `.ACM` extension is appended in uppercase. **[Must-match]**

1. `sound\sfx\` + *n* + `.ACM`.
2. Only if an owning object was supplied, it is a critter, and *n* begins with uppercase `H` or `N`: let *g* = the second character of *n*. If *g* is `A`, replace it with `F` when the object's gender stat is female (1) and `M` otherwise. Any other *g*, including letters other than `F` and `M`, is kept as is. Try `sound\sfx\H` + *g* + `XXXX` + (*n* from character 7 on) + `.ACM`. The new prefix is always `H`, even when *n* began with `N`.
3. Only if step 2 ran and *g* is `F`: try `sound\sfx\HMXXXX` + (*n* from character 7 on) + `.ACM`.
4. If *n* begins with exactly `MALIEU` or `MAMTN2`, a case-sensitive match: try `sound\sfx\MAMTNT` + (*n* from character 7 on) + `.ACM`. This step does not depend on an object, and runs after steps 2-3 whenever those did not succeed.
5. Otherwise there is no sound.

Owning objects are supplied by animation-registered effects. Direct effect playback, including the `0x80A3` opcode and the literal button names, supplies none and so never takes the gender aliases. Step 4 still applies to it. The case-sensitive prefix test means lowercase `malieuan` does not alias, while `MALIEUAN` resolves to `MAMTNTAN`. **[Must-match]**

"*n* from character 7 on" assumes a name of at least 6 characters. **[May-improve]**: for a shorter *n*, the suffix is empty; the reference reads past the string end.

**Active effect limit.** At most 4 effects may be loaded or playing at once. A fifth request fails immediately, with nothing evicted. **[May-improve]**: a mixer with more voices is acceptable, as long as a request is never satisfied by stopping an earlier effect.

### 3.10 Speech File Paths

Dialogue speech uses the talking head's base stem from the head art list, and the message's sound field (SPEC-002):

- Stem *t*: the sound field up to (not including) its first `.`, then **at most 8 characters** of that. **[Must-match]**
- Head directory *h*: the head art stem. Head stems normally contain no dot. **[May-improve]**: when a head stem does contain one, the reference cuts the lip-file path at the dot but does not cut the audio path. An implementation may reject such a stem.
- Lip file: `SOUND\SPEECH\` + *h* + `\` + *t* + `.LIP` (section 2.6).
- Audio: `SOUND\SPEECH\` + *h* + `\` + *s* + `.ACM`. Here *s* is the audio stem stored in the lip file when one was read, and *t* otherwise (section 3.12). Corpus: the stored stem matches *t*, ignoring case, in all 1601 lip files.
- Non-dialogue narration (death and ending narration, the options narrator) passes a relative name such as `narrator\options` and plays `sound\speech\` + name + `.ACM`.
- **[May-improve]**: the reference copies the stem into fixed 8-byte and 16-byte storage. An 8-character stem overruns the first by its terminator, which is harmless there. A stem of 16 or more characters corrupts the lip extension and overruns a stack buffer. An implementation must apply the 8-character rule without memory corruption. Stems of exactly 8 characters (212 corpus files) must work.

Corpus: the 22 speech directories match the head-list stems exactly, plus `narrator`, which has no head entry. Speech stems are 5-8 characters long.

### 3.11 Relative Effect Volume

Effects tied to an object get a relative volume `V` in 0..32767. This is computed for item, critter, and scenery objects; any other object, or no object, gets 32767.

1. Use the object's outermost container or owner if it has one, otherwise the object itself.
2. If that object's screen bounding rectangle overlaps the game display window (inclusive pixel bounds), `V = 32767`.
3. Otherwise let *d* = hex distance from that object to the player, and *P* = the player's Perception:
   - `d <= P` gives `V = 32767`
   - `P < d < 2P` gives `V = 32767 - floor(21844 x (d - P) / P)`
   - otherwise `V = 10922`

   With `P = 0` and `d >= 1`, the result is 10922; there is no division by zero. **[Must-match]**

The applied effect volume is `floor(V x S / 32767)`, where *S* is the effect volume setting (0..32767). **[Must-match]** The mapping to audible attenuation is out of scope.

### 3.12 Lip-Sync Playback

Dialogue replies that carry a sound field play through lip-sync. SPEC-006 section 3.9 says when a reply requests speech. Narration that plays through relative names (section 3.10) has no lip file and no head, and does not use this section.

**Loading.** A speech request has the speech stem *t* and the head directory *h* (section 3.10). It runs these steps in order. `[Must-match]`

1. Stop and release any speech that is still loaded. No head frame is drawn for this.
2. Open the lip file. If it opens, read the version word.
   - Version 2: read the rest of the 44-byte header.
   - Version 1: read the 360-byte header (section 2.6).
   - Any other version is logged and is otherwise handled as a missing `.LIP` (see **Missing or malformed files**).
3. Read P phoneme bytes, then M markers.
4. Check the tables. The checks only log: a phoneme of 42 or more, a marker type other than 0 or 1, a nonzero position at marker 0, and a position lower than the one before it. **Nothing is rejected.** Corpus: no file fails any check, marker positions are strictly increasing in every file, and marker 0 always has type 1 and position 0.
5. If P or M is 0, abandon the speech. Abandoning means that no audio plays and no new speech state begins. Any earlier speech was already stopped by step 1; if its speech state still exists, the next tick ends it as a normal ending.
6. Load the audio from the section 3.10 path, using the stored audio stem. The ACM is decoded **completely** before playback begins. It plays as one non-repeating mono 16-bit 22050 Hz buffer (section 3.6) of 2N bytes. If the audio cannot be loaded, the speech still goes ahead without it (see **Missing or malformed files**).
7. Set the marker index to 0 and the current phoneme to phoneme 0.

**Start.** Playback begins as soon as loading finishes, inside the reply display. The audio starts before the reply text is drawn, and before the next head updater tick (SPEC-006 section 3.11). `[Must-match]`

1. Lip-sync becomes **running**.
2. The audio position is set to byte 0.
3. Starting from marker 0, skip every marker whose position is below 0. After skipping *k* markers, the marker index is *k* and the current phoneme is phoneme *k*. With non-negative positions, as in every corpus file, nothing is skipped. `[May-improve]`: when every position is negative, the reference scan runs past the end of the table. Safe rule: treat a negative position as 0.
4. Set the speech volume to `trunc(0.69 x V)`, where *V* is the speech volume setting (`speech_volume`, 0-32767).
5. Start the audio. If it cannot start, for example because no audio was loaded, lip-sync stops running. The speech state remains, and the next tick ends it.

The reference computes the volume factor as a floating-point product. For some multiples of 100 (300, 600, 700, ...) the result can be one lower than `floor(69 x V / 100)`. That difference cannot be heard. `[May-improve]` Safe rule: use `floor(69 x V / 100)`.

**Per-tick rule.** While a speech state exists, each head updater tick runs the steps below instead of the idle fidget logic. Let *p* be the audio playback position in bytes, read **once per tick**. Let *i* be the marker index, *pos(j)* the position of marker *j*, and *ph(j)* phoneme *j*. `[Must-match]`

1. If lip-sync is running, read *p*. Then, while `p > pos(i)` (**strictly greater**):
   - set the current phoneme to *ph(i)* and add 1 to *i*;
   - if *i* has reached M, the final marker has been passed. Set *i* to 0 and the current phoneme to *ph(0)*. Unless the repeat flag is set, stop the audio at once and stop lip-sync. Leave the loop.
2. If lip-sync is still running and `i >= M - 1`, let *k* = 3 when M > 5 and *k* = 0 otherwise. If `p < pos(k)`, handle it as if the final marker had been passed in step 1. This test catches a playback position that has jumped back to the start. A position that never decreases never triggers it. With *k* = 0 it can never trigger at all. Corpus: one file (`harry\harry_0a.lip`) has M = 5.
3. If the current phoneme differs from the **last drawn phoneme**, set that to the current phoneme and mark a redraw.
4. The mixer checks whether the audio has finished.
5. If a redraw is marked, draw frame *F*(current phoneme) of the current mood's speech art (selectors 9-11, SPEC-006 section 3.11), where *F* is the SPEC-006 phoneme table. Then clear the mark.
6. If the audio is no longer playing, end the speech normally: release it, draw speech frame 0, enter the waiting-for-next-fidget state, set the idle-second counter to 3, and reset the fidget frame counter to 0 (SPEC-006 section 3.11).

So for `pos(j) < p <= pos(j+1)` the head shows phoneme *j*. At `p = pos(j+1)` exactly, phoneme *j* still shows. A tick that passes several markers at once draws only the **last** phoneme it reaches; the phonemes in between are never drawn. Frames change only on ticks, so synchronous work that runs no ticks (a reaction transition, the window scroll-in) delays the mouth while the audio keeps playing. When the next tick comes, the head jumps to the right phoneme. `[Must-match]`

The repeat flag (bit 0 of the flags word) keeps the audio running past the final marker. Each later tick passes all the markers again, so the head holds *ph(0)* until the audio ends. No corpus file sets the flag. `[Must-match]`

**Last drawn phoneme.** The redraw test compares **phoneme numbers**, not frames, and the last drawn phoneme carries over from speech to speech and from session to session. At program start it is 0, with a redraw already marked. In practice, the first speech of a program run draws its first frame on its first tick. A later speech draws nothing until its current phoneme differs from the last phoneme drawn by an earlier speech. Until then the head keeps its previous image: the paused fidget frame, or, for the first reply of a session, the dialogue backdrop with no scene or head drawn yet. In the corpus, 1600 files begin and end with phoneme 0 (`lieut\lieut2.lip` begins with 31 and ends with 19). The first phoneme change comes 150 to 15834 bytes into the speech (median 259 bytes, 6 ms; at most 359 ms). This is a display glitch. `[May-improve]` Safe rule: draw the speech frame for the current phoneme on the first tick of every speech, and keep every later rule unchanged.

**Final marker and audio end.** Speech ends in whichever of these comes first:

- The audio finishes. In 1393 corpus files the final marker lies after the audio end, and in 6 it equals 2N, so these always end this way.
- The playback position passes the final marker. The audio is stopped on that tick, so its tail is cut off. This happens in the 202 corpus files whose final marker lies before 2N, and it cuts at most 3019 bytes (68 ms). `[Must-match]`

Either way, the tick ends with speech frame 0 drawn, and the head goes through the SPEC-006 waiting state. `[Must-match]`

**Stopping without an ending.** These events stop and release speech without drawing a frame. They do not enter the waiting state and do not change the idle-second counter or the fidget frame counter: choosing an option, opening the Review screen, switching to Barter, closing Ask About with Escape, ending the session, and a new speech request. Choosing an option also does its own SPEC-006 reaction handling. Until something else draws, the head keeps the last speech frame drawn. `[Must-match]`

Pressing Ask About (button or `a`) is meant to stop speech the same way. Instead the reference restarts playback on the speech that is already playing, so speech and lip-sync go on while the Ask About panel is open. `[May-improve]` Safe rule: stop speech on Ask About as described for Review and Barter.

**Fidget interaction.** While a speech state exists, the idle fidget is frozen. No fidget frame is drawn, no wait is timed, and no random number is drawn. The fidget frame counter and the retained fidget timestamp keep their values. Lip-sync itself makes **no random draws**. `[Must-match]`

- After a normal ending, the waiting state begins. If no fidget frame has been shown in this session, the retained timestamp is still 0, so the next tick makes the threshold draw and then the next fidget draw (SPEC-006 section 3.11).
- After a stop without an ending, the fidget resumes from its saved frame counter on the next tick.

A reaction transition that a script requests while speech plays runs to completion, with no lip updates, while the audio continues. Afterwards lip-sync uses the speech art of the destination mood. `[Must-match]`

The head updater does nothing while the session's fidget art is not loaded. In that case lip-sync never runs and speech never ends through the per-tick rule. Every corpus head has fidget art. `[May-improve]` Safe rule: run the per-tick rule whenever a speech state exists.

A phoneme of 42 or more indexes past the SPEC-006 table. A phoneme index of P or more, which arises when `P < M - 1`, reads past the phoneme table. In a well-formed file, the only read at index P comes when the final marker is passed, and it is replaced by *ph(0)* on the same step. `[May-improve]` Safe rule: treat both cases as phoneme 0. The corpus has no phoneme above 41, and all 63 speech FRMs (`gp`, `np`, `bp` for 21 heads) have 9 frames, which covers the largest table value, 8.

**Missing or malformed files.**

- **Missing `.LIP`, or a version other than 1 or 2.** The reference keeps the counts, flags, and version of the most recently loaded lip file; at program start these are all 0. With a zero count it abandons the speech (loading step 5). With counts left over from an earlier file, it builds tables of those sizes. For a missing file, no bytes are read into them. For a wrong version, they are filled from the bytes after the version word. In both cases the audio named by *t* then plays, with lip-sync driven by those table contents. `[May-improve]` Safe rule: load the audio from *t* as usual and handle the speech as one phoneme 0 whose final marker lies after the audio end. The head then shows speech frame 0 for the whole speech, and the speech ends when the audio finishes.
- **A file shorter than its header and counts require.** The speech is abandoned (loading step 5). `[Must-match]` The reference also leaves the file open. `[May-improve]` Safe rule: close it.
- **Missing `.ACM` with a `.LIP` present**, or the sound system not initialized (`fallout.cfg` key `initialize` = 0 in the sound section). Loading succeeds with no audio, start step 5 stops lip-sync, and the next tick ends the speech normally. The head shows speech frame 0, the waiting state begins, and the idle-second counter becomes 3. `[Must-match]`

Corpus: 2043 dialogue messages have a sound field. 2039 of them name a stem that has both a `.LIP` and an `.ACM`. Four name a stem with neither: `cabbot.msg` 167, `harry.msg` 174, `laura.msg` 110, and `vree.msg` 194 (`Vree300)`). Four speech ACMs have no `.LIP`, and no message names them: `gizmo\gizmo34a`, `gizmo\gizmo36b`, `gizmo\gizmoalt`, and `harry\hary207b`. The other 41 lip-less ACMs are narration.

**Settings.** Speech volume 0 plays the speech silently, and lip-sync runs normally. The sound-section `fallout.cfg` key `speech`, which switches off narration speech, is **not consulted** for talking-head speech. The subtitles preference does not apply to dialogue either: reply text is always shown. `[Must-match]`

## 4. Synthetic Test Fixtures (Hex / Text)

### 4.1 ACM Vectors A-D: Valid Streams

Each vector is a complete file. Header fields are given as N / channel count / sample rate / L / R. Expected output is the decoded sample sequence (decimal, signed 16-bit). Binary copies are in `oracle/SPEC-010/synthetic/`.

**A** (`4 / 1 / 22050 / 0 / 4`; one column; no transform). Block: `k=2`, `s=100`; format 3; fields 7, 0, 4, 5.

```text
0000: 97 28 03 01 04 00 00 00 01 00 22 56 40 00 42 06
0010: 30 0E 16
```

Expected `300, -400, 0, 100`. Field *v* maps to `T[v-4]`, and since `L = 0` no +1 bias is applied.

**B** (`7 / 1 / 22050 / 1 / 2`; two columns; two blocks; final block truncated).

- Block 1: `k=2`, `s=100`. Column 0 has format 3 with fields 7, 2 (values 300, -200). Column 1 has format 0.
- Block 2: `k=0`, `s=0`, both columns format 0.

```text
0000: 97 28 03 01 07 00 00 00 01 00 22 56 21 00 42 06
0010: 30 2E 00 00 00 00 00
```

Expected `150, 300, 50, -200, -100, 0, 0`.

Block 1 elements are `[300, 0, -200, 0]`, viewed as 4 rows by 1 column. Lifting with zero carry gives `300, 600, 100, -400`. The +1 bias gives `301, 601, 101, -399`, and the shift right by 1 gives `150, 300, 50, -200`; note that -399 shifts to -200, not -199. The level-1 carry becomes `(-200, 0)`.

Block 2 is all zeros, but its first output is `0 + 2x0 + (-200) + 1 = -199`, which shifts to -100. This shows that **carry persists across blocks**. Only 3 of block 2's 4 samples are emitted, because N = 7.

**C** (`5 / 1 / 22050 / 0 / 5`). `k=3`, `s=10`; format 29; fields 0, 120, 27.

```text
0000: 97 28 03 01 05 00 00 00 01 00 22 56 50 00 A3 00
0010: D0 01 F8 0D
```

Expected `-50, -50, 50, 50, 0`. Field 27 has digits (5, 2). Only the first digit fits in the column; the second is discarded, while its 7 bits are still consumed.

**D** (`5 / 1 / 22050 / 0 / 5`). `k=1`, `s=7`; format 17; codes, in stream bit order, `1 1 1`, `0`, `1 1 0`, `1 0`.

```text
0000: 97 28 03 01 05 00 00 00 01 00 22 56 50 00 71 00
0010: 10 6F 01
```

Expected `7, 0, 0, -7, 0`.

### 4.2 ACM Vectors E-H: Edge Cases

**E** (reserved format). `N = 2`, `R = 2`, `L = 0`; `k=0`, `s=0`; column format 1.

```text
0000: 97 28 03 01 02 00 00 00 01 00 22 56 20 00 00 00
0010: 10 00
```

The reference emits **no** samples, a silently empty sound. An implementation emits none and reports a decode error. [Must-match] / [May-improve] per section 3.7.

**F** (out-of-range table index). `k=0`, `s=9`; format 29; field 0, which needs `T[-5]` twice, but only `T[0]` and `T[-1]` exist.

```text
0000: 97 28 03 01 02 00 00 00 01 00 22 56 20 00 90 00
0010: D0 01
```

The implementation must report a decode error [May-improve]. The reference output depends on stale memory and is **not** an expected value.

**G** (16-bit step table). `N = 4`, `L = 1`, `R = 2`. `k=1`, `s=40000`. Column 0 has format 3 with fields 5, 3, which select `T[+1] = -25536` and `T[-1] = +25536`. Column 1 has format 0.

```text
0000: 97 28 03 01 04 00 00 00 01 00 22 56 21 00 01 C4
0010: 39 3A 00
```

Expected `-12768, -25536, 0, 25536` [Must-match]. Lifting gives `-25536, -51072, 0, 51072`; adding the bias and shifting right by 1 gives the expected values. A 32-bit step table would give `20000, -25536, 0, 25536` instead. The first sample therefore separates the two table widths, which `L = 0` vectors cannot do.

**H** (look-ahead past EOF). `N = 4`, `R = 4`, `L = 0`; `k=1`, `s=7`; format 17; codes `1 1 1`, `1 1 0`, `0`. The last code is bit 31, the final stored bit.

```text
0000: 97 28 03 01 04 00 00 00 01 00 22 56 40 00 71 00
0010: 10 3F
```

Expected `7, -7, 0, 0`. Format 17's 3-bit look-ahead needs one zero byte beyond the file, and this must not be an error [Must-match].

### 4.3 Name Construction Vectors

| Input | Expected name |
| --- | --- |
| Character: stem `hmjmps`, animation 16 (punch, suffix `aq`), extra 4 | `HMJMPSZQ` |
| Character: stem `hmjmps`, animation 21 (fall front, `bb`), extra 2 | `HMJMPSYB` |
| Character: stem `hmjmps`, animation 20 (fall back, `ba`), extra 1 | `HMJMPSBA` |
| Character: stem `hmjmps`, animation 38, art weapon bits 0, extra 5 | `HMJMPSHC` (extra supplies weapon 5, letter `h`) |
| Character: animation 39, art weapon bits 0 | No name (weapon 0 rejected) |
| Weapon: effect 4 (H), sound byte `E`, hit mode 2, target item of material 1, damage type 0 | `WHE1MXX1` |
| Weapon: effect 4, sound byte `E`, hit mode 5 (kick), same target | `WHE2MXX1` |
| Weapon: effect 4, sound byte `H`, hit mode 0, critter target | `WHH1FXX1` |
| Weapon: effect 4, sound byte `H`, hit mode 0, target material 1, damage type 6 | `WHH1XXX1` |
| Weapon: effect 1 (A), sound byte `H`, hit mode 3 | `WAH2XXX1` |
| Weapon: effect 0 (R), sound byte `A`, hit mode 3 | `WRA1XXX1` (variant always 1 for R) |
| Weapon: effect 1, no weapon | `WA` (zero sound byte ends the name) |
| Open: scenery door, action 1, scenery sound byte `C` | `SCDOORSC` |
| Open: container, action 0, item sound byte `B` | `IOCNTNRB` |
| Open: container, action 0, item sound byte 0 | `IOCNTNR` |
| Ambient: base `wind` | `A  WIND1` (two spaces) |
| Scenery: selector 1, action 4, base `ab` | `SPU  AB1` |

### 4.4 Lookup Vectors (against an effect directory holding exactly the listed files)

Directory contents: `HMXXXXAN.ACM`, `HFXXXXAA.ACM`, `HMXXXXAA.ACM`, `MAMTNTAN.ACM`, `IB1P1XX1.ACM`.

| Request | Owner | Attempt order | Result |
| --- | --- | --- | --- |
| `HAPOWRAA` | Critter, gender 0 | `HAPOWRAA`, `HMXXXXAA` | `HMXXXXAA` |
| `HAPOWRAA` | Critter, gender 1 | `HAPOWRAA`, `HFXXXXAA` | `HFXXXXAA` |
| `HFCMBTAN` | Critter | `HFCMBTAN`, `HFXXXXAN`, `HMXXXXAN` | `HMXXXXAN` (male fallback) |
| `NACHLDAA` | Critter, gender 1 | `NACHLDAA`, `HFXXXXAA` | `HFXXXXAA` (prefix becomes `H`) |
| `HAPOWRAA` | None | `HAPOWRAA` | No sound |
| `MALIEUAN` | None | `MALIEUAN`, `MAMTNTAN` | `MAMTNTAN` |
| `malieuan` | None | `malieuan` | No sound (case-sensitive prefix) |
| `ib1p1xx1` | None | `ib1p1xx1` | `IB1P1XX1` (case-insensitive file match) |

### 4.5 Relative Volume Vectors (object off-screen)

| *d* | *P* | *V* | Applied at *S* = 16383 |
| ---: | ---: | ---: | ---: |
| 5 | 5 | 32767 | 16383 |
| 6 | 5 | 28399 | 14199 |
| 9 | 5 | 15292 | 7645 |
| 10 | 5 | 10922 | 5460 |
| 7 | 4 | 16384 | 8191 |
| 3 | 0 | 10922 | 5460 |
| 0 | 0 | 32767 | 16383 |

An on-screen object gets *V* = 32767 whatever *d* and *P* are.

### 4.6 Corpus Oracle Tables

These tables are generated from the local corpus. They contain hashes, header values, and a few sample values only, no asset content. Rows are keyed by corpus-relative path and source SHA-256, so they apply only to byte-identical assets.

| File | Contents |
| --- | --- |
| `oracle/SPEC-010/acm-corpus.tsv` | One row per ACM (2511 rows): source SHA-256, size, *N*, header channels/rate, *L*, *R*, block count, zero bytes read past EOF, **SHA-256 of decoded PCM (s16le, 2N bytes)**, first 8 and last 4 samples |
| `oracle/SPEC-010/acm-wrap.tsv` | All 124 output samples that wrap (section 3.5): path, index, shifted value, emitted value |
| `oracle/SPEC-010/acm-summary.txt` | Corpus-wide counters backing the tags in section 3 |
| `oracle/SPEC-010/sfx-resolution.tsv` | 760 name-lookup cases over real critter stems, weapon sound bytes, button literals, and alias prefixes, with attempt order and resolved stem |
| `oracle/SPEC-010/sfx-proto-bytes.tsv` | Distribution of item, scenery, and weapon sound bytes by prototype subtype, and which open/close names exist |

Acceptance for the decoder: for every row of `acm-corpus.tsv`, decoding the asset with that source hash must give the listed PCM hash. A saturating, stereo-interpreting, carry-resetting, or EOF-erroring decoder fails this check.

### 4.7 Lip-Sync Vectors

**LIP-A** is a complete synthetic version 2 file of 97 bytes. Its stem is `SYNTH01`, with P = 5 and M = 6. The phonemes are 0, 9, 19, 23, 0, which map to speech frames 0, 8, 2, 4, 0. The marker positions are 0, 100, 300, 301, 600, 1000, and marker 2 has type 0. The length word is 1001. The companion audio is taken to have N = 500 samples, so 2N = 1000.

```text
0000: 00 00 00 02 00 00 58 00 00 00 00 00 00 00 00 00
0010: 00 00 03 E9 00 00 00 05 00 00 00 00 00 00 00 06
0020: 53 59 4E 54 48 30 31 00 56 4F 43 00 00 09 13 17
0030: 00 00 00 00 01 00 00 00 00 00 00 00 01 00 00 00
0040: 64 00 00 00 00 00 00 01 2C 00 00 00 01 00 00 01
0050: 2D 00 00 00 01 00 00 02 58 00 00 00 01 00 00 03
0060: E8
```

In the table below, each row is one head updater tick and *p* is the position read on that tick. The last drawn phoneme before the speech is 0, with no redraw marked. Expected results:

| *p* | Marker index after the tick | Current phoneme | Frame drawn |
| ---: | ---: | ---: | --- |
| 0 | 0 | 0 | none; the head keeps its previous image |
| 50 | 1 | 0 | none |
| 100 | 1 | 0 | none (`100 > 100` is false) |
| 101 | 2 | 9 | 8 |
| 300 | 2 | 9 | none |
| 301 | 3 | 19 | 2 |
| 302 | 4 | 23 | 4 |
| 601 | 5 | 0 | 0 |
| 999 | 5 | 0 | none (`999 < pos(3) = 301` is false) |
| audio finished | 5 | 0 | speech frame 0; the speech ends normally and the idle-second counter becomes 3 |

The same file with last drawn phoneme 23 left by an earlier speech draws frame 0 on the first tick, at *p* = 0. Under the section 3.12 safe rule, both cases draw frame 0 on the first tick.

**LIP-B** is LIP-A with the final marker position changed to 900. Bytes `0x5D`-`0x60` become `00 00 03 84`. Ticks at 601 and 900 behave as in LIP-A. At *p* = 901 the final marker is passed: the current phoneme becomes *ph(0)* = 0, the audio is stopped at byte 901 (99 bytes, about 2 ms, before its end), and the tick ends the speech with speech frame 0.

**LIP-C** uses LIP-A, but the first tick reads *p* = 350. Markers 0-3 are passed on that one tick, so the marker index becomes 4 and the current phoneme 23. Only frame 4 is drawn; phonemes 9 and 19 never appear.

**LIP-D** checks the units. A marker at position 44100 is reached one second after the speech starts, which is sample 22050. It is not reached at 44.1 seconds or at 2 seconds.

## 5. Clean-Room Verification Checklist
- [x] Zero C/C++ code snippets or struct definitions included
- [x] All multi-byte integer endianness explicitly stated
- [x] Synthetic test vectors included
- [x] Every documented quirk tagged `[Must-match]` or `[May-improve]`
- [x] No internal reference symbol names; only public SSL opcode names and file-format key literals
- [x] Spec linter run against `fallout1-re` symbols with no findings

Verification notes: `tools/spec_lint.py` gives 0 errors. Its 5 plain-word warnings were reviewed and are benign: the math notation "column" and "remaining", the directory name "data", and the resource literal "toggle". Sound-name script opcodes are cited by number only, because CLAUDE.md bans their names. Vectors A-H were decoded by the research oracle and match the expected values above. The section 4.4 lookups were checked against the oracle resolver. The section 4.5 volumes were computed from the section 3.11 formula.

Lip-sync verification: a research-side scan parsed all 1601 corpus `.LIP` files with the section 2.6 layout and compared each with its companion ACM header. The scan found no anomalies:

- Every file is version 2, has `0x5800` at `0x04`, zeros at `0x0C` and `0x18`, `VOC` at `0x28`, and an exact size of `44 + P + 8M`.
- Flags are 0 or 4.
- `P = M - 1` in every file, and M ranges over 5-508.
- Every phoneme is below 42.
- Marker types are 0 (9440 markers) or 1 (72509). Marker 0 always has type 1 and position 0, and positions strictly increase.
- The stored stem equals the file stem, ignoring case, in every file (39 differ in case only). Every lip file has an ACM.
- The length word is 2N or 2N + 1.
- The final marker lies before 2N in 202 files (by at most 3019 bytes), equals it in 6, and lies after it in 1393.

The phoneme-to-frame table was checked against the 63 corpus speech FRMs: every one has 9 frames with zero per-frame offsets. Vectors LIP-A to LIP-C were stepped through the section 3.12 rule by a research-side model. The lip-sync text adds two plain-word linter warnings, both public `fallout.cfg` key names in the sound section: "initialize" and "speech".
