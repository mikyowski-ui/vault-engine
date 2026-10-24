# SPEC-008: Archives, Save Slots, World Travel, and Pip-Boy State

## Clean-Room Rules for Research Agents
- Document ONLY binary file layouts (byte offsets, endianness, data types), mathematical formulas, state transitions, and observable behavior.
- DO NOT copy C/C++ code, struct definitions, function names, or internal variable names from the reference implementation.
- Provide small synthetic hex/text test vectors at the bottom of the specification.

## 1. Overview & Scope

This specification describes Fallout 1 archive indexing and LZSS, selected-database file lookup, save-slot framing and stream order, world/town-map state, Pip-Boy status/rest, and automap storage. Evidence is static inspection of reference revision R1, targeting 32-bit Windows. No original archive or complete save-slot snapshot was used for byte-for-byte validation; fixtures are synthetic and source-derived behavior is not an original-executable runtime test.

Three distinctions are essential:

- Archive metadata starts at **byte zero**. This is not the later archive format with an EOF directory trailer and a different compression codec. `[Must-match]`
- File lookup uses a **selected database**, normally the master database, with explicit switching for critter art. There is no universal loose/master/critter fallback chain. `[Must-match]`
- This revision does **not** load `data/holodisk.txt` or `data/quests.txt`. Pip-Boy associations and quest thresholds are fixed tables, with localized text supplied by message files. No unsupported text-file syntax is invented here. `[Must-match]`

The template's repository-name reference is replaced with generic wording to satisfy this document's identifier restriction. All names below are public resource names, human-readable field descriptions, or mathematical notation. The complete object/map contracts are shared with SPEC-003, and character-stat/clock/check contracts with SPEC-006.

**Compatibility tags.** Every documented quirk carries one tag:

- `[Must-match]`: gameplay-visible behavior, data grammar, formula, ordering, or random-draw sequence. A clean-room engine must reproduce it exactly.
- `[May-improve]`: reference memory-safety defects, undefined behavior, uninitialized reads, stale state, presentation-only transients, or a lookup that reads the wrong table in an obviously unintended way. A clean-room engine may apply the stated safe rule and should keep every other observable behavior. An engine that wants bit-exact parity may offer a compatibility switch that keeps the reference behavior.

Byte-layout tables and catalogs are tagged once, in the sentence that introduces or follows them.

## 2. Binary Layout / Data Format (Byte Offset Tables)

### 2.1 Numeric and String Conventions

Unless explicitly identified as a raw byte array, every multi-byte field described here is **big-endian**. Signed integer fields use two's-complement representation. Counts, offsets, and sizes must be nonnegative for valid ordinary files; flag words are interpreted as bitfields. Saved floating-point options use IEEE-754 binary32, with their four payload bytes big-endian. `[Must-match]`

Archive names use one unsigned byte of byte length followed by exactly that many name bytes, without an on-disk terminator. Save manifest names instead are NUL-terminated byte strings. Fixed save-header strings occupy their full stated width; maximum-width strings need not contain a NUL. None of these formats declares a Unicode encoding. `[Must-match]`

### 2.2 `.DAT` Root Directory and Per-Directory Tables

`MASTER.DAT` and `CRITTER.DAT` use the same format. The leading root directory and every per-directory file table have this 16-byte prefix:

| Table-relative offset | Bytes | Meaning |
| --- | ---: | --- |
| `+0x00` | 4 | Signed 32-bit, big-endian: number of used entries N |
| `+0x04` | 4 | Signed 32-bit, big-endian: stored allocation capacity |
| `+0x08` | 4 | Signed 32-bit, big-endian: per-entry associated-data byte count D |
| `+0x0C` | 4 | Opaque legacy word; not a usable file offset |
| `+0x10` | Variable | N length-prefixed keys, each followed by its D associated bytes |

The root prefix is at file offset zero. Root keys are directory names, usually beginning with the root entry `.`. A normal archive has root associated-data count **zero**, so its keys are contiguous length-prefixed names. Capacity can exceed count without adding unused entries to the serialized stream. If root D is nonzero, the generic reader consumes D raw associated bytes after each directory name; they do not point to the subsequent file tables. `[Must-match]`

After the last root key, store **one complete file table per root directory, in the same directory order**. There is no per-directory offset array and no recursive on-disk tree traversal: a directory key can itself contain a backslash-delimited relative path. Ordinary lookup relies on the name lists' case-insensitive sorted ordering. `[Must-match]`

A file table normally has D equal to 16. Each entry is:

| Entry-relative position | Bytes | Meaning |
| --- | ---: | --- |
| `+0` | 1 | Unsigned filename byte length L |
| `+1` | L | Filename bytes, relative to this directory |
| `+1 + L` | 4 | Unsigned 32-bit, big-endian: attributes |
| `+5 + L` | 4 | 32-bit big-endian: absolute payload offset from archive byte zero |
| `+9 + L` | 4 | 32-bit big-endian: unpacked/logical byte length |
| `+13 + L` | 4 | 32-bit big-endian: packed/stored byte length |

There is no filename NUL, four-byte alignment, CRC, required footer, or signature preceding the root count. Metadata lengths and absolute file offsets determine where data resides; payloads need not be interleaved with their table entries. `[Must-match]`

| Attribute value/mode | Payload interpretation |
| --- | --- |
| `0x20` | Uncompressed bytes; read the logical length directly from the absolute offset |
| `0x40` | Sequence of two-byte block headers and raw/LZSS block bodies |
| `0x10` | Legacy single LZSS stream; packed length bounds compressed input, without the outer block headers |
| `0x00` | Treated as legacy `0x10` by the reader |

Exact attribute zero is first normalized to `0x10`; otherwise dispatch selects attribute bits `0xF0`. Low bits do not change an already selected nonzero mode. `[Must-match]` A low-bits-only nonzero word is not the exact-zero legacy alias: the streaming open fails, but the whole-file read path reports success without filling the caller's buffer. `[May-improve]`: reject such an entry with a diagnostic on every path. The raw `0x20` path does not consult the packed-length word. `[Must-match]` In `0x40`, the stored length describes the packed extent including block headers, but the reader's principal termination criterion is the declared logical length rather than comprehensive packed-extent validation. `[May-improve]`: also bound the block walk by the packed length and reject an entry whose blocks overrun it.

### 2.3 `.DAT` Block Header

At each `0x40` block boundary:

| Block-relative offset | Bytes | Meaning |
| --- | ---: | --- |
| `+0` | 2 | Unsigned 16-bit, big-endian control/length word H |
| `+2` | H modulo 32768 | Block body |

If bit `0x8000` is set, copy the body literally. If clear, decode that many **compressed input bytes** with the LZSS rules in Section 3.1. The flag does not describe an LZSS match/literal token: it selects the entire block's raw-versus-compressed treatment. `[Must-match]`

The buffered archive stream uses 16384-byte logical blocks. Every non-final block must produce 16384 bytes; a final block may be shorter, limited by the entry's remaining logical length. There is no expanded-size word inside a block. `[Must-match]` A short non-final body can expose uninitialized buffered bytes in the reference and is not a valid portable streaming layout. `[May-improve]`: reject a non-final block that expands to fewer than 16384 bytes.

**Final output length.** `[Must-match]` Both compressed modes expose exactly the entry's logical length L, never the decoder's output count. In `0x40`, the final block is decoded in full and only the bytes that still fall within L are visible. A legacy `0x10` stream behaves the same way: it is decoded in one pass into an L-byte buffer, and only those L bytes are visible. So in both modes, output past L is cut off, not reported. Neither path bounds the decoder itself: output past the 16384-byte block buffer or past L is written beyond the buffer, and a `0x10` stream that ends before L leaves the rest of its buffer unset. `[May-improve]`: in both modes, stop writing output at L bytes (16384 for a non-final block) and discard the rest, which gives the same visible bytes. Reject an entry whose output ends before L. All 20456 entries of the corpus `MASTER.DAT` (14997 `0x40`, 4787 `0x20`) and `CRITTER.DAT` (5459 `0x40`) decode to exactly their logical length, with full non-final blocks and block walks that end exactly at the packed length. Neither archive has a `0x10` or `0x00` entry.

### 2.4 Save Slot Structure and `SAVE.DAT` Header

Slots are numbered **01 through 10**, under the writable master patch root. `[Must-match]`

```text
SAVEGAME/SLOT01/SAVE.DAT
SAVEGAME/SLOT01/<map-name>.SAV
SAVEGAME/SLOT01/AUTOMAP.SAV
```

Each slot holds the central file plus the copied saved-map set and a renamed copy of `MAPS/AUTOMAP.DB`. Temporary backup names use `.BAK`, including `SAVE.BAK`; these are not additional data substreams. A valid game restore requires the companions, not just the central file. `[Must-match]`

| `SAVE.DAT` offset | Bytes | Meaning |
| --- | ---: | --- |
| `0x0000` | 24 | ASCII `FALLOUT SAVE FILE` followed by seven zero bytes in the normal writer |
| `0x0018` | 2 | Version major, unsigned 16-bit big-endian; writer emits 1 |
| `0x001A` | 2 | Version minor, unsigned 16-bit big-endian; writer emits 1 |
| `0x001C` | 1 | Release byte; writer emits `0x52`, ASCII `R` |
| `0x001D` | 32 | Character name |
| `0x003D` | 30 | User save description |
| `0x005B` | 2 | Host-calendar day, 16-bit big-endian |
| `0x005D` | 2 | Host-calendar month, 16-bit big-endian |
| `0x005F` | 2 | Host-calendar year, 16-bit big-endian |
| `0x0061` | 4 | Host-time word, 32-bit big-endian; recovered writer uses hour **plus** minute |
| `0x0065` | 2 | Game-calendar month, 16-bit big-endian |
| `0x0067` | 2 | Game-calendar day, 16-bit big-endian |
| `0x0069` | 2 | Game-calendar year, 16-bit big-endian |
| `0x006B` | 4 | Game time in ten ticks per second, 32-bit big-endian |
| `0x006F` | 2 | Current elevation, 16-bit big-endian |
| `0x0071` | 2 | Current map number, 16-bit big-endian |
| `0x0073` | 16 | Current map's working `.SAV` filename: its stem and a lower-case `.sav`, NUL-padded (Section 3.10) |
| `0x0083` | 29792 | Thumbnail: 224 x 133 one-byte palette indices, row-major, top to bottom |
| `0x74E3` | 128 | Reserved bytes, zeroed by the writer and skipped by the reader |
| `0x7563` | Variable | First game-state payload; total header is 30051 bytes |

The thumbnail has neither an embedded palette nor per-row padding/compression. It is scaled from the 640-by-380 game-view image, excluding the lower interface. Its pixels remain palette indices, not RGB triples or a separate bitmap-file header. `[Must-match]`

