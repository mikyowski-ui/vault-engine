# SPEC-002: Bitmap Fonts and Message Text

## Clean-Room Rules for Research Agents
- Document ONLY binary file layouts (byte offsets, endianness, data types), mathematical formulas, state transitions, and observable behavior.
- DO NOT copy C/C++ code, struct definitions, function names, or internal variable names from `fallout1-re`.
- Provide small synthetic hex/text test vectors at the bottom of the specification.

## 1. Overview & Scope

This specification describes the on-disk `.FON`, `.AAF`, and `.MSG` formats and their source-derived loading, measurement, and rendering behavior. The reference is `fallout1-re` revision `ac9784b0735468d57ddb2025b9ca76a43a404f4c`, targeting 32-bit Windows/x86. It is not a specification for Windows resource fonts, Unicode fonts, or other games' similarly named files.

- `.FON`: `font0.fon` through `font5.fon`; font selection numbers 0 through 5. All use the same format.
- `.AAF`: `font0.aaf` through `font4.aaf`; font selection numbers 100 through 104. All use the same format.
- `.MSG`: language-specific files under `text/<language>/`, including `game/` and `dialog/`. A requested relative filename is resolved below the configured language directory; the loader does not try a second language on failure. `[Must-match]`
- Font filenames are resolved at the asset root, not below `text/`. Neither font format embeds a character encoding, palette, font name, or pair-kerning table. `[Must-match]`
- The adjacent `local-corpus/fallout1-unpacked` supplies the observed asset metrics below. It contains no `font4.fon`; that name is supported by the loader, but its file-specific metrics are unverified. No shipped glyph images or message text are reproduced here.

**Compatibility tags.** Every documented quirk carries one tag:

- `[Must-match]`: gameplay-visible behavior, data grammar, formula, ordering, or random-draw sequence. A clean-room engine must reproduce it exactly.
- `[May-improve]`: reference memory-safety defects, undefined behavior, uninitialized reads, stale state, presentation-only transients, or a lookup that reads the wrong table in an obviously unintended way. A clean-room engine may apply the stated safe rule and should keep every other observable behavior. An engine that wants bit-exact parity may offer a compatibility switch that keeps the reference behavior.

Plain layout tables carry one tag for the whole table.

Evidence locations, relative to the research-vault root:

| Subject | Source location |
| --- | --- |
| `.FON` disk layout and binary reads | [redacted-ref]; [redacted-ref] |
| `.FON` drawing and metrics | [redacted-ref] |
| `.AAF` disk layout and binary reads | [redacted-ref] |
| `.AAF` drawing and metrics | [redacted-ref] |
| Palette coverage and intensity mathematics | [redacted-ref] |
| `.MSG` path selection and load outcome | [redacted-ref] |
| `.MSG` replacement, decimal IDs, and field scanning | [redacted-ref] |
| Text-stream newline translation | [redacted-ref] |
| Platform and font selection ranges | `fallout1-re/CMakeSettings.json`; [redacted-ref]; [redacted-ref]; [redacted-ref] |

These are static-analysis results, not a claim that malformed inputs or pixel output were tested in the original executable. Undefined or platform-dependent behavior is identified rather than assigned an invented result.

## 2. Binary Layout / Data Format (Byte Offset Tables)

### 2.1 Conventions

All offsets are zero-based byte offsets. Widths, heights, and spacing are in pixels. A signed integer uses two's-complement representation. ASCII byte values identify syntax and signatures independently of any display encoding. Mathematical letters in this document are local notation, not source identifiers.

### 2.2 `.FON`: Little-Endian Monochrome Bitmap Font

The header is exactly 20 bytes in the reference's 32-bit file format. There is no magic number or version. Every multi-byte integer in the header and glyph directory is **little-endian**. `[Must-match]`

| File offset | Bytes | Type | Meaning |
| --- | ---: | --- | --- |
| `0x00` | 4 | Signed 32-bit, little-endian | Number of glyph-directory entries, denoted N |
| `0x04` | 4 | Signed 32-bit, little-endian | Common glyph height, denoted H |
| `0x08` | 4 | Signed 32-bit, little-endian | Horizontal inter-character spacing, denoted S |
| `0x0C` | 4 | Opaque bytes | Ignored header word; not a file offset |
| `0x10` | 4 | Opaque bytes | Ignored header word; not a file offset |
| `0x14` | 8 x N | Glyph directory | One entry for each consecutive byte code, starting at code zero |
| `0x14 + 8 x N` | Variable | Bitmap bytes | Bitmap-data origin, denoted B |

On successful loading, the two opaque words are consumed but replaced. They need not be zero and carry no usable file address, offset, or directory-pointer meaning. `[Must-match]`

Each directory entry for byte code G begins at `0x14 + 8 x G`:

