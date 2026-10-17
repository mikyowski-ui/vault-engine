# SPEC-001: Palettes, FRM Sprites, and LST Index Files

## Clean-Room Rules for Research Agents
- Document ONLY binary file layouts (byte offsets, endianness, data types), mathematical formulas, state transitions, and observable behavior.
- DO NOT copy C/C++ code, struct definitions, function names, or internal variable names from `fallout1-re`.
- Provide small synthetic hex/text test vectors at the bottom of the specification.

## 1. Overview & Scope

This specification describes the Windows/x86 behavior observable from `fallout1-re` revision `ac9784b0735468d57ddb2025b9ca76a43a404f4c`. It covers `color.pal`, indexed `.FRM` sprites and their `.FR0` through `.FR5` variants, and positional `.LST` name indexes. The evidence is source inspection, not execution against original game assets.

Paths below are logical resource paths, whether supplied by loose files or an archive. `color.pal` is more than a 768-byte RGB palette: it also supplies a color-quantization table and may supply additional color-processing tables. This is not a specification of every file bearing a `.PAL` extension.

The inspected revision reads art indexes from `art/<category>/<category>.lst`, script indexes from `scripts/scripts.lst`, and prototype indexes from `proto/<category>/<category>.lst`. No literal `data/*.lst` consumer is present. Consequently, a file merely placed under `data/` has no established parser or ID base in this revision; the consumer, not the extension or directory name, determines those rules. The data-tree indexes actually supported by the inspected readers are distinguished below rather than assigning an invented universal `data/` rule.

Evidence locations, relative to `fallout1-re/`:

| Subject | Source locations |
| --- | --- |
| Palette bytes, lookup tables, optional extension | [redacted-ref] |
| Display scaling and gamma | [redacted-ref]; [redacted-ref] |
| Palette-cycle ranges, colors, timing | [redacted-ref] |
| Sprite header and frame serialization | [redacted-ref]; [redacted-ref] |
| Sprite selection, list parsing, timing defaults, frame addressing | [redacted-ref]; [redacted-ref] |
| World placement and frame displacement | [redacted-ref]; [redacted-ref] |
| Transparent pixel copying and hit testing | [redacted-ref]; [redacted-ref] |
| Action timing and queued delays | [redacted-ref]; [redacted-ref] |
| Cursor hotspot and preview centering | [redacted-ref]; [redacted-ref] |
| Art-list secondary metadata | [redacted-ref] |
| Script indexes and map-reference conversion | [redacted-ref]; [redacted-ref]; [redacted-ref] |
| Prototype indexes and text/integer input | [redacted-ref]; [redacted-ref] |

## 2. Binary Layout / Data Format (Byte Offset Tables)

### 2.1 `color.pal`

All offsets are from the file's first byte. An RGB entry is three unsigned 8-bit storage bytes in red, green, blue order; valid channel values are 0 through 63. There is no palette header, entry count, alpha channel, or per-entry padding.

| File offset | Byte length | Representation | Meaning |
| --- | ---: | --- | --- |
| `0x0000` | 768 (`0x0300`) | 256 consecutive RGB triples | Palette entries 0 through 255; entry i begins at byte `3 * i` |
| `0x0300` | 32768 (`0x8000`) | Unsigned 8-bit palette indexes | RGB555-to-palette lookup table |
| `0x8300` | 4 | 32-bit word read in native x86 little-endian order | Optional-table discriminator; extension recognized only for numeric value `0x4E455743` |
| `0x8304` | 65536 (`0x10000`) | Unsigned 8-bit palette indexes | Present for recognized discriminator: 256 rows of 256 intensity results |
| `0x18304` | 65536 | Unsigned 8-bit palette indexes | Present for recognized discriminator: 256 by 256 additive-mix results |
| `0x28304` | 65536 | Unsigned 8-bit palette indexes | Present for recognized discriminator: 256 by 256 multiplicative-mix results |

The RGB555 lookup address is `0x0300 + 1024 * red5 + 32 * green5 + blue5`, with each five-bit channel in 0 through 31. Red occupies the most significant five bits of the 15-bit lookup index; blue occupies the least significant five bits. Conversion of a valid palette channel to five bits discards its least significant bit, equivalent to `floor(channel / 2)`. Lookup outputs are palette indexes, not RGB triples.

