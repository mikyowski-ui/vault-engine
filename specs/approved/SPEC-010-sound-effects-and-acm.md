# SPEC-010: Sound Effect Names and ACM Audio Decoding

## Clean-Room Rules for Research Agents
- Document ONLY binary file layouts (byte offsets, endianness, data types), mathematical formulas, state transitions, and observable behavior.
- DO NOT copy C/C++ code, struct definitions, function names, or internal variable names from the reference implementation.
- Provide small synthetic hex/text test vectors at the bottom of the specification.

## 1. Overview & Scope

This specification describes the compressed `.ACM` audio format used for sound effects, speech, and music, the exact decoding of that format to 16-bit PCM, the playback format assigned to decoded effect and speech audio, the construction of sound-effect file names from game state, the lookup and alias chain used to find an effect file, speech file paths, and distance-based effect volume.

Evidence comes from two sources:

- Static inspection of reference revision `R1`, targeting 32-bit Windows.
- Measurements over the local unpacked corpus: all **2511** `.ACM` files (864 under `sound/sfx`, 1646 under `sound/speech`, 1 under `data`), decoded by a research-side oracle that follows the rules below. The oracle tables are in `oracle/SPEC-010/` (section 4.6). No original executable was run; corpus agreement shows these rules are self-consistent with the shipped assets, not that the original game produced identical bytes.

Compatibility tags follow project policy. **[Must-match]** marks behavior an implementation must reproduce exactly: decoded PCM, file layouts, name strings, and formulas. **[May-improve]** marks reference behavior that comes from memory-safety defects or unbounded loops. An implementation must not reproduce that behavior as undefined behavior; it must instead do what the tag text says (an explicit error, or a bounded result). Where a [May-improve] case never occurs in the corpus, this is stated.

Out of scope: the `.LIP` lip-sync format, background-music streaming and fade control, mapping volume values to mixer attenuation, the decoded-effect cache, and the script-opcode wrappers. The wrappers are covered by SPEC-005 by opcode number (`0x80A3`, `0x80A5`, `0x813D`-`0x8142`); this document gives the name builders those opcodes call. Animation suffix letters are defined in SPEC-007 section 2.2, prototype layouts in SPEC-003, archive lookup in SPEC-008, and message sound fields in SPEC-002.

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
- Audio: `SOUND\SPEECH\` + *h* + `\` + *t* + `.ACM`. The companion lip file uses the same stem with extension `.LIP`; its format is out of scope.
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

## 5. Clean-Room Verification Checklist
- [x] Zero C/C++ code snippets or struct definitions included
- [x] All multi-byte integer endianness explicitly stated
- [x] Synthetic test vectors included
- [x] Every reference-derived quirk tagged `[Must-match]` or `[May-improve]`
- [x] Spec linter run against reference symbols with no findings

Verification notes: `tools/spec_lint.py` gives 0 errors. Its 5 plain-word warnings were reviewed and are benign: the math notation "column" and "remaining", the directory name "data", and the resource literal "toggle". Sound-name script opcodes are cited by number only, because CLAUDE.md bans their names. Vectors A-H were decoded by the research oracle and match the expected values above. The section 4.4 lookups were checked against the oracle resolver. The section 4.5 volumes were computed from the section 3.11 formula.