| Entry-relative offset | Bytes | Type | Meaning |
| --- | ---: | --- | --- |
| `+0` | 4 | Signed 32-bit, little-endian | Glyph width, denoted W(G) |
| `+4` | 4 | Signed 32-bit, little-endian | Glyph bitmap offset relative to B, denoted O(G) |

There is no individual glyph height, bearing, advance field, or character-map table. For a positive-width glyph, row stride R(G) is `ceil(W(G) / 8)` bytes, and its bitmap occupies `H x R(G)` bytes beginning at `B + O(G)`. `[Must-match]`

Rows are stored top to bottom; pixels within a row run left to right. Pixel column X uses byte `floor(X / 8)` and bit weight `2^(7 - (X modulo 8))`. Thus the most-significant bit is leftmost. Each row restarts at a byte boundary; unused low bits in the final byte of a row are ignored, not carried into the next row. A set bit paints the requested palette index; a clear bit leaves the destination unchanged. No alpha values or RGB values are stored. `[Must-match]`

The number of bitmap bytes actually read is:

`D = O(N - 1) + H x ceil(W(N - 1) / 8)`

The loader reads D bytes immediately after the directory. This is determined by the **last directory entry**, not the largest extent among all glyphs and not the entire remaining file. A well-formed file must place every drawable glyph inside that loaded range. Glyph offsets need not be contiguous; shared offsets and gaps have no special interpretation. `[Must-match]`

### 2.3 `.AAF`: Big-Endian Antialiased Bitmap Font

The fixed header is 12 bytes, followed by exactly 256 directory entries of 8 bytes each. Bitmap bytes start at decimal offset 2060, hexadecimal `0x080C`. Every multi-byte numeric field is **big-endian**. `[Must-match]`

| File offset | Bytes | Type | Meaning |
| --- | ---: | --- | --- |
| `0x00` | 4 | Literal bytes | `41 41 46 46`, ASCII `AAFF`; required signature |
| `0x04` | 2 | Signed 16-bit, big-endian | Maximum glyph height, denoted H |
| `0x06` | 2 | Signed 16-bit, big-endian | Letter spacing, denoted S |
| `0x08` | 2 | Signed 16-bit, big-endian | Space-character width, denoted Q |
| `0x0A` | 2 | Signed 16-bit, big-endian | Additional line spacing, denoted L |
| `0x0C` | 2048 | 256 directory entries | Consecutive byte codes `0x00` through `0xFF` |
| `0x080C` | File length minus 2060 | Pixel bytes | Bitmap-data origin, denoted B |

Each directory entry for byte code G begins at `0x0C + 8 x G`:

| Entry-relative offset | Bytes | Type | Meaning |
| --- | ---: | --- | --- |
| `+0` | 2 | Signed 16-bit, big-endian | Glyph bitmap width, denoted W(G) |
| `+2` | 2 | Signed 16-bit, big-endian | Glyph bitmap height, denoted T(G) |
| `+4` | 4 | Signed 32-bit, big-endian | Glyph pixel offset relative to B, denoted O(G) |

There are no extra reserved words between the header and directory, and no glyph-count field. A glyph occupies exactly `W(G) x T(G)` bytes, tightly packed by rows, top to bottom and left to right. Pixel `(X,Y)` is the full byte at `B + O(G) + Y x W(G) + X`. There is no bit packing, nibble packing, per-row padding, compression, or embedded palette. Zero-area entries have no pixel bytes to draw. `[Must-match]`

Pixel bytes select palette-blending levels, not ordinary 8-bit opacity. The common coverage range is 0 through 7; level 8 is also present in the observed `font0.aaf`. The additional legacy levels and their limits are specified in Section 3.2. `[Must-match]`

### 2.4 Observed Named-Font Metrics

These values describe the available local assets, not constants enforced by the formats. Width ranges include zero-width directory entries. File lengths and bitmap-data lengths are in bytes.

| `.FON` filename | File length | Glyph count | Height | Letter spacing | Width range | Width at `0x20` | Width at `0x41` | Bitmap-data length |
| --- | ---: | ---: | ---: | ---: | --- | ---: | ---: | ---: |
| `font0.fon` | 3044 | 128 | 16 | 1 | 0-9 | 5 | 7 | 2000 |
| `font1.fon` | 2044 | 128 | 10 | 1 | 0-7 | 5 | 7 | 1000 |
| `font2.fon` | 2122 | 128 | 11 | 1 | 0-11 | 5 | 5 | 1078 |
| `font3.fon` | 2354 | 128 | 10 | 1 | 0-15 | 5 | 8 | 1310 |
| `font4.fon` | Unavailable | Unverified | Unverified | Unverified | Unverified | Unverified | Unverified | Unverified |
| `font5.fon` | 2044 | 128 | 10 | 1 | 0-7 | 5 | 7 | 1000 |