Each extended table is row-major, with byte address `table start + 256 * row + column`. In the intensity table the row selects the input palette index and the column the intensity level; in either mix table the row and column select the two input palette indexes.

The recognized discriminator's file bytes are `43 57 45 4E` (ASCII `CWEN`). Its numeric value spells `NEWC` when written most-significant byte first, but literal file bytes `4E 45 57 43` do not select the extension on x86. This word does not use the big-endian rule used by FRM files. When the discriminator does not match, intensity and mixing tables are generated instead of read. The complete extended layout ends at exclusive offset `0x38304` (230148 bytes); the base RGB-plus-lookup area ends at exclusive offset `0x8300` (33536 bytes).

### 2.2 `.FRM` and `.FR0` through `.FR5`: shared 62-byte header

Every multi-byte sprite integer is big-endian, including frame records. Signed integers use two's-complement interpretation. The reference interprets all 16-bit header quantities as signed, even those which describe positive counts or dimensions. For interoperable ordinary sprites, dimensions and frame counts are positive and do not exceed 32767; offsets and displacements may be negative only where their meanings allow it.

| Header offset | Byte length | Representation | Meaning |
| --- | ---: | --- | --- |
| `0x00` | 4 | 32-bit big-endian word | Version/reserved word; retained without validation or interpretation; fixtures below choose value 4 |
| `0x04` | 2 | Signed 16-bit big-endian | Frames per second |
| `0x06` | 2 | Signed 16-bit big-endian | Action-frame timing value |
| `0x08` | 2 | Signed 16-bit big-endian | Number of frames in each stored direction sequence |
| `0x0A` | 12 | Six signed 16-bit big-endian integers | Direction X shifts, in direction order |
| `0x16` | 12 | Six signed 16-bit big-endian integers | Direction Y shifts, in direction order |
| `0x22` | 24 | Six signed 32-bit big-endian integers | Direction frame-area offsets, relative to byte `0x3E`; expected nonnegative |
| `0x3A` | 4 | Signed 32-bit big-endian | Declared frame-area byte count; expected nonnegative, not a read boundary enforced by the loader |
| `0x3E` | Variable | Frame records | Beginning of the frame-data area |

The six X values are contiguous, followed by all six Y values: the file does not interleave X/Y pairs. For direction d, the X, Y, and area-offset fields begin at `0x0A + 2 * d`, `0x16 + 2 * d`, and `0x22 + 4 * d`, respectively.

| Direction number | Facing | Direction-specific extension |
| ---: | --- | --- |
| 0 | Northeast | `.FR0` |
| 1 | East | `.FR1` |
| 2 | Southeast | `.FR2` |
| 3 | Southwest | `.FR3` |
| 4 | West | `.FR4` |
| 5 | Northwest | `.FR5` |

### 2.3 Per-frame records

These offsets are relative to the beginning of one frame record, not the file header.

| Relative offset | Byte length | Representation | Meaning |
| --- | ---: | --- | --- |
| `0x00` | 2 | Signed 16-bit big-endian | Pixel width |
| `0x02` | 2 | Signed 16-bit big-endian | Pixel height |
| `0x04` | 4 | Signed 32-bit big-endian | Number of pixel-data bytes; expected nonnegative |
| `0x08` | 2 | Signed 16-bit big-endian | Frame X displacement |
| `0x0A` | 2 | Signed 16-bit big-endian | Frame Y displacement |
| `0x0C` | Declared pixel-data byte count | Unsigned 8-bit palette indexes | Raw pixels, left to right within a row, rows top to bottom |

There is no compression, row padding, embedded palette, alpha plane, or alignment padding between frames. Ordinary valid frames have `pixel-data byte count = width * height`. The pixel at zero-based column x and row y occupies relative byte `12 + y * width + x`. The next frame begins immediately after the declared pixel-data byte count, not after an independently recalculated area.

The logical record address for frame f in direction d is:

- `62 + direction area offset + sum of (12 + pixel-data byte count) for frames 0 through f - 1`.
- For frame 0 the sum is empty and therefore zero.
- The declared total frame-area byte count includes frame-record headers and pixel bytes of stored sequences, not the 62-byte file header. Shared sequences need not be counted six times.

### 2.4 `.LST` text and ID representation

These are byte-oriented text files, not binary arrays of stored IDs. There is no header, count word, or explicit ID attached to a name. Normal input is ASCII with LF (`0A`) or CRLF (`0D 0A`) line endings. Text-mode reading converts CRLF to LF. A lone CR is not an ordinary line terminator or one of the delimiters listed below. There is no UTF-8 BOM removal.

| Byte position / unit | Representation | Meaning |
| --- | --- | --- |
| Byte 0 onward | Consecutive text lines | Ordered entries; no sorting or deduplication |
| LF, or CRLF before text conversion | Line boundary | Ends the current bounded line read |
| Final nonempty byte sequence without LF | Final bounded line read | Counts as an entry |
| Empty file | No successful line reads | No actual entries |
| Final LF without further bytes | Terminator only | Does not create an additional entry |

Line selection occurs before name/comment interpretation. Blank lines and comment-like lines therefore consume positions. More exactly, a position is one successful bounded line read: unusually long physical lines split into multiple positions. The limits below include a returned LF but exclude the terminating in-memory text terminator, which is not a file byte.

| Consumer | Maximum bytes per read | Name extraction | Mapping from physical position p, counted from 1 |
| --- | ---: | --- | --- |
| `art/<category>/<category>.lst` | 199 | Prefix before the first space, comma, semicolon, TAB, or LF; keep at most 12 bytes | Art list index `p - 1`; the art identifier's low 12 bits select that index |
| `scripts/scripts.lst` | 259 | Prefix before the first dot; replace the suffix with `.int`; no dot means name lookup fails | Runtime script index `p - 1`; applicable script-reference low 24 bits already contain this zero-based index |
| `proto/<category>/<category>.lst` | 255 | Prefix before the first space or LF; no other generic delimiter | Prototype identifier's low 24 bits select p directly: first entry is 1 |

These physical-position formulas assume lines fit in one read. For split lines, substitute the successful-read position for p. Prototype index 0 is not an entry in these lists. A map's positive script-reference field is separately one-based: reference 1 becomes runtime script index 0, reference 2 becomes index 1. A nonpositive map script reference does not attach a map script. Do not subtract one from an already zero-based prototype/object script reference.

Art categories, in category-number order 0 through 10, are `items`, `critters`, `scenery`, `walls`, `tiles`, `misc`, `intrface`, `inven`, `heads`, `backgrnd`, and `skilldex`. Each has its own index namespace; a prototype ID is not an art-list index.

## 3. Behavioral Rules & Edge Cases

### 3.1 Palette validity, RGB scaling, and transparency

- A palette entry is valid for color mapping only when all three stored channels are at most 63. If any channel exceeds 63, the entire entry becomes `(0, 0, 0)` and is marked unavailable for color mapping. Values are not individually clamped to 63 or treated as already-eight-bit RGB.
- At neutral gamma the observed display conversion for each valid six-bit channel is multiplication by 4. Thus six-bit values `0, 1, 31, 32, 63` become eight-bit values `0, 4, 124, 128, 252`. The output storage domain is 0 through 255, but this conversion only produces 0 through 252. Full-range normalization to 255, rounding `255 * channel / 63`, and bit replication are not the reference behavior.
- With non-neutral gamma, first raise the six-bit channel to the configured gamma exponent, clamp the result to 0 through 63, discard its fractional part, then multiply by 4 for display. Gamma 1 is neutral. The PAL bytes themselves remain six-bit quantities.
- Sprite pixel index 0 is the transparent key in transparent blits and world-object pixel hit testing: it leaves the destination unchanged and is not a solid hit, irrespective of entry 0's RGB triple. No alpha byte exists in either file. A nonzero index whose RGB triple is black is not thereby transparent. Opaque blits can still copy index 0.
- The palette reader checks opening the file but does not consistently check the byte counts returned by palette/table reads. A 768-byte palette-only file or a truncated lookup/discriminator/table region is not a reliably validated `color.pal`; no deterministic missing-byte defaults are established. Do not equate a missing discriminator with an explicit nonmatching four-byte discriminator.

