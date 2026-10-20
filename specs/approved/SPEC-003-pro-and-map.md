# SPEC-003: Prototypes, Map Streams, and Isometric Geometry

## Clean-Room Rules for Research Agents
- Document ONLY binary file layouts (byte offsets, endianness, data types), mathematical formulas, state transitions, and observable behavior.
- DO NOT copy C/C++ code, struct definitions, function names, or internal variable names from `fallout1-re`.
- Provide small synthetic hex/text test vectors at the bottom of the specification.

## 1. Overview & Scope

This specification describes the Windows/x86 resource behavior in `fallout1-re` reference revision R1: the six prototype categories, version-19 map streams, embedded scripts and recursive object inventories, and the hex/square projection used for level rendering. It is based on static inspection of readers, writers, and consumers, not execution against original game assets. Names below describe file meanings rather than implementation identifiers.

`.PRO` files are templates; `.MAP` object records are instances referring to those templates. A map cannot generally be parsed in isolation: item/scenery instance lengths depend on subtype information in the referenced prototypes. Art availability and script indexes are external dependencies. A saved map using `.SAV` has the same map-stream organization; the header's saved-state flag, not merely the filename suffix, changes initialization behavior.

**Compatibility tags.** Every documented quirk carries one tag:

- `[Must-match]`: gameplay-visible behavior, data grammar, formula, ordering, or random-draw sequence. A clean-room engine must reproduce it exactly.
- `[May-improve]`: reference memory-safety defects, undefined behavior, uninitialized reads, stale state, presentation-only transients, or a lookup that reads the wrong table in an obviously unintended way. A clean-room engine may apply the stated safe rule and should keep every other observable behavior.

Plain byte-layout tables are tagged once per table or rule rather than per row.

## 2. Binary Layout / Data Format (Byte Offset Tables)

### 2.1 Numeric conventions and identifier namespaces

Unless explicitly marked otherwise, every numeric field in every table is a four-byte big-endian word. Numeric values use signed 32-bit two's-complement interpretation; identifiers, flags, and packed coordinates additionally have the unsigned bit meanings specified below. A word is always four bytes, including legacy address placeholders. Bytes are unsigned eight-bit values. Strings and byte fields have no endianness. Arrays are contiguous, in listed order, with no implicit padding or alignment. `[Must-match]`

Prototype identifier, abbreviated PID:

- Bits 0-23: one-based entry number in the category's prototype `.LST`; 1 selects the first line, not the second. `[Must-match]`
- Bits 24-27: category number. Bits 28-31 must be zero for ordinary supported identifiers. `[Must-match]`
- Numeric encoding: `category * 16777216 + entry number`. `[Must-match]`
- The inspected dispatcher actually uses the entire most-significant byte, not a masked nibble. A nonzero upper nibble is not silently ignored; with bit 31 set the byte is read as a negative category. `[May-improve]`: reject any PID whose bits 28-31 are nonzero (other than the `0xFFFFFFFF` sentinel) with a diagnostic.
- `0xFFFFFFFF` means no prototype. `0x01000000` is a special runtime player prototype, not line 0 of a critter list; ordinary file-backed critter indexes start at 1. `[Must-match]`

| Category number | Category | Logical index path |
| ---: | --- | --- |
| 0 | Items | `proto/items/items.lst` |
| 1 | Critters | `proto/critters/critters.lst` |
| 2 | Scenery | `proto/scenery/scenery.lst` |
| 3 | Walls | `proto/walls/walls.lst` |
| 4 | Tiles | `proto/tiles/tiles.lst` |
| 5 | Miscellaneous | `proto/misc/misc.lst` |

The category numbering and index paths above are fixed. `[Must-match]`

A PID is not an art identifier: an art identifier's low 12 bits normally select a zero-based art-list entry. Neither is it a script identifier. A prototype's script-reference word uses a script category in its most-significant byte and a zero-based script-file index in its low 24 bits, or -1 for none. A map instance's script-reference word instead identifies a particular serialized script instance. `[Must-match]`

Packed destination/spatial location:

| Bits | Meaning |
| --- | --- |
| 0-25 | Hex number, normally 0-39999 |
| 26-28 | Orientation, normally 0-5 when used |
| 29-31 | Elevation, normally 0-2 |

The ordinary spatial-location encoding is `hex + elevation * 536870912`, with orientation bits zero. Do not confuse this packed word with a plain map-entry hex number. `[Must-match]`

### 2.2 Prototype common prefix and lighting/flag prefixes

All six prototype categories begin with exactly these 12 bytes. There is no magic signature, version word, file-size field, or serialized category name. `[Must-match]`

| File offset | Bytes | Meaning |
| --- | ---: | --- |
| `0x00` | 4 | PID; determines which following layout is read |
| `0x04` | 4 | Message-number base: name uses this number, description uses this number plus 1; -1 means no assigned message |
| `0x08` | 4 | Art identifier |

Items, critters, scenery, walls, and miscellaneous prototypes continue as follows:

| File offset | Bytes | Meaning |
| --- | ---: | --- |
| `0x0C` | 4 | Light radius in hexes |
| `0x10` | 4 | Light intensity; 65536 is full intensity |
| `0x14` | 4 | Basic object flags |
| `0x18` | 4 | Extended prototype flags |
| `0x1C` | 4 | Script reference, for items/critters/scenery/walls only |

Miscellaneous prototypes end at `0x1C` exclusive: they do not contain that script-reference word. Tile prototypes do not have the lighting fields at all; their flags begin immediately at `0x0C`. Thus a universal 32-byte prototype header would be incorrect. `[Must-match]`

### 2.3 Item prototypes and all seven item subtypes

The item-specific prefix follows the script word:

| File offset | Bytes | Meaning |
| --- | ---: | --- |
| `0x20` | 4 | Item subtype |
| `0x24` | 4 | Material |
| `0x28` | 4 | Size, used against container capacity |
| `0x2C` | 4 | Weight |
| `0x30` | 4 | Cost |
| `0x34` | 4 | Inventory art identifier |
| `0x38` | 1 | Uninterpreted stored byte |
| `0x39` | Variable | Subtype payload; deliberately not word-aligned |

The item prefix layout, including the single stored byte that leaves the payload unaligned, is part of the file grammar. `[Must-match]`

| Subtype | Meaning | Payload bytes | Reader-consumed total bytes |
| ---: | --- | ---: | ---: |
| 0 | Armor | 72 | 129 (`0x81`) |
| 1 | Container | 8 | 65 (`0x41`) |
| 2 | Drug | 68 | 125 (`0x7D`) |
| 3 | Weapon | 65 | 122 (`0x7A`) |
| 4 | Ammunition | 24 | 81 (`0x51`) |
| 5 | Miscellaneous item | 12 | 69 (`0x45`) |
| 6 | Key | 4 | 61 (`0x3D`) |