All available `.FON` files have bitmap-data origin `0x0414` (1044). Their last-entry extent exactly matches the available bitmap bytes, and all glyph extents fit. `font1.fon` and `font5.fon` are byte-identical in this corpus.

| `.AAF` filename | File length | Maximum height | Letter spacing | Space width | Extra line spacing | Reported line height | Width range | Glyph-height range | Observed pixel levels |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- | --- |
| `font0.aaf` | 6337 | 9 | 1 | 4 | 3 | 12 | 0-12 | 0-9 | 0-8 |
| `font1.aaf` | 12844 | 9 | 1 | 4 | 1 | 10 | 0-10 | 0-9 | 0-7 |
| `font2.aaf` | 25395 | 24 | 1 | 4 | 4 | 28 | 0-14 | 0-24 | 0-7 |
| `font3.aaf` | 13399 | 16 | 1 | 4 | 4 | 20 | 0-10 | 0-16 | 0-7 |
| `font4.aaf` | 22129 | 22 | 1 | 4 | 4 | 26 | 0-13 | 0-22 | 0-7 |

All 256 entries are present in each observed `.AAF`; all glyph extents fit its bitmap-data section. In these assets, the `0x20` entry has width 4 and height 0. No extra bytes follow the largest glyph extent.

### 2.5 `.MSG`: Byte-Oriented Brace Fields

There is no binary header, signature, count, declared encoding, or fixed record offset. Endianness does not apply to the text syntax: IDs are decimal ASCII text, not binary integers. `[Must-match]`

The conventional record is ID field, sound field, text field, written as `{ID}{SOUND}{TEXT}`. It consists of three consecutive *fields*, not necessarily three fields on one physical line. `[Must-match]`

| Position within a field | Bytes | Meaning |
| --- | --- | --- |
| Before the opening delimiter | Variable | Scanner skips bytes until `7B`; an intervening `7D` is an error |
| Field start | `7B` | ASCII opening brace |
| Field contents | Variable | Bytes through, but not including, the next `7D`; line-feed removal applies |
| Field end | `7D` | ASCII closing brace |

| Field order | Interpretation |
| --- | --- |
| 1 | Nonempty decimal message ID, optionally prefixed by `2B` or `2D` (plus or minus) |
| 2 | Sound-name bytes; an empty field is allowed |
| 3 | Message-text bytes; an empty field is allowed |

Important control bytes are `0A` (line feed), `0D` (carriage return), `00` (string terminator after loading), and the two brace bytes. Literal backslashes, quotes, hashes, and semicolons have no escaping or comment role. `[Must-match]`

## 3. Behavioral Rules & Edge Cases

### 3.1 Font Selection, Measurement, and Placement

The `.FON` startup search tries suffixes 0 through 9, even though this specification's requested names stop at 5. The `.AAF` startup search tries suffixes 0 through 15; the game's registered selection range is 100 through 110. Each subsystem initially chooses its lowest successfully loaded suffix and reports initialization failure if none load. An absent font is not synthesized from another file. Selecting a missing slot does not replace the previously selected bitmap and metrics with a nearest-numbered font. `[Must-match]` In the reference, selecting an unloaded number inside a registered range (0 through 9 or 100 through 110) still switches the active font subsystem and the reported current-font number, while that subsystem keeps drawing with the bitmap and metrics it last selected successfully. `[May-improve]`: treat selection of an unloaded font as a no-op that leaves the current font number, subsystem, bitmap, and metrics unchanged.

For normal proportional measurement:

| Observable | `.FON` | `.AAF` |
| --- | --- | --- |
| Single-character width, excluding spacing | W(G) | Q for byte `0x20`; otherwise W(G) |
| Advance per supported character | W(G) + S | Q + S for `0x20`; otherwise W(G) + S |
| Width of a string | Sum of advances | Sum of advances |
| Final trailing spacing | Included | Included |
| Reported line height | H | H + L |
| Width-times-height buffer-size estimate | String width x H | String width x (H + L) |
| Glyph top relative to the supplied text origin | 0 | H - T(G) |
| Glyph bottom, exclusive | H | H |

The measurement and placement rules in this table are `[Must-match]`.

There is no pair kerning, baseline table, horizontal bearing, automatic wrapping, or newline interpretation in these low-level font operations. Newline and tab bytes are glyph codes if supplied directly; the message loader's newline removal is a separate operation. Byte zero terminates a supplied string, so glyph zero cannot be reached through ordinary string drawing. Empty-string width is zero. A zero-width supported glyph still contributes letter spacing. `[Must-match]`

`.AAF` glyphs are bottom-aligned within the maximum-height band; L adds blank line spacing below that band, not above each glyph. The space byte uses Q for measurement and advance, but drawing still visits its own directory entry. A nonempty synthetic space bitmap would therefore be drawn at its stored width and height, even when that width differs from Q. `[Must-match]`