### 3.2 Palette cycling

All index ranges are inclusive. Cycling replaces display-palette entries; sprite pixel indexes and the on-disk palette do not change. The sequences below are the six-bit RGB triples actually used after initial conversion of the built-in cycle colors.

| Index range | Effect | Base interval, milliseconds | Initial sequence of six-bit RGB triples |
| --- | --- | ---: | --- |
| 229-232 | Slime | 200 | `(0,27,0) (2,28,1) (6,30,3) (10,32,6)` |
| 233-237 | Monitors | 100 | `(26,26,27) (24,25,31) (21,26,35) (0,36,40) (26,46,63)` |
| 238-242 | Slow fire | 200 | `(63,0,0) (53,0,0) (36,10,2) (63,29,0) (63,14,0)` |
| 243-247 | Fast fire | 142 | `(17,0,0) (30,0,0) (44,0,0) (30,0,0) (17,0,0)` |
| 248-253 | Shoreline | 200 | `(20,15,10) (18,14,10) (16,13,9) (15,12,9) (13,11,8) (12,10,8)` |
| 254 | Pulsing red marker | 33 | Red channel `4, 8, ... 60, 56, ... 0, 4, ...`; green and blue remain 0 |

The first due update of a rotating range writes the initial sequence; each following due update rotates it right by one RGB entry. The red marker begins internally at zero and its first due update writes 4. Its maximum six-bit red value is 60, not 63.

Cycling defaults to enabled, with interval multiplier 1. Each effective interval is the table interval multiplied by the configured speed factor. A due check performs one update and resets that group's timestamp to the current time: it does not replay every missed interval after a long pause. Factor 0 makes each group due on every enabled poll; the setting is not positively range-validated.

The three 200-millisecond ranges share a timer. The other intervals have separate timers. Disabled cycling performs no updates. Reset clears timing baselines and enables cycling but does not rewind the rotating phases or red pulse state. Palette fading temporarily disables cycling and restores its previous enabled state afterward. Each changed poll submits entries 229 through 255 inclusive, but no cycle modifies entry 255 itself. Entries 0 through 228 are outside these cycle writes.

### 3.3 Sprite sequence loading and direction-specific files

- A `.FR0` through `.FR5` file has the same complete header and frame-record layout as `.FRM`. The extension is not another field in the file and does not imply a shortened header. In critter filename selection, selector 0 requests `.FRM`; selectors 1 through 6 request `.FR0` through `.FR5`, respectively. These one-based filename selectors are distinct from zero-based direction numbers.
- A direction-specific file can contain one sequence with all six area offsets equal to zero. Access still uses the selected direction's offset and X/Y shifts; the loader does not automatically populate only one header slot based on the suffix. Identical offsets share pixels but need not share shifts.
- Logical addressing is relative to byte 62, but actual file reads are sequential. The loader starts immediately after the header, reads one sequence for direction 0, and reads another sequence only when a direction's offset differs from the immediately preceding direction's offset. Every sequence read contains the header's number of frames.
- There is no file seek to each declared offset. For normal interoperable files, the first offset is zero and successive distinct direction blocks are packed consecutively in traversal order. Equal adjacent offsets reuse the previous block without another read. A repeated nonadjacent offset is not globally deduplicated: it triggers another read and can overwrite the earlier block in memory. Gaps, reordered blocks, or offsets pretending to seek elsewhere do not have ordinary address-table semantics.
- A frame is located by walking each preceding record's declared payload size plus 12 bytes. No check enforces equality of that size to width times height. No check enforces version 4, a positive frame count, a matching total-area byte count, or safe offset bounds. Short header/frame reads cause loading failure, but malformed lengths/offsets can instead cause unsafe behavior. A defensive independent reader should reject such input rather than promise equivalent memory faults.
- Valid frame indexes are 0 through frame count minus 1; valid directions are 0 through 5. The frame count is a count, not the last frame index.