These payload lengths and totals are exact. `[Must-match]`

The following offsets are relative to the payload at file byte `0x39`, not relative to the file.

Armor:

| Payload offset | Bytes | Meaning |
| --- | ---: | --- |
| `0x00` | 4 | Armor class |
| `0x04` | 28 | Seven damage resistances |
| `0x20` | 28 | Seven damage thresholds |
| `0x3C` | 4 | Perk number |
| `0x40` | 4 | Male armor/critter base-art selection |
| `0x44` | 4 | Female armor/critter base-art selection |

Damage-array order everywhere in this specification is normal, laser, fire, plasma, electrical, EMP, explosion. Damage-type numbers are 0 through 6 in that same order. `[Must-match]`

Container:

| Payload offset | Bytes | Meaning |
| --- | ---: | --- |
| `0x00` | 4 | Maximum contained size |
| `0x04` | 4 | Container interaction flags; bit 0 set selects the ground reach animation (10), clear selects the middle reach (11); see SPEC-007 §3.11.3 |

This container prototype word is not the instance's lock-state word and does not contain an inventory list. `[Must-match]`

Drug:

| Payload offset | Bytes | Meaning |
| --- | ---: | --- |
| `0x00` | 12 | Three stat selectors |
| `0x0C` | 12 | Three immediate stat changes |
| `0x18` | 4 | First delayed-effect duration |
| `0x1C` | 12 | Three changes for first delayed effect |
| `0x28` | 4 | Second delayed-effect duration |
| `0x2C` | 12 | Three changes for second delayed effect |
| `0x38` | 4 | Addiction chance |
| `0x3C` | 4 | Withdrawal effect/perk selector |
| `0x40` | 4 | Withdrawal onset |

The armor, container, and drug payload layouts above are exact. `[Must-match]`

Weapon, in reader order:

| Payload offset | Bytes | Meaning |
| --- | ---: | --- |
| `0x00` | 4 | Weapon animation code |
| `0x04` | 4 | Minimum damage |
| `0x08` | 4 | Maximum damage |
| `0x0C` | 4 | Damage type |
| `0x10` | 4 | Primary maximum range |
| `0x14` | 4 | Secondary maximum range |
| `0x18` | 4 | Projectile PID |
| `0x1C` | 4 | Minimum strength |
| `0x20` | 4 | Primary action-point cost |
| `0x24` | 4 | Secondary action-point cost |
| `0x28` | 4 | Critical-failure table selector |
| `0x2C` | 4 | Perk selector |
| `0x30` | 4 | Rounds per burst |
| `0x34` | 4 | Caliber selector |
| `0x38` | 4 | Default ammunition PID |
| `0x3C` | 4 | Ammunition capacity |
| `0x40` | 1 | Sound code |

The weapon writer reverses the two damage-bound words; see section 3.1. The final sound code is one byte, not a four-byte word. `[Must-match]`

Remaining item payloads:

| Subtype | Payload offsets | Meaning in offset order |
| --- | --- | --- |
| Ammunition | `0x00,0x04,0x08,0x0C,0x10,0x14` | Caliber; quantity per ammunition item; armor-class modifier; damage-resistance modifier; damage multiplier; damage divisor |
| Miscellaneous item | `0x00,0x04,0x08` | Power-source PID; power-source type; maximum charges |
| Key | `0x00` | Key code |

Each listed remaining-payload field is four bytes. Materials are numbered 0-7: glass, metal, plastic, wood, dirt, stone, cement, leather. `[Must-match]`

### 2.4 Critter prototypes

After the common lighting/flags/script prefix:

| File offset | Bytes | Meaning |
| --- | ---: | --- |
| `0x20` | 4 | Talking-head art identifier |
| `0x24` | 4 | AI packet number |
| `0x28` | 4 | Team number |
| `0x2C` | 4 | Critter-specific behavioral flags, distinct from basic/extended flags |
| `0x30` | 140 | 35 base-stat words |
| `0xBC` | 140 | 35 bonus-stat words |
| `0x148` | 72 | 18 skill-point words |
| `0x190` | 4 | Body type |
| `0x194` | 4 | Experience awarded |
| `0x198` | 4 | Kill-category number |

Total consumed length is 412 bytes (`0x19C`). `[Must-match]` Each stat array has the following positional meanings:

- 0-6: strength, perception, endurance, charisma, intelligence, agility, luck.
- 7-16: maximum hit points, maximum action points, armor class, unarmed damage, melee damage, carry weight, sequence, healing rate, critical chance, better criticals.
- 17-23: seven damage thresholds in the damage-type order given above.
- 24-30: seven damage resistances in that same order.
- 31-34: radiation resistance, poison resistance, age, gender. `[Must-match]`

Current hit points, poison, and radiation are instance state, not additional prototype array entries. `[Must-match]` Skill order 0-17 is small guns, big guns, energy weapons, unarmed, melee weapons, throwing, first aid, doctor, sneak, lockpick, steal, traps, science, repair, speech, barter, gambling, outdoorsman. Body types 0-2 are biped, quadruped, robotic. Kill categories 0-14 are man, woman, child, super mutant, ghoul, brahmin, radscorpion, rat, floater, centaur, robot, dog, mantis, deathclaw, plant. `[Must-match]`

### 2.5 Scenery, wall, tile, and miscellaneous prototypes

Scenery continues after its script reference:

| File offset | Bytes | Meaning |
| --- | ---: | --- |
| `0x20` | 4 | Scenery subtype |
| `0x24` | 4 | Material |
| `0x28` | 1 | Uninterpreted stored byte |
| `0x29` | Variable | Subtype payload, without alignment padding |

| Subtype | Meaning | Payload fields at offsets relative to `0x29` | Reader-consumed total bytes |
| ---: | --- | --- | ---: |
| 0 | Door | `0x00`: initial door state; `0x04`: key code | 49 (`0x31`) |
| 1 | Stairs | `0x00`: destination map; `0x04`: packed destination | 49 |
| 2 | Elevator | `0x00`: elevator type; `0x04`: level | 49 |
| 3 | Ladder up | `0x00`: packed destination | 45 (`0x2D`) |
| 4 | Ladder down | `0x00`: packed destination | 45 |
| 5 | Generic scenery | `0x00`: uninterpreted retained word | 45 |

All scenery payload fields in this table are four bytes. The writer emits an additional 24-byte suffix after scenery payloads; that suffix is not required by the reader. See section 3.1. `[Must-match]`