The `.FON` maximum-cell metric is `max(W(G)) + S`. Its monospaced string-width query is that value multiplied by the number of bytes before the terminating zero. In proportional mode, nonnegative byte codes outside the `.FON` directory are skipped without any advance; the monospaced width query nevertheless counts those bytes. The drawing operation also skips unsupported codes rather than reserving a cell for them. `[Must-match]`

The `.AAF` legacy maximum-cell metric is **not** the maximum glyph width. Its observable calculation is `H + S` when Q is positive, and `H + L` when Q is zero or negative. No maximum-width field is read from the file. `[Must-match]` for positive Q, which all observed assets use; word-wrapping and title-width layout consult this metric. The zero-or-negative branch arises only because Q is compared against an in-memory value that is never loaded from the file and is always zero. `[May-improve]`: use `H + S` regardless of Q. Its monospaced width query multiplies this result by the number of bytes in the string. For the observed `font0.aaf` through `font4.aaf`, the results are respectively 10, 10, 25, 17, and 23 pixels. In particular, a glyph can be wider than this reported cell. `[Must-match]`

Monospaced drawing uses the corresponding maximum-cell metric as its advance. Within a cell, the horizontal drawing offset is half of `C - w - S`, truncated toward zero, where C is the cell width and w the character width. For `.AAF` space, w in this formula is Q. There is no glyph-width correction if that offset becomes negative. `[Must-match]`

Drawing stops before the first glyph whose **complete advance**, including spacing, would exceed the supplied horizontal limit. It does not draw a partial glyph and then continue. An advance ending exactly at the limit is allowed. Neither low-level font renderer breaks a string into multiple lines. `[Must-match]` Glyph pixels themselves are not individually clipped; callers must provide enough destination rows and a suitable row pitch. `[May-improve]`: clip every glyph, shadow, and underline pixel to the destination surface.

The output is an 8-bit indexed-color surface. The low eight bits of the drawing color select the foreground palette index. Optional drawing flags are `0x10000` for shadow, `0x20000` for underline, and `0x40000` for monospaced placement: `[Must-match]`

- A shadow is drawn first at offset `(1,1)`, in the active palette's black mapping. `.FON` drops underline and monospaced options for this shadow pass; `.AAF` preserves them. `[Must-match]`
- Underlining writes the foreground palette index across the total drawn advance, including its final spacing, on row `H - 1`. For `.AAF`, this is not row `H + L - 1`. `[Must-match]`
- These options do not change the file layout or ordinary reported width and height; shadow extents are not added to those metrics. `[Must-match]`

The `.AAF` full-string width and drawing paths interpret high-bit bytes as indices 128 through 255. `[Must-match]` However, its single-character width path is unsafe for high-bit bytes on the reference's signed-byte target. The `.FON` width paths also use signed-byte indexing, while its drawing path masks the glyph index after a signed comparison. Consequently the reference does **not** establish safe `.FON` behavior for bytes 128 through 255, nor safe single-character `.AAF` queries for those bytes. For the observed 128-entry `.FON` files, high-bit drawing can access beyond the directory rather than cleanly skipping the byte. These are implementation hazards, not an alternative encoding rule. `[May-improve]`: index every font operation with the unsigned byte value 0 through 255; for `.FON`, codes at or above N are skipped with no advance in width, monospaced-width counting stays per byte, and drawing skips them; for `.AAF`, the single-character width of a high-bit byte equals the full-string path's width for that byte.

### 3.2 `.AAF` Coverage and Palette Mathematics

Let A be a stored pixel byte, F the requested foreground palette index, and D the destination palette index at that pixel. The conceptual lookup address is `256 x A + D` in the blend table for F.

Let P map a palette index to its red, green, and blue 5-bit components. Each component is obtained by dividing the active 6-bit palette component by two and discarding the fraction. Let M map a packed RGB5 color back to a palette index through the active 32768-entry color lookup. RGB5 packing is `1024 x red + 32 x green + blue`; this describes a lookup index, not additional font-file bytes.

| Stored byte A | Palette result |
| --- | --- |
| 0 | D, unchanged; transparent `[Must-match]` |
| 1 through 7 | For each RGB5 component, compute `floor((A x P(F) + (7 - A) x P(D)) / 7)`; pack the components and apply M `[Must-match]` |
| 8 through 13 | Foreground-only intensity lookup described below; destination-independent `[Must-match]` |
| 14 through 15 | Blend-table rows exist but are not initialized by the recovered table builder; result is indeterminate `[May-improve]` |
| 16 through 255 | Lookup is beyond the allocated blend table; unsafe, with no defined pixel result `[May-improve]` |

Safe rule for the two `[May-improve]` rows: reject at load, with a diagnostic, any `.AAF` whose glyph pixel bytes exceed 13. No observed asset uses a level above 8.

