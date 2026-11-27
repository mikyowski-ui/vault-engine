# Owner brief: extended art addressing and additive art and prototype rows in `vault-formats`

Basis: `vault-engine` at revision `4d5183a`. Modules: `vault_formats::fid`
(`Fid`, `Fid::new`, `Fid::base`, `ArtCatalog::load`, `ART_DIRECTORIES`,
`FidError`), `vault_formats::lst` (`LstTable::parse`, `LstTable::art_id`,
`LstTable::prototype_id`, the `MAX_ENTRIES` limit of 1,000,000 lines),
`vault_formats::vfs` (`LayeredVfs`, `ModLayer`: the highest layer holding a
file wins it whole), `vault_formats::mod_manifest` (`ModManifest`,
`ModRegistry` with `scripts`, `maps` and `patches`; `SCRIPT_INDEXES`,
`MAP_NUMBERS`), `vault_formats::map`, `vault_formats::pro`,
`vault_formats::savegame`; `vault_core` art resolution; `vault_render`
sprite lookup; `tools/asset-audit` (`--validate-art-ids`). Specifications:
SPEC-001 §2.4 "`.LST` text and ID representation" and §3.6 "LST comments,
whitespace, metadata, and boundary cases"; SPEC-003 §2.1 "Numeric
conventions and identifier namespaces", §2.2 "Prototype common prefix and
lighting/flag prefixes", §2.3 "Item prototypes and all seven item
subtypes", §2.4 "Critter prototypes", §2.8 "Variable pools and roof/floor
tile blocks", §2.10 "Per-elevation object streams and fixed record prefix";
SPEC-005 §2.5 word `0x8149` `obj_art_fid`; SPEC-007 §2.1 "Complete 32-Bit
Art Identifier" and §2.3 "Directional File Selection and Fallback"; SPEC-008
§2.5 "Ordered `SAVE.DAT` Payloads"; SPEC-017 §4.1 "The range `0x8156..=0x8183`" (`reg_anim_change_fid`) and
§5.2 "`metarule3`" rule 107 (`art_change_fid_num`, which keeps only the low
12 bits of its argument); SPEC-024a §2.2 "Manifest additions" and §6
"Determinism"; the filed `lst-writer.md` brief.

Tags: `[Must-match]` must be exactly as stated, and every existing art
identifier keeps its meaning; `[May-improve]` the existing safe rule, or an
implementation freedom that changes no stated result; `[Safe rule]` what
the engine does with data it cannot honour.

## 1. Purpose

Total-conversion and mods with many art files need more than 4,096 entries
in one art category, and want to add art and prototype rows from their own
layer without replacing the base list files whole. Today neither is
possible, and an index past 4,095 fails silently.

What the engine does today (`4d5183a`):

1. **Bits.** A FID is a big-endian 32-bit word: base index bits 0-11,
   weapon code or head variant 12-15, animation 16-23, category 24-27,
   directional selector 28-30; bit 31 is clear in normal construction and
   ignored by filename decoding, and alias reconstruction can discard it
   (SPEC-007 §2.1). Categories 0-10 have directories; 11-15 have none.
2. **Silent wrap.** `Fid::new` masks the base to 12 bits, so index 4096
   packs as row 0, with no error. The category is packed arithmetically and
   unvalidated, so a category of 16 or more spills into the selector bits.
3. **Unaddressable rows.** `ArtCatalog::load` parses whole lists; the LST
   parser accepts up to 1,000,000 lines. `LstTable::art_id` reads
   `id & 0xfff`, so rows 4096 and above load but no FID reaches them, and
   no diagnostic says so.
4. **Bits used per category.** For critters (1), bits 12-23 and 28-30 carry
   the weapon code, animation and selector; for heads (8), bits 12-23 carry
   the variant and animation selector. For every other category, filename
   lookup reads only bits 0-11 and 24-27 (SPEC-007 §2.1).
5. **Tiles are not FIDs on maps.** A map's tile word holds a 12-bit floor
   index and a 12-bit roof index into the tile art list, with two flag
   nibbles (SPEC-003 §2.8), so the tile list has its own 4,096 cap that no
   FID change can lift.
6. **Prototypes are not capped.** A PID holds a 24-bit one-based entry
   number (SPEC-003 §2.1); the issue there is additive registration only.