| Category | Fields following the universal 12-byte prefix | Reader-consumed total bytes |
| --- | --- | ---: |
| Wall | `0x0C` light radius; `0x10` intensity; `0x14` basic flags; `0x18` extended flags; `0x1C` script reference; `0x20` material | 36 (`0x24`) |
| Tile | `0x0C` basic flags; `0x10` extended flags; `0x14` script reference; `0x18` material | 28 (`0x1C`) |
| Miscellaneous | `0x0C` light radius; `0x10` intensity; `0x14` basic flags; `0x18` extended flags | 28 |

All listed fields are four bytes. Miscellaneous prototypes have no serialized subtype or exit-grid destination; exit-grid destinations belong to map object instances. `[Must-match]`

### 2.6 Flag words and their separate namespaces

Basic prototype flags share these bit positions with instance flags, but creating an instance does not simply copy the complete prototype word. The constructor propagates the placement, blocking, multihex, highlighting, translucency, wall-end, light-through, and projectile-through attributes; visibility, equipment, and transient bookkeeping are instance state. `[Must-match]`

| Mask | Basic/instance meaning |
| --- | --- |
| `0x00000001` | Hidden |
| `0x00000004` | Temporary; omitted from ordinary top-level object saves |
| `0x00000008` | Flat rendering |
| `0x00000010` | Does not block movement |
| `0x00000020` | Lighting bookkeeping |
| `0x00000400` | Retained special bit; cleared on loaded top-level critters |
| `0x00000800` | Occupies multiple hexes |
| `0x00001000` | Cannot be highlighted |
| `0x00002000` | Used-state bookkeeping |
| `0x00004000` | Red translucency |
| `0x00008000` | Explicitly no translucency |
| `0x00010000` | Wall translucency |
| `0x00020000` | Glass translucency |
| `0x00040000` | Steam translucency |
| `0x00080000` | Energy translucency |
| `0x01000000` | Equipped in left hand |
| `0x02000000` | Equipped in right hand |
| `0x04000000` | Worn |
| `0x10000000` | Wall transparency end marker |
| `0x20000000` | Light passes through |
| `0x40000000` | Seen-state bookkeeping |
| `0x80000000` | Projectiles pass through |

The bit assignments above are fixed. `[Must-match]`

For prototype-driven translucency selection, the precedence is no translucency, wall, glass, steam, energy, then red. Multiple set bits do not mean that all effects are applied. Unassigned bits are retained bit patterns, not additional payload lengths. `[Must-match]`

Extended prototype flags are a different word:

- Weapon bits 0-3 select the primary attack mode; bits 4-7 select the secondary mode. Values 0-8 mean none, punch, kick, swing, thrust, throw, single fire, burst, continuous fire. Values 9-15 are not valid indexes for the inspected mode tables. `[Must-match]` The reference does not guard them, so a 9-15 nibble indexes past those tables. `[May-improve]`: treat 9-15 as mode 0 (none) and emit a diagnostic.
- `0x00000100` selects big-gun classification for otherwise applicable ranged weapons; `0x00000200` marks a two-handed weapon. `[Must-match]`
- `0x00000800` permits use; containers also permit use by subtype. `0x00001000` permits use on another object; drugs also permit that by subtype. `[Must-match]`
- `0x00004000` permits talking to a non-critter; critters permit talking by category. `0x00008000` permits picking up a container; non-container items permit pickup by subtype. `[Must-match]`
- Other extended bits, including the high orientation/cutaway masks consulted for walls and scenery, are not interchangeable with basic flags. Preserve their exact values rather than applying the basic-flag table to them. `[Must-match]`

Critter behavioral flags at prototype byte `0x2C` are yet another word:

| Mask | Meaning |
| --- | --- |
| `0x00000002` | Barter enabled |
| `0x00000020` | Cannot be stolen from |
| `0x00000040` | Does not drop items |
| `0x00000080` | No limbs |
| `0x00000100` | Does not age |
| `0x00000200` | Does not heal |
| `0x00000400` | Invulnerable |
| `0x00000800` | Flat critter behavior |
| `0x00001000` | Special death behavior |
| `0x00002000` | Long limbs |
| `0x00004000` | No knockback |

These critter-flag assignments are fixed. `[Must-match]`

Lock flags are not equipment flags even though their numeric bits overlap. For a container instance, `0x02000000` means locked and `0x04000000` means jammed in its separate instance-state word. For a door they mean locked/jammed in the door-state word; door bit 0 means open. The container prototype interaction word is separate from both. `[Must-match]`

### 2.7 Map header and top-level stream order

The map header is 236 bytes (`0xEC`), not a textual `MAP` signature followed by tagged chunks. `[Must-match]`

| File offset | Bytes | Meaning |
| --- | ---: | --- |
| `0x00` | 4 | Format version; this revision accepts exactly 19 |
| `0x04` | 16 | Map resource name, raw bytes; ordinary names contain a NUL within this field |
| `0x14` | 4 | Default player-entry hex, 0-39999 |
| `0x18` | 4 | Default player-entry elevation, 0-2 |
| `0x1C` | 4 | Default player-entry orientation, 0-5 |
| `0x20` | 4 | Local-variable pool element count |
| `0x24` | 4 | Map-script file reference: positive values are one-based; nonpositive means no map script |
| `0x28` | 4 | Map-stream flags |
| `0x2C` | 4 | Stored darkness word |
| `0x30` | 4 | Map-global variable element count |
| `0x34` | 4 | World-map map-number association |
| `0x38` | 4 | Last-visit game timestamp, ten ticks per second |
| `0x3C` | 176 | 44 retained, otherwise uninterpreted words |
| `0xEC` | Variable | First map-global variable, or the next section if the count is zero |

The header layout is exact; the 44 retained words are read and written back unchanged. `[Must-match]`

Map-stream flag meanings:

| Mask | Meaning |
| --- | --- |
| `0x00000001` | Saved/initialized map state; changes variable and object initialization |
| `0x00000002` | Elevation 0 tile block omitted / elevation marked empty |
| `0x00000004` | Elevation 1 tile block omitted / elevation marked empty |
| `0x00000008` | Elevation 2 tile block omitted / elevation marked empty |

These omission bits must not be confused with unrelated world-map metadata flags using the same numbers. They do not remove script-category counts or per-elevation object counts from the file. `[Must-match]`

Let G and L be the nonnegative effective global/local element counts, respectively. The reader changes a negative header count to zero before consuming either array. `[Must-match]` Let K be the number of elevation omission bits which are clear.

| Start offset | Bytes | Section |
| --- | ---: | --- |
| `236` | `4 * G` | Map-global variable array |
| `236 + 4 * G` | `4 * L` | Script-local variable pool |
| `236 + 4 * G + 4 * L` | `40000 * K` | Present square-tile elevation blocks, in ascending elevation order |
| `236 + 4 * G + 4 * L + 40000 * K` | Variable | Five script-category streams |
| Immediately after the fifth script category | Variable | Overall object count and three elevation object streams |