### 3.4 FPS and action timing

Stored FPS 0 means 10 FPS. Otherwise the signed stored value is used, without a positive-range clamp. Ordinary animation timing uses integer `1000 / effective FPS` milliseconds per frame, discarding the fractional part: 12 FPS gives 83 milliseconds. Some combat walking playback adds configured speed to FPS before division, so file FPS is not an unconditional wall-clock guarantee.

The action-frame value is returned and used unchanged as a frame-step delay for effects such as attack contact sounds and pickup interactions. It is not decremented from a presumed one-based frame number. In the ordinary nonnegative case, 0 adds no frame-step delay and 1 delays by one step. The loader does not clamp it to the number of frames or interpret it as a frame-area offset. Negative values remain negative; the animation scheduler treats negative queued delays as waiting for preceding activity, not as unsigned frame indexes.

### 3.5 Anchor, bottom-center alignment, and frame displacement

Coordinates increase to the right and downward. In world-object drawing, the anchor is the center of the screen-space hex: the hex's projected reference coordinate plus `(16, 8)`. Let the current object's additional screen displacement be separate from the header's direction shift. The destination rectangle is calculated as:

- Horizontal bottom-center = anchor X + direction X shift + current X displacement.
- Vertical bottom-center = anchor Y + direction Y shift + current Y displacement.
- Left = horizontal bottom-center minus `floor(width / 2)`.
- Top = vertical bottom-center minus height plus 1.
- Right = left plus width minus 1; bottom = vertical bottom-center. Rectangle endpoints are inclusive.

Thus the local anchor before shifts is pixel coordinate `(floor(width / 2), height - 1)`, not the top-left or geometric center of the rectangle. An odd width uses its middle column; an even width uses the column immediately right of the midpoint between its middle columns.

Per-frame X/Y values are displacement increments, not absolute top-left coordinates or substitutes for the six header shifts. During forward animation, entering a frame adds that frame's displacement to the object's current displacement; reversing subtracts the displacement of the frame being left. For an isolated forward pass beginning before frame 0 with zero extra displacement, the displacement at frame f is the sum of frame displacements 0 through f inclusive. World movement, animation transitions, and existing object displacement can change that initial condition. Simply selecting a frame record does not itself perform this accumulation, and drawing does not add its displacement a second time.

A sprite used as a mouse cursor uses direction 0 and hotspot `(floor(width / 2) minus direction X shift, height minus 1 minus direction Y shift)`, without automatically summing per-frame displacements. An unscaled preview that fits its destination box instead centers the pixel rectangle in both axes: left/top equal the box's left/top plus half the unused width/height, with integer truncation. Such previews ignore world anchors and direction shifts. Therefore bottom-center world placement is not a universal rule for every UI use of the same pixels.

### 3.6 LST comments, whitespace, metadata, and boundary cases

- There is no common comment-removal or whitespace-trimming pass. Art names end at the first space, comma, semicolon, TAB, or LF, even inside a purported name. Leading space/TAB/semicolon/comma therefore produces an empty name, not a skipped line. A semicolon can introduce ignored trailing text, but a semicolon-only comment still consumes an index. `#` and `//` have no special art-list comment meaning. Case is preserved; duplicate names keep distinct positions.
- Art names are truncated to 12 bytes after delimiter handling. Critter entries may be stems rather than filenames, with animation and extension selected separately. The first comma's following decimal integer is also read separately as critter alias metadata; absent comma uses the default character art index, initially 0. That alias is zero-based. Ordinary name extraction does not mean every comma tail is globally ignored.
- Head-list entries have a separate metadata read. In this inspected revision, when a comma is present, the first decimal count after it supplies all three reaction/fidget counts; later comma-separated counts are not independently reached. For example, a tail `,2,3,4` yields counts `(2,2,2)`. This describes the observed reader, not a corrected CSV interpretation.
- Script lines are selected by zero-based position without skipping blanks or comment lines. For name lookup, the first dot anywhere in the line ends the stem; leading whitespace is preserved, and `.int` replaces the remainder. A comment line containing a dot can therefore produce a name, whereas a blank line or any line without a dot fails name lookup.
- A separate script-metadata read examines text following the first `#`; the case-sensitive literal token `local_vars=` supplies a following decimal local-variable count. This is annotation handling, not a general comment grammar. Decimal parsing allows leading whitespace and an optional sign, stops at the first nondecimal character, and produces zero if no digits are parsed. This annotation handling does not remove the line from the index.
- Prototype name extraction recognizes only space and LF. TAB, comma, semicolon, `#`, and `//` are not generic delimiters/comments for this reader. Leading space yields an empty filename. All lines still consume their one-based positions.
- Actual art entry counts use successful bounded reads. Script bookkeeping instead records LF count plus 1, even with a trailing LF or an empty file; that does not make a final phantom line readable. Prototype allocation bookkeeping records LF count plus 2 in this revision, independently of its actual readable entries.
- Prototype EOF handling is not a reliable bounds check: for a nonempty list, a request exactly one successful-read position beyond the end can return the last line's name again. It does not establish an extra actual entry. Empty-list lookup and malformed binary/text inputs do not have a dependable compatibility result; an independent implementation should distinguish these defects from supported ID mappings.