7. **Where FIDs live.** Prototype `0x08` (art), item `0x34` (inventory art),
   critter `0x20` (talking head), and the object record prefix `0x20`, in
   maps, working-map saves and the `SAVE.DAT` player tree (SPEC-003 §2.2,
   §2.3, §2.4, §2.10; SPEC-008 §2.5); and script values (`obj_art_fid`,
   `art_change_fid_num`, `reg_anim_change_fid`).
8. **Layers.** A layer's file replaces the lower file whole
   (`LayeredVfs`). `ModRegistry` registers scripts (explicit `index`,
   0-16,777,215, past the base list, unique across mods), maps (numbers
   66-65535) and campaign table patches; there is no art or prototype
   registration.

## 2. Proposed API and manifest rows

### 2.1 Rust

```rust
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FidError {
    // existing variants ...
    #[error("art base index {index} does not fit category {category}'s identifier")]
    BaseRange { category: u8, index: u32 },
    #[error("art category {0} is outside 0..=10")]
    CategoryRange(u8),
}

impl Fid {
    /// As `new`, refusing a base that does not fit and a category above 10.
    pub fn try_new(category: u8, base: u32, animation: u8, weapon: u8, selector: u8)
        -> Result<Self, FidError>;
    /// The extended form (section 3.2) for categories other than 1 and 8.
    pub fn extended(category: u8, base: u32) -> Result<Self, FidError>;
    /// The list row this identifier addresses under `catalog`.
    pub fn row(self, catalog: &ArtCatalog) -> Option<u32>;
}

impl ArtCatalog {
    /// Lists with more than 4,096 rows, and the rows plain identifiers cannot reach.
    pub fn diagnostics(&self) -> &[String];
    /// Apply the `[[art]]` rows of the enabled layers (section 3.4).
    pub fn register(&mut self, rows: &[ArtRegistration]) -> Result<(), String>;
}
```

### 2.2 Manifest rows (`vault-mod.toml`)

```toml
[[art]]
category = "scenery"         # an ART_DIRECTORIES name
file = "tc_rock01.frm"       # a name as an art list line would hold it, under art/scenery/
index = 5000                 # the list row; at least the base list's length, unique across mods

[[protos]]
category = "items"           # a SPEC-003 §2.1 prototype category name
file = "00005000.pro"        # under proto/items/
index = 5000                 # the one-based entry number; past the base list, unique across mods

[[tile_remap]]
map = "tc_hall.map"          # under maps/, in this layer
file = "maps/tc_hall.tlx"    # the remap sidecar (section 3.5)
```

## 3. Behaviour

### 3.1 Explicit wrap (request a)

1. `Fid::try_new` refuses a base above 4095 for every category, and a
   category above 10, with `FidError`. `Fid::new` keeps its masking for the
   bit-exact paths that SPEC-007 §2.1 describes. `[May-improve]`
2. Every engine caller that builds a FID from a list row it computed uses
   `try_new` or `extended`, so no engine path wraps silently. `[May-improve]`
3. `ArtCatalog::load` records one diagnostic per list with more than 4,096
   rows, naming the list, its row count and the first unreachable row. The
   list still loads whole. `[May-improve]`
4. `asset-audit --validate-art-ids` reports the same lines. `[May-improve]`

### 3.2 Extended identifiers (request b, recommended scheme)

1. A FID is **extended** when bit 31 is set, its category is 0-10 and not
   1 or 8, and that category's resolved list has more than 4,096 rows.
   `[Safe rule]`
2. An extended FID addresses row `(bits 12-23) × 4096 + (bits 0-11)`, a
   24-bit row number, in the category's resolved list. `[Safe rule]`
3. Every other FID keeps its SPEC-007 §2.1 meaning exactly: with bit 31
   clear; with bit 31 set in a category whose list has at most 4,096 rows;
   for critters and heads; and `0xFFFFFFFF` (category 15). So a session
   whose lists all fit sees no difference. `[Must-match]`
4. An extended FID whose row is past the list's end has no filename, as
   the SPEC-007 §2.1 `[May-improve]` rule gives for a plain one. The object
   is logged and not drawn, as for any missing art. `[Safe rule]`