There are no section tags, alignment gaps, section-length words, or repeated variable counts between these sections. The header lists the local count before the global count, but the arrays are serialized globals first. `[Must-match]`

### 2.8 Variable pools and roof/floor tile blocks

The header's "global" array is global only to this map: it is the MVAR domain. The local array is the shared backing pool for individual scripts' LVAR slices. These are not the campaign-global GVAR array. `MAP`, `MVAR`, and `LVAR` are not ASCII chunk identifiers or required companion binary files in this reader. `[Must-match]`

Each script-local base is an element index into the local pool, not a byte offset. Script-local element i addresses `local base + i`; its physical file offset, when already present in the serialized pool, is `236 + 4 * G + 4 * (local base + i)`. Map variables themselves are zero-based. `[Must-match]`

Each present elevation block has 10000 four-byte words, one per square-grid position. Positions are row-major, `position = column + 100 * row`, with columns/rows 0-99. There are up to three such blocks, for elevations 0, 1, 2. There are not separate contiguous roof and floor arrays. `[Must-match]`

| Bits within one tile word | Meaning |
| --- | --- |
| 0-11 | Zero-based floor art-list index |
| 12-15 | Floor flag nibble |
| 16-27 | Zero-based roof art-list index |
| 28-31 | Roof flag nibble |

Thus the first two bytes on disk contain the roof half and the next two contain the floor half. These indexes select tile art, not tile prototype PIDs. Index 1 is the ordinary blank tile used for empty floor/roof content. A freshly cleared pair is word `0x00010001`. `[Must-match]`

Floor bit `0x00001000` suppresses floor rendering. Roof bit `0x10000000` suppresses roof rendering; loading a present tile block clears this roof-hide bit, without clearing the floor-hide bit. Roof bit `0x20000000` prevents the normal roof-hide flood fill from crossing that square. Other flag bits remain distinct retained bits. Roof index 1 is not drawn even when the roof-hide bit is clear. `[Must-match]`

### 2.9 Serialized script categories, blocks, and records

Script categories are stored in order 0 system, 1 spatial, 2 timed, 3 item, 4 critter. Each starts with a four-byte active-record count N. Count zero has no following blocks. For a positive N, the reader consumes `ceil(N / 16)` blocks. `[Must-match]` Any nonzero N, including a negative one, makes the reader consume at least one block. `[May-improve]`: reject a negative count with a diagnostic.

Each block consists of exactly 16 serialized record slots, then a four-byte active-slot count, then a four-byte obsolete next-block address. All 16 slots are physically present, even in a partially used last block. The address word is consumed and discarded, not followed as a file offset. The active-slot count determines which slots participate in runtime lookup; it is not a byte count. `[Must-match]` The active-slot count is not checked against 16. `[May-improve]`: reject a block whose active-slot count is negative or exceeds 16.

Each record begins as follows:

| Record-relative offset | Bytes | Meaning |
| --- | ---: | --- |
| `0x00` | 4 | Script instance identifier: category in most-significant byte; instance serial number in low 24 bits |
| `0x04` | 4 | Retained link/next word |
| `0x08` | 8 for spatial, 4 for timed, otherwise 0 | Conditional prefix: spatial packed location then radius; timed time word; absent for other categories |

The category used for this conditional prefix comes from the individual slot's identifier, not simply from the outer category stream. This also applies to inactive/padding slots, which need not contain zeroes or identifiers matching the outer group. `[Must-match]`

Let T be the first byte after the conditional prefix: `0x10` for a spatial record, `0x0C` for a timed record, otherwise `0x08`. The common tail is:

| Offset relative to T | Bytes | Meaning |
| --- | ---: | --- |
| `0x00` | 4 | Script-instance flags |
| `0x04` | 4 | Zero-based script-file list index |
| `0x08` | 4 | Obsolete program address; consumed and discarded |
| `0x0C` | 4 | Associated object identifier |
| `0x10` | 4 | Local-pool base index, or -1 for not yet allocated |
| `0x14` | 4 | Number of script-local elements |
| `0x18` | 4 | Retained result/state word |
| `0x1C` | 4 | Current event/action code |
| `0x20` | 4 | Fixed event parameter |
| `0x24` | 4 | Action-being-used value |
| `0x28` | 4 | Default-action override state |
| `0x2C` | 4 | Retained execution-state word |
| `0x30` | 4 | Event quantity/value |
| `0x34` | 4 | Script annotation flags |

The record layout above is exact. `[Must-match]`

Record lengths are therefore 64 bytes for system/item/critter identifiers, 72 for spatial identifiers, and 68 for timed identifiers. If all 16 slots have the outer category's ordinary type, a block including its two trailer words occupies 1032, 1160, or 1096 bytes, respectively. Do not apply these block constants when inactive slots encode different categories. `[Must-match]`

Script-instance flag `0x00000008` excludes the instance from ordinary saving; the writer compacts/reorders retained entries. `[Must-match]` On loading active records, flag `0x00000001` is cleared, program and object bindings are rebuilt, and the obsolete addresses do not restore live pointers. `[Must-match]` On a map without saved-state bit 0, each record's stored local-element count is reset to zero for subsequent annotation-based initialization; this does not alter how many bytes were consumed from the record. `[Must-match]`

### 2.10 Per-elevation object streams and fixed record prefix

The object section starts with a four-byte total top-level object count. Then, for each elevation 0, 1, 2, there is a four-byte top-level count for that elevation followed immediately by that many object trees. All three elevation counts are present, even when the header omits their tile blocks. For ordinary well-formed files, the total equals the sum of those three counts. Inventory children and stack quantities are not included in that total. `[Must-match]`

Each object record begins with this 72-byte prefix, regardless of prototype category:

| Record-relative offset | Bytes | Meaning |
| --- | ---: | --- |
| `0x00` | 4 | Object's unique identifier |
| `0x04` | 4 | Hex number, or -1 for an object without grid placement |
| `0x08` | 4 | Additional screen X displacement |
| `0x0C` | 4 | Additional screen Y displacement |
| `0x10` | 4 | Stored screen X coordinate |
| `0x14` | 4 | Stored screen Y coordinate |
| `0x18` | 4 | Current art frame, zero-based |
| `0x1C` | 4 | Orientation, 0-5 |
| `0x20` | 4 | Art identifier |
| `0x24` | 4 | Instance basic flags |
| `0x28` | 4 | Stored elevation; replaced with enclosing elevation when loading a map tree |
| `0x2C` | 4 | PID; selects the category and prototype-dependent state trailer |
| `0x30` | 4 | Combat identifier, distinct from the unique object identifier |
| `0x34` | 4 | Light radius |
| `0x38` | 4 | Light intensity |
| `0x3C` | 4 | Stored outline word; consumed but discarded, runtime outline becomes zero |
| `0x40` | 4 | Script instance identifier, or -1 |
| `0x44` | 4 | Cached script-file index / retained script association word |