The reader compares only the first 18 signature bytes: the seventeen text characters plus their terminating zero. The remaining six signature-padding bytes are not checked. `[Must-match]` It reads the version but does **not** enforce a version-equality check in this revision. Acceptance of another version's header is not proof that its following payload schema is compatible. `[May-improve]`: reject, or warn before loading, a header whose version is not 1.1.

Host-date loading labels its first two words in the reverse month/day roles from the writer; the raw order above is the actual write order. This revision never displays the host date, so the swap has no visible effect. `[May-improve]`: read the words in writer order (day, month, year). The host-time word is not HHMM, seconds since midnight, or a Unix timestamp. `[May-improve]`: treat the host-time word as opaque on read; a writer stores the hour-plus-minute sum only for byte parity. Game date/time uses a different, correctly separate set of fields. `[Must-match]`

### 2.5 Ordered `SAVE.DAT` Payloads

The following 27 handler positions are serialized in exactly this order. Numbers are **order indices**, not on-disk tags. There are no section names, per-handler size prefixes, alignment gaps, or general chunk directory. After a variable-length component, the next component begins immediately at its parsed end. `[Must-match]`

Let G be the campaign-global count obtained from the campaign initializer, M the map-manifest count, P the party count, and C the saved combat-list count. G is **not stored before either campaign array**; changing the installed campaign count changes how the save stream is parsed. `[Must-match]`

| Order | Component | Serialized bytes/contract |
| ---: | --- | --- |
| 0 | Preparation | No bytes; loading resets the game and installs the header's game time |
| 1 | Player combat identity | One signed 32-bit big-endian combat identifier |
| 2 | Campaign snapshot A | G signed 32-bit big-endian values, then one water-warning/movie-status byte |
| 3 | Companion-map manifest/copy phase | Signed 32-bit M; M NUL-terminated map filenames; signed 32-bit automap companion byte length |
| 4 | Campaign snapshot B | Same `4 x G + 1` layout; load consumes and discards this second snapshot |
| 5 | Player object/inventory | Recursive player object tree using the SPEC-003 object grammar, then one 32-bit viewport-center hex |
| 6 | Player critter statistics | One 32-bit sneak-working value, then the 368-byte critter-stat prefix from SPEC-006; total 372 |
| 7 | Kill counters | Fifteen signed 32-bit words, total 60 |
| 8 | Tagged skills | Four signed 32-bit skill indices, total 16 |
| 9 | Random generator | No bytes; generator state is not restored here |
| 10 | Perk ranks | Sixty-four signed 32-bit rank words, total 256 |
| 11 | Combat state | Four bytes when inactive; otherwise `32 + 4 x C`, detailed below |
| 12 | Combat AI | No bytes |
| 13 | Player progression | Five signed 32-bit words: unspent skill points, level, experience, reputation, karma; total 20 |
| 14 | Item subsystem | No bytes |
| 15 | Timed queue | Count and type-dependent records, detailed in Section 2.6 |
| 16 | Traits | Two signed 32-bit trait indices, total 8 |
| 17 | Automap display options | One 32-bit flag word; not the automap database contents |
| 18 | Preferences | Twenty four-byte words, total 80; integer/float positions below |
| 19 | Character-editor advancement | One 32-bit last-reconciled level, then one pending-perk byte; total 5 |
| 20 | World-map state | 1028 bytes, detailed in Section 2.7 |
| 21 | Pip-Boy subsystem | No bytes; status/holodisks derive from campaign state |
| 22 | Played-movie flags | Fourteen raw bytes |
| 23 | Skill-use timestamps | Eighteen skills x three signed 32-bit game-tick timestamps, skill-major; total 216 |
| 24 | Party membership | 32-bit P, 32-bit next-item identity counter, then `max(P - 1, 0)` 32-bit object identifiers |
| 25 | Interface state | Four 32-bit values: enabled, hidden, selected hand/item, end-turn panel open; total 16 |
| 26 | Finalization | No bytes; restore name/presentation/music and request combat continuation when appropriate |

The manifest must contain at least one saved map for a normal save. Map filenames include their terminators but no length bytes. The automap byte length follows all filenames, not each filename. The copy phase transfers the listed map files and `AUTOMAP.SAV` between slot and working `MAPS/` locations; automap restore verifies the copied file's byte length against the recorded length. This is not inline embedding of map or automap pixels in `SAVE.DAT`. `[Must-match]`

Ordinary 8.3 map names fit the manifest reader. `[Must-match]` Its recovered string limit is irregular: a terminator on consumed byte 14 reports failure, while reaching byte 15 ends reading without a robust terminator guarantee. On that failure the reference stops copying further maps but continues the step, reading the automap length from wherever the stream then stands. `[May-improve]`: accept at most twelve filename bytes plus NUL per name and reject the save otherwise. A safe authoring profile uses at most twelve filename bytes plus NUL; a safe parser must bound and verify every name rather than copying this failure-prone limit.

The player object starts directly with its object record, without the map object's overall/per-elevation count prefixes. A critter's fixed record is 128 bytes before children; each inventory entry is a 32-bit quantity followed by a complete child tree, depth-first, with subtype sizes determined by prototypes. While writing this payload, only the player's own top-level record is altered: it is written with basic flag `0x00000004` (temporary) clear and with its record word at `+0x40` (script instance identifier, SPEC-003 §2.10) set to -1. Its word at `+0x44` (cached script-file index, the retained association word) keeps its live value, normally 0 for the player. Inventory children in the tree are written unchanged, with their own `+0x40` and `+0x44` words. After writing, the player's live script instance identifier is restored and the player is marked temporary again. On load, the stored -1 means no script is bound from the record; a fresh player script instance is installed instead. Area-map `.MAP`/`.SAV` writing never substitutes -1 for either word; the player is absent from those streams because it is temporary. `[Must-match]`

Combat begins with its 32-bit state word. If active bit `0x01` is clear, that is the whole component. Otherwise follow with seven 32-bit words: turn-running state, free movement, combat experience, combatant count, noncombatant count, total list count C, and player combat identity; then C combat identities in list order. These are not unique object identifiers. `[Must-match]`

Preference word positions 0-19 are: game difficulty, combat difficulty, violence level, target highlighting, burning-running animation preference, combat-message preference, combat taunts, language filter, running preference, subtitles, item highlighting, combat speed, player speed-up, **binary32 text delay**, master volume, music volume, effects volume, speech volume, **binary32 brightness**, and **binary32 mouse sensitivity**. All non-float words are 32-bit integers. `[Must-match]`

### 2.6 Timed-Queue Payload Grammar

The queue starts with a signed 32-bit big-endian count. Each record then contains:

| Record-relative offset | Bytes | Meaning |
| --- | ---: | --- |
| `+0` | 4 | Due game tick, 32-bit big-endian |
| `+4` | 4 | Event type, signed 32-bit big-endian, normally 0-12 |
| `+8` | 4 | Unique owner-object identifier, or -2 for no owner |
| `+12` | Variable | Type-specific payload; no payload-length word |

| Event type | Payload after the 12-byte prefix |
| ---: | --- |
| 0, drug effect | Three 32-bit stat indices, then three 32-bit modifiers; 24 bytes |
| 2, withdrawal | Three 32-bit values: start/end indication, item prototype identifier, perk index; 12 bytes |
| 3, script timer | 32-bit script instance identifier and 32-bit fixed parameter; 8 bytes |
| 6, radiation | 32-bit radiation level and 32-bit healing indication; 8 bytes |
| 1, 4, 5, 7, 8, 9, 10, 11, 12 | No additional bytes |

The zero-payload types are knockout, midnight/game-time, poison, flare, explosion, item drain, sneak, failed/premature explosion, and periodic map update, respectively. All payload words are big-endian. Owner references are resolved through map objects and inventories; these are not serialized native addresses. An identifier that matches no object leaves the event without an owner. No RNG seed follows the queue. `[Must-match]` The reference uses the stored event type as a table index without a range check. `[May-improve]`: reject a record whose type is outside 0-12.

### 2.7 World-Map Save Component

Offsets in this table are relative to payload order 20, not the start of `SAVE.DAT`.

| Component-relative offset | Bytes | Meaning |
| --- | ---: | --- |
| `0x000` | 899 | Raw discovery bytes: 31 rows x 29 columns, row-major |
| `0x383` | 105 | Raw town-entrance knowledge bytes: 15 towns x 7 slots, town-major |
| `0x3EC` | 4 | Known/visited-town bitmask, 32-bit big-endian |
| `0x3F0` | 4 | Six special-encounter consumed bits, 32-bit big-endian |
| `0x3F4` | 4 | Current town, signed 32-bit big-endian |
| `0x3F8` | 4 | Current town-section slot, signed 32-bit big-endian |
| `0x3FC` | 4 | World X pixel coordinate, 32-bit big-endian |
| `0x400` | 4 | World Y pixel coordinate, 32-bit big-endian |
| `0x404` | 0 | End of the 1028-byte component |

The playable lookup grid is 30 rows x 28 columns of 50-by-50 world pixels, but its serialized discovery array has the larger **29-byte stride and 31 rows**. Do not repack it into 840 bytes. `[Must-match]` Discovery values normally mean 0 unknown/black, 1 seen/dim, and 2 visited/fully revealed. `[Must-match]`

### 2.8 Automap Database and Raster

The working file is `MAPS/AUTOMAP.DB`; its slot copy is `AUTOMAP.SAV`. This is an indexed automap database, **not** an area-map `.SAV` despite the slot filename. `[Must-match]`

| Database offset | Bytes | Meaning |
| --- | ---: | --- |
| `0x000` | 1 | Version; exactly 1 is accepted |
| `0x001` | 4 | Total database byte length, 32-bit big-endian |
| `0x005` | 792 | 66 map numbers x 3 elevations of signed 32-bit big-endian absolute offsets |
| `0x31D` | Variable | Entries; initial empty database length is 797 bytes |

The offset word for map M and elevation E is at `5 + 4 x (3 x M + E)`. Zero means recordable but not yet captured; -1 means excluded; positive values point to an entry: `[Must-match]`

| Entry-relative offset | Bytes | Meaning |
| --- | ---: | --- |
| `+0` | 4 | Stored payload byte length, 32-bit big-endian |
| `+4` | 1 | Compression byte: 1 uses LZSS, other values take the raw path |
| `+5` | Variable | Payload |

The expanded raster is exactly **10000 bytes**, holding 200 x 200 two-bit cells. Its LZSS token format matches Section 3.1 but has **no outer two-byte DAT block header** and terminates on 10000 output bytes. `[Must-match]` The decoder bounds final match output to that count rather than reading a known compressed-input length as the archive decoder does. `[May-improve]`: also bound input reads by the stored payload length and reject an entry that would read past it. Its separately allocated dictionary still fills only the first 4078 positions with spaces; the remaining positions are uninitialized, not guaranteed first-use zeros. `[May-improve]`: initialize the remaining eighteen positions to zero. A raw entry normally stores 10000 bytes. `[Must-match]`