5. The canonical form of a row below 4,096 is the plain FID. An extended
   FID of such a row means the same row. `[Safe rule]`
6. Critter (1) and head (8) lists stay capped at 4,096 addressable rows in
   this revision; rows past it get the section 3.1 diagnostic. `[Safe rule]`
   (question 3)
7. Fields keep their size: every FID-bearing field of SPEC-003 and SPEC-008
   still holds one 32-bit word, so no layout changes. `[Must-match]`
8. A new mod word `mod_art_fid` (category I, row I) pushes the canonical FID
   of a row, plain or extended, or -1 for a row that cannot be addressed, so
   scripts never assemble extended FIDs by hand. Its number is the owner's
   to assign in SPEC-024a's `0x830A..=0x83FF` range. `[Safe rule]`

### 3.3 Options considered

| Option | How | For | Against |
| --- | --- | --- | --- |
| A. Per-layer art namespaces | `vault-mod.toml` declares a namespace id; maps and prototypes carry it out of band in sidecar tables | No FID bit changes meaning | Every FID-bearing record needs a sidecar, saves included; a script value cannot carry the namespace; items moving between maps lose it |
| B. Bit 31 plus a per-map remap table | Bit 31 selects a row of a table that each map supplies | Fits in 32 bits | The same FID means different art on different maps, which breaks inventories, saves and scripts that carry FIDs across maps |
| C. 16-bit base in an object/prototype sidecar | High base bits stored beside each object and prototype | Clear separation | New save component; script values truncate; every writer and reader must pair files |
| D. **Bit 31 wide base, session-wide (recommended)** | Section 3.2: bit 31 plus bits 12-23 as the high base bits, for categories whose lookup ignores those bits | No layout change anywhere; one meaning per session; scripts carry it as a plain integer | Bit 31 gains a meaning, but only in a session that has a list past 4,096 rows; critters and heads are not covered |

### 3.4 Additive rows (request c)

1. `[[art]]` adds one row to one category's list, at row `index`.
   `[[protos]]` adds one prototype line at entry number `index`.
   `[Safe rule]`