The 72-byte prefix is identical for every category. `[Must-match]`

For a placed object, stored screen coordinates are not a replacement for projecting its hex and adding displacements; drawing updates the screen coordinates. An unplaced object's explicit screen coordinates can instead serve as its drawing corner. `[Must-match]`

### 2.11 Object inventory header and category-dependent state

Every object record, not just a container, continues with:

| Record-relative offset | Bytes | Meaning |
| --- | ---: | --- |
| `0x48` | 4 | Number of direct inventory entries |
| `0x4C` | 4 | Inventory allocation capacity |
| `0x50` | 4 | Obsolete inventory address; not a file offset or a live pointer |
| `0x54` | Variable | Instance state, selected by PID category |

Capacity does not add padding entries to the file. Only the inventory entry count determines the number of containment records. `[Must-match]` Normal files have nonnegative count and capacity at least as large as the count; the reference does not robustly validate those constraints before allocating. `[May-improve]`: reject an object whose inventory count is negative or whose capacity is smaller than its count.

Critter instance state:

| Record-relative offset | Bytes | Meaning |
| --- | ---: | --- |
| `0x54` | 4 | Reaction-to-player/state word |
| `0x58` | 4 | Damage received last turn |
| `0x5C` | 4 | Combat maneuver flags |
| `0x60` | 4 | Current action points |
| `0x64` | 4 | Combat results/status flags |
| `0x68` | 4 | AI packet |
| `0x6C` | 4 | Team |
| `0x70` | 4 | Last attacker's combat identifier, or -1 |
| `0x74` | 4 | Current hit points |
| `0x78` | 4 | Current radiation |
| `0x7C` | 4 | Current poison |

A critter record before its inventory children is 128 bytes (`0x80`). These mutable words are not the prototype's 35-stat arrays. Combat results include low bits for unconsciousness, knockdown, four crippled limbs, blindness, and death; they remain an independent flag word rather than basic object flags. `[Must-match]`

Every non-critter first has a four-byte mutable state word at record byte `0x54`. Value `0xCCCCCCCC` in that word is normalized to zero on read. `[Must-match]` Any remaining words begin at `0x58`:

| PID category / prototype subtype | Additional words in serialization order | Record bytes before children |
| --- | --- | ---: |
| Item: armor, container, drug | None | 88 (`0x58`) |
| Item: weapon | Current loaded-round count; current ammunition PID | 96 (`0x60`) |
| Item: ammunition | Current ammunition quantity | 92 (`0x5C`) |
| Item: miscellaneous | Current charges | 92 |
| Item: key | Current key code | 92 |
| Scenery: door | Current door-state flags | 92 |
| Scenery: stairs | Destination map; packed destination | 96 |
| Scenery: elevator | Elevator type; level | 96 |
| Scenery: either ladder | Packed destination | 92 |
| Scenery: generic | None | 88 |
| Wall or tile | None | 88 |
| Miscellaneous, except exit grids | None | 88 |
| Miscellaneous PID `0x05000010` through `0x05000017`, inclusive | Destination map; plain destination hex; destination elevation; destination orientation | 104 (`0x68`) |

Each additional word is four bytes; its offset is `0x58 + 4 * its zero-based position in the listed sequence`. Item/scenery subtype is obtained from the referenced `.PRO`, not stored again in the object record. An exit grid's four separate destination words are not one packed spatial-location word. `[Must-match]`

### 2.12 Inventory containment records

Immediately after an object's entire category-dependent state, repeat the following for each direct inventory entry, in order:

| Relative position in a containment entry | Bytes | Meaning |
| --- | ---: | --- |
| `0x00` | 4 | Quantity represented by this entry |
| `0x04` | Variable | One complete child object record, including its inventory header/state and recursively all its own containment entries |

The grammar is depth-first: parent record, quantity, child tree, next quantity, next child tree, then the parent's next sibling or the next elevation count. There are no child offsets, child-length words, owner-address fields, or closing markers. A quantity of 3 means one representative child tree with multiplicity 3, not three serialized child records. `[Must-match]` Parent/owner bindings are rebuilt from nesting; child elevations are set to the enclosing elevation. `[Must-match]`

## 3. Behavioral Rules & Edge Cases

### 3.1 Prototype lookup, validation, and asymmetric writing

- Prototype paths come from one-based category-list positions, not necessarily numeric filenames. Blank/comment-like list lines still consume positions under the prototype-list reader; names stop at space or line ending. `[Must-match]` An entry number exactly one greater than the list's line count is not rejected: the lookup reuses the last line's name. `[May-improve]`: fail the lookup for any entry number above the line count. The prototype's own PID controls its payload dispatch after opening. There is no comprehensive check that it matches the requested list entry/category. `[May-improve]`: reject a prototype whose stored PID differs from the requested PID.
- Short required numeric/byte reads fail. Trailing bytes are not checked for end-of-file, so reader-consumed length need not equal physical file length. `[Must-match]` An unknown top-level prototype category fails. `[Must-match]` By contrast, unknown item/scenery subtypes take no subtype payload in this revision rather than receiving a safe, independently specified layout. Do not interpret an unrecognized subtype as a known subtype with zero-filled fields. `[May-improve]`: reject a prototype with an unknown item or scenery subtype with a diagnostic.
- Weapon reading consumes minimum damage at payload `0x04` and maximum at `0x08`. Writing emits maximum at `0x04` and minimum at `0x08`. A save/reload can therefore exchange the bounds; this is not an endianness rule or a different weapon version. Reading order is `[Must-match]`. The swapped writer is `[May-improve]`: write minimum then maximum, in reader order.
- Scenery writing appends six additional words after its ordinary payload: light radius, light intensity, basic flags, extended flags, script reference, and scenery subtype. Reading stops before these 24 bytes. A reader-compatible scenery file need not include the suffix; a writer-produced file is 24 bytes longer than the lengths in section 2.5. The suffix comes from the scenery writer falling through into the wall writer. Tolerating the suffix on read is `[Must-match]`. Emitting it is `[May-improve]`: omit the suffix when writing scenery.
- A prototype's mutable inventory, current ammunition, hit points, destinations, or lock state is not a hidden serialized prototype suffix. Template defaults initialize new instances; loaded map instances use their state trailers, subject to the initialization adjustments below. `[Must-match]`

### 3.2 Map loading and variable initialization