Thus level 7 is full foreground **through palette remapping**, not necessarily the original numeric index F when the palette has duplicate colors. Levels 1 through 6 are seven-step coverage with integer truncation. Dividing A by 255, unpacking two four-bit pixels per byte, or treating 15 as ordinary opaque alpha is incompatible. `[Must-match]`

For A from 8 through 13, the intensity-table index is:

`K = floor((65535 + floor((A - 8) x 65536 / 7)) / 512)`

The corresponding K values are 127, 146, 164, 182, 201, and 219. `[Must-match]` The result is the active intensity-table entry for foreground F at K. That table may be supplied by the active color resource rather than generated. When generated for a mapped palette color, each foreground RGB5 component C becomes:

- For K below 128: `floor(C x K / 128)`.
- For K at least 128: `C + floor((31 - C) x (K - 128) / 128)`.

The resulting components are packed and mapped through M. Generated intensity entries for unmapped palette colors are zero. Level 8 can therefore differ from level 7 and is not just a second spelling of full opacity. Exact output palette indices depend on the active color and intensity resources; the font alone cannot specify them. `[Must-match]`

The recovered pixel-writing expression does not establish a sequenced destination read and pointer advance. The equations above specify its blend lookup; exact destination sampling order needs original-executable verification. The synthetic palette fixture below explicitly assumes same-coordinate destination sampling and is not evidence that this sequencing ambiguity has been resolved. `[May-improve]`: read the destination index at the pixel being written, then write the blended result to that same pixel.

### 3.3 Font Validation and Malformed Files

Both readers treat missing files, incomplete required reads, and allocation failures as load errors. `.AAF` additionally rejects any signature other than `41 41 46 46`. `.FON` has no signature check. `[Must-match]` A load error is not necessarily a clean rejection: early `.FON` error cleanup can use the opaque header words before replacement, so truncated headers/directories and allocation failures can also cause invalid memory accesses. `[May-improve]`: treat every incomplete read and allocation failure as a clean load error that releases only memory the loader itself allocated.

Neither reader validates all dimensions, arithmetic overflow, glyph bounds, ordering, or level bytes before rendering. `.FON` requires a positive glyph count to make its last-entry size calculation meaningful. Negative dimensions or offsets, zero-height fonts, overflowing sizes, or a glyph outside the loaded bitmap range can cause unsafe behavior rather than a defined rejection. A `.AAF` with no bitmap bytes also enters zero-size allocation/read edge cases; acceptance is not guaranteed. `[May-improve]`: apply the validation in the next paragraph and reject an invalid file with a diagnostic; treat a `.AAF` with no bitmap bytes as a load failure.

A safe independent reader should reject negative metrics and offsets, require a meaningful positive font height, perform size calculations without overflow, and verify every drawable glyph extent. For `.AAF`, glyph heights exceeding H would otherwise place drawing above the text origin. For `.FON`, bounds must use the last-entry-derived loaded length, even if more file bytes exist. `[Must-match]` This defensive validation is distinct from the reference's observed checks.

Extra `.FON` bytes after the D-byte bitmap block are ignored. `.AAF` loads all bytes after `0x080C`; bytes not referenced by any glyph are loaded but have no inherent meaning. There is no checksum in either format. `[Must-match]`

### 3.4 `.MSG` Scanning, Multiline Text, and Comments

The scanner has two states:

1. **Seeking a field:** discard bytes until an opening brace. A closing brace before an opening brace is a delimiter error. End of file here means no next field.
2. **Inside a field:** the first closing brace ends the field. Discard line-feed bytes; append every other byte, including opening braces. End of file before the closing brace is an unterminated-field error. `[Must-match]`

The record reader requests exactly three fields in ID, sound, text order and then repeats. It does not require adjacency of braces, one record per line, or any particular separator between fields. Two complete records can share a line. The sound field is stored as text; loading does not play a sound, validate a sound filename, or require that a corresponding sound asset exists. `[Must-match]`

CRLF pairs are normally translated to a single LF by the text stream; the field scanner then removes that LF. A standalone LF is also removed. A standalone CR is retained. Multiline contents are therefore **concatenated without an inserted space**. Spaces and tabs that were actually present are retained, including indentation and spaces immediately before line endings. The same rules apply to ID and sound fields, not just message text. `[Must-match]`

There is no comment delimiter. Hashes, semicolons, double slashes, and ordinary prose outside fields appear to work as comments only because all non-brace bytes there are skipped. Braces on such lines remain active. A nominal comment containing three valid brace fields creates a record; a stray closing brace in a comment fails the load. Inside a field, comment-looking text is literal content. `[Must-match]`

There is no quoting or escape language. A backslash followed by the letter n remains two bytes; it is not a newline. A backslash does not protect a closing brace. An opening brace inside field contents is retained as a literal byte, without nesting; a closing brace always ends the current field. `[Must-match]`

### 3.5 `.MSG` IDs, Replacement, Errors, and Limits