Raster extraction first updates seen-object bookkeeping. Only objects with seen flag `0x40000000` contribute: walls encode 1; scenery other than prototype `0x02000158` encodes 2; other kinds encode 0/no contribution. `[Must-match]` For source hex T, the recovered writer uses:

`X = 200 - (T modulo 200)`

`byte offset = floor(X / 4) + 50 x floor(T / 200)`

`bit shift = 2 x (3 - (X modulo 4))`

Clear those two bits and insert the cell value. This is intentionally **200 minus column**, not 199 minus column: column zero spills into the following raster row, and the last row's column-zero write falls outside the serialized 10000-byte payload. The cell layout is shared with automap databases written by the original engine, so it is a data grammar. `[Must-match]` Multiple contributing objects can overwrite the same cell in object traversal order. `[Must-match]`

Display consumes four cells per byte, most-significant pair first. Values 1 and 2 draw wall/scenery colors; zero and the otherwise-unused value 3 do not draw. The Pip-Boy view gives each cell two pixels horizontally and a two-row vertical pitch: only the first of the two rows is painted, and the second is left untouched, so a cell paints a 2-by-1 pixel run (SPEC-014 §3.33). `[Must-match]` Its 400-by-400 output starting at screen Y 105 can exceed a 480-row window for nonempty lower rows; this is a reference bounds hazard, not a different raster size. `[May-improve]`: clip drawing to the window.

The initial excluded map-number set is `0,1,2,19,20,21,22,23,49,50,51,52,53,54,56,57,58,59,60,61,62,63,64,65`, on all elevations; other maps start at offset zero. `[Must-match]` Capture can append a new entry or rewrite an existing one through a temporary file. Replacing an entry shifts every later positive offset and the database length by the payload-length difference. Appending requires actual EOF to match the recorded database length. `[Must-match]`

Only the four-byte display-flags word appears at save payload order 17: masks `0x01`, `0x02`, and `0x04` denote in-game rendering, high detail, and scanner use. The database itself is copied separately, and captures occur during appropriate map-save/script transitions. `[Must-match]`

### 2.9 Area-Map `.SAV` Files

An area-map `.SAV` is the same uncompressed version-19 map stream as `.MAP`, with saved-state bit `0x00000001` set. Its suffix does not imply archive block compression. `[Must-match]` Its 236-byte header and nested script/object schemas are the SPEC-003 map contract:

| Header offset | Bytes | Meaning |
| --- | ---: | --- |
| `0x00` | 4 | Version 19, signed 32-bit big-endian |
| `0x04` | 16 | Map resource name |
| `0x14` | 12 | Three 32-bit big-endian entry values: hex, elevation, orientation |
| `0x20` | 4 | Local-variable count |
| `0x24` | 4 | One-based map-script reference, or nonpositive for none |
| `0x28` | 4 | Flags: saved bit 1; tile-block omission bits 2, 4, 8 for elevations 0, 1, 2 |
| `0x2C` | 4 | Stored darkness word |
| `0x30` | 4 | Map-global variable count |
| `0x34` | 4 | Map-number association |
| `0x38` | 4 | Last-visit game tick |
| `0x3C` | 176 | Forty-four retained 32-bit words |
| `0xEC` | Variable | Map payload |

All numeric words are big-endian. Payload order is map-global values, script-local values, one 40000-byte tile block per non-omitted elevation, five script-category streams, then the overall top-level object count and three elevation object streams. The header lists local count before global count, but payload arrays are **globals first**. `[Must-match]`

Objects, nested inventory, script-local state, and map flags persist here; the working map set is not reconstructed solely from `SAVE.DAT`. The player is separately saved in central payload order 5. Complete restoration therefore depends on both the map files and the prototype/compiled-script resources referenced by their records. `[Must-match]`

## 3. Behavioral Rules & Edge Cases

### 3.1 Exact DAT LZSS Token Rules

Decode each compressed block with a fresh logical ring of **4096 bytes** and initial write cursor **4078**, hexadecimal `0xFEE`. The reference fills positions 0-4077 with byte `0x20`, ASCII space. It does **not** freshly fill the final eighteen positions: the archive decoder's retained storage can contain zeros on first use or prior decode bytes later. `[May-improve]`: set the final eighteen positions to zero at the start of every compressed block, which matches the reference's first-use state. Well-formed streams must not depend on an unwritten tail byte; filling the entire ring with spaces would change that edge case.

Consume a control byte and examine its bits **least significant first**, for up to eight tokens. For each bit:

- A set bit consumes one literal byte, emits it, writes it at the current ring cursor, and advances that cursor modulo 4096. `[Must-match]`
- A clear bit consumes two bytes, first A then B. The absolute ring start is `A + 256 x floor(B / 16)`. The match length is `(B modulo 16) + 3`, from 3 through 18. `[Must-match]`
- For match position K, read ring position `(start + K) modulo 4096`, emit it, and immediately write it at the advancing ring cursor. Copies are sequential and may overlap newly written output. `[Must-match]`

The match start is an **absolute 12-bit ring index**, not a backwards distance and not a big-endian 16-bit offset. Only the outer block header is a two-byte big-endian scalar. Refill control bytes after eight tokens. Stop when the declared packed input length is exhausted; unused remaining control bits are ignored. There is no embedded end token, output-length field, XOR transform, or zlib/DEFLATE wrapper. `[Must-match]`

The dictionary resets for each compressed block. A raw outer block copies bytes directly and does not seed a dictionary for a subsequent block. `[Must-match]` Archive buffering uses logical 16384-byte boundaries even though compressed body lengths vary. Packed bytes and total expanded bytes must both be bounded by an independent reader.

The reference does not comprehensively protect against truncated tokens, over-expansion, packed-offset overflow, zero-output blocks, or corrupt lengths. A zero-length/nonprogressing block can cause an endless whole-file decode loop. `[May-improve]`: reject those cases before exposing data, enforce every compressed block's output bound, require full non-final logical blocks, and apply the final-output rule of Section 2.3 to the entry's logical length.

The recovered callback-assisted bulk-extraction path also fails to advance its destination after a compressed block, unlike its raw-block path. It can overwrite output or fail to terminate even for otherwise well-formed compressed input. `[May-improve]`: advance the destination by each compressed block's output, as the raw-block path does. The block algorithm and fixtures specify the codec/streaming contract, not a claim that this defective helper was runtime-verified or silently repaired.

### 3.2 Virtual Filesystem Precedence

The application opens separate master and critter database handles, each with its own optional loose/patch root. The master database is selected after initialization. Opening a normal resource then follows:

1. Consult the **selected database's** loose-file root first, when configured and not excluded by its optional path hash. `[Must-match]`
2. If the loose open fails, search that selected database's archive directory/file tables. `[Must-match]`
3. If absent there, fail. Do not automatically search the other archive. `[Must-match]`

Critter-art operations explicitly select the critter database for that request and restore the previous selection afterward. Thus a critter request normally sees critter loose files before `CRITTER.DAT`; an ordinary master request sees master loose files before `MASTER.DAT`. A same-named member in the unselected archive is not a general fallback. `[Must-match]`