## 4. Synthetic Test Fixtures (Hex / Text)

These fixtures are invented, not extracted game assets. Hex offsets are zero-based; address labels are not file bytes. The PAL fixture specifies repeated-byte ranges explicitly to keep it small. The two sprite dumps list every byte and contain no implicit padding.

### 4.1 Fixture A: six-bit PAL values and an invalid entry

Length after expanding the zero-filled ranges: `0x8304` (33540) bytes. All RGB entries beyond index 3 are zero; every RGB555 lookup result is index 0. The discriminator is explicitly nonmatching, so intensity/mix tables are generated, not read.

```text
0000: 01 02 03 3F 20 01 10 08 04 40 01 02
000C-02FF: 00 repeated 756 times
0300-82FF: 00 repeated 32768 times
8300: 00 00 00 00
```

Expected results at neutral gamma:

| Palette index | Stored RGB | Mapping validity | Eight-bit display RGB | Transparent-sprite behavior |
| ---: | --- | --- | --- | --- |
| 0 | `(1,2,3)` | Available | `(4,8,12)` | Skip pixel despite its nonblack RGB |
| 1 | `(63,32,1)` | Available | `(252,128,4)` | Draw index 1 |
| 2 | `(16,8,4)` | Available | `(64,32,16)` | Draw index 2 |
| 3 | `(64,1,2)` | Unavailable | `(0,0,0)` after whole-entry replacement | Nonzero pixel index is still not the transparent key |

The deliberately constant lookup table is structurally valid but not a useful nearest-color quantizer. No particular visual quality is asserted for its generated intensity/mix results.

### 4.2 Fixture B: one shared frame, zero FPS, and signed header shift

Complete length: `0x4C` (76) bytes. The same bytes can be named `.FRM` or `.FR0`; the loader has no extension-dependent binary layout.

```text
0000: 00 00 00 04
0004: 00 00
0006: 00 00
0008: 00 01
000A: 00 02 00 00 00 00 00 00 00 00 00 00
0016: FF FF 00 00 00 00 00 00 00 00 00 00
0022: 00 00 00 00 00 00 00 00 00 00 00 00
002E: 00 00 00 00 00 00 00 00 00 00 00 00
003A: 00 00 00 0E
003E: 00 02 00 01 00 00 00 02 00 00 00 00
004A: 00 01
```

Expected: effective FPS 10, ordinary frame interval 100 milliseconds, action timing 0, and one frame per direction. All six directions share the frame at file offset `0x3E`; only direction 0 has header shift `(2,-1)`. The frame is 2 by 1 pixels, payload length 2, displacement `(0,0)`, with one row of indexes `0,1`. Frame-area length is 14 bytes, including its 12-byte record header.

At world anchor `(100,100)`, direction 0 and zero extra displacement produce rectangle left 101, top 99, right 102, bottom 99. The pixel at `(101,99)` leaves the destination unchanged; the pixel at `(102,99)` draws index 1. Direction 0 cursor hotspot is `(-1,1)`, showing the subtraction of header shifts rather than treating them as a top-left coordinate.