ID validation accepts a nonempty string consisting of an optional leading plus or minus followed by decimal digits. Leading/trailing spaces and tabs are not trimmed and make an ID invalid. Leading zeroes are allowed and do not request octal interpretation. A legacy exception accepts a plus or minus **alone**, converting it to ID zero. An empty ID is rejected. Any byte `0x80` through `0xFF` in an ID field reaches the reference's digit test as a negative signed character, which is undefined on the reference target. `[May-improve]`: classify such bytes as non-digits, so the ID is invalid and loading fails.

IDs are held as signed 32-bit numbers on the reference target. Values outside `-2147483648` through `2147483647` have no reliable specified result because the decimal conversion does not check overflow. `[May-improve]`: reject an out-of-range ID with a diagnostic. Physical input order need not be numeric order. Records are stored and looked up by numeric ID; a later occurrence replaces **both** sound and text for that ID, including replacement by empty fields. Loading into an existing list merges with it rather than clearing it first. `[Must-match]` The reference orders and looks up IDs by subtracting one signed 32-bit ID from another, so two IDs whose difference exceeds the signed 32-bit range can be misordered, duplicated, or not found. `[May-improve]`: compare IDs numerically without overflow.

End of file while seeking the next **ID** is successful completion, even for an empty file or a trailer containing only non-brace bytes. End of file while seeking a required sound/text field, or inside any field, is failure. Bad IDs and delimiter errors also fail. Earlier completed records remain loaded on failure: there is no rollback. `[Must-match]` Allocation failures can leave additional partial state and are not a transactional contract. `[May-improve]`: treat an allocation failure as a load failure that leaves every stored record complete and consistent, with no dangling or half-filled entry.

Each field has a 1024-byte destination including its terminating zero. The safe maximum is therefore **1023 retained payload bytes**, after LF removal. `[Must-match]` The recovered overflow check is too late: 1024 payload bytes write the terminator out of bounds; a further byte is also written out of bounds before an overlength error is reported. Do not treat 1024 as a safely accepted payload limit or require a clean reference failure for oversized fields. `[May-improve]`: reject any field with more than 1023 retained payload bytes as a load failure, without writing past the field storage.

An embedded zero byte is not an input delimiter or end-of-file marker. The scanner still consumes through the closing brace, but subsequent text storage and ID conversion see only the prefix before the first zero. `[Must-match]` Such files are not safely interchangeable with ordinary text tools and should not be authored.

### 3.6 `.MSG` Encoding and Display

The format is a stream of bytes with ASCII brace/decimal syntax, conventionally in a language-specific single-byte encoding. The loader neither identifies nor converts a code page. Bytes `0x80` through `0xFF` are preserved; display depends on the selected font's glyph mapping and the signed-indexing limitations in Section 3.1. `[Must-match]`

The local English, French, and Spanish corpus is consistent with Western Windows-1252 text, including punctuation in the `0x80`-`0x9F` range and accented bytes above `0xBF`. That is an asset observation, **not a format-wide encoding guarantee**. All 1950 observed `.MSG` files have no UTF-8 byte-order mark; many are not valid UTF-8. No Unicode or UTF-8 decoder, normalization, or multibyte character handling is present. UTF-8 input is not automatically rejected, but each encoded byte remains a separate display code. UTF-16 is not supported by this byte grammar. `[Must-match]`

A UTF-8 byte-order mark before the first opening brace happens to be skipped as ordinary outside-field bytes; it does not switch encoding. Inside a field, the same bytes are retained. `[Must-match]` Preserve raw bytes for interoperable storage, and choose any text-editor decoding explicitly.

Text-stream behavior also depends on storage: a loose file on the reference Windows runtime treats byte `0x1A` as end of file, whereas archive-backed byte reads do not implement that special marker. `[May-improve]`: treat `0x1A` as an ordinary byte for every storage backend. CRLF translation is performed in both normal cases; a bare CR is not a newline to the field scanner. `[Must-match]` Avoid `0x1A` for portable fixtures and assets.

## 4. Synthetic Test Fixtures (Hex / Text)

All fixtures are synthetic. Hex numbers in the left column are file offsets, not file contents. Each stated zero-fill range includes both endpoints and must be expanded into actual zero bytes; nothing else is omitted. These sparse listings specify complete font files without reproducing shipped assets.

### 4.1 Complete `.FON`: Row Boundaries, Padding, and Advances

File length: `0x0232` (562) bytes. There are 67 entries, indexed 0 through 66. Height is 2 and spacing is 1.

| File offset or range | Bytes |
| --- | --- |
| `0x0000` | `43 00 00 00 02 00 00 00 01 00 00 00 DE AD BE EF` |
| `0x0010` | `12 34 56 78` |
| `0x0014`-`0x021B` | Zero-fill, exactly 520 bytes: entries 0 through 64 |
| `0x021C` | `09 00 00 00 00 00 00 00`: entry 65, width 9, bitmap offset 0 |
| `0x0224` | `03 00 00 00 04 00 00 00`: entry 66, width 3, bitmap offset 4 |
| `0x022C` | `80 FF 7F 00 A0 40` |