- The accepted map version is 19; version 20 is not accepted by this revision. Required-data failures and invalid entry elevation/camera centering fail the load and trigger new-map recovery. Ordinary new-map defaults are entry hex 20100, elevation 0, orientation 0, and version 19. Header values replace those defaults on a successful load. `[Must-match]`
- Entry elevation is checked against 0-2. Camera centering also applies interior border restrictions, not just the numeric hex range. `[Must-match]` Ordinary orientation values are 0-5, ordered northeast, east, southeast, southwest, west, northwest; malformed orientation values are not safely and consistently validated as a file-format error: values 6 and above are ignored, leaving the player's previous orientation, and negative values are stored unchecked. `[May-improve]`: use orientation 0 for any header value outside 0-5 and emit a diagnostic.
- The stored darkness word is not applied as ambient illumination on load: ambient light is set to full intensity 65536. Map writing sets the darkness word to 1. Last-visit time affects saved-map aging, but is not a byte offset. `[Must-match]`
- A `.MAP` request can preferentially load an existing saved `.SAV` counterpart. Header saved-state bit 0 remains the decisive distinction for the initialization rules; changing the extension alone does not change the binary layout. `[Must-match]`
- With saved-state bit 0 clear, the binary map-global array is still read first, but later map-specific `.GAM` text initialization replaces the MVAR array when that companion resource opens successfully. The relevant text section is `MAP_GLOBAL_VARS:`. With the bit set, the embedded MVAR values are retained. Campaign GVAR values are neither of these arrays. `[Must-match]`
- Script-local slices can be allocated lazily when a script first requires them: newly appended pool elements are zeroed and the previous pool length becomes the base. A local base of -1 is an unallocated slice, not an instruction to seek backward. New slices do not insert bytes into an already-read map file. `[Must-match]` The pool growth does not check for allocation failure before zeroing. `[May-improve]`: on allocation failure, leave the pool unchanged and fail the variable access with a diagnostic.
- Out-of-range map/global-pool or local-pool reads report an error and return zero; writes report an error rather than extending the array. `[Must-match]` The script-local access path adds the script's base and does not independently enforce every requested element against the script's own slice length, so an oversized index reaches another script's slice. `[May-improve]`: treat an index outside the script's own slice as out of range (read 0, ignore the write, emit a diagnostic). System/map scripts are denied local-variable reads by that access path. `[Must-match]` The write path has no matching system-script check. `[May-improve]`: deny system/map-script local-variable writes as well.
- Omitted tile blocks consume no bytes and leave reset blank content rather than an implicit 40000-byte zero block. Fresh buffers use tile pair `(1,1)`; reset manipulates existing packed bits rather than universally zero-filling reused storage. Loading present blocks clears only the roof's transient hide bit as described above. `[Must-match]`
- Writing recomputes elevation omission bits from blank tile art and absence of non-temporary placed objects. This does not authorize omitting the corresponding elevation object-count word. `[Must-match]`

### 3.3 Script and object restoration

Map-script header reference 1 selects script-file index 0, reference 2 selects index 1, and so on. Prototype script references and script records already contain zero-based file indexes. Script instance identifiers, script-file indexes, object unique identifiers, combat identifiers, and prototype PIDs remain separate namespaces. `[Must-match]`

Objects are read by the three enclosing elevation counts, not by stopping when the overall count is exhausted. That total is used for allocation/preloading and is not robustly cross-validated against the per-elevation counts; a total smaller than the per-elevation sum overruns the preload buffer. `[May-improve]`: reject the stream when the total differs from the sum of the three elevation counts. Top-level saves traverse grid positions 0-39999 in each elevation and exclude temporary objects; children are serialized through containment rather than through those counts. `[Must-match]`

The ordinary inventory loader recursively rebuilds child owners, checks child art, and restores script bindings. `[Must-match]` The optional mapper inventory-repair mode instead reads direct children's record/state without recursively consuming their inventories; it is not the normal nested-inventory grammar. `[May-improve]`: a clean-room engine may omit this mapper-only mode and always use the recursive grammar. Runtime allocation capacities and obsolete address values must not be followed as on-disk offsets. `[Must-match]`

Object script references are resolved against the loaded script-instance records. Missing instances clear the object's reference to -1. A successfully bound top-level object refreshes its cached script-file index. Stored outlines are discarded. Loaded top-level critters clear basic special bit `0x00000400`. `[Must-match]`

On an unsaved/base map, a weapon's current ammunition PID of -1 or `0xCCCCCCCC` is replaced with the prototype default. Its loaded-round count is reset to prototype capacity whenever it differs from that capacity, not only when it is negative. Saved-state maps bypass this stock-initialization adjustment. `[Must-match]` For an exit grid with nonpositive destination map and a current art-list index below 33, the index is increased by 16 when selecting alternate exit-grid art; the four serialized destination words do not change length. `[Must-match]`

Negative/oversized script counts, inconsistent block occupancy, out-of-range identifiers, impossible inventory capacities, or truncated recursive trees are not uniformly rejected before allocation or indexing. `[May-improve]`: reject such malformed streams with a diagnostic rather than reproducing the memory faults. Likewise, a temporary item inside an inventory is not evidence of a special zero-length child encoding: the writer emits the child's quantity, omits its record, and then reports failure for the whole save. `[May-improve]`: skip a temporary inventory child entirely (no quantity, no record) and count only the written entries.

### 3.4 Hex coordinates, six neighbors, and edge stepping

The hex grid has 200 columns and 200 rows: `hex = column + 200 * row`, with both coordinates in 0-199. The stored column increases opposite the projection's horizontal-column basis; do not infer east from increasing the numeric column alone. Elevation 0-2 selects a separate layer, not another part of the hex number. `[Must-match]`

The parity below is the original stored column's parity, not row parity and not the mirrored projection column:

| Orientation | Facing | Even-column change `(column,row)` | Odd-column change `(column,row)` | Even / odd linear hex change | Pixel-center change |
| ---: | --- | --- | --- | --- | --- |
| 0 | Northeast | `(-1,0)` | `(-1,-1)` | `-1 / -201` | `(16,-12)` |
| 1 | East | `(-1,1)` | `(-1,0)` | `199 / -1` | `(32,0)` |
| 2 | Southeast | `(0,1)` | `(0,1)` | `200 / 200` | `(16,12)` |
| 3 | Southwest | `(1,1)` | `(1,0)` | `201 / 1` | `(-16,12)` |
| 4 | West | `(1,0)` | `(1,-1)` | `1 / -199` | `(-32,0)` |
| 5 | Northwest | `(0,-1)` | `(0,-1)` | `-200 / -200` | `(-16,-12)` |

The neighbor table is exact. `[Must-match]`