### 4.3 Fixture C: two stored sequences, two frames each, and cumulative displacement

Complete length: `0x74` (116) bytes. Directions 0 and 1 share the first sequence; directions 2 through 5 share the second. Its first record begins at `0x3E + 0x1C = 0x5A`.

```text
0000: 00 00 00 04
0004: 00 0C
0006: 00 01
0008: 00 02
000A: 00 00 00 00 FF FE 00 00 00 00 00 00
0016: 00 00 00 00 00 03 00 00 00 00 00 00
0022: 00 00 00 00 00 00 00 00 00 00 00 1C
002E: 00 00 00 1C 00 00 00 1C 00 00 00 1C
003A: 00 00 00 36
003E: 00 02 00 01 00 00 00 02 00 01 FF FF
004A: 01 02
004C: 00 01 00 02 00 00 00 02 00 02 00 01
0058: 00 03
005A: 00 01 00 01 00 00 00 01 00 00 00 00
0066: 04
0067: 00 01 00 01 00 00 00 01 00 00 00 00
0073: 05
```

Expected: 12 FPS, ordinary frame interval 83 milliseconds, unchanged action timing 1, and two frames per sequence. Offsets are `0,0,28,28,28,28`; the stored frame area is 54 bytes, not six times a sequence size.

| Directions | Frame | Record file offset | Dimensions | Frame displacement | Pixel rows |
| --- | ---: | --- | --- | --- | --- |
| 0-1 | 0 | `0x3E` | 2 by 1 | `(1,-1)` | `01 02` |
| 0-1 | 1 | `0x4C` | 1 by 2 | `(2,1)` | `00`, then `03` |
| 2-5 | 0 | `0x5A` | 1 by 1 | `(0,0)` | `04` |
| 2-5 | 1 | `0x67` | 1 by 1 | `(0,0)` | `05` |

For a fresh isolated forward pass at anchor `(100,100)` in direction 0, applying frame 0 gives accumulated displacement `(1,-1)` and rectangle `(100,99)` through `(101,99)`. Entering frame 1 gives accumulated displacement `(3,0)` and rectangle `(103,99)` through `(103,100)`. The top pixel is transparent and the lower pixel is index 3. Reversing from frame 1 subtracts `(2,1)`, restoring frame 0's displacement and rectangle.

Direction 2 has header shift `(-2,3)` and zero frame displacements, so either of its 1-by-1 frames is drawn at `(98,103)`. Direction 3 shares those frame records but has zero header shifts and therefore draws at `(100,100)`.

### 4.4 Text vectors: positions are not filtered entries

For the generic art-name parser, all six lines below end with LF. The second line is empty; the third begins with one space.

```text
alpha.frm;ignored

 beta.frm
#note
;ignored
gamma.frm,7
```

Expected art indexes 0 through 5 contain, respectively: `alpha.frm`, empty name, empty name, `#note`, empty name, `gamma.frm`. There are six positions, not two or three. The final LF creates no seventh position. Replacing LF with CRLF preserves these results.

Prototype text, with all three lines LF-terminated:

```text
one.pro

two.pro ;note
```

Expected supported prototype mapping: low-24-bit ID 1 selects `one.pro`, ID 2 selects an empty name, and ID 3 selects `two.pro`. ID 0 does not mean the first entry. The reference's EOF defect can repeat `two.pro` for ID 4; a validated implementation must not count that as a fourth line.

Script text, with both lines LF-terminated:

```text
first.ssl # local_vars=3
second.ssl
```

Expected runtime script index 0 resolves to `first.int` with local-variable count 3; index 1 resolves to `second.int`. Map script references 1 and 2 select those same indexes after subtracting one. An object/prototype script reference already containing index 0 selects the first line without subtraction.

## 5. Clean-Room Verification Checklist
- [x] Zero C/C++ code snippets or struct definitions included
- [x] All multi-byte integer endianness explicitly stated
- [x] Synthetic test vectors included