Expected bitmap-data origin: 556. The last-entry formula yields `4 + 2 x 1 = 6` bitmap bytes. The nonzero opaque header words have no effect.

Using `#` for painted pixels and `.` for unchanged pixels, byte code `0x41` has these two nine-pixel rows:

```text
#.......#
.#######.
```

The seven low padding bits of its second byte on each row do not create extra pixels. Byte code `0x42` has these two three-pixel rows:

```text
#.#
.#.
```

For string bytes `41 42`, the proportional advances are 10 and 4, giving width 14, height 2, and a width-times-height estimate of 28 bytes. The maximum cell is 10, so the monospaced width query returns 20. With horizontal limit 13, only the first glyph is drawn. With limit 9, neither glyph is drawn, despite the first bitmap being only 9 pixels wide. A supplied byte `43` is outside the directory and is skipped without advancing.

Mutation checks:

- Removing the final bitmap byte makes the required six-byte read incomplete: loading fails.
- Appending arbitrary bytes leaves metrics and glyph pixels unchanged: those bytes are ignored.
- Changing the last entry's offset bytes at `0x0228` to `00 00 00 00` changes the loaded bitmap length to 2. The reference does not validate the first drawable glyph's four-byte extent; a safe reader rejects that inconsistent directory rather than rendering it. `[May-improve]` (Section 3.3)

### 4.2 Complete `.AAF`: Byte Pixels, Bottom Alignment, and Space

File length: `0x0814` (2068) bytes. Maximum height is 3, letter spacing is 1, space advance before letter spacing is 2, and extra line spacing is 1.

| File offset or range | Bytes |
| --- | --- |
| `0x0000` | `41 41 46 46 00 03 00 01 00 02 00 01` |
| `0x000C`-`0x010B` | Zero-fill, exactly 256 bytes: entries 0 through 31 |
| `0x010C` | `00 05 00 00 00 00 00 00`: space entry, width 5, height 0, offset 0 |
| `0x0114`-`0x0213` | Zero-fill, exactly 256 bytes: entries 33 through 64 |
| `0x0214` | `00 03 00 02 00 00 00 00`: entry 65, width 3, height 2, offset 0 |
| `0x021C` | `00 02 00 01 00 00 00 06`: entry 66, width 2, height 1, offset 6 |
| `0x0224`-`0x080B` | Zero-fill, exactly 1512 bytes: entries 67 through 255 |
| `0x080C` | `00 01 07 08 03 00 07 00` |

Expected coverage matrices, with each number representing one full byte/pixel:

```text
Byte 41, row 0:  0 1 7
Byte 41, row 1:  8 3 0
Byte 42, row 0:  7 0
```

Byte `41` begins at destination row 1; byte `42` begins at row 2. Both end at row 3, exclusive. Reported line height is 4. Space draws nothing because its height is zero, and its character-width query returns 2, **not** the stored bitmap width 5.

String bytes `41 20 42` have proportional advances 4, 3, and 3: total width 10 and width-times-height estimate 40. The legacy maximum-cell query returns 4 and monospaced width returns 12, even though the directory contains a width-5 entry. A limit of 3 draws no initial glyph; a limit of 4 permits the first glyph. Underline, when requested, is at row 2, not row 3.

For a fully specified synthetic palette subtest, assign each palette index I from 0 through 31 the six-bit RGB components `(2 x I, 2 x I, 2 x I)`. Map each RGB5 grayscale triple `(J,J,J)` back to index J, use the generated intensity table, select foreground index 28, and initialize the destination to index 0. Other palette entries are irrelevant to this subtest. Assuming same-coordinate destination sampling, rendering the first glyph yields these destination indices in its three-column maximum-height band:

```text
Row 0:  0  0  0
Row 1:  0  4 28
Row 2: 27 12  0
```

In particular, coverage 8 yields 27 through generated intensity index 127, while coverage 7 yields 28. This distinguishes legacy coverage from normalized 8-bit alpha.

Mutation checks:

- Changing the first four bytes to `46 46 41 41` fails the signature check.
- Ending the file at offset `0x0800`, exclusive, truncates the fixed directory and fails loading.
- Removing only the final pixel byte leaves a complete header and directory; the reference can load the shorter payload without noticing that entry 66 extends beyond it. A safe reader rejects that extent before drawing. `[May-improve]` (Section 3.3)
- Changing a coverage byte to `0E` selects an uninitialized blend row; changing it to `10` selects beyond the blend table. Neither has a defined color expectation. `[May-improve]`: a safe reader rejects both files at load (Section 3.2).

### 4.3 `.MSG`: Records, Concatenation, Replacement, and False Comments