For an ordinary single neighbor, both resulting coordinates must remain in range; linear arithmetic alone must not wrap across a row. `[Must-match]` The inspected multi-step directional operation has a stronger edge rule: before each step it stops if the current hex is on any of the outer four grid borders, regardless of the direction. Thus an edge starting hex remains unchanged even for a requested inward step. `[Must-match]` Zero or negative requested step counts perform no steps. This operation is not obstacle-aware pathfinding. `[Must-match]` An orientation outside 0-5 indexes past the neighbor table. `[May-improve]`: reject an orientation outside 0-5 and return the starting hex.

### 3.5 Hex distance: lattice metric and reference stepping

For the mathematical shortest-path distance on this neighbor lattice, convert each stored position to axial coordinates:

- First axial coordinate = stored column.
- Second axial coordinate = stored row minus `floor((stored column + 1) / 2)`.
- Let DQ and DR be the differences between the two positions' first and second axial coordinates.
- Lattice distance = `max(abs(DQ), abs(DR), abs(DQ + DR))`, equivalently half of `abs(DQ) + abs(DR) + abs(DQ + DR)`. `[Must-match]`

This measures hex hops, not Euclidean pixel length, square-tile distance, or an obstacle-avoiding route. The reference returns 9999 immediately if either hex input is exactly -1, including when both inputs are -1. Equal valid hexes have distance zero. `[Must-match]`

For exact behavior, the inspected distance calculation uses an angular walk rather than directly evaluating the axial formula. Starting at the first hex, project the target and current hex using the reference-corner coordinates of section 3.6 and repeat until their hex numbers match:

1. Form DX = target pixel X minus current pixel X and DY = target pixel Y minus current pixel Y.
2. If DX is zero, select orientation 0 when DY is negative, otherwise orientation 2.
3. Otherwise take the principal polar angle of vector `(DX,-DY)` in radians. Multiply by `180 * 0.3183098862851122`, discarding the fractional part toward zero. Subtract this integer degree value from 90; if negative, add 360. Divide by 60, discard the fractional part, and cap the resulting orientation at 5.
4. Add that orientation's parity-dependent linear neighbor change and count one step. This walk does not use the border-stop rule of section 3.4. `[Must-match]`

Whenever the walk stays on the grid it returns exactly the axial lattice distance. `[Must-match]` The walk is not safe for every valid pair, however. It follows the screen-space direction, so it can leave the 200-by-200 grid whenever that direction runs outside the map. This happens for interior endpoints too, not only boundary hexes. For example, valid inputs 0 and 199 cause the walk to proceed `0, 1, -198`. From 5225 to 4558, the 53rd step reaches hex -122. Projection then rejects the invalid hex and leaves its outputs unset, so the reference result is undefined. Do not extend the -1 sentinel rule to every invalid integer. `[May-improve]`: return the axial lattice distance for every pair of valid hexes; it equals the walk's result wherever the walk stays on the grid.

### 3.6 Hex and floor/roof world-to-screen projection

All pixel coordinates here are integers, with X increasing rightward and Y downward. World coordinates have an arbitrary translation; the file does not store a unique camera-independent screen origin. Define OX/OY as a shared screen translation. `[Must-match]`

For a hex, let h be `199 - stored column` and let r be its stored row. Its projected reference corner is:

- World reference X = `48 * floor(h / 2) + 32 * (h modulo 2) + 16 * r`.
- World reference Y = `-12 * floor(h / 2) + 12 * r`.
- Screen reference corner = world reference plus `(OX,OY)`.
- Screen hex center = reference corner plus `(16,8)`.

The reference corner bounds a 32-by-16 hex image; it is not the center. The parity term is necessary: replacing the X expression with a constant 24-pixel column stride loses the alternating-column alignment. The formulas use mathematical floor; they are equivalent to the reference's signed-integer division and parity corrections, including hexes left of the camera. `[Must-match]`

For a viewport width W and height H, centering on a selected camera hex places that hex's reference corner at `(floor((W - 32) / 2), floor((H - 16) / 2))`. Subtract the selected hex's world reference from that pair to obtain OX/OY. Use the isometric viewport height, not automatically the entire application-window height. `[Must-match]`

For a floor/roof square, let s be `99 - stored square column` and let t be the stored square row. Its bounding-image corner is:

- Floor screen X = `OX - 16 + 48 * s + 32 * t`.
- Floor screen Y = `OY - 2 - 12 * s + 24 * t`.
- Roof screen X = floor screen X; roof screen Y = floor screen Y minus 96.

The actual square-basis step vectors are `(48,-12)` for increasing mirrored column and `(32,24)` for increasing row; increasing the stored column instead steps `(-48,12)`. The 80-by-36 constants describe the combined diamond footprint, not an 80-pixel step for every column or a 36-pixel step for every row. Relative to the bounding corner, that footprint's vertices are `(0,12), (48,0), (80,24), (32,36)`, and its center is `(40,18)`. Sprite dimensions remain art-dependent; the projection does not rescale them to the footprint. `[Must-match]`

The elevation argument selects a tile/object layer but adds no extra `elevation * height` term to these formulas. Each elevation's roof is 96 pixels above that same elevation's floor projection; it is not the next elevation's floor. `[Must-match]`

With viewport 640 by 380 and camera hex 20100, the camera hex center is `(320,190)` and OX/OY are `(-3680,-430)`. Square index 5050 has floor corner `(256,180)` and roof corner `(256,84)`. These coordinates include the hex-center and tile-image offsets, not just the basis vectors. `[Must-match]`

### 3.7 Screen-to-grid projection and integer boundary rules

Square picking is the inverse affine projection, with a 12-pixel diamond-origin adjustment. For a floor pixel, set DX to screen X minus `(OX - 16)` and DY to screen Y minus `(OY - 2)` minus 12. Then:

- Mirrored square column = `floor((3 * DX - 4 * DY) / 192)`.
- Square row = `floor((DX + 4 * DY) / 128)`.
- Stored square column = 99 minus mirrored square column.
- Return `column + 100 * row` only when both stored coordinates are in 0-99; otherwise return -1. `[Must-match]`

For roof picking, first add 96 to the input screen Y and use the same calculation. These are floor divisions even for negative numerators, not truncation toward zero. `[Must-match]`

Hex picking additionally classifies triangular corners rather than rounding to the nearest Euclidean center:

1. Set Y to screen Y minus OY. Set K to `floor(Y / 12)`.
2. Set V to screen X minus OX minus `16 * K`. Set B to `floor(V / 64)` and R to `V - 64 * B`, so R is in 0-63.
3. Start mirrored hex column at `2 * B`, adding 1 if R is at least 32. Start hex row at `B + K`.
4. Local X is `R modulo 32`; local Y is `Y - 12 * K`, in 0-11.
5. In local rows 0-3, if local X is less than `16 - 4 * local Y`, decrement the hex row. Otherwise, if local X is greater than `16 + 4 * local Y`, increment the mirrored column and decrement the row only if the new mirrored column is odd. All other local pixels leave both coordinates unchanged.
6. Stored column is 199 minus the resulting mirrored column. Return `column + 200 * row` only when both stored coordinates are in 0-199; otherwise return -1. `[Must-match]`