Archive lookup uppercases the path and uses case-insensitive key comparisons. It splits at the final backslash into a directory key and filename key, stripping a leading `.` or `.\` convention. A request without a directory selects directory-table index zero, normally the `.` entry. Forward-slash normalization is not a general archive lookup guarantee. `[Must-match]`

Writes and update opens target loose files, never mutate an archive entry. A leading `@` can bypass the normal root-prefix construction but does not create a multi-archive search. `[Must-match]` An enabled, stale loose-path hash can hide a file added externally until the hash is refreshed or updated through the file layer. `[May-improve]`: consult the loose directory directly, or invalidate the hash when files change. Binary opens expose exact decoded bytes; text-mode newline/EOF translation is a separate layer. `[Must-match]`

### 3.3 Save/Restore State Transitions and Boundaries

Saving writes the header and all 27 ordered payloads while producing the companion map/automap copies. Existing central and companion files are renamed to `.BAK` as a best-effort backup and restored on some failures; successful saving removes backups. `[Must-match]` This is not a single atomic multi-file transaction. `[May-improve]`: write to temporary names and commit the slot only after every file succeeds.

Loading reads the header, resets current game state, installs the saved game tick, reads campaign snapshot A, restores working companion files, and loads the current area map **before** later central player/queue/world-map components. Snapshot B is consumed but ignored. Finalization restores the header's player name and presentation and requests combat if the saved combat state was active. `[Must-match]`

The map-copy phase clears the working saved-map set, replaces the working automap database, and verifies its recorded size. A missing/truncated companion or a later payload error can therefore leave partially changed world/filesystem state before the outer load resets again. `[May-improve]`: validate the central stream and companions before changing any working file. Do not treat malformed-save loading as a harmless header inspection operation.

Most payloads have no independent lengths. Parsing must know campaign count, fixed catalog sizes, active combat state, map filename termination, recursive object/prototype subtypes, and timed-event payload kinds. Random state and Pip-Boy UI state have zero-byte slots; inserting placeholder words for them desynchronizes everything that follows. `[Must-match]`

Header inspection only checks its fixed readable fields and signature, then seeks over thumbnail/reserved bytes. For loose files, seeking can move beyond EOF without proving those bytes exist. `[May-improve]`: a safe parser must separately require the full 30051-byte header and all subsequent payload/companion extents. The reference's acceptance of an unexpected version or a malformed count is not a defined interoperability guarantee.

### 3.4 World Coordinates, Movement, and Travel Time

The world artwork is **1400 x 1500 pixels**, divided into 28 columns x 30 rows of **50 x 50** cells. Increasing X moves right; increasing Y moves down. Cell column/row are `floor(X / 50)` and `floor(Y / 50)`. The world viewport is 450 x 442 at screen (22,21), with scroll origins clamped to X 0-950 and Y 0-1058. `[Must-match]`

Terrain codes are 0 desert, 1 mountain, 2 city, 3 coast. Terrain, encounter-frequency class, and encounter-region number are three independent fixed cell tables in Section 3.6. They are not external configuration-file records. `[Must-match]`

The initial player position is (825,75), the center of Vault 13's cell (16,1). Initial town is 0 and current section is 1. The town-enabled mask initially includes town zero, and only its entrance slot zero starts known. Active travel destinations, interpolation counters, and the encounter-interval counter are not part of the saved world component. `[Must-match]`

Movement keeps the absolute destination differences A and B, direction signs, a zero initial error accumulator, and a zero step count. Each actual movement step increments the count:

- If A is at most B, finish when count exceeds B. Otherwise add A to error; if error is positive, subtract B and move X one signed pixel; always move Y one signed pixel. `[Must-match]`
- If A exceeds B, finish when count exceeds A. Otherwise add B to error; if error **exceeds A**, subtract A and move Y one signed pixel; always move X one signed pixel. `[Must-match]`

The asymmetric strict comparison is intentional reference behavior: a non-horizontal X-major path can finish one Y pixel short of the mathematical target. Do not replace it with an assumed symmetric line rasterizer. `[Must-match]`

For travel speed, let S be displayed Outdoorsman capped **only above at 100**. Compute W and the division with binary32 intermediates; the Pathfinder factor is applied in double precision, which gives the same results for ranks 0-4 because every product is exact:

`W = trunc(60 + 60 x S / 100)`

`Q = trunc(864000 / W)`

`Q = trunc(Q x (1 - 0.25 x Pathfinder ranks))`

W is the number of **movement-loop iterations** between encounter/healing checks. Q is game ticks added on a time-charging iteration. There are ten ticks per second. Typical S values 0, 50, and 100 give W 60, 90, and 120, and pre-Pathfinder Q 14400, 9600, and 7200. `[Must-match]` Malformed negative skills/ranks can make unsigned conversion or division unsafe; these are not normalized into an invented speed range. `[May-improve]`: clamp S below at 0 and Pathfinder ranks to 0-4, so W is at least 60 and Q at least 0.

| Terrain | Interactive movement schedule |
| --- | --- |
| Desert/coast | One pixel-step attempt and Q ticks every iteration; reset the terrain counter |
| Mountain | Decrement the terrain counter; step when it reaches zero, reset it to 2; charge Q every iteration, including the nonmoving one |
| City | Step every iteration; decrement the counter, and when it expires while movement remains active, take an additional step, reset it to 4, and skip that iteration's Q charge |

In a steady city segment, four iterations therefore yield five steps and three time charges, apart from segment boundaries/completion. The counter is shared across terrain transitions. `[Must-match]` A rendering/input cycle normally attempts at most two such iterations and is capped around 24 updates/second; wall-clock speed is not a fixed game-minute-per-frame contract. `[Must-match]`

After a proposed step, when X is below 1064 and Y is positive, a fixed 1500-row, 133-byte-per-row walking mask tests byte `floor(X / 8)` with bit weight `2^(7 - (X modulo 8))`; a set bit blocks travel, restores the prior coordinates, and stops movement. Row Y 0 and X 1064 or more are never tested. It is not the fog-of-war mask. The travel-loop iteration may still have charged time. `[Must-match]`

When a time charge would reach/past the next queued event, temporarily advance to that event tick plus one and process the queue. A nonzero event handler stops world travel and returns to a town or terrain map. Otherwise the loop installs its computed new time. Queue processing and campaign events can interrupt travel independently of encounter dice. `[Must-match]`

Each W iterations, reset that interval counter and apply 24 hours of party resting healing, then perform encounter tests. This happens by loop count, **not by measured elapsed game time**: city travel and Pathfinder can produce a 24-hour healing application after fewer than 24 actual game hours. `[Must-match]`

Script-directed town travel uses a separate path. It runs after every map load made by transition processing (SPEC-013 §3.22 step 3), toward the new map's town, and town -1 does nothing. It also runs when the second of the two timed base explosions sends the party to Vault 13. No SSL opcode starts it. It moves only when the player's current cell is not already the town's cell. It then draws a target with the Section 3.7 jitter rule (at most 16 pixels per axis from the town center), recomputes W and Q, and sets the W-iteration counter and a private terrain counter to zero. It runs the terrain schedule above with that counter until the line completes, all within one call and with nothing drawn. Because the counter starts at zero, the first mountain iteration steps at once, and the first city iteration takes its extra step. It performs no random-encounter tests, no walk-mask checks and no queue processing. Game time is added directly, so events that come due during the trip are handled later by normal queue processing. Its city branch adds no game time at all; other branches charge Q. It still applies 24-hour party healing every W iterations. `[Must-match]`

This path makes **no discovery reveal and no travel-path marks**: neither the every-third-iteration update of Section 3.5 nor the path dots run, so the cells it crosses keep their discovery values. The arrival cell's neighborhood is revealed only when the world map next opens, by the reveal made on opening. `[Must-match]` An additional supplied argument in that path is unused. `[Must-match]`

### 3.5 Discovery and Encounter Rules

Travel-path presentation/discovery normally updates every third movement-loop iteration. Set the current cell to discovery value 2. Its surrounding 3 x 3 cells become 1 unless already 2; Scout expands this to 5 x 5. Clip against 28 columns and 30 rows, and never downgrade a visited 2 to seen 1. `[Must-match]`

An additional west/ocean reveal uses these row thresholds, rows 0-29 in order:

```text
0 0 0 0 0 0 0 1 1 1 2 3 4 5 5 5 5 8 11 12 14 14 17 18 19 19 20 20 20 21
```

When the current column is at most its row threshold, columns zero through threshold minus one on that row become fully revealed. `[Must-match]` Initial/reset clearing only visits rows 0-28 and columns 0-27 of the larger saved array; the last playable row and padding are not comprehensively re-cleared on later resets. `[May-improve]`: clear all 31 x 29 bytes on a new-game reset. Preserve the full saved array rather than assuming those bytes were omitted. `[Must-match]`

At each W-iteration check, draw **three independent inclusive d6 values and sum them**, even when standing in a town cell. A town cell suppresses entry into a random encounter. Otherwise use this frequency-class table: `[Must-match]`

| Frequency class | Encounter when 3d6 sum is | Favorable outcomes out of 216 |
| ---: | --- | ---: |
| 0 | Below 6 | 10 |
| 1 | Below 7 | 20 |
| 2 | Below 9 | 56 |
| 3 | Below 10 | 81 |

There is no percentage encounter roll on every rendered frame, and no generic Outdoorsman avoidance dialogue in this path. Outdoorsman affects W and Q above. `[Must-match]`

After an encounter is selected, test for a special encounter using `3d6 - 5 + effective Luck + 2 x Explorer ranks`. A value below 18, or all six special bits already consumed, gives an ordinary encounter. `[Must-match]` Otherwise draw 1-100:

| Inclusive range | Special map | Consumed bit |
| --- | --- | --- |
| 1-30 | `FOOT.MAP` | `0x01` |
| 31-50 | `TALKCOW.MAP` | `0x02` |
| 51-70 | `USEDCAR.MAP` | `0x04` |
| 71-80 | `TARDIS.MAP` | `0x08` |
| 81-90 | `FSAUSER.MAP` | `0x10` |
| 91-100 | `COLATRUK.MAP` | `0x20` |

If the selected special was already consumed, restart the special test, including the new 3d6 gate; do not merely renormalize the remaining 1-100 weights. A newly selected bit is marked before the map load. `[Must-match]`

Ordinary terrain/dropdown map selection repeatedly draws an index 0-2 until a populated terrain choice is found: `[Must-match]`

| Terrain | Choices |
| --- | --- |
| Desert | `DESERT1.MAP`, `DESERT2.MAP`, `DESERT3.MAP` |
| Mountain | `MOUNTN1.MAP`, `MOUNTN2.MAP`, empty |
| City | `CITY1.MAP`, empty, empty |
| Coast | `COAST1.MAP`, `COAST2.MAP`, empty |

Before loading, campaign global index **65** receives the cell's encounter-region number, not its terrain or frequency code. `[Must-match]` Index **32** receives entry context 1 for voluntary terrain dropout, 2 for ordinary random encounter, or 0 for special encounter. Encounter populations and regional creature selection are handled by the selected map's scripts; no engine-side external weighted-monster text format is defined by this revision. `[Must-match]`

### 3.6 Complete Fixed World Lookup Grids

This is a compact numeric catalog of the compiled lookup data, **not an on-disk text file**. Each row has 28 digits per column group. The first group is terrain, the second frequency class, and the third encounter region. Region digits `A` through `F` mean decimal 10-15; row numbers are decimal 00-29. `[Must-match]`

```text
Row Terrain                      Frequency                    Region
00  1110000001111111111100000000 3333333332211100001112223333 BBBBBBBBB2222224442255500000
01  1110000001111111111000000000 3333333332211100000111222333 BBBBBBBBB2222224444555550000
02  1110000000111111111100110000 3333333332221100000111111133 BBBBBBBBB0222224444255552000
03  1111000000011111111100010000 3333333332211111111111111233 BBBBBBBBB0022222444200662000
04  1111100000001111011100000000 3333333311111111111111111222 BBBBBBBBB0002222442200662000
05  1111200000001111000110100000 2220000001111122222111111222 BBBBBBBBB0002222000220200000
06  3111110000000111000010000022 2220000000111122222111111222 F222220000000222000020000000
07  3311100000000111100010100020 2222222200111122222111111222 FF22200000AAA222200020200000
08  3331000000000121100011100000 2222221111111222222111111122 FFF200000AAAA202200022200000
09  3330000000000111100000000000 2222211221122111111111111112 FFF000000AAAA222777700000000
10  3333000000000111000000000000 2222211222222111111111111112 FFFF00000AAAA222777700000000
11  3333300000000011000000000000 2222221222221111111111111112 FFFFF00000AAA022777779999000
12  3333300002000011000000200000 2222222222222211222212111122 FFFFF00000000022088099999000
13  3333330100000010000002200000 2222222222222212222222211112 FFFFFF020000002E888099999000
14  3333332100000000020000200000 2222222222222222222221111122 FFFFFF02000000EE888099999000
15  3333332201000000000000000200 2222222222222222222221111112 FFFFFF111311111E888889999A11
16  3333333300111000000000000000 2222222222222222222211111111 FFFFFFFF11333DDDDDDDD1999111
17  3333333333300021110000000000 2222222222222222222111111112 FFFFFFFFFFF11DDDDDDDD1111111
18  3333333333322022220011000000 2222222222222222222221111222 FFFFFFFFFFFDDDDDDDDDD3111111
19  3333333333333322200001000000 2222222222222222222221111222 FFFFFFFFFFFFFFDDDDDDD3111111
20  3333333333333332230011100000 2222222222222222222221111222 FFFFFFFFFFFFFFFDDFDDD3331111
21  3333333333333333330011110000 2222222222222222222221111122 FFFFFFFFFFFFFFFFFF1133333111
22  3333333333333333330011111000 2222222222222222222211111111 FFFFFFFFFFFFFFFFFF1133333111
23  3333333333333333333211111000 2222222222222222222221111333 FFFFFFFFFFFFFFFFFFF133333111
24  3333333333333333333211111000 2222222222222222222222333333 FFFFFFFFFFFFFFFFFFF1333CCCC1
25  3333333333333333333311111000 2222222222222222222222333333 FFFFFFFFFFFFFFFFFFFF333CCCC1
26  3333333333333333333331111100 2222222222222222222222333333 FFFFFFFFFFFFFFFFFFFFF33CCCC1
27  3333333333333333333331000000 2222222222222222222222333333 FFFFFFFFFFFFFFFFFFFFF3CCCCC1
28  3333333333333333333330001000 2222222222222222222222333333 FFFFFFFFFFFFFFFFFFFFF1CC3111
29  3333333333333333333330000000 2222222222222222222222333333 FFFFFFFFFFFFFFFFFFFFF1111111
```

### 3.7 Town Locations, Knowledge, and Hotspots

Town detection compares the current 50-pixel cell with these fixed locations. Clicking a town destination aims near the center, `50 x column + 25`, `50 x row + 25`, then applies jitter with exactly four inclusive draws in this order: X magnitude 0-16, X sign 0-1, Y magnitude 0-16, Y sign 0-1. A sign result of 1 adds the magnitude and 0 subtracts it. This is a magnitude draw followed by a sign draw, not one signed draw over -16..16: per axis, offset 0 has probability 1/17 and each of ±1..±16 has probability 1/34. `[Must-match]` The same rule serves clicking a known town, choosing a town entrance or section while travelling, and script-directed town travel (Section 3.4). A click on a known town makes no draws when the player is already travelling to that town or the target is the town the player currently occupies. `[Must-match]`

| Town id | Town | Cell column,row | Known-town campaign index | Entrance campaign indices, slot order |
| ---: | --- | --- | ---: | --- |
| 0 | Vault 13 | 16,1 | 67 | 558,559,560,561 |
| 1 | Vault 15 / Buried Vault | 25,1 | 70 | 562,563,564,565 |
| 2 | Shady Sands | 21,1 | 68 | 566,567,568 |
| 3 | Junktown | 17,10 | 71 | 569,570,571 |
| 4 | Raiders | 22,3 | 69 | 572 |
| 5 | Necropolis | 22,13 | 72 | 573,574,575 |
| 6 | Hub | 17,14 | 73 | 576,577,578,579,580,581 |
| 7 | Brotherhood | 12,9 | 74 | 582,583,584,585,586 |
| 8 | Military Base | 3,1 | 78 | 587,588,589,590,591 |
| 9 | Glow | 24,25 | 76 | 592,593 |
| 10 | Boneyard | 15,18 | 75 | 594,595,596,597,598 |
| 11 | Cathedral | 15,20 | 77 | 599,600 |

For towns 1-11, a known-town campaign value **equal to 1** enables that town, reveals its cell as seen if previously unknown, and enables entrance slot zero. Other nonzero values are not equivalent to 1 in this test. `[Must-match]` Entrance values are first narrowed to a byte and then converted to 0/1, so a nonzero low byte enables an entrance; a value such as 256 does not. `[May-improve]`: test the full 32-bit value for nonzero.

Vault 13's normal entrance update instead enables only slot zero before game tick 864000, and all four slots at/after that tick. Alternate knowledge row 12 is enabled by Cathedral destruction or known Cathedral entrances; row 13 by Military Base destruction/known entrances; row 14 by any known Brotherhood interior slot 1-4. `[Must-match]`

Each compiled hotspot has anchor X/Y, an entry selector, and a map filename. The following complete catalog uses `slot:X/Y/entry-selector/filename`; missing trailing slots are zero/empty, and enumeration stops at the first zero X or Y. `[Must-match]`

```text
00: 0:202/303/0/V13ENT.MAP; 1:271/282/1/VAULT13.MAP; 2:292/237/2/VAULT13.MAP; 3:309/204/3/VAULT13.MAP
01: 0:68/250/0/VAULTENT.MAP; 1:107/209/1/VAULTBUR.MAP; 2:298/187/2/VAULTBUR.MAP; 3:135/290/3/VAULTBUR.MAP
02: 0:158/192/1/SHADYW.MAP; 1:270/253/2/SHADYE.MAP; 2:314/217/3/SHADYE.MAP
03: 0:400/317/3/JUNKENT.MAP; 1:304/257/2/JUNKKILL.MAP; 2:200/279/1/JUNKCSNO.MAP
04: 0:241/398/1/RAIDERS.MAP
05: 0:398/265/3/HOTEL.MAP; 1:239/224/2/HALLDED.MAP; 2:79/207/1/WATRSHD.MAP
06: 0:238/78/1/HUBENT.MAP; 1:205/172/3/HUBDWNTN.MAP; 2:128/138/5/HUBHEIGT.MAP; 3:306/137/2/HUBOLDTN.MAP; 4:272/238/4/HUBWATER.MAP; 5:125/216/0/DETHCLAW.MAP
07: 0:172/167/1/BROHDENT.MAP; 1:254/194/2/BROHD12.MAP; 2:136/263/3/BROHD12.MAP; 3:280/306/4/BROHD34.MAP; 4:161/373/5/BROHD34.MAP
08: 0:197/83/0/MBENT.MAP
09: 0:340/149/0/GLOWENT.MAP; 1:334/195/1/GLOW1.MAP
10: 0:276/239/3/LAADYTUM.MAP; 1:229/195/4/LABLADES.MAP; 2:179/185/5/LAFOLLWR.MAP; 3:346/114/1/LAGUNRUN.MAP; 4:285/159/2/LARIPPER.MAP
11: 0:86/328/0/CHILDRN1.MAP; 1:229/313/1/CHILDRN1.MAP
12: 0:235/301/0/CHILDEAD.MAP
13: 0:106/64/0/MBDEAD.MAP
14: 0:172/167/0/BRODEAD.MAP
```

Only known slots create buttons. Each button is 164 x 25 at `(anchor X - 57, anchor Y - 6)`, with a masked 25 x 13 hotspot image inside its label buffer. Normal label message number is `200 + 10 x town + slot`; Cathedral and Military Base alternate labels instead use message 290 and 501, respectively. The town picture is interface art index `156 + town`, a 453-by-444 image drawn at (20,20). `[Must-match]`

Button event numbers are `514 + visible-button ordinal`; an explicit mapping converts that ordinal back to the original slot, so hidden earlier slots do not renumber stored section indices. Selection passes the slot's entry selector through campaign index 32; it is **not automatically an elevation**, since values can exceed 2. `[Must-match]`

On first entry to a town whose enabled/visited bit is still clear, load its slot-zero map directly and set the bit. Later entry can display the town map. Cathedral destruction, campaign index 18 nonzero, substitutes alternate row 12 and `CHILDEAD.MAP`; Military Base destruction, index 17 nonzero, substitutes row 13 and `MBDEAD.MAP`. These destructive-area substitutions use entry selector zero. `[Must-match]`

### 3.8 Pip-Boy Holodisks and Quest Status

There is no disk/quest definition text-file parser in this version. Disk acquisition and quest state are reconstructed from saved campaign globals and localized Pip-Boy messages. The dedicated Pip-Boy central save component has zero bytes. `[Must-match]`

For disk ordinals 0-17, these are the controlling zero-based campaign indices: `[Must-match]`

```text
Ordinal: 0   1   2   3   4   5   6   7   8   9   10  11  12  13  14  15  16  17
Global:  162 163 161 164 165 166 167 209 210 211 212 234 235 258 259 260 237 311
```

Title message id is `400 + ordinal`. Content begins at `1000 x ordinal + 1000` and is searched through `1000 x ordinal + 1499`, at most 500 message records. `[Must-match]` Exact text `**END-DISK**` terminates a disk; `**END-PAR**` displays a blank paragraph line. Each page consumes at most 35 message records, including paragraph markers. `[Must-match]` The stored last-page index is `floor(records before end / 35)`, so an exact multiple of 35 can expose an extra empty final page. `[May-improve]`: use `floor((records before end - 1) / 35)`, minimum 0. Missing termination is diagnosed rather than defining unlimited text. `[Must-match]`

Content is enumerated by numeric message id, not by physical record order; duplicate ids resolve per SPEC-002 §3.5 before enumeration. A gap, meaning an id absent inside a disk's window, is neither skipped nor fatal and does not stop any pass: the lookup yields the fixed placeholder text `Error` (SPEC-002 §3.5), which counts as one record in page counting, page seeking and the 35-record page budget, and is drawn as an ordinary text line. Only exact `**END-DISK**` ends a pass early; reaching the 500-id window bound without it only logs a diagnostic. `[Must-match]` The display pass is limited to 35 records, not to the window, so the last page of an unterminated disk (500 records, last-page index 14, page start +490) fetches the 25 ids `1000 x ordinal + 1500` through `+1524` past the window, which normally display as `Error`. `[May-improve]`: stop every pass at the window bound. The shipped English, French and Spanish `pipboy.msg` files have no gaps inside any disk window and terminate all 18 disks.

Title/list enumeration treats any nonzero acquisition value as present, but selection-to-disk mapping uses positive values. Normal acquisition flags should therefore be zero or positive; negative values can make the list and selected disk disagree. Title enumeration skips a disk whose value is zero and continues through all 18 ordinals, so an unacquired disk never ends the list. When no positive-valued disk matches the selected visible row, the mapping runs past the table and selects ordinal 18, whose title id 418 and content ids 19000-19499 do not exist and display as `Error`. `[May-improve]`: treat only positive values as acquired in both passes, which makes the ordinal-18 fall-through impossible. There is no independent per-disk read/unread state serialized by the Pip-Boy component. `[Must-match]`

Quest rows use the town ids above. Each row has nine campaign-index positions, with trailing zero sentinels: `[Must-match]`

| Town id | Quest campaign indices before trailing zeros |
| ---: | --- |
| 0 | 238,309,308,101,188 |
| 1 | None |
| 2 | 239,240,103,43 |
| 3 | 144,105,143,145,104,555 |
| 4 | None |
| 5 | 29,31 |
| 6 | 100,112,111,106,107 |
| 7 | 108,109 |
| 8 | None |
| 9 | 140,139 |
| 10 | 135,127,129,133,138,130,128 |
| 11 | None |

A normal quest with value at most zero is hidden; value 1 is active, ordinary green text; a value above 1 is displayed as completed, struck through and dimmed. These are fixed thresholds, not fields parsed from `quests.txt`. The water-chip quest at campaign index 101 is special: always display it as active when its actual value is at most 1, completed when above 1. It therefore makes Vault 13's quest group visible even at zero. `[Must-match]`

Location-title message id is `700 + 10 x town`; quest text id is `701 + 10 x town + zero-based row position`. Visible quest numbering is compact even when intervening entries are hidden. `[Must-match]`

The location-list pass checks all nine positions without stopping at a zero sentinel, and can consequently inspect **campaign global zero** for padded slots. A positive value there can advertise an otherwise empty group. The detail/selection passes do stop at the sentinel. `[May-improve]`: stop at the first zero sentinel in every pass. Apart from that rule, keep the fixed table; do not substitute a different quest registry.

### 3.9 Pip-Boy Rest, Healing, and Timer Advancement

The visible alarm menu offers 10/30 minutes, 1-6 hours, until 06:00/12:00/18:00/00:00, and until healed. A separate until-party-healed enum value exists but is not exposed/handled as an operational extra menu choice here. `[Must-match]`

Rest eligibility examines living non-player critters on the current elevation. Any whose last attacker is the player blocks rest. In team-checking locations, any different combat team also blocks it, regardless of visibility or distance. Team checking is forced for map numbers 35,39,40,45,46; omitted for map 17, map 3 above elevation zero, and otherwise towns 0,2,3,6,7,10. Other locations check teams. This path does not derive permission from an assumed external rest-flag table. `[Must-match]`

One game minute is 600 ticks and one hour 36000. For valid explicit duration H hours and M minutes with T equal to `60 x H + M` and T positive, the intended uninterrupted endpoint is current tick plus `600 x T`. Waiting does not simulate ordinary map animation for each elapsed minute; it interpolates game time through UI update steps. `[Must-match]`

The recovered animation allocation is:

`A = 0.25 + 3.5 x T / 1440` seconds

`N = 20 x A x M / T` minute-phase step count before truncation

`K = 20 x A - N` hour-phase step count before truncation

For integer step I from zero while I is less than `trunc(N)`, the minute-phase candidate is `trunc(start tick + I x (600 x M) / N)`. On successful completion, set the exact minute endpoint. The hour phase then uses its own start tick and `trunc(start tick + I x (36000 x H) / K)` for I below `trunc(K)`, finally setting the exact hour endpoint. Each UI step waits at least 50 wall-clock milliseconds. `[Must-match]`

If a candidate reaches/passes the next queue event, temporarily set time to that due tick plus one and process it. A nonzero handler stops waiting and marks the Pip-Boy for exit. Escape/quit can also stop waiting. A successful event pass can be followed by setting time back to the candidate. The final catch-up pass uses **current time greater than next due time**, not equality. `[Must-match]`

Healing uses a separate accumulated **credited-minute** counter, reset when the Pip-Boy is opened. A completed minute phase credits M once. Each executed hour-phase step instead credits `trunc((60 x H) / K)`. At credit at least 180, reset the counter to zero, discard any overshoot, and apply three hours of party resting healing. There is no final adjustment for fractional/truncated credit or counter remainder. `[Must-match]`

For party resting healing with argument J hours, each critter party member gains `trunc(J / 3) x effective healing rate`, bounded by maximum HP. Thus a rest credit trigger applies one healing-rate unit; travel's 24-hour application applies eight. Healing is not automatically awarded just because the world clock crossed a three-hour boundary. `[Must-match]`

Until-healed first estimates `trunc(3 x missing player HP / player healing rate)` hours and waits in chunks of at most 24. It then repeats three-hour rests while a previously sampled player deficit remains nonzero. This can perform one additional three-hour chunk after the player reaches full HP. It does not independently test all party members; if the player starts full it returns immediately. `[Must-match]` Healing rate zero and malformed HP/rate combinations do not have a safe defined completion guarantee. `[May-improve]`: when the player's healing rate is not positive, perform no until-healed rest.

Wake-up presets target the next requested whole hour. If already exactly at that hour and minute zero, wait 24 hours rather than zero. Queue effects, poison/radiation, campaign deadlines, and ordinary interruptions remain active during explicit time advancement. `[Must-match]`

### 3.10 Map Numbers, Town Lookup and City Music

**Map numbers.** A map number is a message number 0-65 in `text/<language>/game/map.msg`; that message's text is the map's file name. There are exactly 66 numbers. They index the automap database (section 2.8), the area short names (SPEC-014 §3.14), the excluded-map set, the town lookup and the city music below. `[Must-match]`

**Assigning the current map number.** `[Must-match]` After a map stream (`.MAP` or `.SAV`) has been read and the player placed at its entrance, the current map number is set from the **header's name field** (SPEC-003 §2.7, offset `0x04`), not from the requested file name:

1. If the name contains the lowercase text `.sav`, or is exactly `TMP$MAP#.MAP`, keep the header's map-number word (offset `0x34`).
2. Otherwise uppercase the name and replace its first `.SAV` with `.MAP`. Compare it with the texts of messages 0-65 in order; the first exact match gives the number.
3. No match gives -1.