The following is a complete ASCII fixture. Use either LF or CRLF physical line endings; the line break between `North` and `east` is an actual on-disk line ending.

```text
# Outside-field annotation without braces.
{10}{beep}{North
east}
{+002}{}{ keep  spaces }
{11}{old}{First}{11}{}{Last}
# {12}{}{Still parsed}
{13}{}{a{b}
```

Expected outcome: success with five numeric IDs:

| ID | Sound bytes as ASCII | Text bytes as ASCII |
| ---: | --- | --- |
| 2 | Empty | ` keep  spaces `, including the leading/trailing space and doubled internal space |
| 10 | `beep` | `Northeast` |
| 11 | Empty | `Last` |
| 12 | Empty | `Still parsed` |
| 13 | Empty | `a{b` |

ID 12 proves that a hash does not deactivate braces. ID 13 proves that an opening brace inside a field is literal rather than nested syntax.

### 4.4 `.MSG`: Exact Byte Fixtures

Each row is an independent complete input, loaded into an empty list. Hex byte strings contain no implicit line endings.

| Purpose | Complete input bytes, hexadecimal | Expected result |
| --- | --- | --- |
| CRLF, LF, and bare CR within one field | `7B 32 31 7D 7B 7D 7B 41 0D 0A 42 0A 43 0D 44 7D` | Success, ID 21, empty sound; text `41 42 43 0D 44` |
| Preserved high-bit bytes | `7B 32 32 7D 7B 7D 7B 43 61 66 E9 20 92 7D` | Success, ID 22; text `43 61 66 E9 20 92`, unchanged; its glyph interpretation is external |
| Embedded zero | `7B 32 33 7D 7B 7D 7B 41 00 42 7D` | Success, ID 23; returned text is only `41`, although scanning consumes through the final brace |
| Literal backslash and letter n | `7B 32 34 7D 7B 7D 7B 61 5C 6E 62 7D` | Success, ID 24; text `61 5C 6E 62`, not a newline |
| Multiline ID | `7B 31 0A 32 7D 7B 7D 7B 78 7D` | Success, ID 12; text `78` |
| Outside-field UTF-8 marker | `EF BB BF 7B 31 7D 7B 7D 7B 78 7D` | Success, ID 1; text `78`; no encoding switch |

### 4.5 `.MSG`: Acceptance and Failure Boundaries

Each input below is independent. Text enclosed in backticks is literal on-disk ASCII, without an extra newline unless stated.

| Input | Expected outcome |
| --- | --- |
| Zero bytes | Success, no records |
| `Outside prose without braces` | Success, no records |
| `{0}{}{}` | Success, ID 0 with empty sound and text |
| `{-7}{}{x}` | Success, ID -7 |
| `{+}{}{x}` or `{-}{}{x}` | Success, ID 0: legacy sign-only exception |
| `{01}{}{x}` | Success, ID 1, decimal rather than octal |
| `{}{}{x}` | Failure: empty ID |
| `{ 1}{}{x}` or `{1 }{}{x}` | Failure: ID whitespace is retained, not trimmed |
| `{0x10}{}{x}` | Failure: hexadecimal ID notation is not accepted |
| `{1}{}` | Failure: missing required text field |
| `{1}{}{abc` | Failure: unterminated text field |
| `# }` | Failure: unmatched closing brace outside a field |
| `{1}{}{ok}}` | Failure after inserting ID 1; that earlier record remains |
| `{1}{}{ok}{` | Failure: unterminated next ID; ID 1 remains |
| `{1}{}{ok} trailing annotation` | Success: trailer has no active braces |

For length boundaries, form the exact prefix `{30}{}{`, then the specified number of copies of byte `41`, then the exact suffix `}`. At 1023 copies, the text field fits and loading succeeds, absent resource failure. At 1024 copies, the reference writes past the field buffer; there is no safe acceptance/rejection expectation. At 1025 copies, the overflow diagnostic is reached only after out-of-bounds writes. A safe reader rejects both oversized variants without memory corruption. `[May-improve]` (Section 3.5) Copies are byte counts, not Unicode-character counts.

For the storage-specific end-of-file case, use bytes `1A 7B 31 7D 7B 7D 7B 78 7D`. Archive-backed input preserves `1A` as ignored outside-field data and loads ID 1; the reference Windows loose-file text stream stops at the initial `1A` and finishes successfully with no records. `[May-improve]`: a safe reader loads ID 1 from both storage kinds (Section 3.6).

## 5. Clean-Room Verification Checklist
- [x] Zero C/C++ code snippets or struct definitions included
- [x] All multi-byte integer endianness explicitly stated
- [x] Synthetic test vectors included
- [x] Every documented quirk tagged `[Must-match]` or `[May-improve]`
- [x] No internal reference symbol names; only public SSL opcode names and file-format key literals
- [x] Spec linter run against `fallout1-re` symbols with no findings