The strict less-than/greater-than comparisons assign pixels exactly on a triangular boundary to the central region. As with projection, elevation selects a layer but does not alter these pixel formulas. `[Must-match]`

## 4. Synthetic Test Fixtures (Hex / Text)

The three binary fixtures below are synthetic. Offsets are hexadecimal, relative to the start of the stated file or substream; labels are not bytes. Expand any explicitly repeated zero range to the stated number of bytes. These are byte-layout tests, not self-contained game assets: art and prototype-list dependencies must still be supplied for a live engine load.

### 4.1 Fixture A: complete container prototype

Length 65 bytes (`0x41`). This fixture defines PID `0x00000001`, suitable for the first items-list entry.

```text
0000: 00 00 00 01 00 00 00 64 00 00 00 00 00 00 00 00
0010: 00 00 00 00 00 00 00 10 00 00 88 00 FF FF FF FF
0020: 00 00 00 01 00 00 00 01 00 00 00 05 00 00 00 02
0030: 00 00 00 0A 07 00 00 00 A5 00 00 00 64 00 00 00
0040: 01
```

Expected values: item category, one-based prototype index 1, message base 100, art identifier 0, no light, basic nonblocking flag `0x10`, extended use/pickup flags `0x8800`, and script reference -1. Subtype 1 means container; material 1 means metal; size is 5, weight 2, cost 10, and inventory art is `0x07000000`. The uninterpreted byte at `0x38` is `A5` and must not become padding. Maximum contained size 100 begins at unaligned file offset `0x39`; interaction flags at `0x3D` equal 1.

### 4.2 Fixture B: complete empty map stream with both variable pools

Expanded length 284 bytes (`0x11C`). The 176-byte repeated range is the header's 44 reserved words, not tile data.

```text
0000: 00 00 00 13
0004: 53 59 4E 54 48 2E 4D 41 50 00 00 00 00 00 00 00
0014: 00 00 4E 84
0018: 00 00 00 00
001C: 00 00 00 00
0020: 00 00 00 01
0024: 00 00 00 00
0028: 00 00 00 0F
002C: 00 00 00 01
0030: 00 00 00 02
0034: 00 00 00 07
0038: 00 00 00 00
003C-00EB: 00 repeated 176 times
00EC: 00 00 00 07 FF FF FF FE 00 00 00 2A 00 00 00 00
00FC: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
010C: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
```

Expected header: version 19, name `SYNTH.MAP`, entry hex 20100, elevation 0, northeast orientation 0, one local word, two map-global words, no map script, map association 7, and last-visit time zero. Flags `0x0F` retain saved-state variables and omit all three square-tile blocks; there are no hidden zero-filled tile blocks between the arrays and scripts.

Expected MVAR values at `0xEC,0xF0` are `7,-2`. The single LVAR-pool word at `0xF4` is 42. Five zero script counts occupy `0xF8` through `0x10B`. The overall object count at `0x10C` and elevation counts at `0x110,0x114,0x118` are all zero. The last bytes are object counts, not another variable array.

Changing only the version word to `00 00 00 14` makes the map version unsupported. Keeping the `.MAP` name does not override the saved-state bit or cause `.GAM` replacement of these MVAR values.

### 4.3 Fixture C: object section containing one inventory stack

This is a standalone map object-section substream, not a complete map file. Length 196 bytes (`0xC4`). Both objects use Fixture A's container PID, so their individual records are 88 bytes before children. Ordinary recursive inventory loading is assumed.

```text
0000: 00 00 00 01 00 00 00 01 00 00 00 2A 00 00 4E 84
0010: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
0020: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 10
0030: 00 00 00 00 00 00 00 01 FF FF FF FF 00 00 00 00
0040: 00 00 00 00 00 00 00 00 FF FF FF FF FF FF FF FF
0050: 00 00 00 01 00 00 00 01 00 00 00 00 00 00 00 00
0060: 00 00 00 03 00 00 00 2B FF FF FF FF 00 00 00 00
0070: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
0080: 00 00 00 00 00 00 00 00 00 00 00 10 00 00 00 02
0090: 00 00 00 01 FF FF FF FF 00 00 00 00 00 00 00 00
00A0: 00 00 00 00 FF FF FF FF FF FF FF FF 00 00 00 00
00B0: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
00C0: 00 00 00 00
```

Expected top-level total and elevation-0 count are both 1. Parent record starts at `0x08`: unique object identifier 42, hex 20100, PID 1, no script, and one inventory entry with capacity 1. Its state ends at exclusive offset `0x60`.

The quantity at `0x60` is 3. The single child record begins at `0x64`, has unique identifier 43, hex -1, PID 1, no script, and no inventory. Its stored elevation 2 is replaced with the enclosing elevation 0, and its owner is bound to the parent. The zero words at `0xBC` and `0xC0` are the elevation-1 and elevation-2 top-level counts. The overall total is not 4 or 2: neither stack multiplicity nor the representative child adds a top-level object.

### 4.4 Geometry expectations

For viewport 640 by 380, camera hex 20100, and the projection in section 3.6:

| Hex | Relation to 20100 | Screen center | Lattice distance |
| ---: | --- | --- | ---: |
| 20100 | Same | `(320,190)` | 0 |
| 20099 | Northeast | `(336,178)` | 1 |
| 20299 | East | `(352,190)` | 1 |
| 20300 | Southeast | `(336,202)` | 1 |
| 20301 | Southwest | `(304,202)` | 1 |
| 20101 | West | `(288,190)` | 1 |
| 19900 | Northwest | `(304,178)` | 1 |

Picking each listed center returns its original hex. Square 5050's floor center `(296,198)` and roof center `(296,102)` pick square 5050 using their respective inverse projections. From odd-column hex 20101, a northeast step reaches 19900, not 20100. A requested directional step starting at border hex 0 remains at 0 under the border-stop operation; this does not repair the separate distance-walk boundary defect.

## 5. Clean-Room Verification Checklist
- [x] Zero C/C++ code snippets or struct definitions included
- [x] All multi-byte integer endianness explicitly stated
- [x] Synthetic test vectors included
- [x] Every documented quirk tagged `[Must-match]` or `[May-improve]`
- [x] No internal reference symbol names; only public SSL opcode names and file-format key literals
- [x] Spec linter run against `fallout1-re` symbols with no findings