Apart from case 1, the header's offset-`0x34` word is overwritten, so a renamed copy of a map takes the number of its name. A `.SAV` written by the engine stores its name as `<NAME>.SAV` in upper case, so it matches its original map. Case 1 therefore never arises from a file that R1 writes, since every name it writes is upper case. It arises only from a hand-made stream.

**Working `.SAV` names.** `[Must-match]` A loaded map keeps the **file name** it was opened under, in upper case. That name, not the map number, names its working `.SAV`:

1. Every request to open a map by name uppercases the name first. A load by map number uses the number's `map.msg` text from the table below. A town-map hotspot, a random or special encounter, or a destroyed-area substitution uses its file name from sections 3.5 and 3.7. A game load uses the `SAVE.DAT` field at `0x73` (section 2.4).
2. If the uppercased name contains `.MAP`, its first `.MAP` is replaced by `.SAV`, and `MAPS\` plus that name is probed in the selected database (section 3.2). If the probe opens, the saved copy is loaded, and its `.SAV` name becomes the map's file name. Otherwise the `.MAP` is loaded under the requested name. A name without `.MAP` is opened as given, with no probe.
3. The **stem** is the file name up to, not including, its first `.`, or the whole name when it has none. The working name is the stem followed by `.SAV`. Leaving a map writes `MAPS\` plus the working name, or deletes that file when the map is excluded (SPEC-014 §3.35). The header name field (offset `0x04`) of the written stream holds the same working name. A game save writes the same file. It stores the stem and a lower-case `.sav` in the `SAVE.DAT` field at `0x73`, NUL-padded to 16 bytes, for example `VAULTENT.sav`. Loading uppercases that name again.
4. The map number chooses the automap entry, the exclusion test, the town and the music. It never chooses the file. So map 7, loaded by number as `VAULTENT.MAP`, works as `VAULTENT.SAV`. A stream opened as `HUBENT.SAV` whose header keeps word 7 under case 1 still works as `HUBENT.SAV`, with map 7's automap entry, exclusion test, town and music.

**The 72 corpus maps.** The corpus `maps/` directory holds 72 `.MAP` files. 66 of them are the numbered maps below, and each one's header word `0x34` already equals its number. The other six, `CARAVAN.MAP`, `JUNKDEMO.MAP`, `TEMPLAT1.MAP`, `ZDESERT1.MAP`, `ZDESERT2.MAP` and `ZDESERT3.MAP`, are not in `map.msg`, store -1 there, and load with map number -1. Normal play cannot reach them. Hotspots, encounter tables, exit grids and `load_map` all go through map numbers. `load_map` with a name string first matches it against `map.msg` (SPEC-005) by the rule below, so an unlisted name makes no transition. The only engine path that loads a map by file name alone is a self-run demo recording (SPEC-014 §3.6), and the corpus holds no recordings. `[Must-match]`

**Matching a map name.** `[Must-match]` `load_map` with a string finds a map number as follows. Uppercase the string and replace its first `.SAV` with `.MAP`. Then compare it exactly with the texts of messages 0-65, in order; the first equal text gives the number. Nothing else is normalized: no extension is added, and no directory, file or map header is consulted. `hubent.map`, `HUBENT.MAP` and `hubent.sav` give 36. `HUBENT`, `hubent`, `HUBENT.MAP ` with a trailing space, and `MAPS\HUBENT.MAP` match nothing, so no transition happens. The entrance parameter is stored in campaign element 32 before the match, so it is stored even when nothing matches (SPEC-005 §3.5). A string of 16 or more bytes overruns the reference's 16-byte work buffer. `[May-improve]`: treat such a string as no match; no text of messages 0-65 is longer than 12 bytes.

**Town lookup.** `[Must-match]` Each map number has one fixed town id (section 3.7). An out-of-range number, including -1, gives town -1. The table assigns a town to every numbered map, including the interior maps that no hotspot in section 3.7 names: the Necropolis vault (9) belongs to Necropolis, the caves (16) to Shady Sands, the second Cathedral map (18) and both Master's Lair levels (33, 34) to Cathedral, the two inner Military Base maps (31, 32) to Military Base, the second Glow map (43) to Glow, and `HUBMIS1` (65) to Vault 13, although its short name and music are the Hub's. Random-encounter, special-encounter and caravan maps carry town 1 (Vault 15), except the fourth caravan pair (63, 64) and `HUBMIS1`, which carry town 0. These are plain table values, not a "no town" marker.

The town is used in three places:

- **Map transitions** (SPEC-013 §3.22 step 3). After a map is loaded by number, the party is moved on the world map to the new map's town by the script-directed town travel of section 3.4, unless the party's current cell is already that town's cell. Town -1 moves nothing.
- **Rest permission** (section 3.9). Town -1 falls into the team-checking default.
- **Town-map and world-map entry** start from the current town (SPEC-013 §3.22).

**City music.** `[Must-match]` A track name `t` means `sound/music/<t>.acm`, looked up case-insensitively. The world map plays `03wrldmp`. Loading a map first starts the background track `wind2`, looping (SPEC-013 §3.22). When the map number is known, its city track then replaces it:

- The current background track is stopped and the city track starts from its beginning, looping. This happens even when the same track was already playing.
- The city track starts after a saved copy is loaded, after any load by map number, after a town-map or world-map entry loads its map, after a game load, and when a new game starts on its first map.
- One map change can request the city track two or three times in a row, restarting it each time before any of it is heard. `[May-improve]`: start the track once per map change, and do not restart a track that is already playing.
- When the map number is -1, nothing replaces `wind2`, which keeps looping for as long as that map is shown.
- When music is disabled the request is recorded but nothing plays. Re-enabling music later plays the last recorded track.

**Music on/off state.** No key, preference control or script opcode toggles music directly. The state is not stored in save slots. `[Must-match]`

1. At startup, music is off. The `fallout.cfg` key `music` in the `[sound]` section, default 1 (SPEC-014 §2.3), then turns it on if nonzero. With `music` = 0, music stays off for the whole run, and no volume change can turn it on. The game never writes a different value to this key.
2. While music is on, it is turned off **by volume** whenever master volume or music volume is set to 0. That happens from the preferences sliders, from loading a save's preference words, or from configuration at startup. Turning off stops the current track and sets the movie audio level to 0 (SPEC-012 §3.8).
3. Music that was turned off by volume is turned back on when music volume is set again, or master volume is set to a nonzero value while music volume is nonzero. A game reset also turns it back on. Turning on sets the movie level to `trunc(0.94 x music_volume)` and replays the last recorded track. If music volume or master volume is 0 at that moment, music turns off by volume again at once.
4. Dialogue's music handling does not change this state, except through a music volume of 0 as in rule 2. It works as follows. `[Must-match]`
   - **When.** The change is made when the dialogue session opens (`start_gdialog`, SPEC-005), after the head window has been created. It is undone when the session closes (`end_dialogue`, or the dialogue end path of SPEC-006 §3.9), after the windows are destroyed. A session that is already open changes nothing. Barter and the Review and Ask About windows inside the session change nothing.
   - **Talking head.** Opening the session stops the current track. The stored dialogue volume becomes -1. Closing the session replays the last recorded track from its beginning (subject to the on/off state above).
   - **No head (head -1).** Opening the session sets the music volume to `trunc(D / 2)`, where D is the stored dialogue volume, and stores the music volume that was current as the new D. Closing the session sets the music volume back to D. D is -1 at program start and after every talking-head session, and is never reset otherwise. So:
     - The first no-head session of a run, and every one that follows a talking-head session, sets the volume to 0, because `trunc(-1 / 2)` is 0. That turns music off by volume, and closing the session turns it back on and replays the last track.
     - A no-head session that directly follows another no-head session halves the volume stored by the previous session, not the current one. A volume changed in the preferences in between is ignored for the halving but restored on close.

     The use of the previous D is a defect. `[May-improve]` Safe rule: halve the music volume current when the session opens, and restore that volume when it closes. A music volume of 1 still halves to 0, as in rule 2.

**Unnumbered maps.** A map with number -1 has no automap entry, no area short name, no town and no city music. It is always treated as excluded when leaving it. Leaving it therefore writes no working `.SAV` and deletes any that exists (SPEC-014 §3.35). A game save made on it still writes its working `.SAV`, because the exclusion is tested only when leaving (SPEC-014 §3.35, saving step 1), and the next departure deletes that file again. `[Must-match]` That game save also looks up the automap entry for map number -1, which reads a word from before the start of the automap index (section 2.8) and acts on whatever it finds. `[May-improve]`: capture no automap for an unnumbered map.

The complete table follows. Track files are not in the corpus, which has no `sound/music/` directory, so only the names are given. The table is exact data. `[Must-match]`

| Map | `map.msg` text | Town | City music |
| ---: | --- | --- | --- |
| 0 | `DESERT1.MAP` | 1 Vault 15 | `07desert` |
| 1 | `DESERT2.MAP` | 1 Vault 15 | `07desert` |
| 2 | `DESERT3.MAP` | 1 Vault 15 | `07desert` |
| 3 | `HALLDED.MAP` | 5 Necropolis | `14necro` |
| 4 | `HOTEL.MAP` | 5 Necropolis | `14necro` |
| 5 | `WATRSHD.MAP` | 5 Necropolis | `14necro` |
| 6 | `VAULT13.MAP` | 0 Vault 13 | `06vault` |
| 7 | `VAULTENT.MAP` | 1 Vault 15 | `13carvrn` |
| 8 | `VAULTBUR.MAP` | 1 Vault 15 | `13carvrn` |
| 9 | `VAULTNEC.MAP` | 5 Necropolis | `14necro` |
| 10 | `JUNKENT.MAP` | 3 Junktown | `12junktn` |
| 11 | `JUNKCSNO.MAP` | 3 Junktown | `12junktn` |
| 12 | `JUNKKILL.MAP` | 3 Junktown | `12junktn` |
| 13 | `BROHDENT.MAP` | 7 Brotherhood | `04brthrh` |
| 14 | `BROHD12.MAP` | 7 Brotherhood | `04brthrh` |
| 15 | `BROHD34.MAP` | 7 Brotherhood | `04brthrh` |
| 16 | `CAVES.MAP` | 2 Shady Sands | `13carvrn` |
| 17 | `CHILDRN1.MAP` | 11 Cathedral | `11chilrn` |
| 18 | `CHILDRN2.MAP` | 11 Cathedral | `11chilrn` |
| 19 | `CITY1.MAP` | 1 Vault 15 | `11chilrn` |
| 20 | `COAST1.MAP` | 1 Vault 15 | `07desert` |
| 21 | `COAST2.MAP` | 1 Vault 15 | `07desert` |
| 22 | `COLATRUK.MAP` | 1 Vault 15 | `07desert` |
| 23 | `FSAUSER.MAP` | 1 Vault 15 | `07desert` |
| 24 | `RAIDERS.MAP` | 4 Raiders | `05raider` |
| 25 | `SHADYE.MAP` | 2 Shady Sands | `15shady` |
| 26 | `SHADYW.MAP` | 2 Shady Sands | `15shady` |
| 27 | `GLOWENT.MAP` | 9 Glow | `09glow` |
| 28 | `LAADYTUM.MAP` | 10 Boneyard | `10labone` |
| 29 | `LAFOLLWR.MAP` | 10 Boneyard | `16follow` |
| 30 | `MBENT.MAP` | 8 Military Base | `08vats` |
| 31 | `MBSTRG12.MAP` | 8 Military Base | `08vats` |
| 32 | `MBVATS12.MAP` | 8 Military Base | `08vats` |
| 33 | `MSTRLR12.MAP` | 11 Cathedral | `02mstrlr` |
| 34 | `MSTRLR34.MAP` | 11 Cathedral | `02mstrlr` |
| 35 | `V13ENT.MAP` | 0 Vault 13 | `06vault` |
| 36 | `HUBENT.MAP` | 6 Hub | `01hub` |
| 37 | `DETHCLAW.MAP` | 6 Hub | `13carvrn` |
| 38 | `HUBDWNTN.MAP` | 6 Hub | `01hub` |
| 39 | `HUBHEIGT.MAP` | 6 Hub | `01hub` |
| 40 | `HUBOLDTN.MAP` | 6 Hub | `01hub` |
| 41 | `HUBWATER.MAP` | 6 Hub | `01hub` |
| 42 | `GLOW1.MAP` | 9 Glow | `09glow` |
| 43 | `GLOW2.MAP` | 9 Glow | `09glow` |
| 44 | `LABLADES.MAP` | 10 Boneyard | `10labone` |
| 45 | `LARIPPER.MAP` | 10 Boneyard | `10labone` |
| 46 | `LAGUNRUN.MAP` | 10 Boneyard | `10labone` |
| 47 | `CHILDEAD.MAP` | 11 Cathedral | `08vats` |
| 48 | `MBDEAD.MAP` | 8 Military Base | `08vats` |
| 49 | `MOUNTN1.MAP` | 1 Vault 15 | `07desert` |
| 50 | `MOUNTN2.MAP` | 1 Vault 15 | `07desert` |
| 51 | `FOOT.MAP` | 1 Vault 15 | `07desert` |
| 52 | `TARDIS.MAP` | 1 Vault 15 | `07desert` |
| 53 | `TALKCOW.MAP` | 1 Vault 15 | `07desert` |
| 54 | `USEDCAR.MAP` | 1 Vault 15 | `07desert` |
| 55 | `BRODEAD.MAP` | 7 Brotherhood | `08vats` |
| 56 | `DESCRVN1.MAP` | 1 Vault 15 | `07desert` |
| 57 | `DESCRVN2.MAP` | 1 Vault 15 | `07desert` |
| 58 | `MNTCRVN1.MAP` | 1 Vault 15 | `07desert` |
| 59 | `MNTCRVN2.MAP` | 1 Vault 15 | `07desert` |
| 60 | `VIPERS.MAP` | 1 Vault 15 | `07desert` |
| 61 | `DESCRVN3.MAP` | 1 Vault 15 | `07desert` |
| 62 | `MNTCRVN3.MAP` | 1 Vault 15 | `07desert` |
| 63 | `DESCRVN4.MAP` | 0 Vault 13 | `07desert` |
| 64 | `MNTCRVN4.MAP` | 0 Vault 13 | `07desert` |
| 65 | `HUBMIS1.MAP` | 0 Vault 13 | `01hub` |

Tracks grouped: `01hub` 36, 38-41, 65; `02mstrlr` 33, 34; `04brthrh` 13-15; `05raider` 24; `06vault` 6, 35; `07desert` 0-2, 20-23, 49-54, 56-64; `08vats` 30-32, 47, 48, 55; `09glow` 27, 42, 43; `10labone` 28, 44-46; `11chilrn` 17-19; `12junktn` 10-12; `13carvrn` 7, 8, 16, 37; `14necro` 3-5, 9; `15shady` 25, 26; `16follow` 29. Note that `CITY1` (19) plays the Cathedral track, the Deathclaw lair (37) the cavern track, and the destroyed Cathedral (47), destroyed Military Base (48) and destroyed Brotherhood (55) all play `08vats`. `[Must-match]`

## 4. Synthetic Test Fixtures (Hex / Text)

### 4.1 Complete Synthetic DAT Archive

This is a complete **96-byte** archive with one root directory and two files. Each line begins with a hexadecimal archive offset, followed by actual bytes. No trailer, zero-fill range, or external payload is omitted.

```text
0000: 00 00 00 01 00 00 00 01 00 00 00 00 00 00 00 00
0010: 01 2E 00 00 00 02 00 00 00 02 00 00 00 10 00 00
0020: 00 00 08 50 41 43 4B 2E 54 58 54 00 00 00 40 00
0030: 00 00 55 00 00 00 09 00 00 00 08 09 50 4C 41 49
0040: 4E 2E 54 58 54 00 00 00 20 00 00 00 5D 00 00 00
0050: 03 00 00 00 03 00 06 07 41 42 43 EE F3 58 59 5A
```

Expected parse:

| Item | Expected value |
| --- | --- |
| Root prefix | Count 1, capacity 1, associated size 0, opaque word 0 |
| Root directory key | Byte length 1, text `.` |
| File-table prefix at `0x12` | Count 2, capacity 2, associated size 16, opaque word 0 |
| First key | Byte length 8, `PACK.TXT`; attributes `0x40`; offset `0x55`; logical length 9; packed length 8 |
| Second key | Byte length 9, `PLAIN.TXT`; attributes `0x20`; offset `0x5D`; logical length 3; packed word 3 |
| Extracted `PACK.TXT` | `41 42 43 41 42 43 41 42 43`, ASCII `ABCABCABC` |
| Extracted `PLAIN.TXT` | `58 59 5A`, ASCII `XYZ` |

The file keys are case-insensitively sorted. The opaque words are not offsets. The compressed entry is one short final logical block; it is not an invalid short non-final block.

Mutating `PACK.TXT`'s logical length to 10 without adding packed data must fail safe length validation, not invent a tenth byte. Changing its payload offset past EOF likewise requires a safe rejection even though the reference's malformed-input behavior is not a clean-error guarantee.

### 4.2 Worked LZSS and Outer-Block Examples

Each row is an independent complete final block, not concatenated short non-final blocks.

| Complete outer block bytes | Decoding steps | Output |
| --- | --- | --- |
| `80 03 58 59 5A` | High block bit set; copy three raw bytes | `XYZ` |
| `00 03 00 00 00` | Three packed bytes; control bit zero; match ring position 0, length 3 from initial spaces | `20 20 20` |
| `00 04 01 41 EE F2` | Literal `41` at ring position 4078, then match position 4078 with length 5 | Six `41` bytes, ASCII `AAAAAA` |
| `00 06 07 41 42 43 EE F3` | Control bits 0-2 are literals A/B/C; bit 3 is match start 4078, length 6 | `ABCABCABC` |

For the last row, `EE` contributes offset 238 and the high nibble of `F3` contributes 3840, total 4078. Its low nibble 3 gives match length 6. Initial writes place A/B/C at positions 4078-4080; copying then reads those and the newly emitted A/B/C, leaving the write cursor at 4087. A bulk nonoverlapping copy or a distance-based decoder gives a different result.

A valid two-block payload can be constructed as bytes `C0 00`, then **16384 copies of `41`**, then bytes `00 03 00 00 00`. Its logical length is 16387 and stored length is 16391. The output is 16384 A bytes followed by three spaces. The second compressed block resets its dictionary; the first raw block does not make those initial ring positions contain A.

Truncated match example `00 02 00 EE` has a control byte and only one match byte. A bounded decoder must reject it before reading a second token byte outside the packed body.

### 4.3 Synthetic Complete Save Header

Construct exactly **30051 zero bytes**, then overwrite these nonoverlapping ranges. Offsets are hexadecimal; all unlisted bytes remain zero. This specifies an entire header, including its thumbnail and reserved region, but **not a loadable complete game slot**: the 27 payload positions and required companions are deliberately absent.

| Offset | Replacement bytes |
| --- | --- |
| `0x0000` | `46 41 4C 4C 4F 55 54 20 53 41 56 45 20 46 49 4C 45 00 00 00 00 00 00 00` |
| `0x0018` | `00 01 00 01 52` |
| `0x001D` | `53 79 6E 74 68 65 74 69 63 20 45 69 67 68 74 00` |
| `0x003D` | `41 72 63 68 69 76 65 20 74 65 73 74 00` |
| `0x005B` | `00 10 00 0A 07 EA 00 00 00 2E` |
| `0x0065` | `00 0C 00 06 08 71 00 0D 2F 00 00 01 00 06` |
| `0x0073` | `53 59 4E 54 48 2E 53 41 56 00` |
| `0x0083` | `00 01 02 03 04 05 06 07` |
| `0x74E2` | `7F` |

Expected metadata: version 1.1R, name `Synthetic Eight`, description `Archive test`, host date day 16/month 10/year 2026, host-time word 46, game date month 12/day 6/year 2161, game tick 864000, elevation 1, map 6, and filename `SYNTH.SAV`.

The host-time value can represent hour 12 plus minute 34, not HHMM 1234. Thumbnail first-row pixels begin 0 through 7; its last pixel, row 132 column 223, is 127 at byte `0x74E2`. The 128 reserved bytes immediately afterwards are all zero. First central payload begins at `0x7563`, not at the thumbnail start or end.

Changing byte `0x0012`, within unchecked signature padding, does not change the first eighteen compared signature bytes. Changing the NUL at `0x0011` does. Altering version words may still pass this reference's header reader but is not a successful full-save compatibility test; the `[May-improve]` safe rule in section 2.4 rejects or warns. Removing the reserved tail must be rejected by a strict complete-header validator even if the loose-file seek-based reader does not detect it.

### 4.4 World, Automap, Quest, and Rest State Vectors

For displayed Outdoorsman 50 and Pathfinder rank 1, W is 90 and Q is 7200 ticks. Assuming constant desert terrain and no interruption, 90 iterations move 90 pixel steps, advance 648000 ticks (18 hours), then apply **24-hour** party healing and an encounter check. This distinguishes loop-count scheduling from actual elapsed-time healing.

For an isolated X-major movement from (100,100) toward (104,102), the recovered successive positions are (101,100), (102,100), (103,101), (104,101), then completion without another move. The nominal destination's Y 102 is not reached.

From an otherwise unknown discovery grid at cell (10,10), reveal that cell as 2 and the other eight surrounding cells as 1 without Scout. The saved current-cell byte is at relative offset `29 x 10 + 10 = 300`, not at a 28-byte-row-stride offset. With Scout, the neighborhood is 5 x 5 instead.

A seen wall at source tile 199 uses raster byte zero and shift 4, producing `10` in hexadecimal from a zero byte. A seen wall at tile zero instead writes `40` to raster byte 50, in the following serialized raster row. A source tile 39800 computes byte offset 10000 and is not part of the persisted raster payload.

With campaign index 101 zero, the water-chip quest is still visible and active. A normal quest value 2 instead displays completed/struck through. Setting campaign index 162 to 1 enables disk ordinal zero, title message 400, body beginning at 1000. If there are exactly 35 body records followed by `**END-DISK**`, the computed last-page index is 1, exposing the extra empty page. The `[May-improve]` safe rule in section 3.8 gives index 0.

For rest, assume the Pip-Boy has just opened with zero credited minutes, no queue interruption, a player included in the party, and healing rate 1. A single three-hour request gives A 0.6875 and K 13.75: thirteen executed hour steps credit `13 x trunc(180 / 13.75) = 169` minutes. Game time advances 108000 ticks, but no healing trigger occurs. Eighteen ten-minute requests **without closing the Pip-Boy** credit 180 minutes and trigger one healing-rate unit over the same three game hours. A one-HP deficit under until-healed can consequently require repeated three-hour phases rather than the naive three-hour estimate.

Town-target jitter (section 3.7) for the Hub, town 6 at cell 17,14: the center is (875, 725). The four draws X magnitude 5, X sign 0, Y magnitude 16, Y sign 1 give the target (870, 741). The draws 0, 1, 0, 0 give the center itself. Reading the jitter as one signed draw per axis would use two draws instead of four and shift every later draw.

Script-directed travel (section 3.4) from the initial position (825, 75) to Shady Sands, town 2 at cell 21,1, makes the four jitter draws. Its target then lies in X 1059-1091 and Y 59-91, so the whole trip stays in row 1 and crosses columns 17-20. No discovery byte changes on the way. Only the next opening of the world map reveals the area around cell (21,1).

### 4.5 Map Number, Town and Music Vectors

These are computed from section 3.10. Each row gives the file a synthetic map was opened as, its 16-byte header name field (offset `0x04`) and header word `0x34`, and the result after loading and after leaving.

| Opened as | Header name | Word `0x34` | Map number | Town | Music after load | Leaving |
| --- | --- | ---: | ---: | --- | --- | --- |
| `MAPS\HUBENT.MAP`, no saved copy | `hubent.map` | 7 | 36 (name uppercased and matched) | 6 Hub | `01hub` | Writes `HUBENT.SAV` |
| `MAPS\HUBENT.SAV` | `HUBENT.SAV` | -1 | 36 (`.SAV` replaced by `.MAP`) | 6 Hub | `01hub` | Writes `HUBENT.SAV` |
| `MAPS\HUBENT.SAV` | `hubent.sav` | 7 | 7 (lowercase `.sav` keeps the word) | 1 Vault 15 | `13carvrn` | Writes `HUBENT.SAV`, not `VAULTENT.SAV`, with map 7's automap entry |
| `MAPS\MSTRLR34.MAP` | `MSTRLR34.MAP` | 34 | 34 | 11 Cathedral | `02mstrlr` | Writes `MSTRLR34.SAV` |
| `MAPS\DESCRVN4.MAP` | `DESCRVN4.MAP` | 63 | 63 | 0 Vault 13 | `07desert` | Writes nothing (excluded); deletes any `DESCRVN4.SAV` |
| `MAPS\CARAVAN.MAP` | `CARAVAN.MAP` | -1 | -1 (no match) | -1, world map position unchanged | `wind2` keeps looping | Writes nothing (unnumbered) |

In every row the written file's header name is the working name, for example `HUBENT.SAV`, so the next load of row 3's file gives map 36, not 7. A game save made in row 3 stores `HUBENT.sav` at `SAVE.DAT` offset `0x73` and map number 7 at offset `0x71`.

Rest on map 34 checks combat teams (town 11 is not in the omitted set of section 3.9); rest on map 36 does not (town 6 is).

## 5. Clean-Room Verification Checklist
- [x] Zero C/C++ code snippets or struct definitions included
- [x] All multi-byte integer endianness explicitly stated
- [x] Synthetic test vectors included
- [x] Every documented quirk tagged `[Must-match]` or `[May-improve]`
- [x] No internal reference symbol names; only public SSL opcode names and file-format key literals
- [x] Spec linter run against `fallout1-re` symbols with no findings

Verification notes (map numbers, section 3.10): `tools/spec_lint.py` gives 0 errors, and no source path or raw revision hash appears. The 66-row table was generated from the reference tables and checked row by row against the English corpus `map.msg` messages 0-65, which list the same names in the same order. The header word `0x34` and name field of all 72 corpus `maps/*.map` files were read: the 66 numbered maps store their own number, and the six unlisted maps store -1. The corpus has no `sound/music/` directory, so the track names are not checked against audio files.

Update notes (2026-10-22): section 2.3 gains the final-output-length rule, checked by decoding all 20456 `MASTER.DAT` and 5459 `CRITTER.DAT` entries; section 3.4 states that script-directed travel makes no discovery reveal; section 3.10 gains the working `.SAV` name and map-name matching rules and corrects the unnumbered-map sentence; sections 4.4 and 4.5 gain the jitter, travel and working-name vectors. The four-draw jitter order was rechecked against the reference. `tools/spec_lint.py` gives 0 errors.

Update notes (2026-10-23): section 3.10 gains the music on/off state. It is read from the `fallout.cfg` `[sound]` key `music` at startup and is otherwise switched only when master or music volume is set to 0 or back above 0. No key, preference control or opcode toggles it, and it is not saved. Net tags: +1 `[Must-match]`. `tools/spec_lint.py` gives 0 errors.

Update notes (2026-10-23, second pass): section 3.10 rule 4 now says when dialogue changes the music and how. A talking head stops the track and replays it on close. A no-head session halves the volume stored by the previous session, so the first one silences music; this is tagged `[May-improve]` with a safe rule. Net tags: +1 `[Must-match]`, +1 `[May-improve]`. `tools/spec_lint.py` gives 0 errors.