2. The **resolved list** is the list file as `LayeredVfs` serves it (the
   highest layer's copy, or the base), followed by the registered rows at
   their indexes. Rows between the file's end and a registered index are
   empty rows with no name. `[Safe rule]`
3. Numbering is explicit: `index` is required, at least the served list
   file's row count, and unique across enabled layers, as `[[scripts]]`
   `index` is. Nothing is numbered by file listing, hash map order or load
   time. `[Safe rule]`
4. Registration runs once, when the session is built, lowest layer first
   and in manifest row order. `[Safe rule]`
5. A registered `file` is read through the `Vfs` layers like any art or
   prototype file, so a higher layer may replace it as an ordinary file
   override. `[Safe rule]`
6. A layer that replaces a list file whole still works as today; registered
   rows of other layers then apply on top of the replacement, and an index
   below the replacement's length is an error at launch. `[Safe rule]`

### 3.5 Tile remap (map tiles)

1. A `[[tile_remap]]` sidecar maps the 12-bit floor and roof indexes of one
   map to rows of the resolved tile list. `[Safe rule]`
2. Layout, big-endian: magic `56 54 4C 58` (`VTLX`), version 1 (2 bytes),
   entry count N (2 bytes, at most 4096), then N entries of local index
   (2 bytes, 0-4095) and row (4 bytes), ascending by local index with no
   repeats. `[Safe rule]`
3. A listed local index draws the mapped row; an unlisted one draws its own
   index, as today. Index 1 stays the blank tile and may not be listed.
   `[Safe rule]`
4. The map's tile words, roof hiding and the hide bits are unchanged.
   `[Must-match]`

### 3.6 Effects elsewhere (request d)

1. **Saves.** Field sizes and offsets of `SAVE.DAT`, working maps and
   `AUTOMAP.DB` do not change. A save that holds extended FIDs loads in a
   session without the registering layer; its objects resolve to no row and
   are logged and not drawn. No slot records a mod list (SPEC-024a §6 item
   10). `[Must-match]` for the layout, `[Safe rule]` for the load
2. **Map format.** Unchanged; extended FIDs sit in the existing art fields.
   `[Must-match]`
3. **`asset-audit`.** `--validate-art-ids` resolves extended FIDs against
   the resolved lists and reports rows that no identifier can reach.
   `[Safe rule]`
4. **The filed LST writer brief.** Its refusal to append past row 4,096 to
   an art list stays for list files. Rows past it are registered through
   `[[art]]`, not appended to the base file. `[Must-match]` for that brief's
   existing rule

## 4. Validation and errors

Each of these stops the launch with `mod_list:` and the layer's name, as a
malformed manifest does today. `[Safe rule]`

1. An `[[art]]` row with an unknown `category`, a `file` that is not a
   single list name of 1-12 bytes without space, comma, semicolon or TAB, or
   a `file` absent from `art/<category>/` in the layer.
2. An `[[art]]` `index` below the served list's row count, above
   16,777,215, or for category 1 or 8 above 4,095.
3. A `[[protos]]` row with an unknown `category`, an `index` of 0, below or
   at the served list's row count, or above 16,777,215, or a `file` absent
   from `proto/<category>/`.
4. Two enabled layers registering one (category, index), or one manifest
   doing so twice; the message names both, as script conflicts do.
5. A `[[tile_remap]]` naming a map the layer does not hold, a second remap
   for one map, or a sidecar that fails section 3.5 item 2.

At run time:

6. `Fid::try_new` and `Fid::extended` return `FidError`; they never wrap.
   `[May-improve]`
7. An extended FID for a row past the list is the SPEC-007 §2.1 "no
   filename" result. `[Safe rule]`

## 5. Determinism property

1. The resolved lists are a function of the load order and the manifests
   alone, so two sessions with the same layers resolve every FID alike.
   `[Must-match]`
2. Registration adds no value to `state_hash`, `combat_hash` or the
   checkpoint stream; a FID enters the hash only where a covered value
   already holds it. `[Must-match]`
3. A session with no `[[art]]`, `[[protos]]` or `[[tile_remap]]` row and no
   list past 4,096 rows behaves exactly as today. `[Must-match]`
4. Round trip: for every row r of a category other than 1 and 8,
   `Fid::extended(c, r)?.row(catalog) == Some(r)` when the list has more
   than r rows; for r below 4096 the canonical FID equals
   `Fid::try_new(c, r, 0, 0, 0)?`. `[Must-match]`

## 6. Edge cases

- A list of exactly 4,096 rows: no diagnostic, bit 31 ignored.
- A list of 4,097 rows: row 4096 is reachable only as an extended FID.
- An existing FID with bit 31 set in a session whose list has 4,096 rows or
  fewer: unchanged meaning (bit ignored).
- A script that reads an extended FID with `obj_art_fid` and masks
  `& 0xFFF`: it gets the low 12 bits, as it would for any FID.
- `art_change_fid_num` (metarule3 107) with an extended FID: today it keeps
  only bits 0-11 (SPEC-017 §5.2), so it cannot select row 4096 or above
  (question 9).
- A replaced base list that is shorter than a lower layer's registered index:
  the index is still valid if it is at least the replacement's length.
- A registered row whose file a higher layer deletes: the row has a name and
  no file; the object is logged and not drawn.

## 7. Synthetic test vectors

Vector A, a synthetic scenery list of 4,100 rows, `s0000.frm` to
`s4099.frm`, one per line, with no layer registrations:

| Row | Plain FID | Extended FID | Resolves to |
| ---: | --- | --- | --- |
| 0 | `0x02000000` | `0x82000000` | `s0000.frm` |
| 5 | `0x02000005` | `0x82000005` | `s0005.frm` |
| 4095 | `0x02000FFF` | `0x82000FFF` | `s4095.frm` |
| 4096 | none | `0x82001000` | `s4096.frm` |
| 4099 | none | `0x82001003` | `s4099.frm` |
| 4100 | none | `0x82001004` | no filename |

- `Fid::try_new(2, 4096, 0, 0, 0)` is `Err(BaseRange { category: 2, index: 4096 })`;
  `Fid::new(2, 4096, 0, 0, 0)` stays `0x02000000` (row 0).
- The raw FID `0x02001000` (bits 12-15 set, bit 31 clear) is row 0, as today.
- `ArtCatalog::diagnostics` holds one line naming `art/scenery/scenery.lst`,
  4,100 rows, first unreachable row 4096.

Vector B, the same list cut to 4,096 rows: `0x82001000` has bit 31 ignored
and addresses row 0 (`s0000.frm`), as today; no diagnostic.

Vector C, registration. Base art list `art/items/items.lst` of 3 rows; layer `tc` with
`[[art]] category = "items" file = "tc_a.frm" index = 4100`. The resolved
list has 4,101 rows: rows 3-4099 empty, row 4100 `tc_a.frm`;
`mod_art_fid(0, 4100)` pushes `0x80001004`.

Vector D (refused at launch): `[[art]] index = 2` over a 3-row list; two
layers both registering scenery 5000; `[[art]] category = "critters"
index = 4096`; `[[protos]] index = 0`.

Vector E, tile remap: a sidecar `56 54 4C 58 00 01 00 01 0F A0 00 00 13 88`
maps local tile index 4000 to row 5000 of a 5,001-row tile list. A floor
word `0x0FA0` draws row 5000; `0x0FA1` draws row 4001.

## 8. Open questions for the owner

1. **Scheme.** Option D (bit 31 wide base) or another? Recommended: D, since
   it changes no layout and keeps one meaning per session.
2. **When bit 31 counts.** Only when the category's list exceeds 4,096 rows
   (proposed), or always? Recommended: only then, so no existing data can
   change meaning in an ordinary session.
3. **Critters and heads.** Leave them capped, or extend them through bit 31
   alone (8,192 rows)? Recommended: leave them capped in this revision; no
   known need, and their high bits are taken.
4. **Implicit numbering.** Should `index` be optional, taking the next free
   row? Recommended: no; explicit indexes keep a mod's own maps valid when
   other mods are added, as for `[[scripts]]`.
5. **Index ranges.** Should the owner publish suggested index ranges per mod
   to make collisions rarer? Recommended: no registry; conflicts are already
   a launch error naming both mods.
6. **Prototype messages.** Should `[[protos]]` also register name and
   description messages? Recommended: no; message files stay whole-file
   layer data in this revision.
7. **Word number.** Which `0x830A..=0x83FF` number for `mod_art_fid`?
   Recommended: the owner assigns it together with any other pending mod
   words, in one SPEC-024a revision.
8. **`asset-audit` exit code.** Should an unreachable row fail
   `--validate-art-ids`? Recommended: report only; exit status unchanged.
9. **`art_change_fid_num`.** Should metarule3 107 take bits 12-23 too when
   its argument is an extended FID under section 3.2 item 1? Recommended:
   yes, under exactly that condition, so every existing call keeps its
   `[Must-match]` result.

## 9. Out of scope

- Larger PIDs, script indexes or map numbers.
- Merging two layers' copies of one list file line by line.
- New art categories or directories.
- Changing the FRM format or directional-file rules.

## 10. Owner decisions and what was built (2026-11-27)

The nine questions of section 8 take their recommended answers, and
`mod_art_fid` is word `0x830A` (question 7); the refused range is now
`0x830B..=0x83FF`. SPEC-007 section 2.1.1 and 2.1.2, SPEC-024a section 2.4
and SPEC-017 section 5.2 hold the encoding and the rows as built. Notes:

- Section 2.2's example `file = "tc_rock01.frm"` is 13 bytes and is refused
  by section 4 item 1's own 12-byte rule; use a name of at most 12 bytes.
- `mod_art_fid` is in the word set only with manifest scripting on, like
  the other mod words, so a layer needs a `[[hooks]]` or
  `[[global_scripts]]` row to call it. It gives -1 for any row past the
  resolved list, below 4096 included.
- Conflicts found when the registry is built are reported as
  `mod registration:`; the per-layer file checks are reported as
  `mod_list:`, as script rows are.
- `metarule3` rule 107 looks the row up in the category of the object's
  current art.
- A critter row that does not fit is refused with a diagnostic each time
  it happens, not once per session.
- `asset-audit --validate-art-ids` reads the corpus folder only; it counts
  an extended identifier past its list as missing, not invalid.
- The `lst-writer.md` brief that section 3.6 item 4 cites is not in the
  repository, so nothing was changed for it.
- A save of a layer's own map does not load without the layer when its
  objects use a prototype only the layer registers; an extended identifier
  on a base prototype's object loads and is not drawn.
